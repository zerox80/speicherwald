#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{HeaderMap, Request, StatusCode},
    };
    use axum::middleware::from_fn_with_state;
    use tower::ServiceExt;
    use serde_json::{json, Value};
    use http_body_util::BodyExt; // for .collect()
    use crate::state::AppState;
    use crate::error::AppError;
    use crate::routes;
    use sqlx::sqlite::SqlitePoolOptions;
    use tempfile::{tempdir, NamedTempFile};
    use uuid::Uuid;
    async fn setup_test_app() -> (axum::Router, AppState) {
        // Create temporary database
        let temp_db = NamedTempFile::new().unwrap();
        let db_url = format!("sqlite:{}", temp_db.path().display());
        
        // Create database
        sqlx::Sqlite::create_database(&db_url).await.unwrap();
        
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&db_url)
            .await
            .unwrap();
        // Initialize schema
        crate::db::init_db(&pool).await.unwrap();
        // Create test config
        let config = crate::config::AppConfig {
            server: crate::config::ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 8080,
            },
            database: crate::config::DatabaseConfig {
                url: db_url,
            },
            scan_defaults: crate::config::ScanDefaultsConfig {
                follow_symlinks: false,
                include_hidden: true,
                measure_logical: true,
                measure_allocated: true,
                excludes: vec![],
                max_depth: None,
                concurrency: None,
            },
            scanner: crate::config::ScannerConfig {
                batch_size: 100,
                flush_threshold: 200,
                flush_interval_ms: 100,
                handle_limit: None,
                dir_concurrency: Some(4),
            },
        };
        let state = AppState::new(pool, config);
        let app = axum::Router::new()
            .route("/healthz", axum::routing::get(routes::health::healthz))
            .route("/readyz", axum::routing::get(routes::health::readyz))
            .route("/metrics", axum::routing::get(routes::health::metrics))
            .route("/version", axum::routing::get(routes::health::version))
            .route("/drives", axum::routing::get(routes::drives::list_drives))
            .route("/scans", 
                axum::routing::post(routes::scans::create_scan)
                .get(routes::scans::list_scans))
            .route("/scans/:id", 
                axum::routing::get(routes::scans::get_scan)
                .delete(routes::scans::cancel_scan))
            .route("/scans/:id/search", axum::routing::get(routes::search::search_scan))
            .with_state(state.clone())
            .layer(from_fn_with_state(
                state.config.clone(),
                crate::middleware::security_headers::security_headers_middleware,
            ));
        (app, state)
    }
    #[tokio::test]
    async fn test_healthz_endpoint() {
        let (app, _) = setup_test_app().await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    #[tokio::test]
    async fn test_security_headers_present() {
        let (app, _) = setup_test_app().await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers();
        assert!(headers.contains_key("x-content-type-options"));
        assert!(headers.contains_key("x-frame-options"));
        assert!(headers.contains_key("referrer-policy"));
        assert!(headers.contains_key("permissions-policy"));
        assert!(headers.contains_key("cross-origin-opener-policy"));
        assert!(headers.contains_key("cross-origin-resource-policy"));
    }
    #[tokio::test]
    async fn test_readyz_endpoint() {
        let (app, _) = setup_test_app().await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/readyz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    #[tokio::test]
    async fn test_metrics_endpoint() {
        let (app, _) = setup_test_app().await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();
        
        assert!(json.get("uptime_seconds").is_some());
        assert!(json.get("scans_started").is_some());
        assert!(json.get("scans_completed").is_some());
    }
    #[tokio::test]
    async fn test_version_endpoint() {
        let (app, _) = setup_test_app().await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/version")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert!(json.get("name").is_some());
        assert!(json.get("version").is_some());
        assert!(json.get("build").is_some());
    }
    #[tokio::test]
    async fn test_list_drives_endpoint() {
        let (app, _) = setup_test_app().await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/drives")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();
        
        assert!(json.get("items").is_some());
        assert!(json.get("items").unwrap().is_array());
    }
    #[tokio::test]
    async fn test_create_scan_endpoint() {
        let (app, _) = setup_test_app().await;
        
        let temp_dir = tempfile::TempDir::new().unwrap();
        let scan_request = json!({
            "root_paths": [temp_dir.path().to_str().unwrap()],
            "follow_symlinks": false,
            "include_hidden": true,
            "measure_logical": true,
            "measure_allocated": true,
            "excludes": [],
            "max_depth": null,
            "concurrency": 4
        });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/scans")
                    .header("content-type", "application/json")
                    .body(Body::from(scan_request.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();
        
        assert!(json.get("id").is_some());
        assert!(json.get("status").is_some());
    }
    #[tokio::test]
    async fn test_list_scans_endpoint() {
        let (app, _) = setup_test_app().await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/scans")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();
        
        assert!(json.is_array());
    }
    #[tokio::test]
    async fn test_get_scan_not_found() {
        let (app, _) = setup_test_app().await;
        let missing_id = uuid::Uuid::new_v4();
        let response = app
            .oneshot(
                Request::builder()
                    .uri(&format!("/scans/{}", missing_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    #[tokio::test]
    async fn test_cancel_scan_not_found() {
        let (app, _) = setup_test_app().await;
        let missing_id = uuid::Uuid::new_v4();
        let response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(&format!("/scans/{}", missing_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
    #[tokio::test]
    async fn test_search_endpoint_returns_results() {
        use uuid::Uuid;
        let (app, state) = setup_test_app().await;
        // Prepare a finished scan with some nodes and files
        let scan_id = Uuid::new_v4();
        let roots_json = serde_json::to_string(&vec![r#"C:\data"#.to_string()]).unwrap();
        let options_json = serde_json::to_string(&crate::types::ScanOptions::default()).unwrap();
        sqlx::query(
            r#"INSERT INTO scans (id, status, root_paths, options, finished_at, total_logical_size, total_allocated_size, dir_count, file_count, warning_count)
               VALUES (?1, 'done', ?2, ?3, strftime('%Y-%m-%dT%H:%M:%SZ','now'), 12345, 23456, 3, 2, 0)"#,
        )
        .bind(scan_id.to_string())
        .bind(roots_json)
        .bind(options_json)
        .execute(&state.db)
        .await
        .unwrap();
        // Insert directories (nodes)
        let node_sql = r#"INSERT INTO nodes
            (scan_id, path, parent_path, depth, is_dir, logical_size, allocated_size, file_count, dir_count)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"#;
        sqlx::query(node_sql)
            .bind(scan_id.to_string())
            .bind("C:/data")
            .bind(Option::<String>::None)
            .bind(0i64)
            .bind(1i64)
            .bind(1000i64)
            .bind(2000i64)
            .bind(1i64)
            .bind(1i64)
            .execute(&state.db)
            .await
            .unwrap();
        sqlx::query(node_sql)
            .bind(scan_id.to_string())
            .bind("C:/data/foo")
            .bind(Some("C:/data".to_string()))
            .bind(1i64)
            .bind(1i64)
            .bind(5000i64)
            .bind(10000i64)
            .bind(1i64)
            .bind(0i64)
            .execute(&state.db)
            .await
            .unwrap();
        // Insert files
        let file_sql = r#"INSERT INTO files
            (scan_id, path, parent_path, logical_size, allocated_size)
            VALUES (?1, ?2, ?3, ?4, ?5)"#;
        sqlx::query(file_sql)
            .bind(scan_id.to_string())
            .bind("C:/data/foo/report.pdf")
            .bind(Some("C:/data/foo".to_string()))
            .bind(4096i64)
            .bind(8192i64)
            .execute(&state.db)
            .await
            .unwrap();
        sqlx::query(file_sql)
            .bind(scan_id.to_string())
            .bind("C:/data/foo/readme.txt")
            .bind(Some("C:/data/foo".to_string()))
            .bind(1024i64)
            .bind(4096i64)
            .execute(&state.db)
            .await
            .unwrap();
        // Query the search endpoint
        let uri = format!("/scans/{}/search?query=foo&limit=50", scan_id);
        let response = app
            .oneshot(
                Request::builder()
                    .uri(&uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        // Validate structure
        assert!(json.get("items").is_some());
        assert!(json.get("total_count").is_some());
        let items = json.get("items").unwrap().as_array().unwrap();
        assert!(!items.is_empty());
        // Each item must have a type (Dir|File)
        assert!(items.iter().all(|it| it.get("type").is_some()));
    }
    #[tokio::test]
    async fn test_create_scan_invalid_path() {
        let (app, _) = setup_test_app().await;
        
        let scan_request = json!({
            "root_paths": ["/non/existent/path"],
            "follow_symlinks": false,
            "include_hidden": true,
            "measure_logical": true,
            "measure_allocated": true,
            "excludes": [],
            "max_depth": null,
            "concurrency": 4
        });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/scans")
                    .header("content-type", "application/json")
                    .body(Body::from(scan_request.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    #[tokio::test]
    async fn test_create_scan_empty_paths() {
        let (app, _) = setup_test_app().await;
        
        let scan_request = json!({
            "root_paths": [],
            "follow_symlinks": false,
            "include_hidden": true,
            "measure_logical": true,
            "measure_allocated": true,
            "excludes": [],
            "max_depth": null,
            "concurrency": 4
        });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/scans")
                    .header("content-type", "application/json")
                    .body(Body::from(scan_request.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    #[tokio::test]
    async fn create_scan_rejects_file_root() {
        let temp = tempdir().unwrap();
        let file_path = temp.path().join("not_a_dir.txt");
        std::fs::write(&file_path, b"data").unwrap();
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::db::init_db(&pool).await.unwrap();
        let mut config = crate::config::AppConfig::default();
        config.database.url = "sqlite::memory:".to_string();
        let state = AppState::new(pool, config);
        let request = crate::types::CreateScanRequest {
            root_paths: vec![file_path.to_string_lossy().to_string()],
            follow_symlinks: None,
            include_hidden: None,
            measure_logical: None,
            measure_allocated: None,
            excludes: None,
            max_depth: None,
            concurrency: None,
        };
        let result = routes::scans::create_scan(
            axum::extract::State(state),
            HeaderMap::new(),
            axum::Json(request),
        )
        .await;
        assert!(matches!(result, Err(AppError::BadRequest(msg)) if msg.contains("not a directory")));
    }
    #[tokio::test]
    async fn export_csv_clamps_limit_and_escapes() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::db::init_db(&pool).await.unwrap();
        let mut config = crate::config::AppConfig::default();
        config.database.url = "sqlite::memory:".to_string();
        let state = AppState::new(pool.clone(), config);
        let scan_id = Uuid::new_v4();
        let options_json = serde_json::to_string(&crate::types::ScanOptions::default()).unwrap();
        let roots_json = serde_json::to_string(&vec![r#"C:\root"#.to_string()]).unwrap();
        sqlx::query(
            r#"INSERT INTO scans (id, status, root_paths, options) VALUES (?1, 'done', ?2, ?3)"#,
        )
        .bind(scan_id.to_string())
        .bind(roots_json)
        .bind(options_json)
        .execute(&state.db)
        .await
        .unwrap();
        let parent = r#"C:\Parent, "special""#;
        sqlx::query(
            r#"INSERT INTO nodes (scan_id, path, parent_path, depth, is_dir, logical_size, allocated_size, file_count, dir_count)
               VALUES (?1, ?2, ?3, 1, 1, 10, 200, 1, 0),
                      (?1, ?4, ?3, 1, 1, 5, 100, 1, 0)"#,
        )
        .bind(scan_id.to_string())
        .bind(r#"C:\root"#)
        .bind(parent)
        .bind(r#"C:\other"#)
        .execute(&state.db)
        .await
        .unwrap();
        sqlx::query(
            r#"INSERT INTO files (scan_id, path, parent_path, logical_size, allocated_size)
               VALUES (?1, ?2, ?3, 20, 300),
                      (?1, ?4, ?3, 10, 150)"#,
        )
        .bind(scan_id.to_string())
        .bind(r#"C:\root\file1.txt"#)
        .bind(parent)
        .bind(r#"C:\root\file2.txt"#)
        .execute(&state.db)
        .await
        .unwrap();
        let query = routes::export::ExportQuery {
            format: "csv".to_string(),
            scope: Some("all".to_string()),
            limit: Some(-5),
        };
        let response = routes::export::export_scan(
            axum::extract::State(state),
            axum::extract::Path(scan_id),
            axum::extract::Query(query),
        )
        .await
        .unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let csv = String::from_utf8(body.to_vec()).unwrap();
        assert!(csv.contains("Type,Path,Parent Path,Logical Size,Allocated Size"));
        assert_eq!(csv.lines().filter(|line| line.starts_with("Node,\"")).count(), 1);
        assert_eq!(csv.lines().filter(|line| line.starts_with("File,\"")).count(), 1);
        assert!(csv.contains("Node,\"C:\\root\",\"C:\\Parent, ""special""\","));
        assert!(csv.contains("\n\nType,Path,Parent Path,Logical Size,Allocated Size"));
        assert!(!csv.contains("C:\\other"));
        assert!(!csv.contains("file2.txt"));
}
