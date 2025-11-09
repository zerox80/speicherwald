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
///
/// This struct provides mechanisms to control and communicate with a scan job,
/// including cancellation and event broadcasting capabilities.
#[derive(Clone)]
pub struct JobHandle {
    /// A cancellation token for stopping the job.
    /// 
    /// When this token is cancelled, the scan job should gracefully terminate
    /// its operations and clean up any resources.
    pub cancel: CancellationToken,
    /// A broadcast sender for sending scan events.
    ///
    /// Used to emit real-time updates about scan progress, warnings, and completion
    /// to connected clients via Server-Sent Events (SSE).
    pub sender: broadcast::Sender<ScanEvent>,
}

/// The shared application state.
///
/// This struct holds all the core shared data structures that need to be accessed
/// across different parts of the application, including HTTP handlers, middleware,
/// and background tasks. It's designed to be thread-safe and cloneable for use
/// with Axum's request extraction system.
#[derive(Clone)]
pub struct AppState {
    /// The database connection pool.
    ///
    /// Provides connections to the SQLite database for storing scan results,
    /// job metadata, and other persistent data.
    pub db: sqlx::SqlitePool,
    /// A map of running scan jobs.
    ///
    /// Maps scan UUIDs to their corresponding job handles, allowing for
    /// job management, cancellation, and event broadcasting.
    pub jobs: Arc<RwLock<HashMap<Uuid, JobHandle>>>,
    /// The application configuration.
    ///
    /// Contains server settings, database configuration, scan defaults,
    /// and other runtime parameters.
    pub config: Arc<AppConfig>,
    /// The application metrics.
    ///
    /// Tracks performance counters and statistics about scans, files processed,
    /// and other operational metrics.
    pub metrics: Metrics,
    /// The per-endpoint rate limiter.
    ///
    /// Provides rate limiting functionality for different API endpoints
    /// to prevent abuse and ensure fair usage.
    pub rate_limiter: EndpointRateLimiter,
}

impl AppState {
    /// Creates a new `AppState` with initialized components.
    ///
    /// This function sets up the shared application state with database connection,
    /// empty job registry, configuration, metrics, and rate limiting.
    ///
    /// # Arguments
    ///
    /// * `db` - The database connection pool for persistence operations
    /// * `config` - The application configuration containing all runtime settings
    ///
    /// # Returns
    ///
    /// A new `AppState` instance with:
    /// - Database connection pool
    /// - Empty job registry HashMap
    /// - Wrapped configuration in Arc
    /// - Fresh metrics instance
    /// - Rate limiter with default endpoint limits:
    ///   - 60 scans per minute
    ///   - 600 searches per minute  
    ///   - 120 drive lists per minute
    ///   - 30 move operations per minute
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
