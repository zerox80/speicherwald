//! Path manipulation utilities for cross-platform file system operations.
//!
//! This module provides helper functions for handling file paths across different
//! operating systems, with special handling for Windows drive paths and UNC paths.
//! These utilities are used throughout the application to ensure consistent
//! path handling regardless of the underlying platform.

use std::path::Path;

/// Windows-specific function to get the volume root for a given path.
///
/// This function extracts the volume root from various Windows path formats:
/// - Standard drive paths (e.g., `C:\Users`) → `C:\`
/// - UNC paths (e.g., `\\server\share\folder`) → `\\server\share`
/// - Invalid or malformed paths → `C:\` (default)
///
/// This is essential for operations that need to work with volume-level
/// information, such as determining available drive space.
///
/// # Arguments
///
/// * `path` - The path to extract the volume root from
///
/// # Returns
///
/// A string containing the volume root path
#[cfg(windows)]
pub fn get_volume_root(path: &Path) -> String {
    let path_str = path.to_string_lossy();
    
    // Extract drive letter for regular paths
    if path_str.len() >= 2 && path_str.chars().nth(1) == Some(':') {
        let drive = path_str.chars().next().unwrap_or('C');
        return format!("{}:\\", drive);
    }
    
    // For UNC paths, return the server\share part
    if path_str.starts_with("\\\\") {
        let parts: Vec<&str> = path_str.trim_start_matches("\\\\").split('\\').collect();
        if parts.len() >= 2 {
            return format!("\\\\{}\\{}", parts[0], parts[1]);
        }
    }
    
    // Default to C:
    "C:\\".to_string()
}

/// Non-Windows fallback function for getting the volume root.
///
/// On Unix-like systems (Linux, macOS, etc.), there is a single unified
/// filesystem hierarchy starting at the root directory `/`. This function
/// provides a consistent interface across platforms by always returning the
/// Unix root path.
///
/// # Arguments
///
/// * `_path` - The path parameter is ignored on non-Windows systems
///
/// # Returns
///
/// Always returns `"/"` - the Unix root directory
#[cfg(not(windows))]
pub fn get_volume_root(_path: &Path) -> String {
    "/".to_string()
}
