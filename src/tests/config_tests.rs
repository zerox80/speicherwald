#[cfg(test)]
mod tests {
    use crate::config::{AppConfig, ServerConfig, DatabaseConfig, ScanDefaultsConfig, ScannerConfig};
    use std::env;
    use tempfile::NamedTempFile;
    use std::fs;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.database.url, "sqlite://data/speicherwald.db");
        assert!(!config.scan_defaults.follow_symlinks);
        assert!(config.scan_defaults.include_hidden);
        assert!(config.scan_defaults.measure_logical);
        assert!(config.scan_defaults.measure_allocated);
        assert_eq!(config.scan_defaults.excludes.len(), 0);
    }

    #[test]
    fn test_config_from_env() {
        // Set environment variables
        env::set_var("SPEICHERWALD__SERVER__HOST", "0.0.0.0");
        env::set_var("SPEICHERWALD__SERVER__PORT", "3000");
        env::set_var("SPEICHERWALD__DATABASE__URL", "sqlite://test.db");
        env::set_var("SPEICHERWALD__SCAN_DEFAULTS__FOLLOW_SYMLINKS", "true");
        env::set_var("SPEICHERWALD__SCAN_DEFAULTS__INCLUDE_HIDDEN", "false");
        
        let config = crate::config::load().unwrap();
        
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.port, 3000);
        assert_eq!(config.database.url, "sqlite://test.db");
        assert!(config.scan_defaults.follow_symlinks);
        assert!(!config.scan_defaults.include_hidden);
        
        // Clean up
        env::remove_var("SPEICHERWALD__SERVER__HOST");
        env::remove_var("SPEICHERWALD__SERVER__PORT");
        env::remove_var("SPEICHERWALD__DATABASE__URL");
        env::remove_var("SPEICHERWALD__SCAN_DEFAULTS__FOLLOW_SYMLINKS");
        env::remove_var("SPEICHERWALD__SCAN_DEFAULTS__INCLUDE_HIDDEN");
    }

    #[test]
    fn test_config_from_file() {
        let temp_file = NamedTempFile::new().unwrap();
        let config_content = r#"
[server]
host = "192.168.1.1"
port = 9000

[database]
url = "sqlite://custom.db"

[scan_defaults]
follow_symlinks = true
include_hidden = false
excludes = ["**/node_modules", "**/.git"]

[scanner]
batch_size = 5000
flush_threshold = 10000
flush_interval_ms = 1000
dir_concurrency = 16
"#;
        
        fs::write(temp_file.path(), config_content).unwrap();
        
        // Set config path
        let config_path = temp_file.path().with_extension("");
        env::set_var("SPEICHERWALD_CONFIG", config_path.to_str().unwrap());
        
        let config = crate::config::load().unwrap();
        
        assert_eq!(config.server.host, "192.168.1.1");
        assert_eq!(config.server.port, 9000);
        assert_eq!(config.database.url, "sqlite://custom.db");
        assert!(config.scan_defaults.follow_symlinks);
        assert!(!config.scan_defaults.include_hidden);
        assert_eq!(config.scan_defaults.excludes.len(), 2);
        assert_eq!(config.scanner.batch_size, 5000);
        assert_eq!(config.scanner.flush_threshold, 10000);
        assert_eq!(config.scanner.flush_interval_ms, 1000);
        assert_eq!(config.scanner.dir_concurrency, Some(16));
        
        // Clean up
        env::remove_var("SPEICHERWALD_CONFIG");
    }

    #[test]
    fn test_scanner_config_defaults() {
        let config = ScannerConfig::default();
        
        assert_eq!(config.batch_size, 4000);
        assert_eq!(config.flush_threshold, 8000);
        assert_eq!(config.flush_interval_ms, 750);
        assert_eq!(config.dir_concurrency, Some(12));
        assert!(config.handle_limit.is_none());
    }

    #[test]
    fn test_ensure_sqlite_parent_dir() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let db_path = temp_dir.path().join("subdir/test.db");
        let db_url = format!("sqlite://{}", db_path.display());
        
        assert!(!db_path.parent().unwrap().exists());
        
        crate::config::ensure_sqlite_parent_dir(&db_url).unwrap();
        
        assert!(db_path.parent().unwrap().exists());
    }

    #[test]
    fn test_ensure_sqlite_parent_dir_non_sqlite() {
        // Non-SQLite URL should not create directories
        let result = crate::config::ensure_sqlite_parent_dir("postgres://localhost/db");
        assert!(result.is_ok());
    }

    #[test]
    fn test_config_priority() {
        // Test that environment variables override file config
        let temp_file = NamedTempFile::new().unwrap();
        let config_content = r#"
[server]
port = 7000
"#;
        fs::write(temp_file.path(), config_content).unwrap();
        
        let config_path = temp_file.path().with_extension("");
        env::set_var("SPEICHERWALD_CONFIG", config_path.to_str().unwrap());
        env::set_var("SPEICHERWALD__SERVER__PORT", "8888");
        
        let config = crate::config::load().unwrap();
        
        // Environment variable should override file config
        assert_eq!(config.server.port, 8888);
        
        // Clean up
        env::remove_var("SPEICHERWALD_CONFIG");
        env::remove_var("SPEICHERWALD__SERVER__PORT");
    }

    #[test]
    fn test_scan_defaults_excludes_from_env() {
        env::set_var("SPEICHERWALD__SCAN_DEFAULTS__EXCLUDES", r#"["**/target","**/.git","**/node_modules"]"#);
        
        let config = crate::config::load().unwrap();
        
        assert_eq!(config.scan_defaults.excludes.len(), 3);
        assert!(config.scan_defaults.excludes.contains(&"**/target".to_string()));
        assert!(config.scan_defaults.excludes.contains(&"**/.git".to_string()));
        assert!(config.scan_defaults.excludes.contains(&"**/node_modules".to_string()));
        
        // Clean up
        env::remove_var("SPEICHERWALD__SCAN_DEFAULTS__EXCLUDES");
    }
}
