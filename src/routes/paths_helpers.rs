// Helper functions for path operations

/// (Windows specific) Gets the root of the volume for a given path.
///
/// For standard paths (e.g., `C:\Users`), this returns the drive root (`C:\`).
/// For UNC paths (e.g., `\\server\share\folder`), this returns the share root (`\\server\share`).
///
/// # Arguments
///
/// * `path` - The path to get the volume root for.
#[cfg(windows)]
pub fn get_volume_root(path: &std::path::Path) -> String {
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

/// (Non-Windows) Fallback for getting the volume root.
///
/// This function always returns `/` as the root.
#[cfg(not(windows))]
pub fn get_volume_root(_path: &std::path::Path) -> String {
    "/".to_string()
}
