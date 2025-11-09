//! Integration and unit tests for the Speicherwald application.
//!
//! This module organizes all test modules for the application, providing
//! comprehensive test coverage for different components and functionality.
//!
//! ## Test Modules
//!
//! - **scanner_tests**: Tests for the file system scanning functionality
//! - **api_tests**: General API endpoint tests
//! - **error_tests**: Error handling and validation tests
//! - **config_tests**: Configuration loading and validation tests
//! - **db_tests**: Database operations and migration tests
//! - **health_api_tests**: Health check endpoint tests
//! - **drives_api_tests**: Drive management API tests
//!
//! ## Running Tests
//!
//! Tests can be run using:
//! ```bash
//! cargo test
//! ```
//!
//! Individual test modules can be run with:
//! ```bash
//! cargo test scanner_tests
//! cargo test api_tests
//! # etc.
//! ```

pub mod scanner_tests;
pub mod api_tests;
pub mod error_tests;
pub mod config_tests;
pub mod db_tests;
pub mod health_api_tests;
pub mod drives_api_tests;
