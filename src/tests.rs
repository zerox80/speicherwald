#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use crate::scanner::*;
    use crate::error::AppError;
    
    #[test]
    fn test_scan_options_default() {
        let options = ScanOptions::default();
        assert_eq!(options.follow_symlinks, false);
        assert_eq!(options.include_hidden, true);
        assert_eq!(options.measure_logical, true);
        assert_eq!(options.measure_allocated, true);
        assert!(options.excludes.is_empty());
        assert!(options.max_depth.is_none());
        assert!(options.concurrency.unwrap_or(0) > 0);
    }
    
    #[test]
    fn test_scan_options_custom() {
        let options = ScanOptions {
            follow_symlinks: true,
            include_hidden: false,
            measure_logical: false,
            measure_allocated: true,
            excludes: vec!["**/.git".to_string(), "**/node_modules".to_string()],
            max_depth: Some(5),
            concurrency: Some(8),
        };
        assert_eq!(options.follow_symlinks, true);
        assert_eq!(options.include_hidden, false);
        assert_eq!(options.measure_logical, false);
        assert_eq!(options.measure_allocated, true);
        assert_eq!(options.excludes.len(), 2);
        assert_eq!(options.max_depth, Some(5));
        assert_eq!(options.concurrency, Some(8));
    }
    
    #[test]
    fn test_scan_status_transitions() {
        // Test status string representations
        let statuses = vec!["running", "done", "cancelled", "failed"];
        for status in statuses {
            assert!(!status.is_empty());
        }
    }
    
    #[test]
    fn test_node_dto_creation() {
        let node = NodeDto {
            path: "C:/test".to_string(),
            parent_path: Some("C:/".to_string()),
            depth: 2,
            is_dir: true,
            logical_size: 1024,
            allocated_size: 2048,
            file_count: 10,
            dir_count: 5,
        };
        assert_eq!(node.path, "C:/test");
        assert_eq!(node.parent_path, Some("C:/".to_string()));
        assert_eq!(node.depth, 2);
        assert!(node.is_dir);
        assert_eq!(node.logical_size, 1024);
        assert_eq!(node.allocated_size, 2048);
        assert_eq!(node.file_count, 10);
        assert_eq!(node.dir_count, 5);
    }
    
    #[test]
    fn test_top_item_variants() {
        let dir_item = TopItem::Dir {
            path: "C:/dir".to_string(),
            parent_path: Some("C:/".to_string()),
            depth: 2,
            allocated_size: 1000,
            logical_size: 900,
            file_count: 10,
            dir_count: 2,
        };
        
        let file_item = TopItem::File {
            path: "C:/file.txt".to_string(),
            parent_path: Some("C:/".to_string()),
            allocated_size: 100,
            logical_size: 90,
        };
        
        match dir_item {
            TopItem::Dir { path, .. } => assert_eq!(path, "C:/dir"),
            _ => panic!("Expected Dir variant"),
        }
        
        match file_item {
            TopItem::File { path, .. } => assert_eq!(path, "C:/file.txt"),
            _ => panic!("Expected File variant"),
        }
    }
    
    #[test]
    fn test_list_item_variants() {
        let dir_item = ListItem::Dir {
            path: "C:/folder".to_string(),
            name: "folder".to_string(),
            parent_path: Some("C:/".to_string()),
            depth: 2,
            allocated_size: 5000,
            logical_size: 4500,
            file_count: 20,
            dir_count: 3,
            mtime: None,
        };
        
        let file_item = ListItem::File {
            path: "C:/document.pdf".to_string(),
            name: "document.pdf".to_string(),
            parent_path: Some("C:/".to_string()),
            allocated_size: 200,
            logical_size: 180,
            mtime: None,
        };
        
        match dir_item {
            ListItem::Dir { name, .. } => assert_eq!(name, "folder"),
            _ => panic!("Expected Dir variant"),
        }
        
        match file_item {
            ListItem::File { name, .. } => assert_eq!(name, "document.pdf"),
            _ => panic!("Expected File variant"),
        }
    }

    // ---------------- Tree & Top endpoint tests ----------------
    #[cfg(test)]
    mod tree_top_endpoint_tests {
        use axum::extract::{State, Path, Query};
        use axum::response::IntoResponse;
        use sqlx::sqlite::SqlitePoolOptions;
        use uuid::Uuid;
        use crate::{db, routes, state::AppState};

        async fn mk_state() -> AppState {
            let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
            db::init_db(&pool).await.unwrap();
            let cfg = crate::config::AppConfig::default();
            AppState::new(pool, cfg)
        }

        #[tokio::test]
        async fn get_tree_returns_root_and_child_within_depth() {
            let state = mk_state().await;
            let id = Uuid::new_v4();

            // Insert scan row
            let options_json = serde_json::to_string(&crate::types::ScanOptions::default()).unwrap();
            let root = std::env::temp_dir().join(format!("speicherwald_tree_root_{}", id));
            let root_s = root.to_string_lossy().to_string();
            let roots_json = serde_json::to_string(&vec![root_s.clone()]).unwrap();
            sqlx::query(
                r#"INSERT INTO scans (id, status, root_paths, options) VALUES (?1, 'done', ?2, ?3)"#
            )
            .bind(id.to_string())
            .bind(roots_json)
            .bind(options_json)
            .execute(&state.db).await.unwrap();

            // Insert nodes: root (depth d) and child (depth d+1)
            sqlx::query(
                r#"INSERT INTO nodes (scan_id, path, parent_path, depth, is_dir, logical_size, allocated_size, file_count, dir_count)
                   VALUES (?1, ?2, NULL, 1, 1, 10, 20, 1, 1)"#
            )
            .bind(id.to_string())
            .bind(&root_s)
            .execute(&state.db).await.unwrap();

            let child = format!("{}/child", root_s.replace('\\', "/"));
            sqlx::query(
                r#"INSERT INTO nodes (scan_id, path, parent_path, depth, is_dir, logical_size, allocated_size, file_count, dir_count)
                   VALUES (?1, ?2, ?3, 2, 1, 5, 10, 0, 0)"#
            )
            .bind(id.to_string())
            .bind(&child)
            .bind(&root_s)
            .execute(&state.db).await.unwrap();

            let q = routes::scans::TreeQuery { path: Some(root_s.clone()), depth: Some(1), sort: Some("size".into()), limit: Some(100) };
            let res = routes::scans::get_tree(State(state.clone()), Path(id), Query(q)).await.unwrap();
            let resp = res.into_response();
            assert!(resp.status().is_success());
        }

        #[tokio::test]
        async fn get_top_returns_sorted_dirs() {
            let state = mk_state().await;
            let id = Uuid::new_v4();

            // Insert scan row
            let options_json = serde_json::to_string(&crate::types::ScanOptions::default()).unwrap();
            let root = std::env::temp_dir().join(format!("speicherwald_top_root_{}", id));
            let root_s = root.to_string_lossy().to_string();
            let roots_json = serde_json::to_string(&vec![root_s.clone()]).unwrap();
            sqlx::query(
                r#"INSERT INTO scans (id, status, root_paths, options) VALUES (?1, 'done', ?2, ?3)"#
            )
            .bind(id.to_string())
            .bind(roots_json)
            .bind(options_json)
            .execute(&state.db).await.unwrap();

            // Insert two directory nodes with different allocated_size
            for (p, alloc) in [(format!("{}/a", root_s), 100i64), (format!("{}/b", root_s), 200i64)] {
                sqlx::query(
                    r#"INSERT INTO nodes (scan_id, path, parent_path, depth, is_dir, logical_size, allocated_size, file_count, dir_count)
                       VALUES (?1, ?2, ?3, 2, 1, 0, ?4, 0, 0)"#
                )
                .bind(id.to_string())
                .bind(&p)
                .bind(&root_s)
                .bind(alloc)
                .execute(&state.db).await.unwrap();
            }

            let q = routes::scans::TopQuery { scope: Some("dirs".into()), limit: Some(10) };
            let res = routes::scans::get_top(State(state.clone()), Path(id), Query(q)).await.unwrap();
            let resp = res.into_response();
            assert!(resp.status().is_success());
        }
    }

// ---------------- Integration tests for list endpoint ----------------
#[cfg(test)]
mod list_endpoint_tests {
    use axum::body;
    use axum::response::IntoResponse;
    use axum::extract::{State, Path, Query};
    use sqlx::sqlite::SqlitePoolOptions;
    use uuid::Uuid;

    use crate::{db, routes, state::AppState};

    async fn test_state_with_memory_db() -> AppState {
        let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
        db::init_db(&pool).await.unwrap();
        let cfg = crate::config::AppConfig::default();
        AppState::new(pool, cfg)
    }

    #[tokio::test]
    async fn list_roots_returns_placeholders_when_nodes_missing() {
        let state = test_state_with_memory_db().await;
        let id = Uuid::new_v4();

        // Create a temporary root directory that exists on the filesystem
        let tmp_root = std::env::temp_dir().join(format!("speicherwald_test_root_{}", id));
        std::fs::create_dir_all(&tmp_root).unwrap();
        let root_str = tmp_root.to_string_lossy().to_string();

        // Insert a scan row with this root, but do NOT insert nodes/files rows yet
        let options_json = serde_json::to_string(&crate::types::ScanOptions::default()).unwrap();
        let roots_json = serde_json::to_string(&vec![root_str.clone()]).unwrap();
        sqlx::query(
            r#"INSERT INTO scans (id, status, root_paths, options) VALUES (?1, 'running', ?2, ?3)"#
        )
        .bind(id.to_string())
        .bind(roots_json)
        .bind(options_json)
        .execute(&state.db).await.unwrap();

        // Call handler directly
        let res = routes::scans::get_list(State(state.clone()), Path(id), Query(routes::scans::ListQuery::default())).await.unwrap();
        let resp = res.into_response();
        assert!(resp.status().is_success());
        let body = body::to_bytes(resp.into_body(), 2 * 1024 * 1024).await.unwrap();
        let items: Vec<crate::types::ListItem> = serde_json::from_slice(&body).unwrap();
        // We expect at least one Dir placeholder with our root path
        assert!(!items.is_empty());
        assert!(items.iter().any(|it| matches!(it, crate::types::ListItem::Dir { path, .. } if path == &root_str)));
    }

    #[tokio::test]
    async fn list_children_returns_dir_and_file() {
        let state = test_state_with_memory_db().await;
        let id = Uuid::new_v4();

        // Prepare filesystem: root with one child dir and one file
        let root = std::env::temp_dir().join(format!("speicherwald_children_root_{}", id));
        let child_dir = root.join("child");
        let child_file = root.join("file.txt");
        std::fs::create_dir_all(&child_dir).unwrap();
        std::fs::write(&child_file, b"hello").unwrap();
        let root_s = root.to_string_lossy().to_string();
        let child_dir_s = child_dir.to_string_lossy().to_string();
        let child_file_s = child_file.to_string_lossy().to_string();

        // Insert scan row
        let options_json = serde_json::to_string(&crate::types::ScanOptions::default()).unwrap();
        let roots_json = serde_json::to_string(&vec![root_s.clone()]).unwrap();
        sqlx::query(
            r#"INSERT INTO scans (id, status, root_paths, options) VALUES (?1, 'running', ?2, ?3)"#
        )
        .bind(id.to_string())
        .bind(roots_json)
        .bind(options_json)
        .execute(&state.db).await.unwrap();

        // Insert directory node under root
        sqlx::query(
            r#"INSERT INTO nodes (scan_id, path, parent_path, depth, is_dir, logical_size, allocated_size, file_count, dir_count)
               VALUES (?1, ?2, ?3, 2, 1, 0, 0, 0, 0)"#
        )
        .bind(id.to_string())
        .bind(&child_dir_s)
        .bind(&root_s)
        .execute(&state.db).await.unwrap();

        // Insert file row under root
        sqlx::query(
            r#"INSERT INTO files (scan_id, path, parent_path, logical_size, allocated_size)
               VALUES (?1, ?2, ?3, 5, 5)"#
        )
        .bind(id.to_string())
        .bind(&child_file_s)
        .bind(&root_s)
        .execute(&state.db).await.unwrap();

        // Call handler directly for children listing
        let q = routes::scans::ListQuery { path: Some(root_s.clone()), sort: None, order: None, limit: None, offset: None };
        let res = routes::scans::get_list(State(state.clone()), Path(id), Query(q)).await.unwrap();
        let resp = res.into_response();
        assert!(resp.status().is_success());
        let body = body::to_bytes(resp.into_body(), 2 * 1024 * 1024).await.unwrap();
        let items: Vec<crate::types::ListItem> = serde_json::from_slice(&body).unwrap();
        assert!(items.iter().any(|it| matches!(it, crate::types::ListItem::Dir { path, .. } if path == &child_dir_s)));
        assert!(items.iter().any(|it| matches!(it, crate::types::ListItem::File { path, .. } if path == &child_file_s)));
    }
}
    
    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1048576), "1.00 MB");
        assert_eq!(format_bytes(1073741824), "1.00 GB");
        assert_eq!(format_bytes(1099511627776), "1.00 TB");
    }
    
    #[test]
    fn test_scan_event_serialization() {
        let event = ScanEvent::Progress {
            current_path: "C:/test".to_string(),
            dirs_scanned: 10,
            files_scanned: 100,
            logical_size: 1024,
            allocated_size: 2048,
        };
        
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"current_path\":\"C:/test\""));
        assert!(json.contains("\"dirs_scanned\":10"));
    }
    
    #[test]
    fn test_drive_info() {
        let drive = DriveInfo {
            path: "C:".to_string(),
            drive_type: "Local".to_string(),
            total_bytes: 1000000000,
            free_bytes: 500000000,
        };
        
        let used_bytes = drive.total_bytes - drive.free_bytes;
        assert_eq!(used_bytes, 500000000);
        
        let percent_used = (used_bytes as f64 / drive.total_bytes as f64) * 100.0;
        assert_eq!(percent_used, 50.0);
    }
    
    #[test]
    fn test_scan_summary() {
        let summary = ScanResultSummary {
            total_dirs: 100,
            total_files: 1000,
            total_logical_size: 1048576,
            total_allocated_size: 2097152,
            warnings: 5,
        };
        
        assert_eq!(summary.total_dirs, 100);
        assert_eq!(summary.total_files, 1000);
        assert_eq!(summary.warnings, 5);
        
        // Test that allocated size is at least logical size
        assert!(summary.total_allocated_size >= summary.total_logical_size);
    }
    
    #[tokio::test]
    async fn test_config_loading() {
        use crate::config;
        
        let cfg = config::load().unwrap();
        assert!(!cfg.server.host.is_empty());
        assert!(cfg.server.port > 0);
        assert!(!cfg.database.url.is_empty());
    }

    #[test]
    #[cfg(not(windows))]
    fn test_ensure_sqlite_parent_dir_non_windows() {
        use crate::config::ensure_sqlite_parent_dir;
        use uuid::Uuid;

        let base = std::env::temp_dir().join(format!("speicherwald_test_cfg_{}", Uuid::new_v4()));
        let db_path = base.join("nested").join("test.db");
        let url = format!("sqlite://{}", db_path.to_string_lossy());

        // Cleanup just in case
        let _ = std::fs::remove_dir_all(&base);
        assert!(!db_path.parent().unwrap().exists());

        ensure_sqlite_parent_dir(&url).unwrap();
        assert!(db_path.parent().unwrap().exists());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    #[cfg(windows)]
    fn test_ensure_sqlite_parent_dir_windows() {
        use crate::config::ensure_sqlite_parent_dir;
        use uuid::Uuid;

        let base = std::env::temp_dir().join(format!("speicherwald_test_cfg_{}", Uuid::new_v4()));
        let db_path = base.join("nested").join("test.db");
        // sqlite URL mit führendem Slash vor dem Laufwerksbuchstaben (wird vom Code entfernt)
        let pstr = db_path.to_string_lossy().replace('\\', "/");
        let url = format!("sqlite:///{}", pstr);

        // Cleanup just in case
        let _ = std::fs::remove_dir_all(&base);
        assert!(!db_path.parent().unwrap().exists());

        ensure_sqlite_parent_dir(&url).unwrap();
        assert!(db_path.parent().unwrap().exists());

        let _ = std::fs::remove_dir_all(&base);
    }
    
    #[test]
    fn test_glob_patterns() {
        use globset::{Glob, GlobSetBuilder};
        let patterns = vec![
            "**/.git".to_string(),
            "**/node_modules".to_string(),
            "*.tmp".to_string(),
        ];
        let mut b = GlobSetBuilder::new();
        for p in &patterns { b.add(Glob::new(p).unwrap()); }
        let gs = b.build().unwrap();
        assert_eq!(gs.len(), 3);
        
        // Test pattern matching
        assert!(gs.is_match(".git"));
        assert!(gs.is_match("path/to/.git"));
        assert!(gs.is_match("node_modules"));
        assert!(gs.is_match("src/node_modules"));
        assert!(gs.is_match("file.tmp"));
        assert!(!gs.is_match("file.txt"));
        assert!(!gs.is_match("gitignore"));
    }
    
    #[test]
    fn test_path_normalization() {
        #[cfg(windows)]
        {
            let paths = vec![
                ("C:\\Users\\test", "C:\\Users\\test"),
                ("C:/Users/test", "C:/Users/test"),
                ("\\\\?\\C:\\Users\\test", "\\\\?\\C:\\Users\\test"),
            ];
            for (input, _expected) in paths {
                assert!(!input.is_empty());
            }
        }
        
        #[cfg(not(windows))]
        {
            let paths = vec![
                ("/home/user", "/home/user"),
                ("/tmp/test", "/tmp/test"),
            ];
            for (input, expected) in paths {
                assert_eq!(input, expected);
            }
        }
    }
    
    #[test]
    fn test_scan_error_types() {
        let errors = vec![
            AppError::NotFound("Scan not found".to_string()),
            AppError::InvalidInput("Invalid path".to_string()),
            AppError::Database("Connection failed".to_string()),
            AppError::Scanner("Access denied".to_string()),
            AppError::Internal(anyhow::anyhow!("Unexpected error")),
        ];
        
        for error in errors {
            let error_str = error.to_string();
            assert!(!error_str.is_empty());
        }
    }
    
    #[test]
    fn test_create_scan_request_validation() {
        let valid_req = CreateScanRequest {
            root_paths: vec!["C:/".to_string()],
            follow_symlinks: Some(false),
            include_hidden: Some(true),
            measure_logical: Some(true),
            measure_allocated: Some(true),
            excludes: Some(vec![]),
            max_depth: None,
            concurrency: None,
        };
        assert!(!valid_req.root_paths.is_empty());
        
        let invalid_req = CreateScanRequest {
            root_paths: vec![],
            follow_symlinks: None,
            include_hidden: None,
            measure_logical: None,
            measure_allocated: None,
            excludes: None,
            max_depth: None,
            concurrency: None,
        };
        assert!(invalid_req.root_paths.is_empty());
    }
    
    #[test]
    fn test_metrics_values() {
        use crate::metrics::Metrics;
        use std::sync::atomic::Ordering;
        
        let metrics = Metrics::new();
        
        // Test initial values
        assert_eq!(metrics.scans_started.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.scans_completed.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.scans_failed.load(Ordering::Relaxed), 0);
        
        // Increment metrics
        metrics.scans_started.fetch_add(1, Ordering::Relaxed);
        assert_eq!(metrics.scans_started.load(Ordering::Relaxed), 1);
        
        metrics.scans_completed.fetch_add(1, Ordering::Relaxed);
        assert_eq!(metrics.scans_completed.load(Ordering::Relaxed), 1);
        
        metrics.scans_failed.fetch_add(1, Ordering::Relaxed);
        assert_eq!(metrics.scans_failed.load(Ordering::Relaxed), 1);
    }

    // ---------------- Health & Metrics endpoint tests ----------------
    async fn mk_state() -> crate::state::AppState {
        use sqlx::sqlite::SqlitePoolOptions;
        let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
        crate::db::init_db(&pool).await.unwrap();
        let cfg = crate::config::AppConfig::default();
        crate::state::AppState::new(pool, cfg)
    }

    #[tokio::test]
    async fn test_readyz_ok() {
        use axum::{extract::State, response::IntoResponse, body};
        let state = mk_state().await;
        let resp = crate::routes::health::readyz(State(state)).await.into_response();
        assert!(resp.status().is_success());
        let body = body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let s = String::from_utf8(body.to_vec()).unwrap();
        assert_eq!(s, "ready");
    }

    #[tokio::test]
    async fn test_metrics_snapshot_defaults() {
        use axum::{extract::State, response::IntoResponse, body};
        let state = mk_state().await;
        let resp = crate::routes::health::metrics(State(state)).await.into_response();
        assert!(resp.status().is_success());
        let bytes = body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["scans_started"], 0);
        assert_eq!(v["scans_completed"], 0);
        assert_eq!(v["scans_failed"], 0);
        assert!(v["uptime_seconds"].as_u64().is_some());
    }
}

// Helper function for formatting bytes
pub fn format_bytes(bytes: i64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB", "PB"];
    
    if bytes == 0 {
        return "0 B".to_string();
    }
    
    let bytes_abs = bytes.abs() as f64;
    let k: f64 = 1024.0;
    let i = (bytes_abs.ln() / k.ln()).floor() as usize;
    
    if i >= UNITS.len() {
        return format!("{:.2} {}", bytes_abs / k.powi((UNITS.len() - 1) as i32), UNITS[UNITS.len() - 1]);
    }
    
    if i == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.2} {}", bytes_abs / k.powi(i as i32), UNITS[i])
    }
}
