#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        Router,
    };
    use speicherwald::config::AppConfig;
    use speicherwald::middleware::rate_limit::EndpointRateLimiter;
    use speicherwald::routes::drives::list_drives;
    use speicherwald::state::AppState;
    use sqlx::SqlitePool;
    use tower::ServiceExt;
    use axum::routing::get;

    async fn setup_test_app(rate_limiter: EndpointRateLimiter) -> Router {
        let config = AppConfig::default();
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();

        let mut state = AppState::new(pool, config);
        state.rate_limiter = rate_limiter;

        Router::new()
            .route("/api/drives", get(list_drives))
            .with_state(state)
    }

    // Helper to create a rate limiter with specific limits for testing
    fn create_rate_limiter(max_requests: usize, window_seconds: u64) -> EndpointRateLimiter {
        EndpointRateLimiter::new().with_limits(vec![("/drives", max_requests, window_seconds)])
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn test_list_drives_endpoint_non_windows() {
        let rate_limiter = create_rate_limiter(10, 10);
        let app = setup_test_app(rate_limiter).await;

        let response = app
            .oneshot(Request::builder().uri("/api/drives").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = hyper::body::to_bytes(response.into_body()).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(v["items"].is_array());
        assert!(v["items"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_drives_endpoint_rate_limiting() {
        // Configure a very strict rate limiter: 1 request per second, burst of 1
        let rate_limiter = create_rate_limiter(1, 1);
        let app = setup_test_app(rate_limiter).await;

        // First request should succeed
        let response1 = app.clone()
            .oneshot(Request::builder().uri("/api/drives").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response1.status(), StatusCode::OK);

        // Second request immediately after should be rate-limited
        let response2 = app
            .oneshot(Request::builder().uri("/api/drives").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response2.status(), StatusCode::TOO_MANY_REQUESTS);
    }
}