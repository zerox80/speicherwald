use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::anyhow;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use tokio::task::spawn_blocking;
use walkdir::WalkDir;

use crate::{
    error::{AppError, AppResult},
    middleware::{
        ip::extract_ip_from_headers,
        validation::{sanitize_for_logging, validate_file_path},
    },
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
    headers: HeaderMap,
    Json(req): Json<MovePathRequest>,
) -> AppResult<Response> {
    let ip = extract_ip_from_headers(&headers, None);
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

    let outcome = spawn_blocking(move || perform_move(job_req))
        .await
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
        if req.overwrite {
            let dest_meta = fs::metadata(destination)?;
            if dest_meta.is_dir() {
                fs::remove_dir_all(destination)?;
            } else {
                fs::remove_file(destination)?;
            }
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
        if req.overwrite {
            let dest_meta = fs::metadata(destination)?;
            if dest_meta.is_dir() {
                fs::remove_dir_all(destination)?;
            } else {
                fs::remove_file(destination)?;
            }
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
    let mut bytes_copied = 0u64;
    let dest_existed = destination.exists();
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    if !dest_existed {
        fs::create_dir_all(destination)?;
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
            }
            continue;
        }

        if entry.file_type().is_symlink() {
            warnings.push(format!("Symlink uebersprungen (manuell pruefen): {}", entry.path().display()));
            continue;
        }

        if let Some(parent) = target.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                warnings.push(format!(
                    "Zielordner konnte nicht erstellt werden ({}): {}",
                    e.kind(),
                    parent.display()
                ));
                continue;
            }
        }

        if target.exists() {
            if overwrite {
                if target.is_file() {
                    fs::remove_file(&target)?;
                } else {
                    fs::remove_dir_all(&target)?;
                }
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
            Ok(bytes) => bytes_copied += bytes,
            Err(e) => {
                if remove_source {
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

    if bytes_copied == 0 && !dest_existed {
        if destination.read_dir().map(|mut it| it.next().is_none()).unwrap_or(false) {
            let _ = fs::remove_dir_all(destination);
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
