use std::path::Path;

use serde::Deserialize;

/// Configuration for the HTTP server.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// The host address to bind to.
    pub host: String,
    /// The port to listen on.
    pub port: u16,
}

/// Configuration for the database connection.
#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    /// The database connection URL.
    pub url: String,
}

/// Default settings for new scans.
#[derive(Debug, Clone, Deserialize)]
pub struct ScanDefaultsConfig {
    /// Whether to follow symbolic links.
    pub follow_symlinks: bool,
    /// Whether to include hidden files and directories.
    pub include_hidden: bool,
    /// Whether to measure logical file size.
    pub measure_logical: bool,
    /// Whether to measure allocated disk space.
    pub measure_allocated: bool,
    /// A list of glob patterns to exclude from scans.
    pub excludes: Vec<String>,
    /// The maximum scan depth.
    pub max_depth: Option<u32>,
    /// The number of concurrent scanner threads.
    pub concurrency: Option<usize>,
}

/// Configuration for the file scanner.
#[derive(Debug, Clone, Deserialize)]
pub struct ScannerConfig {
    /// The number of file records to batch before flushing to the database.
    pub batch_size: usize,
    /// The number of pending records that triggers a flush to the database.
    pub flush_threshold: usize,
    /// The interval in milliseconds at which to flush pending records to the database.
    pub flush_interval_ms: u64,
    /// The maximum number of open file handles.
    pub handle_limit: Option<usize>,
    /// The number of concurrent directory traversers.
    pub dir_concurrency: Option<usize>,
}

/// Configuration for security-related HTTP headers.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SecurityConfig {
    /// Whether to enable the HTTP Strict-Transport-Security (HSTS) header.
    pub enable_hsts: Option<bool>,
    /// The `max-age` value for the HSTS header.
    pub hsts_max_age: Option<u64>,
    /// Whether to include subdomains in the HSTS header.
    pub hsts_include_subdomains: Option<bool>,
    /// The Content-Security-Policy (CSP) header value.
    pub csp: Option<String>,
}

/// The main application configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    /// Server configuration.
    pub server: ServerConfig,
    /// Database configuration.
    pub database: DatabaseConfig,
    /// Default scan settings.
    pub scan_defaults: ScanDefaultsConfig,
    /// Scanner configuration.
    pub scanner: ScannerConfig,
    /// Security headers configuration.
    pub security: Option<SecurityConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        // Fallback: parse the embedded default TOML
        let defaults: &str = include_str!("../config/default.toml");
        match ::config::Config::builder()
            .add_source(::config::File::from_str(defaults, ::config::FileFormat::Toml))
            .build()
        {
            Ok(cfg) => match cfg.try_deserialize() {
                Ok(app_cfg) => app_cfg,
                Err(e) => {
                    eprintln!("FATAL: Failed to deserialize default config: {}", e);
                    panic!("Failed to deserialize default config: {}", e);
                }
            },
            Err(e) => {
                eprintln!("FATAL: Failed to parse default config: {}", e);
                panic!("Failed to parse default config: {}", e);
            }
        }
    }
}

impl Default for ScannerConfig {
    fn default() -> Self {
        // Mirror defaults from config/default.toml
        Self {
            batch_size: 4000,
            flush_threshold: 8000,
            flush_interval_ms: 750,
            handle_limit: None,
            dir_concurrency: Some(12),
        }
    }
}

/// Loads the application configuration from various sources.
///
/// This function loads configuration in the following order of precedence (highest to lowest):
/// 1. Environment variables with the prefix `SPEICHERWALD__`.
/// 2. A custom configuration file specified by the `SPEICHERWALD_CONFIG` environment variable.
/// 3. A local `speicherwald.toml` file in the current working directory.
/// 4. The embedded default configuration from `config/default.toml`.
///
/// It also loads environment variables from a `.env` file if present.
///
/// # Returns
///
/// * `anyhow::Result<AppConfig>` - The loaded and validated application configuration.
pub fn load() -> anyhow::Result<AppConfig> {
    // Load .env first (optional)
    let _ = dotenvy::dotenv();

    let defaults: &str = include_str!("../config/default.toml");
    let mut builder = ::config::Config::builder()
        .add_source(::config::File::from_str(defaults, ::config::FileFormat::Toml))
        // Optional local file: speicherwald.toml (in CWD)
        .add_source(::config::File::with_name("speicherwald").required(false));

    if let Ok(custom_path) = std::env::var("SPEICHERWALD_CONFIG") {
        builder = builder.add_source(::config::File::with_name(&custom_path).required(false));
    }
    // Environment variables last to have highest precedence
    builder = builder.add_source(::config::Environment::with_prefix("SPEICHERWALD").separator("__"));

    let cfg = builder.build()?;
    let app_cfg: AppConfig = cfg.try_deserialize()?;
    validate(&app_cfg)?;
    Ok(app_cfg)
}

fn validate(cfg: &AppConfig) -> anyhow::Result<()> {
    // Server
    if cfg.server.port == 0 {
        return Err(anyhow::anyhow!("invalid server.port: {}", cfg.server.port));
    }
    // Warn for privileged ports on Unix-like systems
    #[cfg(unix)]
    if cfg.server.port < 1024 {
        tracing::warn!("Using privileged port {} - may require elevated permissions", cfg.server.port);
    }

    // Scanner
    if cfg.scanner.batch_size == 0 {
        return Err(anyhow::anyhow!("scanner.batch_size must be > 0"));
    }
    if cfg.scanner.flush_threshold == 0 {
        return Err(anyhow::anyhow!("scanner.flush_threshold must be > 0"));
    }
    if cfg.scanner.flush_threshold <= cfg.scanner.batch_size {
        return Err(anyhow::anyhow!("scanner.flush_threshold must be > batch_size"));
    }
    if cfg.scanner.flush_interval_ms == 0 {
        return Err(anyhow::anyhow!("scanner.flush_interval_ms must be > 0"));
    }
    if let Some(dc) = cfg.scanner.dir_concurrency {
        if dc == 0 || dc > 256 {
            return Err(anyhow::anyhow!("scanner.dir_concurrency must be in 1..=256"));
        }
    }
    if let Some(h) = cfg.scanner.handle_limit {
        if h == 0 {
            return Err(anyhow::anyhow!("scanner.handle_limit must be > 0 when set"));
        }
    }

    // Scan defaults
    if let Some(c) = cfg.scan_defaults.concurrency {
        if c == 0 || c > 256 {
            return Err(anyhow::anyhow!("scan_defaults.concurrency must be in 1..=256"));
        }
    }

    Ok(())
}

/// Ensures that the parent directory for a SQLite database file exists.
///
/// This function parses a SQLite connection URL, extracts the file path, and
/// creates the parent directory if it doesn't already exist.
///
/// # Arguments
///
/// * `url` - The SQLite connection URL (e.g., `sqlite://data/speicherwald.db`).
///
/// # Returns
///
/// * `anyhow::Result<()>` - `Ok(())` on success, or an error if the directory
///   could not be created.
pub fn ensure_sqlite_parent_dir(url: &str) -> anyhow::Result<()> {
    if let Some(path) = url.strip_prefix("sqlite://") {
        // On Windows, handle URLs like sqlite:///C:/... by stripping the leading '/'
        // FIX Bug #49 - Only allow valid ASCII drive letters (A-Z, a-z)
        #[cfg(windows)]
        let path = {
            let bytes = path.as_bytes();
            // Check for drive letter: /C:/ or /c:/
            if bytes.len() >= 3 && bytes[0] == b'/' && bytes[2] == b':' {
                let drive_byte = bytes[1];
                // Only allow valid ASCII drive letters (A-Z, a-z)
                // Extended ASCII (>= 128) is NOT valid for Windows drive letters
                if (drive_byte >= b'A' && drive_byte <= b'Z') || (drive_byte >= b'a' && drive_byte <= b'z') {
                    &path[1..]
                } else {
                    // Invalid drive letter, keep the path as-is and let it fail naturally
                    tracing::warn!("Invalid drive letter in path: {:?}", path);
                    path
                }
            } else {
                path
            }
        };
        let p = Path::new(path);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}
