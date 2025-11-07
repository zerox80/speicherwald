// Helper functions for path operations

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

#[cfg(not(windows))]
pub fn get_volume_root(_path: &std::path::Path) -> String {
    "/".to_string()
}
