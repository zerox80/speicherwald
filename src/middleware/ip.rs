use axum::http::HeaderMap;
use std::net::IpAddr;

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
