//! HTTP route handlers for the Speicherwald API.
//!
//! This module contains all the HTTP endpoint handlers for the file scanning and
//! management system. Each sub-module handles a specific domain of functionality:
//!
//! - `drives`: Drive management and detection endpoints
//! - `export`: Data export functionality
//! - `health`: Health check and system status endpoints
//! - `paths`: File path management and metadata
//! - `paths_helpers`: Utility functions for path handling
//! - `scans`: File scanning operations and scan management
//! - `search`: File search and filtering capabilities

pub mod drives;
pub mod export;
pub mod health;
pub mod paths;
pub mod paths_helpers;
pub mod scans;
pub mod search;
