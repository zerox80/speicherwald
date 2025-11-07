use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, PRAGMA};
use axum::{
    extract::{Request, State},
    http::{HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;

use crate::config::AppConfig;

/// Adds standard security-related HTTP headers to all responses.
/// Conservative defaults chosen to avoid breaking the Web UI.
pub async fn security_headers_middleware(
    State(cfg): State<Arc<AppConfig>>,
    req: Request,
    next: Next,
) -> Response {
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

    // Optional: HSTS & CSP via configuration
    if let Some(sec) = cfg.security.as_ref() {
        if sec.enable_hsts.unwrap_or(false) {
            let max_age = sec.hsts_max_age.unwrap_or(31536000); // 1 year
            let include_sub =
                if sec.hsts_include_subdomains.unwrap_or(false) { "; includeSubDomains" } else { "" };
            let value = format!("max-age={}{}", max_age, include_sub);
            headers.insert(
                HeaderName::from_static("strict-transport-security"),
                HeaderValue::from_str(&value).unwrap_or(HeaderValue::from_static("max-age=31536000")),
            );
        }
        if let Some(csp) = &sec.csp {
            if !csp.trim().is_empty() {
                if let Ok(val) = HeaderValue::from_str(csp) {
                    headers.insert(HeaderName::from_static("content-security-policy"), val);
                }
            }
        }
    }

    // Defensive caching policy for API responses (JSON) and SSE streams only
    // FIX Bug #38: Better handling of invalid UTF-8 in Content-Type
    let ct_val: Option<String> = headers.get(CONTENT_TYPE).and_then(|ct| {
        ct.to_str().map_err(|e| {
            tracing::warn!("Invalid UTF-8 in Content-Type header: {}", e);
            e
        }).ok().map(|s| s.to_string())
    });
    if let Some(s) = ct_val.as_deref() {
        let is_json = s.starts_with("application/json");
        let is_sse = s.starts_with("text/event-stream");
        if is_json || is_sse {
            headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
            headers.insert(PRAGMA, HeaderValue::from_static("no-cache"));
            // Hint for reverse proxies not to buffer SSE
            if is_sse {
                headers.insert(HeaderName::from_static("x-accel-buffering"), HeaderValue::from_static("no"));
            }
        } else {
            // Long-lived caching for static assets
            let is_css = s.starts_with("text/css");
            let is_js = s.starts_with("application/javascript") || s.starts_with("text/javascript");
            let is_wasm = s.starts_with("application/wasm");
            if is_css || is_js || is_wasm {
                headers
                    .insert(CACHE_CONTROL, HeaderValue::from_static("public, max-age=31536000, immutable"));
                // Remove pragma if previously set by proxies
                headers.remove(PRAGMA);
            }
        }
    }

    res
}
