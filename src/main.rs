use std::net::SocketAddr;

use axum::extract::DefaultBodyLimit;
use axum::http::header::CONTENT_TYPE;
use axum::middleware::{from_fn, from_fn_with_state};
use axum::{
    routing::{get, post},
    Router,
};
use sqlx::{migrate::MigrateDatabase, sqlite::SqlitePoolOptions, Sqlite};
use tokio::time::{self, Duration as TokioDuration};
use tower_http::compression::predicate::{DefaultPredicate, Predicate};
use tower_http::{
    compression::CompressionLayer,
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod db;
mod error;
mod metrics;
mod middleware;
mod routes;
mod scanner;
mod state;
mod types;

use state::AppState;

const UI_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");
const UI_INDEX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui/index.html");

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Logging (stdout + tägliche Datei-Rotation unter ./logs)
    std::fs::create_dir_all("logs").ok();
    let (stdout_nb, stdout_guard) = tracing_appender::non_blocking(std::io::stdout());
    let file_appender = tracing_appender::rolling::daily("logs", "speicherwald.log");
    let (file_nb, file_guard) = tracing_appender::non_blocking(file_appender);
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,tower_http=info".into());
    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer().with_writer(stdout_nb))
        .with(tracing_subscriber::fmt::layer().with_ansi(false).with_writer(file_nb))
        .init();
    // Guards am Leben halten (nicht fallen lassen), damit Non-Blocking Writer korrekt flushen
    let _log_guards = (stdout_guard, file_guard);

    // Load configuration (embedded defaults -> speicherwald.toml -> env/.env)
    let app_cfg = config::load()?;

    // Prepare data dir (if sqlite)
    let db_url = &app_cfg.database.url;
    config::ensure_sqlite_parent_dir(db_url)?;
    if !Sqlite::database_exists(db_url).await.unwrap_or(false) {
        info!("Creating SQLite database at {}", db_url);
        Sqlite::create_database(db_url).await?;
    }
    // Configurable max connections via environment variable with error logging
    let max_conns = std::env::var("SPEICHERWALD_DB_MAX_CONNECTIONS")
        .ok()
        .and_then(|v| {
            v.parse::<u32>()
                .map_err(|e| {
                    tracing::warn!("Invalid SPEICHERWALD_DB_MAX_CONNECTIONS value '{}': {}", v, e);
                    e
                })
                .ok()
        })
        .unwrap_or(16)
        .clamp(1, 64);
    let pool = SqlitePoolOptions::new()
        .max_connections(max_conns)
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                let _ = sqlx::query("PRAGMA foreign_keys=ON;").execute(&mut *conn).await;
                let _ = sqlx::query("PRAGMA busy_timeout=10000;").execute(&mut *conn).await;
                let _ = sqlx::query("PRAGMA cache_size=-65536;").execute(&mut *conn).await; // ~64MB page cache
                let _ = sqlx::query("PRAGMA temp_store=MEMORY;").execute(&mut *conn).await;
                let _ = sqlx::query("PRAGMA mmap_size=268435456;").execute(&mut *conn).await; // 256MB mmap
                Ok(())
            })
        })
        .connect(db_url)
        .await?;

    // Initialize DB schema
    db::init_db(&pool).await?;

    // App state (includes rate limiting)
    let state = AppState::new(pool.clone(), app_cfg.clone());

    // Spawn periodic cleanup for per-endpoint rate limiters to avoid memory growth
    {
        let rl = state.rate_limiter.clone();
        // Configurable cleanup interval
        let cleanup_secs = std::env::var("SPEICHERWALD_RATE_LIMIT_CLEANUP_INTERVAL")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(300)
            .clamp(60, 3600);
        tokio::spawn(async move {
            let mut ticker = time::interval(TokioDuration::from_secs(cleanup_secs));
            loop {
                ticker.tick().await;
                rl.cleanup_all().await;
            }
        });
    }

    // Static file service für Web UI mit SPA-Fallback
    // Priorisiere Laufzeitpfad relativ zum Binary (<exe_dir>/ui), fallback auf Build-Zeit-Pfade
    let (ui_root, ui_index) = {
        let runtime_ui = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("ui")))
            .unwrap_or_else(|| std::path::PathBuf::from("ui"));
        let runtime_index = runtime_ui.join("index.html");
        if runtime_ui.is_dir() && runtime_index.is_file() {
            (runtime_ui, runtime_index)
        } else {
            (std::path::PathBuf::from(UI_DIR), std::path::PathBuf::from(UI_INDEX))
        }
    };
    let static_ui_service = ServeDir::new(ui_root)
        .append_index_html_on_directories(true)
        .not_found_service(ServeFile::new(ui_index));

    // Router
    // Build compression layer but exclude SSE (text/event-stream) to avoid breaking live streams.
    #[derive(Clone)]
    struct NoSseDefault(DefaultPredicate);
    impl Predicate for NoSseDefault {
        fn should_compress<B: axum::body::HttpBody>(&self, res: &axum::http::Response<B>) -> bool {
            if let Some(ct) = res.headers().get(CONTENT_TYPE) {
                if let Ok(s) = ct.to_str() {
                    // Also exclude chunked responses for better compatibility
                    if s.starts_with("text/event-stream") || s.starts_with("multipart/") {
                        return false;
                    }
                }
            }
            self.0.should_compress(res)
        }
    }
    let compression = CompressionLayer::new().compress_when(NoSseDefault(DefaultPredicate::new()));

    // Clone config Arc for stateful middleware
    let cfg_arc = state.config.clone();

    // FIX Bug #32: Configure per-endpoint rate limits
    // Note: Only static routes work with string-based endpoint matching.
    // Dynamic routes like /scans/{id}/events cannot be matched this way.
    // TODO: Implement pattern-based route matching for per-endpoint rate limiting
    let state_with_limits = {
        let mut s = state.clone();
        s.rate_limiter = s.rate_limiter.with_limits(vec![
            ("/scans", 30, 60),           // 30 requests per minute for scan creation
            ("/paths/move", 10, 60),      // 10 move operations per minute
            // Removed: ("/scans/{id}/events", ...) - doesn't work with parametrized routes
        ]);
        s
    };

    let app = Router::new()
        .route("/healthz", get(routes::health::healthz))
        .route("/readyz", get(routes::health::readyz))
        .route("/metrics", get(routes::health::metrics))
        .route("/metrics/prometheus", get(routes::health::metrics_prometheus))
        .route("/version", get(routes::health::version))
        .route("/scans", post(routes::scans::create_scan).get(routes::scans::list_scans))
        .route("/scans/{id}", get(routes::scans::get_scan).delete(routes::scans::cancel_scan))
        .route("/scans/{id}/events", get(routes::scans::scan_events))
        .route("/scans/{id}/tree", get(routes::scans::get_tree))
        .route("/scans/{id}/top", get(routes::scans::get_top))
        .route("/scans/{id}/list", get(routes::scans::get_list))
        .route("/scans/{id}/recent", get(routes::scans::get_recent))
        .route("/scans/{id}/search", get(routes::search::search_scan))
        .route("/scans/{id}/export", get(routes::export::export_scan))
        .route("/scans/{id}/statistics", get(routes::export::export_statistics))
        .route("/drives", get(routes::drives::list_drives))
        .route("/paths/move", post(routes::paths::move_path))
        .fallback_service(static_ui_service)
        .with_state(state_with_limits)
        // Globales Body-Limit – schützt vor übergroßen Requests (configurable via env)
        .layer(DefaultBodyLimit::max(
            std::env::var("SPEICHERWALD_MAX_BODY_SIZE")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(10 * 1024 * 1024)
                .clamp(1024 * 1024, 50 * 1024 * 1024), // 1MB to 50MB
        ))
        .layer(from_fn(middleware::validation::validate_request_middleware))
        .layer(from_fn(middleware::rate_limit::rate_limit_middleware))
        .layer(compression)
        .layer(TraceLayer::new_for_http())
        .layer(from_fn_with_state(cfg_arc, middleware::security_headers::security_headers_middleware));

    // CORS: in Debug permissiv (für lokale Entwicklung mit separater UI), in Release nicht nötig (same-origin)
    let app = if cfg!(debug_assertions) { app.layer(CorsLayer::permissive()) } else { app };

    // Server listen addr (from config)
    let port: u16 = app_cfg.server.port;
    let host: String = app_cfg.server.host.clone();
    let addr: SocketAddr = format!("{}:{}", host, port)
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid listen addr {}:{} - {}", host, port, e))?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    info!("SpeicherWald listening on http://{}", listener.local_addr()?);
    let make_service = app.into_make_service_with_connect_info::<SocketAddr>();
    axum::serve(listener, make_service).with_graceful_shutdown(shutdown_signal()).await?;

    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = term.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
    info!("Shutdown signal received. Stopping server...");
    // Small delay to allow log buffers to flush
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
}
