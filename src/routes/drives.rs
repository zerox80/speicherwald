//! Drive management and enumeration API endpoints.
//!
//! This module provides HTTP endpoints for discovering and retrieving information
//! about storage drives available to the system. The implementation is platform-specific,
//! with full functionality on Windows and a fallback implementation for other platforms.
//!
//! ## Features
//!
//! - **Windows**: Full drive enumeration with type detection and space information
//! - **Network Drives**: Timeout-protected network drive queries
//! - **Cross-platform**: Graceful fallback on non-Windows systems
//! - **Rate Limiting**: Per-endpoint rate limiting to prevent abuse

use axum::{
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

use crate::state::AppState;
use crate::{middleware::ip::{extract_ip_from_headers, MaybeRemoteAddr}, types::DriveInfo};

/// Response structure for the drives listing endpoint.
///
/// This wrapper structure provides a consistent response format
/// for the drives API endpoint.
#[derive(Serialize)]
struct DrivesResponse {
    /// List of available drives with their metadata
    items: Vec<DriveInfo>,
}

/// (Windows specific) Lists the available drives and their storage information.
///
/// This function uses the Windows API to enumerate logical drives and retrieve
/// their type, total size, and free space.
///
/// # Arguments
///
/// * `state` - The application state.
/// * `maybe_remote` - The optional remote address of the client.
/// * `headers` - The request headers.
///
/// # Returns
///
/// * `Response` - A JSON response containing a list of `DriveInfo` objects.
#[cfg(windows)]
pub async fn list_drives(
    State(state): State<AppState>,
    maybe_remote: MaybeRemoteAddr,
    headers: HeaderMap,
) -> Response {
    use std::time::Duration;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};

    // Per-endpoint rate limit: "/drives"
    let fallback_ip = maybe_remote.0.map(|addr| addr.ip());
    let ip = extract_ip_from_headers(&headers, fallback_ip);
    if let Err((status, body)) = state.rate_limiter.check_endpoint_limit("/drives", ip).await {
        return (status, body).into_response();
    }

    // 1. Enumerate drives and types (fast, blocking)
    let drive_candidates = tokio::task::spawn_blocking(move || {
        let mut candidates = Vec::new();
        unsafe {
            let mask = GetLogicalDrives();
            if mask == 0 {
                tracing::error!("GetLogicalDrives failed");
                return candidates;
            }
            for i in 0..26u32 {
                if (mask & (1u32 << i)) == 0 { continue; }
                let letter = (b'A' + (i as u8)) as char;
                let path = format!("{}:\\", letter);
                let w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

                let dtype = GetDriveTypeW(PCWSTR(w.as_ptr()));
                if dtype == 0 || dtype == 1 { continue; }
                
                let type_str = match dtype {
                    0 => "unknown", 1 => "invalid", 2 => "removable", 3 => "fixed",
                    4 => "network", 5 => "cdrom", 6 => "ramdisk", _ => "other",
                };
                candidates.push((path, type_str.to_string(), dtype == 4)); // dtype 4 is network
            }
        }
        candidates
    })
    .await
    .unwrap_or_else(|e| {
        tracing::error!("Drive enumeration failed: {}", e);
        Vec::new()
    });

    // 2. Query space info with bounded concurrency (async)
    // FIX Bug #5: Use buffer_unordered to limit concurrent threads
    use futures::stream::{self, StreamExt};

    // Global semaphore to prevent thread exhaustion across multiple requests
    static DRIVE_CHECK_LIMIT: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    let sem = DRIVE_CHECK_LIMIT.get_or_init(|| tokio::sync::Semaphore::new(32));
    
    let items = stream::iter(drive_candidates)
        .map(|(path, drive_type, is_network)| async move {
            let _permit = sem.acquire().await; // Global limit
            let path_clone = path.clone();
            let space_info = if is_network {
                // Network drive: with timeout
                let timeout_ms = std::env::var("SPEICHERWALD_NETWORK_DRIVE_TIMEOUT_MS")
                    .ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(1000).clamp(100, 5000);
                
                let result = tokio::time::timeout(
                    Duration::from_millis(timeout_ms),
                    tokio::task::spawn_blocking(move || {
                        get_drive_space(&path_clone)
                    })
                ).await;

                match result {
                    Ok(Ok(info)) => info, // Success
                    Ok(Err(e)) => { // Panic in task
                        tracing::error!("Drive space task panicked: {}", e);
                        (0, 0, 0)
                    },
                    Err(_) => { // Timeout
                        // tracing::warn!("Timeout getting space for {}", path); 
                        (0, 0, 0) 
                    }
                }
            } else {
                // Local drive: plain blocking
                 tokio::task::spawn_blocking(move || {
                    get_drive_space(&path_clone)
                }).await.unwrap_or((0, 0, 0))
            };
            
            DriveInfo {
                path,
                drive_type,
                total_bytes: space_info.1,
                free_bytes: space_info.2,
            }
        })
        .buffer_unordered(8) // process at most 8 drives concurrently
        .collect::<Vec<_>>()
        .await;


    Json(DrivesResponse { items }).into_response()
}

#[cfg(windows)]
fn get_drive_space(path: &str) -> (u64, u64, u64) {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut free: u64 = 0;
    let mut total: u64 = 0;
    let mut total_free: u64 = 0;
    unsafe {
        let _ = GetDiskFreeSpaceExW(
            PCWSTR(w.as_ptr()),
            Some(&mut free),
            Some(&mut total),
            Some(&mut total_free),
        );
    }
    (free, total, total_free)
}

/// (Non-Windows) Fallback implementation for listing drives.
///
/// This function returns an empty list of drives, as the drive enumeration
/// logic is specific to the Windows API.
///
/// # Arguments
///
/// * `state` - The application state.
/// * `maybe_remote` - The optional remote address of the client.
/// * `headers` - The request headers.
///
/// # Returns
///
/// * `Response` - A JSON response containing an empty list.
#[cfg(not(windows))]
pub async fn list_drives(
    State(state): State<AppState>,
    maybe_remote: MaybeRemoteAddr,
    headers: HeaderMap,
) -> Response {
    // Per-endpoint rate limit: "/drives"
    let fallback_ip = maybe_remote.0.map(|addr| addr.ip());
    let ip = extract_ip_from_headers(&headers, fallback_ip);
    if let Err((status, body)) = state.rate_limiter.check_endpoint_limit("/drives", ip).await {
        return (status, body).into_response();
    }
    // Fallback für Nicht-Windows: leere Liste zurückgeben.
    Json(DrivesResponse { items: Vec::new() }).into_response()
}
