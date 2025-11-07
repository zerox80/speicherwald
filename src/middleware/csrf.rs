// FIX Bug #30: Add CSRF protection for state-changing endpoints

use axum::{
    extract::Request,
    http::{HeaderMap, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

const CSRF_HEADER: &str = "X-CSRF-Token";
const CSRF_EXPECTED_VALUE: &str = "speicherwald-api-request";

/// CSRF protection middleware for state-changing operations
/// 
/// This is a simplified CSRF protection suitable for APIs accessed by
/// a trusted web UI. For production use with untrusted clients, consider:
/// - Generating unique tokens per session
/// - Token expiration
/// - Cryptographic token validation
pub async fn csrf_protection_middleware(req: Request, next: Next) -> Response {
    let method = req.method();
    
    // Only check CSRF for state-changing methods
    if matches!(method, &Method::POST | &Method::PUT | &Method::DELETE | &Method::PATCH) {
        let headers = req.headers();
        
        if !validate_csrf_token(headers) {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": {
                        "code": "CSRF_TOKEN_MISSING",
                        "message": format!("CSRF token required. Include '{}' header with value '{}'", 
                            CSRF_HEADER, CSRF_EXPECTED_VALUE),
                    },
                    "status": 403,
                })),
            )
                .into_response();
        }
    }
    
    next.run(req).await
}

fn validate_csrf_token(headers: &HeaderMap) -> bool {
    headers
        .get(CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|v| v == CSRF_EXPECTED_VALUE)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::HeaderValue;

    #[test]
    fn test_csrf_validation() {
        let mut headers = HeaderMap::new();
        assert!(!validate_csrf_token(&headers));
        
        headers.insert(CSRF_HEADER, HeaderValue::from_static(CSRF_EXPECTED_VALUE));
        assert!(validate_csrf_token(&headers));
        
        headers.insert(CSRF_HEADER, HeaderValue::from_static("wrong-value"));
        assert!(!validate_csrf_token(&headers));
    }
}
