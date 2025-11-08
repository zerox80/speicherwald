use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Options for configuring a scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanOptions {
    /// Whether to follow symbolic links.
    pub follow_symlinks: bool,
    /// Whether to include hidden files and directories.
    pub include_hidden: bool,
    /// Whether to measure logical file size.
    pub measure_logical: bool,
    /// Whether to measure allocated disk space.
    pub measure_allocated: bool,
    /// A list of glob patterns to exclude from the scan.
    pub excludes: Vec<String>,
    /// The maximum depth of the scan.
    pub max_depth: Option<u32>,
    /// The number of concurrent scanner threads.
    pub concurrency: Option<usize>,
}

/// A data transfer object for a node (directory) in the scanned tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDto {
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
    /// The modification time of the node.
    pub mtime: Option<i64>,
    /// The access time of the node.
    pub atime: Option<i64>,
}

/// A data transfer object for a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDto {
    /// The path of the file.
    pub path: String,
    /// The parent path of the file.
    pub parent_path: Option<String>,
    /// The logical size of the file in bytes.
    pub logical_size: i64,
    /// The allocated size of the file in bytes.
    pub allocated_size: i64,
}

/// An item in the "top" list, which can be either a file or a directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TopItem {
    /// A directory item.
    Dir {
        /// The path of the directory.
        path: String,
        /// The parent path of the directory.
        parent_path: Option<String>,
        /// The depth of the directory in the directory tree.
        depth: i64,
        /// The logical size of the directory in bytes.
        logical_size: i64,
        /// The allocated size of the directory in bytes.
        allocated_size: i64,
        /// The number of files in the directory.
        file_count: i64,
        /// The number of subdirectories in the directory.
        dir_count: i64,
        /// The modification time of the directory.
        mtime: Option<i64>,
        /// The access time of the directory.
        atime: Option<i64>,
    },
    /// A file item.
    File {
        /// The path of the file.
        path: String,
        /// The parent path of the file.
        parent_path: Option<String>,
        /// The logical size of the file in bytes.
        logical_size: i64,
        /// The allocated size of the file in bytes.
        allocated_size: i64,
        /// The modification time of the file.
        mtime: Option<i64>,
        /// The access time of the file.
        atime: Option<i64>,
    },
}

/// An item in a directory listing, which can be either a file or a directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ListItem {
    /// A directory item.
    Dir {
        /// The name of the directory.
        name: String,
        /// The path of the directory.
        path: String,
        /// The parent path of the directory.
        parent_path: Option<String>,
        /// The depth of the directory in the directory tree.
        depth: i64,
        /// The logical size of the directory in bytes.
        logical_size: i64,
        /// The allocated size of the directory in bytes.
        allocated_size: i64,
        /// The number of files in the directory.
        file_count: i64,
        /// The number of subdirectories in the directory.
        dir_count: i64,
        /// The modification time of the directory.
        mtime: Option<i64>,
        /// The access time of the directory.
        atime: Option<i64>,
    },
    /// A file item.
    File {
        /// The name of the file.
        name: String,
        /// The path of the file.
        path: String,
        /// The parent path of the file.
        parent_path: Option<String>,
        /// The logical size of the file in bytes.
        logical_size: i64,
        /// The allocated size of the file in bytes.
        allocated_size: i64,
        /// The modification time of the file.
        mtime: Option<i64>,
        /// The access time of the file.
        atime: Option<i64>,
    },
}

/// Information about a drive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveInfo {
    /// The path of the drive (e.g., "C:\\").
    pub path: String,
    /// The type of the drive (e.g., "fixed", "network").
    pub drive_type: String,
    /// The total size of the drive in bytes.
    pub total_bytes: u64,
    /// The amount of free space on the drive in bytes.
    pub free_bytes: u64,
}

/// A request to move or copy a file or directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovePathRequest {
    /// The source path.
    pub source: String,
    /// The destination path.
    pub destination: String,
    /// Whether to remove the source after the operation.
    #[serde(default)]
    pub remove_source: bool,
    /// Whether to overwrite the destination if it already exists.
    #[serde(default)]
    pub overwrite: bool,
}

/// The response from a move path operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovePathResponse {
    /// The status of the operation.
    pub status: String,
    /// The source path.
    pub source: String,
    /// The destination path.
    pub destination: String,
    /// The total number of bytes to transfer.
    pub bytes_to_transfer: u64,
    /// The number of bytes that were successfully moved or copied.
    pub bytes_moved: u64,
    /// The number of bytes freed by the operation.
    pub freed_bytes: u64,
    /// The duration of the operation in milliseconds.
    pub duration_ms: u128,
    /// The start time of the operation.
    pub started_at: String,
    /// The end time of the operation.
    pub finished_at: String,
    /// Any warnings that occurred during the operation.
    pub warnings: Vec<String>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        // Calculate concurrency: use half the CPU cores, minimum 2, maximum 16
        let cpu_count = num_cpus::get();
        let default_concurrency = (cpu_count / 2).max(2).min(16);

        Self {
            follow_symlinks: false,
            include_hidden: true,
            measure_logical: true,
            measure_allocated: true,
            excludes: vec![],
            max_depth: None,
            concurrency: Some(default_concurrency),
        }
    }
}

/// A request to create a new scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateScanRequest {
    /// The root paths to scan.
    pub root_paths: Vec<String>,
    /// Whether to follow symbolic links.
    pub follow_symlinks: Option<bool>,
    /// Whether to include hidden files and directories.
    pub include_hidden: Option<bool>,
    /// Whether to measure logical file size.
    pub measure_logical: Option<bool>,
    /// Whether to measure allocated disk space.
    pub measure_allocated: Option<bool>,
    /// A list of glob patterns to exclude from the scan.
    pub excludes: Option<Vec<String>>,
    /// The maximum depth of the scan.
    pub max_depth: Option<u32>,
    /// The number of concurrent scanner threads.
    pub concurrency: Option<usize>,
}

/// The response from a create scan request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateScanResponse {
    /// The ID of the new scan.
    pub id: Uuid,
    /// The status of the new scan.
    pub status: String,
    /// The start time of the new scan.
    pub started_at: String,
}

/// A summary of a scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanSummary {
    /// The ID of the scan.
    pub id: Uuid,
    /// The status of the scan.
    pub status: String,
    /// The start time of the scan.
    pub started_at: Option<String>,
    /// The end time of the scan.
    pub finished_at: Option<String>,
    /// The total logical size of all files scanned.
    pub total_logical_size: i64,
    /// The total allocated size of all files scanned.
    pub total_allocated_size: i64,
    /// The total number of directories scanned.
    pub dir_count: i64,
    /// The total number of files scanned.
    pub file_count: i64,
    /// The number of warnings generated during the scan.
    pub warning_count: i64,
}

/// An event that occurs during a scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScanEvent {
    /// The scan has started.
    Started {
        /// The root paths of the scan.
        root_paths: Vec<String>,
    },
    /// A progress update.
    Progress {
        /// The path currently being scanned.
        current_path: String,
        /// The number of directories scanned so far.
        dirs_scanned: u64,
        /// The number of files scanned so far.
        files_scanned: u64,
        /// The logical size of the scanned files so far.
        logical_size: u64,
        /// The allocated size of the scanned files so far.
        allocated_size: u64,
    },
    /// A warning has occurred.
    Warning {
        /// The path associated with the warning.
        path: String,
        /// The warning code.
        code: String,
        /// The warning message.
        message: String,
    },
    /// The scan has completed.
    Done {
        /// The total number of directories scanned.
        total_dirs: u64,
        /// The total number of files scanned.
        total_files: u64,
        /// The total logical size of all files scanned.
        total_logical_size: u64,
        /// The total allocated size of all files scanned.
        total_allocated_size: u64,
    },
    /// The scan has been cancelled.
    Cancelled,
    /// The scan has failed.
    Failed {
        /// The error message.
        message: String,
    },
}
