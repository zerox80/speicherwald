use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ScanSummary {
    pub id: String,
    pub status: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub total_logical_size: i64,
    pub total_allocated_size: i64,
    pub dir_count: i64,
    pub file_count: i64,
    pub warning_count: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct DrivesResponse { pub items: Vec<DriveInfo> }

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct DriveInfo {
    pub path: String,
    pub drive_type: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct CreateScanResp { pub id: String, pub status: String, pub started_at: String }

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct NodeDto {
    pub path: String,
    pub parent_path: Option<String>,
    pub depth: i64,
    pub is_dir: bool,
    pub logical_size: i64,
    pub allocated_size: i64,
    pub file_count: i64,
    pub dir_count: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TopItem {
    Dir { path: String, parent_path: Option<String>, depth: i64, logical_size: i64, allocated_size: i64, file_count: i64, dir_count: i64 },
    File { path: String, parent_path: Option<String>, logical_size: i64, allocated_size: i64 },
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ListItem {
    Dir { name: String, path: String, parent_path: Option<String>, depth: i64, logical_size: i64, allocated_size: i64, file_count: i64, dir_count: i64, mtime: Option<i64> },
    File { name: String, path: String, parent_path: Option<String>, logical_size: i64, allocated_size: i64, mtime: Option<i64> },
}

// Search results
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct SearchResult {
    pub items: Vec<SearchItem>,
    pub total_count: i64,
    pub query: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
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

// SSE events from backend
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScanEvent {
    Started { root_paths: Vec<String> },
    Progress { current_path: String, dirs_scanned: u64, files_scanned: u64, logical_size: u64, allocated_size: u64 },
    Warning { path: String, code: String, message: String },
    Done { total_dirs: u64, total_files: u64, total_logical_size: u64, total_allocated_size: u64 },
    Cancelled,
    Failed { message: String },
}
