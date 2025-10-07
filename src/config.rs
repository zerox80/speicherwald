use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScanDefaultsConfig {
    pub follow_symlinks: bool,
    pub include_hidden: bool,
    pub measure_logical: bool,
    pub measure_allocated: bool,
    pub excludes: Vec<String>,
    pub max_depth: Option<u32>,
    pub concurrency: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScannerConfig {
    pub batch_size: usize,
    pub flush_threshold: usize,
    pub flush_interval_ms: u64,
    pub handle_limit: Option<usize>,
    pub dir_concurrency: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SecurityConfig {
    pub enable_hsts: Option<bool>,
    pub hsts_max_age: Option<u64>,
    pub hsts_include_subdomains: Option<bool>,
    pub csp: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub scan_defaults: ScanDefaultsConfig,
    pub scanner: ScannerConfig,
    pub security: Option<SecurityConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        // Fallback: parse the embedded default TOML
        let defaults: &str = include_str!("../config/default.toml");
        let cfg: AppConfig = ::config::Config::builder()
            .add_source(::config::File::from_str(defaults, ::config::FileFormat::Toml))
            .build()
            .expect("default config parse")
            .try_deserialize()
            .expect("default config deserialize");
        cfg
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

    // Scanner
    if cfg.scanner.batch_size == 0 {
        return Err(anyhow::anyhow!("scanner.batch_size must be > 0"));
    }
    if cfg.scanner.flush_threshold == 0 {
        return Err(anyhow::anyhow!("scanner.flush_threshold must be > 0"));
    }
    if cfg.scanner.flush_threshold < cfg.scanner.batch_size {
        return Err(anyhow::anyhow!("scanner.flush_threshold must be >= batch_size"));
    }
    if cfg.scanner.flush_interval_ms == 0 {
        return Err(anyhow::anyhow!("scanner.flush_interval_ms must be > 0"));
    }
    if let Some(dc) = cfg.scanner.dir_concurrency {
        if dc == 0 || dc > 1000 {
            return Err(anyhow::anyhow!("scanner.dir_concurrency must be in 1..=1000"));
        }
    }
    if let Some(h) = cfg.scanner.handle_limit {
        if h == 0 {
            return Err(anyhow::anyhow!("scanner.handle_limit must be > 0 when set"));
        }
    }

    // Scan defaults
    if let Some(c) = cfg.scan_defaults.concurrency {
        if c == 0 || c > 1000 {
            return Err(anyhow::anyhow!("scan_defaults.concurrency must be in 1..=1000"));
        }
    }

    Ok(())
}

pub fn ensure_sqlite_parent_dir(url: &str) -> anyhow::Result<()> {
    if let Some(path) = url.strip_prefix("sqlite://") {
        // On Windows, handle URLs like sqlite:///C:/... by stripping the leading '/'
        #[cfg(windows)]
        let path = {
            let bytes = path.as_bytes();
            if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
                &path[1..]
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
