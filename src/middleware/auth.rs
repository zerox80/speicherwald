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
    // FIX Bug #6: Cache the token lookup to avoid env var overhead on every request
    static AUTH_TOKEN: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let expected_token_opt = AUTH_TOKEN.get_or_init(|| {
        std::env::var("SPEICHERWALD_AUTH_TOKEN").ok().filter(|t| !t.is_empty())
    });

    let expected_token = match expected_token_opt {
        Some(t) => t,
        None => return Ok(next.run(req).await),
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
