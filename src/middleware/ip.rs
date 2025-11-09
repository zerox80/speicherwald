//! IP address extraction utilities for HTTP requests.
//!
//! This module provides utilities for extracting client IP addresses from HTTP requests,
//! with support for proxy headers and fallback mechanisms. It includes extractors
//! that work reliably in various deployment scenarios including behind load balancers,
//! reverse proxies, and in test environments.

use axum::{
    extract::{connect_info::ConnectInfo, FromRequestParts},
    http::{request::Parts, HeaderMap},
};
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};

/// Extracts the client's IP address from common proxy headers.
///
/// This function checks for IP addresses in proxy headers in the following order:
/// 1. `x-forwarded-for` - Standard header for client IP through proxies/load balancers
/// 2. `x-real-ip` - Common header used by Nginx and other proxies
/// 3. Fallback IP address if provided
/// 4. Default to `127.0.0.1` if no fallback is provided
///
/// The `x-forwarded-for` header can contain multiple IPs (client, proxy1, proxy2, ...),
/// in which case the first IP is the original client IP.
///
/// # Arguments
///
/// * `headers` - A `HeaderMap` containing the request headers
/// * `fallback` - An optional `IpAddr` to use if no proxy headers are found
///
/// # Returns
///
/// * `IpAddr` - The extracted client IP address or fallback/default value
///
/// # Examples
///
/// ```rust
/// use axum::http::HeaderMap;
/// use std::net::IpAddr;
/// 
/// let mut headers = HeaderMap::new();
/// headers.insert("x-forwarded-for", "203.0.113.1, 10.0.0.1");
/// 
/// let client_ip = extract_ip_from_headers(&headers, None);
/// assert_eq!(client_ip, IpAddr::V4(203, 0, 113, 1));
/// ```
pub fn extract_ip_from_headers(headers: &HeaderMap, fallback: Option<IpAddr>) -> IpAddr {
    if let Some(h) = headers.get("x-forwarded-for").and_then(|hv| hv.to_str().ok()) {
        if let Some(first) = h.split(',').next() {
            if let Ok(ip) = first.trim().parse::<IpAddr>() {
                return ip;
            }
        }
    }
    if let Some(h) = headers.get("x-real-ip").and_then(|hv| hv.to_str().ok()) {
        if let Ok(ip) = h.parse::<IpAddr>() {
            return ip;
        }
    }
    if let Some(ip) = fallback {
        return ip;
    }
    IpAddr::from([127, 0, 0, 1])
}

/// An extractor for the remote socket address that never rejects requests.
///
/// This extractor attempts to extract the remote socket address from the request,
/// but unlike Axum's built-in `ConnectInfo`, it returns `None` instead of
/// rejecting the request when connection information is not available.
///
/// This is useful in environments where the connection information might be missing,
/// such as:
/// - Test environments
/// - Running behind certain proxies or load balancers
/// - When using request forwarding mechanisms
/// - Serverless environments
///
/// # Examples
///
/// ```rust
/// use axum::{extract::Request, middleware::Next, response::Response};
/// 
/// async fn handler(addr: MaybeRemoteAddr) -> String {
///     match addr.0 {
///         Some(socket_addr) => format!("Connected from {}", socket_addr),
///         None => "Connection info unavailable".to_string(),
///     }
/// }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct MaybeRemoteAddr(pub Option<SocketAddr>);

impl<S> FromRequestParts<S> for MaybeRemoteAddr
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    /// Attempts to extract the remote socket address from request parts.
    ///
    /// This implementation wraps Axum's `ConnectInfo` extractor but catches
    /// any extraction errors and returns `None` instead of rejecting the request.
    ///
    /// # Arguments
    ///
    /// * `parts` - The request parts containing connection information
    /// * `state` - The request state
    ///
    /// # Returns
    ///
    /// `Ok(Self)` containing `Some(SocketAddr)` if extraction succeeds,
    /// or `None` if connection information is not available
    fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        async move {
            match ConnectInfo::<SocketAddr>::from_request_parts(parts, state).await {
                Ok(ConnectInfo(addr)) => Ok(MaybeRemoteAddr(Some(addr))),
                Err(_) => Ok(MaybeRemoteAddr(None)),
            }
        }
    }
}
