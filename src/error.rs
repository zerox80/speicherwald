// FIX Bug #19: Removed dead_code annotation - address dead code properly

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::error::Error;
use std::fmt;

/// The primary error type for the application.
///
/// This enum consolidates all possible errors that can occur within the application,
/// providing a unified way to handle and respond to failures.
#[derive(Debug)]
pub enum AppError {
    /// For internal server errors that are not expected to be handled by the client.
    Internal(anyhow::Error),
    /// For client errors due to invalid requests.
    BadRequest(String),
    /// For when a requested resource is not found.
    NotFound(String),
    /// For when a request conflicts with the current state of the server.
    Conflict(String),
    /// For when a service is temporarily unavailable.
    ServiceUnavailable(String),
    /// For errors related to database operations.
    Database(String),
    /// For when user input is invalid.
    InvalidInput(String),
    /// For errors that occur during the scanning process.
    Scanner(String),
    /// For when a request is not authorized.
    Unauthorized(String),
    /// For when a client has sent too many requests in a given amount of time.
    RateLimited {
        /// The number of seconds to wait before retrying the request.
        retry_after_seconds: u64,
    },
    /// For when a specific field in a request fails validation.
    ValidationError {
        /// The name of the field that failed validation.
        field: String,
        /// A message describing the validation error.
        message: String,
    },
    /// For errors related to I/O operations.
    IoError(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Internal(e) => write!(f, "Internal error: {}", e),
            AppError::BadRequest(msg) => write!(f, "Bad request: {}", msg),
            AppError::NotFound(msg) => write!(f, "Not found: {}", msg),
            AppError::Conflict(msg) => write!(f, "Conflict: {}", msg),
            AppError::ServiceUnavailable(msg) => write!(f, "Service unavailable: {}", msg),
            AppError::Database(msg) => write!(f, "Database error: {}", msg),
            AppError::InvalidInput(msg) => write!(f, "Invalid input: {}", msg),
            AppError::Scanner(msg) => write!(f, "Scanner error: {}", msg),
            AppError::Unauthorized(msg) => write!(f, "Unauthorized: {}", msg),
            AppError::RateLimited { retry_after_seconds } => {
                write!(f, "Rate limited. Retry after {} seconds", retry_after_seconds)
            }
            AppError::ValidationError { field, message } => {
                write!(f, "Validation error on field '{}': {}", field, message)
            }
            AppError::IoError(msg) => write!(f, "I/O error: {}", msg),
        }
    }
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            AppError::Internal(e) => Some(e.as_ref()),
            _ => None,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_code, error_message, details) = match self {
            AppError::Internal(e) => {
                tracing::error!("Internal error: {:?}", e);
                let error_id = uuid::Uuid::new_v4();
                tracing::error!("Error ID: {}", error_id);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "An internal server error occurred".to_string(),
                    Some(json!({ "error_id": error_id.to_string() })),
                )
            }
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, "BAD_REQUEST", msg, None),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, "NOT_FOUND", msg, None),
            AppError::Conflict(msg) => (StatusCode::CONFLICT, "CONFLICT", msg, None),
            AppError::ServiceUnavailable(msg) => {
                (StatusCode::SERVICE_UNAVAILABLE, "SERVICE_UNAVAILABLE", msg, None)
            }
            AppError::Database(msg) => {
                tracing::error!("Database error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATABASE_ERROR",
                    "A database error occurred".to_string(),
                    Some(json!({ "details": msg })),
                )
            }
            AppError::InvalidInput(msg) => (StatusCode::BAD_REQUEST, "INVALID_INPUT", msg, None),
            AppError::Scanner(msg) => {
                tracing::warn!("Scanner error: {}", msg);
                (StatusCode::INTERNAL_SERVER_ERROR, "SCANNER_ERROR", msg, None)
            }
            AppError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, "UNAUTHORIZED", msg, None),
            AppError::RateLimited { retry_after_seconds } => (
                StatusCode::TOO_MANY_REQUESTS,
                "RATE_LIMITED",
                format!("Too many requests. Please retry after {} seconds", retry_after_seconds),
                Some(json!({ "retry_after_seconds": retry_after_seconds })),
            ),
            AppError::ValidationError { field, message } => (
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                format!("Validation failed for field '{}'", field),
                Some(json!({ "field": field, "message": message })),
            ),
            AppError::IoError(msg) => {
                tracing::error!("I/O error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "IO_ERROR",
                    "An I/O error occurred".to_string(),
                    Some(json!({ "details": msg })),
                )
            }
        };

        let mut body = json!({
            "error": {
                "code": error_code,
                "message": error_message,
            },
            "status": status.as_u16(),
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        if let Some(details) = details {
            body["error"]["details"] = details;
        }

        (status, Json(body)).into_response()
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        AppError::Internal(err)
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::RowNotFound => AppError::NotFound("Record not found".to_string()),
            sqlx::Error::Database(db_err) => {
                AppError::Database(format!("Database error: {}", db_err.message()))
            }
            sqlx::Error::PoolTimedOut => {
                AppError::ServiceUnavailable("Database connection pool timed out".to_string())
            }
            _ => AppError::Database(format!("Database error: {}", err)),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::IoError(format!("{}: {}", err.kind(), err))
    }
}

impl From<globset::Error> for AppError {
    fn from(err: globset::Error) -> Self {
        AppError::InvalidInput(format!("Invalid glob pattern: {}", err))
    }
}

/// A type alias for `Result<T, AppError>`, used throughout the application.
pub type AppResult<T> = Result<T, AppError>;

/// An extension trait for `Option` that provides a convenient way to convert
/// an `Option` to a `Result` with a `NotFound` error.
pub trait OptionExt<T> {
    /// Converts an `Option<T>` to a `Result<T, AppError>`.
    ///
    /// # Arguments
    ///
    /// * `entity` - A string describing the entity that was not found.
    ///
    /// # Returns
    ///
    /// * `Ok(T)` if the `Option` is `Some(T)`.
    /// * `Err(AppError::NotFound)` if the `Option` is `None`.
    fn ok_or_not_found(self, entity: &str) -> AppResult<T>;
}

impl<T> OptionExt<T> for Option<T> {
    fn ok_or_not_found(self, entity: &str) -> AppResult<T> {
        self.ok_or_else(|| AppError::NotFound(format!("{} not found", entity)))
    }
}

/// A module containing helper functions for request validation.
pub mod validation {
    use super::*;
    use std::path::Path;

    /// Validates a file path.
    ///
    /// This function checks if a path is empty or contains null characters.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to validate.
    ///
    /// # Returns
    ///
    /// * `AppResult<()>` - `Ok(())` if the path is valid, or an `AppError` if it's not.
    pub fn validate_path(path: &str) -> AppResult<()> {
        if path.is_empty() {
            return Err(AppError::ValidationError {
                field: "path".to_string(),
                message: "Path cannot be empty".to_string(),
            });
        }

        if path.contains('\0') {
            return Err(AppError::ValidationError {
                field: "path".to_string(),
                message: "Path contains null characters".to_string(),
            });
        }

        Ok(())
    }

    /// Validates that a number is positive.
    ///
    /// # Arguments
    ///
    /// * `value` - The number to validate.
    /// * `field` - The name of the field being validated.
    ///
    /// # Returns
    ///
    /// * `AppResult<()>` - `Ok(())` if the number is positive, or an `AppError` if it's not.
    pub fn validate_positive_number(value: Option<i64>, field: &str) -> AppResult<()> {
        if let Some(v) = value {
            if v <= 0 {
                return Err(AppError::ValidationError {
                    field: field.to_string(),
                    message: format!("Value must be positive, got {}", v),
                });
            }
        }
        Ok(())
    }

    /// Validates that a list of paths exist on the filesystem.
    ///
    // # Arguments
    ///
    /// * `paths` - A slice of paths to validate.
    ///
    /// # Returns
    ///
    /// * `AppResult<()>` - `Ok(())` if all paths exist, or an `AppError` if any of them don't.
    pub fn validate_paths_exist(paths: &[String]) -> AppResult<()> {
        for path_str in paths {
            let path = Path::new(path_str);
            if !path.exists() {
                return Err(AppError::NotFound(format!("Path does not exist: {}", path_str)));
            }
        }
        Ok(())
    }
}
