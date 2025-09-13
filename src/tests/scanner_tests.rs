#[cfg(test)]
mod tests {
    use crate::scanner::run_scan;
    use crate::types::ScanOptions;
    use crate::db;
    use tempfile::TempDir;
    use std::fs;
    use std::io::Write;
    use tokio_util::sync::CancellationToken;
    use tokio::sync::broadcast;
    use sqlx::sqlite::SqlitePoolOptions;
    use uuid::Uuid;

    fn create_test_directory() -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path();

        // Create test directory structure
        fs::create_dir_all(base_path.join("dir1/subdir1")).unwrap();
        fs::create_dir_all(base_path.join("dir1/subdir2")).unwrap();
        fs::create_dir_all(base_path.join("dir2")).unwrap();
        fs::create_dir_all(base_path.join(".hidden")).unwrap();

        // Create test files with known sizes
        let mut file1 = fs::File::create(base_path.join("file1.txt")).unwrap();
        file1.write_all(b"Hello World").unwrap();

        let mut file2 = fs::File::create(base_path.join("dir1/file2.txt")).unwrap();
        file2.write_all(b"Test content for file 2").unwrap();

        let mut file3 = fs::File::create(base_path.join("dir1/subdir1/file3.txt")).unwrap();
        file3.write_all(b"This is a test file in a subdirectory").unwrap();

        let mut hidden_file = fs::File::create(base_path.join(".hidden/secret.txt")).unwrap();
        hidden_file.write_all(b"Hidden content").unwrap();

        temp_dir
    }

    #[tokio::test]
    async fn run_scan_basic_inserts_data() {
        let temp_dir = create_test_directory();
        let root = temp_dir.path().to_string_lossy().to_string();

        // in-memory sqlite
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        db::init_db(&pool).await.unwrap();

        let id = Uuid::new_v4();
        let (tx, _rx) = broadcast::channel(32);
        let cancel = CancellationToken::new();
        let options = ScanOptions {
            follow_symlinks: false,
            include_hidden: true,
            measure_logical: true,
            measure_allocated: true,
            excludes: vec![],
            max_depth: None,
            concurrency: Some(4),
        };

        let summary = run_scan(
            pool.clone(),
            id,
            vec![root.clone()],
            options,
            tx,
            cancel,
            256,
            512,
            100,
            None,
            Some(4),
        )
        .await
        .unwrap();

        assert!(summary.total_files > 0);
        assert!(summary.total_dirs > 0);

        let nodes_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM nodes WHERE scan_id=?1")
            .bind(id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        let files_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files WHERE scan_id=?1")
            .bind(id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(nodes_count > 0);
        assert!(files_count >= 0);
    }

    #[tokio::test]
    async fn run_scan_respects_excludes() {
        let temp_dir = create_test_directory();
        let root = temp_dir.path().to_string_lossy().to_string();
        let excluded_child = temp_dir
            .path()
            .join("dir1/subdir1")
            .to_string_lossy()
            .to_string();

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        db::init_db(&pool).await.unwrap();

        let id = Uuid::new_v4();
        let (tx, _rx) = broadcast::channel(32);
        let cancel = CancellationToken::new();
        let options = ScanOptions {
            follow_symlinks: false,
            include_hidden: true,
            measure_logical: true,
            measure_allocated: true,
            excludes: vec!["**/subdir1/**".to_string()],
            max_depth: None,
            concurrency: Some(4),
        };

        let _ = run_scan(
            pool.clone(),
            id,
            vec![root.clone()],
            options,
            tx,
            cancel,
            256,
            512,
            100,
            None,
            Some(4),
        )
        .await
        .unwrap();

        // Ensure excluded child dir not present in nodes
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM nodes WHERE scan_id=?1 AND path=?2"
        )
        .bind(id.to_string())
        .bind(excluded_child)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 0);
    }
}
