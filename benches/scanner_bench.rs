use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use speicherwald::db;
use speicherwald::scanner::run_scan;
use speicherwald::types::ScanOptions;
use sqlx::sqlite::SqlitePoolOptions;
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use tokio::runtime::Runtime;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn create_test_tree(depth: usize, files_per_dir: usize, dirs_per_level: usize) -> TempDir {
    let temp_dir = TempDir::new().unwrap();

    fn create_level(
        path: &Path,
        current_depth: usize,
        max_depth: usize,
        files_per_dir: usize,
        dirs_per_level: usize,
    ) {
        if current_depth >= max_depth {
            return;
        }

        // Create files
        for i in 0..files_per_dir {
            let file_path = path.join(format!("file_{}.txt", i));
            fs::write(&file_path, format!("Test content {}", i)).unwrap();
        }

        // Create subdirectories
        for i in 0..dirs_per_level {
            let dir_path = path.join(format!("dir_{}", i));
            fs::create_dir(&dir_path).unwrap();
            create_level(dir_path.as_path(), current_depth + 1, max_depth, files_per_dir, dirs_per_level);
        }
    }

    create_level(temp_dir.path(), 0, depth, files_per_dir, dirs_per_level);
    temp_dir
}

fn benchmark_small_tree(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let temp_dir = create_test_tree(3, 10, 3);
    let path = temp_dir.path().to_str().unwrap().to_string();

    c.bench_function("scan_small_tree", |b| {
        b.iter(|| {
            rt.block_on(async {
                let options = ScanOptions {
                    follow_symlinks: false,
                    include_hidden: true,
                    measure_logical: true,
                    measure_allocated: true,
                    excludes: vec![],
                    max_depth: None,
                    concurrency: Some(4),
                };

                let pool =
                    SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
                db::init_db(&pool).await.unwrap();
                let id = Uuid::new_v4();
                let (tx, _rx) = broadcast::channel(32);
                let cancel = CancellationToken::new();
                black_box(
                    run_scan(pool, id, vec![path.clone()], options, tx, cancel, 256, 512, 100, None, Some(4))
                        .await,
                )
            })
        })
    });
}

fn benchmark_large_tree(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let temp_dir = create_test_tree(4, 20, 4);
    let path = temp_dir.path().to_str().unwrap().to_string();

    c.bench_function("scan_large_tree", |b| {
        b.iter(|| {
            rt.block_on(async {
                let options = ScanOptions {
                    follow_symlinks: false,
                    include_hidden: true,
                    measure_logical: true,
                    measure_allocated: true,
                    excludes: vec![],
                    max_depth: None,
                    concurrency: Some(8),
                };

                let pool =
                    SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
                db::init_db(&pool).await.unwrap();
                let id = Uuid::new_v4();
                let (tx, _rx) = broadcast::channel(32);
                let cancel = CancellationToken::new();
                black_box(
                    run_scan(pool, id, vec![path.clone()], options, tx, cancel, 256, 512, 100, None, Some(8))
                        .await,
                )
            })
        })
    });
}

fn benchmark_concurrency(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let temp_dir = create_test_tree(3, 15, 3);
    let path = temp_dir.path().to_str().unwrap().to_string();

    let mut group = c.benchmark_group("concurrency");

    for concurrency in [1, 2, 4, 8, 16].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(concurrency), concurrency, |b, &concurrency| {
            b.iter(|| {
                rt.block_on(async {
                    let options = ScanOptions {
                        follow_symlinks: false,
                        include_hidden: true,
                        measure_logical: true,
                        measure_allocated: true,
                        excludes: vec![],
                        max_depth: None,
                        concurrency: Some(concurrency),
                    };
                    let pool =
                        SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
                    db::init_db(&pool).await.unwrap();
                    let id = Uuid::new_v4();
                    let (tx, _rx) = broadcast::channel(32);
                    let cancel = CancellationToken::new();
                    black_box(
                        run_scan(
                            pool,
                            id,
                            vec![path.clone()],
                            options,
                            tx,
                            cancel,
                            256,
                            512,
                            100,
                            None,
                            Some(concurrency),
                        )
                        .await,
                    )
                })
            })
        });
    }
    group.finish();
}

fn benchmark_exclude_patterns(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let temp_dir = create_test_tree(3, 10, 3);
    let path = temp_dir.path().to_str().unwrap().to_string();

    let mut group = c.benchmark_group("exclude_patterns");

    group.bench_function("no_excludes", |b| {
        b.iter(|| {
            rt.block_on(async {
                let options = ScanOptions {
                    follow_symlinks: false,
                    include_hidden: true,
                    measure_logical: true,
                    measure_allocated: true,
                    excludes: vec![],
                    max_depth: None,
                    concurrency: Some(4),
                };
                let pool =
                    SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
                db::init_db(&pool).await.unwrap();
                let id = Uuid::new_v4();
                let (tx, _rx) = broadcast::channel(32);
                let cancel = CancellationToken::new();
                black_box(
                    run_scan(pool, id, vec![path.clone()], options, tx, cancel, 256, 512, 100, None, Some(4))
                        .await,
                )
            })
        })
    });

    group.bench_function("with_excludes", |b| {
        b.iter(|| {
            rt.block_on(async {
                let options = ScanOptions {
                    follow_symlinks: false,
                    include_hidden: true,
                    measure_logical: true,
                    measure_allocated: true,
                    excludes: vec!["**/dir_1/**".to_string(), "**/file_5.txt".to_string()],
                    max_depth: None,
                    concurrency: Some(4),
                };
                let pool =
                    SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
                db::init_db(&pool).await.unwrap();
                let id = Uuid::new_v4();
                let (tx, _rx) = broadcast::channel(32);
                let cancel = CancellationToken::new();
                black_box(
                    run_scan(pool, id, vec![path.clone()], options, tx, cancel, 256, 512, 100, None, Some(4))
                        .await,
                )
            })
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_small_tree,
    benchmark_large_tree,
    benchmark_concurrency,
    benchmark_exclude_patterns
);
criterion_main!(benches);
