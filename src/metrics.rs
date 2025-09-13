use serde::Serialize;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Performance metrics for monitoring
#[derive(Clone)]
pub struct Metrics {
    pub scans_started: Arc<AtomicUsize>,
    pub scans_completed: Arc<AtomicUsize>,
    pub scans_failed: Arc<AtomicUsize>,
    pub files_processed: Arc<AtomicU64>,
    pub dirs_processed: Arc<AtomicU64>,
    pub bytes_scanned: Arc<AtomicU64>,
    pub warnings_count: Arc<AtomicUsize>,
    pub start_time: Instant,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            scans_started: Arc::new(AtomicUsize::new(0)),
            scans_completed: Arc::new(AtomicUsize::new(0)),
            scans_failed: Arc::new(AtomicUsize::new(0)),
            files_processed: Arc::new(AtomicU64::new(0)),
            dirs_processed: Arc::new(AtomicU64::new(0)),
            bytes_scanned: Arc::new(AtomicU64::new(0)),
            warnings_count: Arc::new(AtomicUsize::new(0)),
            start_time: Instant::now(),
        }
    }

    pub fn inc_scans_started(&self) {
        self.scans_started.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_scans_completed(&self) {
        self.scans_completed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_scans_failed(&self) {
        self.scans_failed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_files(&self, count: u64) {
        self.files_processed.fetch_add(count, Ordering::Relaxed);
    }

    pub fn add_dirs(&self, count: u64) {
        self.dirs_processed.fetch_add(count, Ordering::Relaxed);
    }

    pub fn add_bytes(&self, bytes: u64) {
        self.bytes_scanned.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn add_warnings(&self, count: usize) {
        self.warnings_count.fetch_add(count, Ordering::Relaxed);
    }

    pub fn get_snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            scans_started: self.scans_started.load(Ordering::Relaxed),
            scans_completed: self.scans_completed.load(Ordering::Relaxed),
            scans_failed: self.scans_failed.load(Ordering::Relaxed),
            files_processed: self.files_processed.load(Ordering::Relaxed),
            dirs_processed: self.dirs_processed.load(Ordering::Relaxed),
            bytes_scanned: self.bytes_scanned.load(Ordering::Relaxed),
            warnings_count: self.warnings_count.load(Ordering::Relaxed),
            uptime_seconds: self.start_time.elapsed().as_secs(),
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Serialize)]
pub struct MetricsSnapshot {
    pub scans_started: usize,
    pub scans_completed: usize,
    pub scans_failed: usize,
    pub files_processed: u64,
    pub dirs_processed: u64,
    pub bytes_scanned: u64,
    pub warnings_count: usize,
    pub uptime_seconds: u64,
}
