use sqlx::SqlitePool;

pub async fn init_db(pool: &SqlitePool) -> anyhow::Result<()> {
    // FIX Bug #57 - Log PRAGMA failures
    // Pragmas for better durability/performance
    if let Err(e) = sqlx::query("PRAGMA journal_mode=WAL;").execute(pool).await {
        tracing::warn!("Failed to set WAL journal mode: {}", e);
    }
    if let Err(e) = sqlx::query("PRAGMA synchronous=NORMAL;").execute(pool).await {
        tracing::warn!("Failed to set synchronous mode: {}", e);
    }
    // Foreign keys are critical - fail if this doesn't work
    sqlx::query("PRAGMA foreign_keys=ON;").execute(pool).await?;

    // Additional tuning (best-effort) - FIX Bug #10: Log failures
    if let Err(e) = sqlx::query("PRAGMA busy_timeout=10000;").execute(pool).await {
        tracing::warn!("Failed to set busy_timeout: {}", e);
    }
    if let Err(e) = sqlx::query("PRAGMA cache_size=-65536;").execute(pool).await {
        tracing::warn!("Failed to set cache_size: {}", e);
    }
    if let Err(e) = sqlx::query("PRAGMA temp_store=MEMORY;").execute(pool).await {
        tracing::warn!("Failed to set temp_store: {}", e);
    }
    if let Err(e) = sqlx::query("PRAGMA mmap_size=268435456;").execute(pool).await {
        tracing::warn!("Failed to set mmap_size: {}", e);
    }

    // scans table
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS scans (
            id TEXT PRIMARY KEY,
            status TEXT NOT NULL,
            root_paths TEXT NOT NULL,
            options TEXT NOT NULL,
            started_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
            finished_at TEXT NULL,
            total_logical_size INTEGER NULL,
            total_allocated_size INTEGER NULL,
            dir_count INTEGER NULL,
            file_count INTEGER NULL,
            warning_count INTEGER NULL
        )"#,
    )
    .execute(pool)
    .await?;

    // warnings table (optional for future use)
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS warnings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scan_id TEXT NOT NULL,
            path TEXT NOT NULL,
            code TEXT NOT NULL,
            message TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
            FOREIGN KEY(scan_id) REFERENCES scans(id) ON DELETE CASCADE
        )"#,
    )
    .execute(pool)
    .await?;

    // nodes table (directories aggregated, optionally files too if desired)
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS nodes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scan_id TEXT NOT NULL,
            path TEXT NOT NULL,
            parent_path TEXT NULL,
            depth INTEGER NOT NULL,
            is_dir INTEGER NOT NULL,
            logical_size INTEGER NOT NULL,
            allocated_size INTEGER NOT NULL,
            file_count INTEGER NOT NULL,
            dir_count INTEGER NOT NULL,
            mtime INTEGER NULL,
            atime INTEGER NULL,
            FOREIGN KEY(scan_id) REFERENCES scans(id) ON DELETE CASCADE
        )"#,
    )
    .execute(pool)
    .await?;

    // files table (individual files for Top-N queries)
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scan_id TEXT NOT NULL,
            path TEXT NOT NULL,
            parent_path TEXT NULL,
            logical_size INTEGER NOT NULL,
            allocated_size INTEGER NOT NULL,
            mtime INTEGER NULL,
            atime INTEGER NULL,
            FOREIGN KEY(scan_id) REFERENCES scans(id) ON DELETE CASCADE
        )"#,
    )
    .execute(pool)
    .await?;

    // FIX Bug #56 - Better error detection for migrations
    // Add timestamp columns if they don't exist (migrations)
    for (table, column) in [("nodes", "mtime"), ("nodes", "atime"), ("files", "mtime"), ("files", "atime")] {
        let query = format!("ALTER TABLE {} ADD COLUMN {} INTEGER NULL", table, column);
        if let Err(e) = sqlx::query(&query).execute(pool).await {
            // Check if it's a benign "column already exists" error
            match &e {
                sqlx::Error::Database(db_err) => {
                    let msg = db_err.message().to_lowercase();
                    if !msg.contains("duplicate") && !msg.contains("already exists") {
                        tracing::error!("Failed to add {} column to {}: {}", column, table, e);
                        return Err(anyhow::anyhow!("Migration failed: {}", e));
                    }
                }
                _ => {
                    tracing::error!("Unexpected error adding {} to {}: {}", column, table, e);
                    return Err(anyhow::anyhow!("Migration failed: {}", e));
                }
            }
        }
    }

    // FIX Bug #62 - Log index creation failures
    let indexes = [
        ("idx_scans_status_started", "CREATE INDEX IF NOT EXISTS idx_scans_status_started ON scans(status, started_at DESC)"),
        ("idx_warnings_scan", "CREATE INDEX IF NOT EXISTS idx_warnings_scan ON warnings(scan_id)"),
        ("idx_nodes_scan_path", "CREATE INDEX IF NOT EXISTS idx_nodes_scan_path ON nodes(scan_id, path)"),
        ("idx_nodes_scan_isdir", "CREATE INDEX IF NOT EXISTS idx_nodes_scan_isdir ON nodes(scan_id, is_dir)"),
        ("idx_nodes_scan_parent", "CREATE INDEX IF NOT EXISTS idx_nodes_scan_parent ON nodes(scan_id, parent_path)"),
        ("idx_nodes_scan_isdir_alloc_desc", "CREATE INDEX IF NOT EXISTS idx_nodes_scan_isdir_alloc_desc ON nodes(scan_id, is_dir, allocated_size DESC)"),
        ("idx_files_scan_parent", "CREATE INDEX IF NOT EXISTS idx_files_scan_parent ON files(scan_id, parent_path)"),
        ("idx_files_scan_size", "CREATE INDEX IF NOT EXISTS idx_files_scan_size ON files(scan_id, allocated_size DESC)"),
        ("idx_files_scan_path", "CREATE INDEX IF NOT EXISTS idx_files_scan_path ON files(scan_id, path)"),
    ];

    // FIX Bug #39: Better handling of duplicate index creation
    for (name, query) in indexes {
        if let Err(e) = sqlx::query(query).execute(pool).await {
            // Check if it's a "already exists" error
            match &e {
                sqlx::Error::Database(db_err) => {
                    let msg = db_err.message().to_lowercase();
                    if msg.contains("already exists") || msg.contains("duplicate") {
                        tracing::debug!("Index {} already exists, skipping", name);
                    } else {
                        tracing::warn!("Failed to create index {}: {}", name, e);
                    }
                }
                _ => {
                    tracing::warn!("Failed to create index {}: {}", name, e);
                }
            }
        }
    }

    Ok(())
}
