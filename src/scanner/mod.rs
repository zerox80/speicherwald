use std::{
    fs,
    path::{Path, PathBuf},
};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

use globset::{Glob, GlobSet, GlobSetBuilder};
use sqlx::QueryBuilder;
use std::sync::Arc;
use tokio::sync::{mpsc, Semaphore};
use tokio::task;
use tokio::time::{interval, Duration};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::types::{ScanEvent, ScanOptions};

#[derive(Debug, Default, Clone)]
pub struct ScanResultSummary {
    pub total_dirs: u64,
    pub total_files: u64,
    pub total_logical_size: u64,
    pub total_allocated_size: u64,
    pub warnings: u64,
}

#[derive(Debug, Clone)]
struct NodeRecord {
    path: String,
    parent_path: Option<String>,
    depth: u32,
    is_dir: bool,
    logical_size: u64,
    allocated_size: u64,
    file_count: u64,
    dir_count: u64,
}

#[derive(Debug, Clone)]
struct FileRecord {
    path: String,
    parent_path: Option<String>,
    logical_size: u64,
    allocated_size: u64,
}

#[allow(clippy::too_many_arguments)]
pub async fn run_scan(
    pool: sqlx::SqlitePool,
    id: Uuid,
    root_paths: Vec<String>,
    options: ScanOptions,
    tx: tokio::sync::broadcast::Sender<ScanEvent>,
    cancel: CancellationToken,
    batch_size: usize,
    flush_threshold: usize,
    flush_interval_ms: u64,
    handle_limit: Option<usize>,
    dir_concurrency: Option<usize>,
) -> anyhow::Result<ScanResultSummary> {
    let mut summary = ScanResultSummary::default();
    let mut nodes: Vec<NodeRecord> = Vec::with_capacity(flush_threshold.max(batch_size) * 2);
    let mut files: Vec<FileRecord> = Vec::with_capacity(flush_threshold.max(batch_size) * 2);

    // Optimierte Parallelität basierend auf CPU-Kernen und System-Ressourcen
    let cpu_cores = num_cpus::get();
    let optimal_workers = (cpu_cores * 3 / 4).max(2); // 75% der CPU-Kerne nutzen
    let mut concurrency = options.concurrency.unwrap_or(optimal_workers);
    if let Some(h) = handle_limit {
        concurrency = concurrency.min(h.max(1));
    }
    let sem = Arc::new(Semaphore::new(concurrency.max(1)));
    // Größerer Channel-Buffer für besseren Durchsatz
    let channel_size = (concurrency * 8 + 128).min(1024);
    let (tx_res, mut rx_res) =
        mpsc::channel::<(Vec<NodeRecord>, Vec<FileRecord>, ScanResultSummary)>(channel_size);

    for root in root_paths {
        if cancel.is_cancelled() {
            break;
        }
        let root_path = PathBuf::from(&root);
        if !root_path.exists() {
            summary.warnings += 1;
            let _ = tx.send(ScanEvent::Warning {
                path: root.clone(),
                code: "missing_root".into(),
                message: "root path does not exist".into(),
            });
            continue;
        }

        let permit = sem.clone().acquire_owned().await.unwrap();
        let tx_res_cl = tx_res.clone();
        let tx_clone = tx.clone();
        let cancel_child = cancel.clone();
        let options_cl = options.clone();
        let root_clone = root_path.clone();
        let flush_thr = flush_threshold;
        let dir_conc = dir_concurrency.unwrap_or(1);
        let root_str = root_clone.to_string_lossy().to_string();
        task::spawn_blocking(move || {
            let gs_opt = build_globset(&options_cl.excludes).ok();
            if gs_opt.is_none() {
                drop(permit);
                return;
            }
            let gs = gs_opt.unwrap();

            // Skip excluded/hidden/reparse roots
            if matches_excludes(&root_clone, &gs) {
                drop(permit);
                return;
            }
            let meta = match fs::metadata(&root_clone) {
                Ok(m) => m,
                Err(_) => {
                    let _ = tx_clone.send(ScanEvent::Warning {
                        path: root_clone.to_string_lossy().to_string(),
                        code: "metadata_failed".into(),
                        message: "failed to stat root".into(),
                    });
                    drop(permit);
                    return;
                }
            };
            if !options_cl.follow_symlinks && is_reparse_point(&meta) {
                drop(permit);
                return;
            }
            if !options_cl.include_hidden && is_hidden_or_system(&meta) {
                drop(permit);
                return;
            }

            // Enumerate root entries
            let mut subdirs: Vec<PathBuf> = Vec::new();
            let mut root_files: u64 = 0;
            let mut root_files_logical: u64 = 0;
            let mut root_files_alloc: u64 = 0;
            let mut root_file_buf: Vec<FileRecord> = Vec::with_capacity(flush_thr);
            match fs::read_dir(&root_clone) {
                Ok(rd) => {
                    for entry in rd.flatten() {
                        if cancel_child.is_cancelled() {
                            break;
                        }
                        let p = entry.path();
                        if matches_excludes(&p, &gs) {
                            continue;
                        }
                        let md = match entry.metadata() {
                            Ok(m) => m,
                            Err(_) => {
                                let _ = tx_clone.send(ScanEvent::Warning {
                                    path: p.to_string_lossy().to_string(),
                                    code: "metadata_failed".into(),
                                    message: "failed to stat".into(),
                                });
                                continue;
                            }
                        };
                        if md.is_dir() {
                            if !options_cl.follow_symlinks && is_reparse_point(&md) {
                                continue;
                            }
                            if !options_cl.include_hidden && is_hidden_or_system(&md) {
                                continue;
                            }
                            if let Some(max_d) = options_cl.max_depth {
                                if max_d == 0 {
                                    continue;
                                }
                            }
                            subdirs.push(p);
                        } else if md.is_file() {
                            if !options_cl.include_hidden && is_hidden_or_system(&md) {
                                continue;
                            }
                            root_files += 1;
                            let logical_sz = md.len();
                            if options_cl.measure_logical {
                                root_files_logical = root_files_logical.saturating_add(logical_sz);
                            }
                            let alloc_sz = if options_cl.measure_allocated {
                                unsafe_get_allocated_size(&p).unwrap_or(logical_sz)
                            } else {
                                logical_sz
                            };
                            if options_cl.measure_allocated {
                                root_files_alloc = root_files_alloc.saturating_add(alloc_sz);
                            }
                            // buffer file record at root level, flush in batches
                            root_file_buf.push(FileRecord {
                                path: p.to_string_lossy().to_string(),
                                parent_path: Some(root_str.clone()),
                                logical_size: logical_sz,
                                allocated_size: alloc_sz,
                            });
                            if root_file_buf.len() >= flush_thr.max(1) {
                                let mut out_files: Vec<FileRecord> = Vec::new();
                                std::mem::swap(&mut out_files, &mut root_file_buf);
                                let _ = tx_res_cl.blocking_send((
                                    Vec::new(),
                                    out_files,
                                    ScanResultSummary::default(),
                                ));
                            }
                        }
                    }
                    // final flush of root file buffer
                    if !root_file_buf.is_empty() {
                        let mut out_files: Vec<FileRecord> = Vec::new();
                        std::mem::swap(&mut out_files, &mut root_file_buf);
                        let _ =
                            tx_res_cl.blocking_send((Vec::new(), out_files, ScanResultSummary::default()));
                    }
                }
                Err(_) => {
                    let _ = tx_clone.send(ScanEvent::Warning {
                        path: root_clone.to_string_lossy().to_string(),
                        code: "read_dir_failed".into(),
                        message: "failed to read directory".into(),
                    });
                }
            }

            // Spawn limited number of workers for subdirs
            let mut idx = 0usize;
            let mut running: Vec<std::thread::JoinHandle<ScanResultSummary>> = Vec::new();
            let sub_count = subdirs.len();
            let dir_limit = dir_conc.max(1);
            let mut sub_dirs_total: u64 = 0;
            let mut sub_files_total: u64 = 0;
            let mut subtree_logical: u64 = 0;
            let mut subtree_alloc: u64 = 0;
            while idx < sub_count || !running.is_empty() {
                while running.len() < dir_limit && idx < sub_count {
                    let sub = subdirs[idx].clone();
                    idx += 1;
                    let tx_res_sub = tx_res_cl.clone();
                    let tx_sse = tx_clone.clone();
                    let cancel_th = cancel_child.clone();
                    let opt = options_cl.clone();
                    let gs2 = gs.clone();
                    let handle = std::thread::spawn(move || {
                        let mut ssum = ScanResultSummary::default();
                        let mut snodes: Vec<NodeRecord> = Vec::with_capacity(flush_thr);
                        let mut sfiles: Vec<FileRecord> = Vec::with_capacity(flush_thr);
                        let _ = scan_dir(
                            id,
                            &sub,
                            1,
                            &opt,
                            &gs2,
                            &tx_sse,
                            &cancel_th,
                            &mut ssum,
                            &mut snodes,
                            &mut sfiles,
                            &tx_res_sub,
                            flush_thr,
                        );
                        // send remaining
                        let _ = tx_res_sub.blocking_send((snodes, sfiles, ssum.clone()));
                        ssum
                    });
                    running.push(handle);
                }
                if !running.is_empty() {
                    if let Some(handle) = running.pop() {
                        if let Ok(ssum) = handle.join() {
                            // accumulate into root aggregates
                            subtree_logical = subtree_logical.saturating_add(ssum.total_logical_size);
                            subtree_alloc = subtree_alloc.saturating_add(ssum.total_allocated_size);
                            // directories/files from subtrees
                            sub_dirs_total = sub_dirs_total.saturating_add(ssum.total_dirs);
                            sub_files_total = sub_files_total.saturating_add(ssum.total_files);
                        }
                    }
                }
            }

            // Emit root node record
            let root_node = NodeRecord {
                path: root_str,
                parent_path: parent_path_string(&root_clone),
                depth: calc_depth(&root_clone),
                is_dir: true,
                logical_size: root_files_logical.saturating_add(subtree_logical),
                allocated_size: root_files_alloc.saturating_add(subtree_alloc),
                file_count: root_files.saturating_add(sub_files_total),
                dir_count: sub_dirs_total,
            };
            let _ = tx_res_cl.blocking_send((
                vec![root_node],
                Vec::new(),
                ScanResultSummary {
                    total_dirs: 1,
                    total_files: root_files,
                    total_logical_size: root_files_logical,
                    total_allocated_size: root_files_alloc,
                    warnings: 0,
                },
            ));
            drop(permit);
        });
    }

    drop(tx_res);

    let mut ticker = interval(Duration::from_millis(flush_interval_ms.max(1)));
    loop {
        tokio::select! {
            maybe = rx_res.recv() => {
                match maybe {
                    Some((mut ns, mut fs, sum)) => {
                        // aggregate summary
                        summary.total_dirs = summary.total_dirs.saturating_add(sum.total_dirs);
                        summary.total_files = summary.total_files.saturating_add(sum.total_files);
                        summary.total_logical_size = summary.total_logical_size.saturating_add(sum.total_logical_size);
                        summary.total_allocated_size = summary.total_allocated_size.saturating_add(sum.total_allocated_size);
                        summary.warnings = summary.warnings.saturating_add(sum.warnings);

                        // accumulate and persist in batches
                        nodes.append(&mut ns);
                        files.append(&mut fs);
                        if nodes.len() + files.len() >= flush_threshold.max(batch_size) {
                            let _ = persist_batches(&pool, id, &mut nodes, &mut files, batch_size).await;
                        }
                    }
                    None => break,
                }
            }
            _ = ticker.tick() => {
                if !nodes.is_empty() || !files.is_empty() {
                    let _ = persist_batches(&pool, id, &mut nodes, &mut files, batch_size).await;
                }
                // Fortschritt periodisch in scans Tabelle schreiben, damit UI während running Zahlen sieht
                let _ = sqlx::query(
                    r#"UPDATE scans SET
                        total_logical_size=?1,
                        total_allocated_size=?2,
                        dir_count=?3,
                        file_count=?4,
                        warning_count=?5
                      WHERE id=?6"#
                )
                .bind(summary.total_logical_size as i64)
                .bind(summary.total_allocated_size as i64)
                .bind(summary.total_dirs as i64)
                .bind(summary.total_files as i64)
                .bind(summary.warnings as i64)
                .bind(id.to_string())
                .execute(&pool).await;
            }
        }
    }

    // Persist any remaining records
    persist_batches(&pool, id, &mut nodes, &mut files, batch_size).await?;

    Ok(summary)
}

#[allow(clippy::too_many_arguments)]
fn scan_dir(
    _scan_id: Uuid,
    dir: &Path,
    depth: u32,
    options: &ScanOptions,
    globset: &GlobSet,
    tx: &tokio::sync::broadcast::Sender<ScanEvent>,
    cancel: &CancellationToken,
    summary: &mut ScanResultSummary,
    nodes: &mut Vec<NodeRecord>,
    files: &mut Vec<FileRecord>,
    tx_out: &mpsc::Sender<(Vec<NodeRecord>, Vec<FileRecord>, ScanResultSummary)>,
    flush_threshold: usize,
) -> anyhow::Result<(u64, u64, u64, u64)> {
    // (dirs, files, logical, allocated)
    if cancel.is_cancelled() {
        anyhow::bail!("cancelled")
    }

    if matches_excludes(dir, globset) {
        return Ok((0, 0, 0, 0));
    }

    let meta = match fs::metadata(dir) {
        Ok(m) => m,
        Err(e) => anyhow::bail!(e),
    };

    if !options.follow_symlinks && is_reparse_point(&meta) {
        return Ok((0, 0, 0, 0));
    }
    if !options.include_hidden && is_hidden_or_system(&meta) {
        return Ok((0, 0, 0, 0));
    }

    let mut local_dirs: u64 = 1; // count this dir
    let mut local_files: u64 = 0;
    let mut logical: u64 = 0;
    let mut allocated: u64 = 0;

    let mut sent = 0u32;
    let dir_str = dir.to_string_lossy().to_string();

    match fs::read_dir(dir) {
        Ok(rd) => {
            for entry in rd.flatten() {
                if cancel.is_cancelled() {
                    anyhow::bail!("cancelled");
                }
                let path = entry.path();
                if matches_excludes(&path, globset) {
                    continue;
                }
                let md = match entry.metadata() {
                    Ok(m) => m,
                    Err(_) => {
                        summary.warnings += 1;
                        continue;
                    }
                };

                if md.is_dir() {
                    if !options.follow_symlinks && is_reparse_point(&md) {
                        continue;
                    }
                    if !options.include_hidden && is_hidden_or_system(&md) {
                        continue;
                    }
                    if let Some(max_d) = options.max_depth {
                        if depth >= max_d {
                            continue;
                        }
                    }
                    let (d_dirs, d_files, d_logical, d_alloc) = scan_dir(
                        _scan_id,
                        &path,
                        depth + 1,
                        options,
                        globset,
                        tx,
                        cancel,
                        summary,
                        nodes,
                        files,
                        tx_out,
                        flush_threshold,
                    )?;
                    local_dirs += d_dirs;
                    local_files += d_files;
                    logical = logical.saturating_add(d_logical);
                    allocated = allocated.saturating_add(d_alloc);
                } else if md.is_file() {
                    if !options.include_hidden && is_hidden_or_system(&md) {
                        continue;
                    }
                    local_files += 1;
                    let logical_sz = md.len();
                    if options.measure_logical {
                        logical = logical.saturating_add(logical_sz);
                    }
                    let alloc_sz = if options.measure_allocated {
                        unsafe_get_allocated_size(&path).unwrap_or(logical_sz)
                    } else {
                        logical_sz
                    };
                    if options.measure_allocated {
                        allocated = allocated.saturating_add(alloc_sz);
                    }

                    // collect file record
                    files.push(FileRecord {
                        path: path.to_string_lossy().to_string(),
                        parent_path: Some(dir_str.clone()),
                        logical_size: logical_sz,
                        allocated_size: alloc_sz,
                    });
                }

                sent = sent.wrapping_add(1);
                // Reduzierte Progress-Updates für bessere Performance
                if sent % 512 == 0 {
                    let _ = tx.send(ScanEvent::Progress {
                        current_path: path.to_string_lossy().to_string(),
                        dirs_scanned: summary.total_dirs + local_dirs,
                        files_scanned: summary.total_files + local_files,
                        logical_size: summary.total_logical_size + logical,
                        allocated_size: summary.total_allocated_size + allocated,
                    });
                }

                // Partial flush to aggregator when buffers grow large
                if (nodes.len() + files.len()) >= flush_threshold.max(1) {
                    let mut out_nodes: Vec<NodeRecord> = Vec::new();
                    let mut out_files: Vec<FileRecord> = Vec::new();
                    std::mem::swap(&mut out_nodes, nodes);
                    std::mem::swap(&mut out_files, files);
                    let _ = tx_out.blocking_send((out_nodes, out_files, ScanResultSummary::default()));
                }
            }
        }
        Err(_) => {
            summary.warnings += 1;
            let _ = tx.send(ScanEvent::Warning {
                path: dir_str.clone(),
                code: "read_dir_failed".into(),
                message: "failed to read directory".into(),
            });
        }
    }

    summary.total_dirs = summary.total_dirs.saturating_add(local_dirs);
    summary.total_files = summary.total_files.saturating_add(local_files);
    summary.total_logical_size = summary.total_logical_size.saturating_add(logical);
    summary.total_allocated_size = summary.total_allocated_size.saturating_add(allocated);

    // collect node record for this directory
    nodes.push(NodeRecord {
        path: dir_str,
        parent_path: parent_path_string(dir),
        depth: calc_depth(dir),
        is_dir: true,
        logical_size: logical,
        allocated_size: allocated,
        file_count: local_files,
        dir_count: local_dirs.saturating_sub(1), // exclude self
    });

    Ok((local_dirs, local_files, logical, allocated))
}

fn build_globset(patterns: &[String]) -> anyhow::Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        if p.trim().is_empty() {
            continue;
        }
        // Normalisiere Backslashes zu Slashes, damit Muster plattformunabhängig mit
        // der Pfadnormalisierung in `matches_excludes` (\\ -> /) übereinstimmen.
        let norm = p.trim().replace('\\', "/");
        let g = Glob::new(&norm)?;
        b.add(g);
    }
    Ok(b.build()?)
}

fn matches_excludes(path: &Path, set: &GlobSet) -> bool {
    if set.is_empty() {
        return false;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    set.is_match(&s)
}

#[cfg(windows)]
fn is_hidden_or_system(md: &fs::Metadata) -> bool {
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    const FILE_ATTRIBUTE_SYSTEM: u32 = 0x4;
    let attrs = md.file_attributes();
    (attrs & FILE_ATTRIBUTE_HIDDEN) != 0 || (attrs & FILE_ATTRIBUTE_SYSTEM) != 0
}

#[cfg(not(windows))]
fn is_hidden_or_system(_md: &fs::Metadata) -> bool {
    // Auf Nicht-Windows: keine Hidden/System-Attribute – immer false
    false
}

#[cfg(windows)]
fn is_reparse_point(md: &fs::Metadata) -> bool {
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    (md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_md: &fs::Metadata) -> bool {
    false
}

// Cache für häufig abgefragte Pfade
use lru::LruCache;
use std::sync::Mutex;

lazy_static::lazy_static! {
    static ref SIZE_CACHE: Mutex<LruCache<PathBuf, u64>> = Mutex::new(LruCache::new(std::num::NonZeroUsize::new(10000).unwrap()));
}

#[cfg(windows)]
fn unsafe_get_allocated_size(path: &Path) -> Option<u64> {
    // Zuerst im Cache nachschauen
    if let Ok(mut cache) = SIZE_CACHE.lock() {
        if let Some(&size) = cache.get(&path.to_path_buf()) {
            return Some(size);
        }
    }

    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::GetLastError;
    use windows::Win32::Storage::FileSystem::{GetCompressedFileSizeW, INVALID_FILE_SIZE};

    let w: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut high: u32 = 0;
    unsafe {
        let low = GetCompressedFileSizeW(PCWSTR(w.as_ptr()), Some(&mut high as *mut u32));
        // When the low part is INVALID_FILE_SIZE (0xFFFFFFFF), check GetLastError
        if low == INVALID_FILE_SIZE && GetLastError().0 != 0 {
            return None;
        }
        let size = ((high as u64) << 32) | (low as u64);

        // In Cache speichern
        if let Ok(mut cache) = SIZE_CACHE.lock() {
            cache.put(path.to_path_buf(), size);
        }

        Some(size)
    }
}

#[cfg(not(windows))]
fn unsafe_get_allocated_size(_path: &Path) -> Option<u64> {
    // Auf Nicht-Windows approximieren wir die Allokationsgröße mit der logischen Größe (None -> Fallback im Aufrufer)
    None
}

fn parent_path_string(path: &Path) -> Option<String> {
    path.parent().map(|p| p.to_string_lossy().to_string())
}

fn calc_depth(path: &Path) -> u32 {
    // Rough component count as depth, suitable for sorting/filtering
    path.components().count() as u32
}

async fn persist_batches(
    pool: &sqlx::SqlitePool,
    id: Uuid,
    nodes: &mut Vec<NodeRecord>,
    files: &mut Vec<FileRecord>,
    batch_size: usize,
) -> anyhow::Result<()> {
    if nodes.is_empty() && files.is_empty() {
        return Ok(());
    }
    let sid = id.to_string();
    let mut txdb = pool.begin().await?;

    // Respect SQLite variable limit (commonly 999). Each row consumes a fixed
    // number of bound parameters; cap chunk sizes accordingly so a single
    // INSERT statement never exceeds this limit.
    const SQLITE_MAX_VARS: usize = 999;
    const NODE_BINDS_PER_ROW: usize = 9; // scan_id, path, parent_path, depth, is_dir, logical_size, allocated_size, file_count, dir_count
    const FILE_BINDS_PER_ROW: usize = 5; // scan_id, path, parent_path, logical_size, allocated_size

    let max_node_rows_per_stmt = SQLITE_MAX_VARS / NODE_BINDS_PER_ROW;
    let max_file_rows_per_stmt = SQLITE_MAX_VARS / FILE_BINDS_PER_ROW;

    // nodes in chunks
    let node_chunk = batch_size.max(1).min(max_node_rows_per_stmt.max(1));
    for chunk in nodes.chunks(node_chunk) {
        let mut qb = QueryBuilder::new(
            "INSERT INTO nodes (scan_id, path, parent_path, depth, is_dir, logical_size, allocated_size, file_count, dir_count) "
        );
        qb.push_values(chunk, |mut b, n| {
            b.push_bind(&sid)
                .push_bind(&n.path)
                .push_bind(n.parent_path.as_deref())
                .push_bind(n.depth as i64)
                .push_bind(if n.is_dir { 1i64 } else { 0i64 })
                .push_bind(n.logical_size as i64)
                .push_bind(n.allocated_size as i64)
                .push_bind(n.file_count as i64)
                .push_bind(n.dir_count as i64);
        });
        qb.build().execute(&mut *txdb).await?;
    }

    // files in chunks
    let file_chunk = batch_size.max(1).min(max_file_rows_per_stmt.max(1));
    for chunk in files.chunks(file_chunk) {
        let mut qb = QueryBuilder::new(
            "INSERT INTO files (scan_id, path, parent_path, logical_size, allocated_size) ",
        );
        qb.push_values(chunk, |mut b, f| {
            b.push_bind(&sid)
                .push_bind(&f.path)
                .push_bind(f.parent_path.as_deref())
                .push_bind(f.logical_size as i64)
                .push_bind(f.allocated_size as i64);
        });
        qb.build().execute(&mut *txdb).await?;
    }

    txdb.commit().await?;
    nodes.clear();
    files.clear();
    Ok(())
}
