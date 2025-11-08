// FIX Bug #20: Removed dead_code annotation
use super::ip::extract_ip_from_headers;
use axum::{
    extract::{connect_info::ConnectInfo, Request},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use tokio::sync::RwLock;

/// A thread-safe rate limiter based on the sliding window algorithm.
#[derive(Clone)]
pub struct RateLimiter {
    requests: Arc<RwLock<HashMap<IpAddr, Vec<Instant>>>>,
    max_requests: usize,
    window: Duration,
}

impl RateLimiter {
    /// Creates a new `RateLimiter`.
    ///
    /// # Arguments
    ///
    /// * `max_requests` - The maximum number of requests allowed within the time window.
    /// * `window_seconds` - The duration of the time window in seconds.
    pub fn new(max_requests: usize, window_seconds: u64) -> Self {
        Self {
            requests: Arc::new(RwLock::new(HashMap::new())),
            max_requests,
            window: Duration::from_secs(window_seconds),
        }
    }

    /// Checks if a request from a given IP address is allowed.
    ///
    /// If the request is allowed, it is recorded and `Ok(())` is returned.
    /// If the request is rate-limited, an `Err` containing the HTTP response is returned.
    ///
    /// # Arguments
    ///
    /// * `ip` - The IP address of the client.
    pub async fn check_rate_limit(&self, ip: IpAddr) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
        let now = Instant::now();
        let mut requests = self.requests.write().await;

        // Get or create entry for this IP
        let timestamps = requests.entry(ip).or_insert_with(Vec::new);

        // Remove old timestamps outside the window (safe against time skew)
        timestamps.retain(|&t| {
            // FIX Bug #6: On time skew, keep the timestamp (conservative approach)
            // This prevents incorrectly allowing rate-limited requests when clock jumps backward
            now.checked_duration_since(t).map(|d| d < self.window).unwrap_or(true)
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

    /// Removes old entries from the rate limiter's storage.
    ///
    /// This function iterates through the stored IP addresses and removes timestamps
    /// that are outside the time window.
    pub async fn cleanup_old_entries(&self) {
        let now = Instant::now();
        let mut requests = self.requests.write().await;

        // Remove IPs with no recent requests (handle time skew)
        requests.retain(|_, timestamps| {
            timestamps.retain(|&t| now.checked_duration_since(t).map(|d| d < self.window).unwrap_or(true));
            !timestamps.is_empty()
        });
    }
}

/// An Axum middleware for global rate limiting.
///
/// This middleware uses a global `RateLimiter` instance to limit the number of
/// requests per IP address.
///
/// # Arguments
///
/// * `req` - The incoming `Request`.
/// * `next` - The next middleware in the chain.
///
/// # Returns
///
/// * `Response` - The response from the next middleware, or a `429 Too Many Requests`
///  response if the client is rate-limited.
pub async fn rate_limit_middleware(req: Request, next: Next) -> Response {
    // Extract IP address via shared helper
    let remote_ip = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|info| info.0.ip());
    let ip = extract_ip_from_headers(req.headers(), remote_ip);

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

    // FIX Bug #22: Cleanup task is already spawned in main.rs, don't duplicate it here
    // Note: The per-endpoint rate limiter cleanup in main.rs:98-113 handles this
    // Start a periodic cleanup task exactly once to avoid unbounded growth of the
    // in-memory IP map for the global limiter in long-running processes.
    GLOBAL_CLEANUP_STARTED.get_or_init(|| {
        let limiter = GLOBAL_RATE_LIMITER.clone();
        // Configurable cleanup interval
        let cleanup_secs = std::env::var("SPEICHERWALD_GLOBAL_RATE_LIMIT_CLEANUP_INTERVAL")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(600) // Default to 10 minutes for global limiter
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

/// A manager for per-endpoint rate limiters.
///
/// This struct holds a collection of `RateLimiter` instances, each associated with a
/// specific endpoint.
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
    /// Creates a new, empty `EndpointRateLimiter`.
    pub fn new() -> Self {
        Self { limiters: Arc::new(RwLock::new(HashMap::new())) }
    }

    /// Configures the rate limiter with a set of endpoint-specific limits.
    ///
    /// This method extends the existing limits rather than replacing them. If an endpoint
    /// already has a limit, it will be updated.
    ///
    /// # Arguments
    ///
    /// * `limits` - A vector of tuples, where each tuple contains the endpoint path,
    ///   the maximum number of requests, and the time window in seconds.
    pub fn with_limits(self, limits: Vec<(&str, usize, u64)>) -> Self {
        // Extract existing limiters or create new HashMap
        let mut limiters_map = match Arc::try_unwrap(self.limiters) {
            Ok(rwlock) => rwlock.into_inner(),
            Err(arc) => arc
                .try_read()
                .map(|guard| guard.clone())
                .unwrap_or_else(|_| HashMap::new()),
        };
        
        // Add/update new limits
        for (endpoint, max_requests, window_seconds) in limits {
            limiters_map.insert(endpoint.to_string(), RateLimiter::new(max_requests, window_seconds));
        }
        
        Self {
            limiters: Arc::new(RwLock::new(limiters_map))
        }
    }

    /// Checks if a request to a specific endpoint from a given IP address is allowed.
    ///
    /// # Arguments
    ///
    /// * `endpoint` - The path of the endpoint being accessed.
    /// * `ip` - The IP address of the client.
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

    /// Cleans up old entries from all endpoint-specific rate limiters.
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

/// A background task that periodically cleans up old entries from a `RateLimiter`.
///
/// # Arguments
///
/// * `limiter` - The `RateLimiter` to clean up.
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
