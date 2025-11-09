//! # Speicherwald Backend Library
//!
//! This is the core library for Speicherwald, a file system scanning and management application.
//! Speicherwald provides efficient directory traversal, file analysis, and storage management
//! capabilities with a REST API interface.
//!
//! ## Architecture
//!
//! The application is built using:
//! - **Axum**: Modern web framework for HTTP server and routing
//! - **SQLx**: Asynchronous database operations with SQLite
//! - **Tokio**: Async runtime for concurrent operations
//! - **Serde**: Serialization/deserialization for JSON APIs
//!
//! ## Core Components
//!
//! - [`config`]: Application configuration management
//! - [`db`]: Database schema initialization and migrations
//! - [`error`]: Centralized error handling and HTTP error responses
//! - [`metrics`]: Application performance and usage metrics
//! - [`middleware`]: HTTP middleware for security, rate limiting, and validation
//! - [`routes`]: HTTP API endpoint handlers
//! - [`scanner`]: File system scanning and analysis engine
//! - [`state`]: Shared application state and resource management
//! - [`types`]: Data transfer objects and shared type definitions
//!
//! ## Features
//!
//! - Concurrent directory scanning with configurable depth and filtering
//! - Real-time progress updates via Server-Sent Events (SSE)
//! - File size analysis (logical vs allocated space)
//! - Export capabilities for scan results
//! - Search and filtering within scanned directories
//! - Drive information and management
//! - Rate limiting and security headers
//! - Comprehensive error handling and logging

pub mod config;
pub mod db;
pub mod error;
pub mod metrics;
pub mod middleware;
pub mod routes;
pub mod scanner;
pub mod state;
pub mod types;
