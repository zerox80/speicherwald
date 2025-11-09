use std::time::{Instant, SystemTime};
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

/// A summary of the results of a scan.
#[derive(Debug, Default, Clone)]
pub struct ScanResultSummary {
    /// The total number of directories scanned.
    pub total_dirs: u64,
    /// The total number of files scanned.
    pub total_files: u64,
    /// The total logical size of all files scanned.
    pub total_logical_size: u64,
    /// The total allocated size of all files scanned.
    pub total_allocated_size: u64,
    /// The number of warnings generated during the scan.
    pub warnings: u64,
    /// The most recent modification time of any file or directory scanned.
    pub latest_mtime: Option<i64>,
    /// The most recent access time of any file or directory scanned.
    pub latest_atime: Option<i64>,
}

/// A record of a scanned node (file or directory).
#[derive(Debug, Clone)]
pub struct NodeRecord {
    /// The path of the node.
    pub path: String,
    /// The parent path of the node.
    pub parent_path: Option<String>,
    /// The depth of the node in the directory tree.
    pub depth: u32,
    /// Whether the node is a directory.
    pub is_dir: bool,
    /// The logical size of the node in bytes.
    pub logical_size: u64,
    /// The allocated size of the node in bytes.
    pub allocated_size: u64,
    /// The number of files in the node.
    pub file_count: u64,
    /// The number of subdirectories in the node.
    pub dir_count: u64,
    /// The modification time of the node.
    pub mtime: Option<i64>,
    /// The access time of the node.
    pub atime: Option<i64>,
}

#[derive(Debug, Clone)]
struct FileRecord {
    path: String,
    parent_path: Option<String>,
    logical_size: u64,
    allocated_size: u64,
    mtime: Option<i64>,
    atime: Option<i64>,
}

fn system_time_to_secs(st: Option<SystemTime>) -> Option<i64> {
    st.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64)
}

fn max_opt(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (Some(x), None) => Some(x),
        (None, Some(y)) => Some(y),
        (None, None) => None,
    }
}

/// Runs a directory scan and persists the results to the database.
///
/// This is the main entry point for the scanning process. It spawgns a pool of
/// worker threads to traverse the directory tree and collect file and directory
/// information. The results are then collected and inserted into the database in
/// batches.
///
/// # Arguments
///
/// * `pool` - The database connection pool.
/// * `id` - The ID of the scan.
/// * `root_paths` - The root paths to scan.
/// * `options` - The scan options.
/// * `tx` - A broadcast sender for sending scan events.
/// * `cancel` - A cancellation token for stopping the scan.
/// * `batch_size` - The number of records to insert in a single database transaction.
/// * `flush_threshold` - The number of pending records that triggers a flush to the database.
/// * `flush_interval_ms` - The interval in milliseconds at which to flush pending records.
/// * `handle_limit` - The maximum number of open file handles.
/// * `dir_concurrency` - The number of concurrent directory traversers.
///
/// # Returns
///
/// * `anyhow::Result<ScanResultSummary>` - The summary of the scan results.
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
    // Limit capacity to prevent excessive memory allocation
    let safe_capacity = flush_threshold.max(batch_size).saturating_mul(2).min(50_000);
    let mut nodes: Vec<NodeRecord> = Vec::with_capacity(safe_capacity);
    let mut files: Vec<FileRecord> = Vec::with_capacity(safe_capacity);

    // FIX Bug #70 - Better CPU core calculation
    let cpu_cores = num_cpus::get().max(1); // Ensure at least 1 core
                                            // Use 75% of CPU cores for larger systems, 50% for smaller ones
    let optimal_workers = if cpu_cores >= 4 {
        ((cpu_cores * 3) / 4).max(2)
    } else if cpu_cores >= 2 {
        cpu_cores / 2
    } else {
        1
    };
    // FIX Bug #27 - Better channel size calculation with overflow protection
    let mut concurrency = options.concurrency.unwrap_or(optimal_workers).max(1);
    if let Some(h) = handle_limit {
        concurrency = concurrency.min(h.max(1));
    }
    let sem = Arc::new(Semaphore::new(concurrency));
    // Channel buffer size: ensure it's large enough but bounded
    // FIX Bug #7: Log warning when overflow occurs
    let channel_size = match concurrency.checked_mul(8).and_then(|v| v.checked_add(128)) {
        Some(size) => size.clamp(256, 2048),
        None => {
            tracing::warn!("Channel size calculation overflow for concurrency={}, using default 2048", concurrency);
            2048
        }
    };
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

        let permit = match sem.clone().acquire_owned().await {
            Ok(p) => p,
            Err(e) => {
                // Semaphore closed, likely shutdown
                tracing::error!("Semaphore acquisition failed: {}", e);
                summary.warnings += 1;
                continue;
            }
        };
        let tx_res_cl = tx_res.clone();
        let tx_clone = tx.clone();
        let cancel_child = cancel.clone();
        let options_cl = options.clone();
        let root_clone = root_path.clone();
        let flush_thr = flush_threshold;
        let dir_conc = dir_concurrency.or(options_cl.concurrency).unwrap_or(1);
        let root_str = root_clone.to_string_lossy().to_string();
        task::spawn_blocking(move || {
            let gs = match build_globset(&options_cl.excludes) {
                Ok(gs) => gs,
                Err(e) => {
                    let _ = tx_clone.send(ScanEvent::Warning {
                        path: root_str.clone(),
                        code: "invalid_exclude_pattern".into(),
                        message: format!("Failed to build exclude pattern: {}", e),
                    });
                    drop(permit);
                    return;
                }
            };

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
                    let warn_summary = ScanResultSummary { warnings: 1, ..Default::default() };
                    let _ = tx_res_cl.blocking_send((Vec::new(), Vec::new(), warn_summary));
                    drop(permit);
                    return;
                }
            };
            let root_mtime = system_time_to_secs(meta.modified().ok());
            let root_atime = system_time_to_secs(meta.accessed().ok());
            let mut root_latest_mtime = root_mtime;
            let mut root_latest_atime = root_atime;
            if !options_cl.follow_symlinks && is_reparse_point(&meta) {
                // UNC/DFS shares and mapped network drives should be traversed even if marked as reparse points
                if !is_network_path(&root_clone) {
                    drop(permit);
                    return;
                }
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
                                let warn_summary = ScanResultSummary { warnings: 1, ..Default::default() };
                                let _ = tx_res_cl.blocking_send((Vec::new(), Vec::new(), warn_summary));
                                continue;
                            }
                        };
                        let entry_mtime = system_time_to_secs(md.modified().ok());
                        let entry_atime = system_time_to_secs(md.accessed().ok());
                        root_latest_mtime = max_opt(root_latest_mtime, entry_mtime);
                        root_latest_atime = max_opt(root_latest_atime, entry_atime);
                        if md.is_dir() {
                            if !options_cl.follow_symlinks && is_reparse_point(&md) {
                                // Allow DFS/UNC and mapped network dirs even if marked as reparse points
                                if !is_network_path(&p) {
                                    continue;
                                }
                            }
                            if !options_cl.include_hidden && is_hidden_or_system(&md) {
                                continue;
                            }
                            // FIX Bug #66 - max_depth = 0 means scan only root, depth >= 1 means too deep
                            if let Some(max_d) = options_cl.max_depth {
                                if max_d == 0 {
                                    // Don't recurse into subdirectories
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
                            // FIX Bug #40: Return error on overflow instead of setting to u64::MAX
                            let (new_logical, overflow1) = root_files_logical.overflowing_add(logical_sz);
                            if overflow1 && options_cl.measure_logical {
                                tracing::error!("Logical size overflow at path: {:?}", p);
                                let _ = tx_clone.send(ScanEvent::Failed {
                                    message: format!("Size overflow at: {:?}", p),
                                });
                                let warn_summary = ScanResultSummary { warnings: 1, ..Default::default() };
                                let _ = tx_res_cl.blocking_send((Vec::new(), Vec::new(), warn_summary));
                                continue; // Skip this file but continue scanning
                            } else if options_cl.measure_logical {
                                root_files_logical = new_logical;
                            }
                            let alloc_sz = if options_cl.measure_allocated {
                                unsafe_get_allocated_size(&p).unwrap_or(logical_sz)
                            } else {
                                logical_sz
                            };
                            let (new_alloc, overflow2) = root_files_alloc.overflowing_add(alloc_sz);
                            if overflow2 {
                                tracing::error!("Allocated size overflow at path: {:?}", p);
                                let _ = tx_clone.send(ScanEvent::Failed {
                                    message: format!("Size overflow at: {:?}", p),
                                });
                                let warn_summary = ScanResultSummary { warnings: 1, ..Default::default() };
                                let _ = tx_res_cl.blocking_send((Vec::new(), Vec::new(), warn_summary));
                                continue; // Skip this file but continue scanning
                            } else {
                                root_files_alloc = new_alloc;
                            }
                            // buffer file record at root level, flush in batches (ensure flush_thr >= 1)
                            let flush_limit = flush_thr.max(1);
                            root_file_buf.push(FileRecord {
                                path: p.to_string_lossy().to_string(),
                                parent_path: Some(root_str.clone()),
                                logical_size: logical_sz,
                                allocated_size: alloc_sz,
                                mtime: entry_mtime,
                                atime: entry_atime,
                            });
                            if root_file_buf.len() >= flush_limit {
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
                    let warn_summary = ScanResultSummary { warnings: 1, ..Default::default() };
                    let _ = tx_res_cl.blocking_send((Vec::new(), Vec::new(), warn_summary));
                }
            }

            // FIX Bug #39 - Limit total threads spawned
            let mut idx = 0usize;
            let mut running: Vec<std::thread::JoinHandle<ScanResultSummary>> = Vec::new();
            let sub_count = subdirs.len();
            // Cap dir_limit to prevent resource exhaustion
            let dir_limit = dir_conc.max(1).min(64);
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
                        // FIX Bug #11: Ensure proper cleanup even on panic
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let mut ssum = ScanResultSummary::default();
                            let mut last_sent_summary = ScanResultSummary::default();
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
                            let delta = diff_summary(&ssum, &last_sent_summary);
                            let _ = tx_res_sub.blocking_send((snodes, sfiles, delta));
                            ssum
                        }));
                        result.unwrap_or_else(|_| {
                            tracing::error!("Thread panicked during scan");
                            ScanResultSummary::default()
                        })
                    });
                    running.push(handle);
                }
                if !running.is_empty() {
                    if let Some(handle) = running.pop() {
                        // FIX Bug #41 - Handle thread panics
                        match handle.join() {
                            Ok(ssum) => {
                                // accumulate into root aggregates
                                subtree_logical = subtree_logical.saturating_add(ssum.total_logical_size);
                                subtree_alloc = subtree_alloc.saturating_add(ssum.total_allocated_size);
                                // directories/files from subtrees
                                sub_dirs_total = sub_dirs_total.saturating_add(ssum.total_dirs);
                                sub_files_total = sub_files_total.saturating_add(ssum.total_files);
                            }
                            Err(e) => {
                                tracing::error!("Worker thread panicked: {:?}", e);
                                // Continue processing other threads
                            }
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
                mtime: root_latest_mtime,
                atime: root_latest_atime,
            };
            let root_delta = ScanResultSummary {
                total_dirs: 1,
                total_files: root_files,
                total_logical_size: root_files_logical,
                total_allocated_size: root_files_alloc,
                warnings: 0,
                latest_mtime: root_latest_mtime,
                latest_atime: root_latest_atime,
            };
            let _ = tx_res_cl.blocking_send((vec![root_node], Vec::new(), root_delta));
            drop(permit);
        });
    }

    drop(tx_res);

    let mut ticker = interval(Duration::from_millis(flush_interval_ms.max(1)));
    // Remember last sent totals and time to avoid spamming, but still emit a heartbeat on slow shares
    // Use atomic types to prevent data races (though single-threaded in this context)
    let mut last_progress_totals: (u64, u64, u64, u64) = (0, 0, 0, 0);
    let mut last_sse_emit = Instant::now();
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
                        summary.latest_mtime = max_opt(summary.latest_mtime, sum.latest_mtime);
                        summary.latest_atime = max_opt(summary.latest_atime, sum.latest_atime);

                        // accumulate and persist in batches
                        nodes.append(&mut ns);
                        files.append(&mut fs);
                        if nodes.len() + files.len() >= flush_threshold.max(batch_size) {
                            if let Err(e) = persist_batches(&pool, id, &mut nodes, &mut files, batch_size).await {
                                tracing::error!("Failed to persist scan batch: {:?}", e);
                                return Err(e);
                            }
                        }
                    }
                    None => break,
                }
            }
            _ = ticker.tick() => {
                if !nodes.is_empty() || !files.is_empty() {
                    if let Err(e) = persist_batches(&pool, id, &mut nodes, &mut files, batch_size).await {
                        tracing::error!("Failed to persist scan batch: {:?}", e);
                        return Err(e);
                    }
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

                // Emit a throttled SSE progress update if totals changed since last tick
                let current_totals = (
                    summary.total_dirs,
                    summary.total_files,
                    summary.total_logical_size,
                    summary.total_allocated_size,
                );
                // Emit progress if changed or 5s heartbeat
                if current_totals != last_progress_totals || last_sse_emit.elapsed() >= std::time::Duration::from_secs(5) {
                    let _ = tx.send(ScanEvent::Progress {
                        current_path: String::new(),
                        dirs_scanned: summary.total_dirs,
                        files_scanned: summary.total_files,
                        logical_size: summary.total_logical_size,
                        allocated_size: summary.total_allocated_size,
                    });
                    last_progress_totals = current_totals;
                    last_sse_emit = Instant::now();
                }
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

    let dir_mtime = system_time_to_secs(meta.modified().ok());
    let dir_atime = system_time_to_secs(meta.accessed().ok());

    summary.latest_mtime = max_opt(summary.latest_mtime, dir_mtime);
    summary.latest_atime = max_opt(summary.latest_atime, dir_atime);

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

    // FIX Bug #12: Use u64 instead of u32 to prevent overflow on large directories
    let mut sent = 0u64;
    let mut last_emit = Instant::now();
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

                let entry_mtime = system_time_to_secs(md.modified().ok());
                let entry_atime = system_time_to_secs(md.accessed().ok());
                summary.latest_mtime = max_opt(summary.latest_mtime, entry_mtime);
                summary.latest_atime = max_opt(summary.latest_atime, entry_atime);

                if md.is_dir() {
                    if !options.follow_symlinks && is_reparse_point(&md) {
                        continue;
                    }
                    if !options.include_hidden && is_hidden_or_system(&md) {
                        continue;
                    }
                    // FIX Bug #9: Check max_depth: depth is 0-indexed from root
                    // If we're at depth N and max_depth is N, we can still recurse one level
                    // Only block when depth > max_depth (not >=)
                    if let Some(max_d) = options.max_depth {
                        if depth > max_d {
                            continue; // Don't recurse deeper
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
                    let alloc_sz = if options.measure_allocated {
                        unsafe_get_allocated_size(&path).unwrap_or(logical_sz)
                    } else {
                        logical_sz
                    };
                    // FIX Bug #22 - Check for overflow in scan_dir (consistent handling)
                    if options.measure_logical {
                        let (new_logical, overflow_logical) = logical.overflowing_add(logical_sz);
                        if overflow_logical {
                            tracing::warn!("Logical size overflow at path: {:?}", path);
                            logical = u64::MAX;
                        } else {
                            logical = new_logical;
                        }
                    }
                    let (new_alloc, overflow_alloc) = allocated.overflowing_add(alloc_sz);
                    if overflow_alloc {
                        tracing::warn!("Allocated size overflow at path: {:?}", path);
                        allocated = u64::MAX;
                    } else {
                        allocated = new_alloc;
                    }

                    // collect file record
                    files.push(FileRecord {
                        path: path.to_string_lossy().to_string(),
                        parent_path: Some(dir_str.clone()),
                        logical_size: logical_sz,
                        allocated_size: alloc_sz,
                        mtime: entry_mtime,
                        atime: entry_atime,
                    });
                }

                sent = sent.saturating_add(1);
                // Reduzierte Progress-Updates für bessere Performance
                // FIX Bug #13: Remove redundant sent > 0 check (modulo handles zero)
                if sent % 512 == 0 {
                    let _ = tx.send(ScanEvent::Progress {
                        current_path: path.to_string_lossy().to_string(),
                        dirs_scanned: summary.total_dirs + local_dirs,
                        files_scanned: summary.total_files + local_files,
                        logical_size: summary.total_logical_size + logical,
                        allocated_size: summary.total_allocated_size + allocated,
                    });
                }

                // Zusätzlich: Zeitbasierte Fortschrittsupdates (z. B. auf langsamen Netzlaufwerken)
                if last_emit.elapsed() >= std::time::Duration::from_millis(2000) {
                    let _ = tx.send(ScanEvent::Progress {
                        current_path: path.to_string_lossy().to_string(),
                        dirs_scanned: summary.total_dirs + local_dirs,
                        files_scanned: summary.total_files + local_files,
                        logical_size: summary.total_logical_size + logical,
                        allocated_size: summary.total_allocated_size + allocated,
                    });
                    last_emit = Instant::now();
                }

                // FIX Bug #45 - Partial flush with proper error handling
                let flush_limit = flush_threshold.max(1);
                if (nodes.len() + files.len()) >= flush_limit {
                    let mut out_nodes: Vec<NodeRecord> = Vec::new();
                    let mut out_files: Vec<FileRecord> = Vec::new();
                    std::mem::swap(&mut out_nodes, nodes);
                    std::mem::swap(&mut out_files, files);
                    if tx_out.blocking_send((out_nodes, out_files, ScanResultSummary::default())).is_err() {
                        tracing::warn!("Channel closed during partial flush");
                        anyhow::bail!("Aggregator channel closed");
                    }
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
    // FIX Bug #18 & #23: Return error if local_dirs is invalid instead of continuing
    // local_dirs includes this dir (initialized to 1) plus all recursive subdirs
    // Subtract 1 to exclude self from the count
    let dir_count_value = if local_dirs > 0 {
        local_dirs - 1
    } else {
        tracing::error!("local_dirs is 0 at {:?}, this indicates a logic error", dir);
        anyhow::bail!("Invalid directory count detected");
    };
    nodes.push(NodeRecord {
        path: dir_str,
        parent_path: parent_path_string(dir),
        depth: calc_depth(dir),
        is_dir: true,
        logical_size: logical,
        allocated_size: allocated,
        file_count: local_files,
        dir_count: dir_count_value,
        mtime: dir_mtime,
        atime: dir_atime,
    });

    Ok((local_dirs, local_files, logical, allocated))
}

fn build_globset(patterns: &[String]) -> anyhow::Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        let trimmed = p.trim();
        if trimmed.is_empty() {
            continue;
        }
        // FIX Bug #29: Validate pattern length and complexity to prevent DoS
        if trimmed.len() > 1024 {
            tracing::warn!("Glob pattern too long, skipping: {} chars", trimmed.len());
            continue;
        }
        // Check for excessive wildcards that could cause exponential backtracking
        let wildcard_count = trimmed.chars().filter(|&c| c == '*' || c == '?').count();
        if wildcard_count > 20 {
            tracing::warn!("Glob pattern has too many wildcards ({}), skipping: {}", wildcard_count, trimmed);
            continue;
        }
        // FIX Bug #24: Normalize path separators consistently
        // We use forward slashes internally for cross-platform compatibility
        // Windows APIs handle forward slashes correctly in most cases
        let norm = trimmed.replace('\\', "/");
        // Catch glob compilation errors
        let g = Glob::new(&norm).map_err(|e| anyhow::anyhow!("Invalid glob pattern '{}': {}", norm, e))?;
        b.add(g);
    }
    Ok(b.build()?)
}

fn matches_excludes(path: &Path, set: &GlobSet) -> bool {
    if set.is_empty() {
        return false;
    }
    // FIX Bug #25: Check for replacement characters from invalid UTF-8
    let s = path.to_string_lossy();
    if s.contains('\u{FFFD}') {
        tracing::warn!("Path contains invalid UTF-8, skipping: {:?}", path);
        return true; // Exclude paths with invalid UTF-8
    }
    let normalized = s.replace('\\', "/");
    set.is_match(&normalized)
}

#[cfg(windows)]
#[inline]
fn is_unc_path(path: &Path) -> bool {
    // Detect classic UNC (\\server\share\...), and extended UNC (\\?\UNC\server\share\...)
    let s = path.as_os_str().to_string_lossy();
    s.starts_with("\\\\?\\UNC\\") || s.starts_with("\\\\")
}

#[cfg(not(windows))]
#[inline]
fn is_unc_path(_path: &Path) -> bool {
    false
}

#[cfg(windows)]
#[inline]
fn is_network_path(path: &Path) -> bool {
    if is_unc_path(path) {
        return true;
    }
    // Detect mapped network drives (e.g., Z:\)
    let s = path.as_os_str().to_string_lossy();
    if s.len() >= 2 && s.chars().nth(1) == Some(':') {
        // FIX Bug #4: Validate drive letter properly
        let drive_char = s.chars().next().unwrap_or('C');
        if !drive_char.is_ascii_alphabetic() {
            tracing::warn!("Invalid drive letter in path: {}", s);
            return false;
        }
        let root = format!("{}:\\", drive_char);
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::GetDriveTypeW;
        let w: Vec<u16> = std::ffi::OsStr::new(&root).encode_wide().chain(std::iter::once(0)).collect();
        unsafe {
            let ty = GetDriveTypeW(PCWSTR(w.as_ptr()));
            // DRIVE_REMOTE (4) indicates network drive
            // DRIVE_UNKNOWN (0) or other values indicate error or local drive
            const DRIVE_UNKNOWN: u32 = 0;
            const DRIVE_REMOTE: u32 = 4;
            if ty == DRIVE_UNKNOWN {
                // Error occurred, log and assume not network
                tracing::debug!("GetDriveTypeW returned UNKNOWN for {}", root);
                return false;
            }
            return ty == DRIVE_REMOTE;
        }
    }
    false
}

#[cfg(not(windows))]
#[inline]
fn is_network_path(_path: &Path) -> bool {
    false
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

// Configurable cache size via environment variable, default 10000
fn get_cache_size() -> usize {
    std::env::var("SPEICHERWALD_SIZE_CACHE_ENTRIES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10000)
        .clamp(100, 100_000)
}

lazy_static::lazy_static! {
    static ref SIZE_CACHE: Mutex<LruCache<PathBuf, Option<u64>>> = {
        let size = get_cache_size();
        // Ensure size is non-zero
        let non_zero = std::num::NonZeroUsize::new(size)
            .unwrap_or_else(|| std::num::NonZeroUsize::new(1000).unwrap());
        Mutex::new(LruCache::new(non_zero))
    };
}

#[cfg(windows)]
fn unsafe_get_allocated_size(path: &Path) -> Option<u64> {
    // Zuerst im Cache nachschauen - handle lock poisoning
    match SIZE_CACHE.lock() {
        Ok(mut cache) => {
            if let Some(entry) = cache.get(&path.to_path_buf()) {
                return *entry;
            }
        }
        Err(e) => {
            tracing::warn!("SIZE_CACHE lock poisoned: {}", e);
            // Continue without cache
        }
    }

    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{GetLastError, ERROR_NOT_SUPPORTED, NO_ERROR};
    use windows::Win32::Storage::FileSystem::{GetCompressedFileSizeW, INVALID_FILE_SIZE};

    let w: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut high: u32 = 0;
    unsafe {
        let low = GetCompressedFileSizeW(PCWSTR(w.as_ptr()), Some(&mut high as *mut u32));
        // When the low part is INVALID_FILE_SIZE (0xFFFFFFFF), check GetLastError
        if low == INVALID_FILE_SIZE {
            let err = GetLastError();
            // NO_ERROR means the actual size is 0xFFFFFFFF (very rare but valid)
            if err != NO_ERROR {
                // ERROR_NOT_SUPPORTED or other errors - return None to fallback to logical size
                if err == ERROR_NOT_SUPPORTED {
                    tracing::debug!("GetCompressedFileSizeW not supported for {:?}", path);
                }
                if let Ok(mut cache) = SIZE_CACHE.lock() {
                    cache.put(path.to_path_buf(), None);
                }
                return None;
            }
        }
        let size = ((high as u64) << 32) | (low as u64);

        // In Cache speichern - ignore lock poisoning
        if let Ok(mut cache) = SIZE_CACHE.lock() {
            cache.put(path.to_path_buf(), Some(size));
        } else {
            tracing::debug!("Failed to update SIZE_CACHE (lock poisoned)");
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
    // Saturate at u32::MAX to prevent overflow on extremely deep paths
    let count = path.components().count();
    if count > u32::MAX as usize {
        tracing::warn!("Path depth exceeds u32::MAX, clamping: {:?}", path);
        u32::MAX
    } else {
        count as u32
    }
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
    const NODE_BINDS_PER_ROW: usize = 11; // scan_id, path, parent_path, depth, is_dir, logical_size, allocated_size, file_count, dir_count, mtime, atime
    const FILE_BINDS_PER_ROW: usize = 7; // scan_id, path, parent_path, logical_size, allocated_size, mtime, atime

    // Ensure we never compute 0 rows per statement
    let max_node_rows_per_stmt = (SQLITE_MAX_VARS / NODE_BINDS_PER_ROW).max(1);
    let max_file_rows_per_stmt = (SQLITE_MAX_VARS / FILE_BINDS_PER_ROW).max(1);

    // nodes in chunks
    let node_chunk = batch_size.max(1).min(max_node_rows_per_stmt.max(1));
    for chunk in nodes.chunks(node_chunk) {
        let mut qb = QueryBuilder::new(
            "INSERT INTO nodes (scan_id, path, parent_path, depth, is_dir, logical_size, allocated_size, file_count, dir_count, mtime, atime) "
        );
        qb.push_values(chunk, |mut b, n| {
            // Clamp u64 values to i64::MAX to prevent overflow when converting to i64 for SQLite
            let logical_size_safe = n.logical_size.min(i64::MAX as u64) as i64;
            let allocated_size_safe = n.allocated_size.min(i64::MAX as u64) as i64;
            let file_count_safe = n.file_count.min(i64::MAX as u64) as i64;
            let dir_count_safe = n.dir_count.min(i64::MAX as u64) as i64;

            b.push_bind(&sid)
                .push_bind(&n.path)
                .push_bind(n.parent_path.as_deref())
                .push_bind(n.depth as i64)
                .push_bind(if n.is_dir { 1i64 } else { 0i64 })
                .push_bind(logical_size_safe)
                .push_bind(allocated_size_safe)
                .push_bind(file_count_safe)
                .push_bind(dir_count_safe)
                .push_bind(n.mtime)
                .push_bind(n.atime);
        });
        qb.build().execute(&mut *txdb).await?;
    }

    // files in chunks
    let file_chunk = batch_size.max(1).min(max_file_rows_per_stmt.max(1));
    for chunk in files.chunks(file_chunk) {
        let mut qb = QueryBuilder::new(
            "INSERT INTO files (scan_id, path, parent_path, logical_size, allocated_size, mtime, atime) ",
        );
        qb.push_values(chunk, |mut b, f| {
            // Clamp u64 values to i64::MAX to prevent overflow when converting to i64 for SQLite
            let logical_size_safe = f.logical_size.min(i64::MAX as u64) as i64;
            let allocated_size_safe = f.allocated_size.min(i64::MAX as u64) as i64;

            b.push_bind(&sid)
                .push_bind(&f.path)
                .push_bind(f.parent_path.as_deref())
                .push_bind(logical_size_safe)
                .push_bind(allocated_size_safe)
                .push_bind(f.mtime)
                .push_bind(f.atime);
        });
        qb.build().execute(&mut *txdb).await?;
    }

    txdb.commit().await?;
    nodes.clear();
    files.clear();
    Ok(())
}

fn diff_summary(current: &ScanResultSummary, previous: &ScanResultSummary) -> ScanResultSummary {
    ScanResultSummary {
        total_dirs: current.total_dirs.saturating_sub(previous.total_dirs),
        total_files: current.total_files.saturating_sub(previous.total_files),
        total_logical_size: current.total_logical_size.saturating_sub(previous.total_logical_size),
        total_allocated_size: current.total_allocated_size.saturating_sub(previous.total_allocated_size),
        warnings: current.warnings.saturating_sub(previous.warnings),
        latest_mtime: current.latest_mtime,
        latest_atime: current.latest_atime,
    }
}
