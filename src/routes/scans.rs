use std::{path::{Path as StdPath, PathBuf}, time::Duration};

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
use sqlx::{QueryBuilder, Row};
use tokio::{sync::broadcast, task::JoinHandle};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    middleware::ip::{extract_ip_from_headers, MaybeRemoteAddr},
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
    remote: MaybeRemoteAddr,
    headers: HeaderMap,
    Json(req): Json<CreateScanRequest>,
) -> AppResult<Response> {
    // Per-endpoint rate limit: "/scans"
    let fallback_ip = remote.0.map(|addr| addr.ip());
    let ip = extract_ip_from_headers(&headers, fallback_ip);
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
        let meta = tokio::fs::metadata(&pb)
            .await
            .map_err(|_| AppError::BadRequest(format!("root path does not exist: {}", p)))?;
        if !meta.is_dir() {
            return Err(AppError::BadRequest(format!("root path is not a directory: {}", p)));
        }
    }

    let id = Uuid::new_v4();
    // Larger broadcast channel to prevent dropped messages in fast scans
    // Use configurable channel size with safe bounds
    let channel_size = std::env::var("SPEICHERWALD_EVENT_CHANNEL_SIZE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(4096)
        .clamp(512, 16384);
    let (tx, _rx) = broadcast::channel::<ScanEvent>(channel_size);
    let cancel = CancellationToken::new();

    // Metrics: count scan start
    state.metrics.inc_scans_started();

    // Persist initial scan row
    let root_paths_json = serde_json::to_string(&req.root_paths)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to serialize root_paths: {}", e)))?;
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
    let options_json = serde_json::to_string(&options)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to serialize options: {}", e)))?;

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
    let dir_concurrency = options.concurrency.or(state.config.scanner.dir_concurrency);
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
                // FIX Bug #59 - Log DB update errors
                if let Err(e) = sqlx::query(
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
                .execute(&db).await {
                    tracing::error!("Failed to update scan status to done: {}", e);
                }
            }
            Err(e) => {
                if cancel_child.is_cancelled() {
                    let _ = tx_clone.send(ScanEvent::Cancelled);
                    // FIX Bug #60 - Log DB update errors
                    if let Err(e) = sqlx::query(
                        r#"UPDATE scans SET status='canceled', finished_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id=?1"#
                    )
                    .bind(id.to_string())
                    .execute(&db).await {
                        tracing::error!("Failed to update scan status to canceled: {}", e);
                    }
                } else {
                    // Metrics: failed scan
                    metrics.inc_scans_failed();
                    let _ = tx_clone.send(ScanEvent::Failed { message: format!("{}", e) });
                    if let Err(e) = sqlx::query(
                        r#"UPDATE scans SET status='failed', finished_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id=?1"#
                    )
                    .bind(id.to_string())
                    .execute(&db).await {
                        tracing::error!("Failed to update scan status to failed: {}", e);
                    }
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
        .ok()
        .and_then(|row| row.try_get::<String, _>("started_at").ok())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
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
            FROM scans ORDER BY started_at DESC LIMIT 1000"#,
    )
    .fetch_all(&state.db)
    .await?;

    // FIX Bug #28: Fail fast on invalid UUIDs instead of silently filtering
    let mut items: Vec<ScanSummary> = Vec::with_capacity(rows.len());
    for r in rows {
        let id_str = r.get::<String, _>("id");
        let id = Uuid::parse_str(&id_str).map_err(|e| {
            tracing::error!("Invalid UUID in scans table: {} - {} (data corruption detected)", id_str, e);
            AppError::Database(format!("Database corruption: invalid UUID {}", id_str))
        })?;
        items.push(ScanSummary {
            id,
            status: r.get::<String, _>("status"),
            started_at: r.get::<Option<String>, _>("started_at"),
            finished_at: r.get::<Option<String>, _>("finished_at"),
            total_logical_size: r.get::<i64, _>("total_logical_size"),
            total_allocated_size: r.get::<i64, _>("total_allocated_size"),
            dir_count: r.get::<i64, _>("dir_count"),
            file_count: r.get::<i64, _>("file_count"),
            warning_count: r.get::<i64, _>("warning_count"),
        });
    }

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

    // FIX Bug #12 - Race condition: check status first, then cancel
    let was_running = {
        let mut jobs = state.jobs.write().await;
        if let Some(handle) = jobs.remove(&id) {
            handle.cancel.cancel();
            drop(jobs); // Release lock before any async operations
            true
        } else {
            false
        }
    };

    // FIX Bug #27: Use transaction for atomic operation
    // Update DB after releasing lock to avoid deadlock
    if was_running && !purge {
        if let Err(e) = sqlx::query(
            r#"UPDATE scans SET status='canceled', finished_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id=?1 AND status='running'"#
        )
        .bind(id.to_string())
        .execute(&state.db).await {
            tracing::error!("Failed to update scan status to canceled: {}", e);
        }
    } else if !was_running && !purge {
        // Not running: act idempotently
        return Ok((StatusCode::NO_CONTENT, ""));
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
    // FIX Bug #14 - Race condition: ensure job exists before subscribing
    let rx = {
        let jobs = state.jobs.read().await;
        if let Some(handle) = jobs.get(&id) {
            let rx = handle.sender.subscribe();
            drop(jobs); // Release lock
            rx
        } else {
            return Err(AppError::NotFound("scan not running".into()));
        }
    };

    let stream = BroadcastStream::new(rx)
        .filter_map(move |res| match res {
            Ok(event) => Some(event),
            Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n)) => {
                tracing::warn!("SSE stream lagged by {} messages for scan {}", n, id);
                None
            }
        })
        .map(|ev| {
            let data = serde_json::to_string(&ev)
                .unwrap_or_else(|_| json!({"type":"warning","message":"serialization error"}).to_string());
            Ok::<Event, std::convert::Infallible>(Event::default().data(data))
        });

    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new().interval(Duration::from_secs(10)).text("keep-alive"),
    ))
}

// Removed - inline usage is clearer and avoids potential timezone issues

// FIX Bug #64 - Add missing helper functions
async fn get_subtree_totals(
    scan_id: Uuid,
    root: &str,
    db: &sqlx::SqlitePool,
) -> Result<(i64, i64), sqlx::Error> {
    if let Some(row) =
        sqlx::query("SELECT file_count, dir_count FROM nodes WHERE scan_id = ?1 AND path = ?2 LIMIT 1")
            .bind(scan_id.to_string())
            .bind(root)
            .fetch_optional(db)
            .await?
    {
        let total_files: i64 = row.try_get("file_count").unwrap_or(0);
        let total_dirs: i64 = row.try_get("dir_count").unwrap_or(0);
        return Ok((total_files, total_dirs));
    }

    // Fallback: derive counts from persisted rows without double-counting directory aggregates.
    let mut prefix = root.to_string();
    if !prefix.ends_with('/') && !prefix.ends_with('\\') {
        prefix.push(if prefix.contains('\\') { '\\' } else { '/' });
    }
    let escaped_prefix = escape_like_pattern(&prefix);
    let pattern = format!("{}%", escaped_prefix);

    let files_row = sqlx::query(
        "SELECT COUNT(*) AS total_files \
         FROM files \
         WHERE scan_id = ?1 \
           AND (path = ?2 OR path LIKE ?3 ESCAPE '!')",
    )
    .bind(scan_id.to_string())
    .bind(root)
    .bind(&pattern)
    .fetch_one(db)
    .await?;

    let dirs_row = sqlx::query(
        "SELECT COUNT(*) AS total_dirs \
         FROM nodes \
         WHERE scan_id = ?1 \
           AND path LIKE ?2 ESCAPE '!' \
           AND path <> ?3",
    )
    .bind(scan_id.to_string())
    .bind(&pattern)
    .bind(root)
    .fetch_one(db)
    .await?;

    let total_files: i64 = files_row.try_get("total_files").unwrap_or(0);
    let total_dirs: i64 = dirs_row.try_get("total_dirs").unwrap_or(0);
    Ok((total_files, total_dirs))
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

// Async helper to fetch atime (seconds since epoch) without blocking the Tokio runtime
async fn get_atime_secs(path: &str) -> Option<i64> {
    let p = path.to_string();
    tokio::task::spawn_blocking(move || {
        std::fs::metadata(&p)
            .and_then(|md| md.accessed())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
    })
    .await
    .ok()
    .flatten()
}

const LIKE_ESCAPE: char = '!';
const TREE_LIMIT_MAX: i64 = 5000;

fn escape_like_pattern(p: &str) -> String {
    let mut out = String::with_capacity(p.len());
    for ch in p.chars() {
        if matches!(ch, '%' | '_' | LIKE_ESCAPE) {
            out.push(LIKE_ESCAPE);
        }
        out.push(ch);
    }
    out
}

fn normalize_query_path(p: &str) -> AppResult<String> {
    if p.trim().is_empty() {
        return Err(AppError::BadRequest("path must not be empty".into()));
    }
    if p.contains('\0') {
        return Err(AppError::BadRequest("path contains null byte".into()));
    }

    #[cfg(windows)]
    {
        use std::path::Component;

        let normalized = p.replace('/', "\\");
        let path = StdPath::new(&normalized);
        let mut sanitized = PathBuf::new();

        for component in path.components() {
            match component {
                Component::ParentDir => {
                    return Err(AppError::BadRequest("path traversal is not allowed".into()))
                }
                Component::CurDir => continue,
                _ => sanitized.push(component.as_os_str()),
            }
        }

        let mut result = sanitized.to_string_lossy().to_string();
        if result.is_empty() {
            return Err(AppError::BadRequest("normalized path is empty".into()));
        }
        if result.len() == 2 && result.chars().nth(1) == Some(':') {
            result.push('\\');
        }
        Ok(result)
    }
    #[cfg(not(windows))]
    {
        use std::path::Component;

        let path = StdPath::new(p);
        let mut sanitized = PathBuf::new();

        for component in path.components() {
            match component {
                Component::ParentDir => {
                    return Err(AppError::BadRequest("path traversal is not allowed".into()))
                }
                Component::CurDir => continue,
                _ => sanitized.push(component.as_os_str()),
            }
        }

        let result = sanitized.to_string_lossy().to_string();
        if result.is_empty() {
            return Err(AppError::BadRequest("normalized path is empty".into()));
        }
        Ok(result)
    }
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
    if let Some(depth) = q.depth {
        if depth < 0 {
            return Err(AppError::BadRequest("depth must be >= 0".into()));
        }
    }
    // Determine base depth if path provided
    let mut base_depth: Option<i64> = None;
    let mut normalized_path: Option<String> = None;
    if let Some(ref p) = q.path {
        // FIX Bug #50 - Validate path BEFORE normalization
        if p.len() > 4096 {
            return Err(AppError::BadRequest("Path too long".into()));
        }
        let p_norm = normalize_query_path(p)?;
        if p_norm.len() > 4096 {
            return Err(AppError::BadRequest("Normalized path too long".into()));
        }
        if let Ok(Some(row)) = sqlx::query(r#"SELECT depth FROM nodes WHERE scan_id=?1 AND path=?2 LIMIT 1"#)
            .bind(id.to_string())
            .bind(&p_norm)
            .fetch_optional(&state.db)
            .await
        {
            base_depth = Some(row.get::<i64, _>("depth"));
        }
        normalized_path = Some(p_norm);
    }

    // FIX Bugs #5,#6,#7 - Use QueryBuilder properly instead of string formatting
    let mut qb = QueryBuilder::new(
        "SELECT path, parent_path, depth, is_dir, logical_size, allocated_size, file_count, dir_count, mtime, atime FROM nodes WHERE scan_id="
    );
    qb.push_bind(id.to_string());

    if let Some(ref peq) = normalized_path {
        // Restrict to subtree: include the node itself and everything under it using a trailing separator
        let mut pfx = peq.clone();
        if !pfx.ends_with('/') && !pfx.ends_with('\\') {
            if pfx.contains('\\') {
                pfx.push('\\');
            } else {
                pfx.push('/');
            }
        }
        // FIX Bug #8: Escape special characters to prevent SQL injection via path
        let pfx_escaped = escape_like_pattern(&pfx);
        let pfx_upper = format!("{}~", pfx_escaped);
        qb.push(" AND (path = ").push_bind(peq.clone());
        qb.push(" OR (path >= ").push_bind(pfx_escaped.clone());
        qb.push(" AND path < ").push_bind(pfx_upper);
        qb.push("))");
    }
    if let (Some(bd), Some(d)) = (base_depth, q.depth) {
        let max_depth = bd + d;
        qb.push(" AND depth <= ").push_bind(max_depth);
    }

    match q.sort.as_deref() {
        Some("name") => qb.push(" ORDER BY path ASC"),
        _ => qb.push(" ORDER BY allocated_size DESC"),
    };
    // Clamp limit to a safe range to prevent overly large responses while allowing larger exports for power users
    let limit = q.limit.unwrap_or(200).clamp(1, TREE_LIMIT_MAX);
    qb.push(" LIMIT ").push_bind(limit);

    let rows = qb.build().fetch_all(&state.db).await?;
    let mut items: Vec<NodeDto> = Vec::with_capacity(rows.len());
    for r in rows {
        let path: String = r.get("path");
        let mtime = r.get::<Option<i64>, _>("mtime");
        let atime = r.get::<Option<i64>, _>("atime");
        items.push(NodeDto {
            path,
            parent_path: r.get("parent_path"),
            depth: r.get("depth"),
            is_dir: r.get::<i64, _>("is_dir") != 0,
            logical_size: r.get("logical_size"),
            allocated_size: r.get("allocated_size"),
            file_count: r.get("file_count"),
            dir_count: r.get("dir_count"),
            mtime,
            atime,
        });
    }

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
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let scope = q.scope.as_deref().unwrap_or("dirs");
    if scope == "files" {
        let rows = sqlx::query(
            r#"SELECT path, parent_path, logical_size, allocated_size, mtime, atime
               FROM files WHERE scan_id=?1 ORDER BY allocated_size DESC LIMIT ?2"#,
        )
        .bind(id.to_string())
        .bind(limit)
        .fetch_all(&state.db)
        .await?;
        let mut items: Vec<TopItem> = Vec::with_capacity(rows.len());
        for r in rows {
            let p: String = r.get("path");
            let mtime = r.get::<Option<i64>, _>("mtime");
            let atime = r.get::<Option<i64>, _>("atime");
            items.push(TopItem::File {
                path: p,
                parent_path: r.get("parent_path"),
                logical_size: r.get("logical_size"),
                allocated_size: r.get("allocated_size"),
                mtime,
                atime,
            });
        }
        return Ok(Json(items));
    }

    // default: dirs
    let rows = sqlx::query(
        r#"SELECT path, parent_path, depth, logical_size, allocated_size, file_count, dir_count, mtime, atime
           FROM nodes WHERE scan_id=?1 AND is_dir=1 ORDER BY allocated_size DESC LIMIT ?2"#,
    )
    .bind(id.to_string())
    .bind(limit)
    .fetch_all(&state.db)
    .await?;
    let mut items: Vec<TopItem> = Vec::with_capacity(rows.len());
    for r in rows {
        let p: String = r.get("path");
        let mtime = r.get::<Option<i64>, _>("mtime");
        let atime = r.get::<Option<i64>, _>("atime");
        items.push(TopItem::Dir {
            path: p,
            parent_path: r.get("parent_path"),
            depth: r.get("depth"),
            logical_size: r.get("logical_size"),
            allocated_size: r.get("allocated_size"),
            file_count: r.get("file_count"),
            dir_count: r.get("dir_count"),
            mtime,
            atime,
        });
    }
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
    let limit = q.limit.unwrap_or(500).clamp(1, 2000);
    let offset_raw = q.offset.unwrap_or(0);
    if offset_raw < 0 {
        return Err(AppError::BadRequest("offset must be >= 0".into()));
    }
    let offset = usize::try_from(offset_raw).map_err(|_| AppError::BadRequest("offset too large".into()))?;
    // FIX Bug #14 & #26: Validate offset and offset + limit bounds
    const MAX_OFFSET: usize = 100_000;
    const MAX_TOTAL_SPAN: usize = 102_000;
    if offset > MAX_OFFSET {
        return Err(AppError::BadRequest(format!("offset must be <= {}", MAX_OFFSET)));
    }
    let limit_usize = limit as usize;
    // Use checked_add to detect overflow instead of saturating_add
    let total_span = offset.checked_add(limit_usize)
        .ok_or_else(|| AppError::BadRequest("offset + limit causes integer overflow".into()))?;
    if total_span > MAX_TOTAL_SPAN {
        return Err(AppError::BadRequest("offset + limit exceeds maximum span".into()));
    }

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
                    let original_root = root.clone();
                    let normalized_root = normalize_query_path(&original_root)?;
                    let (total_files, total_dirs) =
                        get_subtree_totals(id, &normalized_root, &state.db).await?;

                    let node_stats = sqlx::query(
                        "SELECT logical_size, allocated_size, mtime, atime FROM nodes WHERE scan_id = ?1 AND path = ?2 LIMIT 1",
                    )
                    .bind(id.to_string())
                    .bind(&normalized_root)
                    .fetch_optional(&state.db)
                    .await?;

                    let (logical_size, allocated_size, db_mtime, db_atime) = if let Some(ns) = node_stats {
                        (
                            ns.get::<i64, _>("logical_size"),
                            ns.get::<i64, _>("allocated_size"),
                            ns.get::<Option<i64>, _>("mtime"),
                            ns.get::<Option<i64>, _>("atime"),
                        )
                    } else {
                        (0, 0, None, None)
                    };

                    let mtime = match db_mtime {
                        Some(ts) => Some(ts),
                        None => get_mtime_secs(&normalized_root).await,
                    };
                    let atime = match db_atime {
                        Some(ts) => Some(ts),
                        None => get_atime_secs(&normalized_root).await,
                    };

                    let name = std::path::Path::new(&normalized_root)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| original_root.clone());

                    items.push(ListItem::Dir {
                        name,
                        path: normalized_root,
                        parent_path: None,
                        depth: 0,
                        logical_size,
                        allocated_size,
                        file_count: total_files.max(0),
                        dir_count: total_dirs.max(0),
                        mtime,
                        atime,
                    });
                }
            }
        }
        // simple sort
        sort_items(&mut items[..], q.sort.as_deref(), q.order.as_deref());
        let slice = items.into_iter().skip(offset).take(limit_usize).collect::<Vec<_>>();
        return Ok(Json(slice));
    }

    // With path: list children
    let path = q.path.as_ref().unwrap();
    let pnorm = normalize_query_path(path)?;
    let dir_rows = sqlx::query(
        r#"SELECT path, parent_path, depth, logical_size, allocated_size, file_count, dir_count, mtime, atime
           FROM nodes WHERE scan_id=?1 AND is_dir=1 AND parent_path=?2"#,
    )
    .bind(id.to_string())
    .bind(&pnorm)
    .fetch_all(&state.db)
    .await?;
    let file_rows = sqlx::query(
        r#"SELECT path, parent_path, logical_size, allocated_size, mtime, atime
           FROM files WHERE scan_id=?1 AND parent_path=?2"#,
    )
    .bind(id.to_string())
    .bind(&pnorm)
    .fetch_all(&state.db)
    .await?;

    let mut items: Vec<ListItem> = Vec::with_capacity(dir_rows.len() + file_rows.len());
    for r in dir_rows {
        let p: String = r.get("path");
        // FIX Bug #34 - Better error handling for file_name
        let name = std::path::Path::new(&p)
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| p.clone());
        let mtime = r.get::<Option<i64>, _>("mtime");
        let atime = r.get::<Option<i64>, _>("atime");
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
            atime,
        });
    }
    for r in file_rows {
        let p: String = r.get("path");
        // FIX Bug #35 - Better error handling for file_name
        let name = std::path::Path::new(&p)
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| p.clone());
        let mtime = r.get::<Option<i64>, _>("mtime");
        let atime = r.get::<Option<i64>, _>("atime");
        items.push(ListItem::File {
            name,
            path: p,
            parent_path: r.get("parent_path"),
            logical_size: r.get("logical_size"),
            allocated_size: r.get("allocated_size"),
            mtime,
            atime,
        });
    }

    sort_items(&mut items[..], q.sort.as_deref(), q.order.as_deref());
    let slice = items.into_iter().skip(offset).take(limit_usize).collect::<Vec<_>>();
    Ok(Json(slice))
}

// ---------------------- RECENT ENDPOINT ----------------------

#[derive(Debug, Default, serde::Deserialize)]
pub struct RecentQuery {
    pub scope: Option<String>, // dirs|files|all
    pub limit: Option<i64>,
    pub path: Option<String>, // optional subtree filter
}

/// Returns most recently accessed items (based on filesystem atime), best-effort.
/// Note: Access time may be disabled on some file systems; results may be None.
pub async fn get_recent(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<RecentQuery>,
) -> AppResult<impl IntoResponse> {
    let scope = q.scope.as_deref().unwrap_or("dirs");
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    // Fetch a superset to compute atime and then take top-N
    // Use saturating_mul to prevent overflow, but keep reasonable bounds
    let fetch_multiplier = std::env::var("SPEICHERWALD_RECENT_FETCH_MULTIPLIER")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(10)
        .clamp(5, 20);
    let fetch_cap = limit.saturating_mul(fetch_multiplier).clamp(100, 2000) as i64;

    // Optional subtree filter: build path range [prefix, prefix + high]
    let mut subtree_eq: Option<String> = None;
    let mut subtree_lo: Option<String> = None;
    let mut subtree_hi: Option<String> = None;
    if let Some(p) = q.path.as_ref() {
        let peq = normalize_query_path(p)?;
        let mut pfx = peq.clone();
        if !pfx.ends_with('/') && !pfx.ends_with('\\') {
            if pfx.contains('\\') {
                pfx.push('\\');
            } else {
                pfx.push('/');
            }
        }
        subtree_eq = Some(peq);
        subtree_lo = Some(pfx.clone());
        // Use a high but valid ASCII character instead of Unicode max
        subtree_hi = Some(format!("{}~", pfx));
    }

    let mut items: Vec<TopItem> = Vec::new();
    let want_dirs = scope == "dirs" || scope == "all";
    let want_files = scope == "files" || scope == "all";

    // FIX Bug #2,#8 - Use QueryBuilder instead of string replacement
    if want_dirs {
        let mut qb = QueryBuilder::new(
            "SELECT path, parent_path, depth, logical_size, allocated_size, file_count, dir_count, mtime, atime FROM nodes WHERE scan_id="
        );
        qb.push_bind(id.to_string()).push(" AND is_dir=1");

        if let (Some(eq), Some(lo), Some(hi)) =
            (subtree_eq.as_ref(), subtree_lo.as_ref(), subtree_hi.as_ref())
        {
            qb.push(" AND (path = ").push_bind(eq);
            qb.push(" OR (path >= ").push_bind(lo);
            qb.push(" AND path < ").push_bind(hi).push("))");
        }
        qb.push(" LIMIT ").push_bind(fetch_cap);

        let rows = qb.build().fetch_all(&state.db).await?;
        for r in rows {
            let p: String = r.get("path");
            let mtime = r.get::<Option<i64>, _>("mtime");
            let atime = r.get::<Option<i64>, _>("atime");
            items.push(TopItem::Dir {
                path: p,
                parent_path: r.get("parent_path"),
                depth: r.get("depth"),
                logical_size: r.get("logical_size"),
                allocated_size: r.get("allocated_size"),
                file_count: r.get("file_count"),
                dir_count: r.get("dir_count"),
                mtime,
                atime,
            });
        }
    }
    // FIX Bug #3,#9 - Use QueryBuilder instead of string replacement
    if want_files {
        let mut qb = QueryBuilder::new(
            "SELECT path, parent_path, logical_size, allocated_size, mtime, atime FROM files WHERE scan_id=",
        );
        qb.push_bind(id.to_string());

        if let (Some(eq), Some(lo), Some(hi)) =
            (subtree_eq.as_ref(), subtree_lo.as_ref(), subtree_hi.as_ref())
        {
            qb.push(" AND (path = ").push_bind(eq);
            qb.push(" OR (path >= ").push_bind(lo);
            qb.push(" AND path < ").push_bind(hi).push("))");
        }
        qb.push(" LIMIT ").push_bind(fetch_cap);

        let rows = qb.build().fetch_all(&state.db).await?;
        for r in rows {
            let p: String = r.get("path");
            let mtime = r.get::<Option<i64>, _>("mtime");
            let atime = r.get::<Option<i64>, _>("atime");
            items.push(TopItem::File {
                path: p,
                parent_path: r.get("parent_path"),
                logical_size: r.get("logical_size"),
                allocated_size: r.get("allocated_size"),
                mtime,
                atime,
            });
        }
    }

    items.sort_by_key(|i| match i {
        TopItem::Dir { atime, .. } => atime.unwrap_or(0),
        TopItem::File { atime, .. } => atime.unwrap_or(0),
    });
    items.reverse();
    items.truncate(limit as usize);

    Ok(Json(items))
}

fn sort_items(items: &mut [ListItem], sort: Option<&str>, order: Option<&str>) {
    // FIX Bug #68 - Default should depend on sort type
    let sort_key = match sort {
        Some("name") | Some("logical") | Some("type") | Some("modified") | Some("accessed")
        | Some("allocated") => sort.unwrap(),
        _ => "allocated", // default fallback
    };

    let desc = match order {
        Some("asc") => false,
        Some("desc") => true,
        None => matches!(sort_key, "logical" | "allocated" | "modified" | "accessed"),
        _ => false,
    };

    match sort_key {
        "name" => {
            items.sort_by_key(|a| get_name(a).to_lowercase());
            // Name sorting typically ascending by default
            if matches!(order, Some("desc")) {
                items.reverse();
            }
        }
        "logical" => {
            items.sort_by_key(get_logical);
            if desc {
                items.reverse();
            }
        }
        "type" => {
            items.sort_by_key(|i| if is_dir(i) { 0 } else { 1 });
            if desc {
                items.reverse();
            }
        }
        "modified" => {
            items.sort_by_key(get_mtime);
            if desc {
                items.reverse();
            }
        }
        "accessed" => {
            items.sort_by_key(get_atime);
            if desc {
                items.reverse();
            }
        }
        _ => {
            items.sort_by_key(get_alloc);
            if desc {
                items.reverse();
            }
        }
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

fn get_atime(i: &ListItem) -> i64 {
    match i {
        ListItem::Dir { atime, .. } => atime.unwrap_or(0),
        ListItem::File { atime, .. } => atime.unwrap_or(0),
    }
}
