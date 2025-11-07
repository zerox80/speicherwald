use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::anyhow;
use axum::{
    extract::{connect_info::ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use tokio::task::spawn_blocking;
use walkdir::WalkDir;
use std::net::SocketAddr;

use crate::{
    error::{AppError, AppResult},
    middleware::{
        ip::extract_ip_from_headers,
        validation::{sanitize_for_logging, validate_file_path},
    },
    routes::paths_helpers::get_volume_root,
    state::AppState,
    types::{MovePathRequest, MovePathResponse},
};

struct MoveOutcome {
    bytes_to_transfer: u64,
    bytes_moved: u64,
    freed_bytes: u64,
    warnings: Vec<String>,
}

pub async fn move_path(
    State(state): State<AppState>,
    maybe_remote: Option<ConnectInfo<SocketAddr>>,
    headers: HeaderMap,
    Json(req): Json<MovePathRequest>,
) -> AppResult<Response> {
    let fallback_ip = maybe_remote.map(|ConnectInfo(addr)| addr.ip());
    let ip = extract_ip_from_headers(&headers, fallback_ip);
    if let Err((status, body)) = state.rate_limiter.check_endpoint_limit("/paths/move", ip).await {
        return Ok((status, body).into_response());
    }

    let source_trimmed = req.source.trim();
    if source_trimmed.is_empty() {
        return Err(AppError::BadRequest("source path must not be empty".into()));
    }
    let dest_trimmed = req.destination.trim();
    if dest_trimmed.is_empty() {
        return Err(AppError::BadRequest("destination path must not be empty".into()));
    }

    let source_valid = match validate_file_path(source_trimmed) {
        Ok(path) => path,
        Err((status, body)) => return Ok((status, body).into_response()),
    };
    let dest_valid = match validate_file_path(dest_trimmed) {
        Ok(path) => path,
        Err((status, body)) => return Ok((status, body).into_response()),
    };

    if source_valid.eq_ignore_ascii_case(&dest_valid) {
        return Err(AppError::BadRequest("source and destination must be different".into()));
    }

    tracing::info!(
        "Move request: '{}' -> '{}' (remove_source={}, overwrite={})",
        sanitize_for_logging(&source_valid),
        sanitize_for_logging(&dest_valid),
        req.remove_source,
        req.overwrite
    );

    let started_at = Utc::now();
    let started_instant = Instant::now();

    let mut job_req = req.clone();
    job_req.source = source_valid.clone();
    job_req.destination = dest_valid.clone();

    // FIX Bug #36: Add timeout to blocking operations (30 minutes for large file ops)
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(1800),
        spawn_blocking(move || perform_move(job_req))
    )
    .await
    .map_err(|_| AppError::ServiceUnavailable("File operation timed out after 30 minutes".into()))?
    .map_err(|e| AppError::Internal(anyhow!("move task join error: {}", e)))??;

    let duration_ms = started_instant.elapsed().as_millis();
    let response = MovePathResponse {
        status: "completed".to_string(),
        source: source_valid,
        destination: dest_valid,
        bytes_to_transfer: outcome.bytes_to_transfer,
        bytes_moved: outcome.bytes_moved,
        freed_bytes: outcome.freed_bytes,
        duration_ms,
        started_at: started_at.to_rfc3339(),
        finished_at: Utc::now().to_rfc3339(),
        warnings: outcome.warnings,
    };

    Ok((StatusCode::OK, Json(response)).into_response())
}

fn perform_move(req: MovePathRequest) -> AppResult<MoveOutcome> {
    let source_path = PathBuf::from(&req.source);
    if !source_path.exists() {
        return Err(AppError::NotFound(format!("source path does not exist: {}", req.source)));
    }

    let dest_path = PathBuf::from(&req.destination);
    if dest_path.starts_with(&source_path) {
        return Err(AppError::BadRequest("destination cannot be inside the source path".into()));
    }

    if let Some(parent) = dest_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    } else if !dest_path.exists() {
        return Err(AppError::BadRequest("destination path must include a parent directory".into()));
    }

    let metadata = fs::metadata(&source_path)?;
    let mut warnings = Vec::new();
    let bytes_to_transfer =
        if metadata.is_dir() { compute_directory_size(&source_path, &mut warnings)? } else { metadata.len() };

    // FIX Bug #34: Check available disk space before proceeding
    if !req.remove_source {
        // For copy operations, check if destination has enough space
        if let Some(parent) = dest_path.parent() {
            #[cfg(windows)]
            {
                use std::os::windows::ffi::OsStrExt;
                use windows::core::PCWSTR;
                use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
                
                let root_path = get_volume_root(parent);
                let w: Vec<u16> = std::ffi::OsStr::new(&root_path)
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                
                unsafe {
                    let mut free_bytes_available = 0u64;
                    if GetDiskFreeSpaceExW(
                        PCWSTR(w.as_ptr()),
                        Some(&mut free_bytes_available),
                        None,
                        None,
                    ).is_ok() {
                        // Add 10% buffer for safety
                        let required = bytes_to_transfer + (bytes_to_transfer / 10);
                        if free_bytes_available < required {
                            return Err(AppError::BadRequest(format!(
                                "Insufficient disk space: {} available, {} required",
                                free_bytes_available, required
                            )));
                        }
                    }
                }
            }
            #[cfg(unix)]
            {
                // Unix disk space check could be added here using statvfs
                tracing::debug!("Disk space check not implemented on Unix");
            }
        }
    }

    let bytes_moved = if metadata.is_file() {
        move_file(&source_path, &dest_path, &req)?
    } else if metadata.is_dir() {
        move_directory(&source_path, &dest_path, &req, &mut warnings)?
    } else {
        return Err(AppError::BadRequest("source must refer to a file or directory".into()));
    };

    let freed_bytes = if req.remove_source { bytes_to_transfer } else { 0 };

    Ok(MoveOutcome { bytes_to_transfer, bytes_moved, freed_bytes, warnings })
}

fn move_file(source: &Path, destination: &Path, req: &MovePathRequest) -> AppResult<u64> {
    if destination.exists() {
        let dest_meta = fs::metadata(destination)?;
        if dest_meta.is_dir() {
            return Err(AppError::Conflict(format!(
                "destination refers to a directory: {}",
                destination.display()
            )));
        }
        if req.overwrite {
            fs::remove_file(destination)?;
        } else {
            return Err(AppError::Conflict(format!(
                "destination file already exists: {}",
                destination.display()
            )));
        }
    }

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }

    if req.remove_source {
        match fs::rename(source, destination) {
            Ok(_) => return Ok(fs::metadata(destination)?.len()),
            Err(err) => {
                tracing::info!(
                    "Rename failed for file {} ({}), falling back to copy",
                    source.display(),
                    err.kind()
                );
                let copied = copy_file(source, destination)?;
                fs::remove_file(source)?;
                return Ok(copied);
            }
        }
    }

    copy_file(source, destination)
}

fn move_directory(
    source: &Path,
    destination: &Path,
    req: &MovePathRequest,
    warnings: &mut Vec<String>,
) -> AppResult<u64> {
    if destination.exists() {
        let dest_meta = fs::metadata(destination)?;
        if !dest_meta.is_dir() {
            return Err(AppError::Conflict(format!(
                "destination refers to a file: {}",
                destination.display()
            )));
        }
        if req.overwrite {
            fs::remove_dir_all(destination)?;
        } else {
            return Err(AppError::Conflict(format!(
                "destination directory already exists: {}",
                destination.display()
            )));
        }
    }

    if req.remove_source {
        match fs::rename(source, destination) {
            Ok(_) => return compute_directory_size(destination, warnings),
            Err(err) => {
                tracing::info!(
                    "Rename failed for directory {} ({}), falling back to copy",
                    source.display(),
                    err.kind()
                );
                let bytes = copy_directory(source, destination, req.overwrite, req.remove_source, warnings)?;
                fs::remove_dir_all(source)?;
                return Ok(bytes);
            }
        }
    }

    copy_directory(source, destination, req.overwrite, req.remove_source, warnings)
}

fn copy_file(source: &Path, destination: &Path) -> AppResult<u64> {
    let bytes = fs::copy(source, destination)?;
    Ok(bytes)
}

fn copy_directory(
    source: &Path,
    destination: &Path,
    overwrite: bool,
    remove_source: bool,
    warnings: &mut Vec<String>,
) -> AppResult<u64> {
    // FIX Bug #35: Log errors during rollback instead of silently ignoring
    fn rollback_partial(files: &[PathBuf], dirs: &[PathBuf]) {
        for file in files.iter().rev() {
            if let Err(e) = fs::remove_file(file) {
                tracing::error!("Rollback: failed to remove file {}: {}", file.display(), e);
            }
        }
        for dir in dirs.iter().rev() {
            if let Err(e) = fs::remove_dir_all(dir) {
                tracing::error!("Rollback: failed to remove directory {}: {}", dir.display(), e);
            }
        }
    }

    let mut bytes_copied = 0u64;
    let dest_existed = destination.exists();
    let mut created_files: Vec<PathBuf> = Vec::new();
    let mut created_dirs: Vec<PathBuf> = Vec::new();
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    if !dest_existed {
        fs::create_dir_all(destination)?;
        created_dirs.push(destination.to_path_buf());
    }

    for entry in WalkDir::new(source).into_iter() {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                warnings.push(format!("Eintrag uebersprungen: {}", e));
                continue;
            }
        };
        let rel = match entry.path().strip_prefix(source) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if rel.as_os_str().is_empty() {
            continue;
        }

        let target = destination.join(rel);
        if entry.file_type().is_dir() {
            if let Err(e) = fs::create_dir_all(&target) {
                warnings.push(format!(
                    "Ordner konnte nicht erstellt werden ({}): {}",
                    e.kind(),
                    target.display()
                ));
                if remove_source {
                    rollback_partial(&created_files, &created_dirs);
                }
                continue;
            }
            created_dirs.push(target.clone());
            continue;
        }

        // FIX Bug #33: Better handling of symlinks and junction points on Windows
        let file_type = entry.file_type();
        if file_type.is_symlink() {
            warnings.push(format!("Symlink uebersprungen (manuell pruefen): {}", entry.path().display()));
            continue;
        }
        // On Windows, also check for reparse points (junctions)
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if let Ok(metadata) = entry.metadata() {
                const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
                if (metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
                    warnings.push(format!("Reparse point/junction uebersprungen: {}", entry.path().display()));
                    continue;
                }
            }
        }

        if let Some(parent) = target.parent() {
            let parent_missing = !parent.exists();
            if let Err(e) = fs::create_dir_all(parent) {
                warnings.push(format!(
                    "Zielordner konnte nicht erstellt werden ({}): {}",
                    e.kind(),
                    parent.display()
                ));
                continue;
            }
            if parent_missing {
                created_dirs.push(parent.to_path_buf());
            }
        }

        if target.exists() {
            if target.is_dir() {
                if overwrite {
                    fs::remove_dir_all(&target)?;
                } else if remove_source {
                    return Err(AppError::Conflict(format!(
                        "Konflikt: Ziel ist bereits ein Ordner und Ueberschreiben ist deaktiviert ({})",
                        target.display()
                    )));
                } else {
                    warnings.push(format!(
                        "Ordner bereits vorhanden, uebersprungen: {}",
                        target.display()
                    ));
                    continue;
                }
            } else if overwrite {
                fs::remove_file(&target)?;
            } else if remove_source {
                return Err(AppError::Conflict(format!(
                    "Konflikt: Ziel existiert bereits und Ueberschreiben ist deaktiviert ({})",
                    target.display()
                )));
            } else {
                warnings.push(format!("Datei bereits vorhanden, uebersprungen: {}", target.display()));
                continue;
            }
        }

        match fs::copy(entry.path(), &target) {
            Ok(bytes) => {
                bytes_copied += bytes;
                created_files.push(target.clone());
            }
            Err(e) => {
                if remove_source {
                    rollback_partial(&created_files, &created_dirs);
                    return Err(AppError::IoError(format!(
                        "Datei konnte nicht kopiert werden ({}): {}",
                        e.kind(),
                        entry.path().display()
                    )));
                } else {
                    warnings.push(format!(
                        "Datei konnte nicht kopiert werden ({}): {}",
                        e.kind(),
                        entry.path().display()
                    ));
                }
            }
        }
    }

    // FIX Bug #23: Log error instead of silently ignoring
    if bytes_copied == 0 && !dest_existed {
        match destination.read_dir() {
            Ok(mut it) => {
                if it.next().is_none() {
                    let _ = fs::remove_dir_all(destination);
                }
            }
            Err(e) => {
                tracing::warn!("Failed to check if destination is empty: {}", e);
            }
        }
    }

    Ok(bytes_copied)
}

fn compute_directory_size(path: &Path, warnings: &mut Vec<String>) -> AppResult<u64> {
    let mut total = 0u64;
    for entry in WalkDir::new(path).into_iter() {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                warnings.push(format!("Eintrag uebersprungen: {}", e));
                continue;
            }
        };
        if entry.file_type().is_file() {
            match entry.metadata() {
                Ok(meta) => total += meta.len(),
                Err(e) => warnings.push(format!(
                    "Metadaten konnten nicht gelesen werden: {} ({})",
                    entry.path().display(),
                    e
                )),
            }
        }
    }
    Ok(total)
}
