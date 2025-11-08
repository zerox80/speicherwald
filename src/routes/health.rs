use crate::state::AppState;
use axum::{
    extract::State,
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};

/// A simple health check endpoint.
///
/// This endpoint can be used to check if the application is running. It does not
/// perform any external checks (e.g., database connectivity).
pub async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

/// A readiness probe that checks for database connectivity.
///
/// This endpoint is used to determine if the application is ready to handle requests.
/// It performs a simple query to the database to ensure a connection can be established.
pub async fn readyz(State(state): State<AppState>) -> impl IntoResponse {
    // Add timeout to prevent hanging readiness checks
    let query = sqlx::query("SELECT 1").fetch_one(&state.db);
    match tokio::time::timeout(std::time::Duration::from_secs(5), query).await {
        Ok(Ok(_)) => (StatusCode::OK, "ready").into_response(),
        Ok(Err(e)) => (StatusCode::SERVICE_UNAVAILABLE, format!("not ready: {}", e)).into_response(),
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, "not ready: timeout").into_response(),
    }
}

/// Returns a JSON snapshot of the application's metrics.
pub async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    let snapshot = state.metrics.get_snapshot();
    Json(snapshot)
}

/// Returns the application's metrics in Prometheus exposition format.
pub async fn metrics_prometheus(State(state): State<AppState>) -> impl IntoResponse {
    let m = state.metrics.get_snapshot();
    let body = format!(
        "# HELP speicherwald_scans_started Total scans started\n# TYPE speicherwald_scans_started counter\nspeicherwald_scans_started {}\n\
# HELP speicherwald_scans_completed Total scans completed\n# TYPE speicherwald_scans_completed counter\nspeicherwald_scans_completed {}\n\
# HELP speicherwald_scans_failed Total scans failed\n# TYPE speicherwald_scans_failed counter\nspeicherwald_scans_failed {}\n\
# HELP speicherwald_files_processed Files processed\n# TYPE speicherwald_files_processed counter\nspeicherwald_files_processed {}\n\
# HELP speicherwald_dirs_processed Directories processed\n# TYPE speicherwald_dirs_processed counter\nspeicherwald_dirs_processed {}\n\
# HELP speicherwald_bytes_scanned Bytes scanned\n# TYPE speicherwald_bytes_scanned counter\nspeicherwald_bytes_scanned {}\n\
# HELP speicherwald_warnings_count Warnings count\n# TYPE speicherwald_warnings_count counter\nspeicherwald_warnings_count {}\n\
# HELP speicherwald_uptime_seconds Uptime seconds\n# TYPE speicherwald_uptime_seconds gauge\nspeicherwald_uptime_seconds {}\n",
        m.scans_started,
        m.scans_completed,
        m.scans_failed,
        m.files_processed,
        m.dirs_processed,
        m.bytes_scanned,
        m.warnings_count,
        m.uptime_seconds,
    );
    ([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], body)
}

/// Returns the application's version and build information.
pub async fn version() -> impl IntoResponse {
    let body = serde_json::json!({
        "name": env!("CARGO_PKG_NAME"),
        "version": env!("CARGO_PKG_VERSION"),
        "package": {
            "description": env!("CARGO_PKG_DESCRIPTION"),
            "authors": env!("CARGO_PKG_AUTHORS"),
            "license": env!("CARGO_PKG_LICENSE"),
        },
        "build": {
            "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
        }
    });
    (StatusCode::OK, Json(body))
}
