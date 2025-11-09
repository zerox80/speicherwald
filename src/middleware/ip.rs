use axum::{
    extract::{connect_info::ConnectInfo, FromRequestParts},
    http::{request::Parts, HeaderMap},
};
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};

/// Extracts the client's IP address from common proxy headers.
///
/// This function checks for the `x-forwarded-for` and `x-real-ip` headers in that order.
/// If neither header is present, it returns the fallback IP address. If no fallback is
/// provided, it defaults to `127.0.0.1`.
///
/// # Arguments
///
/// * `headers` - A `HeaderMap` containing the request headers.
/// * `fallback` - An optional `IpAddr` to use if no proxy headers are found.
///
/// # Returns
///
/// * `IpAddr` - The extracted or fallback IP address.
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

/// An extractor for the remote socket address that does not reject the request
/// if the connection information is not available.
///
/// This is useful in environments where the connection information might be missing,
/// such as in tests or when running behind certain proxies.
#[derive(Clone, Copy, Debug, Default)]
pub struct MaybeRemoteAddr(pub Option<SocketAddr>);

impl<S> FromRequestParts<S> for MaybeRemoteAddr
where
    S: Send + Sync,
{
    type Rejection = Infallible;

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
