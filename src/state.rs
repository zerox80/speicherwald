#![allow(dead_code)]
use std::{collections::HashMap, sync::Arc};

use tokio::sync::{broadcast, RwLock};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::metrics::Metrics;
use crate::middleware::EndpointRateLimiter;
use crate::types::ScanEvent;

#[derive(Clone)]
pub struct JobHandle {
    pub cancel: CancellationToken,
    pub sender: broadcast::Sender<ScanEvent>,
}

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::SqlitePool,
    pub jobs: Arc<RwLock<HashMap<Uuid, JobHandle>>>,
    pub config: Arc<AppConfig>,
    pub metrics: Metrics,
    pub rate_limiter: EndpointRateLimiter,
}

impl AppState {
    pub fn new(db: sqlx::SqlitePool, config: AppConfig) -> Self {
        let rate_limiter = EndpointRateLimiter::new().with_limits(vec![
            ("/scans", 60, 60),             // 60 scans per minute
            ("/scans/:id/search", 600, 60), // 600 searches per minute
            ("/drives", 120, 60),           // 120 drive lists per minute
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
