use serde::Serialize;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// A collection of atomic counters for tracking application performance metrics.
///
/// This struct is thread-safe and can be shared across multiple threads.
#[derive(Clone)]
pub struct Metrics {
    /// The total number of scans that have been started.
    pub scans_started: Arc<AtomicUsize>,
    /// The total number of scans that have completed successfully.
    pub scans_completed: Arc<AtomicUsize>,
    /// The total number of scans that have failed.
    pub scans_failed: Arc<AtomicUsize>,
    /// The total number of files processed across all scans.
    pub files_processed: Arc<AtomicU64>,
    /// The total number of directories processed across all scans.
    pub dirs_processed: Arc<AtomicU64>,
    /// The total number of bytes scanned across all scans.
    pub bytes_scanned: Arc<AtomicU64>,
    /// The total number of warnings generated across all scans.
    pub warnings_count: Arc<AtomicUsize>,
    /// The time at which the application was started.
    pub start_time: Instant,
}

impl Metrics {
    /// Creates a new `Metrics` instance with all counters initialized to zero.
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

    /// Increments the `scans_started` counter by one.
    pub fn inc_scans_started(&self) {
        self.scans_started.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the `scans_completed` counter by one.
    pub fn inc_scans_completed(&self) {
        self.scans_completed.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the `scans_failed` counter by one.
    pub fn inc_scans_failed(&self) {
        self.scans_failed.fetch_add(1, Ordering::Relaxed);
    }

    /// Adds the given count to the `files_processed` counter.
    pub fn add_files(&self, count: u64) {
        self.files_processed.fetch_add(count, Ordering::Relaxed);
    }

    /// Adds the given count to the `dirs_processed` counter.
    pub fn add_dirs(&self, count: u64) {
        self.dirs_processed.fetch_add(count, Ordering::Relaxed);
    }

    /// Adds the given number of bytes to the `bytes_scanned` counter.
    pub fn add_bytes(&self, bytes: u64) {
        self.bytes_scanned.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Adds the given count to the `warnings_count` counter.
    pub fn add_warnings(&self, count: usize) {
        self.warnings_count.fetch_add(count, Ordering::Relaxed);
    }

    /// Returns a snapshot of the current metrics.
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

/// A snapshot of the application metrics at a specific point in time.
#[derive(Serialize)]
pub struct MetricsSnapshot {
    /// The total number of scans that have been started.
    pub scans_started: usize,
    /// The total number of scans that have completed successfully.
    pub scans_completed: usize,
    /// The total number of scans that have failed.
    pub scans_failed: usize,
    /// The total number of files processed across all scans.
    pub files_processed: u64,
    /// The total number of directories processed across all scans.
    pub dirs_processed: u64,
    /// The total number of bytes scanned across all scans.
    pub bytes_scanned: u64,
    /// The total number of warnings generated across all scans.
    pub warnings_count: usize,
    /// The uptime of the application in seconds.
    pub uptime_seconds: u64,
}
