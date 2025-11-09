//! Type definitions for the SpeicherWald web UI.
//!
//! This module contains all the data structures used for communication
//! between the frontend and backend API. These types mirror the backend
//! data structures and provide serialization/deserialization support.
//!
//! ## Main Categories
//!
//! - **Scan Types**: Data structures for file system scans and results
//! - **API Types**: Request/response types for backend communication
//! - **UI Types**: Types used specifically for UI state management
//! - **Event Types**: Server-sent events for real-time updates

use serde::{Deserialize, Serialize};

/// A summary representation of a file system scan.
///
/// This struct contains metadata about a completed or running scan,
/// including timing information, statistics, and status.
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

/// Response containing a list of available drives.
///
/// This is the API response structure for the drives endpoint,
/// containing information about storage devices available for scanning.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct DrivesResponse {
    /// List of available drives/volumes
    pub items: Vec<DriveInfo>
}

/// Information about a storage drive or volume.
///
/// Contains details about a drive's capacity, free space, and type,
/// helping users make informed decisions about which drives to scan.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct DriveInfo {
    pub path: String,
    pub drive_type: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

/// Response when creating a new scan.
///
/// Contains the ID and initial status of a newly created scan,
/// allowing the frontend to track the scan's progress.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct CreateScanResp {
    /// Unique identifier for the scan
    pub id: String,
    /// Initial status (typically "running")
    pub status: String,
    /// ISO timestamp when the scan was started
    pub started_at: String
}

/// A node in the file system tree structure.
///
/// Represents either a file or directory in the scanned file system,
/// with metadata about size, counts, and timestamps.
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
    pub mtime: Option<i64>,
    pub atime: Option<i64>,
}

/// An item in a "top items" list (largest files or directories).
///
/// Used for displaying the largest files or directories in a scan,
/// helping users identify what's consuming the most space.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TopItem {
    Dir { path: String, parent_path: Option<String>, depth: i64, logical_size: i64, allocated_size: i64, file_count: i64, dir_count: i64, mtime: Option<i64>, atime: Option<i64> },
    File { path: String, parent_path: Option<String>, logical_size: i64, allocated_size: i64, mtime: Option<i64>, atime: Option<i64> },
}

/// An item in a directory listing.
///
/// Represents a file or directory when listing the contents of a directory,
/// used for navigation and file browsing in the UI.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ListItem {
    Dir { name: String, path: String, parent_path: Option<String>, depth: i64, logical_size: i64, allocated_size: i64, file_count: i64, dir_count: i64, mtime: Option<i64>, atime: Option<i64> },
    File { name: String, path: String, parent_path: Option<String>, logical_size: i64, allocated_size: i64, mtime: Option<i64>, atime: Option<i64> },
}

/// Results from a file system search operation.
///
/// Contains the items matching a search query along with metadata
/// about the search itself, including total count and query string.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct SearchResult {
    pub items: Vec<SearchItem>,
    pub total_count: i64,
    pub query: String,
}

/// An item that matches a search query.
///
/// Represents a file or directory that was found during a search operation,
/// with relevant metadata for display and selection.
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

/// Real-time events from a running scan.
///
/// These events are sent via Server-Sent Events (SSE) to provide
/// live updates about scan progress, warnings, and completion status.
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

/// Request to move or copy a file or directory.
///
/// Used when users want to move files between directories or drives,
/// with options for copy vs move and overwrite behavior.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct MovePathRequest {
    pub source: String,
    pub destination: String,
    #[serde(default)]
    pub remove_source: bool,
    #[serde(default)]
    pub overwrite: bool,
}

/// Response from a move/copy operation.
///
/// Contains detailed information about the result of a move or copy operation,
/// including timing, data transferred, and any warnings that occurred.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct MovePathResponse {
    pub status: String,
    pub source: String,
    pub destination: String,
    pub bytes_to_transfer: u64,
    pub bytes_moved: u64,
    pub freed_bytes: u64,
    pub duration_ms: u128,
    pub started_at: String,
    pub finished_at: String,
    pub warnings: Vec<String>,
}
