use axum::{
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

use crate::state::AppState;
use crate::{middleware::ip::{extract_ip_from_headers, MaybeRemoteAddr}, types::DriveInfo};

#[derive(Serialize)]
struct DrivesResponse {
    items: Vec<DriveInfo>,
}

#[cfg(windows)]
pub async fn list_drives(
    State(state): State<AppState>,
    maybe_remote: MaybeRemoteAddr,
    headers: HeaderMap,
) -> Response {
    use std::{sync::mpsc, thread, time::Duration};
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives};

    // Per-endpoint rate limit: "/drives"
    let fallback_ip = maybe_remote.0.map(|addr| addr.ip());
    let ip = extract_ip_from_headers(&headers, fallback_ip);
    if let Err((status, body)) = state.rate_limiter.check_endpoint_limit("/drives", ip).await {
        return (status, body).into_response();
    }

    let mut items: Vec<DriveInfo> = Vec::new();

    unsafe {
        let mask = GetLogicalDrives();
        // Check if GetLogicalDrives failed (returns 0 on error)
        if mask == 0 {
            tracing::error!("GetLogicalDrives failed");
            return Json(DrivesResponse { items }).into_response();
        }
        for i in 0..26u32 {
            if (mask & (1u32 << i)) == 0 {
                continue;
            }
            let letter = (b'A' + (i as u8)) as char;
            let path = format!("{}:\\", letter);
            let w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

            let dtype = GetDriveTypeW(PCWSTR(w.as_ptr()));
            // Skip invalid/unknown drive types (0 = unknown/error, 1 = invalid)
            if dtype == 0 || dtype == 1 {
                continue;
            }
            let drive_type = match dtype {
                0 => "unknown",
                1 => "invalid",
                2 => "removable",
                3 => "fixed",
                4 => "network",
                5 => "cdrom",
                6 => "ramdisk",
                _ => "other",
            }
            .to_string();

            // Query free/total bytes
            let (_free, total, total_free) = if dtype == 4 {
                // network drive
                // Run the blocking call in a short-lived thread with timeout to avoid UI hangs
                // Configurable timeout for network drives (default 1000ms for slower connections)
                let timeout_ms = std::env::var("SPEICHERWALD_NETWORK_DRIVE_TIMEOUT_MS")
                    .ok()
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(1000)
                    .clamp(100, 5000);
                let (tx, rx) = mpsc::channel();
                let w2 = w.clone();
                thread::spawn(move || {
                    let mut free: u64 = 0;
                    let mut total: u64 = 0;
                    let mut total_free: u64 = 0;
                    let _ = GetDiskFreeSpaceExW(
                        PCWSTR(w2.as_ptr()),
                        Some(&mut free),
                        Some(&mut total),
                        Some(&mut total_free),
                    );
                    let _ = tx.send((free, total, total_free));
                });
                rx.recv_timeout(Duration::from_millis(timeout_ms)).unwrap_or_default()
            } else {
                let mut free: u64 = 0;
                let mut total: u64 = 0;
                let mut total_free: u64 = 0;
                let _ = GetDiskFreeSpaceExW(
                    PCWSTR(w.as_ptr()),
                    Some(&mut free),
                    Some(&mut total),
                    Some(&mut total_free),
                );
                (free, total, total_free)
            };

            items.push(DriveInfo { path, drive_type, total_bytes: total, free_bytes: total_free });
        }
    }

    Json(DrivesResponse { items }).into_response()
}

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
