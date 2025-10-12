#![allow(dead_code)]
use super::ip::extract_ip_from_headers;
use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct RateLimiter {
    requests: Arc<RwLock<HashMap<IpAddr, Vec<Instant>>>>,
    max_requests: usize,
    window: Duration,
}

impl RateLimiter {
    pub fn new(max_requests: usize, window_seconds: u64) -> Self {
        Self {
            requests: Arc::new(RwLock::new(HashMap::new())),
            max_requests,
            window: Duration::from_secs(window_seconds),
        }
    }

    pub async fn check_rate_limit(&self, ip: IpAddr) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
        let now = Instant::now();
        let mut requests = self.requests.write().await;

        // Get or create entry for this IP
        let timestamps = requests.entry(ip).or_insert_with(Vec::new);

        // Remove old timestamps outside the window (safe against time skew)
        timestamps.retain(|&t| {
            // Handle potential time skew by checking duration validity
            now.checked_duration_since(t)
                .map(|d| d < self.window)
                .unwrap_or(false)
        });

        // Check if rate limit exceeded
        if timestamps.len() >= self.max_requests {
            // Calculate retry_after based on oldest timestamp
            let oldest = timestamps.first().copied().unwrap_or(now);
            let retry_after = if let Some(elapsed) = now.checked_duration_since(oldest) {
                self.window.saturating_sub(elapsed)
            } else {
                // Time went backwards, reset window
                Duration::from_secs(1)
            };

            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "error": {
                        "code": "RATE_LIMITED",
                        "message": format!("Too many requests. Please retry after {} seconds", retry_after.as_secs()),
                    },
                    "retry_after_seconds": retry_after.as_secs(),
                    "status": 429,
                })),
            ));
        }

        // Add current timestamp
        timestamps.push(now);
        Ok(())
    }

    pub async fn cleanup_old_entries(&self) {
        let now = Instant::now();
        let mut requests = self.requests.write().await;

        // Remove IPs with no recent requests (handle time skew)
        requests.retain(|_, timestamps| {
            timestamps.retain(|&t| {
                now.checked_duration_since(t)
                    .map(|d| d < self.window)
                    .unwrap_or(false)
            });
            !timestamps.is_empty()
        });
    }
}

pub async fn rate_limit_middleware(req: Request, next: Next) -> Response {
    // Extract IP address via shared helper
    let ip = extract_ip_from_headers(req.headers());

    // Use global limiter shared across requests
    // Defaults: 1000 req / 60s, can be overridden via env:
    // SPEICHERWALD_RATE_LIMIT_MAX_REQUESTS, SPEICHERWALD_RATE_LIMIT_WINDOW_SECONDS
    lazy_static::lazy_static! {
        static ref GLOBAL_RATE_LIMITER: RateLimiter = {
            let max = std::env::var("SPEICHERWALD_RATE_LIMIT_MAX_REQUESTS")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(1000);
            let win = std::env::var("SPEICHERWALD_RATE_LIMIT_WINDOW_SECONDS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(60);
            RateLimiter::new(max, win)
        };
        static ref GLOBAL_CLEANUP_STARTED: OnceLock<()> = OnceLock::new();
    }

    // Start a periodic cleanup task exactly once to avoid unbounded growth of the
    // in-memory IP map for the global limiter in long-running processes.
    GLOBAL_CLEANUP_STARTED.get_or_init(|| {
        let limiter = GLOBAL_RATE_LIMITER.clone();
        // Configurable cleanup interval
        let cleanup_secs = std::env::var("SPEICHERWALD_RATE_LIMIT_CLEANUP_INTERVAL")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(300)
            .clamp(60, 3600);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(cleanup_secs));
            loop {
                interval.tick().await;
                limiter.cleanup_old_entries().await;
            }
        });
    });

    let limiter: &RateLimiter = &GLOBAL_RATE_LIMITER;

    match limiter.check_rate_limit(ip).await {
        Ok(()) => next.run(req).await,
        Err((status, body)) => (status, body).into_response(),
    }
}

// Per-endpoint rate limiting
#[derive(Clone)]
pub struct EndpointRateLimiter {
    limiters: Arc<RwLock<HashMap<String, RateLimiter>>>,
}

impl Default for EndpointRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl EndpointRateLimiter {
    pub fn new() -> Self {
        Self { limiters: Arc::new(RwLock::new(HashMap::new())) }
    }

    pub fn with_limits(mut self, limits: Vec<(&str, usize, u64)>) -> Self {
        let mut limiters_map = HashMap::new();
        for (endpoint, max_requests, window_seconds) in limits {
            limiters_map.insert(endpoint.to_string(), RateLimiter::new(max_requests, window_seconds));
        }
        self.limiters = Arc::new(RwLock::new(limiters_map));
        self
    }

    pub async fn check_endpoint_limit(
        &self,
        endpoint: &str,
        ip: IpAddr,
    ) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
        let limiters = self.limiters.read().await;

        if let Some(limiter) = limiters.get(endpoint) {
            limiter.check_rate_limit(ip).await
        } else {
            // No specific limit for this endpoint
            Ok(())
        }
    }

    /// Periodically prune old entries from all endpoint-specific limiters
    pub async fn cleanup_all(&self) {
        // Clone out current limiters to avoid holding the read lock across awaits.
        let snapshot: Vec<RateLimiter> = {
            let limiters = self.limiters.read().await;
            limiters.values().cloned().collect()
        };
        for limiter in snapshot {
            limiter.cleanup_old_entries().await;
        }
    }
}

// Cleanup task that runs periodically
pub async fn cleanup_task(limiter: RateLimiter) {
    let mut interval = tokio::time::interval(Duration::from_secs(300)); // Clean every 5 minutes

    loop {
        interval.tick().await;
        limiter.cleanup_old_entries().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter() {
        let limiter = RateLimiter::new(3, 1);
        let ip = IpAddr::from([127, 0, 0, 1]);

        // First 3 requests should succeed
        assert!(limiter.check_rate_limit(ip).await.is_ok());
        assert!(limiter.check_rate_limit(ip).await.is_ok());
        assert!(limiter.check_rate_limit(ip).await.is_ok());

        // 4th request should fail
        assert!(limiter.check_rate_limit(ip).await.is_err());

        // Wait for window to expire
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Should succeed again
        assert!(limiter.check_rate_limit(ip).await.is_ok());
    }

    #[tokio::test]
    async fn test_different_ips() {
        let limiter = RateLimiter::new(1, 1);
        let ip1 = IpAddr::from([127, 0, 0, 1]);
        let ip2 = IpAddr::from([127, 0, 0, 2]);

        // Both IPs should get their own limit
        assert!(limiter.check_rate_limit(ip1).await.is_ok());
        assert!(limiter.check_rate_limit(ip2).await.is_ok());

        // Both should be rate limited on second request
        assert!(limiter.check_rate_limit(ip1).await.is_err());
        assert!(limiter.check_rate_limit(ip2).await.is_err());
    }
}
