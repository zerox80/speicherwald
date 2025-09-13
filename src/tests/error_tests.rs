#[cfg(test)]
mod tests {
    use crate::error::{AppError, AppResult, OptionExt, validation};
    use axum::response::IntoResponse;
    use axum::http::StatusCode;
    use std::io;

    #[test]
    fn test_app_error_display() {
        let error = AppError::BadRequest("Invalid input".to_string());
        assert_eq!(format!("{}", error), "Bad request: Invalid input");

        let error = AppError::NotFound("Resource not found".to_string());
        assert_eq!(format!("{}", error), "Not found: Resource not found");

        let error = AppError::RateLimited { retry_after_seconds: 60 };
        assert_eq!(format!("{}", error), "Rate limited. Retry after 60 seconds");
    }

    #[test]
    fn test_app_error_into_response() {
        let error = AppError::BadRequest("Test error".to_string());
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let error = AppError::NotFound("Not found".to_string());
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let error = AppError::Conflict("Conflict".to_string());
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);

        let error = AppError::ServiceUnavailable("Service down".to_string());
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        let error = AppError::Unauthorized("No access".to_string());
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let error = AppError::RateLimited { retry_after_seconds: 30 };
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn test_from_io_error() {
        let io_error = io::Error::new(io::ErrorKind::NotFound, "File not found");
        let app_error: AppError = io_error.into();
        
        match app_error {
            AppError::IoError(msg) => {
                assert!(msg.contains("NotFound"));
                assert!(msg.contains("File not found"));
            }
            _ => panic!("Expected IoError variant"),
        }
    }

    #[test]
    fn test_from_globset_error() {
        let glob_result = globset::Glob::new("[invalid");
        assert!(glob_result.is_err());
        
        let app_error: AppError = glob_result.unwrap_err().into();
        match app_error {
            AppError::InvalidInput(msg) => {
                assert!(msg.contains("Invalid glob pattern"));
            }
            _ => panic!("Expected InvalidInput variant"),
        }
    }

    #[test]
    fn test_option_ext() {
        let some_value: Option<i32> = Some(42);
        let result: AppResult<i32> = some_value.ok_or_not_found("test entity");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);

        let none_value: Option<i32> = None;
        let result: AppResult<i32> = none_value.ok_or_not_found("test entity");
        assert!(result.is_err());
        
        match result.unwrap_err() {
            AppError::NotFound(msg) => {
                assert_eq!(msg, "test entity not found");
            }
            _ => panic!("Expected NotFound error"),
        }
    }

    #[test]
    fn test_validate_path() {
        // Valid path
        assert!(validation::validate_path("/valid/path").is_ok());
        assert!(validation::validate_path("C:\\Windows\\System32").is_ok());
        
        // Empty path
        let result = validation::validate_path("");
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::ValidationError { field, message } => {
                assert_eq!(field, "path");
                assert_eq!(message, "Path cannot be empty");
            }
            _ => panic!("Expected ValidationError"),
        }
        
        // Path with null character
        let result = validation::validate_path("path\0with\0null");
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::ValidationError { field, message } => {
                assert_eq!(field, "path");
                assert_eq!(message, "Path contains null characters");
            }
            _ => panic!("Expected ValidationError"),
        }
    }

    #[test]
    fn test_validate_positive_number() {
        // Valid positive numbers
        assert!(validation::validate_positive_number(Some(1), "test_field").is_ok());
        assert!(validation::validate_positive_number(Some(100), "test_field").is_ok());
        assert!(validation::validate_positive_number(None, "test_field").is_ok());
        
        // Zero
        let result = validation::validate_positive_number(Some(0), "test_field");
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::ValidationError { field, message } => {
                assert_eq!(field, "test_field");
                assert!(message.contains("must be positive"));
            }
            _ => panic!("Expected ValidationError"),
        }
        
        // Negative number
        let result = validation::validate_positive_number(Some(-5), "test_field");
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::ValidationError { field, message } => {
                assert_eq!(field, "test_field");
                assert!(message.contains("must be positive"));
                assert!(message.contains("-5"));
            }
            _ => panic!("Expected ValidationError"),
        }
    }

    #[test]
    fn test_validate_paths_exist() {
        use std::fs;
        use tempfile::TempDir;
        
        // Create temporary directory and file
        let temp_dir = TempDir::new().unwrap();
        let temp_file_path = temp_dir.path().join("test.txt");
        fs::write(&temp_file_path, "test content").unwrap();
        
        // Valid existing paths
        let paths = vec![
            temp_dir.path().to_str().unwrap().to_string(),
            temp_file_path.to_str().unwrap().to_string(),
        ];
        assert!(validation::validate_paths_exist(&paths).is_ok());
        
        // Non-existent path
        let paths = vec![
            "/this/path/does/not/exist".to_string(),
        ];
        let result = validation::validate_paths_exist(&paths);
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::NotFound(msg) => {
                assert!(msg.contains("Path does not exist"));
                assert!(msg.contains("/this/path/does/not/exist"));
            }
            _ => panic!("Expected NotFound error"),
        }
    }

    #[test]
    fn test_validation_error_creation() {
        let error = AppError::ValidationError {
            field: "email".to_string(),
            message: "Invalid email format".to_string(),
        };
        
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
