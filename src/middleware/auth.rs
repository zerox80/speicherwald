use axum::{
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};

/// Middleware that checks for a Bearer token in the Authorization header.
///
/// If `SPEICHERWALD_AUTH_TOKEN` is set in the environment, this middleware enforces
/// that all requests must have a matching `Authorization: Bearer <token>` header.
/// If the environment variable is not set, the middleware is a no-op (authentication disabled).
pub async fn auth_middleware(req: Request, next: Next) -> Result<Response, StatusCode> {
    // Check if auth is enabled via env var
    // In a real app, this should be cached/loaded once in state, but for simplicity here we read env
    // or rely on the OS caching it. Ideally, pass this via AppState.
    // For this fix, we'll check the env var. If it's empty, we skip auth.
    let expected_token = match std::env::var("SPEICHERWALD_AUTH_TOKEN") {
        Ok(t) if !t.is_empty() => t,
        _ => return Ok(next.run(req).await),
    };

    // Check header
    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok());

    match auth_header {
        Some(auth_val) if auth_val.starts_with("Bearer ") => {
            let provided_token = &auth_val[7..];
            // FIX Bug #6: Use constant-time comparison to prevent timing attacks
            // Simple constant-time comparison implementation
            let provided_bytes = provided_token.as_bytes();
            let expected_bytes = expected_token.as_bytes();
            if provided_bytes.len() != expected_bytes.len() {
                return Err(StatusCode::UNAUTHORIZED);
            }
            let mut diff = 0u8;
            for (i, &b) in provided_bytes.iter().enumerate() {
                diff |= b ^ expected_bytes[i];
            }
            if diff == 0 {
                Ok(next.run(req).await)
            } else {
                Err(StatusCode::UNAUTHORIZED)
            }
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}
