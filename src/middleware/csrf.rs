//! Cross-Site Request Forgery (CSRF) protection middleware.
//!
//! This module provides CSRF protection for state-changing HTTP operations to prevent
//! unauthorized requests from malicious websites. The current implementation uses a
//! simple static token approach suitable for trusted web UI clients.

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

/// CSRF protection middleware for state-changing operations.
/// 
/// This middleware validates CSRF tokens for HTTP methods that modify state
/// (POST, PUT, DELETE, PATCH) to prevent Cross-Site Request Forgery attacks.
/// 
/// # Security Note
/// 
/// This is a simplified CSRF protection suitable for APIs accessed by
/// a trusted web UI. For production use with untrusted clients, consider:
/// - Generating unique tokens per session
/// - Token expiration and rotation
/// - Cryptographic token validation
/// - SameSite cookie attributes
/// 
/// # Arguments
/// 
/// * `req` - The incoming HTTP request
/// * `next` - The next middleware in the chain
/// 
/// # Returns
/// 
/// A response that either continues the request chain or returns a 403 Forbidden
/// status with error details if CSRF validation fails.
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

/// Validates the CSRF token in the request headers.
/// 
/// # Arguments
/// 
/// * `headers` - The HTTP request headers to validate
/// 
/// # Returns
/// 
/// `true` if the CSRF token is present and valid, `false` otherwise.
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

    /// Tests CSRF token validation with various scenarios.
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
