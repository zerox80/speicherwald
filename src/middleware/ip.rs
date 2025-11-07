use axum::{
    extract::{connect_info::ConnectInfo, FromRequestParts},
    http::{request::Parts, HeaderMap},
};
use async_trait::async_trait;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};

/// Extract client IP from proxy headers and optional transport metadata.
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

/// Optional extractor for remote socket address. Unlike `ConnectInfo`, this never rejects
/// if the connection info extension is absent (e.g. in tests or custom services).
#[derive(Clone, Copy, Debug, Default)]
pub struct MaybeRemoteAddr(pub Option<SocketAddr>);

#[async_trait]
impl<S> FromRequestParts<S> for MaybeRemoteAddr
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match ConnectInfo::<SocketAddr>::from_request_parts(parts, state).await {
            Ok(ConnectInfo(addr)) => Ok(MaybeRemoteAddr(Some(addr))),
            Err(_) => Ok(MaybeRemoteAddr(None)),
        }
    }
}
