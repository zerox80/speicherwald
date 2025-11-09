//! API client for communicating with the SpeicherWald backend.
//!
//! This module provides functions for making HTTP requests to the backend API,
//! handling both regular REST endpoints and Server-Sent Events (SSE) for
//! real-time updates.
//!
//! ## Features
//!
//! - **Type-safe API calls**: All functions return strongly-typed results
//! - **Error handling**: Network errors are converted to user-friendly messages
//! - **SSE support**: Real-time event streaming for scan progress
//! - **Query parameter handling**: Automatic URL encoding for complex queries

use serde::Serialize;
use serde_json::Value as JsonValue;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{EventSource, MessageEvent};

use crate::types::*;

/// Base URL for API requests.
///
/// Using an empty string leverages same-origin relative URLs,
/// which works well when the frontend is served by the same backend.
pub const BASE: &str = "";

/// Constructs a full URL from a path segment.
///
/// # Arguments
///
/// * `path` - The API endpoint path
///
/// # Returns
///
/// The complete URL for the API request
fn url(path: &str) -> String { format!("{}{}", BASE, path) }

/// Retrieves a list of all scans from the backend.
///
/// # Returns
///
/// * `Result<Vec<ScanSummary>, String>` - A list of scan summaries or an error message
pub async fn list_scans() -> Result<Vec<ScanSummary>, String> {
    let resp = reqwasm::http::Request::get(&url("/scans")).send().await.map_err(map_net)?;
    if !resp.ok() { return Err(resp.text().await.unwrap_or_else(|_| "HTTP Fehler".into())); }
    resp.json().await.map_err(map_net)
}

/// Retrieves information about available storage drives.
///
/// # Returns
///
/// * `Result<DrivesResponse, String>` - Drive information or an error message
pub async fn list_drives() -> Result<DrivesResponse, String> {
    let resp = reqwasm::http::Request::get(&url("/drives")).send().await.map_err(map_net)?;
    if !resp.ok() { return Err(resp.text().await.unwrap_or_else(|_| "HTTP Fehler".into())); }
    resp.json().await.map_err(map_net)
}

/// Checks if the backend server is healthy and responding.
///
/// # Returns
///
/// * `Result<bool, String>` - True if healthy, false if not, or an error message
pub async fn healthz() -> Result<bool, String> {
    let resp = reqwasm::http::Request::get(&url("/healthz")).send().await.map_err(map_net)?;
    Ok(resp.ok())
}

/// Request to create a new file system scan.
///
/// Contains all the configuration options for starting a scan,
/// including paths to scan and various behavioral settings.
#[derive(Debug, Clone, Serialize)]
pub struct CreateScanReq {
    /// List of root paths to scan
    pub root_paths: Vec<String>,
    /// Whether to follow symbolic links
    pub follow_symlinks: Option<bool>,
    /// Whether to include hidden files and directories
    pub include_hidden: Option<bool>,
    /// Whether to measure logical file sizes
    pub measure_logical: Option<bool>,
    /// Whether to measure allocated (disk) sizes
    pub measure_allocated: Option<bool>,
    /// Patterns to exclude from scanning
    pub excludes: Option<Vec<String>>,
    /// Maximum scan depth
    pub max_depth: Option<u32>,
    /// Concurrency level for scanning
    pub concurrency: Option<usize>,
}

/// Creates a new file system scan.
///
/// Starts a scan operation on the specified paths with the given configuration.
/// The scan runs asynchronously and its progress can be monitored via SSE events.
///
/// # Arguments
///
/// * `req` - A reference to a `CreateScanReq` containing the scan configuration
///   including root paths, exclusion patterns, and various scanning options
///
/// # Returns
///
/// * `Result<CreateScanResp, String>` - The scan creation response containing
///   the scan ID and initial status, or an error message if creation failed
///
/// # Notes
///
/// - The scan starts immediately after creation
/// - Use the returned scan ID to monitor progress via SSE
/// - Multiple root paths can be specified in a single scan request
pub async fn create_scan(req: &CreateScanReq) -> Result<CreateScanResp, String> {
    let resp = reqwasm::http::Request::post(&url("/scans"))
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(req).unwrap())
        .send().await.map_err(map_net)?;
    if !resp.ok() { return Err(resp.text().await.unwrap_or_else(|_| "HTTP Fehler".into())); }
    resp.json().await.map_err(map_net)
}

/// Retrieves detailed information about a specific scan.
///
/// Fetches the current status and metadata of a scan, including progress information,
/// scan statistics, and current state.
///
/// # Arguments
///
/// * `id` - The unique identifier of the scan to retrieve
///
/// # Returns
///
/// * `Result<ScanSummary, String>` - The scan summary containing detailed information
///   about the scan, or an error message if the scan doesn't exist or the request failed
///
/// # Notes
///
/// - Can be used to check the current progress of an ongoing scan
/// - Returns complete metadata for completed scans
/// - Invalid scan IDs will result in an error
pub async fn get_scan(id: &str) -> Result<ScanSummary, String> {
    let resp = reqwasm::http::Request::get(&url(&format!("/scans/{}", id))).send().await.map_err(map_net)?;
    if !resp.ok() { return Err(resp.text().await.unwrap_or_else(|_| "HTTP Fehler".into())); }
    resp.json().await.map_err(map_net)
}

/// Cancels an ongoing scan and optionally purges its data.
///
/// Stops a running scan and optionally removes all associated data from storage.
/// This operation is irreversible when purge is set to true.
///
/// # Arguments
///
/// * `id` - The unique identifier of the scan to cancel
/// * `purge` - If true, removes all scan data; if false, just stops the scan
///
/// # Returns
///
/// * `Result<(), String>` - Success indicator or an error message if the operation failed
///
/// # Notes
///
/// - When `purge` is true, all scan data is permanently deleted
/// - When `purge` is false, the scan is stopped but data remains accessible
/// - This operation cannot be undone
/// - Already completed scans can still be purged
pub async fn cancel_scan(id: &str, purge: bool) -> Result<(), String> {
    let resp = reqwasm::http::Request::delete(&url(&format!("/scans/{}?purge={}", id, if purge {"true"} else {"false"})))
        .send().await.map_err(map_net)?;
    if !resp.ok() { return Err(resp.text().await.unwrap_or_else(|_| "HTTP Fehler".into())); }
    Ok(())
}

/// Query parameters for retrieving tree data from a scan.
///
/// Used to filter and sort the directory tree when fetching hierarchical data.
/// All parameters are optional to provide flexible querying capabilities.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TreeQuery {
    /// Path to start the tree traversal from (root if not specified)
    pub path: Option<String>,
    /// Maximum depth to traverse (unlimited if not specified)
    pub depth: Option<i64>,
    /// Sort order for the results (e.g., "name", "size", "modified")
    pub sort: Option<String>,
    /// Maximum number of nodes to return (unlimited if not specified)
    pub limit: Option<i64>
}

/// Retrieves hierarchical tree data from a scan.
///
/// Fetches directory tree information with optional filtering by path, depth,
/// sorting, and result limiting. Returns a hierarchical view of the file system.
///
/// # Arguments
///
/// * `id` - The unique identifier of the scan to query
/// * `q` - A `TreeQuery` containing filtering and sorting parameters
///
/// # Returns
///
/// * `Result<Vec<NodeDto>, String>` - A vector of directory tree nodes or an error message
///
/// # Notes
///
/// - Returns hierarchical data representing the directory structure
/// - Use the `path` parameter to start from a specific directory
/// - Use `depth` to limit how deep the tree traversal goes
/// - Results can be sorted by various criteria using the `sort` parameter
/// - The `limit` parameter helps control response size for large directories
pub async fn get_tree(id: &str, q: &TreeQuery) -> Result<Vec<NodeDto>, String> {
    let mut qs = vec![];
    if let Some(p) = &q.path { qs.push(format!("path={}", urlencoding::encode(p))); }
    if let Some(d) = q.depth { qs.push(format!("depth={}", d)); }
    if let Some(s) = &q.sort { qs.push(format!("sort={}", urlencoding::encode(s))); }
    if let Some(l) = q.limit { qs.push(format!("limit={}", l)); }
    let qstr = if qs.is_empty() { String::new() } else { format!("?{}", qs.join("&")) };
    let resp = reqwasm::http::Request::get(&url(&format!("/scans/{}/tree{}", id, qstr))).send().await.map_err(map_net)?;
    if !resp.ok() { return Err(resp.text().await.unwrap_or_else(|_| "HTTP Fehler".into())); }
    resp.json().await.map_err(map_net)
}

/// Query parameters for retrieving top items from a scan.
///
/// Used to get the largest items within a specific scope, useful for
/// identifying space-consuming files and directories.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TopQuery {
    /// Scope to limit the search (path or directory, root if not specified)
    pub scope: Option<String>,
    /// Maximum number of top items to return
    pub limit: Option<i64>
}

/// Retrieves the largest items from a scan.
///
/// Fetches the top space-consuming files and directories within a specified scope,
/// sorted by size in descending order. Useful for storage analysis and cleanup.
///
/// # Arguments
///
/// * `id` - The unique identifier of the scan to query
/// * `q` - A `TopQuery` containing scope and limit parameters
///
/// # Returns
///
/// * `Result<Vec<TopItem>, String>` - A vector of top items sorted by size or an error message
///
/// # Notes
///
/// - Results are sorted by size in descending order (largest first)
/// - Use the `scope` parameter to limit analysis to a specific directory
/// - The `limit` parameter controls how many top items to return
/// - Useful for identifying which files and directories consume the most space
pub async fn get_top(id: &str, q: &TopQuery) -> Result<Vec<TopItem>, String> {
    let mut qs = vec![];
    if let Some(s) = &q.scope { qs.push(format!("scope={}", urlencoding::encode(s))); }
    if let Some(l) = q.limit { qs.push(format!("limit={}", l)); }
    let qstr = if qs.is_empty() { String::new() } else { format!("?{}", qs.join("&")) };
    let resp = reqwasm::http::Request::get(&url(&format!("/scans/{}/top{}", id, qstr))).send().await.map_err(map_net)?;
    if !resp.ok() { return Err(resp.text().await.unwrap_or_else(|_| "HTTP Fehler".into())); }
    resp.json().await.map_err(map_net)
}

/// Query parameters for retrieving a paginated list of items from a scan.
///
/// Used to get flat listings of files and directories with comprehensive
/// sorting, ordering, and pagination capabilities.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ListQuery {
    /// Path to list items from (root if not specified)
    pub path: Option<String>,
    /// Field to sort by (e.g., "name", "size", "modified")
    pub sort: Option<String>,
    /// Sort order ("asc" or "desc", ascending if not specified)
    pub order: Option<String>,
    /// Maximum number of items to return per page
    pub limit: Option<i64>,
    /// Number of items to skip for pagination
    pub offset: Option<i64>
}

/// Retrieves a paginated list of items from a scan.
///
/// Fetches a flat listing of files and directories with comprehensive filtering,
/// sorting, and pagination options. Handles rate limiting with proper error messages.
///
/// # Arguments
///
/// * `id` - The unique identifier of the scan to query
/// * `q` - A `ListQuery` containing path, sorting, and pagination parameters
///
/// # Returns
///
/// * `Result<Vec<ListItem>, String>` - A vector of list items or an error message
///
/// # Notes
///
/// - Supports pagination using `limit` and `offset` parameters for large directories
/// - Results can be sorted by various fields using the `sort` parameter
/// - Sort order can be controlled with the `order` parameter ("asc" or "desc")
/// - Use the `path` parameter to list items from a specific directory
/// - Handles HTTP 429 rate limiting with informative error messages
/// - Returns flat listings, not hierarchical tree structures
pub async fn get_list(id: &str, q: &ListQuery) -> Result<Vec<ListItem>, String> {
    let mut qs = vec![];
    if let Some(p) = &q.path { qs.push(format!("path={}", urlencoding::encode(p))); }
    if let Some(s) = &q.sort { qs.push(format!("sort={}", urlencoding::encode(s))); }
    if let Some(o) = &q.order { qs.push(format!("order={}", urlencoding::encode(o))); }
    if let Some(l) = q.limit { qs.push(format!("limit={}", l)); }
    if let Some(o) = q.offset { qs.push(format!("offset={}", o)); }
    let qstr = if qs.is_empty() { String::new() } else { format!("?{}", qs.join("&")) };
    let resp = reqwasm::http::Request::get(&url(&format!("/scans/{}/list{}", id, qstr))).send().await.map_err(map_net)?;
    if !resp.ok() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_else(|_| "HTTP Fehler".into());
        if status == 429 {
            // Try to parse retry_after_seconds from backend JSON
            if let Ok(v) = serde_json::from_str::<JsonValue>(&text) {
                if let Some(sec) = v.get("retry_after_seconds").and_then(|x| x.as_u64()) {
                    return Err(format!("Zu viele Anfragen (429). Bitte nach {} Sekunden erneut versuchen.", sec));
                }
                if let Some(obj) = v.get("error").and_then(|e| e.as_object()) {
                    if let Some(msg) = obj.get("message").and_then(|m| m.as_str()) {
                        return Err(format!("Zu viele Anfragen (429). {}", msg));
                    }
                }
            }
        }
        return Err(text);
    }
    resp.json().await.map_err(map_net)
}

/// Query parameters for searching items within a scan.
///
/// Provides comprehensive search capabilities including text matching,
/// size filtering, file type filtering, and result limiting.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SearchQuery {
    /// Search query string for matching file/directory names
    pub query: String,
    /// Maximum number of results to return
    pub limit: Option<i64>,
    /// Number of results to skip for pagination
    pub offset: Option<i64>,
    /// Minimum file size filter (in bytes)
    pub min_size: Option<i64>,
    /// Maximum file size filter (in bytes)
    pub max_size: Option<i64>,
    /// File type filter (e.g., "image", "video", "document")
    pub file_type: Option<String>,
    /// Whether to include files in results (true if not specified)
    pub include_files: Option<bool>,
    /// Whether to include directories in results (false if not specified)
    pub include_dirs: Option<bool>,
}

/// Searches for items within a scan using various filters.
///
/// Performs a comprehensive search across the scan data using text matching,
/// size filters, file type filters, and other criteria. Returns paginated results.
///
/// # Arguments
///
/// * `id` - The unique identifier of the scan to search
/// * `q` - A `SearchQuery` containing search criteria and filters
///
/// # Returns
///
/// * `Result<SearchResult, String>` - Search results including items and metadata,
///   or an error message if the search failed
///
/// # Notes
///
/// - The search query is matched against file and directory names
/// - Size filters can be used to find files within specific size ranges
/// - File type filtering helps find specific types of content
/// - Use `include_files` and `include_dirs` to control result types
/// - Results can be paginated using `limit` and `offset` parameters
/// - Search is case-insensitive for text matching
pub async fn search_scan(id: &str, q: &SearchQuery) -> Result<SearchResult, String> {
    let mut qs = vec![];
    qs.push(format!("query={}", urlencoding::encode(&q.query)));
    if let Some(l) = q.limit { qs.push(format!("limit={}", l)); }
    if let Some(o) = q.offset { qs.push(format!("offset={}", o)); }
    if let Some(s) = q.min_size { qs.push(format!("min_size={}", s)); }
    if let Some(s) = q.max_size { qs.push(format!("max_size={}", s)); }
    if let Some(t) = &q.file_type { qs.push(format!("file_type={}", urlencoding::encode(t))); }
    if let Some(f) = q.include_files { qs.push(format!("include_files={}", f)); }
    if let Some(d) = q.include_dirs { qs.push(format!("include_dirs={}", d)); }
    let qstr = format!("?{}", qs.join("&"));
    let resp = reqwasm::http::Request::get(&url(&format!("/scans/{}/search{}", id, qstr))).send().await.map_err(map_net)?;
    if !resp.ok() { return Err(resp.text().await.unwrap_or_else(|_| "HTTP Fehler".into())); }
    resp.json().await.map_err(map_net)
}

/// Converts network errors into user-friendly error messages.
///
/// Maps low-level `reqwasm::Error` instances to human-readable German error
/// messages that can be displayed to users in the UI.
///
/// # Arguments
///
/// * `e` - The network error to convert
///
/// # Returns
///
/// A user-friendly error message string describing the network problem
fn map_net(e: reqwasm::Error) -> String { format!("Netzwerkfehler: {}", e) }

/// Moves a path to a new location within the file system.
///
/// Performs a move operation to relocate files or directories to a new location.
/// This operation updates the scan data to reflect the new path structure.
///
/// # Arguments
///
/// * `req` - A `MovePathRequest` containing source path, destination path,
///   and other move operation parameters
///
/// # Returns
///
/// * `Result<MovePathResponse, String>` - The response containing operation results
///   and updated path information, or an error message if the move failed
///
/// # Notes
///
/// - The move operation updates scan data to maintain consistency
/// - Source and destination paths must be within scanned directories
/// - Moving to an existing location may result in overwriting
/// - Large directories may take time to process
/// - The operation is atomic - either fully succeeds or fails completely
pub async fn move_path(req: &MovePathRequest) -> Result<MovePathResponse, String> {
    let resp = reqwasm::http::Request::post(&url("/paths/move"))
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(req).unwrap())
        .send()
        .await
        .map_err(map_net)?;
    if !resp.ok() {
        return Err(resp.text().await.unwrap_or_else(|_| "HTTP Fehler".into()));
    }
    resp.json().await.map_err(map_net)
}

/// Establishes a Server-Sent Events (SSE) connection for real-time scan updates.
///
/// Creates an EventSource connection to receive real-time updates about scan progress,
/// completion, and other events. This enables live monitoring of ongoing scans.
///
/// # Arguments
///
/// * `id` - The unique identifier of the scan to monitor
/// * `on_message` - A callback function that will be invoked for each `ScanEvent` received
///
/// # Returns
///
/// * `Result<EventSource, String>` - The EventSource instance for maintaining the connection,
///   or an error message if the connection failed
///
/// # Type Parameters
///
/// * `F` - A closure type that implements `FnMut(ScanEvent)` and has a `'static` lifetime
///
/// # Notes
///
/// - The returned EventSource must be kept alive to maintain the connection
/// - The callback is invoked for each scan event (progress updates, completion, etc.)
/// - Connection errors will be logged but won't automatically close the EventSource
/// - The closure is intentionally leaked to keep it alive as long as the EventSource
/// - Use EventSource.close() to clean up the connection when done
/// - Events are automatically deserialized from JSON into ScanEvent structs
pub fn sse_attach<F>(id: &str, mut on_message: F) -> Result<EventSource, String>
where F: 'static + FnMut(ScanEvent) {
    let es = EventSource::new(&url(&format!("/scans/{}/events", id))).map_err(|e| format!("SSE Fehler: {:?}", e))?;
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
        if let Ok(me) = ev.dyn_into::<MessageEvent>() {
            if let Some(text) = me.data().as_string() {
                if let Ok(ev) = serde_json::from_str::<ScanEvent>(&text) {
                    on_message(ev);
                }
            }
        }
    });
    es.set_onmessage(Some(closure.as_ref().unchecked_ref()));
    // Leak the closure to keep it as long as the EventSource lives (we close ES on drop by the owner)
    closure.forget();
    Ok(es)
}
