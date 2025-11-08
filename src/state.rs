#![allow(dead_code)]
use std::{collections::HashMap, sync::Arc};

use tokio::sync::{broadcast, RwLock};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::metrics::Metrics;
use crate::middleware::EndpointRateLimiter;
use crate::types::ScanEvent;

/// A handle to a running scan job.
#[derive(Clone)]
pub struct JobHandle {
    /// A cancellation token for stopping the job.
    pub cancel: CancellationToken,
    /// A broadcast sender for sending scan events.
    pub sender: broadcast::Sender<ScanEvent>,
}

/// The shared application state.
///
/// This struct holds the database connection pool, a map of running jobs,
/// the application configuration, and metrics.
#[derive(Clone)]
pub struct AppState {
    /// The database connection pool.
    pub db: sqlx::SqlitePool,
    /// A map of running scan jobs.
    pub jobs: Arc<RwLock<HashMap<Uuid, JobHandle>>>,
    /// The application configuration.
    pub config: Arc<AppConfig>,
    /// The application metrics.
    pub metrics: Metrics,
    /// The per-endpoint rate limiter.
    pub rate_limiter: EndpointRateLimiter,
}

impl AppState {
    /// Creates a new `AppState`.
    ///
    /// # Arguments
    ///
    /// * `db` - The database connection pool.
    /// * `config` - The application configuration.
    pub fn new(db: sqlx::SqlitePool, config: AppConfig) -> Self {
        let rate_limiter = EndpointRateLimiter::new().with_limits(vec![
            ("/scans", 60, 60),             // 60 scans per minute
            ("/scans/:id/search", 600, 60), // 600 searches per minute
            ("/drives", 120, 60),           // 120 drive lists per minute
            ("/paths/move", 30, 60),        // 30 move operations per minute
        ]);

        Self {
            db,
            jobs: Arc::new(RwLock::new(HashMap::new())),
            config: Arc::new(config),
            metrics: Metrics::new(),
            rate_limiter,
        }
    }
}
