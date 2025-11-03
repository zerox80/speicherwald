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

#[derive(Debug, Deserialize)]
pub struct ExportQuery {
    pub format: String,        // csv or json
    pub scope: Option<String>, // nodes, files, or all
    pub limit: Option<i64>,
}

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

#[derive(Debug, Serialize)]
pub struct ExportData {
    pub scan_id: String,
    pub exported_at: String,
    pub format: String,
    pub nodes: Option<Vec<NodeExport>>,
    pub files: Option<Vec<FileExport>>,
}

#[derive(Debug, Serialize)]
pub struct NodeExport {
    pub path: String,
    pub parent_path: Option<String>,
    pub depth: i64,
    pub is_dir: bool,
    pub logical_size: i64,
    pub allocated_size: i64,
    pub file_count: i64,
    pub dir_count: i64,
}

#[derive(Debug, Serialize)]
pub struct FileExport {
    pub path: String,
    pub parent_path: Option<String>,
    pub logical_size: i64,
    pub allocated_size: i64,
}

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

async fn export_csv(state: AppState, scan_id: Uuid, scope: &str, limit: i64) -> AppResult<impl IntoResponse> {
    use axum::http::HeaderValue;

    const NODE_HEADER: &str =
        "Type,Path,Parent Path,Depth,Is Directory,Logical Size,Allocated Size,File Count,Dir Count\n";
    const FILE_HEADER: &str = "Type,Path,Parent Path,Logical Size,Allocated Size\n";

    let include_nodes = scope == "all" || scope == "nodes";
    let include_files = scope == "all" || scope == "files";

    let mut csv_content = String::new();

    if include_nodes {
        csv_content.push_str(NODE_HEADER);
        let nodes = fetch_nodes_export(&state, scan_id, limit).await?;

        for node in nodes {
            csv_content.push_str(&format_node_csv(&node));
        }
    }

    if include_nodes && include_files && !csv_content.is_empty() {
        csv_content.push('\n');
    }

    if include_files {
        csv_content.push_str(FILE_HEADER);
        let files = fetch_files_export(&state, scan_id, limit).await?;

        for file in files {
            csv_content.push_str(&format!(
                "File,\"{}\",\"{}\",{},{}\n",
                escape_csv(&file.path),
                escape_csv(file.parent_path.as_deref().unwrap_or("")),
                file.logical_size,
                file.allocated_size,
            ));
        }
    }

    let mut response = csv_content.into_response();
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/csv; charset=utf-8"));
    let filename = format!("attachment; filename=\"scan_{}.csv\"", scan_id);
    if let Ok(header_val) = HeaderValue::from_str(&filename) {
        response.headers_mut().insert(header::CONTENT_DISPOSITION, header_val);
    }
    Ok(response)
}

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
        export_data.nodes = Some(fetch_nodes_export(&state, scan_id, limit).await?);
    }

    if scope == "all" || scope == "files" {
        export_data.files = Some(fetch_files_export(&state, scan_id, limit).await?);
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

fn escape_csv(s: &str) -> String {
    // FIX Bug #55 - Proper CSV escaping: double quotes become two double quotes
    // More efficient: do all replacements in one pass
    s.chars()
        .flat_map(|c| match c {
            '"' => vec!['"', '"'],            // Escape double quote as ""
            '\n' | '\r' => vec![' '],         // Replace newlines with space
            c if c.is_control() => vec![' '], // Replace other control chars
            c => vec![c],
        })
        .collect()
}

const EXPORT_CHUNK_SIZE: i64 = 800;

async fn fetch_nodes_export(state: &AppState, scan_id: Uuid, limit: i64) -> Result<Vec<NodeExport>, sqlx::Error> {
    let mut results = Vec::new();
    let mut offset: i64 = 0;
    let sid = scan_id.to_string();

    while offset < limit {
        let batch = (limit - offset).min(EXPORT_CHUNK_SIZE);
        let rows = sqlx::query(
            "SELECT path, parent_path, depth, is_dir, logical_size, allocated_size, file_count, dir_count \
             FROM nodes WHERE scan_id = ?1 ORDER BY allocated_size DESC LIMIT ?2 OFFSET ?3",
        )
        .bind(&sid)
        .bind(batch)
        .bind(offset)
        .fetch_all(&state.db)
        .await?;

        if rows.is_empty() {
            break;
        }

        for row in rows.iter() {
            results.push(NodeExport {
                path: row.get("path"),
                parent_path: row.get("parent_path"),
                depth: row.get("depth"),
                is_dir: row.get::<i64, _>("is_dir") != 0,
                logical_size: row.get("logical_size"),
                allocated_size: row.get("allocated_size"),
                file_count: row.get("file_count"),
                dir_count: row.get("dir_count"),
            });
        }

        let fetched = rows.len() as i64;
        offset += fetched;
        if fetched < batch {
            break;
        }
    }

    Ok(results)
}

async fn fetch_files_export(state: &AppState, scan_id: Uuid, limit: i64) -> Result<Vec<FileExport>, sqlx::Error> {
    let mut results = Vec::new();
    let mut offset: i64 = 0;
    let sid = scan_id.to_string();

    while offset < limit {
        let batch = (limit - offset).min(EXPORT_CHUNK_SIZE);
        let rows = sqlx::query(
            "SELECT path, parent_path, logical_size, allocated_size \
             FROM files WHERE scan_id = ?1 ORDER BY allocated_size DESC LIMIT ?2 OFFSET ?3",
        )
        .bind(&sid)
        .bind(batch)
        .bind(offset)
        .fetch_all(&state.db)
        .await?;

        if rows.is_empty() {
            break;
        }

        for row in rows.iter() {
            results.push(FileExport {
                path: row.get("path"),
                parent_path: row.get("parent_path"),
                logical_size: row.get("logical_size"),
                allocated_size: row.get("allocated_size"),
            });
        }

        let fetched = rows.len() as i64;
        offset += fetched;
        if fetched < batch {
            break;
        }
    }

    Ok(results)
}

// Statistics Export
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
