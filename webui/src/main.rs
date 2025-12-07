//! SpeicherWald Web UI - Main Application Entry Point
//!
//! This is the main frontend application built with Dioxus that provides a web interface
//! for the SpeicherWald file system analysis tool. It handles scan management, real-time
//! updates, data visualization, and user interactions.
//!
//! ## Main Features
//!
//! - **Scan Management**: Create, monitor, and manage file system scans
//! - **Real-time Updates**: Live progress updates via Server-Sent Events
//! - **Data Exploration**: Browse, search, and analyze scan results
//! - **Interactive UI**: File operations, downloads, and data exports
//! - **Responsive Design**: Works across different screen sizes
//!
//! ## Architecture
//!
//! - **Routing**: Simple two-page structure (Home and Scan details)
//! - **State Management**: Uses Dioxus signals and local state
//! - **API Integration**: Type-safe communication with backend
//! - **Real-time**: SSE for live scan progress monitoring
//! - **Error Handling**: User-friendly error messages and fallbacks

use dioxus::events::FormData;
use dioxus::prelude::*;

use dioxus_router::{use_navigator, Link, Routable, Router};
use js_sys::Date;
use web_sys::console;
use std::rc::Rc;

mod api;
mod types;
mod ui_utils;
use ui_utils::{fmt_bytes, fmt_ago_short, copy_to_clipboard, download_csv, trigger_download, show_toast};

/// State for the move/copy dialog functionality.
///
/// Manages the UI state and data for the file move/copy dialog,
/// including source information, destination selection, and operation progress.
#[derive(Debug, Clone, PartialEq)]
struct MoveDialogState {
    source_path: String,
    source_name: String,
    logical_size: i64,
    allocated_size: i64,
    destination: String,
    selected_drive: Option<String>,
    remove_source: bool,
    overwrite: bool,
    in_progress: bool,
    done: bool,
    result: Option<types::MovePathResponse>,
    error: Option<String>,
}

/// Application routing configuration.
///
/// Defines the available routes in the application using Dioxus Router.
/// Currently supports a simple two-page structure with home and scan detail views.
#[derive(Routable, Clone, Debug, PartialEq)]
pub enum Route {
    /// Home page - lists all scans and allows creating new scans
    #[route("/")]
    Home {},
    /// Scan detail page - shows detailed information and exploration for a specific scan
    #[route("/scan/:id")]
    Scan { id: String },
}

/// Main entry point for the Dioxus web application.
///
/// Initializes panic hooks for better error reporting in development
/// and launches the Dioxus application with the root `app` component.
pub fn main() {
    console_error_panic_hook::set_once();
    dioxus_web::launch::launch(app, vec![], Default::default());
}

/// Root application component.
///
/// Renders the main application layout including the header,
/// navigation, router for page content, and toast container
/// for user notifications.
fn app() -> Element {
    rsx! {
        div { // root wrapper
            // App Header
            div { class: "app-header",
                div { class: "container",
                    div { class: "brand",
                        span { "🌲 SpeicherWald" }
                    }
                    nav {
                        Link { to: Route::Home {}, "Home" }
                    }
                }
            }
            // App Content (Router)
            Router::<Route> {}
            // Toast container for notifications
            div { id: "toasts", class: "toast-container" }
        }
    }
}

// ----- Home: einfache Scan-Übersicht -----
#[component]
fn Home() -> Element {
    let scans = use_signal(|| Vec::<types::ScanSummary>::new());
    let new_root = use_signal(|| String::new());
    let server_ok = use_signal(|| None as Option<bool>);
    let drives = use_signal(|| Vec::<types::DriveInfo>::new());
    let home_loading = use_signal(|| true);
    let err_scans = use_signal(|| None as Option<String>);
    let err_drives = use_signal(|| None as Option<String>);
    let err_health = use_signal(|| None as Option<String>);

    // initial laden
    {
        let scans = scans.clone();
        let drives_state = drives.clone();
        let server_state = server_ok.clone();
        let loading = home_loading.clone();
        let e_scans = err_scans.clone();
        let e_drives = err_drives.clone();
        let e_health = err_health.clone();
        use_effect(move || {
            let loading = loading.clone();
            let mut loading2 = loading.clone();
            loading2.set(true);
            let scans = scans.clone();
            let e_scans = e_scans.clone();
            let drives_state = drives_state.clone();
            let e_drives = e_drives.clone();
            let server_state = server_state.clone();
            let e_health = e_health.clone();
            let loading_done = loading.clone();
            spawn(async move {
                let mut scans = scans.clone();
                let mut e_scans = e_scans.clone();
                let mut drives_state = drives_state.clone();
                let mut e_drives = e_drives.clone();
                let mut server_state = server_state.clone();
                let mut e_health = e_health.clone();
                let mut loading_done = loading_done.clone();
                match api::list_scans().await { Ok(list) => { scans.set(list); e_scans.set(None); }, Err(e) => e_scans.set(Some(e)) }
                match api::list_drives().await { Ok(dr) => { drives_state.set(dr.items); e_drives.set(None); }, Err(e) => e_drives.set(Some(e)) }
                match api::healthz().await { Ok(ok) => { server_state.set(Some(ok)); e_health.set(None); }, Err(e) => e_health.set(Some(e)) }
                loading_done.set(false);
            });
        });
    }

    // (removed recent panel effect)

    let reload = {
        let scans = scans.clone();
        let e_scans = err_scans.clone();
        move |_| {
            let mut scans2 = scans.clone();
            let mut e2 = e_scans.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::list_scans().await { Ok(list) => { scans2.set(list); e2.set(None); }, Err(e) => e2.set(Some(e)) }
            });
        }
    };

    let reload_drives = {
        let drives = drives.clone();
        let server_ok = server_ok.clone();
        let e_drives = err_drives.clone();
        let e_health = err_health.clone();
        move |_| {
            let mut d2 = drives.clone();
            let mut h2 = server_ok.clone();
            let mut ed2 = e_drives.clone();
            let mut eh2 = e_health.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::list_drives().await { Ok(dr) => { d2.set(dr.items); ed2.set(None); }, Err(e) => ed2.set(Some(e)) }
                match api::healthz().await { Ok(ok) => { h2.set(Some(ok)); eh2.set(None); }, Err(e) => eh2.set(Some(e)) }
            });
        }
    };

    let nav = use_navigator();
    let start_scan = {
        let root = new_root.clone();
        move |_| {
            let root_val = root.read().trim().to_string();
            if root_val.is_empty() {
                show_toast("Bitte geben Sie einen Pfad ein");
                return;
            }
            let nav = nav.clone();
            let path = root_val.clone();
            show_toast("Scan wird gestartet...");
            spawn(async move {
                let req = api::CreateScanReq {
                    root_paths: vec![path],
                    follow_symlinks: None,
                    include_hidden: None,
                    measure_logical: None,
                    measure_allocated: None,
                    excludes: None,
                    max_depth: None,
                    concurrency: None,
                };
                match api::create_scan(&req).await {
                    Ok(resp) => {
                        show_toast(&format!("Scan {} gestartet", resp.id));
                        nav.push(Route::Scan { id: resp.id });
                    }
                    Err(e) => {
                        show_toast(&format!("Fehler beim Starten: {}", e));
                    }
                }
            });
        }
    };

    // vorab: Texte für Dashboard
    let server_text = match server_ok.read().to_owned() { Some(true) => "OK", Some(false) => "Fehler", None => "..." };

    rsx! {
        section { class: "panel",
            h2 { "SpeicherWald – Scans" }
            // Dashboard: Server-Status & Laufwerke
            div { class: "toolbar", style: "margin-top:6px;",
                span { "Server: {server_text}" }
                span { "Laufwerke: {drives.read().len()}" }
                { home_loading.read().to_owned().then(|| rsx!(span { class: "spinner", "" })) }
                button { class: "btn", onclick: reload_drives, "Laufwerke aktualisieren" }
            }
            { err_health.read().as_ref().map(|e| rsx!(div { class: "alert alert-error", "Health-Fehler: {e}" })) }
            { err_drives.read().as_ref().map(|e| rsx!(div { class: "alert alert-error", "Laufwerke-Fehler: {e}" })) }
            { err_scans.read().as_ref().map(|e| rsx!(div { class: "alert alert-error", "Scans-Fehler: {e}" })) }
            // Laufwerks-Übersicht
            details { open: true,
                summary { "Laufwerke (Übersicht)" }
                div { style: "display:grid;grid-template-columns:repeat(auto-fill,minmax(320px,1fr));gap:10px;margin-top:8px;",
                    { drives.read().iter().map(|d| {
                        let path = d.path.clone();
                        // FIX Bug #5: Handle invalid disk stats where free > total
                        let used = if d.free_bytes > d.total_bytes {
                            console::warn_1(&format!("Invalid disk stats for {}: free_bytes ({}) > total_bytes ({})", path, d.free_bytes, d.total_bytes).into());
                            0
                        } else {
                            d.total_bytes - d.free_bytes
                        };
                        let percent = if d.total_bytes > 0 { (used as f64) / (d.total_bytes as f64) * 100.0 } else { 0.0 };
                        let bar_width = format!("width:{:.1}%;", percent);
                        rsx!{ div { style: "border:1px solid #222533;background:#0f1117;border-radius:10px;padding:10px;display:flex;flex-direction:column;gap:8px;",
                            div { style: "display:flex;justify-content:space-between;gap:8px;",
                                div { style: "color:#e5e7eb;", strong { "{path}" } }
                                span { style: "color:#9aa0a6;", "{d.drive_type}" }
                            }
                            div { style: "display:flex;gap:10px;align-items:center;",
                                span { style: "min-width:80px;color:#a0aec0;", "{fmt_bytes(used as i64)} / {fmt_bytes(d.total_bytes as i64)}" }
                                div { class: "bar-shell",
                                    div { class: "bar-fill-purple", style: "{bar_width}" }
                                }
                            }
                            div { style: "display:flex;gap:8px;flex-wrap:wrap;",
                                button { class: "btn", onclick: move |_| {
                                    let nav = nav.clone();
                                    let p2 = path.clone();
                                    show_toast(&format!("Starte Scan für {}...", p2));
                                    spawn(async move {
                                        let req = api::CreateScanReq {
                                            root_paths: vec![p2],
                                            follow_symlinks: None,
                                            include_hidden: None,
                                            measure_logical: None,
                                            measure_allocated: None,
                                            excludes: None,
                                            max_depth: None,
                                            concurrency: None,
                                        };
                                        match api::create_scan(&req).await {
                                            Ok(resp) => {
                                                show_toast(&format!("Scan {} gestartet", resp.id));
                                                nav.push(Route::Scan { id: resp.id });
                                            }
                                            Err(e) => {
                                                show_toast(&format!("Fehler beim Starten: {}", e));
                                            }
                                        }
                                    });
                                }, "Scan starten" }
                            }
                        } }
                    }) }
                }
            }
            div { class: "input-group",
                input { class: "form-control", value: "{new_root}", placeholder: "Root-Pfad (z. B. C:\\ oder \\\\server\\share)",
                    oninput: move |e: Event<FormData>| { let mut new_root2 = new_root.clone(); new_root2.set(e.value().clone()); } }
                div { class: "input-group-append",
                    button { class: "btn btn-primary", onclick: start_scan, "Scan starten" }
                    button { class: "btn", onclick: reload, "Aktualisieren" }
                }
            }
            ul { class: "list-unstyled",
                { (scans.read().is_empty() && !home_loading.read().to_owned()).then(|| rsx!(li { class: "text-muted", "Noch keine Scans." })) }
                { scans.read().iter().map(|s| {
                    let id = s.id.clone();
                    rsx!{ li { style: "margin:6px 0;",
                        Link { to: Route::Scan { id: id.clone() },
                            "{id} – {s.status} – Ordner {s.dir_count} – Dateien {s.file_count} – Allokiert {fmt_bytes(s.total_allocated_size)}" }
                    } }
                }) }
            }
        }
    }
}
// ----- Scan-Detailseite mit Live-Log & Tabellen -----
#[component]
fn Scan(id: String) -> Element {
    // KPI/Meta und Log
    let kpi = use_signal(|| None as Option<types::ScanSummary>);
    let log = use_signal(|| String::new());

    // EventSource-Handle, damit die Verbindung lebt
    let es_ref = use_signal(|| None as Option<web_sys::EventSource>);

    // Tabellenzustände
    let tree_items = use_signal(|| Vec::<types::NodeDto>::new());
    let top_items = use_signal(|| Vec::<types::TopItem>::new());
    let list_items = use_signal(|| Vec::<types::ListItem>::new());
    let err_tree = use_signal(|| None as Option<String>);
    let err_top = use_signal(|| None as Option<String>);
    let err_list = use_signal(|| None as Option<String>);
    let loading_tree = use_signal(|| false);
    let loading_list = use_signal(|| false);

    // Steuerung für Baum/Top
    let tree_path = use_signal(|| None as Option<String>);
    let tree_depth = use_signal(|| 3_i64);
    let tree_limit = use_signal(|| 200_i64);
    let tree_sort = use_signal(|| "size".to_string()); // server hint: "size" | "name"
    // Client-side sort controls for Tree table
    let tree_sort_view = use_signal(|| "allocated".to_string()); // allocated|logical|name|type|modified
    let tree_order = use_signal(|| "desc".to_string());
    let top_scope = use_signal(|| "dirs".to_string()); // "dirs" | "files"
    let top_show = use_signal(|| 15_usize);
    // Client-side sort controls for Top table
    let top_sort = use_signal(|| "allocated".to_string()); // allocated|logical|name|type|modified
    let top_order = use_signal(|| "desc".to_string());
    // Explorer (Liste) Steuerung
    let list_path = use_signal(|| None as Option<String>);
    let list_sort = use_signal(|| "allocated".to_string());
    let list_order = use_signal(|| "desc".to_string());
    // Default page size reduced for better paging experience
    let list_limit = use_signal(|| 50_i64);
    let list_offset = use_signal(|| 0_i64);
    // Pagination helper: track if another next page likely exists (based on last page size)
    let list_has_more = use_signal(|| true);
    // Sequence ID to drop stale responses when multiple requests overlap
    let list_req_id = use_signal(|| 0_i64);
    // Move dialog & drive targets
    let move_dialog = use_signal(|| None as Option<MoveDialogState>);
    let drive_targets = use_signal(|| Vec::<types::DriveInfo>::new());
    let drive_fetch_error = use_signal(|| None as Option<String>);

    // Filter und Suche
    let search_query = use_signal(|| String::new());
    let min_size_filter = use_signal(|| 0_i64);
    let min_size_unit = use_signal(|| "b".to_string());
    let file_type_filter = use_signal(|| "all".to_string());
    let show_hidden = use_signal(|| false);

    // Navigation History für Breadcrumbs
    let nav_history = use_signal(|| Vec::<String>::new());

    // Ensure pagination starts from 0 whenever the path changes
    {
        let list_offset0 = list_offset.clone();
        let list_path0 = list_path.clone();
        let nav_hist0 = nav_history.clone();
        use_effect(move || {
            let mut list_offset0 = list_offset0.clone();
            list_offset0.set(0);
            match &list_path0.read().clone() {
                Some(p) => {
                    let mut hist = nav_hist0.read().clone();
                    if hist.last().map(|s| s.as_str()) != Some(p.as_str()) {
                        hist.push(p.clone());
                        let mut nav_hist0 = nav_hist0.clone();
                        nav_hist0.set(hist);
                    }
                }
                None => {
                    let mut nav_hist0 = nav_hist0.clone();
                    nav_hist0.set(Vec::new());
                }
            }
        });
    }

    // Export-Steuerung
    let export_scope = use_signal(|| "all".to_string()); // all|nodes|files
    let export_limit = use_signal(|| 10000_i64);

    // Live-Update & Throttle
    let live_update = use_signal(|| true);
    let last_refresh = use_signal(|| 0.0_f64);

    // KPI initial laden
    {
        let id_state = id.clone();
        let kpi_state = kpi.clone();
        use_effect(move || {
            let id = id_state.clone();
            let kpi = kpi_state.clone();
            spawn(async move {
                let mut kpi = kpi.clone();
                if let Ok(summary) = api::get_scan(&id).await {
                    kpi.set(Some(summary));
                }
            });
        });
    }

    // Laufwerksliste einmalig laden (für Move-Dialog)
    {
        let drive_targets_state = drive_targets.clone();
        let drive_err_state = drive_fetch_error.clone();
        use_effect(move || {
            let mut drive_targets = drive_targets_state.clone();
            let mut drive_err = drive_err_state.clone();
            spawn(async move {
                match api::list_drives().await {
                    Ok(dr) => {
                        drive_targets.set(dr.items);
                        drive_err.set(None);
                    }
                    Err(e) => {
                        drive_err.set(Some(e));
                    }
                }
            });
        });
    }

    // Erste Ladung für Tree/Top
    {
        let id_state = id.clone();
        let tree_items_state = tree_items.clone();
        let tree_path_state = tree_path.clone();
        let tree_depth_state = tree_depth.clone();
        let tree_limit_state = tree_limit.clone();
        let tree_sort_state = tree_sort.clone();
        let top_items_state = top_items.clone();
        let top_scope_state = top_scope.clone();
        let err_tree_state = err_tree.clone();
        let err_top_state = err_top.clone();
        let loading_tree_state = loading_tree.clone();
        use_effect(move || {
            let id = id_state.clone();
            let tree_items = tree_items_state.clone();
            let tree_path = tree_path_state.read().clone();
            let tree_depth = *tree_depth_state.read();
            let tree_limit = *tree_limit_state.read();
            let tree_sort = tree_sort_state.read().clone();
            let top_items = top_items_state.clone();
            let top_scope = top_scope_state.read().clone();
            let err_tree = err_tree_state.clone();
            let err_top = err_top_state.clone();
            let mut loading_tree = loading_tree_state.clone();

            *loading_tree.write() = true;

            spawn(async move {
                let mut tree_items = tree_items.clone();
                let mut err_tree = err_tree.clone();
                let mut loading_tree = loading_tree.clone();
                let mut top_items = top_items.clone();
                let mut err_top = err_top.clone();
                let tq = api::TreeQuery {
                    path: tree_path,
                    depth: Some(tree_depth),
                    sort: Some(tree_sort.clone()),
                    limit: Some(tree_limit),
                };

                match api::get_tree(&id, &tq).await {
                    Ok(list) => {
                        *tree_items.write() = list;
                        *err_tree.write() = None;
                    }
                    Err(e) => {
                        *err_tree.write() = Some(e);
                    }
                }

                *loading_tree.write() = false;

                let qq = api::TopQuery {
                    scope: Some(top_scope),
                    limit: Some(100),
                };

                match api::get_top(&id, &qq).await {
                    Ok(list) => {
                        *top_items.write() = list;
                        *err_top.write() = None;
                    }
                    Err(e) => {
                        *err_top.write() = Some(e);
                    }
                }
            });
        });
    }

    // Initial-Ladung für Explorer (Liste)
    {
        let id_state = id.clone();
        let list_items_state = list_items.clone();
        let list_path_state = list_path.clone();
        let list_sort_state = list_sort.clone();
        let list_order_state = list_order.clone();
        let list_limit_state = list_limit.clone();
        let list_offset_state = list_offset.clone();
        let list_has_more_state = list_has_more.clone();
        let err_list_state = err_list.clone();
        let loading_list_state = loading_list.clone();

        use_effect(move || {
            let id = id_state.clone();
            let list_items = list_items_state.clone();
            let list_has_more = list_has_more_state.clone();
            let err_list = err_list_state.clone();
            let mut loading_list = loading_list_state.clone();
            let path = list_path_state.read().clone();
            let sort = list_sort_state.read().clone();
            let order = list_order_state.read().clone();
            let limit = *list_limit_state.read();
            let offset = *list_offset_state.read();

            *loading_list.write() = true;

            spawn(async move {
                let mut list_items = list_items.clone();
                let mut list_has_more = list_has_more.clone();
                let mut err_list = err_list.clone();
                let mut loading_list = loading_list.clone();
                let lq = api::ListQuery {
                    path,
                    sort: Some(sort),
                    order: Some(order),
                    limit: Some(limit + 1),
                    offset: Some(offset),
                };

                match api::get_list(&id, &lq).await {
                    Ok(list) => {
                        let has_more = (list.len() as i64) > limit;
                        let items_page: Vec<types::ListItem> =
                            list.into_iter().take(limit as usize).collect();
                        *list_has_more.write() = has_more;
                        *list_items.write() = items_page;
                        *err_list.write() = None;
                    }
                    Err(e) => {
                        *list_has_more.write() = false;
                        *list_items.write() = Vec::new();
                        *err_list.write() = Some(e);
                    }
                }
                *loading_list.write() = false;
            });
        });
    }

    // Auto-Reload Explorer (Liste), sobald relevante Zustände geändert werden
    // Fix für den "2x klicken"-Effekt: Wir laden nun automatisch, nachdem z. B. list_path gesetzt wurde.
    {
        let id_state = id.clone();
        let list_items_state = list_items.clone();
        let list_path_state = list_path.clone();
        let list_sort_state = list_sort.clone();
        let list_order_state = list_order.clone();
        let list_limit_state = list_limit.clone();
        let list_offset_state = list_offset.clone();
        let err_list_state = err_list.clone();
        let loading_list_state = loading_list.clone();
        let list_has_more_state = list_has_more.clone();
        let req_ref_state = list_req_id.clone();

        use_effect(move || {
            let id = id_state.clone();
            let list_items = list_items_state.clone();
            let list_path_val = list_path_state.read().clone();
            let list_sort_val = list_sort_state.read().clone();
            let list_order_val = list_order_state.read().clone();
            let list_limit_val = *list_limit_state.read();
            let list_offset_val = *list_offset_state.read();
            let err_list = err_list_state.clone();
            let loading_list = loading_list_state.clone();
            let list_has_more = list_has_more_state.clone();
            let mut req_ref = req_ref_state.clone();
            let list_offset_handle = list_offset_state.clone();

            let my_id = {
                let mut rid = req_ref.write();
                *rid += 1;
                *rid
            };

            let mut loading_list = loading_list.clone();
            *loading_list.write() = true;

            spawn(async move {
                let mut list_items = list_items.clone();
                let mut list_has_more = list_has_more.clone();
                let mut err_list = err_list.clone();
                let mut loading_list = loading_list.clone();
                let mut list_offset_handle = list_offset_handle.clone();
                let lq = api::ListQuery {
                    path: list_path_val,
                    sort: Some(list_sort_val),
                    order: Some(list_order_val),
                    limit: Some(list_limit_val + 1),
                    offset: Some(list_offset_val),
                };

                match api::get_list(&id, &lq).await {
                    Ok(list) => {
                        let has_more = (list.len() as i64) > list_limit_val;
                        let items_page: Vec<types::ListItem> =
                            list.into_iter().take(list_limit_val as usize).collect();
                        let is_latest = *req_ref.read() == my_id;
                        if is_latest {
                            let current_offset = *list_offset_handle.read();
                            if items_page.is_empty() && current_offset > 0 {
                                show_toast("Keine weitere Seite");
                                let back_off = (current_offset - list_limit_val).max(0);
                                *list_offset_handle.write() = back_off;
                            } else {
                                *list_has_more.write() = has_more;
                                *list_items.write() = items_page;
                                *err_list.write() = None;
                                *loading_list.write() = false;
                            }
                        }
                    }
                    Err(e) => {
                        let is_latest = *req_ref.read() == my_id;
                        if is_latest {
                            *err_list.write() = Some(e);
                            *loading_list.write() = false;
                        }
                    }
                }
            });
        });
    }

    // Loader: Baum/Top
    let do_load_tree = {
        let id_val = id.clone();
        let tree_items_state = tree_items.clone();
        let tree_path_state = tree_path.clone();
        let tree_depth_state = tree_depth.clone();
        let tree_limit_state = tree_limit.clone();
        let tree_sort_state = tree_sort.clone();
        let e_tree = err_tree.clone();
        let l_tree = loading_tree.clone();
        Rc::new(move || {
            let id_c = id_val.clone();
            let tree_items2 = tree_items_state.clone();
            let q_path = tree_path_state.read().clone();
            let q_depth = *tree_depth_state.read();
            let q_limit = *tree_limit_state.read();
            let q_sort = tree_sort_state.read().clone();
            let e2 = e_tree.clone();
            let mut l2 = l_tree.clone();
            l2.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                let mut tree_items2 = tree_items2.clone();
                let mut e2 = e2.clone();
                let mut l2 = l2.clone();
                let q = api::TreeQuery { path: q_path, depth: Some(q_depth), sort: Some(q_sort), limit: Some(q_limit) };
                match api::get_tree(&id_c, &q).await { Ok(list) => { tree_items2.set(list); e2.set(None); }, Err(e) => e2.set(Some(e)) }
                l2.set(false);
            });
        })
    };

    // Hinweis: bisher keine separate "Top laden"-Aktion nötig – Top wird initial und per SSE-Refresh geladen

    // Loader: Explorer (Liste)
    let do_load_list: Rc<dyn Fn()> = {
        let id_val = id.clone();
        let list_items_state = list_items.clone();
        let list_path_state = list_path.clone();
        let list_sort_state = list_sort.clone();
        let list_order_state = list_order.clone();
        let list_limit_state = list_limit.clone();
        let list_offset_state = list_offset.clone();
        let e_list = err_list.clone();
        let l_list = loading_list.clone();
        let list_has_more_state = list_has_more.clone();
        let req_ref = list_req_id.clone();
        Rc::new(move || {
            let id_c = id_val.clone();
            let list_items2 = list_items_state.clone();
            let q_path = list_path_state.read().clone();
            let q_sort = list_sort_state.read().clone();
            let q_order = list_order_state.read().clone();
            let q_limit = *list_limit_state.read();
            let q_offset = *list_offset_state.read();
            let e2 = e_list.clone();
            let mut l2 = l_list.clone();
            // Start a new request and track sequence id
            let my_id = if let Ok(mut rid) = req_ref.try_write_unchecked() {
                *rid += 1;
                *rid
            } else {
                0
            };
            l2.set(true);
            // Clone state handles for use inside async block to avoid moving from the outer closure (keeps Fn instead of FnOnce)
            let has_more2 = list_has_more_state.clone();
            // Clone the request ref handle for use inside the async block (avoid moving the captured variable)
            let req_ref_async = req_ref.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let mut list_items2 = list_items2.clone();
                let mut has_more2 = has_more2.clone();
                let mut e2 = e2.clone();
                let mut l2 = l2.clone();
                let q = api::ListQuery { path: q_path, sort: Some(q_sort), order: Some(q_order), limit: Some(q_limit + 1), offset: Some(q_offset) };
                match api::get_list(&id_c, &q).await {
                    Ok(list) => {
                        let has_more = (list.len() as i64) > q_limit;
                        let items_page: Vec<types::ListItem> = list.into_iter().take(q_limit as usize).collect();
                        let is_latest = req_ref_async.with(|rid| my_id == *rid);
                        if is_latest {
                            has_more2.set(has_more);
                            list_items2.set(items_page);
                            e2.set(None);
                            l2.set(false);
                        }
                    },
                    Err(e) => {
                        let is_latest = req_ref_async.with(|rid| my_id == *rid);
                        if is_latest { e2.set(Some(e)); l2.set(false); }
                    }
                }
            });
        })
    };

    // SSE attach + gedrosselte Live-Refreshes
    {
        let id_for_sse = id.clone();
        let id_for_cb = id.clone();
        let kpi = kpi.clone();
        let log_state = log.clone();
        let es_ref_state = es_ref.clone();
        let tree_items_h = tree_items.clone();
        let tree_path_h = tree_path.clone();
        let tree_depth_h = tree_depth.clone();
        let tree_limit_h = tree_limit.clone();
        let tree_sort_h = tree_sort.clone();
        let top_items_h = top_items.clone();
        let top_scope_h = top_scope.clone();
        let list_items_h = list_items.clone();
        let list_path_h = list_path.clone();
        let list_sort_h = list_sort.clone();
        let list_order_h = list_order.clone();
        let list_limit_h = list_limit.clone();
        let list_offset_h = list_offset.clone();
        let list_has_more_h = list_has_more.clone();
        let loading_list_h = loading_list.clone();
        let nav_hist_h = nav_history.clone();
        let live_h = live_update.clone();
        let mut last_h = last_refresh.clone();

        use_effect(move || {
            let mut log_state_in = log_state.clone();
            let log_state_err = log_state.clone();
            let es_holder = es_ref_state.clone();
            let id_for_cb = id_for_cb.clone();

            let result = api::sse_attach(&id_for_sse, move |ev| {
                let id_for_cb = id_for_cb.clone();
                
                let mut newlog = log_state_in.read().clone();
                // FIX Bug #3: Limit log size to prevent unbounded growth (max 50KB)
                const MAX_LOG_SIZE: usize = 50_000;
                if newlog.len() > MAX_LOG_SIZE {
                    // Keep only the last 80% when limit is reached
                    let keep_size = (MAX_LOG_SIZE * 4) / 5;
                    newlog = newlog.chars().rev().take(keep_size).collect::<String>().chars().rev().collect();
                    newlog.insert_str(0, "[...log truncated...]\n");
                }
                
                match &ev {
                    types::ScanEvent::Started { root_paths } => newlog.push_str(&format!("Started: {}\n", root_paths.join(", "))),
                    types::ScanEvent::Progress { current_path, dirs_scanned, files_scanned, allocated_size, .. } => newlog.push_str(&format!("Progress: {} | dirs={} files={} alloc={}\n", current_path, dirs_scanned, files_scanned, fmt_bytes(*allocated_size as i64))),
                    types::ScanEvent::Warning { path, code, message } => newlog.push_str(&format!("Warning: {} ({}) : {}\n", path, code, message)),
                    types::ScanEvent::Done { .. } => newlog.push_str("Done\n"),
                    types::ScanEvent::Cancelled => newlog.push_str("Cancelled\n"),
                    types::ScanEvent::Failed { message } => newlog.push_str(&format!("Failed: {}\n", message)),
                }
                // FIX Bug #2: Remove redundant clone
                log_state_in.set(newlog);

                if let types::ScanEvent::Done { .. } = ev {
                    if list_path_h.read().is_none() {
                        let id_aut = id_for_cb.clone();
                        let lp_state = list_path_h.clone();
                        let list_items2 = list_items_h.clone();
                        let sort_state = list_sort_h.clone();
                        let order_state = list_order_h.clone();
                        let limit_state = list_limit_h.clone();
                        let nav_hist_state = nav_hist_h.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let mut list_items2 = list_items2.clone();
                            let mut lp_state = lp_state.clone();
                            let mut nav_hist_state = nav_hist_state.clone();
                            let q_roots = api::ListQuery {
                                path: None,
                                sort: Some(sort_state.read().clone()),
                                order: Some(order_state.read().clone()),
                                limit: Some(*limit_state.read()),
                                offset: Some(0),
                            };
                            if let Ok(list) = api::get_list(&id_aut, &q_roots).await {
                                list_items2.set(list.clone());
                                if lp_state.read().is_none() {
                                    if let Some(first_root) = list.iter().find_map(|it| match it {
                                        types::ListItem::Dir { path, .. } => Some(path.clone()),
                                        _ => None,
                                    }) {
                                        lp_state.set(Some(first_root.clone()));
                                        nav_hist_state.set(vec![first_root.clone()]);
                                        let id_list2 = id_aut.clone();
                                        let list_items3 = list_items2.clone();
                                        wasm_bindgen_futures::spawn_local(async move {
                                            let mut list_items3 = list_items3.clone();
                                            let q_child = api::ListQuery {
                                                path: Some(first_root),
                                                sort: Some("allocated".into()),
                                                order: Some("desc".into()),
                                                limit: Some(500),
                                                offset: Some(0),
                                            };
                                            if let Ok(list2) = api::get_list(&id_list2, &q_child).await {
                                                list_items3.set(list2);
                                            }
                                        });
                                    }
                                }
                            }
                        });
                    }
                }

                let id2 = id_for_cb.clone();
                let kpi2 = kpi.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let mut kpi2 = kpi2.clone();
                    if let Ok(s) = api::get_scan(&id2).await {
                        kpi2.set(Some(s));
                    }
                });

                if *live_h.read() {
                    let now = Date::now();
                    let mut should = false;
                    // FIX Bug #21: Use safe write instead of unsafe try_write_unchecked
                    {
                        let mut last = last_h.write();
                        if now - *last > 5000.0 {
                            *last = now;
                            should = true;
                        }
                    }
                    if should {
                        let id_top = id_for_cb.clone();
                        let top_items2 = top_items_h.clone();
                        let scope = top_scope_h.read().clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let mut top_items2 = top_items2.clone();
                            let q = api::TopQuery { scope: Some(scope), limit: Some(100) };
                            if let Ok(list) = api::get_top(&id_top, &q).await {
                                top_items2.set(list);
                            }
                        });

                        let id_tree = id_for_cb.clone();
                        let tree_items2 = tree_items_h.clone();
                        let q_path = tree_path_h.read().clone();
                        let q_depth = *tree_depth_h.read();
                        let q_limit = *tree_limit_h.read();
                        let q_sort = tree_sort_h.read().clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let mut tree_items2 = tree_items2.clone();
                            let q = api::TreeQuery {
                                path: q_path,
                                depth: Some(q_depth),
                                sort: Some(q_sort),
                                limit: Some(q_limit),
                            };
                            if let Ok(list) = api::get_tree(&id_tree, &q).await {
                                tree_items2.set(list);
                            }
                        });

                        if !*loading_list_h.read() {
                            let id_list = id_for_cb.clone();
                            let list_items2 = list_items_h.clone();
                            let has_more2 = list_has_more_h.clone();
                            let q_path_l = list_path_h.read().clone();
                            let q_sort_l = list_sort_h.read().clone();
                            let q_order_l = list_order_h.read().clone();
                            let q_limit_l = *list_limit_h.read();
                            let q_offset_l = *list_offset_h.read();
                            wasm_bindgen_futures::spawn_local(async move {
                                let mut has_more2 = has_more2.clone();
                                let mut list_items2 = list_items2.clone();
                                let q = api::ListQuery {
                                    path: q_path_l,
                                    sort: Some(q_sort_l),
                                    order: Some(q_order_l),
                                    limit: Some(q_limit_l + 1),
                                    offset: Some(q_offset_l),
                                };
                                if let Ok(list) = api::get_list(&id_list, &q).await {
                                    let has_more = (list.len() as i64) > q_limit_l;
                                    let items_page: Vec<types::ListItem> =
                                        list.into_iter().take(q_limit_l as usize).collect();
                                    has_more2.set(has_more);
                                    list_items2.set(items_page);
                                }
                            });
                        }
                    }
                }
            });

            match result {
                Ok(es) => {
                    if let Ok(mut slot) = es_holder.try_write_unchecked() {
                        *slot = Some(es);
                    }
                }
                Err(e) => {
                    let mut newlog = log_state_err.read().clone();
                    newlog.push_str(&format!("SSE Fehler: {}\n", e));
                    let mut log_state_err = log_state_err.clone();
                    log_state_err.set(newlog);
                }
            }

            // Cleanup when effect runs again or component unmounts
            // (Dioxus 0.6 use_effect doesn't return cleanup in the same way)
        });
    }

    // Cancel/Purge
    let nav = use_navigator();
    let cancel = {
        let id_val = id.clone();
        move |_| {
            let id2 = id_val.clone();
            wasm_bindgen_futures::spawn_local(async move { let _ = api::cancel_scan(&id2, false).await; });
        }
    };
    let purge = {
        let id_val = id.clone();
        let nav = nav.clone();
        move |_| {
            let id2 = id_val.clone();
            let nav2 = nav.clone();
            wasm_bindgen_futures::spawn_local(async move { let _ = api::cancel_scan(&id2, true).await; });
            nav2.push(Route::Home {});
        }
    };

    // UI
    let do_load_tree_btn = do_load_tree.clone();
    let do_load_list_btn = do_load_list.clone();
    // Top-N Sichtbarkeit
    let top_more = {
        let top_show = top_show.clone();
        move |_| { let n = *top_show.read(); let m = (n + 10).min(100); let mut top_show = top_show.clone(); top_show.set(m); }
    };
    let top_less = {
        let top_show = top_show.clone();
        move |_| { let n = *top_show.read(); let m = if n > 10 { n - 10 } else { 5 }; let mut top_show = top_show.clone(); top_show.set(m); }
    };
    // Tree Komfort-Buttons
    let more_tree = {
        let tree_limit = tree_limit.clone();
        let do_btn = do_load_tree.clone();
        move |_| { let current_limit = *tree_limit.read(); let mut tree_limit = tree_limit.clone(); tree_limit.set(current_limit + 200); (do_btn.as_ref())(); }
    };
    let less_tree = {
        let tree_limit = tree_limit.clone();
        let do_btn = do_load_tree.clone();
        move |_| { let current_limit = *tree_limit.read(); let v = (current_limit - 200).max(10); let mut tree_limit = tree_limit.clone(); tree_limit.set(v); (do_btn.as_ref())(); }
    };
    // Explorer Paginierung
    let next_page = {
        let list_offset = list_offset.clone();
        let list_limit = list_limit.clone();
        let list_path_dbg = list_path.clone();
        let do_btn = do_load_list_btn.clone();
        move |_| {
            // Always advance; the loader will clamp behavior by returning fewer items
            // and updating `list_has_more` accordingly.
            let current_offset = *list_offset.read();
            let current_limit = *list_limit.read();
            let new_off = current_offset + current_limit;
            console::log_1(&format!("Next click: offset {} -> {} (limit {}), path={:?}", current_offset, new_off, current_limit, list_path_dbg.read().clone()).into());
            let mut list_offset = list_offset.clone();
            list_offset.set(new_off);
            // Trigger immediate reload for snappier UX
            (do_btn.as_ref())();
        }
    };
    let max_alloc_bar: i64 = top_items
        .read()
        .iter()
        .map(|it| match it {
            types::TopItem::Dir { allocated_size, .. } => allocated_size,
            types::TopItem::File { allocated_size, .. } => allocated_size,
        })
        .max()
        .copied()
        .unwrap_or(0);
    let max_alloc_list: i64 = list_items
        .read()
        .iter()
        .map(|it| match it {
            types::ListItem::Dir { allocated_size, .. } => allocated_size,
            types::ListItem::File { allocated_size, .. } => allocated_size,
        })
        .max()
        .copied()
        .unwrap_or(0);
    let max_alloc_tree: i64 = tree_items
        .read()
        .iter()
        .map(|n| n.allocated_size)
        .max()
        .unwrap_or(0);
    rsx! {
        section { class: "panel",
            h2 { "Scan {id}" }
            div { style: "color:#a0aec0;margin:4px 0 8px 0;", "Status: {kpi.read().as_ref().map(|s| s.status.clone()).unwrap_or_else(|| \"...\".into())}" }
            div { style: "display:flex;gap:12px;flex-wrap:wrap;",
                button { class: "btn", onclick: cancel, "Abbrechen" }
                button { class: "btn btn-danger", onclick: purge, "Purge" }
            }
            // Export-Bereich
            details { open: true,
                summary { "Export" }
                div { style: "display:flex;gap:10px;align-items:center;flex-wrap:wrap;margin:8px 0;",
                    span { "Scope:" }
                    select { value: "{export_scope}", oninput: move |e| { let mut export_scope = export_scope.clone(); export_scope.set(e.value()); },
                        option { value: "all", "All" }
                        option { value: "nodes", "Nodes" }
                        option { value: "files", "Files" }
                    }
                    span { "Limit:" }
                    input { r#type: "number", min: "1", value: "{export_limit}", oninput: move |e| {
                        let value = e.value();
                        let mut export_limit = export_limit.clone();
                        if let Ok(v) = value.parse::<i64>() { export_limit.set(v.max(1)); }
                    } }
                    // Download Buttons
                    button { class: "btn", onclick: {
                        let id_csv = id.clone();
                        let scope = export_scope.clone();
                        let limit = export_limit.clone();
                        move |_| {
                            let url = format!("/scans/{}/export?format=csv&scope={}&limit={}", id_csv, scope.read().clone(), *limit.read());
                            trigger_download(&url, Some(&format!("scan_{}.csv", id_csv)));
                        }
                    }, "CSV" }
                    button { class: "btn", onclick: {
                        let id_json = id.clone();
                        let scope = export_scope.clone();
                        let limit = export_limit.clone();
                        move |_| {
                            let url = format!("/scans/{}/export?format=json&scope={}&limit={}", id_json, scope.read().clone(), *limit.read());
                            trigger_download(&url, Some(&format!("scan_{}.json", id_json)));
                        }
                    }, "JSON" }
                    button { class: "btn", onclick: {
                        let id_stats = id.clone();
                        move |_| {
                            let url = format!("/scans/{}/statistics", id_stats);
                            trigger_download(&url, Some(&format!("scan_{}_stats.json", id_stats)));
                        }
                    }, "Statistics" }
                }
            }
             details { open: true,
                 summary { "Live-Fortschritt" }
                 pre { style: "background:#0b0c0f;border:1px solid #222533;border-radius:8px;padding:10px;max-height:240px;overflow:auto;white-space:pre-wrap;", "{log}" }
             }
            // Breadcrumbs Navigation
            { (!nav_history.read().is_empty()).then(|| rsx!{
                div { class: "breadcrumbs",
                    span { class: "text-muted", "Navigationspfad:" }
                    { nav_history.read().iter().enumerate().map(|(i, path)| {
                        let p = path.clone();
                        let nav_hist = nav_history.clone();
                        let tree_path_nav = tree_path.clone();
                        let list_path_nav = list_path.clone();
                        let do_nav = do_load_tree.clone();
                        rsx!{
                            span { style: "display:flex;gap:4px;align-items:center;",
                                { (i > 0).then(|| rsx!(span { class: "sep", "›" })) }
                                button {
                                    onclick: move |_| {
                                        let new_path = if i == 0 { None } else { Some(p.clone()) };
                                        let mut tree_path_nav = tree_path_nav.clone();
                                        let mut list_path_nav = list_path_nav.clone();
                                        let hist_slice = nav_hist.read()[..=i].to_vec();
                                        let mut nav_hist = nav_hist.clone();
                                        tree_path_nav.set(new_path.clone());
                                        list_path_nav.set(new_path.clone());
                                        nav_hist.set(hist_slice);
                                        (do_nav.as_ref())();
                                    },
                                    "{path}"
                                }
                            }
                        }
                    }) }
                    button {
                        onclick: move |_| {
                            let mut nav_history = nav_history.clone();
                            let mut tree_path = tree_path.clone();
                            let mut list_path = list_path.clone();
                            nav_history.set(Vec::new());
                            tree_path.set(None);
                            list_path.set(None);
                            (do_load_tree.as_ref())();
                        },
                        "Zurücksetzen"
                    }
                }
            }) }
            
            div { style: "margin-top:12px;display:flex;gap:12px;align-items:center;flex-wrap:wrap;",
                button { class: "btn", onclick: move |_| (do_load_tree_btn.as_ref())(), "Baum laden" }
                span { "Pfad:" }
                input { value: "{tree_path.read().as_ref().cloned().unwrap_or_default()}", placeholder: "leer = alle Wurzeln",
                    oninput: move |e| {
                        let value = e.value();
                        let mut tree_path = tree_path.clone();
                        tree_path.set(if value.is_empty() { None } else { Some(value) });
                    }
                }
                span { "Tiefe:" }
                input { r#type: "number", min: "1", value: "{tree_depth}", oninput: move |e| {
                        let value = e.value();
                        let mut tree_depth = tree_depth.clone();
                        if let Ok(v) = value.parse::<i64>() { tree_depth.set(v.max(1)); }
                    }
                }
                span { "Sort:" }
                select { value: "{tree_sort}", oninput: move |e| { let mut tree_sort = tree_sort.clone(); tree_sort.set(e.value()); },
                    option { value: "size", "Größe" }
                    option { value: "name", "Name" }
                }
                span { "Limit:" }
                input { r#type: "number", min: "10", value: "{tree_limit}", oninput: move |e| {
                        let value = e.value();
                        let mut tree_limit = tree_limit.clone();
                        if let Ok(v) = value.parse::<i64>() { tree_limit.set(v.max(10)); }
                    }
                }
                button { class: "btn", onclick: more_tree, "Mehr" }
                button { class: "btn", onclick: less_tree, "Weniger" }
                label { style: "display:flex;gap:6px;align-items:center;", input { r#type: "checkbox", checked: *live_update.read(), oninput: move |_| { let current = *live_update.read(); let mut live_update = live_update.clone(); live_update.set(!current); } } " Live-Update Tabellen" }
                span { "Einträge: {tree_items.len()}" }
                { (*loading_tree.read()).then(|| rsx!(span { class: "spinner", "" })) }
                { err_tree.read().as_ref().map(|e| rsx!(span { class: "text-danger", " Fehler: {e}" })) }
            }
            // Top-N Bereich + Visuelle Übersicht
            div { style: "margin-top:8px;display:flex;gap:12px;align-items:center;flex-wrap:wrap;",
                span { "Top-N:" }
                select { value: "{top_scope}", oninput: {
                        let id = id.clone();
                        let top_scope = top_scope.clone();
                        let top_show = top_show.clone();
                        let top_items = top_items.clone();
                        move |e: Event<FormData>| {
                        let value = e.value();
                        let mut top_scope = top_scope.clone();
                        let mut top_show2 = top_show.clone();
                        top_scope.set(value.clone());
                        top_show2.set(15);
                        let top_items2 = top_items.clone();
                        let id_top = id.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let mut top_items2 = top_items2.clone();
                            let q = api::TopQuery { scope: Some(value), limit: Some(100) };
                            if let Ok(list) = api::get_top(&id_top, &q).await { top_items2.set(list); }
                        });
                    }
                    },
                    option { value: "dirs", "Ordner" }
                    option { value: "files", "Dateien" }
                }
                button { style: btn_style(), onclick: top_less, "Weniger" }
                button { style: btn_style(), onclick: top_more, "Mehr" }
                button { class: "btn", onclick: {
                        let top_items = top_items.clone();
                        let top_show = top_show.clone();
                        let id_csv = id.clone();
                        move |_| {
                            // FIX Bug #37: Note that this builds CSV in memory
                            // For very large exports, consider using server-side streaming
                            let mut csv = String::from("type,path,allocated,logical,depth,file_count,dir_count\n");
                            let show_count = *top_show.read();
                            for it in top_items.read().iter().take(show_count) {
                                match it {
                                    types::TopItem::Dir { path, allocated_size, logical_size, depth, file_count, dir_count, .. } => {
                                        csv.push_str(&format!("dir,\"{}\",{},{},{},{},{}\n", path.replace('"', ""), allocated_size, logical_size, depth, file_count, dir_count));
                                    }
                                    types::TopItem::File { path, allocated_size, logical_size, .. } => {
                                        csv.push_str(&format!("file,\"{}\",{},{},,,\n", path.replace('"', ""), allocated_size, logical_size));
                                    }
                                }
                            }
                            let fname = format!("speicherwald_top_{}.csv", id_csv);
                            download_csv(&fname, &csv);
                        }
                    }, "CSV export" }
                { err_top.read().as_ref().map(|e| rsx!(span { style: "color:#f87171;", " Fehler: {e}" })) }
            }
            // Visuelle Übersicht (Top-N Balken)
            div { style: "margin-top:8px;",
                { let show_count = *top_show.read(); top_items.read().iter().take(show_count).map(|it| {
                    let nav_h = nav_history.clone();
                    let (label, alloc) = match it {
                        types::TopItem::Dir { path, allocated_size, .. } => (path.clone(), *allocated_size),
                        types::TopItem::File { path, allocated_size, .. } => (path.clone(), *allocated_size),
                    };
                    let mut blocks: usize = 1;
                    // FIX Bug #16: Add bounds check for float to usize cast
                    if max_alloc_bar > 0 { 
                        let calc = ((alloc as f64) / (max_alloc_bar as f64) * 40.0).round();
                        blocks = if calc >= 0.0 && calc <= 1000.0 {
                            (calc as usize).max(1).min(1000)
                        } else {
                            1
                        };
                    }
                    let bar = "█".repeat(blocks);
                    rsx!{ div { style: "display:flex;gap:10px;align-items:center;font-family:monospace;",
                        span { style: "min-width:80px;color:#a0aec0;", "{fmt_bytes(alloc)}" }
                        pre { style: "margin:0;line-height:1.1;color:#60a5fa;cursor:pointer;", onclick: move |_| { 
                            let mut list_path = list_path.clone();
                            list_path.set(Some(label.clone())); 
                            let mut hist = nav_h.read().clone();
                            if !hist.contains(&label) { hist.push(label.clone()); }
                            let mut nav_h = nav_h.clone();
                            nav_h.set(hist);
                        }, "{bar}" }
                        span { style: "color:#93c5fd;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:480px;", "{label}" }
                    } }
                }) }
            }
            // Top-Tabelle
            table { class: "responsive-table",
                thead { tr {
                    th { style: "text-align:left;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "type".to_string();
                        let current_sort = top_sort.read().clone();
                        let current_order = top_order.read().clone();
                        let mut top_sort = top_sort.clone();
                        let mut top_order = top_order.clone();
                        if current_sort == key { top_order.set(if current_order == "desc" { "asc".into() } else { "desc".into() }); } else { top_sort.set(key); top_order.set("desc".into()); }
                    }, "Typ" }
                    th { class: "hide-mobile", style: "text-align:left;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "modified".to_string();
                        let current_sort = top_sort.read().clone();
                        let current_order = top_order.read().clone();
                        let mut top_sort = top_sort.clone();
                        let mut top_order = top_order.clone();
                        if current_sort == key { top_order.set(if current_order == "desc" { "asc".into() } else { "desc".into() }); } else { top_sort.set(key); top_order.set("desc".into()); }
                    }, "Zuletzt" }
                    th { style: "text-align:right;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "allocated".to_string();
                        let current_sort = top_sort.read().clone();
                        let current_order = top_order.read().clone();
                        let mut top_sort = top_sort.clone();
                        let mut top_order = top_order.clone();
                        if current_sort == key { top_order.set(if current_order == "desc" { "asc".into() } else { "desc".into() }); } else { top_sort.set(key); top_order.set("desc".into()); }
                    }, "Allokiert" }
                    th { class: "hide-mobile", style: "text-align:right;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "logical".to_string();
                        let current_sort = top_sort.read().clone();
                        let current_order = top_order.read().clone();
                        let mut top_sort = top_sort.clone();
                        let mut top_order = top_order.clone();
                        if current_sort == key { top_order.set(if current_order == "desc" { "asc".into() } else { "desc".into() }); } else { top_sort.set(key); top_order.set("desc".into()); }
                    }, "Logisch" }
                    th { style: "text-align:left;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "name".to_string();
                        let current_sort = top_sort.read().clone();
                        let current_order = top_order.read().clone();
                        let mut top_sort = top_sort.clone();
                        let mut top_order = top_order.clone();
                        if current_sort == key { top_order.set(if current_order == "desc" { "asc".into() } else { "desc".into() }); } else { top_sort.set(key); top_order.set("desc".into()); }
                    }, "Pfad" }
                    th { style: "text-align:left;padding:6px;border-bottom:1px solid #222533;", "Aktionen" }
                } }
                tbody {
                    {
                        let mut rows = top_items.read().clone();
                        // Sort key
                        rows.sort_by_key(|it| match it {
                            types::TopItem::Dir { allocated_size, logical_size, mtime, .. } => match top_sort.read().as_str() {
                                "logical" => *logical_size,
                                "name" => 0,
                                "type" => 0,
                                "modified" => mtime.unwrap_or(0),
                                _ => *allocated_size,
                            },
                            types::TopItem::File { allocated_size, logical_size, mtime, .. } => match top_sort.read().as_str() {
                                "logical" => *logical_size,
                                "name" => 0,
                                "type" => 1,
                                "modified" => mtime.unwrap_or(0),
                                _ => *allocated_size,
                            },
                        });
                        let current_sort = top_sort.read().clone();
                        let current_order = top_order.read().clone();
                        if current_sort == "name" {
                            rows.sort_by_key(|it| match it { types::TopItem::Dir { path, .. } | types::TopItem::File { path, .. } => path.to_lowercase() });
                        }
                        if current_sort == "type" {
                            rows.sort_by_key(|it| match it { types::TopItem::Dir { .. } => 0, _ => 1 });
                        }
                        if current_order == "desc" { rows.reverse(); }
                        rows.into_iter().map(|it| {
                            match it {
                                types::TopItem::Dir { path, allocated_size, logical_size, mtime, .. } => {
                                    let p_nav = path.clone();
                                    let p_copy = path.clone();
                                    let recent = mtime;
                                    rsx!{ tr {
                                        td { style: "padding:6px;border-bottom:1px solid #1b1e2a;", "Ordner" }
                                        td { class: "hide-mobile", style: "padding:6px;border-bottom:1px solid #1b1e2a;", "{fmt_ago_short(recent)}" }
                                        td { style: "padding:6px;text-align:right;border-bottom:1px solid #1b1e2a;", "{fmt_bytes(allocated_size)}" }
                                        td { class: "hide-mobile", style: "padding:6px;text-align:right;border-bottom:1px solid #1b1e2a;", "{fmt_bytes(logical_size)}" }
                                        td { style: "padding:6px;border-bottom:1px solid #1b1e2a;cursor:pointer;color:#9cdcfe;", onclick: move |_| { 
                                            let mut list_path = list_path.clone();
                                            list_path.set(Some(p_nav.clone())); 
                                            let mut hist = nav_history.read().clone();
                                            if !hist.contains(&p_nav) { hist.push(p_nav.clone()); }
                                            let mut nav_history = nav_history.clone();
                                            nav_history.set(hist);
                                        }, "{path}" }
                                        td { style: "padding:6px;border-bottom:1px solid #1b1e2a;",
                                            button { style: btn_style(), onclick: move |_| { copy_to_clipboard(p_copy.clone()); }, "Kopieren" }
                                        }
                                    } }
                                },
                                types::TopItem::File { path, allocated_size, logical_size, mtime, .. } => {
                                    let recent = mtime;
                                    rsx!{ tr {
                                        td { style: "padding:6px;border-bottom:1px solid #1b1e2a;", "Datei" }
                                        td { class: "hide-mobile", style: "padding:6px;border-bottom:1px solid #1b1e2a;", "{fmt_ago_short(recent)}" }
                                        td { style: "padding:6px;text-align:right;border-bottom:1px solid #1b1e2a;", "{fmt_bytes(allocated_size)}" }
                                        td { class: "hide-mobile", style: "padding:6px;text-align:right;border-bottom:1px solid #1b1e2a;", "{fmt_bytes(logical_size)}" }
                                        td { style: "padding:6px;border-bottom:1px solid #1b1e2a;", "{path}" }
                                        td { style: "padding:6px;border-bottom:1px solid #1b1e2a;",
                                            button { style: btn_style(), onclick: move |_| { copy_to_clipboard(path.clone()); }, "Kopieren" }
                                        }
                                    } }
                                },
                            }
                        })
                    }
                }
            }  // table close
            // (removed recent panel UI)
            // Baum-Ergebnisse (Detail-Liste)  
            h3 { style: "margin-top:16px;", "Baum – Ergebnisse" }
            table { class: "responsive-table",
                thead { tr {
                    th { style: "text-align:left;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "type".to_string();
                        let current_sort = tree_sort_view.read().clone();
                        let current_order = tree_order.read().clone();
                        let mut tree_sort_view = tree_sort_view.clone();
                        let mut tree_order = tree_order.clone();
                        if current_sort == key { tree_order.set(if current_order == "desc" { "asc".into() } else { "desc".into() }); } else { tree_sort_view.set(key); tree_order.set("desc".into()); }
                    }, "Typ" }
                    th { class: "hide-mobile", style: "text-align:left;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "modified".to_string();
                        let current_sort = tree_sort_view.read().clone();
                        let current_order = tree_order.read().clone();
                        let mut tree_sort_view = tree_sort_view.clone();
                        let mut tree_order = tree_order.clone();
                        if current_sort == key { tree_order.set(if current_order == "desc" { "asc".into() } else { "desc".into() }); } else { tree_sort_view.set(key); tree_order.set("desc".into()); }
                    }, "Zuletzt" }
                    th { style: "text-align:right;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "allocated".to_string();
                        let current_sort = tree_sort_view.read().clone();
                        let current_order = tree_order.read().clone();
                        let mut tree_sort_view = tree_sort_view.clone();
                        let mut tree_order = tree_order.clone();
                        if current_sort == key { tree_order.set(if current_order == "desc" { "asc".into() } else { "desc".into() }); } else { tree_sort_view.set(key); tree_order.set("desc".into()); }
                    }, "Allokiert" }
                    th { class: "hide-mobile", style: "text-align:right;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "logical".to_string();
                        let current_sort = tree_sort_view.read().clone();
                        let current_order = tree_order.read().clone();
                        let mut tree_sort_view = tree_sort_view.clone();
                        let mut tree_order = tree_order.clone();
                        if current_sort == key { tree_order.set(if current_order == "desc" { "asc".into() } else { "desc".into() }); } else { tree_sort_view.set(key); tree_order.set("desc".into()); }
                    }, "Logisch" }
                    th { style: "text-align:left;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "name".to_string();
                        let current_sort = tree_sort_view.read().clone();
                        let current_order = tree_order.read().clone();
                        let mut tree_sort_view = tree_sort_view.clone();
                        let mut tree_order = tree_order.clone();
                        if current_sort == key { tree_order.set(if current_order == "desc" { "asc".into() } else { "desc".into() }); } else { tree_sort_view.set(key); tree_order.set("desc".into()); }
                    }, "Pfad" }
                    th { class: "hide-mobile", style: "text-align:left;padding:6px;border-bottom:1px solid #222533;", "Visual" }
                    th { style: "text-align:left;padding:6px;border-bottom:1px solid #222533;", "Aktionen" }
                    th { style: "text-align:left;padding:6px;border-bottom:1px solid #222533;", "Aktionen" }
                } }
                tbody {
                    { let mut rows = tree_items.read().clone();
                      let current_sort = tree_sort_view.read().clone();
                      let current_order = tree_order.read().clone();
                      rows.sort_by_key(|n| match current_sort.as_str() {
                          "logical" => n.logical_size,
                          "name" => 0,
                          "type" => if n.is_dir { 0 } else { 1 },
                          "modified" => n.mtime.unwrap_or(0),
                          _ => n.allocated_size,
                      });
                      if current_sort == "name" { rows.sort_by_key(|n| n.path.to_lowercase()); }
                      if current_order == "desc" { rows.reverse(); }
                      rows.into_iter().map(|n| {
                        let t = if n.is_dir { "Ordner" } else { "Datei" };
                        let alloc = n.allocated_size; let logical = n.logical_size; let p = n.path.clone();
                        let percent = if max_alloc_tree > 0 { ((alloc as f64) / (max_alloc_tree as f64) * 100.0).clamp(1.0, 100.0) } else { 0.0 };
                        let bar_width = format!("width:{:.1}%;", percent);
                        let p_nav = p.clone();
                        let p_copy = p.clone();
                        let p_for_move = p.clone();
                        let move_signal = move_dialog.clone();
                        let bar_class = if n.is_dir { "bar-fill-indigo" } else { "bar-fill-green" };
                        let item_name = p
                            .rsplit_once(['\\', '/'])
                            .map(|(_, tail)| tail.to_string())
                            .unwrap_or_else(|| {
                                p_for_move
                                    .trim_end_matches(['\\', '/'])
                                    .rsplit_once(['\\', '/'])
                                    .map(|(_, tail)| tail.to_string())
                                    .unwrap_or_else(|| p_for_move.clone())
                            });
                        rsx!{ tr {
                            td { style: "padding:6px;border-bottom:1px solid #1b1e2a;", "{t}" }
                            td { class: "hide-mobile", style: "padding:6px;border-bottom:1px solid #1b1e2a;", "{fmt_ago_short(n.mtime)}" }
                            td { style: "padding:6px;text-align:right;border-bottom:1px solid #1b1e2a;", "{fmt_bytes(alloc)}" }
                            td { class: "hide-mobile", style: "padding:6px;text-align:right;border-bottom:1px solid #1b1e2a;", "{fmt_bytes(logical)}" }
                            td { style: "padding:6px;border-bottom:1px solid #1b1e2a;cursor:pointer;color:#9cdcfe;", onclick: move |_| { 
                                let mut list_path = list_path.clone();
                                list_path.set(Some(p_nav.clone())); 
                                let mut hist = nav_history.read().clone();
                                if !hist.contains(&p_nav) { hist.push(p_nav.clone()); }
                                let mut nav_history = nav_history.clone();
                                nav_history.set(hist);
                            }, "{p}" }
                            td { class: "hide-mobile", style: "padding:6px;border-bottom:1px solid #1b1e2a;min-width:160px;",
                                div { class: "bar-shell",
                                    div { class: "{bar_class}", style: "{bar_width}" }
                                }
                            }
                            td { style: "padding:6px;border-bottom:1px solid #1b1e2a;",
                                div { style: "display:flex;gap:8px;flex-wrap:wrap;",
                                    button { class: "btn", onclick: {
                                            let move_dialog = move_signal.clone();
                                            let path = p_for_move.clone();
                                            let name = item_name.clone();
                                            move |_| {
                                                let mut dlg = move_dialog.clone();
                                                dlg.set(Some(MoveDialogState {
                                                    source_path: path.clone(),
                                                    source_name: name.clone(),
                                                    logical_size: logical,
                                                    allocated_size: alloc,
                                                    destination: String::new(),
                                                    selected_drive: None,
                                                    remove_source: true,
                                                    overwrite: false,
                                                    in_progress: false,
                                                    done: false,
                                                    result: None,
                                                    error: None,
                                                }));
                                            }
                                        }, "Verschieben" }
                                    button { class: "btn", onclick: move |_| { copy_to_clipboard(p_copy.clone()); }, "Kopieren" }
                                }
                            }
                        } }
                    }) }
                }
            }

            // Explorer (Liste) – zeigt Kinder des aktuellen Pfads mit visuellen Größen-Balken
            div { style: "margin-top:16px;",
                div { style: "display:flex;gap:12px;align-items:center;flex-wrap:wrap;",
                    h3 { style: "margin:0 12px 0 0;", "Explorer (Liste)" }
                    button { class: "btn", disabled: *loading_list.read(), onclick: {
                        let f = do_load_list_btn.clone();
                        move |_| (f.as_ref())()
                    }, "Kinder laden" }
                    span { "Pfad:" }
                    input { value: "{list_path.read().as_ref().cloned().unwrap_or_default()}", placeholder: "leer = Wurzeln",
                        oninput: move |e| {
                            let value = e.value();
                            let mut list_path = list_path.clone();
                            let mut list_offset = list_offset.clone();
                            list_path.set(if value.is_empty() { None } else { Some(value) });
                            list_offset.set(0);
                        }
                    }
                    span { "Sort:" }
                    select { value: "{list_sort}", oninput: move |e| {
                            let value = e.value();
                            let mut list_sort = list_sort.clone();
                            let mut list_offset = list_offset.clone();
                            list_sort.set(value);
                            list_offset.set(0);
                        },
                        option { value: "allocated", "Allokiert" }
                        option { value: "logical", "Logisch" }
                        option { value: "name", "Name" }
                        option { value: "type", "Typ" }
                        option { value: "modified", "Änderungsdatum" }
                    }
                    span { "Reihenfolge:" }
                    select { value: "{list_order}", oninput: move |e| {
                            let value = e.value();
                            let mut list_order = list_order.clone();
                            let mut list_offset = list_offset.clone();
                            list_order.set(value);
                            list_offset.set(0);
                        },
                        option { value: "desc", "Absteigend" }
                        option { value: "asc", "Aufsteigend" }
                    }
                }
                // Filter-Bereich
                details { open: false, style: "margin-top:8px;padding:8px;background:#0f1117;border:1px solid #222533;border-radius:8px;",
                    summary { style: "cursor:pointer;color:#e5e7eb;", "Filter & Suche" }
                    div { style: "display:flex;gap:12px;align-items:center;flex-wrap:wrap;margin-top:8px;",
                        span { "Suche:" }
                        input { 
                            class: "form-control",
                            value: "{search_query}", 
                            placeholder: "Datei/Ordner suchen...",
                            style: "background:#1f2937;color:#e5e7eb;border:1px solid #374151;border-radius:6px;padding:4px 8px;",
                            oninput: move |e| {
                                let value = e.value();
                                let mut search_query = search_query.clone();
                                search_query.set(value);
                            }
                        }
                        span { "Min. Größe:" }
                        input { 
                            class: "form-control",
                            r#type: "number", 
                            min: "0", 
                            value: {
                                let unit = min_size_unit.read().clone();
                                let bytes = *min_size_filter.read();
                                match unit.as_str() {
                                    "mb" => {
                                        let val = (bytes as f64 / (1024.0 * 1024.0)).max(0.0);
                                        if (val.fract() - 0.0).abs() < f64::EPSILON {
                                            format!("{:.0}", val)
                                        } else {
                                            format!("{:.2}", val)
                                        }
                                    }
                                    "gb" => {
                                        let val = (bytes as f64 / (1024.0 * 1024.0 * 1024.0)).max(0.0);
                                        if (val.fract() - 0.0).abs() < f64::EPSILON {
                                            format!("{:.0}", val)
                                        } else {
                                            format!("{:.2}", val)
                                        }
                                    }
                                    _ => bytes.max(0).to_string(),
                                }
                            },
                            style: "background:#1f2937;color:#e5e7eb;border:1px solid #374151;border-radius:6px;padding:4px 8px;width:120px;",
                            oninput: move |e| {
                                let value = e.value();
                                let mut min_size_filter = min_size_filter.clone();
                                let unit = min_size_unit.read().clone();
                                if value.trim().is_empty() {
                                    min_size_filter.set(0);
                                } else if let Ok(v) = value.parse::<f64>() {
                                    let multiplier = match unit.as_str() {
                                        "mb" => 1024.0 * 1024.0,
                                        "gb" => 1024.0 * 1024.0 * 1024.0,
                                        _ => 1.0,
                                    };
                                    let bytes = (v.max(0.0) * multiplier).round() as i64;
                                    min_size_filter.set(bytes);
                                }
                            }
                        }
                        select { 
                            value: "{min_size_unit}",
                            style: "background:#1f2937;color:#e5e7eb;border:1px solid #374151;border-radius:6px;padding:4px 8px;",
                            oninput: move |e| {
                                let mut unit = min_size_unit.clone();
                                unit.set(e.value());
                            },
                            option { value: "b", "Bytes" }
                            option { value: "mb", "→ MB" }
                            option { value: "gb", "→ GB" }
                        }
                        span { "Typ:" }
                        select { 
                            value: "{file_type_filter}",
                            style: "background:#1f2937;color:#e5e7eb;border:1px solid #374151;border-radius:6px;padding:4px 8px;",
                            oninput: move |e| { let mut file_type_filter = file_type_filter.clone(); file_type_filter.set(e.value()); },
                            option { value: "all", "Alle" }
                            option { value: "dirs", "Nur Ordner" }
                            option { value: "files", "Nur Dateien" }
                        }
                        label { style: "display:flex;gap:6px;align-items:center;",
                            input { 
                                r#type: "checkbox", 
                                checked: *show_hidden.read(),
                                oninput: move |_| { let current = *show_hidden.read(); let mut show_hidden = show_hidden.clone(); show_hidden.set(!current); }
                            }
                            "Versteckte anzeigen"
                        }
                        button { 
                            style: "background:#2563eb;color:#fff;border:none;border-radius:6px;padding:6px 12px;cursor:pointer;",
                            onclick: move |_| {
                                let mut search_query = search_query.clone();
                                let mut min_size_filter = min_size_filter.clone();
                                let mut min_size_unit = min_size_unit.clone();
                                let mut file_type_filter = file_type_filter.clone();
                                let mut show_hidden = show_hidden.clone();
                                search_query.set(String::new());
                                min_size_filter.set(0);
                                min_size_unit.set("b".to_string());
                                file_type_filter.set("all".to_string());
                                show_hidden.set(false);
                            },
                            "Filter zurücksetzen"
                        }
                    }
                }
                // Pagination & Status
                div { style: "display:flex;gap:12px;align-items:center;flex-wrap:wrap;margin-top:8px;",
                    span { "Limit:" }
                    input { r#type: "number", min: "10", value: "{list_limit}", oninput: move |e| {
                            let value = e.value();
                            let mut list_limit = list_limit.clone();
                            let mut list_offset = list_offset.clone();
                            if let Ok(v) = value.parse::<i64>() {
                                list_limit.set(v.max(10));
                                list_offset.set(0);
                            }
                        }
                    }
                    // quick set buttons for common page sizes
                    button { class: "btn", style: "padding:4px 8px;", onclick: {
                        let list_limit = list_limit.clone(); let list_offset = list_offset.clone();
                        move |_| { let mut list_limit = list_limit.clone(); let mut list_offset = list_offset.clone(); list_limit.set(50); list_offset.set(0); }
                    }, "50" }
                    button { class: "btn", style: "padding:4px 8px;", onclick: {
                        let list_limit = list_limit.clone(); let list_offset = list_offset.clone();
                        move |_| { let mut list_limit = list_limit.clone(); let mut list_offset = list_offset.clone(); list_limit.set(100); list_offset.set(0); }
                    }, "100" }
                    // Make Prev always visible and clickable; on first page show a toast instead of disabling.
                    button { class: "btn btn-primary", r#type: "button", style: btn_primary_style(), onclick: {
                        let list_offset = list_offset.clone();
                        let list_limit = list_limit.clone();
                        let list_has_more = list_has_more.clone();
                        let list_path = list_path.clone();
                        let nav_hist = nav_history.clone();
                        let do_btn = do_load_list_btn.clone();
                        move |_| {
                            let current_offset = *list_offset.read();
                            if current_offset <= 0 {
                                // On first page: step back in navigation history if available, otherwise compute parent path
                                let mut hist = nav_hist.read().clone();
                                let mut nav_hist_mut = nav_hist.clone();
                                let mut list_path_mut = list_path.clone();
                                let mut list_offset_mut = list_offset.clone();
                                if hist.is_empty() {
                                    // Try compute parent path from current list_path
                                    let current_path = list_path.read().clone();
                                    if let Some(cur) = current_path {
                                        let s = cur.trim_end_matches(['\\','/']).to_string();
                                        let mut cut: Option<usize> = None;
                                        for (i, ch) in s.char_indices().rev() { if ch == '\\' || ch == '/' { cut = Some(i); break; } }
                                        let parent = cut.map(|i| s[..i].to_string());
                                        if let Some(par) = parent.filter(|v| !v.is_empty() && !v.ends_with(':') && v.len() > 2) {
                                            nav_hist_mut.set(vec![par.clone()]);
                                            list_path_mut.set(Some(par));
                                            list_offset_mut.set(0);
                                            (do_btn.as_ref())();
                                            show_toast("Zurück");
                                            console::log_1(&"Prev click: computed parent".into());
                                        } else {
                                            // No parent left → roots
                                            nav_hist_mut.set(Vec::new());
                                            list_path_mut.set(None);
                                            list_offset_mut.set(0);
                                            (do_btn.as_ref())();
                                            show_toast("Zurück (Wurzeln)");
                                            console::log_1(&"Prev click: to roots".into());
                                        }
                                    } else {
                                        show_toast("Keine vorherige Seite");
                                        console::log_1(&format!("Prev click on page 1 (offset=0). No nav history. path=None").into());
                                    }
                                } else {
                                    // Remove current entry
                                    let _ = hist.pop();
                                    // Determine target: previous path or None (roots)
                                    let target = hist.last().cloned();
                                    nav_hist_mut.set(hist);
                                    list_path_mut.set(target);
                                    list_offset_mut.set(0);
                                    (do_btn.as_ref())();
                                    show_toast("Zurück");
                                    console::log_1(&"Prev click: history back".into());
                                }
                            } else {
                                let old_off = current_offset;
                                let current_limit = *list_limit.read();
                                let old_page = (old_off / current_limit) + 1;
                                let new_off = (old_off - current_limit).max(0);
                                let mut list_has_more_mut = list_has_more.clone();
                                let mut list_offset_mut = list_offset.clone();
                                if new_off < current_offset { list_has_more_mut.set(true); }
                                console::log_1(&format!("Prev click: offset {} -> {} (limit {}), path={:?}", old_off, new_off, current_limit, list_path.read().clone()).into());
                                list_offset_mut.set(new_off);
                                // Trigger immediate reload for snappier UX
                                (do_btn.as_ref())();
                                let new_page = (new_off / current_limit) + 1;
                                let msg = format!("Seite {} → {}", old_page, new_page);
                                show_toast(&msg);
                            }
                        }
                    }, "Vorherige Seite" }
                    // Allow trying to load the next page even if `has_more` is currently false.
                    // The effect re-fetch will update `list_has_more` and item list accordingly.
                    button { class: "btn btn-primary", r#type: "button", style: btn_primary_style(), disabled: *loading_list.read(), title: if *loading_list.read() { "Laden läuft…" } else { "Nächste Seite laden" }, onclick: next_page, "Nächste Seite" }
                    span { "Seite: {(*list_offset.read() / *list_limit.read()) + 1} (Offset: {*list_offset.read()})" }
                    span { "Einträge (Seite): {list_items.len()}" }
                    button { class: "btn", onclick: {
                            let list_items = list_items.clone();
                            let search_query = search_query.clone();
                            let min_size_filter = min_size_filter.clone();
                            let file_type_filter = file_type_filter.clone();
                            let show_hidden = show_hidden.clone();
                            move |_| {
                                let mut csv = String::from("type,name,path,allocated,logical,mtime\n");
                                let query_val = search_query.read().to_lowercase();
                                let min_size_val = min_size_filter.read().clone();
                                let type_filter_val = file_type_filter.read().clone();
                                let show_hidden_val = *show_hidden.read();
                                for it in list_items.read().iter().filter(|it| {
                                    let name_match = if query_val.is_empty() { true } else { match it { types::ListItem::Dir { name, .. } => name.to_lowercase().contains(&query_val), types::ListItem::File { name, .. } => name.to_lowercase().contains(&query_val), } };
                                    let size_match = match it { types::ListItem::Dir { allocated_size, .. } => *allocated_size >= min_size_val, types::ListItem::File { allocated_size, .. } => *allocated_size >= min_size_val, };
                                    let type_match = match type_filter_val.as_str() { "dirs" => matches!(it, types::ListItem::Dir { .. }), "files" => matches!(it, types::ListItem::File { .. }), _ => true };
                                    let hidden_match = if !show_hidden_val { match it { types::ListItem::Dir { name, .. } => !name.starts_with('.'), types::ListItem::File { name, .. } => !name.starts_with('.'), } } else { true };
                                    name_match && size_match && type_match && hidden_match
                                }) {
                                    match it {
                                        types::ListItem::Dir { name, path, allocated_size, logical_size, mtime, .. } => {
                                            csv.push_str(&format!("dir,\"{}\",\"{}\",{},{},{}\n", name.replace('"', ""), path.replace('"', ""), allocated_size, logical_size, mtime.unwrap_or(0)));
                                        }
                                        types::ListItem::File { name, path, allocated_size, logical_size, mtime, .. } => {
                                            csv.push_str(&format!("file,\"{}\",\"{}\",{},{},{}\n", name.replace('"', ""), path.replace('"', ""), allocated_size, logical_size, mtime.unwrap_or(0)));
                                        }
                                    }
                                }
                                download_csv("speicherwald_list.csv", &csv);
                            }
                        }, "CSV export" }
                    { (*loading_list.read()).then(|| rsx!(span { class: "spinner", "" })) }
                    { err_list.read().as_ref().map(|e| rsx!(span { class: "text-danger", " Fehler: {e}" })) }
                }

                { (list_items.read().is_empty() && list_path.read().is_none() && !*loading_list.read()).then(|| rsx!(
                    div { class: "alert alert-warning", "Keine Daten für Wurzeln – der Scan läuft eventuell noch oder die Root-Knoten wurden noch nicht gespeichert. Versuche es gleich erneut oder nutze Baum/Top." }
                )) }
            }
            table { class: "responsive-table",
                thead { tr {
                    th { style: "text-align:left;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "name".to_string();
                        let current_sort = list_sort.read().clone();
                        let current_order = list_order.read().clone();
                        let mut list_sort = list_sort.clone();
                        let mut list_order = list_order.clone();
                        let mut list_offset = list_offset.clone();
                        if current_sort == key { list_order.set(if current_order == "desc" { "asc".into() } else { "desc".into() }); } else { list_sort.set(key); list_order.set("desc".into()); }
                        list_offset.set(0);
                    }, "Name" }
                    th { style: "text-align:left;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "type".to_string();
                        let current_sort = list_sort.read().clone();
                        let current_order = list_order.read().clone();
                        let mut list_sort = list_sort.clone();
                        let mut list_order = list_order.clone();
                        let mut list_offset = list_offset.clone();
                        if current_sort == key { list_order.set(if current_order == "desc" { "asc".into() } else { "desc".into() }); } else { list_sort.set(key); list_order.set("desc".into()); }
                        list_offset.set(0);
                    }, "Typ" }
                    th { class: "hide-mobile", style: "text-align:left;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "modified".to_string();
                        let current_sort = list_sort.read().clone();
                        let current_order = list_order.read().clone();
                        let mut list_sort = list_sort.clone();
                        let mut list_order = list_order.clone();
                        let mut list_offset = list_offset.clone();
                        if current_sort == key { list_order.set(if current_order == "desc" { "asc".into() } else { "desc".into() }); } else { list_sort.set(key); list_order.set("desc".into()); }
                        list_offset.set(0);
                    }, "Zuletzt" }
                    th { style: "text-align:right;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "allocated".to_string();
                        let current_sort = list_sort.read().clone();
                        let current_order = list_order.read().clone();
                        let mut list_sort = list_sort.clone();
                        let mut list_order = list_order.clone();
                        let mut list_offset = list_offset.clone();
                        if current_sort == key { list_order.set(if current_order == "desc" { "asc".into() } else { "desc".into() }); } else { list_sort.set(key); list_order.set("desc".into()); }
                        list_offset.set(0);
                    }, "Allokiert" }
                    th { class: "hide-mobile", style: "text-align:right;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "logical".to_string();
                        let current_sort = list_sort.read().clone();
                        let current_order = list_order.read().clone();
                        let mut list_sort = list_sort.clone();
                        let mut list_order = list_order.clone();
                        let mut list_offset = list_offset.clone();
                        if current_sort == key { list_order.set(if current_order == "desc" { "asc".into() } else { "desc".into() }); } else { list_sort.set(key); list_order.set("desc".into()); }
                        list_offset.set(0);
                    }, "Logisch" }
                    th { class: "hide-mobile", style: "text-align:left;padding:6px;border-bottom:1px solid #222533;", "Visual" }
                } }
                tbody {
                    { 
                      let query_val = search_query.read().to_lowercase();
                      let min_size_val = min_size_filter.read().clone();
                      let type_filter_val = file_type_filter.read().clone();
                      let show_hidden_val = *show_hidden.read();
                      let filtered: Vec<_> = list_items.read().iter()
                        .filter(|it| {
                            // Suchfilter
                            let name_match = if query_val.is_empty() { 
                                true 
                            } else {
                                match it {
                                    types::ListItem::Dir { name, .. } => name.to_lowercase().contains(&query_val),
                                    types::ListItem::File { name, .. } => name.to_lowercase().contains(&query_val),
                                }
                            };
                            
                            // Größenfilter
                            let size_match = match it {
                                types::ListItem::Dir { allocated_size, .. } => *allocated_size >= min_size_val,
                                types::ListItem::File { allocated_size, .. } => *allocated_size >= min_size_val,
                            };
                            
                            // Typfilter
                            let type_match = match type_filter_val.as_str() {
                                "dirs" => matches!(it, types::ListItem::Dir { .. }),
                                "files" => matches!(it, types::ListItem::File { .. }),
                                _ => true,
                            };
                            
                            // Versteckte Dateien Filter
                            let hidden_match = if !show_hidden_val {
                                match it {
                                    types::ListItem::Dir { name, .. } => !name.starts_with('.'),
                                    types::ListItem::File { name, .. } => !name.starts_with('.'),
                                }
                            } else {
                                true
                            };
                            
                            name_match && size_match && type_match && hidden_match
                        })
                        .cloned()
                        .collect();
                      
                      filtered.into_iter().map(|it| {
                        match it {
                            types::ListItem::Dir { name, path, allocated_size, logical_size, mtime, .. } => {
                                let alloc = allocated_size; let logical = logical_size; let p = path.clone();
                                let percent = if max_alloc_list > 0 { ((alloc as f64) / (max_alloc_list as f64) * 100.0).clamp(1.0, 100.0) } else { 0.0 };
                                let bar_width = format!("width:{:.1}%;", percent);
                                let recent = mtime;
                                let move_signal = move_dialog.clone();
                                let path_for_dialog = p.clone();
                                let name_for_dialog = name.clone();
                                rsx!{ tr {
                                    td { style: "padding:6px;border-bottom:1px solid #1b1e2a;cursor:pointer;color:#9cdcfe;", onclick: move |_| { 
                                        let hist = nav_history.read().clone();
                                        let mut list_path = list_path.clone();
                                        list_path.set(Some(p.clone())); 
                                        let mut hist = hist;
                                        if !hist.contains(&p) { hist.push(p.clone()); }
                                        let mut nav_history = nav_history.clone();
                                        nav_history.set(hist);
                                    }, "{name}" }
                                    td { style: "padding:6px;border-bottom:1px solid #1b1e2a;", "Ordner" }
                                    td { class: "hide-mobile", style: "padding:6px;border-bottom:1px solid #1b1e2a;", "{fmt_ago_short(recent)}" }
                                    td { style: "padding:6px;text-align:right;border-bottom:1px solid #1b1e2a;", "{fmt_bytes(alloc)}" }
                                    td { class: "hide-mobile", style: "padding:6px;text-align:right;border-bottom:1px solid #1b1e2a;", "{fmt_bytes(logical)}" }
                                    td { class: "hide-mobile", style: "padding:6px;border-bottom:1px solid #1b1e2a;min-width:160px;",
                                        div { class: "bar-shell", 
                                            div { class: "bar-fill-blue", style: "{bar_width}" }
                                        }
                                    }
                                    td { style: "padding:6px;border-bottom:1px solid #1b1e2a;",
                                        div { style: "display:flex;gap:8px;flex-wrap:wrap;",
                                            button { class: "btn", onclick: {
                                                    let signal = move_signal.clone();
                                                    let path_value = path_for_dialog.clone();
                                                    let label_value = name_for_dialog.clone();
                                                    move |_| {
                                                        let mut dlg = signal.clone();
                                                        dlg.set(Some(MoveDialogState {
                                                            source_path: path_value.clone(),
                                                            source_name: label_value.clone(),
                                                            logical_size: logical,
                                                            allocated_size: alloc,
                                                            destination: String::new(),
                                                            selected_drive: None,
                                                            remove_source: true,
                                                            overwrite: false,
                                                            in_progress: false,
                                                            done: false,
                                                            result: None,
                                                            error: None,
                                                        }));
                                                    }
                                                }, "Verschieben" }
                                            button { class: "btn", onclick: move |_| { copy_to_clipboard(path_for_dialog.clone()); }, "Kopieren" }
                                        }
                                    }
                                } }
                            }
                            types::ListItem::File { name, path, allocated_size, logical_size, mtime, .. } => {
                                let alloc = allocated_size; let logical = logical_size;
                                let percent = if max_alloc_list > 0 { ((alloc as f64) / (max_alloc_list as f64) * 100.0).clamp(1.0, 100.0) } else { 0.0 };
                                let bar_width = format!("width:{:.1}%;", percent);
                                let recent = mtime;
                                let move_signal = move_dialog.clone();
                                let path_for_dialog = path.clone();
                                let name_for_dialog = name.clone();
                                rsx!{ tr {
                                    td { style: "padding:6px;border-bottom:1px solid #1b1e2a;", "{name}" }
                                    td { style: "padding:6px;border-bottom:1px solid #1b1e2a;", "Datei" }
                                    td { class: "hide-mobile", style: "padding:6px;border-bottom:1px solid #1b1e2a;", "{fmt_ago_short(recent)}" }
                                    td { style: "padding:6px;text-align:right;border-bottom:1px solid #1b1e2a;", "{fmt_bytes(alloc)}" }
                                    td { class: "hide-mobile", style: "padding:6px;text-align:right;border-bottom:1px solid #1b1e2a;", "{fmt_bytes(logical)}" }
                                    td { class: "hide-mobile", style: "padding:6px;border-bottom:1px solid #1b1e2a;min-width:160px;",
                                        div { class: "bar-shell", 
                                            div { class: "bar-fill-green", style: "{bar_width}" }
                                        }
                                    }
                                    td { style: "padding:6px;border-bottom:1px solid #1b1e2a;",
                                        div { style: "display:flex;gap:8px;flex-wrap:wrap;",
                                            button { class: "btn", onclick: {
                                                    let signal = move_signal.clone();
                                                    let path_value = path_for_dialog.clone();
                                                    let label_value = name_for_dialog.clone();
                                                    move |_| {
                                                        let mut dlg = signal.clone();
                                                        dlg.set(Some(MoveDialogState {
                                                            source_path: path_value.clone(),
                                                            source_name: label_value.clone(),
                                                            logical_size: logical,
                                                            allocated_size: alloc,
                                                            destination: String::new(),
                                                            selected_drive: None,
                                                            remove_source: true,
                                                            overwrite: false,
                                                            in_progress: false,
                                                            done: false,
                                                            result: None,
                                                            error: None,
                                                        }));
                                                    }
                                                }, "Verschieben" }
                                            button { class: "btn", onclick: move |_| { copy_to_clipboard(path_for_dialog.clone()); }, "Kopieren" }
                                        }
                                    }
                                } }
                            }
                        }
                    }) }
                }
            }  // table close
        }  // section close
        { move_dialog.read().as_ref().map(|dlg| move_dialog_view(dlg, move_dialog.clone(), drive_targets.clone(), drive_fetch_error.clone())) }
    }
}

fn move_dialog_view(
    dialog: &MoveDialogState,
    move_signal: Signal<Option<MoveDialogState>>,
    drive_targets: Signal<Vec<types::DriveInfo>>,
    drive_error: Signal<Option<String>>,
) -> Element {
    let drives_snapshot = drive_targets.read().clone();
    let drive_error_val = drive_error.read().clone();
    let is_done = dialog.done;
    let is_running = dialog.in_progress;
    let destination_blank = dialog.destination.trim().is_empty();

    let transfer_size_txt = fmt_bytes(dialog.allocated_size);
    let logical_size_txt = fmt_bytes(dialog.logical_size);
    let estimated_gain_txt = if dialog.remove_source {
        fmt_bytes(dialog.allocated_size)
    } else {
        "0 B".to_string()
    };

    let selected_drive_txt = dialog
        .selected_drive
        .clone()
        .unwrap_or_else(|| "Manuelle Eingabe".to_string());
    let source_name_display = if dialog.source_name.trim().is_empty() {
        "Ausgewaehlter Pfad".to_string()
    } else {
        dialog.source_name.clone()
    };
    let source_path_clip = dialog.source_path.clone();
    let quick_drive_hint = if dialog.selected_drive.is_some() {
        "Schnellwahl aktiv"
    } else {
        "Optional: Laufwerk fuer Schnellwahl auswaehlen"
    };

    let (status_label, status_grad_start, status_grad_end, status_border, status_text) = if is_done {
        ("Abgeschlossen", "#0f3b2a", "#166534", "#22c55e", "#bbf7d0")
    } else if is_running {
        ("Aktiv", "#0f3a58", "#1d4ed8", "#38bdf8", "#dbeafe")
    } else {
        ("Bereit", "#1e293b", "#101827", "#4b5563", "#e5e7eb")
    };
    let status_chip_style = format!(
        "background:linear-gradient(135deg, {} 0%, {} 100%);color:{};border:1px solid {};border-radius:999px;padding:4px 12px;font-size:12px;font-weight:600;letter-spacing:0.08em;text-transform:uppercase;box-shadow:0 10px 24px rgba(15,23,42,0.45);",
        status_grad_start, status_grad_end, status_text, status_border
    );

    rsx! {
        div { style: "position:fixed;top:0;left:0;width:100vw;height:100vh;padding:24px;display:flex;align-items:center;justify-content:center;background:rgba(7,11,20,0.82);backdrop-filter:blur(6px) brightness(0.92);z-index:2000;",
            div { style: "background:linear-gradient(150deg,#0b111c 0%,#111b2b 60%,#0c1626 100%);border:1px solid #1f2a3d;border-radius:20px;padding:28px;max-width:720px;width:100%;color:#e5e7eb;box-shadow:0 28px 46px rgba(3,7,18,0.55);display:flex;flex-direction:column;gap:20px;max-height:92vh;overflow:auto;",
                div { style: "display:flex;justify-content:space-between;gap:18px;align-items:flex-start;",
                    div { style: "display:flex;flex-direction:column;gap:10px;",
                        span { style: "{status_chip_style}", "{status_label}" }
                        h3 { style: "margin:0;font-size:22px;font-weight:600;color:#fafafa;", "Pfad verschieben" }
                        span { style: "color:#94a3b8;font-size:13px;", "Waehle ein Ziel und starte den Transfer." }
                    }
                    button {
                        class: "btn",
                        style: "background:transparent;border:1px solid #2d3445;color:#cbd5f5;border-radius:10px;padding:8px 14px;font-size:13px;letter-spacing:0.04em;text-transform:uppercase;",
                        onclick: {
                            let close_signal = move_signal.clone();
                            move |_| {
                                let mut signal = close_signal.clone();
                                signal.set(None);
                            }
                        },
                        "Schliessen"
                    }
                }
                div { style: "display:grid;gap:14px;grid-template-columns:repeat(auto-fit,minmax(240px,1fr));",
                    div { style: "background:rgba(17,25,38,0.9);border:1px solid #253246;border-radius:14px;padding:14px;display:flex;flex-direction:column;gap:8px;",
                        span { style: "color:#64748b;font-size:12px;letter-spacing:0.08em;text-transform:uppercase;", "Quelle" }
                        span { style: "font-size:16px;font-weight:600;color:#e2e8f0;", "{source_name_display}" }
                        div { style: "display:flex;gap:8px;align-items:center;justify-content:space-between;flex-wrap:wrap;",
                            code { style: "flex:1;font-size:12px;background:#101828;border:1px solid #1f2937;border-radius:8px;padding:8px;word-break:break-all;color:#cbd5f5;", "{dialog.source_path}" }
                            button {
                                class: "btn",
                                style: "background:#1f2937;border:1px solid #334155;color:#93c5fd;border-radius:8px;padding:6px 10px;font-size:12px;",
                                onclick: move |_| {
                                    copy_to_clipboard(source_path_clip.clone());
                                    show_toast("Pfad kopiert");
                                },
                                "Kopieren"
                            }
                        }
                        span { style: "color:#6b7280;font-size:12px;", "Logisch: {logical_size_txt} | Belegt: {transfer_size_txt}" }
                    }
                    div { style: "background:rgba(17,25,38,0.9);border:1px solid #253246;border-radius:14px;padding:14px;display:flex;flex-direction:column;gap:8px;",
                        span { style: "color:#64748b;font-size:12px;letter-spacing:0.08em;text-transform:uppercase;", "Schnellwahl Laufwerk" }
                        span { style: "font-size:16px;font-weight:600;color:#e2e8f0;", "{selected_drive_txt}" }
                        span { style: "color:#6b7280;font-size:12px;", "{quick_drive_hint}" }
                        { (!drives_snapshot.is_empty()).then(|| rsx!{
                            div { style: "display:flex;flex-wrap:wrap;gap:10px;",
                                { drives_snapshot.iter().map(|drive| {
                                    let drive_path = drive.path.clone();
                                    let free_fmt = fmt_bytes((drive.free_bytes.min(i64::MAX as u64)) as i64);
                                    let is_selected = dialog.selected_drive.as_ref().map(|p| p == &drive_path).unwrap_or(false);
                                    let button_style = if is_selected {
                                        "background:linear-gradient(140deg,#1d4fd8 0%,#2563eb 100%);color:#fff;border:1px solid #4b9fff;border-radius:10px;padding:8px 12px;cursor:pointer;box-shadow:0 12px 24px rgba(37,99,235,0.35);transition:all 0.2s ease;"
                                    } else {
                                        "background:rgba(17,24,39,0.85);color:#e5e7eb;border:1px solid #2e3a4f;border-radius:10px;padding:8px 12px;cursor:pointer;transition:all 0.2s ease;"
                                    };
                                    let move_signal_drive = move_signal.clone();
                                    let dialog_snapshot = dialog.clone();
                                    let dest_suggestion = if drive_path.ends_with('\\') || drive_path.ends_with('/') {
                                        format!("{}{}", drive_path, dialog_snapshot.source_name)
                                    } else {
                                        format!("{}\\{}", drive_path, dialog_snapshot.source_name)
                                    };
                                    rsx!{
                                        button { style: "{button_style}", onclick: move |_| {
                                                let mut next = dialog_snapshot.clone();
                                                let previously_selected = next.selected_drive.clone();
                                                next.selected_drive = Some(drive_path.clone());
                                                if next.destination.trim().is_empty() || previously_selected.as_ref() != Some(&drive_path) {
                                                    next.destination = dest_suggestion.clone();
                                                }
                                                next.error = None;
                                                let mut signal = move_signal_drive.clone();
                                                signal.set(Some(next));
                                            }, "{drive_path} - frei: {free_fmt}" }
                                    }
                                }) }
                            }
                        }) }
                        { (drives_snapshot.is_empty()).then(|| rsx!{
                            span { style: "color:#64748b;font-size:12px;", "Keine Laufwerke verfuegbar." }
                        }) }
                        { drive_error_val.as_ref().map(|err| rsx!{
                            div { style: "margin-top:4px;padding:8px;background:#331414;border:1px solid #7f1d1d;border-radius:8px;color:#fca5a5;font-size:12px;",
                                "Laufwerke konnten nicht aktualisiert werden: {err}"
                            }
                        }) }
                    }
                }
                div { style: "display:flex;flex-direction:column;gap:6px;",
                    span { style: "color:#64748b;font-size:12px;letter-spacing:0.08em;text-transform:uppercase;", "Ziel" }
                    input {
                        class: "form-control",
                        value: "{dialog.destination}",
                        placeholder: "\\\\server\\share\\ordner",
                        style: "background:#101828;color:#e5e7eb;border:1px solid #2e3a4f;border-radius:10px;padding:10px;font-size:14px;",
                        oninput: {
                            let move_signal_dest = move_signal.clone();
                            let snapshot = dialog.clone();
                            move |e: Event<FormData>| {
                                let mut next = snapshot.clone();
                                next.destination = e.value();
                                next.error = None;
                                let mut signal = move_signal_dest.clone();
                                signal.set(Some(next));
                            }
                        }
                    }
                    span { style: "color:#6b7280;font-size:12px;", "Tipp: Trage einen UNC Pfad ein oder nutze die Schnellwahl." }
                }
                div { style: "display:grid;gap:12px;grid-template-columns:repeat(auto-fit,minmax(240px,1fr));",
                    div { style: "background:#121c2b;border:1px solid #203049;border-radius:14px;padding:14px;display:flex;gap:8px;align-items:flex-start;",
                        div { style: "flex:1;display:flex;flex-direction:column;gap:6px;",
                            span { style: "color:#94a3b8;font-size:12px;text-transform:uppercase;letter-spacing:0.08em;", "Aktionen" }
                            div { style: "display:flex;flex-direction:column;gap:10px;",
                                label { style: "display:flex;gap:10px;align-items:flex-start;background:#152135;border:1px solid #233146;border-radius:10px;padding:10px;",
                                    input {
                                        r#type: "checkbox",
                                        checked: dialog.remove_source,
                                        oninput: {
                                            let move_signal_remove = move_signal.clone();
                                            let snapshot = dialog.clone();
                                            move |_| {
                                                let mut next = snapshot.clone();
                                                next.remove_source = !snapshot.remove_source;
                                                next.error = None;
                                                let mut signal = move_signal_remove.clone();
                                                signal.set(Some(next));
                                            }
                                        }
                                    }
                                    div { style: "display:flex;flex-direction:column;gap:2px;",
                                        span { style: "font-size:13px;font-weight:600;color:#e2e8f0;", "Quelle nach Abschluss loeschen" }
                                        span { style: "font-size:12px;color:#94a3b8;", "Erhoeht den freigegebenen Speicher um {estimated_gain_txt}." }
                                    }
                                }
                                label { style: "display:flex;gap:10px;align-items:flex-start;background:#152135;border:1px solid #233146;border-radius:10px;padding:10px;",
                                    input {
                                        r#type: "checkbox",
                                        checked: dialog.overwrite,
                                        oninput: {
                                            let move_signal_overwrite = move_signal.clone();
                                            let snapshot = dialog.clone();
                                            move |_| {
                                                let mut next = snapshot.clone();
                                                next.overwrite = !snapshot.overwrite;
                                                next.error = None;
                                                let mut signal = move_signal_overwrite.clone();
                                                signal.set(Some(next));
                                            }
                                        }
                                    }
                                    div { style: "display:flex;flex-direction:column;gap:2px;",
                                        span { style: "font-size:13px;font-weight:600;color:#e2e8f0;", "Vorhandene Dateien ueberschreiben" }
                                        span { style: "font-size:12px;color:#94a3b8;", "Ersetzt Zielkonflikte automatisch." }
                                    }
                                }
                            }
                        }
                    }
                    div { style: "background:#121c2b;border:1px solid #203049;border-radius:14px;padding:14px;display:flex;flex-direction:column;gap:8px;font-size:13px;",
                        span { style: "color:#94a3b8;font-size:12px;text-transform:uppercase;letter-spacing:0.08em;", "Zusammenfassung" }
                        div { style: "display:flex;flex-direction:column;gap:8px;",
                            span { style: "display:flex;justify-content:space-between;gap:12px;",
                                span { style: "color:#6b7280;", "Transfermenge" }
                                span { style: "color:#f8fafc;font-weight:600;", "{transfer_size_txt}" }
                            }
                            span { style: "display:flex;justify-content:space-between;gap:12px;",
                                span { style: "color:#6b7280;", "Logischer Umfang" }
                                span { style: "color:#f8fafc;font-weight:600;", "{logical_size_txt}" }
                            }
                            span { style: "display:flex;justify-content:space-between;gap:12px;",
                                span { style: "color:#6b7280;", "Freier Speicher" }
                                span { style: "color:#34d399;font-weight:600;", "{estimated_gain_txt}" }
                            }
                        }
                    }
                }
                { if is_running {
                    Some(rsx!{
                        div { style: "display:flex;gap:10px;align-items:center;color:#60a5fa;font-size:13px;padding:10px 12px;border-radius:12px;background:rgba(37,99,235,0.12);border:1px solid rgba(96,165,250,0.35);",
                            span { class: "spinner" }
                            span { "Verschiebe Daten ..." }
                        }
                    })
                } else {
                    None
                } }
                { dialog.error.as_ref().map(|err| rsx!{
                    div { style: "padding:12px;background:#331414;border:1px solid #7f1d1d;border-radius:10px;color:#fca5a5;font-size:13px;",
                        "Fehler: {err}"
                    }
                }) }
                { dialog.result.as_ref().map(|res| {
                    let freed_fmt = fmt_bytes((res.freed_bytes.min(i64::MAX as u64)) as i64);
                    let moved_fmt = fmt_bytes((res.bytes_moved.min(i64::MAX as u64)) as i64);
                    let total_fmt = fmt_bytes((res.bytes_to_transfer.min(i64::MAX as u64)) as i64);
                    let duration_sec = (res.duration_ms as f64) / 1000.0;
                    let duration_txt = format!("{:.1} s", duration_sec);
                    let warnings = res.warnings.clone();
                    rsx!{
                        div { style: "padding:14px;background:#172031;border:1px solid #22304b;border-radius:12px;display:flex;flex-direction:column;gap:8px;font-size:13px;",
                            span { style: "color:#93c5fd;font-weight:600;", "Status: {res.status}" }
                            span { "Daten verschoben: {moved_fmt} von {total_fmt}" }
                            span { "Freier Speicher: {freed_fmt}" }
                            span { "Dauer: {duration_txt}" }
                            { if !warnings.is_empty() {
                                Some(rsx!{
                                    div { style: "display:flex;flex-direction:column;gap:4px;",
                                        span { style: "color:#facc15;", "Hinweise" }
                                        ul { style: "margin:0 0 0 16px;padding:0;display:flex;flex-direction:column;gap:4px;",
                                            { warnings.iter().map(|w| rsx!{ li { style: "list-style:disc;color:#facc15;", "{w}" } }) }
                                        }
                                    }
                                })
                            } else { None } }
                        }
                    }
                }) }
                span { style: "color:#6b7280;font-size:12px;", "Hinweis: Tabellen aktualisieren sich nach einem neuen Scan." }
                div { style: "display:flex;justify-content:flex-end;gap:12px;margin-top:4px;flex-wrap:wrap;",
                    button {
                        class: "btn",
                        style: "background:transparent;border:1px solid #2d3445;color:#cbd5f5;border-radius:10px;padding:8px 14px;font-size:13px;letter-spacing:0.04em;text-transform:uppercase;",
                        disabled: is_running,
                        onclick: {
                            let close_signal = move_signal.clone();
                            move |_| {
                                let mut signal = close_signal.clone();
                                signal.set(None);
                            }
                        },
                        { if is_done { "Schliessen" } else { "Abbrechen" } }
                    }
                    { (!is_done).then(|| rsx!{
                        button {
                            class: "btn btn-primary",
                            style: btn_primary_style(),
                            disabled: is_running || destination_blank,
                            onclick: {
                                let move_signal_start = move_signal.clone();
                                let drives_signal_async = drive_targets.clone();
                                let drive_error_signal_async = drive_error.clone();
                                let dialog_snapshot = dialog.clone();
                                move |_| {
                                    if dialog_snapshot.in_progress {
                                        return;
                                    }

                                    let mut inflight = dialog_snapshot.clone();
                                    inflight.in_progress = true;
                                    inflight.error = None;
                                    inflight.done = false;
                                    inflight.result = None;

                                    let request = types::MovePathRequest {
                                        source: dialog_snapshot.source_path.clone(),
                                        destination: dialog_snapshot.destination.trim().to_string(),
                                        remove_source: dialog_snapshot.remove_source,
                                        overwrite: dialog_snapshot.overwrite,
                                    };

                                    let mut signal = move_signal_start.clone();
                                    signal.set(Some(inflight.clone()));

                                    let mut drive_error_signal = drive_error_signal_async.clone();
                                    drive_error_signal.set(None);

                                    wasm_bindgen_futures::spawn_local({
                                        let move_signal_async = move_signal_start.clone();
                                        let drives_signal_async = drives_signal_async.clone();
                                        let drive_error_signal_async = drive_error_signal_async.clone();
                                        let inflight_state = inflight.clone();
                                        let request = request.clone();

                                        async move {
                                            match api::move_path(&request).await {
                                                Ok(resp) => {
                                                    let mut updated = inflight_state.clone();
                                                    updated.in_progress = false;
                                                    updated.done = true;
                                                    updated.result = Some(resp);

                                                    let mut move_signal_async = move_signal_async.clone();
                                                    move_signal_async.set(Some(updated));

                                                    show_toast("Pfad wurde verschoben");

                                                    match api::list_drives().await {
                                                        Ok(dr) => {
                                                            let mut drives_signal_async = drives_signal_async.clone();
                                                            drives_signal_async.set(dr.items);

                                                            let mut drive_error_signal_async = drive_error_signal_async.clone();
                                                            drive_error_signal_async.set(None);
                                                        }
                                                        Err(err) => {
                                                            let mut drive_error_signal_async = drive_error_signal_async.clone();
                                                            drive_error_signal_async.set(Some(err));
                                                        }
                                                    }
                                                }
                                                Err(err) => {
                                                    let mut updated = inflight_state.clone();
                                                    updated.in_progress = false;
                                                    updated.error = Some(err.clone());

                                                    let mut move_signal_async = move_signal_async.clone();
                                                    move_signal_async.set(Some(updated));

                                                    show_toast(&format!("Fehler beim Verschieben: {}", err));
                                                }
                                            }
                                        }
                                    });
                                }
                            },
                            "Verschieben starten"
                        }
                    }) }
                }
            }
        }
    }
}

// ----- Styles & Helfer -----
fn panel_style() -> &'static str {
    "max-width:1200px;margin:20px auto;padding:16px;background:#0b0c10;color:#e5e7eb;border:1px solid #222533;border-radius:12px;"
}

fn btn_style() -> &'static str {
    "background:#1f2937;color:#e5e7eb;border:1px solid #374151;border-radius:8px;padding:6px 10px;cursor:pointer;"
}

fn btn_danger_style() -> &'static str {
    "background:#7f1d1d;color:#fff;border:1px solid #991b1b;border-radius:8px;padding:6px 10px;cursor:pointer;"
}

fn btn_primary_style() -> &'static str {
    "background:#2563eb;color:#fff;border:none;border-radius:8px;padding:6px 10px;cursor:pointer;"
}

// fn table_style() -> &'static str {
//     "width:100%;border-collapse:collapse;margin-top:8px;background:#0f1117;border:1px solid #222533;border-radius:8px;"
// }

// helper functions (fmt_bytes, copy_to_clipboard, show_toast, download_csv)
// are imported from ui_utils module
