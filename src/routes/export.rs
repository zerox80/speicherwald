//! Data export API endpoints for scan results.
//!
//! This module provides HTTP endpoints for exporting scan data in various formats
//! including CSV and JSON. It supports both partial and full exports of scan
//! results with configurable limits and scopes.
//!
//! ## Features
//!
//! - **Multiple Formats**: Export data as CSV or JSON
//! - **Flexible Scopes**: Export nodes (directories), files, or both
//! - **Configurable Limits**: Control the number of records exported
//! - **Statistics**: Export summary statistics for scans
//! - **CSV Escaping**: Proper CSV escaping for special characters
//! - **Batch Processing**: Efficient chunked database queries

use axum::{
    extract::{Path, Query, State},
    http::header,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    state::AppState,
};

/// Query parameters for the export endpoint.
#[derive(Debug, Deserialize)]
pub struct ExportQuery {
    /// The export format (e.g., "csv", "json").
    pub format: String,        // csv or json
    /// The scope of the export (e.g., "nodes", "files", "all").
    pub scope: Option<String>, // nodes, files, or all
    /// The maximum number of records to export.
    pub limit: Option<i64>,
}

/// Formats a node record as a CSV line.
///
/// This function converts a directory node into a properly escaped CSV format
/// with all relevant metadata fields.
///
/// # Arguments
///
/// * `node` - The node to format
///
/// # Returns
///
/// A string containing the CSV-formatted node record
fn format_node_csv(node: &NodeExport) -> String {
    format!(
        "Dir,\"{}\",\"{}\",{},{},{},{},{},{}\n",
        escape_csv(&node.path),
        escape_csv(node.parent_path.as_deref().unwrap_or("")),
        node.depth,
        if node.is_dir { 1 } else { 0 },
        node.logical_size,
        node.allocated_size,
        node.file_count,
        node.dir_count
    )
}

/// The structure of the JSON export.
#[derive(Debug, Serialize)]
pub struct ExportData {
    /// The ID of the scan being exported.
    pub scan_id: String,
    /// The timestamp of the export.
    pub exported_at: String,
    /// The export format.
    pub format: String,
    /// The exported nodes.
    pub nodes: Option<Vec<NodeExport>>,
    /// The exported files.
    pub files: Option<Vec<FileExport>>,
}

/// A node (directory) record for export.
#[derive(Debug, Serialize)]
pub struct NodeExport {
    /// The path of the node.
    pub path: String,
    /// The parent path of the node.
    pub parent_path: Option<String>,
    /// The depth of the node in the directory tree.
    pub depth: i64,
    /// Whether the node is a directory.
    pub is_dir: bool,
    /// The logical size of the node in bytes.
    pub logical_size: i64,
    /// The allocated size of the node in bytes.
    pub allocated_size: i64,
    /// The number of files in the node.
    pub file_count: i64,
    /// The number of subdirectories in the node.
    pub dir_count: i64,
}

/// A file record for export.
#[derive(Debug, Serialize)]
pub struct FileExport {
    /// The path of the file.
    pub path: String,
    /// The parent path of the file.
    pub parent_path: Option<String>,
    /// The logical size of the file in bytes.
    pub logical_size: i64,
    /// The allocated size of the file in bytes.
    pub allocated_size: i64,
}

/// Exports the data of a scan in either CSV or JSON format.
///
/// # Arguments
///
/// * `state` - The application state.
/// * `id` - The ID of the scan to export.
/// * `query` - The export query parameters.
///
/// # Returns
///
/// * `AppResult<Response>` - The exported data as a file download.
pub async fn export_scan(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<ExportQuery>,
) -> AppResult<Response> {
    // Validate scan exists
    let scan = sqlx::query("SELECT id FROM scans WHERE id = ?1")
        .bind(id.to_string())
        .fetch_optional(&state.db)
        .await?;

    if scan.is_none() {
        return Err(AppError::NotFound("Scan not found".to_string()));
    }

    let requested_limit = query.limit.unwrap_or(10_000);
    // Log warning if user requests excessive limit
    if requested_limit > 25_000 {
        tracing::warn!("Export limit clamped from {} to 25000 for scan {}", requested_limit, id);
    }
    let limit = requested_limit.clamp(1, 25_000); // Reduced to prevent server overload and memory issues
    let scope = query.scope.as_deref().unwrap_or("all");

    match query.format.as_str() {
        "csv" => export_csv(state, id, scope, limit).await.map(|r| r.into_response()),
        "json" => export_json(state, id, scope, limit).await.map(|r| r.into_response()),
        _ => Err(AppError::BadRequest("Invalid format. Use 'csv' or 'json'".to_string())),
    }
}

/// Exports scan data in CSV format.
///
/// This function generates a CSV file containing scan results based on the specified scope.
/// It includes proper headers and handles both nodes (directories) and files.
///
/// # Arguments
///
/// * `state` - The application state containing database connection
/// * `scan_id` - The UUID of the scan to export
/// * `scope` - The export scope: "nodes", "files", or "all"
/// * `limit` - Maximum number of records to export
///
/// # Returns
///
/// An HTTP response with CSV content and appropriate headers for file download
/// Exports scan data in CSV format.
///
/// This function generates a CSV file containing scan results based on the specified scope.
/// It includes proper headers and handles both nodes (directories) and files.
///
/// # Arguments
///
/// * `state` - The application state containing database connection
/// * `scan_id` - The UUID of the scan to export
/// * `scope` - The export scope: "nodes", "files", or "all"
/// * `limit` - Maximum number of records to export
///
/// # Returns
///
/// An HTTP response with CSV content and appropriate headers for file download
async fn export_csv(state: AppState, scan_id: Uuid, scope: &str, limit: i64) -> AppResult<impl IntoResponse> {
    use axum::body::Body;
    use axum::http::HeaderValue;
    use futures::stream::TryStreamExt;

    let include_nodes = scope == "all" || scope == "nodes";
    let include_files = scope == "all" || scope == "files";
    let scope_str = scope.to_string();

    // Initial state: (last_node_cursor, last_file_cursor, nodes_done, files_done, header_sent, exported_count)
    let initial_state = (None::<String>, None::<(i64, String)>, false, false, false, 0i64);

    let stream = futures::stream::try_unfold(
        initial_state,
        move |(mut last_node_cursor, mut last_file_cursor, mut nodes_done, mut files_done, mut header_sent, mut count)| {
            let state = state.clone();
            let scope = scope_str.clone();
            async move {
                if nodes_done && files_done {
                    // Type annotation needed for the compiler
                    return Ok::<Option<(String, (Option<String>, Option<(i64, String)>, bool, bool, bool, i64))>, AppError>(None);
                }
                
                let remaining = limit - count;
                if remaining <= 0 {
                    return Ok(None); 
                }

                let mut chunk = String::new();

                // 1. Send Headers if not sent
                if !header_sent {
                    header_sent = true;
                }

                let batch_size = EXPORT_CHUNK_SIZE.min(remaining);
                
                // 2. Fetch Nodes
                if include_nodes && !nodes_done {
                    if count == 0 {
                        chunk.push_str("Type,Path,Parent Path,Depth,Is Directory,Logical Size,Allocated Size,File Count,Dir Count\n");
                    }
                    
                    if batch_size <= 0 {
                        nodes_done = true;
                    } else {
                        let nodes = fetch_nodes_batch(&state, scan_id, batch_size, last_node_cursor.clone()).await.map_err(AppError::from)?;
                        if nodes.is_empty() {
                            nodes_done = true;
                        } else {
                            for node in &nodes {
                                chunk.push_str(&format_node_csv(node));
                            }
                            if let Some(last) = nodes.last() {
                                last_node_cursor = Some(last.path.clone());
                            }
                            count += nodes.len() as i64;
                        }
                    }
                    
                    if nodes_done {
                        last_node_cursor = None; 
                        if include_files {
                            chunk.push('\n');
                        }
                    }
                } 
                // 3. Fetch Files
                else if include_files && !files_done {
                     if last_file_cursor.is_none() { 
                         chunk.push_str("Type,Path,Parent Path,Logical Size,Allocated Size\n");
                     }
 
                     let remaining = limit - count;
                     let batch_size = EXPORT_CHUNK_SIZE.min(remaining);

                     if batch_size <= 0 {
                         files_done = true;
                     } else {
                         let files = fetch_files_batch(&state, scan_id, batch_size, last_file_cursor.clone()).await.map_err(AppError::from)?;
                         if files.is_empty() {
                             files_done = true;
                         } else {
                             for file in &files {
                                 chunk.push_str(&format!(
                                     "File,\"{}\",\"{}\",{},{}\n",
                                     escape_csv(&file.path),
                                     escape_csv(file.parent_path.as_deref().unwrap_or("")),
                                     file.logical_size,
                                     file.allocated_size,
                                 ));
                             }
                             if let Some(last) = files.last() {
                                 last_file_cursor = Some((last.allocated_size, last.path.clone()));
                             }
                             count += files.len() as i64;
                         }
                     }
                } else {
                    return Ok(None);
                }
                
                Ok(Some((chunk, (last_node_cursor, last_file_cursor, nodes_done, files_done, header_sent, count))))
            }
        },
    );

    let mut response = Response::builder()
        .header(header::CONTENT_TYPE, HeaderValue::from_static("text/csv; charset=utf-8"))
        .body(Body::from_stream(stream))
        .unwrap();

    let filename = format!("attachment; filename=\"scan_{}.csv\"", scan_id);
    if let Ok(header_val) = HeaderValue::from_str(&filename) {
        response.headers_mut().insert(header::CONTENT_DISPOSITION, header_val);
    }
    Ok(response)
}

/// Exports scan data in JSON format.
///
/// This function generates a JSON file containing scan results based on the specified scope.
/// The JSON structure includes metadata about the export and arrays of nodes and files.
///
/// # Arguments
///
/// * `state` - The application state containing database connection
/// * `scan_id` - The UUID of the scan to export
/// * `scope` - The export scope: "nodes", "files", or "all"
/// * `limit` - Maximum number of records to export
///
/// # Returns
///
/// An HTTP response with JSON content and appropriate headers for file download
async fn export_json(
    state: AppState,
    scan_id: Uuid,
    scope: &str,
    limit: i64,
) -> AppResult<impl IntoResponse> {
    let mut export_data = ExportData {
        scan_id: scan_id.to_string(),
        exported_at: chrono::Utc::now().to_rfc3339(),
        format: "json".to_string(),
        nodes: None,
        files: None,
    };

    if scope == "all" || scope == "nodes" {
        export_data.nodes = Some(fetch_nodes_all(&state, scan_id, limit).await?);
    }

    if scope == "all" || scope == "files" {
        export_data.files = Some(fetch_files_all(&state, scan_id, limit).await?);
    }

    use axum::http::HeaderValue;

    let mut response = Json(export_data).into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8"));
    let filename = format!("attachment; filename=\"scan_{}.json\"", scan_id);
    if let Ok(header_val) = HeaderValue::from_str(&filename) {
        response.headers_mut().insert(header::CONTENT_DISPOSITION, header_val);
    }
    Ok(response)
}

/// Escapes a string for safe CSV output.
///
/// This function handles CSV escaping by replacing dangerous characters:
/// - Double quotes are escaped as two double quotes
/// - Newline and carriage return are replaced with spaces
/// - Other control characters are replaced with spaces
///
/// # Arguments
///
/// * `s` - The string to escape
///
/// # Returns
///
/// A CSV-safe version of the input string
fn escape_csv(s: &str) -> String {
    // FIX Bug #7 - Optimization: Avoid excessive allocations from flat_map/vec!
    let mut out = String::with_capacity(s.len() + 10);
    for c in s.chars() {
        match c {
            '"' => { out.push('"'); out.push('"'); },
            '\n' | '\r' => out.push(' '),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Chunk size for database export queries.
///
/// This constant defines the number of records fetched per database query
/// to balance memory usage and performance.
const EXPORT_CHUNK_SIZE: i64 = 800;

// Modified for streaming: just fetch one batch at the specific offset and return it.
// The caller (stream) manages the offset loop.
/// Fetches all nodes for JSON export (or non-streaming).
async fn fetch_nodes_all(state: &AppState, scan_id: Uuid, limit: i64) -> Result<Vec<NodeExport>, sqlx::Error> {
    let mut results = Vec::new();
    let mut current_cursor: Option<String> = None;
    let mut count = 0;
    loop {
        let remaining = limit - count;
        if remaining <= 0 { break; }
        let batch_size = EXPORT_CHUNK_SIZE.min(remaining);
        
        let batch = fetch_nodes_batch(state, scan_id, batch_size, current_cursor.clone()).await?;

        if batch.is_empty() { break; }
        
        if let Some(last) = batch.last() {
            current_cursor = Some(last.path.clone());
        }

        count += batch.len() as i64;
        results.extend(batch);
    }
    Ok(results)
}

/// Fetches a single batch of nodes for export.
async fn fetch_nodes_batch(
    state: &AppState, 
    scan_id: Uuid, 
    limit: i64, 
    cursor_path: Option<String>
) -> Result<Vec<NodeExport>, sqlx::Error> {
    let sid = scan_id.to_string();
    let query_str = if cursor_path.is_some() {
        "SELECT path, parent_path, depth, is_dir, logical_size, allocated_size, file_count, dir_count \
         FROM nodes WHERE scan_id = ?1 AND is_dir = 1 AND path > ?2 ORDER BY path ASC LIMIT ?3"
    } else {
        "SELECT path, parent_path, depth, is_dir, logical_size, allocated_size, file_count, dir_count \
         FROM nodes WHERE scan_id = ?1 AND is_dir = 1 ORDER BY path ASC LIMIT ?2"
    };

    let query = if let Some(path) = cursor_path.as_ref() {
         sqlx::query(query_str)
             .bind(&sid)
             .bind(path)
             .bind(limit)
    } else {
         sqlx::query(query_str)
             .bind(&sid)
             .bind(limit)
    };
    
    let rows = query.fetch_all(&state.db).await?;


    let mut results = Vec::with_capacity(rows.len());
    for row in rows {
        results.push(NodeExport {
            path: row.get("path"),
            parent_path: row.get("parent_path"),
            depth: row.get("depth"),
            is_dir: row.get("is_dir"),
            logical_size: row.get("logical_size"),
            allocated_size: row.get("allocated_size"),
            file_count: row.get("file_count"),
            dir_count: row.get("dir_count"),
        });
    }
    Ok(results)
}

/// Fetches all files for JSON export (or non-streaming).
async fn fetch_files_all(state: &AppState, scan_id: Uuid, limit: i64) -> Result<Vec<FileExport>, sqlx::Error> {
    let mut results = Vec::new();
    let mut current_cursor: Option<(i64, String)> = None;
    let mut count = 0;
    loop {
        let remaining = limit - count;
        if remaining <= 0 { break; }
        let batch_size = EXPORT_CHUNK_SIZE.min(remaining);
        
        let batch = fetch_files_batch(state, scan_id, batch_size, current_cursor.clone()).await?;

        if batch.is_empty() { break; }
        
        if let Some(last) = batch.last() {
            current_cursor = Some((last.allocated_size, last.path.clone()));
        }
        
        count += batch.len() as i64;
        results.extend(batch);
    }
    Ok(results)
}

/// Fetches a single batch of files for export.
async fn fetch_files_batch(
    state: &AppState, 
    scan_id: Uuid, 
    limit: i64, 
    cursor: Option<(i64, String)>
) -> Result<Vec<FileExport>, sqlx::Error> {
    let sid = scan_id.to_string();
    // Keyset: (allocated_size, path) < (last_alloc, last_path)
    // DESC order for allocated_size, ASC for path (determinism)
    // WHERE allocated_size < ? OR (allocated_size = ? AND path > ?) 
    
    let query_str = if cursor.is_some() {
        "SELECT path, parent_path, logical_size, allocated_size \
         FROM files WHERE scan_id = ?1 AND (allocated_size < ?2 OR (allocated_size = ?3 AND path > ?4)) \
         ORDER BY allocated_size DESC, path ASC LIMIT ?5"
    } else {
        "SELECT path, parent_path, logical_size, allocated_size \
         FROM files WHERE scan_id = ?1 ORDER BY allocated_size DESC, path ASC LIMIT ?2"
    };

    let query = if let Some((last_alloc, last_path)) = cursor {
         sqlx::query(query_str)
             .bind(&sid)
             .bind(last_alloc)
             .bind(last_alloc)
             .bind(last_path)
             .bind(limit)
    } else {
         sqlx::query(query_str)
             .bind(&sid)
             .bind(limit)
    };
    
    let rows = query.fetch_all(&state.db).await?;


    let mut results = Vec::with_capacity(rows.len());
    for row in rows {
        results.push(FileExport {
            path: row.get("path"),
            parent_path: row.get("parent_path"),
            logical_size: row.get("logical_size"),
            allocated_size: row.get("allocated_size"),
        });
    }
    Ok(results)
}

/// Exports summary statistics for a scan.
///
/// # Arguments
///
/// * `state` - The application state.
/// * `id` - The ID of the scan.
///
/// # Returns
///
/// * `AppResult<impl IntoResponse>` - A JSON response containing the scan statistics.
pub async fn export_statistics(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<impl IntoResponse> {
    let stats = sqlx::query(
        r#"
        SELECT 
            s.id,
            s.status,
            s.started_at,
            s.finished_at,
            s.total_logical_size,
            s.total_allocated_size,
            s.dir_count,
            s.file_count,
            s.warning_count,
            (SELECT COUNT(*) FROM nodes WHERE scan_id = s.id) as total_nodes,
            (SELECT COUNT(*) FROM files WHERE scan_id = s.id) as total_files,
            (SELECT MAX(depth) FROM nodes WHERE scan_id = s.id) as max_depth,
            (SELECT path FROM nodes WHERE scan_id = s.id ORDER BY allocated_size DESC LIMIT 1) as largest_dir,
            (SELECT path FROM files WHERE scan_id = s.id ORDER BY allocated_size DESC LIMIT 1) as largest_file
        FROM scans s
        WHERE s.id = ?1
        "#,
    )
    .bind(id.to_string())
    .fetch_optional(&state.db)
    .await?;

    if let Some(row) = stats {
        let stats_json = serde_json::json!({
            "scan_id": row.get::<String, _>("id"),
            "status": row.get::<String, _>("status"),
            "started_at": row.get::<Option<String>, _>("started_at"),
            "finished_at": row.get::<Option<String>, _>("finished_at"),
            "total_logical_size": row.get::<Option<i64>, _>("total_logical_size"),
            "total_allocated_size": row.get::<Option<i64>, _>("total_allocated_size"),
            "dir_count": row.get::<Option<i64>, _>("dir_count"),
            "file_count": row.get::<Option<i64>, _>("file_count"),
            "warning_count": row.get::<Option<i64>, _>("warning_count"),
            "total_nodes": row.get::<i64, _>("total_nodes"),
            "total_files": row.get::<i64, _>("total_files"),
            "max_depth": row.get::<Option<i64>, _>("max_depth"),
            "largest_dir": row.get::<Option<String>, _>("largest_dir"),
            "largest_file": row.get::<Option<String>, _>("largest_file"),
            "exported_at": chrono::Utc::now().to_rfc3339(),
        });

        Ok(Json(stats_json))
    } else {
        Err(AppError::NotFound("Scan not found".to_string()))
    }
}
