# SpeicherWald 🌲

> A high-performance disk space analyzer for Windows

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=flat&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Windows](https://img.shields.io/badge/Windows-0078D6?style=flat&logo=windows&logoColor=white)](https://www.microsoft.com/windows)

SpeicherWald is a powerful, open-source disk space analyzer built with Rust and modern web technologies. It provides fast directory size analysis for both local and network drives on Windows systems.

## 🌟 Highlights

- **⚡ Lightning Fast**: Multi-threaded scanning with intelligent caching
- **🎯 Accurate**: Measures both logical and allocated disk space
- **🌐 Web-Based UI**: Modern, responsive interface using Dioxus/WASM
- **🖥️ Desktop App**: Native experience via Tauri
- **📊 Real-time Updates**: Live progress tracking with Server-Sent Events
- **💾 Persistent Storage**: SQLite database for scan history
- **🔍 Smart Filtering**: Exclude patterns, hidden files handling

## Table of Contents

- [🌟 Highlights](#-highlights)
- [🛠️ Technology Stack](#️-technology-stack)
- [📋 System Requirements](#-system-requirements)
- [🎯 Target Audience](#-target-audience)
- [Overview](#overview)
- [✨ Features](#-features)
- [📁 Project Structure](#-project-structure)
- [🧱 Architecture](#-architecture)
- [🗄️ Data model](#️-data-model)
- [🚀 Quick Start](#-quick-start)
- [📦 Installation](#-installation-recommended)
- [🔨 Manual Builds](#-manual-builds)
- [🐳 Docker/Compose Quick Start](#-dockercompose-quick-start)
- [⚙️ Configuration](#️-configuration)
- [🔒 Rate Limiting](#-rate-limiting)
- [💡 Usage](#-usage)
- [🔌 API Reference](#-api-reference)
- [🔧 Troubleshooting](#-troubleshooting)
- [🧪 Development & Testing](#-development--testing)
- [♻️ Continuous Integration](#-continuous-integration)
- [📦 Packaging & Distribution](#-packaging--distribution)
- [⚡ Performance Tuning](#-performance-tuning)
- [⚠️ Known Limitations & Notes](#️-known-limitations--notes)
- [🗺️ Roadmap (suggested)](#️-roadmap-suggested)
- [🔒 Security](#-security)
- [🤝 Contributing](#-contributing)
- [📄 License](#-license)

## 🛠️ Technology Stack

- Backend (Rust, 2021 edition)
  - Web: `axum` 0.7 (routing, SSE), `tower-http` 0.5 (CORS, static files, compression)
  - Async: `tokio` 1 (full features), `futures` 0.3
  - Database: `sqlx` 0.7 (SQLite, runtime tokio-rustls, macros, uuid, time)
    - SQLite is bundled statically via `libsqlite3-sys` with the `bundled` feature for reproducible CI builds
  - Config: `config` 0.14 (TOML) + `.env` via `dotenvy`
  - Observability: `tracing`, `tracing-subscriber`, `tracing-appender` (stdout + daily-rotating file logs under `./logs`)
  - Performance: `lru` cache, `num_cpus` for worker sizing, batching and chunked inserts to respect SQLite var limits
  - Windows APIs: `windows` 0.56 for precise allocated size via `GetCompressedFileSizeW`
  - Middleware: security headers, global and per-endpoint rate limiting, request validation (see `src/middleware/`)
- Web UI (WASM, Dioxus)
  - `dioxus`/`dioxus-web`/`dioxus-router` 0.4, `reqwasm` for HTTP, `web-sys` with `EventSource` for SSE
  - Built with `trunk`, outputs directly to `ui/` per `webui/Trunk.toml` (served by the backend at `/`)
  - WASM tooling: `wasm-bindgen`, `console_error_panic_hook`, optional `wasm-opt` tuning via Trunk attributes
- Desktop (Tauri 1.x)
  - Shell open allowlist; serves UI from `../../ui`; uses Microsoft Edge WebView2 runtime
  - App launches the backend on a free localhost port and opens the UI window
- Tooling & Packaging
  - `rust-toolchain.toml` pins the stable channel, with `rustfmt` and `clippy`
  - Docker: multi-stage build that compiles backend and bundles static UI
  - Docker Compose: single service exposing `8080`, persistent `./data` volume
  - Benchmarks via `criterion` (see `benches/scanner_bench.rs`)

## 📋 System Requirements

- **OS**: Windows 11, Windows Server 2019 or later
- **Runtime**: Microsoft Edge WebView2 (for desktop app)
- **Network**: Supports already-connected UNC paths (no credential management in v0.1)

## 🎯 Target Audience

System administrators and power users who need to quickly identify storage-intensive directories and manage disk space efficiently.

## Overview

SpeicherWald is a Rust-based service and UI for fast disk usage analysis on Windows. The backend (Axum) scans local and UNC paths, measuring both logical and allocated sizes. Results are persisted to SQLite and streamed to the UI via Server-Sent Events (SSE). A Tauri desktop app wraps the same HTTP API on `localhost` for a native experience using Microsoft Edge WebView2.

- License: GPLv3 (see `LICENSE`)
- Language: UI is currently German; API field names are English. This README is in English.
- Platforms: Windows 11 and Windows Server 2019+
- Network shares: Only already-connected/accessible UNC paths are scanned (no credential management in v0.1)

Focus: performance, stability, and a clear minimal UI for administrators and power users.

## ✨ Features

- Local and accessible UNC path scanning
- Metrics: logical size and allocated size (precise on Windows via `GetCompressedFileSizeW`)
- Options: `follow_symlinks` (default false), `include_hidden` (default true), `excludes` (glob), `max_depth`, `concurrency`
- Persistence: SQLite for scans and metadata (bundled libsqlite for portability)
- Streaming: SSE for progress/warnings/completion with reduced update frequency for performance
- Endpoints: drive overview (`/drives`), directory tree (`/scans/{id}/tree`), top-N (`/scans/{id}/top`), listing and search
- Static Web UI (Dioxus) served at `/` with SPA fallback

## 📁 Project Structure

- Backend (Axum): `src/`, entry point `src/main.rs`
- Web-UI (Dioxus, WASM): `webui/` with build output to `ui/` (see `webui/Trunk.toml`)
- Static UI artifacts: `ui/` (served by the backend via `ServeDir`)
- Desktop (Tauri): `desktop/src-tauri/`, entry point `desktop/src-tauri/src/main.rs`
- Defaults: `config/default.toml`
- Installer scripts: `scripts/`

## 🧱 Architecture

The application consists of a Rust backend, a Dioxus (WASM) Web UI, and an optional Tauri desktop wrapper. Key building blocks:

- Router and static UI
  - `src/main.rs` wires routes for health, scans, queries, exports, and drives.
  - Static UI is served from `ui/` with SPA fallback via `ServeDir` + `ServeFile`.
  - At runtime, if `<exe_dir>/ui` exists it is preferred; otherwise, compile-time paths (`UI_DIR`, `UI_INDEX`) are used.

- Middleware chain (`src/main.rs` and `src/middleware/`)
  - `DefaultBodyLimit` at 10 MB for global protection.
  - `validation::validate_request_middleware` blocks path traversal and oversized payloads; warns on suspicious user agents.
  - Global rate limiting (`rate_limit::rate_limit_middleware`) with env-tunable window and request counts; respects `X-Forwarded-For`/`X-Real-IP`.
  - Per-endpoint limits via `EndpointRateLimiter` configured in `src/state.rs`.
  - Compression layer with a custom predicate that disables compression for SSE (`text/event-stream`) to keep live streams stable.
  - `TraceLayer` for request logging and `security_headers` for conservative default headers.
  - `CorsLayer::permissive()` is added only in debug builds to simplify local development.

- Jobs and SSE
  - `AppState` tracks active scan jobs in a `jobs` map keyed by `Uuid` and exposes a `broadcast::Sender<ScanEvent>` for SSE.
  - Each job has a `CancellationToken` for `/scans/:id` DELETE.

- Scanner pipeline (`src/scanner/mod.rs`)
  - For each root path a blocking worker enumerates entries under a semaphore-limited concurrency.
  - Excludes are matched with `globset`; optional handling for hidden/system flags and reparse points on Windows.
  - File sizes: logical via metadata; allocated via `GetCompressedFileSizeW` (`windows` crate). Results are cached in an LRU to cut syscalls.
  - Workers send `NodeRecord`/`FileRecord` batches over a bounded channel to an async aggregator.
  - Aggregator periodically flushes to SQLite respecting the 999 variable limit by chunking inserts (`sqlx::QueryBuilder`).
  - SSE progress is throttled to reduce UI churn and API load.

- Database and configuration
  - SQLite schema initialized in `src/db.rs`; WAL mode with tuned pragmas (busy timeout, cache size, mmap).
  - Config precedence: embedded defaults → optional `speicherwald.toml` → optional `SPEICHERWALD_CONFIG` → environment (`SPEICHERWALD__*`).
  - Logs go to stdout and daily-rotated files under `./logs` (`tracing-appender`).

- Build profiles
  - Backend release: `lto=true`, `codegen-units=1`, `opt-level=3`.
  - Web UI release: `lto=true`, `codegen-units=1`, `panic=abort`, `strip=true`; optional `wasm-release` with `opt-level="z"`.

## 🗄️ Data model

SQLite schema with cascade deletes to keep related rows consistent:

```sql
-- scans: one row per scan (times are stored as UTC strings)
CREATE TABLE IF NOT EXISTS scans (
  id TEXT PRIMARY KEY,                  -- UUID v4
  status TEXT NOT NULL,                 -- e.g., running, finished, canceled
  root_paths TEXT NOT NULL,             -- JSON-encoded array
  options TEXT NOT NULL,                -- JSON-encoded ScanOptions
  started_at TEXT NOT NULL,
  finished_at TEXT NULL,
  total_logical_size INTEGER NULL,
  total_allocated_size INTEGER NULL,
  dir_count INTEGER NULL,
  file_count INTEGER NULL,
  warning_count INTEGER NULL
);

-- aggregated directories (and root nodes)
CREATE TABLE IF NOT EXISTS nodes (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  scan_id TEXT NOT NULL,
  path TEXT NOT NULL,
  parent_path TEXT NULL,
  depth INTEGER NOT NULL,
  is_dir INTEGER NOT NULL,              -- 0/1
  logical_size INTEGER NOT NULL,
  allocated_size INTEGER NOT NULL,
  file_count INTEGER NOT NULL,
  dir_count INTEGER NOT NULL,
  FOREIGN KEY(scan_id) REFERENCES scans(id) ON DELETE CASCADE
);

## 🚀 Quick Start

Prerequisites: Rust (stable), Cargo. For the Web UI you need Trunk (the installers take care of this; for manual builds see below).

```powershell
# Build the backend (release)
cargo build --release

# Start the server (host/port via config or env)
./target/release/speicherwald.exe

# Open the Web UI (served by the backend)
start http://localhost:8080/
```

Note: Default listen address is `127.0.0.1:8080` (see `config/default.toml`).

## 📦 Installation (recommended)

The provided scripts build the backend, Web UI (Dioxus/Trunk), and desktop app (Tauri), then copy the artifacts to the installation directory. WebView2 is checked and installed if needed.

- Per-user install (no admin rights; to `%LocalAppData%\Programs\SpeicherWald`):
  - `scripts\install_user.cmd`
- Per-machine (e.g., to `%ProgramFiles%\Speicherwald`):
  - Run as Administrator: `scripts\install_admin.cmd`

After installation, launch the desktop app via `SpeicherWald.exe` in the install directory. The desktop app automatically starts the local HTTP server on a free port and opens the UI.

## 🔨 Manual Builds

### Web-UI (Dioxus/WASM)

The Web UI is built with Trunk and outputs directly to `ui/` (see `webui/Trunk.toml`).

```powershell
# Prerequisites
rustup target add wasm32-unknown-unknown
cargo install trunk --locked

# Build (Release) – artifacts land in ../ui
cd webui
trunk build --release
```

Note on wasm-opt/Validator flags:
- The WASM feature flags required for the validator are set via Trunk asset directives in `webui/index.html`.
- Specifically: `data-wasm-opt="z"` (size optimization) and `data-wasm-opt-params="--enable-bulk-memory --enable-nontrapping-float-to-int"` are already set.
- Advantage: Other users can simply run `trunk build --release` – without additional environment variables.
- Switching optimization: In `webui/index.html`, adjust the value of `data-wasm-opt` (`1|2|3|4` for speed, `s|z` for size) or set `data-wasm-opt="0"` to disable.

For live development, you can use:

```powershell
cd webui
trunk watch --release    # builds continuously to ../ui
# In a second terminal, start the backend serving / (UI)
cargo run
```

### Backend (Axum)

```powershell
# Development
cargo run

# Production
cargo build --release
```

### Desktop (Tauri)

```powershell
# Release build of the Tauri app
cd desktop/src-tauri
# Important note: On Windows, you may need to build with a single job (-j 1) to avoid file locks
cargo build --release -j 1

# Result (typically):
# desktop/src-tauri/target/release/speicherwald-desktop.exe
```

The desktop app (`desktop/src-tauri/src/main.rs`) searches for the backend binary (e.g., `speicherwald.exe`) at startup, starts it on a free localhost port (`127.0.0.1:<port>`), and then opens the UI window. For packaged releases, the UI and backend are delivered together. WebView2 (Edge) is required.

## 🐳 Docker/Compose Quick Start

Quick start with Docker (the UI is baked into the image):

```powershell
# Build image
docker build -t speicherwald:latest .

# Run container (DB under ./data, port 8080)
docker run --rm -p 8080:8080 -v %cd%/data:/app/data speicherwald:latest

# Open UI
start http://localhost:8080/
```

Using Docker Compose (handy for development):

```powershell
docker compose up -d
start http://localhost:8080/
```

Notes:

- Volumes: SQLite DB is persisted to `./data` by default.
- Ports: The container exposes port `8080`.
- Env: Configure via `SPEICHERWALD__*` (see below) or `docker-compose.yml`.

## ⚙️ Configuration

Precedence (highest priority first):

- Environment variables with prefix `SPEICHERWALD__` (e.g., `SPEICHERWALD__SERVER__PORT=9090`)
- Optional: explicit config file via `SPEICHERWALD_CONFIG` (path without extension; e.g., `C:/cfg/prod` loads `prod.toml`)
- Local `speicherwald.toml` in the working directory (optional)
- Embedded defaults from `config/default.toml` (compiled into the binary)

Examples (PowerShell):

```powershell
# Port via env var
$env:SPEICHERWALD__SERVER__PORT = "9090"
# Host via env var (default 127.0.0.1)
$env:SPEICHERWALD__SERVER__HOST = "127.0.0.1"

# SQLite-URL per Env-Var
$env:SPEICHERWALD__DATABASE__URL = "sqlite://data/dev.db"

# Scan defaults (used for POST /scans when fields are omitted)
$env:SPEICHERWALD__SCAN_DEFAULTS__EXCLUDES = '["**/target","**/.git"]'

# Use an alternative config file
$env:SPEICHERWALD_CONFIG = "C:/path/to/prod"  # loads prod.toml, for example

Default values (see `config/default.toml`):

```toml
[server]
host = "127.0.0.1"
port = 8080

[database]
url = "sqlite://data/speicherwald.db"

[scan_defaults]
follow_symlinks = false
include_hidden = true
measure_logical = true
measure_allocated = true
excludes = []

[scanner]
batch_size = 4000
flush_threshold = 8000
flush_interval_ms = 750
dir_concurrency = 12
# handle_limit optional — omitting means no explicit limit
#handle_limit = 2048
```

Desktop specifics: The desktop app sets the database to a user-writable location (`%LocalAppData%\SpeicherWald\speicherwald.db`) at runtime via `SPEICHERWALD__DATABASE__URL` to avoid permission issues.

## 🗄️ Data model

SQLite schema with cascade deletes to keep related rows consistent:

```sql
-- scans: one row per scan (times are stored as UTC strings)
CREATE TABLE IF NOT EXISTS scans (
  id TEXT PRIMARY KEY,                  -- UUID v4
  status TEXT NOT NULL,                 -- e.g., running, finished, canceled
  root_paths TEXT NOT NULL,             -- JSON-encoded array
  options TEXT NOT NULL,                -- JSON-encoded ScanOptions
  started_at TEXT NOT NULL,
  finished_at TEXT NULL,
  total_logical_size INTEGER NULL,
  total_allocated_size INTEGER NULL,
  dir_count INTEGER NULL,
  file_count INTEGER NULL,
  warning_count INTEGER NULL
);

-- aggregated directories (and root nodes)
CREATE TABLE IF NOT EXISTS nodes (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  scan_id TEXT NOT NULL,
  path TEXT NOT NULL,
  parent_path TEXT NULL,
  depth INTEGER NOT NULL,
  is_dir INTEGER NOT NULL,              -- 0/1
  logical_size INTEGER NOT NULL,
  allocated_size INTEGER NOT NULL,
  file_count INTEGER NOT NULL,
  dir_count INTEGER NOT NULL,
  FOREIGN KEY(scan_id) REFERENCES scans(id) ON DELETE CASCADE
);

-- individual files for top-N/list queries
CREATE TABLE IF NOT EXISTS files (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  scan_id TEXT NOT NULL,
  path TEXT NOT NULL,
  parent_path TEXT NULL,
  logical_size INTEGER NOT NULL,
  allocated_size INTEGER NOT NULL,
  FOREIGN KEY(scan_id) REFERENCES scans(id) ON DELETE CASCADE
);

-- optional warnings collected during scanning
CREATE TABLE IF NOT EXISTS warnings (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  scan_id TEXT NOT NULL,
  path TEXT NOT NULL,
  code TEXT NOT NULL,
  message TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(scan_id) REFERENCES scans(id) ON DELETE CASCADE
);
```

Indexes: see `src/db.rs` for the full list. Highlights include `idx_nodes_scan_isdir_alloc_desc` for fast top-by-size and `idx_files_scan_size`/`idx_files_scan_parent` for listing and top-N.

Data location: by default `sqlite://data/speicherwald.db` (container: `/app/data`). Deleting a scan (`DELETE /scans/:id?purge=true`) removes related rows via `ON DELETE CASCADE`.

## 🔒 Rate Limiting

The backend protects itself against abuse via rate limiting:

- Global limit (configurable via environment):
  - `SPEICHERWALD_RATE_LIMIT_MAX_REQUESTS` (default: `1000`)
  - `SPEICHERWALD_RATE_LIMIT_WINDOW_SECONDS` (default: `60`)
  - IP-based across all endpoints (respects `X-Forwarded-For`/`X-Real-IP` headers).
- Per-endpoint limits (see `src/state.rs`):
  - `POST /scans`: 60/minute/IP
  - `GET /scans/:id/search`: 600/minute/IP
  - `GET /drives`: 120/minute/IP

Old entries are pruned every 5 minutes to keep memory usage bounded.

### UI & pagination behavior

- The Web UI throttles SSE-triggered table reloads to roughly every 5 seconds to avoid unnecessary API load.
- The explorer/list pagination is defensive:
  - “Previous page” is disabled when `offset == 0` or during loading.
  - “Next page” is enabled only if the last query returned at least `limit` items (heuristic “likely more”), and is disabled while loading.
  - Navigating to a new path resets `offset` to `0`.
  - Concurrent requests are skipped while a request is in flight.

If you still encounter `429 Too Many Requests` (e.g., rapid manual navigation), wait for the indicated `retry_after_seconds` and try again.

## 🔧 Troubleshooting

- WebView2 missing: Install the Microsoft Edge WebView2 Runtime. The installer (`scripts/install*.cmd` → `install.ps1`) attempts this automatically. Manual: https://developer.microsoft.com/microsoft-edge/webview2/
- Port in use: Set `SPEICHERWALD__SERVER__PORT` to a free port or adjust `speicherwald.toml`. The desktop app auto-selects a free port.
- Write permissions/DB: When starting in a working directory, the SQLite DB is created under `data/`. The desktop app uses `%LocalAppData%\SpeicherWald\speicherwald.db`.
- Long paths: Paths with the `\\?\` prefix are supported where possible.
- Reparse points / symlinks: Not followed by default; can be enabled via `follow_symlinks`.
- Hidden/System: Included by default; can be disabled via `include_hidden`.
- UNC paths: Only already-connected/accessible resources are scanned (no credential management in v0.1).

## 🧪 Development & Testing

- Rust toolchain
  - Format: `cargo fmt -- --check`
  - Lint: `cargo clippy -D warnings`
  - Tests: `cargo test --verbose` (unit + integration; in-memory SQLite used widely)
  - Benchmarks (Criterion): `cargo bench` (Windows recommended; builds benches without running in CI)

- Web UI
  - Live dev: `cd webui && trunk watch --release` (outputs to `../ui/`; start backend separately with `cargo run`)

- Desktop (Tauri)
  - Build: `cd desktop/src-tauri && cargo build --release -j 1`
  - The app spawns the backend on a free `127.0.0.1:<port>` and opens the UI.

## ♻️ Continuous Integration

The GitHub Actions workflow at `.github/workflows/ci.yml` runs on Windows and Ubuntu:

- Formatting and Clippy
- Tests on stable Rust (normal and all-features)
- Code coverage on Ubuntu via `cargo-tarpaulin` with upload to Codecov
- Release build and UI build
- Windows-only benches build (`cargo bench --no-run`)
- Optional jobs package a portable ZIP and build the desktop app, uploaded as artifacts

## 📦 Packaging & Distribution

- Installer scripts (Windows): `scripts/install_user.cmd` and `scripts/install_admin.cmd`
  - Wrap `install.ps1` to build backend, Web UI (Trunk), optionally Tauri desktop, and stage artifacts into the chosen directory
  - Creates a `RUN-SpeicherWald.cmd` helper to start the server and open the browser

- Portable ZIP: `python scripts/package_portable.py [--include-desktop]`
  - Builds UI, backend, optionally desktop; stages `speicherwald.exe` next to `ui/` and creates a timestamped ZIP under `dist/`
  - Optional env `SPEICHERWALD_EXTRA_FILES` can add extra files (e.g., `README.md;LICENSE`)

- Docker: see Docker section above (multi-stage build; UI bundled)

## ⚡ Performance Tuning

- Scanner configuration (`[scanner]` in config or `SPEICHERWALD__SCANNER__*` env vars)
  - `batch_size`, `flush_threshold`, `flush_interval_ms` influence DB write batching
  - `dir_concurrency` limits concurrent directory workers per root
  - `handle_limit` can cap OS handles to avoid pressure on large trees

- Concurrency heuristic
  - Default worker count ≈ 75% of CPU cores (at least 2), further clamped by `handle_limit`

- SQLite pragmas
  - WAL mode, `busy_timeout`, large page cache, and `mmap_size` applied on connect

- SSE throttling
  - UI throttles SSE-triggered refreshes to about every 5s to reduce load; SSE responses are not compressed to maintain stability

## ⚠️ Known Limitations & Notes

- Windows focus: allocated size relies on `GetCompressedFileSizeW`; on non-Windows, allocated size falls back to logical size
- No credential handling for UNC paths: only scans already-connected resources
- Long path support: `\\?\` prefixes are supported where feasible
- UI language is currently German; contributions for i18n are welcome
- Rate limiting: aggressive navigation can produce `429`; respect `retry_after_seconds`

## 🗺️ Roadmap (suggested)

- Optional authentication for remote deployments
- i18n for UI (EN/DE)
- Configurable Prometheus metrics labels and histograms
- Pluggable storage backends (e.g., Postgres) behind `sqlx` feature flags
- Export enhancements (filtering, separate CSVs for nodes/files)

## 🔒 Security

- No storage of access data in v0.1. Only already-connected resources are scanned.

## 🤝 Contributing

Feedback and PRs are welcome. Please keep code style consistent, include tests where relevant, and align with the project goals (performance, stability, minimalist UI).

## 📄 License

GPLv3 – see `LICENSE`. Ensure that the full license text is included in releases.
