use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen::JsValue;
use js_sys::Date;

// Format bytes using binary units
pub fn fmt_bytes(n: i64) -> String {
    let mut v = n as f64;
    let units = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut i = 0usize;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if v >= 10.0 {
        format!("{:.0} {}", v, units[i])
    } else {
        format!("{:.1} {}", v, units[i])
    }
}

// Format a short relative time label: e.g. 3M (months), 2Y (years), 12D (days).
// Accepts timestamps in various epoch units (seconds, ms, µs, ns). For sub-day differences show hours/minutes. If None, show "—".
pub fn fmt_ago_short(ts: Option<i64>) -> String {
    match ts {
        Some(secs) => {
            // FIX Bug #25 - Better overflow validation
            if secs <= 0 || secs > 253402300799 { // Max valid Unix timestamp (year 9999)
                return "—".to_string();
            }
            
            let now_sec = Date::now() / 1000.0;
            let base = secs as f64;
            let mut best = base;
            let mut best_diff = (now_sec - base).abs();
            
            // FIX Bug #26 - Safely calculate candidates with overflow protection
            let candidates = [
                base,
                base / 1_000.0,
                base / 1_000_000.0,
                base / 1_000_000_000.0,
            ];
            // Add multiplication candidates only if they won't overflow
            // Also check that result is reasonable (not too far in past/future)
            let max_reasonable_ts = now_sec * 3.0; // Allow up to 3x current time
            let mult_candidates: Vec<f64> = vec![
                if base > 0.0 && base < (f64::MAX / 1_000.0) && (base * 1_000.0) < max_reasonable_ts { Some(base * 1_000.0) } else { None },
                if base > 0.0 && base < (f64::MAX / 1_000_000.0) && (base * 1_000_000.0) < max_reasonable_ts { Some(base * 1_000_000.0) } else { None },
            ].into_iter().flatten().collect();
            
            for cand in candidates.iter().chain(mult_candidates.iter()) {
                if !cand.is_finite() || *cand <= 0.0 || *cand > f64::MAX / 2.0 {
                    continue;
                }
                let diff = (now_sec - cand).abs();
                // Only consider candidates that make sense (within 100 years)
                if diff < best_diff && diff < (100.0 * 365.25 * 86400.0) {
                    best_diff = diff;
                    best = *cand;
                }
            }
            
            let diff_sec = (now_sec - best).max(0.0);
            if diff_sec < 60.0 {
                return "<1m".to_string();
            }
            if diff_sec < 3600.0 {
                let minutes = (diff_sec / 60.0).floor() as i64;
                return format!("{}m", minutes.max(1));
            }
            if diff_sec < 86_400.0 {
                let hours = (diff_sec / 3600.0).floor() as i64;
                return format!("{}H", hours.max(1));
            }
            let diff_days = (diff_sec / 86_400.0).floor() as i64;
            // Use more accurate year calculation (365.25 days per year on average)
            if diff_days >= 365 {
                let years = (diff_days as f64 / 365.25).floor() as i64;
                return format!("{}Y", years.max(1));
            }
            // Use more accurate month calculation (30.44 days per month on average)
            if diff_days >= 30 {
                let months = (diff_days as f64 / 30.44).floor() as i64;
                return format!("{}M", months.max(1));
            }
            if diff_days > 0 {
                format!("{}D", diff_days)
            } else {
                "<1D".to_string()
            }
        }
        None => "—".to_string(),
    }
}

/// (removed duplicate trigger_download)

// Copy text to clipboard and show a toast on success
pub fn copy_to_clipboard(text: String) {
    if let Some(win) = web_sys::window() {
        let nav = win.navigator();
        let clip = nav.clipboard();
        let promise = clip.write_text(&text);
        wasm_bindgen_futures::spawn_local(async move {
            match JsFuture::from(promise).await {
                Ok(_) => show_toast("In Zwischenablage kopiert"),
                Err(_) => show_toast("Fehler beim Kopieren"),
            }
        });
    }
}

// Show a transient toast in the #toasts container
pub fn show_toast(message: &str) {
    if let Some(win) = web_sys::window() {
        if let Some(doc) = win.document() {
            if let Some(container) = doc.get_element_by_id("toasts") {
                if let Ok(toast) = doc.create_element("div") {
                    toast.set_class_name("toast fade-in");
                    toast.set_text_content(Some(message));
                    if container.append_child(&toast).is_err() {
                        return; // Failed to append, exit early
                    }

                    // Auto-remove after timeout
                    let container_clone = container.clone();
                    let toast_clone = toast.clone();
                    let cb = Closure::wrap(Box::new(move || {
                        let _ = container_clone.remove_child(&toast_clone);
                    }) as Box<dyn FnMut()>);
                    let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(
                        cb.as_ref().unchecked_ref(),
                        1600,
                    );
                    cb.forget();
                }
            }
        }
    }
}

// Format an optional UNIX timestamp (seconds) into a short local time string.
// Output: "YYYY-MM-DD HH:MM" or "—" if None.
pub fn fmt_time_opt(ts: Option<i64>) -> String {
    match ts {
        Some(secs) => {
            // FIX Bug #24 - Better overflow protection
            // Max valid Unix timestamp (year 9999)
            if secs <= 0 || secs > 253402300799 {
                return "—".to_string();
            }
            // Safe to multiply now (validated range)
            let ms = (secs as f64) * 1000.0;
            // Additional safety check on result
            if !ms.is_finite() || ms < 0.0 {
                return "—".to_string();
            }
            let d = Date::new(&JsValue::from_f64(ms));
            // FIX Bug #37 - Better error handling for date conversion
            let iso = d.to_iso_string();
            let s = match iso.as_string() {
                Some(s) => s,
                None => {
                    let fallback = d.to_string();
                    fallback.as_string().unwrap_or_else(|| "Invalid Date".to_string())
                }
            };
            if let Some((date, time)) = s.split_once('T') {
                let mut hhmm = String::new();
                for (i, ch) in time.chars().enumerate() {
                    if i >= 5 { break; }
                    hhmm.push(ch);
                }
                format!("{} {}", date, hhmm)
            } else {
                s
            }
        }
        None => "—".to_string(),
    }
}

// Trigger a download from a regular URL (server-provided content)
/// If a suggested filename is provided, set the 'download' attribute to hint the browser.
pub fn trigger_download(url: &str, suggested_filename: Option<&str>) {
    if let Some(win) = web_sys::window() {
        if let Some(doc) = win.document() {
            if let Ok(a) = doc.create_element("a") {
                let _ = a.set_attribute("href", url);
                if let Some(name) = suggested_filename {
                    let _ = a.set_attribute("download", name);
                }
                if let Some(body) = doc.body() {
                    let _ = body.append_child(&a);
                    if let Some(ae) = a.dyn_ref::<web_sys::HtmlElement>() {
                        ae.click();
                    }
                    let _ = body.remove_child(&a);
                }
            }
        }
    }
}

// Trigger a CSV download using a data URI
pub fn download_csv(filename: &str, content: &str) {
    if let Some(win) = web_sys::window() {
        if let Some(doc) = win.document() {
            if let Ok(a) = doc.create_element("a") {
                let href = format!(
                    "data:text/csv;charset=utf-8,{}",
                    urlencoding::encode(content)
                );
                let _ = a.set_attribute("href", &href);
                let _ = a.set_attribute("download", filename);
                if let Some(body) = doc.body() {
                    let _ = body.append_child(&a);
                    if let Some(ae) = a.dyn_ref::<web_sys::HtmlElement>() {
                        ae.click();
                    }
                    let _ = body.remove_child(&a);
                }
            }
        }
    }
}
