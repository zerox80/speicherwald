use axum::http::HeaderMap;
use std::net::IpAddr;

/// Extract client IP from standard proxy headers, falling back to 127.0.0.1
pub fn extract_ip_from_headers(headers: &HeaderMap) -> IpAddr {
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
    IpAddr::from([127, 0, 0, 1])
}
