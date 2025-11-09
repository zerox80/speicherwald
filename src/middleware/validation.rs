#![allow(dead_code)]
use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use uuid::Uuid;

/// An Axum middleware that validates incoming requests for common security issues.
///
/// This middleware checks for:
/// - Path traversal attempts in the request URI.
/// - Suspicious user agents.
/// - Excessive content length.
///
/// # Arguments
///
/// * `req` - The incoming `Request`.
/// * `next` - The next middleware in the chain.
///
/// # Returns
///
/// * `Response` - The response from the next middleware, or a `400 Bad Request`
///   or `413 Payload Too Large` response if a validation check fails.
pub async fn validate_request_middleware(req: Request, next: Next) -> Response {
    // Check for path traversal attempts in URL
    let uri_path = req.uri().path();
    if contains_path_traversal(uri_path) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": "INVALID_PATH",
                    "message": "Path traversal detected in request",
                },
                "status": 400,
            })),
        )
            .into_response();
    }

    // Check for suspicious headers
    if let Some(user_agent) = req.headers().get("user-agent") {
        if let Ok(ua_str) = user_agent.to_str() {
            if is_suspicious_user_agent(ua_str) {
                tracing::warn!("Suspicious user agent detected: {}", ua_str);
            }
        }
    }

    // Check content length for POST/PUT requests
    // This is redundant with DefaultBodyLimit but provides early rejection
    if matches!(req.method(), &axum::http::Method::POST | &axum::http::Method::PUT) {
        if let Some(content_length) = req.headers().get("content-length") {
            if let Ok(length_str) = content_length.to_str() {
                if let Ok(length) = length_str.parse::<usize>() {
                    // Use configurable limit matching main.rs
                    let max_body_size = std::env::var("SPEICHERWALD_MAX_BODY_SIZE")
                        .ok()
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(10 * 1024 * 1024)
                        .clamp(1024 * 1024, 50 * 1024 * 1024);
                    if length > max_body_size {
                        return (
                            StatusCode::PAYLOAD_TOO_LARGE,
                            Json(json!({
                                "error": {
                                    "code": "PAYLOAD_TOO_LARGE",
                                    "message": format!("Request body exceeds maximum size of {} bytes", max_body_size),
                                },
                                "status": 413,
                            })),
                        ).into_response();
                    }
                }
            }
        }
    }

    next.run(req).await
}

/// Check if a path contains traversal attempts (FIX Bugs #46, #47, #48)
/// More comprehensive check for actual directory traversal patterns
fn contains_path_traversal(path: &str) -> bool {
    // Check for actual path traversal sequences
    let lower = path.to_lowercase();

    // Direct traversal patterns
    if path.contains("/..") || path.contains("\\..") || path.starts_with("..") {
        return true;
    }

    // Current directory references that could be dangerous
    if path.contains("/./") || path.contains("\\.\\") {
        return true;
    }

    // Multiple dots (bypass attempt: ....)
    if path.contains("....") {
        return true;
    }

    // URL-encoded variants (single and double encoding)
    let encoded_patterns = [
        "%2e%2e",
        "%252e%252e", // .. and double-encoded ..
        "%2e/",
        "%252e%2f", // ./
        "/%2e",
        "%2f%2e", // /.
        "%2e\\",
        "%2e%5c", // .\\
        "%5c%2e",
        "%5c%5c", // \\.
        "%00",    // Null byte
    ];

    for pattern in &encoded_patterns {
        if lower.contains(pattern) {
            return true;
        }
    }

    // Null bytes
    path.contains('\0')
}

/// Check for suspicious user agents (simple heuristic)
fn is_suspicious_user_agent(ua: &str) -> bool {
    let ua_lower = ua.to_lowercase();
    // Only flag if it contains scanner OR if it contains crawler but NOT legitimate bots
    ua_lower.contains("scanner")
        || (ua_lower.contains("crawler") && !ua_lower.contains("googlebot") && !ua_lower.contains("bingbot"))
        || ua_lower.contains("nikto")
        || ua_lower.contains("sqlmap")
        || ua_lower.contains("havij")
        || ua_lower.contains("acunetix")
}

/// Validates the format of a UUID string.
///
/// # Arguments
///
/// * `id` - The UUID string to validate.
///
/// # Returns
///
/// * `Result<Uuid, (StatusCode, Json<serde_json::Value>)>` - The parsed `Uuid` on success,
///   or a `400 Bad Request` response on failure.
pub fn validate_uuid(id: &str) -> Result<Uuid, (StatusCode, Json<serde_json::Value>)> {
    Uuid::parse_str(id).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": "INVALID_UUID",
                    "message": format!("Invalid UUID format: {}", id),
                },
                "status": 400,
            })),
        )
    })
}

/// Validates and sanitizes a file path.
///
/// This function checks for:
/// - Empty paths.
/// - Null bytes.
/// - Path traversal attempts.
/// - Excessive path length.
/// - Invalid characters on Windows.
///
/// # Arguments
///
/// * `path` - The file path to validate.
///
/// # Returns
///
/// * `Result<String, (StatusCode, Json<serde_json::Value>)>` - The validated path on success,
///   or a `400 Bad Request` response on failure.
pub fn validate_file_path(path: &str) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": "INVALID_PATH",
                    "message": "Path must not be empty"
                }
            })),
        ));
    }
    // FIX Bug #17: Add consistent null byte checking
    if trimmed.contains('\0') {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": "INVALID_PATH",
                    "message": "Path contains null byte"
                }
            })),
        ));
    }

    // Check for path traversal
    // FIX Bug #15: Note that this checks for .. in paths, but symlink-based
    // traversal requires runtime filesystem checks which should be done at point of use
    if contains_path_traversal(path) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": "PATH_TRAVERSAL",
                    "message": "Path traversal attempt detected",
                },
                "status": 400,
            })),
        ));
    }

    // Validate path length
    const MAX_PATH_LENGTH: usize = 4096;
    if path.len() > MAX_PATH_LENGTH {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "code": "PATH_TOO_LONG",
                    "message": format!("Path exceeds maximum length of {} characters", MAX_PATH_LENGTH),
                },
                "status": 400,
            })),
        ));
    }

    // Additional Windows-specific validation
    #[cfg(windows)]
    {
        // Check for invalid characters in Windows paths (excluding colon after drive letter)
        const INVALID_CHARS: &[char] = &['<', '>', '"', '|', '?', '*'];
        for c in INVALID_CHARS {
            if path.contains(*c) && !path.starts_with("\\\\?\\") {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": {
                            "code": "INVALID_PATH_CHARS",
                            "message": format!("Path contains invalid character: {}", c),
                        },
                        "status": 400,
                    })),
                ));
            }
        }

        // Allow colon only in drive letter position (e.g., C:\ or C:/) or UNC paths
        if path.contains(':') {
            let colon_count = path.matches(':').count();
            // Valid cases: drive letter (C:), extended path (\\?\), UNC path (\\server\share)
            let is_drive_path = path.len() >= 2 && path.chars().nth(1) == Some(':');
            let is_extended_path = path.starts_with("\\\\?\\");
            let is_unc_path = path.starts_with("\\\\");

            // More than one colon is never valid
            if colon_count > 1 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": {
                            "code": "INVALID_PATH_CHARS",
                            "message": "Multiple colons in path",
                        },
                        "status": 400,
                    })),
                ));
            }

            // Single colon must be in valid position
            if colon_count == 1 && !is_drive_path && !is_extended_path && !is_unc_path {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": {
                            "code": "INVALID_PATH_CHARS",
                            "message": "Invalid use of colon in path",
                        },
                        "status": 400,
                    })),
                ));
            }
        }
    }

    Ok(path.to_string())
}

/// Validates the scan options provided by the user.
///
/// # Arguments
///
/// * `max_depth` - The maximum scan depth.
/// * `concurrency` - The number of concurrent scanner threads.
///
/// # Returns
///
/// * `Result<(), (StatusCode, Json<serde_json::Value>)>` - `Ok(())` on success,
///   or a `400 Bad Request` response on failure.
pub fn validate_scan_options(
    max_depth: Option<u32>,
    concurrency: Option<usize>,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    // Validate max_depth
    if let Some(depth) = max_depth {
        const MAX_ALLOWED_DEPTH: u32 = 100;
        if depth > MAX_ALLOWED_DEPTH {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": {
                        "code": "INVALID_DEPTH",
                        "message": format!("Max depth {} exceeds maximum allowed value of {}", depth, MAX_ALLOWED_DEPTH),
                    },
                    "status": 400,
                })),
            ));
        }
    }

    // Validate concurrency (align with config.rs max of 256)
    if let Some(conc) = concurrency {
        const MAX_ALLOWED_CONCURRENCY: usize = 256;
        if conc == 0 || conc > MAX_ALLOWED_CONCURRENCY {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": {
                        "code": "INVALID_CONCURRENCY",
                        "message": format!("Concurrency {} exceeds maximum allowed value of {}", conc, MAX_ALLOWED_CONCURRENCY),
                    },
                    "status": 400,
                })),
            ));
        }
        // Combined check above, this is now redundant
    }

    Ok(())
}

/// Sanitizes user input for logging purposes.
///
/// This function removes control characters, limits the length of the string,
/// and escapes special characters.
///
/// # Arguments
///
/// * `input` - The string to sanitize.
///
/// # Returns
///
/// * `String` - The sanitized string.
pub fn sanitize_for_logging(input: &str) -> String {
    // Remove control characters (except whitespace), escape quotes, and limit length
    input
        .chars()
        .filter(|c| !c.is_control() || c.is_whitespace())
        .take(200)
        .collect::<String>()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\'', "\\\'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_traversal_detection() {
        assert!(contains_path_traversal("../etc/passwd"));
        assert!(contains_path_traversal("./../../etc/passwd"));
        assert!(contains_path_traversal("/path/../etc"));
        assert!(contains_path_traversal("%2e%2e/etc"));
        assert!(contains_path_traversal("path\0with\0null"));

        assert!(!contains_path_traversal("/normal/path"));
        assert!(!contains_path_traversal("C:\\Users\\test"));
    }

    #[test]
    fn test_suspicious_user_agents() {
        assert!(is_suspicious_user_agent("nikto/2.1.5"));
        assert!(is_suspicious_user_agent("sqlmap/1.0"));
        assert!(is_suspicious_user_agent("random scanner bot"));

        assert!(!is_suspicious_user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64)"));
        assert!(!is_suspicious_user_agent("Googlebot/2.1"));
    }

    #[test]
    fn test_uuid_validation() {
        assert!(validate_uuid("550e8400-e29b-41d4-a716-446655440000").is_ok());
        assert!(validate_uuid("not-a-uuid").is_err());
        assert!(validate_uuid("550e8400-e29b-41d4-a716").is_err());
    }

    #[test]
    fn test_file_path_validation() {
        assert!(validate_file_path("/normal/path").is_ok());
        assert!(validate_file_path("C:\\Users\\test").is_ok());

        assert!(validate_file_path("../etc/passwd").is_err());
        assert!(validate_file_path("path\0with\0null").is_err());

        #[cfg(windows)]
        {
            assert!(validate_file_path("C:\\file<name>.txt").is_err());
            assert!(validate_file_path("\\\\?\\C:\\file<name>.txt").is_ok()); // Extended path syntax allows it
        }

        let long_path = "a".repeat(5000);
        assert!(validate_file_path(&long_path).is_err());
    }

    #[test]
    fn test_scan_options_validation() {
        assert!(validate_scan_options(Some(10), Some(5)).is_ok());
        assert!(validate_scan_options(None, None).is_ok());

        // FIX Bug #69 - Use correct MAX_ALLOWED_CONCURRENCY (256, not 50)
        assert!(validate_scan_options(Some(101), Some(5)).is_err()); // max_depth too high
        assert!(validate_scan_options(Some(10), Some(257)).is_err()); // concurrency > 256
        assert!(validate_scan_options(Some(10), Some(0)).is_err()); // concurrency == 0
        assert!(validate_scan_options(Some(10), Some(256)).is_ok()); // concurrency == 256 is OK
    }

    #[test]
    fn test_sanitize_for_logging() {
        assert_eq!(sanitize_for_logging("normal text"), "normal text");
        assert_eq!(sanitize_for_logging("text\nwith\nnewlines"), "text\nwith\nnewlines");

        let with_control = "text\x00with\x01control\x02chars";
        let sanitized = sanitize_for_logging(with_control);
        assert!(!sanitized.contains('\x00'));
        assert!(!sanitized.contains('\x01'));
        assert!(!sanitized.contains('\x02'));

        let long_text = "a".repeat(300);
        assert_eq!(sanitize_for_logging(&long_text).len(), 200);
    }
}
