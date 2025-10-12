use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanOptions {
    pub follow_symlinks: bool,
    pub include_hidden: bool,
    pub measure_logical: bool,
    pub measure_allocated: bool,
    pub excludes: Vec<String>,
    pub max_depth: Option<u32>,
    pub concurrency: Option<usize>,
}

// DTOs for tree and top endpoints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDto {
    pub path: String,
    pub parent_path: Option<String>,
    pub depth: i64,
    pub is_dir: bool,
    pub logical_size: i64,
    pub allocated_size: i64,
    pub file_count: i64,
    pub dir_count: i64,
    pub mtime: Option<i64>,
    pub atime: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDto {
    pub path: String,
    pub parent_path: Option<String>,
    pub logical_size: i64,
    pub allocated_size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TopItem {
    Dir {
        path: String,
        parent_path: Option<String>,
        depth: i64,
        logical_size: i64,
        allocated_size: i64,
        file_count: i64,
        dir_count: i64,
        mtime: Option<i64>,
        atime: Option<i64>,
    },
    File {
        path: String,
        parent_path: Option<String>,
        logical_size: i64,
        allocated_size: i64,
        mtime: Option<i64>,
        atime: Option<i64>,
    },
}

// Items for file-manager style listing (children of a directory)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ListItem {
    Dir {
        name: String,
        path: String,
        parent_path: Option<String>,
        depth: i64,
        logical_size: i64,
        allocated_size: i64,
        file_count: i64,
        dir_count: i64,
        mtime: Option<i64>,
        atime: Option<i64>,
    },
    File {
        name: String,
        path: String,
        parent_path: Option<String>,
        logical_size: i64,
        allocated_size: i64,
        mtime: Option<i64>,
        atime: Option<i64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveInfo {
    pub path: String,
    pub drive_type: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            follow_symlinks: false,
            include_hidden: true,
            measure_logical: true,
            measure_allocated: true,
            excludes: vec![],
            max_depth: None,
            concurrency: Some(num_cpus::get().max(2usize) / 2usize + 1usize),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateScanRequest {
    pub root_paths: Vec<String>,
    pub follow_symlinks: Option<bool>,
    pub include_hidden: Option<bool>,
    pub measure_logical: Option<bool>,
    pub measure_allocated: Option<bool>,
    pub excludes: Option<Vec<String>>,
    pub max_depth: Option<u32>,
    pub concurrency: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateScanResponse {
    pub id: Uuid,
    pub status: String,
    pub started_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanSummary {
    pub id: Uuid,
    pub status: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub total_logical_size: i64,
    pub total_allocated_size: i64,
    pub dir_count: i64,
    pub file_count: i64,
    pub warning_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScanEvent {
    Started {
        root_paths: Vec<String>,
    },
    Progress {
        current_path: String,
        dirs_scanned: u64,
        files_scanned: u64,
        logical_size: u64,
        allocated_size: u64,
    },
    Warning {
        path: String,
        code: String,
        message: String,
    },
    Done {
        total_dirs: u64,
        total_files: u64,
        total_logical_size: u64,
        total_allocated_size: u64,
    },
    Cancelled,
    Failed {
        message: String,
    },
}
