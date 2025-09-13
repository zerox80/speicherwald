#[cfg(test)]
mod tests {
    use crate::db;
    use sqlx::sqlite::SqlitePoolOptions;
    use tempfile::NamedTempFile;
    use uuid::Uuid;

    async fn setup_test_db() -> sqlx::SqlitePool {
        let temp_db = NamedTempFile::new().unwrap();
        let db_url = format!("sqlite:{}", temp_db.path().display());
        
        sqlx::Sqlite::create_database(&db_url).await.unwrap();
        
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&db_url)
            .await
            .unwrap();

        db::init_db(&pool).await.unwrap();
        
        pool
    }

    #[tokio::test]
    async fn test_init_db() {
        let pool = setup_test_db().await;
        
        // Check if tables exist
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        
        assert!(tables.contains(&"scans".to_string()));
        assert!(tables.contains(&"warnings".to_string()));
        assert!(tables.contains(&"nodes".to_string()));
        assert!(tables.contains(&"files".to_string()));
    }

    #[tokio::test]
    async fn test_scan_crud() {
        let pool = setup_test_db().await;
        let scan_id = Uuid::new_v4();
        
        // Create scan
        sqlx::query(
            "INSERT INTO scans (id, status, root_paths, options) VALUES (?1, ?2, ?3, ?4)"
        )
        .bind(scan_id.to_string())
        .bind("running")
        .bind("[\"C:/test\"]")
        .bind("{}")
        .execute(&pool)
        .await
        .unwrap();
        
        // Read scan
        let row = sqlx::query("SELECT * FROM scans WHERE id = ?1")
            .bind(scan_id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        
        assert_eq!(row.get::<String, _>("status"), "running");
        
        // Update scan
        sqlx::query("UPDATE scans SET status = 'done' WHERE id = ?1")
            .bind(scan_id.to_string())
            .execute(&pool)
            .await
            .unwrap();
        
        let updated = sqlx::query("SELECT status FROM scans WHERE id = ?1")
            .bind(scan_id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        
        assert_eq!(updated.get::<String, _>("status"), "done");
        
        // Delete scan
        sqlx::query("DELETE FROM scans WHERE id = ?1")
            .bind(scan_id.to_string())
            .execute(&pool)
            .await
            .unwrap();
        
        let deleted = sqlx::query("SELECT COUNT(*) as count FROM scans WHERE id = ?1")
            .bind(scan_id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        
        assert_eq!(deleted.get::<i64, _>("count"), 0);
    }

    #[tokio::test]
    async fn test_nodes_insertion() {
        let pool = setup_test_db().await;
        let scan_id = Uuid::new_v4();
        
        // Create scan first
        sqlx::query(
            "INSERT INTO scans (id, status, root_paths, options) VALUES (?1, 'done', '[]', '{}')"
        )
        .bind(scan_id.to_string())
        .execute(&pool)
        .await
        .unwrap();
        
        // Insert nodes
        sqlx::query(
            "INSERT INTO nodes (scan_id, path, parent_path, depth, is_dir, logical_size, allocated_size, file_count, dir_count) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"
        )
        .bind(scan_id.to_string())
        .bind("C:/test")
        .bind(None::<String>)
        .bind(1)
        .bind(1)
        .bind(1024)
        .bind(4096)
        .bind(10)
        .bind(5)
        .execute(&pool)
        .await
        .unwrap();
        
        let node = sqlx::query("SELECT * FROM nodes WHERE scan_id = ?1")
            .bind(scan_id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        
        assert_eq!(node.get::<String, _>("path"), "C:/test");
        assert_eq!(node.get::<i64, _>("logical_size"), 1024);
        assert_eq!(node.get::<i64, _>("file_count"), 10);
    }

    #[tokio::test]
    async fn test_cascade_delete() {
        let pool = setup_test_db().await;
        let scan_id = Uuid::new_v4();
        
        // Create scan with related data
        sqlx::query(
            "INSERT INTO scans (id, status, root_paths, options) VALUES (?1, 'done', '[]', '{}')"
        )
        .bind(scan_id.to_string())
        .execute(&pool)
        .await
        .unwrap();
        
        // Add warning
        sqlx::query(
            "INSERT INTO warnings (scan_id, path, code, message) VALUES (?1, '/test', 'test', 'test message')"
        )
        .bind(scan_id.to_string())
        .execute(&pool)
        .await
        .unwrap();
        
        // Add node
        sqlx::query(
            "INSERT INTO nodes (scan_id, path, parent_path, depth, is_dir, logical_size, allocated_size, file_count, dir_count) 
             VALUES (?1, '/test', NULL, 1, 1, 0, 0, 0, 0)"
        )
        .bind(scan_id.to_string())
        .execute(&pool)
        .await
        .unwrap();
        
        // Add file
        sqlx::query(
            "INSERT INTO files (scan_id, path, parent_path, logical_size, allocated_size) 
             VALUES (?1, '/test/file.txt', '/test', 100, 4096)"
        )
        .bind(scan_id.to_string())
        .execute(&pool)
        .await
        .unwrap();
        
        // Verify data exists
        let warning_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM warnings WHERE scan_id = ?1")
            .bind(scan_id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(warning_count, 1);
        
        // Delete scan - should cascade
        sqlx::query("DELETE FROM scans WHERE id = ?1")
            .bind(scan_id.to_string())
            .execute(&pool)
            .await
            .unwrap();
        
        // Verify all related data is deleted
        let warning_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM warnings WHERE scan_id = ?1")
            .bind(scan_id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(warning_count, 0);
        
        let node_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM nodes WHERE scan_id = ?1")
            .bind(scan_id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(node_count, 0);
        
        let file_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files WHERE scan_id = ?1")
            .bind(scan_id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(file_count, 0);
    }
}
