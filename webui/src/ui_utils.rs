use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

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

/// Trigger a download from a regular URL (server-provided content)
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

// Copy text to clipboard and show a toast on success
pub fn copy_to_clipboard(text: String) {
    if let Some(win) = web_sys::window() {
        let nav = win.navigator();
        let clip = nav.clipboard();
        let promise = clip.write_text(&text);
        wasm_bindgen_futures::spawn_local(async move {
            let _ = JsFuture::from(promise).await;
            show_toast("In Zwischenablage kopiert");
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
                    let _ = container.append_child(&toast);

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
