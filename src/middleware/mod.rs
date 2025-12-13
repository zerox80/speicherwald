//! Middleware components for HTTP request processing.
//!
//! This module provides various middleware components that handle cross-cutting concerns
//! such as security, rate limiting, request validation, and client identification.
//! These middleware components can be layered with Axum's routing system to provide
//! comprehensive request processing pipeline.

pub mod auth;
pub mod ip;
pub mod rate_limit;
pub mod security_headers;
pub mod validation;
pub mod csrf; // FIX Bug #30: CSRF protection

pub use rate_limit::EndpointRateLimiter;
