use sqlx::SqlitePool;

pub async fn init_db(pool: &SqlitePool) -> anyhow::Result<()> {
    // Pragmas for better durability/performance
    sqlx::query("PRAGMA journal_mode=WAL;").execute(pool).await.ok();
    sqlx::query("PRAGMA synchronous=NORMAL;").execute(pool).await.ok();
    sqlx::query("PRAGMA foreign_keys=ON;").execute(pool).await.ok();
    // Additional tuning (best-effort)
    sqlx::query("PRAGMA busy_timeout=10000;").execute(pool).await.ok();
    // negative cache_size means KB; here ~64MB
    sqlx::query("PRAGMA cache_size=-65536;").execute(pool).await.ok();
    sqlx::query("PRAGMA temp_store=MEMORY;").execute(pool).await.ok();
    // Enable mmap to reduce syscall overhead (256MB)
    sqlx::query("PRAGMA mmap_size=268435456;").execute(pool).await.ok();

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
            FOREIGN KEY(scan_id) REFERENCES scans(id) ON DELETE CASCADE
        )"#,
    )
    .execute(pool)
    .await?;

    // helpful indexes
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_scans_status_started ON scans(status, started_at DESC);")
        .execute(pool)
        .await
        .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_warnings_scan ON warnings(scan_id);")
        .execute(pool)
        .await
        .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_nodes_scan_path ON nodes(scan_id, path);")
        .execute(pool)
        .await
        .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_nodes_scan_isdir ON nodes(scan_id, is_dir);")
        .execute(pool)
        .await
        .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_nodes_scan_parent ON nodes(scan_id, parent_path);")
        .execute(pool)
        .await
        .ok();
    // Improve ORDER BY allocated_size DESC for dirs
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_nodes_scan_isdir_alloc_desc ON nodes(scan_id, is_dir, allocated_size DESC);")
        .execute(pool).await.ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_files_scan_parent ON files(scan_id, parent_path);")
        .execute(pool)
        .await
        .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_files_scan_size ON files(scan_id, allocated_size DESC);")
        .execute(pool)
        .await
        .ok();
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_files_scan_path ON files(scan_id, path);")
        .execute(pool)
        .await
        .ok();

    Ok(())
}
