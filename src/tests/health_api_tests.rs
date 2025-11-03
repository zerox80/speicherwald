#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        Router,
    };
    use speicherwald::config::AppConfig;
    use speicherwald::middleware::EndpointRateLimiter;
    use speicherwald::routes::health::{healthz, readyz, metrics, metrics_prometheus, version};
    use speicherwald::state::AppState;
    use sqlx::SqlitePool;
    use std::sync::Arc;
    use tower::ServiceExt;
    use axum::routing::get;

    async fn setup_test_app() -> Router {
        let config = AppConfig::default();
        // Use an in-memory SQLite database for tests
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();

        let state = AppState {
            db: pool,
            jobs: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            config: Arc::new(config),
            metrics: speicherwald::metrics::Metrics::new(),
            rate_limiter: EndpointRateLimiter::new(),
        };

        Router::new()
            .route("/healthz", get(healthz))
            .route("/readyz", get(readyz))
            .route("/metrics", get(metrics))
            .route("/metrics/prometheus", get(metrics_prometheus))
            .route("/version", get(version))
            .with_state(state)
    }

    #[tokio::test]
    async fn test_healthz_endpoint() {
        let app = setup_test_app().await;

        let response = app
            .oneshot(Request::builder().uri("/healthz").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        assert_eq!(&body[..], b"ok");
    }

    #[tokio::test]
    async fn test_version_endpoint() {
        let app = setup_test_app().await;

        let response = app
            .oneshot(Request::builder().uri("/version").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["name"], "speicherwald");
        assert!(!v["version"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_readyz_endpoint_ok() {
        let app = setup_test_app().await;

        let response = app
            .oneshot(Request::builder().uri("/readyz").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        assert_eq!(&body[..], b"ready");
    }

    #[tokio::test]
    async fn test_readyz_endpoint_db_error() {
        let config = AppConfig::default();
        // This will fail because the directory does not exist.
        let pool = SqlitePool::connect("sqlite:///nonexistent/path/to/fail.db").await.unwrap();
        pool.close().await;

        let state = AppState {
            db: pool,
            jobs: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            config: Arc::new(config),
            metrics: speicherwald::metrics::Metrics::new(),
            rate_limiter: EndpointRateLimiter::new(),
        };

        let app = Router::new()
            .route("/readyz", get(readyz))
            .with_state(state);

        let response = app
            .oneshot(Request::builder().uri("/readyz").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("not ready"));
    }


    #[tokio::test]
    async fn test_metrics_endpoint() {
        let app = setup_test_app().await;

        let response = app
            .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["scans_started"], 0);
        assert_eq!(v["bytes_scanned"], 0);
    }

    #[tokio::test]
    async fn test_metrics_prometheus_endpoint() {
        let app = setup_test_app().await;

        let response = app
            .oneshot(Request::builder().uri("/metrics/prometheus").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_str.contains("speicherwald_scans_started 0"));
        assert!(body_str.contains("speicherwald_bytes_scanned 0"));
        assert!(body_str.contains("# TYPE speicherwald_uptime_seconds gauge"));
    }
}