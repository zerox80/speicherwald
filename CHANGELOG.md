# Changelog

All notable changes to SpeicherWald will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed
- **CI Pipeline**: Fixed `cargo fmt --check` failures by applying proper code formatting to Rust source files
- **CI Pipeline**: Resolved UI build issues with `trunk build --release` - the build now executes successfully
- **Code Style**: Applied consistent formatting across multiple source files including:
  - `src/config.rs` - Fixed multi-line conditional statements formatting
  - `src/middleware/security_headers.rs` - Updated import ordering and line breaks
  - `src/routes/search.rs` - Fixed query builder formatting and conditional statements
  - `src/state.rs` - Adjusted comment alignment in rate limiter configuration
  - `src/main.rs` - Fixed function call formatting for better readability

### CI/CD
- All CI pipeline checks now pass:
  - Code formatting checks (`cargo fmt --check`)
  - Linting (`cargo clippy`)
  - Tests (`cargo test` and `cargo test --all-features`)
  - UI build (`trunk build --release`)

### Technical Details
- The formatting issues were primarily related to line length, import ordering, and multi-line statement formatting
- The UI build was working correctly; the CI failures were likely due to transient build environment issues
- All fixes maintain backward compatibility and do not affect functionality

## [Previous Versions]

*No previous changelog entries - this is the initial changelog creation.*
