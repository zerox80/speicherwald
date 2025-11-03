use axum::{
    extract::{connect_info::ConnectInfo, Path, Query, State},
    http::HeaderMap,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Row};
use uuid::Uuid;
use std::net::SocketAddr;

use crate::{
    error::{AppError, AppResult},
    middleware::ip::extract_ip_from_headers,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    #[serde(default)]
    pub min_size: Option<i64>,
    #[serde(default)]
    pub max_size: Option<i64>,
    #[serde(default)]
    #[serde(alias = "type")]
    pub file_type: Option<String>, // e.g., "txt", "pdf", "jpg" (also accepts query param 'type')
    #[serde(default)]
    pub include_files: Option<bool>,
    #[serde(default)]
    pub include_dirs: Option<bool>,
}

fn default_limit() -> i64 {
    100
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub items: Vec<SearchItem>,
    pub total_count: i64,
    pub query: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum SearchItem {
    Dir {
        path: String,
        name: String,
        allocated_size: i64,
        logical_size: i64,
        file_count: i64,
        dir_count: i64,
        depth: i64,
    },
    File {
        path: String,
        name: String,
        allocated_size: i64,
        logical_size: i64,
        extension: Option<String>,
    },
}

const LIKE_ESCAPE: char = '!';

fn escape_like_pattern(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '%' | '_' | LIKE_ESCAPE) {
            out.push(LIKE_ESCAPE);
        }
        out.push(ch);
    }
    out
}

fn sanitize_search_term(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidInput("Search query cannot be empty".to_string()));
    }
    if trimmed.chars().count() > 500 {
        return Err(AppError::InvalidInput("Search query too long".to_string()));
    }
    let sanitized: String = trimmed.chars().filter(|ch| !ch.is_control() || ch.is_whitespace()).collect();
    if sanitized.trim().is_empty() {
        return Err(AppError::InvalidInput("Search query contains only special characters".to_string()));
    }
    Ok(sanitized)
}

pub async fn search_scan(
    State(state): State<AppState>,
    Path(scan_id): Path<Uuid>,
    maybe_remote: Option<ConnectInfo<SocketAddr>>,
    headers: HeaderMap,
    Query(query): Query<SearchQuery>,
) -> AppResult<impl IntoResponse> {
    // Per-endpoint rate limit: "/scans/:id/search"
    let fallback_ip = maybe_remote.map(|ConnectInfo(addr)| addr.ip());
    let ip = extract_ip_from_headers(&headers, fallback_ip);
    if let Err((status, body)) = state.rate_limiter.check_endpoint_limit("/scans/:id/search", ip).await {
        return Ok((status, body).into_response());
    }
    // Sanitize search query to prevent LIKE injection while preserving legitimate characters
    let sanitized_query = sanitize_search_term(&query.query)?;
    let search_pattern = format!("%{}%", escape_like_pattern(&sanitized_query));
    let include_files = query.include_files.unwrap_or(true);
    let include_dirs = query.include_dirs.unwrap_or(true);

    if !include_files && !include_dirs {
        return Err(AppError::InvalidInput("Must include at least files or directories".to_string()));
    }

    // We'll execute a single UNION query with global ORDER+LIMIT+OFFSET.
    // Clamp to keep resource usage bounded even with large offsets. (FIX Bug #19)
    let limit_clamped = query.limit.clamp(1, 1000);
    let offset_clamped = query.offset.max(0).min(10_000); // Prevent excessive offset and performance issues

    // Validate that offset + limit doesn't overflow
    if let Some(_overflow) = offset_clamped.checked_add(limit_clamped) {
        // OK
    } else {
        return Err(AppError::InvalidInput("Offset and limit combination would overflow".to_string()));
    }

    // Build COUNT queries (parameterized)
    let total_dirs = if include_dirs {
        let mut qb = QueryBuilder::new("SELECT COUNT(*) AS cnt FROM nodes WHERE scan_id = ");
        qb.push_bind(scan_id.to_string())
            .push(" AND is_dir = 1 AND path LIKE ")
            .push_bind(&search_pattern)
            .push(" ESCAPE '!'");
        if let Some(min_size) = query.min_size {
            qb.push(" AND allocated_size >= ").push_bind(min_size);
        }
        if let Some(max_size) = query.max_size {
            qb.push(" AND allocated_size <= ").push_bind(max_size);
        }
        let row = qb.build().fetch_one(&state.db).await?;
        row.try_get::<i64, _>("cnt")?
    } else {
        0
    };

    let total_files = if include_files {
        let mut qb = QueryBuilder::new("SELECT COUNT(*) AS cnt FROM files WHERE scan_id = ");
        qb.push_bind(scan_id.to_string())
            .push(" AND path LIKE ")
            .push_bind(&search_pattern)
            .push(" ESCAPE '!'");
        if let Some(min_size) = query.min_size {
            qb.push(" AND allocated_size >= ").push_bind(min_size);
        }
        if let Some(max_size) = query.max_size {
            qb.push(" AND allocated_size <= ").push_bind(max_size);
        }
        if let Some(file_type) = &query.file_type {
            // Sanitize file_type to prevent injection (for COUNT query) (FIX Bug #53)
            let sanitized = file_type
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                .take(20)
                .collect::<String>();
            if !sanitized.is_empty() {
                // Use parameterized pattern to prevent any LIKE injection
                let ext_pattern = format!(".{}", sanitized.to_lowercase());
                qb.push(" AND LOWER(path) LIKE '%' || ").push_bind(ext_pattern).push(" ESCAPE '!'");
            }
        }
        let row = qb.build().fetch_one(&state.db).await?;
        row.try_get::<i64, _>("cnt")?
    } else {
        0
    };

    let total_count = total_dirs + total_files;

    // Build UNION query via QueryBuilder
    let mut qb = QueryBuilder::new(
        "SELECT kind, path, logical_size, allocated_size, file_count, dir_count, depth FROM (",
    );
    let mut first = true;
    if include_dirs {
        qb.push("SELECT 'dir' AS kind, path, logical_size, allocated_size, file_count, dir_count, depth FROM nodes WHERE scan_id = ")
            .push_bind(scan_id.to_string())
            .push(" AND is_dir = 1 AND path LIKE ")
            .push_bind(&search_pattern)
            .push(" ESCAPE '!'");
        if let Some(min_size) = query.min_size {
            qb.push(" AND allocated_size >= ").push_bind(min_size);
        }
        if let Some(max_size) = query.max_size {
            qb.push(" AND allocated_size <= ").push_bind(max_size);
        }
        first = false;
    }
    if include_files {
        if !first {
            qb.push(" UNION ALL ");
        }
        qb.push("SELECT 'file' AS kind, path, logical_size, allocated_size, NULL AS file_count, NULL AS dir_count, NULL AS depth FROM files WHERE scan_id = ")
            .push_bind(scan_id.to_string())
            .push(" AND path LIKE ")
            .push_bind(&search_pattern)
            .push(" ESCAPE '!'");
        if let Some(min_size) = query.min_size {
            qb.push(" AND allocated_size >= ").push_bind(min_size);
        }
        if let Some(max_size) = query.max_size {
            qb.push(" AND allocated_size <= ").push_bind(max_size);
        }
        if let Some(file_type) = &query.file_type {
            // Sanitize file_type to prevent injection (for UNION query) (FIX Bug #53)
            let sanitized = file_type
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                .take(20)
                .collect::<String>();
            if !sanitized.is_empty() {
                // Use parameterized pattern to prevent any LIKE injection
                let ext_pattern = format!(".{}", sanitized.to_lowercase());
                qb.push(" AND LOWER(path) LIKE '%' || ").push_bind(ext_pattern).push(" ESCAPE '!'");
            }
        }
    }
    qb.push(") ORDER BY allocated_size DESC LIMIT ")
        .push_bind(limit_clamped)
        .push(" OFFSET ")
        .push_bind(offset_clamped);

    let rows = qb.build().fetch_all(&state.db).await?;
    let mut items: Vec<SearchItem> = Vec::with_capacity(rows.len());
    for row in rows {
        let kind: String = row.try_get("kind")?;
        let path: String = row.try_get("path")?;
        // FIX Bug #32 - Better path name extraction
        let name =
            std::path::Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or(&path).to_string();
        if kind == "dir" {
            items.push(SearchItem::Dir {
                path,
                name,
                allocated_size: row.try_get("allocated_size")?,
                logical_size: row.try_get("logical_size")?,
                file_count: row.try_get("file_count")?,
                dir_count: row.try_get("dir_count")?,
                depth: row.try_get("depth")?,
            });
        } else {
            // Extract file extension properly with better validation (FIX Bug #4)
            let extension = path.rsplit_once('.').and_then(|(_, ext)| {
                // Validate extension:
                // 1. Not empty
                // 2. No path separators in extension
                // 3. Reasonable length
                // 4. Only alphanumeric characters
                if !ext.is_empty()
                    && !ext.contains(['\\', '/'])
                    && ext.len() <= 15
                    && ext.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
                {
                    Some(ext.to_lowercase())
                } else {
                    None
                }
            });
            items.push(SearchItem::File {
                path,
                name,
                allocated_size: row.try_get("allocated_size")?,
                logical_size: row.try_get("logical_size")?,
                extension,
            });
        }
    }

    Ok(Json(SearchResult { items, total_count, query: query.query }).into_response())
}
