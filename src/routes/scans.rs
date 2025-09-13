use std::{path::PathBuf, time::Duration};

use axum::response::sse::{Event, Sse};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures::Stream;
use globset::Glob;
use serde_json::json;
use sqlx::Row;
use tokio::{sync::broadcast, task::JoinHandle};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    middleware::ip::extract_ip_from_headers,
    middleware::validation::{validate_file_path, validate_scan_options},
    scanner,
    state::{AppState, JobHandle},
    types::{
        CreateScanRequest, CreateScanResponse, ListItem, NodeDto, ScanEvent, ScanOptions, ScanSummary,
        TopItem,
    },
};

pub async fn create_scan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateScanRequest>,
) -> AppResult<Response> {
    // Per-endpoint rate limit: "/scans"
    let ip = extract_ip_from_headers(&headers);
    if let Err((status, body)) = state.rate_limiter.check_endpoint_limit("/scans", ip).await {
        return Ok((status, body).into_response());
    }
    if req.root_paths.is_empty() {
        return Err(AppError::BadRequest("root_paths must not be empty".into()));
    }

    // Validate paths
    for path in &req.root_paths {
        validate_file_path(path).map_err(|_| AppError::InvalidInput(format!("Invalid path: {}", path)))?;
    }

    // Validate scan options
    validate_scan_options(req.max_depth, req.concurrency)
        .map_err(|_| AppError::InvalidInput("Invalid scan options".into()))?;

    // Validate roots exist
    for p in &req.root_paths {
        let pb = PathBuf::from(p);
        if !pb.exists() {
            return Err(AppError::BadRequest(format!("root path does not exist: {}", p)));
        }
    }

    let id = Uuid::new_v4();
    let (tx, _rx) = broadcast::channel::<ScanEvent>(256);
    let cancel = CancellationToken::new();

    // Metrics: count scan start
    state.metrics.inc_scans_started();

    // Persist initial scan row
    let root_paths_json = serde_json::to_string(&req.root_paths).unwrap();
    // Apply config defaults if fields are None
    let d = &state.config.scan_defaults;
    // Normalize and validate exclude patterns early (improves cache hit-rate and avoids late failures)
    let excludes_src: Vec<String> = req.excludes.clone().unwrap_or_else(|| d.excludes.clone());
    let mut excludes_norm: Vec<String> = Vec::with_capacity(excludes_src.len());
    for pat in excludes_src {
        let norm = pat.trim().replace('\\', "/");
        if norm.is_empty() {
            continue;
        }
        if let Err(e) = Glob::new(&norm) {
            return Err(AppError::InvalidInput(format!("Invalid exclude pattern: {} ({})", pat, e)));
        }
        excludes_norm.push(norm);
    }

    let options = ScanOptions {
        follow_symlinks: req.follow_symlinks.unwrap_or(d.follow_symlinks),
        include_hidden: req.include_hidden.unwrap_or(d.include_hidden),
        measure_logical: req.measure_logical.unwrap_or(d.measure_logical),
        measure_allocated: req.measure_allocated.unwrap_or(d.measure_allocated),
        excludes: excludes_norm,
        max_depth: req.max_depth.or(d.max_depth),
        concurrency: req.concurrency.or(d.concurrency),
    };
    let options_json = serde_json::to_string(&options).unwrap();

    sqlx::query(
        r#"INSERT INTO scans (id, status, root_paths, options)
           VALUES (?1, 'running', ?2, ?3)"#,
    )
    .bind(id.to_string())
    .bind(root_paths_json)
    .bind(options_json)
    .execute(&state.db)
    .await?;

    // Spawn background task
    let db = state.db.clone();
    let tx_clone = tx.clone();
    let cancel_child = cancel.clone();
    let root_paths = req.root_paths.clone();
    let batch_size = state.config.scanner.batch_size;
    let flush_threshold = state.config.scanner.flush_threshold;
    let flush_interval_ms = state.config.scanner.flush_interval_ms;
    let handle_limit = state.config.scanner.handle_limit;
    let dir_concurrency = state.config.scanner.dir_concurrency;
    let jobs_map = state.jobs.clone();
    let metrics = state.metrics.clone();

    let _handle: JoinHandle<()> = tokio::spawn(async move {
        let res = scanner::run_scan(
            db.clone(),
            id,
            root_paths,
            options.clone(),
            tx_clone.clone(),
            cancel_child.clone(),
            batch_size,
            flush_threshold,
            flush_interval_ms,
            handle_limit,
            dir_concurrency,
        )
        .await;
        match res {
            Ok(summary) => {
                // Metrics: successful scan
                metrics.inc_scans_completed();
                metrics.add_dirs(summary.total_dirs);
                metrics.add_files(summary.total_files);
                metrics.add_bytes(summary.total_allocated_size);
                metrics.add_warnings(summary.warnings as usize);
                let _ = tx_clone.send(ScanEvent::Done {
                    total_dirs: summary.total_dirs,
                    total_files: summary.total_files,
                    total_logical_size: summary.total_logical_size,
                    total_allocated_size: summary.total_allocated_size,
                });
                let _ = sqlx::query(
                    r#"UPDATE scans SET status='done', finished_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                        total_logical_size=?1, total_allocated_size=?2, dir_count=?3, file_count=?4, warning_count=?5
                        WHERE id=?6"#
                )
                .bind(summary.total_logical_size as i64)
                .bind(summary.total_allocated_size as i64)
                .bind(summary.total_dirs as i64)
                .bind(summary.total_files as i64)
                .bind(summary.warnings as i64)
                .bind(id.to_string())
                .execute(&db).await;
            }
            Err(e) => {
                if cancel_child.is_cancelled() {
                    let _ = tx_clone.send(ScanEvent::Cancelled);
                    let _ = sqlx::query(
                        r#"UPDATE scans SET status='canceled', finished_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id=?1"#
                    )
                    .bind(id.to_string())
                    .execute(&db).await;
                } else {
                    // Metrics: failed scan
                    metrics.inc_scans_failed();
                    let _ = tx_clone.send(ScanEvent::Failed { message: format!("{}", e) });
                    let _ = sqlx::query(
                        r#"UPDATE scans SET status='failed', finished_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id=?1"#
                    )
                    .bind(id.to_string())
                    .execute(&db).await;
                }
            }
        }
        // Always remove job handle after completion
        {
            let mut jobs = jobs_map.write().await;
            jobs.remove(&id);
        }
    });

    // Register job
    {
        let mut jobs = state.jobs.write().await;
        jobs.insert(id, JobHandle { cancel: cancel.clone(), sender: tx.clone() });
    }

    // Signal started
    let _ = tx.send(ScanEvent::Started { root_paths: req.root_paths.clone() });

    // Read back ISO UTC started_at from DB for response
    let started_at_iso: String = sqlx::query("SELECT started_at FROM scans WHERE id=?1")
        .bind(id.to_string())
        .fetch_one(&state.db)
        .await
        .map(|row| row.get::<String, _>("started_at"))
        .unwrap_or_else(|_| chrono_now_utc());
    let resp = CreateScanResponse { id, status: "running".into(), started_at: started_at_iso };
    Ok((StatusCode::ACCEPTED, Json(resp)).into_response())
}

pub async fn list_scans(State(state): State<AppState>) -> AppResult<impl IntoResponse> {
    let rows = sqlx::query(
        r#"SELECT id, status, started_at, finished_at,
                   COALESCE(total_logical_size,0) AS total_logical_size,
                   COALESCE(total_allocated_size,0) AS total_allocated_size,
                   COALESCE(dir_count,0) AS dir_count,
                   COALESCE(file_count,0) AS file_count,
                   COALESCE(warning_count,0) AS warning_count
            FROM scans ORDER BY started_at DESC"#,
    )
    .fetch_all(&state.db)
    .await?;

    let items: Vec<ScanSummary> = rows
        .into_iter()
        .map(|r| ScanSummary {
            id: Uuid::parse_str(r.get::<String, _>("id").as_str()).unwrap(),
            status: r.get::<String, _>("status"),
            started_at: r.get::<Option<String>, _>("started_at"),
            finished_at: r.get::<Option<String>, _>("finished_at"),
            total_logical_size: r.get::<i64, _>("total_logical_size"),
            total_allocated_size: r.get::<i64, _>("total_allocated_size"),
            dir_count: r.get::<i64, _>("dir_count"),
            file_count: r.get::<i64, _>("file_count"),
            warning_count: r.get::<i64, _>("warning_count"),
        })
        .collect();

    Ok(Json(items))
}

pub async fn get_scan(State(state): State<AppState>, Path(id): Path<Uuid>) -> AppResult<impl IntoResponse> {
    let r = sqlx::query(
        r#"SELECT id, status, started_at, finished_at,
                   COALESCE(total_logical_size,0) AS total_logical_size,
                   COALESCE(total_allocated_size,0) AS total_allocated_size,
                   COALESCE(dir_count,0) AS dir_count,
                   COALESCE(file_count,0) AS file_count,
                   COALESCE(warning_count,0) AS warning_count
            FROM scans WHERE id = ?1"#,
    )
    .bind(id.to_string())
    .fetch_optional(&state.db)
    .await?;

    if let Some(r) = r {
        let item = ScanSummary {
            id,
            status: r.get::<String, _>("status"),
            started_at: r.get::<Option<String>, _>("started_at"),
            finished_at: r.get::<Option<String>, _>("finished_at"),
            total_logical_size: r.get::<i64, _>("total_logical_size"),
            total_allocated_size: r.get::<i64, _>("total_allocated_size"),
            dir_count: r.get::<i64, _>("dir_count"),
            file_count: r.get::<i64, _>("file_count"),
            warning_count: r.get::<i64, _>("warning_count"),
        };
        Ok(Json(item))
    } else {
        Err(AppError::NotFound("scan not found".into()))
    }
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct CancelQuery {
    pub purge: Option<bool>,
}

pub async fn cancel_scan(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<CancelQuery>,
) -> AppResult<impl IntoResponse> {
    let purge = q.purge.unwrap_or(false);

    // Cancel if running
    {
        let mut jobs = state.jobs.write().await;
        if let Some(handle) = jobs.remove(&id) {
            handle.cancel.cancel();
            if !purge {
                let _ = sqlx::query(
                    r#"UPDATE scans SET status='canceled', finished_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id=?1 AND status='running'"#
                )
                .bind(id.to_string())
                .execute(&state.db).await;
            }
        } else if !purge {
            // Not running: act idempotently
            return Ok((StatusCode::NO_CONTENT, ""));
        }
    }

    if purge {
        // Delete scan row (cascade to nodes/files/warnings)
        let _ = sqlx::query(r#"DELETE FROM scans WHERE id=?1"#).bind(id.to_string()).execute(&state.db).await;
    }

    Ok((StatusCode::NO_CONTENT, ""))
}

pub async fn scan_events(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>> {
    let rx = {
        let jobs = state.jobs.read().await;
        if let Some(handle) = jobs.get(&id) {
            handle.sender.subscribe()
        } else {
            return Err(AppError::NotFound("scan not running".into()));
        }
    };

    let stream = BroadcastStream::new(rx).filter_map(|res| res.ok()).map(|ev| {
        let data = serde_json::to_string(&ev)
            .unwrap_or_else(|_| json!({"type":"warning","message":"serialization error"}).to_string());
        Ok::<Event, std::convert::Infallible>(Event::default().data(data))
    });

    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new().interval(Duration::from_secs(10)).text("keep-alive"),
    ))
}

fn chrono_now_utc() -> String {
    // Match DB default format (UTC ISO-8601). A 'Z' suffix indicates UTC.
    // The exact seconds precision suffices for our API and logs.
    chrono::Utc::now().to_rfc3339()
}

// Async helper to fetch mtime (seconds since epoch) without blocking the Tokio runtime
async fn get_mtime_secs(path: &str) -> Option<i64> {
    let p = path.to_string();
    tokio::task::spawn_blocking(move || {
        std::fs::metadata(&p)
            .and_then(|md| md.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
    })
    .await
    .ok()
    .flatten()
}

fn normalize_query_path(p: &str) -> String {
    let mut s = p.replace('/', "\\");
    if s.len() == 2 && s.chars().nth(1) == Some(':') {
        s.push('\\');
    }
    s
}

// ---------------------- TREE ENDPOINT ----------------------

#[derive(Debug, Default, serde::Deserialize)]
pub struct TreeQuery {
    pub path: Option<String>,
    pub depth: Option<i64>,
    pub sort: Option<String>, // size|name
    pub limit: Option<i64>,
}

pub async fn get_tree(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<TreeQuery>,
) -> AppResult<impl IntoResponse> {
    // Determine base depth if path provided
    let mut base_depth: Option<i64> = None;
    if let Some(ref p) = q.path {
        let p_norm = normalize_query_path(p);
        if let Ok(Some(row)) = sqlx::query(r#"SELECT depth FROM nodes WHERE scan_id=?1 AND path=?2 LIMIT 1"#)
            .bind(id.to_string())
            .bind(&p_norm)
            .fetch_optional(&state.db)
            .await
        {
            base_depth = Some(row.get::<i64, _>("depth"));
        }
    }

    let mut sql = String::from(
        "SELECT path, parent_path, depth, is_dir, logical_size, allocated_size, file_count, dir_count FROM nodes WHERE scan_id=?1"
    );
    let mut idx = 2;
    let mut pattern_eq: Option<String> = None;
    let mut pattern_lo: Option<String> = None;
    let mut pattern_hi: Option<String> = None;
    let mut max_depth: Option<i64> = None;

    if let Some(ref p) = q.path {
        // Restrict to subtree: include the node itself and everything under it using a trailing separator
        let peq = normalize_query_path(p); // exact node path as stored
        let mut pfx = peq.clone();
        if !pfx.ends_with('/') && !pfx.ends_with('\\') {
            if pfx.contains('\\') {
                pfx.push('\\');
            } else {
                pfx.push('/');
            }
        }
        sql.push_str(&format!(" AND (path = ?{} OR (path >= ?{} AND path < ?{}))", idx, idx + 1, idx + 2));
        pattern_eq = Some(peq);
        pattern_lo = Some(pfx.clone());
        pattern_hi = Some(format!("{}{}", pfx, '\u{10ffff}'));
        idx += 3;
    }
    if let (Some(bd), Some(d)) = (base_depth, q.depth) {
        sql.push_str(&format!(" AND depth <= ?{}", idx));
        idx += 1;
        max_depth = Some(bd + d);
    }

    match q.sort.as_deref() {
        Some("name") => sql.push_str(" ORDER BY path ASC"),
        _ => sql.push_str(" ORDER BY allocated_size DESC"),
    }
    // Clamp limit to a safe range to prevent overly large responses
    let limit = q.limit.unwrap_or(200).clamp(1, 5000);
    sql.push_str(&format!(" LIMIT ?{}", idx));

    let mut qx = sqlx::query(&sql).bind(id.to_string());
    if let Some(eq) = pattern_eq {
        qx = qx.bind(eq);
    }
    if let Some(pat) = pattern_lo {
        qx = qx.bind(pat);
    }
    if let Some(hi) = pattern_hi {
        qx = qx.bind(hi);
    }
    if let Some(md) = max_depth {
        qx = qx.bind(md);
    }
    qx = qx.bind(limit);

    let rows = qx.fetch_all(&state.db).await?;
    let items: Vec<NodeDto> = rows
        .into_iter()
        .map(|r| NodeDto {
            path: r.get::<String, _>("path"),
            parent_path: r.get::<Option<String>, _>("parent_path"),
            depth: r.get::<i64, _>("depth"),
            is_dir: r.get::<i64, _>("is_dir") != 0,
            logical_size: r.get::<i64, _>("logical_size"),
            allocated_size: r.get::<i64, _>("allocated_size"),
            file_count: r.get::<i64, _>("file_count"),
            dir_count: r.get::<i64, _>("dir_count"),
        })
        .collect();

    Ok(Json(items))
}

// ---------------------- TOP ENDPOINT ----------------------

#[derive(Debug, Default, serde::Deserialize)]
pub struct TopQuery {
    pub scope: Option<String>, // dirs|files
    pub limit: Option<i64>,
}

pub async fn get_top(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<TopQuery>,
) -> AppResult<impl IntoResponse> {
    // Clamp limit to a safe range to prevent overly large responses
    let limit = q.limit.unwrap_or(100).clamp(1, 5000);
    let scope = q.scope.as_deref().unwrap_or("dirs");
    if scope == "files" {
        let rows = sqlx::query(
            r#"SELECT path, parent_path, logical_size, allocated_size
               FROM files WHERE scan_id=?1 ORDER BY allocated_size DESC LIMIT ?2"#,
        )
        .bind(id.to_string())
        .bind(limit)
        .fetch_all(&state.db)
        .await?;
        let items: Vec<TopItem> = rows
            .into_iter()
            .map(|r| TopItem::File {
                path: r.get::<String, _>("path"),
                parent_path: r.get::<Option<String>, _>("parent_path"),
                logical_size: r.get::<i64, _>("logical_size"),
                allocated_size: r.get::<i64, _>("allocated_size"),
            })
            .collect();
        return Ok(Json(items));
    }

    // default: dirs
    let rows = sqlx::query(
        r#"SELECT path, parent_path, depth, logical_size, allocated_size, file_count, dir_count
           FROM nodes WHERE scan_id=?1 AND is_dir=1 ORDER BY allocated_size DESC LIMIT ?2"#,
    )
    .bind(id.to_string())
    .bind(limit)
    .fetch_all(&state.db)
    .await?;
    let items: Vec<TopItem> = rows
        .into_iter()
        .map(|r| TopItem::Dir {
            path: r.get::<String, _>("path"),
            parent_path: r.get::<Option<String>, _>("parent_path"),
            depth: r.get::<i64, _>("depth"),
            logical_size: r.get::<i64, _>("logical_size"),
            allocated_size: r.get::<i64, _>("allocated_size"),
            file_count: r.get::<i64, _>("file_count"),
            dir_count: r.get::<i64, _>("dir_count"),
        })
        .collect();
    Ok(Json(items))
}

// ---------------------- LIST ENDPOINT ----------------------

#[derive(Debug, Default, serde::Deserialize)]
pub struct ListQuery {
    pub path: Option<String>,  // if None: list roots only (directories)
    pub sort: Option<String>,  // allocated|logical|name|type
    pub order: Option<String>, // asc|desc
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn get_list(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<ListQuery>,
) -> AppResult<impl IntoResponse> {
    let limit = q.limit.unwrap_or(500).clamp(1, 5000);
    let offset = q.offset.unwrap_or(0).max(0);

    // If no path specified, return the scan roots as directories
    if q.path.is_none() {
        let row = sqlx::query("SELECT root_paths FROM scans WHERE id=?1")
            .bind(id.to_string())
            .fetch_optional(&state.db)
            .await?;
        let mut items: Vec<ListItem> = vec![];
        if let Some(r) = row {
            if let Ok(roots) = serde_json::from_str::<Vec<String>>(&r.get::<String, _>("root_paths")) {
                // fetch nodes for these paths to get sizes/counts
                for root in roots {
                    match sqlx::query(
                        r#"SELECT path, parent_path, depth, logical_size, allocated_size, file_count, dir_count
                           FROM nodes WHERE scan_id=?1 AND path=?2 LIMIT 1"#)
                        .bind(id.to_string()).bind(&root).fetch_optional(&state.db).await {
                        Ok(Some(nr)) => {
                            let name = std::path::Path::new(&root)
                                .file_name().and_then(|s| s.to_str()).unwrap_or(&root).to_string();
                            let path_val: String = nr.get::<String,_>("path");
                            let mtime = get_mtime_secs(&path_val).await;
                            items.push(ListItem::Dir {
                                name,
                                path: path_val,
                                parent_path: nr.get::<Option<String>,_>("parent_path"),
                                depth: nr.get::<i64,_>("depth"),
                                logical_size: nr.get::<i64,_>("logical_size"),
                                allocated_size: nr.get::<i64,_>("allocated_size"),
                                file_count: nr.get::<i64,_>("file_count"),
                                dir_count: nr.get::<i64,_>("dir_count"),
                                mtime,
                            });
                        }
                        _ => {
                            // Fallback: Root-Knoten noch nicht in DB (Scan läuft). Platzhalter zurückgeben
                            let name = std::path::Path::new(&root)
                                .file_name()
                                .and_then(|s| s.to_str())
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| {
                                    let t = root.trim_end_matches(['\\','/']).to_string();
                                    if t.is_empty() { root.clone() } else { t }
                                });
                            let mtime = get_mtime_secs(&root).await;
                            items.push(ListItem::Dir {
                                name,
                                path: root.clone(),
                                parent_path: None,
                                depth: 0,
                                logical_size: 0,
                                allocated_size: 0,
                                file_count: 0,
                                dir_count: 0,
                                mtime,
                            });
                        }
                    }
                }
            }
        }
        // simple sort
        sort_items(&mut items[..], q.sort.as_deref(), q.order.as_deref());
        let slice = items.into_iter().skip(offset as usize).take(limit as usize).collect::<Vec<_>>();
        return Ok(Json(slice));
    }

    // With path: list children
    let path = q.path.as_ref().unwrap();
    let pnorm = normalize_query_path(path);
    let dir_rows = sqlx::query(
        r#"SELECT path, parent_path, depth, logical_size, allocated_size, file_count, dir_count
           FROM nodes WHERE scan_id=?1 AND is_dir=1 AND parent_path=?2"#,
    )
    .bind(id.to_string())
    .bind(&pnorm)
    .fetch_all(&state.db)
    .await?;
    let file_rows = sqlx::query(
        r#"SELECT path, parent_path, logical_size, allocated_size
           FROM files WHERE scan_id=?1 AND parent_path=?2"#,
    )
    .bind(id.to_string())
    .bind(&pnorm)
    .fetch_all(&state.db)
    .await?;

    let mut items: Vec<ListItem> = Vec::with_capacity(dir_rows.len() + file_rows.len());
    for r in dir_rows {
        let p: String = r.get("path");
        let name = std::path::Path::new(&p).file_name().and_then(|s| s.to_str()).unwrap_or(&p).to_string();
        let mtime = get_mtime_secs(&p).await;
        items.push(ListItem::Dir {
            name,
            path: p,
            parent_path: r.get("parent_path"),
            depth: r.get("depth"),
            logical_size: r.get("logical_size"),
            allocated_size: r.get("allocated_size"),
            file_count: r.get("file_count"),
            dir_count: r.get("dir_count"),
            mtime,
        });
    }
    for r in file_rows {
        let p: String = r.get("path");
        let name = std::path::Path::new(&p).file_name().and_then(|s| s.to_str()).unwrap_or(&p).to_string();
        let mtime = get_mtime_secs(&p).await;
        items.push(ListItem::File {
            name,
            path: p,
            parent_path: r.get("parent_path"),
            logical_size: r.get("logical_size"),
            allocated_size: r.get("allocated_size"),
            mtime,
        });
    }

    sort_items(&mut items[..], q.sort.as_deref(), q.order.as_deref());
    let slice = items.into_iter().skip(offset as usize).take(limit as usize).collect::<Vec<_>>();
    Ok(Json(slice))
}

fn sort_items(items: &mut [ListItem], sort: Option<&str>, order: Option<&str>) {
    let desc = matches!(order, Some("desc")) || order.is_none();
    match sort.unwrap_or("allocated") {
        "name" => items.sort_by_key(|a| get_name(a).to_lowercase()),
        "logical" => items.sort_by_key(get_logical),
        "type" => items.sort_by_key(|i| if is_dir(i) { 0 } else { 1 }),
        "modified" => items.sort_by_key(get_mtime),
        _ => items.sort_by_key(get_alloc),
    }
    if desc {
        items.reverse();
    }
}

fn get_name(i: &ListItem) -> String {
    match i {
        ListItem::Dir { name, .. } => name.clone(),
        ListItem::File { name, .. } => name.clone(),
    }
}
fn get_alloc(i: &ListItem) -> i64 {
    match i {
        ListItem::Dir { allocated_size, .. } => *allocated_size,
        ListItem::File { allocated_size, .. } => *allocated_size,
    }
}
fn get_logical(i: &ListItem) -> i64 {
    match i {
        ListItem::Dir { logical_size, .. } => *logical_size,
        ListItem::File { logical_size, .. } => *logical_size,
    }
}
fn is_dir(i: &ListItem) -> bool {
    matches!(i, ListItem::Dir { .. })
}

fn get_mtime(i: &ListItem) -> i64 {
    match i {
        ListItem::Dir { mtime, .. } => mtime.unwrap_or(0),
        ListItem::File { mtime, .. } => mtime.unwrap_or(0),
    }
}
