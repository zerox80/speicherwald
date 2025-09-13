use axum::{
    extract::Request,
    http::{HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};

/// Adds standard security-related HTTP headers to all responses.
/// Conservative defaults chosen to avoid breaking the Web UI.
pub async fn security_headers_middleware(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let headers = res.headers_mut();

    // X-Content-Type-Options: nosniff
    headers.insert(HeaderName::from_static("x-content-type-options"), HeaderValue::from_static("nosniff"));

    // X-Frame-Options: SAMEORIGIN
    headers.insert(HeaderName::from_static("x-frame-options"), HeaderValue::from_static("SAMEORIGIN"));

    // Referrer-Policy: no-referrer
    headers.insert(HeaderName::from_static("referrer-policy"), HeaderValue::from_static("no-referrer"));

    // Permissions-Policy: disable sensitive APIs by default
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("geolocation=(), microphone=(), camera=()"),
    );

    // COOP/ CORP to reduce cross-origin risks
    headers.insert(
        HeaderName::from_static("cross-origin-opener-policy"),
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        HeaderName::from_static("cross-origin-resource-policy"),
        HeaderValue::from_static("same-origin"),
    );

    // Note: Not setting HSTS/CSP here to avoid breaking setups behind HTTP or strict CSP needs.
    // Consider making CSP and HSTS configurable via AppConfig.

    res
}
