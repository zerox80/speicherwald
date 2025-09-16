use dioxus::prelude::*;
use dioxus_router::prelude::*;
use js_sys::Date;
use web_sys::console;
use std::rc::Rc;

mod api;
mod types;
mod ui_utils;
use ui_utils::{fmt_bytes, fmt_ago_short, copy_to_clipboard, download_csv, trigger_download, show_toast};

// ----- Routing -----
#[derive(Routable, Clone, Debug, PartialEq)]
pub enum Route {
    #[route("/")]
    Home {},
    #[route("/scan/:id")]
    Scan { id: String },
}

pub fn main() {
    console_error_panic_hook::set_once();
    dioxus_web::launch(app);
}

fn app(cx: Scope) -> Element {
    cx.render(rsx! {
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
    })
}

// ----- Home: einfache Scan-Übersicht -----
#[component]
fn Home(cx: Scope) -> Element {
    let scans = use_state(cx, || Vec::<types::ScanSummary>::new());
    let new_root = use_state(cx, || String::new());
    let server_ok = use_state(cx, || None as Option<bool>);
    let drives = use_state(cx, || Vec::<types::DriveInfo>::new());
    let home_loading = use_state(cx, || true);
    let err_scans = use_state(cx, || None as Option<String>);
    let err_drives = use_state(cx, || None as Option<String>);
    let err_health = use_state(cx, || None as Option<String>);

    // initial laden
    {
        let scans = scans.clone();
        let drives_state = drives.clone();
        let server_state = server_ok.clone();
        let loading = home_loading.clone();
        let e_scans = err_scans.clone();
        let e_drives = err_drives.clone();
        let e_health = err_health.clone();
        use_effect(cx, (), move |_| async move {
            loading.set(true);
            match api::list_scans().await { Ok(list) => { scans.set(list); e_scans.set(None); }, Err(e) => e_scans.set(Some(e)) }
            match api::list_drives().await { Ok(dr) => { drives_state.set(dr.items); e_drives.set(None); }, Err(e) => e_drives.set(Some(e)) }
            match api::healthz().await { Ok(ok) => { server_state.set(Some(ok)); e_health.set(None); }, Err(e) => e_health.set(Some(e)) }
            loading.set(false);
        });
    }

    // (removed recent panel effect)

    let reload = {
        let scans = scans.clone();
        let e_scans = err_scans.clone();
        move |_| {
            let scans2 = scans.clone();
            let e2 = e_scans.clone();
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
            let d2 = drives.clone();
            let h2 = server_ok.clone();
            let ed2 = e_drives.clone();
            let eh2 = e_health.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match api::list_drives().await { Ok(dr) => { d2.set(dr.items); ed2.set(None); }, Err(e) => ed2.set(Some(e)) }
                match api::healthz().await { Ok(ok) => { h2.set(Some(ok)); eh2.set(None); }, Err(e) => eh2.set(Some(e)) }
            });
        }
    };

    let nav = use_navigator(cx);
    let start_scan = {
        let root = new_root.clone();
        move |_| {
            let root_val = root.get().trim().to_string();
            if root_val.is_empty() { return; }
            let nav = nav.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let req = api::CreateScanReq {
                    root_paths: vec![root_val],
                    follow_symlinks: None,
                    include_hidden: None,
                    measure_logical: None,
                    measure_allocated: None,
                    excludes: None,
                    max_depth: None,
                    concurrency: None,
                };
                if let Ok(resp) = api::create_scan(&req).await {
                    nav.push(Route::Scan { id: resp.id });
                }
            });
        }
    };

    // vorab: Texte für Dashboard
    let server_text = match *server_ok.get() { Some(true) => "OK", Some(false) => "Fehler", None => "..." };

    cx.render(rsx! {
        section { class: "panel",
            h2 { "SpeicherWald – Scans" }
            // Dashboard: Server-Status & Laufwerke
            div { class: "toolbar", style: "margin-top:6px;",
                span { "Server: {server_text}" }
                span { "Laufwerke: {drives.get().len()}" }
                { (*home_loading.get()).then(|| rsx!(span { class: "spinner", "" })) }
                button { class: "btn", onclick: reload_drives, "Laufwerke aktualisieren" }
            }
            { err_health.get().as_ref().map(|e| rsx!(div { class: "alert alert-error", "Health-Fehler: {e}" })) }
            { err_drives.get().as_ref().map(|e| rsx!(div { class: "alert alert-error", "Laufwerke-Fehler: {e}" })) }
            { err_scans.get().as_ref().map(|e| rsx!(div { class: "alert alert-error", "Scans-Fehler: {e}" })) }
            // Laufwerks-Übersicht
            details { open: true,
                summary { "Laufwerke (Übersicht)" }
                div { style: "display:grid;grid-template-columns:repeat(auto-fill,minmax(320px,1fr));gap:10px;margin-top:8px;",
                    { drives.get().iter().map(|d| {
                        let path = d.path.clone();
                        let used = d.total_bytes.saturating_sub(d.free_bytes);
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
                                    wasm_bindgen_futures::spawn_local(async move {
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
                                        if let Ok(resp) = api::create_scan(&req).await { nav.push(Route::Scan { id: resp.id }); }
                                    });
                                }, "Scan starten" }
                            }
                        } }
                    }) }
                }
            }
            div { class: "input-group",
                input { class: "form-control", value: "{new_root}", placeholder: "Root-Pfad (z. B. C:\\ oder \\server\\share)",
                    oninput: move |e| new_root.set(e.value.clone()) }
                div { class: "input-group-append",
                    button { class: "btn btn-primary", onclick: start_scan, "Scan starten" }
                    button { class: "btn", onclick: reload, "Aktualisieren" }
                }
            }
            ul { class: "list-unstyled",
                { (scans.get().is_empty() && !*home_loading.get()).then(|| rsx!(li { class: "text-muted", "Noch keine Scans." })) }
                { scans.get().iter().map(|s| {
                    let id = s.id.clone();
                    rsx!{ li { style: "margin:6px 0;",
                        Link { to: Route::Scan { id: id.clone() },
                            "{id} – {s.status} – Ordner {s.dir_count} – Dateien {s.file_count} – Allokiert {fmt_bytes(s.total_allocated_size)}" }
                    } }
                }) }
            }
        }
    })
}

// ----- Scan-Detailseite mit Live-Log & Tabellen -----
#[component]
fn Scan(cx: Scope, id: String) -> Element {
    // KPI/Meta und Log
    let kpi = use_state(cx, || None as Option<types::ScanSummary>);
    let log = use_state(cx, || String::new());

    // EventSource-Handle, damit die Verbindung lebt
    let es_ref = use_ref(cx, || None as Option<web_sys::EventSource>);

    // Tabellenzustände
    let tree_items = use_state(cx, || Vec::<types::NodeDto>::new());
    let top_items = use_state(cx, || Vec::<types::TopItem>::new());
    let list_items = use_state(cx, || Vec::<types::ListItem>::new());
    let err_tree = use_state(cx, || None as Option<String>);
    let err_top = use_state(cx, || None as Option<String>);
    let err_list = use_state(cx, || None as Option<String>);
    let loading_tree = use_state(cx, || false);
    let loading_list = use_state(cx, || false);

    // Steuerung für Baum/Top
    let tree_path = use_state(cx, || None as Option<String>);
    let tree_depth = use_state(cx, || 3_i64);
    let tree_limit = use_state(cx, || 200_i64);
    let tree_sort = use_state(cx, || "size".to_string()); // server hint: "size" | "name"
    // Client-side sort controls for Tree table
    let tree_sort_view = use_state(cx, || "allocated".to_string()); // allocated|logical|name|type|accessed
    let tree_order = use_state(cx, || "desc".to_string());
    let top_scope = use_state(cx, || "dirs".to_string()); // "dirs" | "files"
    let top_show = use_state(cx, || 15_usize);
    // Client-side sort controls for Top table
    let top_sort = use_state(cx, || "allocated".to_string()); // allocated|logical|name|type|accessed
    let top_order = use_state(cx, || "desc".to_string());
    // Explorer (Liste) Steuerung
    let list_path = use_state(cx, || None as Option<String>);
    let list_sort = use_state(cx, || "allocated".to_string());
    let list_order = use_state(cx, || "desc".to_string());
    // Default page size reduced for better paging experience
    let list_limit = use_state(cx, || 50_i64);
    let list_offset = use_state(cx, || 0_i64);
    // Pagination helper: track if another next page likely exists (based on last page size)
    let list_has_more = use_state(cx, || true);
    // Sequence ID to drop stale responses when multiple requests overlap
    let list_req_id = use_ref(cx, || 0_i64);
    
    // Filter und Suche
    let search_query = use_state(cx, || String::new());
    let min_size_filter = use_state(cx, || 0_i64);
    let file_type_filter = use_state(cx, || "all".to_string());
    let show_hidden = use_state(cx, || false);
    
    // Navigation History für Breadcrumbs
    let nav_history = use_state(cx, || Vec::<String>::new());

    // Ensure pagination starts from 0 whenever the path changes
    {
        let list_offset0 = list_offset.clone();
        let list_path0 = list_path.clone();
        let nav_hist0 = nav_history.clone();
        use_effect(cx, &list_path0.get().clone(), move |_| async move {
            list_offset0.set(0);
            // Maintain navigation history whenever the current list path changes
            match list_path0.get().clone() {
                Some(p) => {
                    let mut hist = nav_hist0.get().clone();
                    if hist.last().map(|s| s.as_str()) != Some(p.as_str()) {
                        hist.push(p);
                        nav_hist0.set(hist);
                    }
                }
                None => {
                    // Reset history when navigating back to roots
                    nav_hist0.set(Vec::new());
                }
            }
        });
    }

    // Export-Steuerung
    let export_scope = use_state(cx, || "all".to_string()); // all|nodes|files
    let export_limit = use_state(cx, || 10000_i64);

    // Live-Update & Throttle
    let live_update = use_state(cx, || true);
    let last_refresh = use_ref(cx, || 0.0_f64);

    // KPI initial laden
    {
        let id = id.clone();
        let kpi = kpi.clone();
        use_effect(cx, (), move |_| async move { if let Ok(s) = api::get_scan(&id).await { kpi.set(Some(s)); } });
    }

    // Erste Ladung für Tree/Top
    {
        let id0 = id.clone();
        let tree_items0 = tree_items.clone();
        let tree_path0 = tree_path.clone();
        let tree_depth0 = tree_depth.clone();
        let tree_limit0 = tree_limit.clone();
        let tree_sort0 = tree_sort.clone();
        let top_items0 = top_items.clone();
        let top_scope0 = top_scope.clone();
        let e_tree0 = err_tree.clone();
        let e_top0 = err_top.clone();
        let l_tree0 = loading_tree.clone();
        use_effect(cx, (), move |_| async move {
            l_tree0.set(true);
            let tq = api::TreeQuery { path: tree_path0.get().clone(), depth: Some(*tree_depth0.get()), sort: Some(tree_sort0.get().clone()), limit: Some(*tree_limit0.get()) };
            match api::get_tree(&id0, &tq).await { Ok(list) => { tree_items0.set(list); e_tree0.set(None); }, Err(e) => e_tree0.set(Some(e)) }
            l_tree0.set(false);
            let qq = api::TopQuery { scope: Some(top_scope0.get().clone()), limit: Some(100) };
            match api::get_top(&id0, &qq).await { Ok(list) => { top_items0.set(list); e_top0.set(None); }, Err(e) => e_top0.set(Some(e)) }
        });
    }

    // Initial-Ladung für Explorer (Liste)
    {
        let id0 = id.clone();
        let list_items0 = list_items.clone();
        let list_path0 = list_path.clone();
        let list_sort0 = list_sort.clone();
        let list_order0 = list_order.clone();
        let list_limit0 = list_limit.clone();
        let list_offset0 = list_offset.clone();
        let list_has_more0 = list_has_more.clone();
        use_effect(cx, (), move |_| async move {
            let page_limit = *list_limit0.get();
            let lq = api::ListQuery {
                path: list_path0.get().clone(),
                sort: Some(list_sort0.get().clone()),
                order: Some(list_order0.get().clone()),
                limit: Some(page_limit + 1), // fetch one extra to detect if there is a next page
                offset: Some(*list_offset0.get()),
            };
            if let Ok(list) = api::get_list(&id0, &lq).await {
                let has_more = (list.len() as i64) > page_limit;
                let items_page: Vec<types::ListItem> = list.into_iter().take(page_limit as usize).collect();
                list_has_more0.set(has_more);
                list_items0.set(items_page);
            }
        });
    }

    // Auto-Reload Explorer (Liste), sobald relevante Zustände geändert werden
    // Fix für den "2x klicken"-Effekt: Wir laden nun automatisch, nachdem z. B. list_path gesetzt wurde.
    {
        let id0 = id.clone();
        let list_items0 = list_items.clone();
        let list_path0 = list_path.clone();
        let list_sort0 = list_sort.clone();
        let list_order0 = list_order.clone();
        let list_limit0 = list_limit.clone();
        let list_offset0 = list_offset.clone();
        let e_list0 = err_list.clone();
        let l_list0 = loading_list.clone();
        let list_has_more0 = list_has_more.clone();
        let req_ref0 = list_req_id.clone();
        use_effect(
            cx,
            &(
                list_path0.get().clone(),
                list_sort0.get().clone(),
                list_order0.get().clone(),
                *list_limit0.get(),
                *list_offset0.get(),
            ),
            move |_| async move {
                // Bump request sequence and capture this request's id
                let my_id = req_ref0.with_mut(|rid| { *rid += 1; *rid });
                l_list0.set(true);
                let page_limit = *list_limit0.get();
                let lq = api::ListQuery {
                    path: list_path0.get().clone(),
                    sort: Some(list_sort0.get().clone()),
                    order: Some(list_order0.get().clone()),
                    limit: Some(page_limit + 1),
                    offset: Some(*list_offset0.get()),
                };
                match api::get_list(&id0, &lq).await {
                    Ok(list) => {
                        let has_more = (list.len() as i64) > page_limit;
                        let items_page: Vec<types::ListItem> = list.into_iter().take(page_limit as usize).collect();
                        // Only apply if this is still the latest request
                        let is_latest = req_ref0.with(|rid| my_id == *rid);
                        if is_latest {
                            // If we went beyond the last page, step back automatically and show a hint
                            if items_page.is_empty() && *list_offset0.get() > 0 {
                                show_toast("Keine weitere Seite");
                                let back_off = (*list_offset0.get() - page_limit).max(0);
                                list_offset0.set(back_off);
                                // Leave loading true; the next effect run will clear it
                            } else {
                                list_has_more0.set(has_more);
                                list_items0.set(items_page);
                                e_list0.set(None);
                                l_list0.set(false);
                            }
                        }
                    }
                    Err(e) => {
                        // Apply error only if latest; also clear loading
                        let is_latest = req_ref0.with(|rid| my_id == *rid);
                        if is_latest {
                            e_list0.set(Some(e));
                            l_list0.set(false);
                        }
                    },
                }
            },
        );
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
            let q_path = tree_path_state.get().clone();
            let q_depth = *tree_depth_state.get();
            let q_limit = *tree_limit_state.get();
            let q_sort = tree_sort_state.get().clone();
            let e2 = e_tree.clone();
            let l2 = l_tree.clone();
            l2.set(true);
            wasm_bindgen_futures::spawn_local(async move {
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
            let q_path = list_path_state.get().clone();
            let q_sort = list_sort_state.get().clone();
            let q_order = list_order_state.get().clone();
            let q_limit = *list_limit_state.get();
            let q_offset = *list_offset_state.get();
            let e2 = e_list.clone();
            let l2 = l_list.clone();
            // Start a new request and track sequence id
            let my_id = req_ref.with_mut(|rid| { *rid += 1; *rid });
            l2.set(true);
            // Clone state handles for use inside async block to avoid moving from the outer closure (keeps Fn instead of FnOnce)
            let has_more2 = list_has_more_state.clone();
            // Clone the request ref handle for use inside the async block (avoid moving the captured variable)
            let req_ref_async = req_ref.clone();
            wasm_bindgen_futures::spawn_local(async move {
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
        let es_ref2 = es_ref.clone();
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
        let last_h = last_refresh.clone();
        use_effect(cx, (), move |_| async move {
            // separate state handles to avoid move-after-move issues
            let log_state_in = log_state.clone();
            let log_state_err = log_state.clone();
            match api::sse_attach(&id_for_sse, move |ev| {
                let mut newlog = log_state_in.get().clone();
                match &ev {
                    types::ScanEvent::Started{ root_paths } => newlog.push_str(&format!("Started: {}\n", root_paths.join(", "))),
                    types::ScanEvent::Progress{ current_path, dirs_scanned, files_scanned, allocated_size, .. } => newlog.push_str(&format!("Progress: {} | dirs={} files={} alloc={}\n", current_path, dirs_scanned, files_scanned, fmt_bytes(*allocated_size as i64))),
                    types::ScanEvent::Warning{ path, code, message } => newlog.push_str(&format!("Warning: {} ({}) : {}\n", path, code, message)),
                    types::ScanEvent::Done{ .. } => newlog.push_str("Done\n"),
                    types::ScanEvent::Cancelled => newlog.push_str("Cancelled\n"),
                    types::ScanEvent::Failed{ message } => newlog.push_str(&format!("Failed: {}\n", message)),
                }
                log_state_in.set(newlog);

                // Wenn Scan fertig ist und noch kein Pfad gewählt wurde, automatisch ersten Root setzen
                if let types::ScanEvent::Done { .. } = ev {
                    if list_path_h.get().is_none() {
                        let id_aut = id_for_cb.clone();
                        let lp_state = list_path_h.clone();
                        let list_items2 = list_items_h.clone();
                        let sort_state = list_sort_h.clone();
                        let order_state = list_order_h.clone();
                        let limit_state = list_limit_h.clone();
                        let nav_hist_state = nav_hist_h.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let q_roots = api::ListQuery {
                                path: None,
                                sort: Some(sort_state.get().clone()),
                                order: Some(order_state.get().clone()),
                                limit: Some(*limit_state.get()),
                                offset: Some(0),
                            };
                            if let Ok(list) = api::get_list(&id_aut, &q_roots).await {
                                list_items2.set(list.clone());
                                if lp_state.get().is_none() {
                                    if let Some(first_root) = list.iter().find_map(|it| match it {
                                        types::ListItem::Dir { path, .. } => Some(path.clone()),
                                        _ => None,
                                    }) {
                                        lp_state.set(Some(first_root.clone()));
                                        nav_hist_state.set(vec![first_root.clone()]);
                                        // Optional: direkt Kinder des ersten Roots laden
                                        let id_list2 = id_aut.clone();
                                        let list_items3 = list_items2.clone();
                                        wasm_bindgen_futures::spawn_local(async move {
                                            let q_child = api::ListQuery { path: Some(first_root), sort: Some("allocated".into()), order: Some("desc".into()), limit: Some(500), offset: Some(0) };
                                            if let Ok(list2) = api::get_list(&id_list2, &q_child).await { list_items3.set(list2); }
                                        });
                                    }
                                }
                            }
                        });
                    }
                }

                // KPI aktualisieren
                let id2 = id_for_cb.clone();
                let kpi2 = kpi.clone();
                wasm_bindgen_futures::spawn_local(async move { if let Ok(s) = api::get_scan(&id2).await { kpi2.set(Some(s)); } });

                // Gedrosselte Reloads
                if *live_h.get() {
                    let now = Date::now();
                    let mut should = false;
                    // Reduce auto-refresh frequency from 1s to 5s to avoid rate limiting
                    last_h.with_mut(|last| { if now - *last > 5000.0 { *last = now; should = true; } });
                    if should {
                        // Top
                        let id_top = id_for_cb.clone();
                        let top_items2 = top_items_h.clone();
                        let scope = top_scope_h.get().clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let q = api::TopQuery { scope: Some(scope), limit: Some(100) };
                            if let Ok(list) = api::get_top(&id_top, &q).await { top_items2.set(list); }
                        });
                        // Tree
                        let id_tree = id_for_cb.clone();
                        let tree_items2 = tree_items_h.clone();
                        let q_path = tree_path_h.get().clone();
                        let q_depth = *tree_depth_h.get();
                        let q_limit = *tree_limit_h.get();
                        let q_sort = tree_sort_h.get().clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let q = api::TreeQuery { path: q_path, depth: Some(q_depth), sort: Some(q_sort), limit: Some(q_limit) };
                            if let Ok(list) = api::get_tree(&id_tree, &q).await { tree_items2.set(list); }
                        });
                        // Liste (Explorer)
                        // Skip auto-refresh for list if a manual/other load is in-flight
                        if !*loading_list_h.get() {
                            let id_list = id_for_cb.clone();
                            let list_items2 = list_items_h.clone();
                            let has_more2 = list_has_more_h.clone();
                            let q_path_l = list_path_h.get().clone();
                            let q_sort_l = list_sort_h.get().clone();
                            let q_order_l = list_order_h.get().clone();
                            let q_limit_l = *list_limit_h.get();
                            let q_offset_l = *list_offset_h.get();
                            wasm_bindgen_futures::spawn_local(async move {
                                let q = api::ListQuery { path: q_path_l, sort: Some(q_sort_l), order: Some(q_order_l), limit: Some(q_limit_l + 1), offset: Some(q_offset_l) };
                                if let Ok(list) = api::get_list(&id_list, &q).await {
                                    let has_more = (list.len() as i64) > q_limit_l;
                                    let items_page: Vec<types::ListItem> = list.into_iter().take(q_limit_l as usize).collect();
                                    has_more2.set(has_more);
                                    list_items2.set(items_page);
                                }
                            });
                        }
                    }
                }
            }) {
                Ok(es) => { es_ref2.with_mut(|slot| *slot = Some(es)); }
                Err(e) => {
                    let mut newlog = log_state_err.get().clone();
                    newlog.push_str(&format!("SSE Fehler: {}\n", e));
                    log_state_err.set(newlog);
                }
            }
        });
    }

    // Cancel/Purge
    let nav = use_navigator(cx);
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
        move |_| { let n = *top_show.get(); let m = (n + 10).min(100); top_show.set(m); }
    };
    let top_less = {
        let top_show = top_show.clone();
        move |_| { let n = *top_show.get(); let m = if n > 10 { n - 10 } else { 5 }; top_show.set(m); }
    };
    // Tree Komfort-Buttons
    let more_tree = {
        let tree_limit = tree_limit.clone();
        let do_btn = do_load_tree.clone();
        move |_| { tree_limit.set(*tree_limit.get() + 200); (do_btn.as_ref())(); }
    };
    let less_tree = {
        let tree_limit = tree_limit.clone();
        let do_btn = do_load_tree.clone();
        move |_| { let v = (*tree_limit.get() - 200).max(10); tree_limit.set(v); (do_btn.as_ref())(); }
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
            let new_off = *list_offset.get() + *list_limit.get();
            console::log_1(&format!("Next click: offset {} -> {} (limit {}), path={:?}", *list_offset.get(), new_off, *list_limit.get(), list_path_dbg.get().clone()).into());
            list_offset.set(new_off);
            // Trigger immediate reload for snappier UX
            (do_btn.as_ref())();
        }
    };
    let max_alloc_bar: i64 = top_items
        .get()
        .iter()
        .map(|it| match it {
            types::TopItem::Dir { allocated_size, .. } => *allocated_size,
            types::TopItem::File { allocated_size, .. } => *allocated_size,
        })
        .max()
        .unwrap_or(0);
    let max_alloc_list: i64 = list_items
        .get()
        .iter()
        .map(|it| match it {
            types::ListItem::Dir { allocated_size, .. } => *allocated_size,
            types::ListItem::File { allocated_size, .. } => *allocated_size,
        })
        .max()
        .unwrap_or(0);
    let max_alloc_tree: i64 = tree_items
        .get()
        .iter()
        .map(|n| n.allocated_size)
        .max()
        .unwrap_or(0);
    cx.render(rsx! {
        section { class: "panel",
            h2 { "Scan {id}" }
            div { style: "color:#a0aec0;margin:4px 0 8px 0;", "Status: {kpi.get().as_ref().map(|s| s.status.clone()).unwrap_or_else(|| \"...\".into())}" }
            div { style: "display:flex;gap:12px;flex-wrap:wrap;",
                button { class: "btn", onclick: cancel, "Abbrechen" }
                button { class: "btn btn-danger", onclick: purge, "Purge" }
            }
            // Export-Bereich
            details { open: true,
                summary { "Export" }
                div { style: "display:flex;gap:10px;align-items:center;flex-wrap:wrap;margin:8px 0;",
                    span { "Scope:" }
                    select { value: "{export_scope}", oninput: move |e| export_scope.set(e.value.clone()),
                        option { value: "all", "All" }
                        option { value: "nodes", "Nodes" }
                        option { value: "files", "Files" }
                    }
                    span { "Limit:" }
                    input { r#type: "number", min: "1", value: "{export_limit}", oninput: move |e| { if let Ok(v) = e.value.parse::<i64>() { export_limit.set(v.max(1)); } } }
                    // Download Buttons
                    button { class: "btn", onclick: {
                        let id_csv = id.clone();
                        let scope = export_scope.clone();
                        let limit = export_limit.clone();
                        move |_| {
                            let url = format!("/scans/{}/export?format=csv&scope={}&limit={}", id_csv, scope.get(), *limit.get());
                            trigger_download(&url, Some(&format!("scan_{}.csv", id_csv)));
                        }
                    }, "CSV" }
                    button { class: "btn", onclick: {
                        let id_json = id.clone();
                        let scope = export_scope.clone();
                        let limit = export_limit.clone();
                        move |_| {
                            let url = format!("/scans/{}/export?format=json&scope={}&limit={}", id_json, scope.get(), *limit.get());
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
            { (!nav_history.get().is_empty()).then(|| rsx!{
                div { class: "breadcrumbs",
                    span { class: "text-muted", "Navigationspfad:" }
                    { nav_history.get().iter().enumerate().map(|(i, path)| {
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
                                        tree_path_nav.set(new_path.clone());
                                        list_path_nav.set(new_path.clone());
                                        nav_hist.set(nav_hist.get()[..=i].to_vec());
                                        (do_nav.as_ref())();
                                    },
                                    "{path}"
                                }
                            }
                        }
                    }) }
                    button {
                        onclick: move |_| {
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
                input { value: "{tree_path.as_ref().cloned().unwrap_or_default()}", placeholder: "leer = alle Wurzeln",
                    oninput: move |e| tree_path.set(if e.value.is_empty() { None } else { Some(e.value.clone()) }) }
                span { "Tiefe:" }
                input { r#type: "number", min: "1", value: "{tree_depth}", oninput: move |e| { if let Ok(v) = e.value.parse::<i64>() { tree_depth.set(v.max(1)); } } }
                span { "Sort:" }
                select { value: "{tree_sort}", oninput: move |e| tree_sort.set(e.value.clone()),
                    option { value: "size", "Größe" }
                    option { value: "name", "Name" }
                }
                span { "Limit:" }
                input { r#type: "number", min: "10", value: "{tree_limit}", oninput: move |e| { if let Ok(v) = e.value.parse::<i64>() { tree_limit.set(v.max(10)); } } }
                button { class: "btn", onclick: more_tree, "Mehr" }
                button { class: "btn", onclick: less_tree, "Weniger" }
                label { style: "display:flex;gap:6px;align-items:center;", input { r#type: "checkbox", checked: *live_update.get(), oninput: move |_| live_update.set(!*live_update.get()) } " Live-Update Tabellen" }
                span { "Einträge: {tree_items.len()}" }
                { (*loading_tree.get()).then(|| rsx!(span { class: "spinner", "" })) }
                { err_tree.get().as_ref().map(|e| rsx!(span { class: "text-danger", " Fehler: {e}" })) }
            }
            // Top-N Bereich + Visuelle Übersicht
            div { style: "margin-top:8px;display:flex;gap:12px;align-items:center;flex-wrap:wrap;",
                span { "Top-N:" }
                select { value: "{top_scope}", oninput: move |e| {
                        let val = e.value.clone();
                        top_scope.set(val.clone());
                        let top_show2 = top_show.clone();
                        top_show2.set(15);
                        let id_top = id.clone();
                        let top_items2 = top_items.clone();
                        wasm_bindgen_futures::spawn_local(async move {
                            let q = api::TopQuery { scope: Some(val), limit: Some(100) };
                            if let Ok(list) = api::get_top(&id_top, &q).await { top_items2.set(list); }
                        });
                    },
                    option { value: "dirs", "Ordner" }
                    option { value: "files", "Dateien" }
                }
                button { style: btn_style(), onclick: top_less, "Weniger" }
                button { style: btn_style(), onclick: top_more, "Mehr" }
                button { style: btn_style(), onclick: {
                        let top_items = top_items.clone();
                        let top_show = top_show.clone();
                        let id_csv = id.clone();
                        move |_| {
                            let mut csv = String::from("type,path,allocated,logical,depth,file_count,dir_count\n");
                            for it in top_items.get().iter().take(*top_show.get()) {
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
                { err_top.get().as_ref().map(|e| rsx!(span { style: "color:#f87171;", " Fehler: {e}" })) }
            }
            // Visuelle Übersicht (Top-N Balken)
            div { style: "margin-top:8px;",
                { top_items.get().iter().take(*top_show.get()).map(|it| {
                    let nav_h = nav_history.clone();
                    let (label, alloc) = match it {
                        types::TopItem::Dir { path, allocated_size, .. } => (path.clone(), *allocated_size),
                        types::TopItem::File { path, allocated_size, .. } => (path.clone(), *allocated_size),
                    };
                    let mut blocks: usize = 1;
                    if max_alloc_bar > 0 { blocks = (((alloc as f64) / (max_alloc_bar as f64) * 40.0).round() as usize).max(1); }
                    let bar = "█".repeat(blocks);
                    rsx!{ div { style: "display:flex;gap:10px;align-items:center;font-family:monospace;",
                        span { style: "min-width:80px;color:#a0aec0;", "{fmt_bytes(alloc)}" }
                        pre { style: "margin:0;line-height:1.1;color:#60a5fa;cursor:pointer;", onclick: move |_| { 
                            list_path.set(Some(label.clone())); 
                            let mut hist = nav_h.get().clone();
                            if !hist.contains(&label) { hist.push(label.clone()); }
                            nav_h.set(hist);
                        }, "{bar}" }
                        span { style: "color:#93c5fd;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:480px;", "{label}" }
                    } }
                }) }
            }
            // Top-Tabelle
            table { style: table_style(),
                thead { tr {
                    th { style: "text-align:left;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "type".to_string();
                        if *top_sort.get() == key { top_order.set(if *top_order.get() == "desc" { "asc".into() } else { "desc".into() }); } else { top_sort.set(key); top_order.set("desc".into()); }
                    }, "Typ" }
                    th { style: "text-align:left;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "accessed".to_string();
                        if *top_sort.get() == key { top_order.set(if *top_order.get() == "desc" { "asc".into() } else { "desc".into() }); } else { top_sort.set(key); top_order.set("desc".into()); }
                    }, "Zuletzt" }
                    th { style: "text-align:right;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "allocated".to_string();
                        if *top_sort.get() == key { top_order.set(if *top_order.get() == "desc" { "asc".into() } else { "desc".into() }); } else { top_sort.set(key); top_order.set("desc".into()); }
                    }, "Allokiert" }
                    th { style: "text-align:right;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "logical".to_string();
                        if *top_sort.get() == key { top_order.set(if *top_order.get() == "desc" { "asc".into() } else { "desc".into() }); } else { top_sort.set(key); top_order.set("desc".into()); }
                    }, "Logisch" }
                    th { style: "text-align:left;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "name".to_string();
                        if *top_sort.get() == key { top_order.set(if *top_order.get() == "desc" { "asc".into() } else { "desc".into() }); } else { top_sort.set(key); top_order.set("desc".into()); }
                    }, "Pfad" }
                    th { style: "text-align:left;padding:6px;border-bottom:1px solid #222533;", "Aktionen" }
                } }
                tbody {
                    {
                        let mut rows = top_items.get().clone();
                        // Sort key
                        rows.sort_by_key(|it| match it {
                            types::TopItem::Dir { allocated_size, logical_size, path, atime, .. } => match top_sort.get().as_str() {
                                "logical" => *logical_size,
                                "name" => 0,
                                "type" => 0,
                                "accessed" => atime.unwrap_or(0),
                                _ => *allocated_size,
                            },
                            types::TopItem::File { allocated_size, logical_size, path, atime, .. } => match top_sort.get().as_str() {
                                "logical" => *logical_size,
                                "name" => 0,
                                "type" => 1,
                                "accessed" => atime.unwrap_or(0),
                                _ => *allocated_size,
                            },
                        });
                        if *top_sort.get() == "name" {
                            rows.sort_by_key(|it| match it { types::TopItem::Dir { path, .. } | types::TopItem::File { path, .. } => path.to_lowercase() });
                        }
                        if *top_sort.get() == "type" {
                            rows.sort_by_key(|it| match it { types::TopItem::Dir { .. } => 0, _ => 1 });
                        }
                        if *top_order.get() == "desc" { rows.reverse(); }
                        rows.into_iter().map(|it| {
                            match it {
                                types::TopItem::Dir { path, allocated_size, logical_size, atime, .. } => {
                                    let p_nav = path.clone();
                                    let p_copy = path.clone();
                                    rsx!{ tr {
                                        td { style: "padding:6px;border-bottom:1px solid #1b1e2a;", "Ordner" }
                                        td { style: "padding:6px;border-bottom:1px solid #1b1e2a;", "{fmt_ago_short(atime)}" }
                                        td { style: "padding:6px;text-align:right;border-bottom:1px solid #1b1e2a;", "{fmt_bytes(allocated_size)}" }
                                        td { style: "padding:6px;text-align:right;border-bottom:1px solid #1b1e2a;", "{fmt_bytes(logical_size)}" }
                                        td { style: "padding:6px;border-bottom:1px solid #1b1e2a;cursor:pointer;color:#9cdcfe;", onclick: move |_| { 
                                            list_path.set(Some(p_nav.clone())); 
                                            let mut hist = nav_history.get().clone();
                                            if !hist.contains(&p_nav) { hist.push(p_nav.clone()); }
                                            nav_history.set(hist);
                                        }, "{path}" }
                                        td { style: "padding:6px;border-bottom:1px solid #1b1e2a;",
                                            button { style: btn_style(), onclick: move |_| { copy_to_clipboard(p_copy.clone()); }, "Kopieren" }
                                        }
                                    } }
                                },
                                types::TopItem::File { path, allocated_size, logical_size, atime, .. } => rsx!{ tr {
                                    td { style: "padding:6px;border-bottom:1px solid #1b1e2a;", "Datei" }
                                    td { style: "padding:6px;border-bottom:1px solid #1b1e2a;", "{fmt_ago_short(atime)}" }
                                    td { style: "padding:6px;text-align:right;border-bottom:1px solid #1b1e2a;", "{fmt_bytes(allocated_size)}" }
                                    td { style: "padding:6px;text-align:right;border-bottom:1px solid #1b1e2a;", "{fmt_bytes(logical_size)}" }
                                    td { style: "padding:6px;border-bottom:1px solid #1b1e2a;", "{path}" }
                                    td { style: "padding:6px;border-bottom:1px solid #1b1e2a;",
                                        button { style: btn_style(), onclick: move |_| { copy_to_clipboard(path.clone()); }, "Kopieren" }
                                    }
                                } },
                            }
                        })
                    }
                }
            }  // table close
            // (removed recent panel UI)
            // Baum-Ergebnisse (Detail-Liste)  
            h3 { style: "margin-top:16px;", "Baum – Ergebnisse" }
            table { style: table_style(),
                thead { tr {
                    th { style: "text-align:left;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "type".to_string();
                        if *tree_sort_view.get() == key { tree_order.set(if *tree_order.get() == "desc" { "asc".into() } else { "desc".into() }); } else { tree_sort_view.set(key); tree_order.set("desc".into()); }
                    }, "Typ" }
                    th { style: "text-align:left;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "accessed".to_string();
                        if *tree_sort_view.get() == key { tree_order.set(if *tree_order.get() == "desc" { "asc".into() } else { "desc".into() }); } else { tree_sort_view.set(key); tree_order.set("desc".into()); }
                    }, "Zuletzt" }
                    th { style: "text-align:right;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "allocated".to_string();
                        if *tree_sort_view.get() == key { tree_order.set(if *tree_order.get() == "desc" { "asc".into() } else { "desc".into() }); } else { tree_sort_view.set(key); tree_order.set("desc".into()); }
                    }, "Allokiert" }
                    th { style: "text-align:right;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "logical".to_string();
                        if *tree_sort_view.get() == key { tree_order.set(if *tree_order.get() == "desc" { "asc".into() } else { "desc".into() }); } else { tree_sort_view.set(key); tree_order.set("desc".into()); }
                    }, "Logisch" }
                    th { style: "text-align:left;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "name".to_string();
                        if *tree_sort_view.get() == key { tree_order.set(if *tree_order.get() == "desc" { "asc".into() } else { "desc".into() }); } else { tree_sort_view.set(key); tree_order.set("desc".into()); }
                    }, "Pfad" }
                    th { style: "text-align:left;padding:6px;border-bottom:1px solid #222533;", "Visual" }
                    th { style: "text-align:left;padding:6px;border-bottom:1px solid #222533;", "Aktionen" }
                } }
                tbody {
                    { let mut rows = tree_items.get().clone();
                      rows.sort_by_key(|n| match tree_sort_view.get().as_str() {
                          "logical" => n.logical_size,
                          "name" => 0,
                          "type" => if n.is_dir { 0 } else { 1 },
                          "accessed" => n.atime.unwrap_or(0),
                          _ => n.allocated_size,
                      });
                      if *tree_sort_view.get() == "name" { rows.sort_by_key(|n| n.path.to_lowercase()); }
                      if *tree_order.get() == "desc" { rows.reverse(); }
                      rows.into_iter().map(|n| {
                        let t = if n.is_dir { "Ordner" } else { "Datei" };
                        let alloc = n.allocated_size; let logical = n.logical_size; let p = n.path.clone();
                        let percent = if max_alloc_tree > 0 { ((alloc as f64) / (max_alloc_tree as f64) * 100.0).clamp(1.0, 100.0) } else { 0.0 };
                        let bar_width = format!("width:{:.1}%;", percent);
                        let p_nav = p.clone();
                        let p_copy = p.clone();
                        let bar_class = if n.is_dir { "bar-fill-indigo" } else { "bar-fill-green" };
                        rsx!{ tr {
                            td { style: "padding:6px;border-bottom:1px solid #1b1e2a;", "{t}" }
                            td { style: "padding:6px;border-bottom:1px solid #1b1e2a;", "{fmt_ago_short(n.atime)}" }
                            td { style: "padding:6px;text-align:right;border-bottom:1px solid #1b1e2a;", "{fmt_bytes(alloc)}" }
                            td { style: "padding:6px;text-align:right;border-bottom:1px solid #1b1e2a;", "{fmt_bytes(logical)}" }
                            td { style: "padding:6px;border-bottom:1px solid #1b1e2a;cursor:pointer;color:#9cdcfe;", onclick: move |_| { 
                                list_path.set(Some(p_nav.clone())); 
                                let mut hist = nav_history.get().clone();
                                if !hist.contains(&p_nav) { hist.push(p_nav.clone()); }
                                nav_history.set(hist);
                            }, "{p}" }
                            td { style: "padding:6px;border-bottom:1px solid #1b1e2a;min-width:160px;",
                                div { class: "bar-shell",
                                    div { class: "{bar_class}", style: "{bar_width}" }
                                }
                            }
                            td { style: "padding:6px;border-bottom:1px solid #1b1e2a;",
                                button { class: "btn", onclick: move |_| { copy_to_clipboard(p_copy.clone()); }, "Kopieren" }
                            }
                        } }
                    }) }
                }
            }

            // Explorer (Liste) – zeigt Kinder des aktuellen Pfads mit visuellen Größen-Balken
            div { style: "margin-top:16px;",
                div { style: "display:flex;gap:12px;align-items:center;flex-wrap:wrap;",
                    h3 { style: "margin:0 12px 0 0;", "Explorer (Liste)" }
                    button { class: "btn", disabled: *loading_list.get(), onclick: {
                        let f = do_load_list_btn.clone();
                        move |_| (f.as_ref())()
                    }, "Kinder laden" }
                    span { "Pfad:" }
                    input { value: "{list_path.as_ref().cloned().unwrap_or_default()}", placeholder: "leer = Wurzeln",
                        oninput: move |e| { list_path.set(if e.value.is_empty() { None } else { Some(e.value.clone()) }); list_offset.set(0); } }
                    span { "Sort:" }
                    select { value: "{list_sort}", oninput: move |e| { list_sort.set(e.value.clone()); list_offset.set(0); },
                        option { value: "allocated", "Allokiert" }
                        option { value: "logical", "Logisch" }
                        option { value: "name", "Name" }
                        option { value: "type", "Typ" }
                        option { value: "modified", "Änderungsdatum" }
                    }
                    span { "Reihenfolge:" }
                    select { value: "{list_order}", oninput: move |e| { list_order.set(e.value.clone()); list_offset.set(0); },
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
                            value: "{search_query}", 
                            placeholder: "Datei/Ordner suchen...",
                            style: "background:#1f2937;color:#e5e7eb;border:1px solid #374151;border-radius:6px;padding:4px 8px;",
                            oninput: move |e| search_query.set(e.value.clone())
                        }
                        span { "Min. Größe:" }
                        input { 
                            r#type: "number", 
                            min: "0", 
                            value: "{min_size_filter}",
                            style: "background:#1f2937;color:#e5e7eb;border:1px solid #374151;border-radius:6px;padding:4px 8px;width:120px;",
                            oninput: move |e| { if let Ok(v) = e.value.parse::<i64>() { min_size_filter.set(v.max(0)); } }
                        }
                        select { 
                            style: "background:#1f2937;color:#e5e7eb;border:1px solid #374151;border-radius:6px;padding:4px 8px;",
                            oninput: move |e| {
                                let val = e.value.clone();
                                match val.as_str() {
                                    "mb" => min_size_filter.set(*min_size_filter.get() * 1024 * 1024),
                                    "gb" => min_size_filter.set(*min_size_filter.get() * 1024 * 1024 * 1024),
                                    _ => {}
                                }
                            },
                            option { value: "b", "Bytes" }
                            option { value: "mb", "→ MB" }
                            option { value: "gb", "→ GB" }
                        }
                        span { "Typ:" }
                        select { 
                            value: "{file_type_filter}",
                            style: "background:#1f2937;color:#e5e7eb;border:1px solid #374151;border-radius:6px;padding:4px 8px;",
                            oninput: move |e| file_type_filter.set(e.value.clone()),
                            option { value: "all", "Alle" }
                            option { value: "dirs", "Nur Ordner" }
                            option { value: "files", "Nur Dateien" }
                        }
                        label { style: "display:flex;gap:6px;align-items:center;",
                            input { 
                                r#type: "checkbox", 
                                checked: *show_hidden.get(),
                                oninput: move |_| show_hidden.set(!*show_hidden.get())
                            }
                            "Versteckte anzeigen"
                        }
                        button { 
                            style: "background:#2563eb;color:#fff;border:none;border-radius:6px;padding:6px 12px;cursor:pointer;",
                            onclick: move |_| {
                                search_query.set(String::new());
                                min_size_filter.set(0);
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
                    input { r#type: "number", min: "10", value: "{list_limit}", oninput: move |e| { if let Ok(v) = e.value.parse::<i64>() { list_limit.set(v.max(10)); list_offset.set(0); } } }
                    // quick set buttons for common page sizes
                    button { class: "btn", style: "padding:4px 8px;", onclick: {
                        let list_limit = list_limit.clone(); let list_offset = list_offset.clone();
                        move |_| { list_limit.set(50); list_offset.set(0); }
                    }, "50" }
                    button { class: "btn", style: "padding:4px 8px;", onclick: {
                        let list_limit = list_limit.clone(); let list_offset = list_offset.clone();
                        move |_| { list_limit.set(100); list_offset.set(0); }
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
                            if *list_offset.get() <= 0 {
                                // On first page: step back in navigation history if available, otherwise compute parent path
                                let mut hist = nav_hist.get().clone();
                                if hist.is_empty() {
                                    // Try compute parent path from current list_path
                                    if let Some(cur) = list_path.get().clone() {
                                        let mut s = cur.trim_end_matches(['\\','/']).to_string();
                                        let mut cut: Option<usize> = None;
                                        for (i, ch) in s.char_indices().rev() { if ch == '\\' || ch == '/' { cut = Some(i); break; } }
                                        let parent = cut.map(|i| s[..i].to_string());
                                        if let Some(par) = parent.filter(|v| !v.is_empty() && !v.ends_with(':') && v.len() > 2) {
                                            nav_hist.set(vec![par.clone()]);
                                            list_path.set(Some(par));
                                            list_offset.set(0);
                                            (do_btn.as_ref())();
                                            show_toast("Zurück");
                                            console::log_1(&"Prev click: computed parent".into());
                                        } else {
                                            // No parent left → roots
                                            nav_hist.set(Vec::new());
                                            list_path.set(None);
                                            list_offset.set(0);
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
                                    nav_hist.set(hist);
                                    list_path.set(target);
                                    list_offset.set(0);
                                    (do_btn.as_ref())();
                                    show_toast("Zurück");
                                    console::log_1(&"Prev click: history back".into());
                                }
                            } else {
                                let old_off = *list_offset.get();
                                let old_page = (old_off / *list_limit.get()) + 1;
                                let new_off = (old_off - *list_limit.get()).max(0);
                                if new_off < *list_offset.get() { list_has_more.set(true); }
                                console::log_1(&format!("Prev click: offset {} -> {} (limit {}), path={:?}", old_off, new_off, *list_limit.get(), list_path.get().clone()).into());
                                list_offset.set(new_off);
                                // Trigger immediate reload for snappier UX
                                (do_btn.as_ref())();
                                let new_page = (new_off / *list_limit.get()) + 1;
                                let msg = format!("Seite {} → {}", old_page, new_page);
                                show_toast(&msg);
                            }
                        }
                    }, "Vorherige Seite" }
                    // Allow trying to load the next page even if `has_more` is currently false.
                    // The effect re-fetch will update `list_has_more` and item list accordingly.
                    button { class: "btn btn-primary", r#type: "button", style: btn_primary_style(), disabled: *loading_list.get(), title: if *loading_list.get() { "Laden läuft…" } else { "Nächste Seite laden" }, onclick: next_page, "Nächste Seite" }
                    span { "Seite: {(*list_offset.get() / *list_limit.get()) + 1} (Offset: {*list_offset.get()})" }
                    span { "Einträge (Seite): {list_items.len()}" }
                    button { class: "btn", onclick: {
                            let list_items = list_items.clone();
                            let search_query = search_query.clone();
                            let min_size_filter = min_size_filter.clone();
                            let file_type_filter = file_type_filter.clone();
                            let show_hidden = show_hidden.clone();
                            move |_| {
                                let mut csv = String::from("type,name,path,allocated,logical,mtime\n");
                                for it in list_items.get().iter().filter(|it| {
                                    let query = search_query.get().to_lowercase();
                                    let name_match = if query.is_empty() { true } else { match it { types::ListItem::Dir { name, .. } => name.to_lowercase().contains(&query), types::ListItem::File { name, .. } => name.to_lowercase().contains(&query), } };
                                    let size_match = match it { types::ListItem::Dir { allocated_size, .. } => *allocated_size >= *min_size_filter.get(), types::ListItem::File { allocated_size, .. } => *allocated_size >= *min_size_filter.get(), };
                                    let type_match = match file_type_filter.get().as_str() { "dirs" => matches!(it, types::ListItem::Dir { .. }), "files" => matches!(it, types::ListItem::File { .. }), _ => true };
                                    let hidden_match = if !*show_hidden.get() { match it { types::ListItem::Dir { name, .. } => !name.starts_with('.'), types::ListItem::File { name, .. } => !name.starts_with('.'), } } else { true };
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
                    { (*loading_list.get()).then(|| rsx!(span { class: "spinner", "" })) }
                    { err_list.get().as_ref().map(|e| rsx!(span { class: "text-danger", " Fehler: {e}" })) }
                }

                { (list_items.get().is_empty() && list_path.get().is_none() && !*loading_list.get()).then(|| rsx!(
                    div { class: "alert alert-warning", "Keine Daten für Wurzeln – der Scan läuft eventuell noch oder die Root-Knoten wurden noch nicht gespeichert. Versuche es gleich erneut oder nutze Baum/Top." }
                )) }
            }
            table { style: table_style(),
                thead { tr {
                    th { style: "text-align:left;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "name".to_string();
                        if *list_sort.get() == key { list_order.set(if *list_order.get() == "desc" { "asc".into() } else { "desc".into() }); } else { list_sort.set(key); list_order.set("desc".into()); }
                        list_offset.set(0);
                    }, "Name" }
                    th { style: "text-align:left;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "type".to_string();
                        if *list_sort.get() == key { list_order.set(if *list_order.get() == "desc" { "asc".into() } else { "desc".into() }); } else { list_sort.set(key); list_order.set("desc".into()); }
                        list_offset.set(0);
                    }, "Typ" }
                    th { style: "text-align:left;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "accessed".to_string();
                        if *list_sort.get() == key { list_order.set(if *list_order.get() == "desc" { "asc".into() } else { "desc".into() }); } else { list_sort.set(key); list_order.set("desc".into()); }
                        list_offset.set(0);
                    }, "Zuletzt" }
                    th { style: "text-align:right;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "allocated".to_string();
                        if *list_sort.get() == key { list_order.set(if *list_order.get() == "desc" { "asc".into() } else { "desc".into() }); } else { list_sort.set(key); list_order.set("desc".into()); }
                        list_offset.set(0);
                    }, "Allokiert" }
                    th { style: "text-align:right;padding:6px;border-bottom:1px solid #222533;cursor:pointer;", onclick: move |_| {
                        let key = "logical".to_string();
                        if *list_sort.get() == key { list_order.set(if *list_order.get() == "desc" { "asc".into() } else { "desc".into() }); } else { list_sort.set(key); list_order.set("desc".into()); }
                        list_offset.set(0);
                    }, "Logisch" }
                    th { style: "text-align:left;padding:6px;border-bottom:1px solid #222533;", "Visual" }
                } }
                tbody {
                    { list_items.get().iter()
                        .filter(|it| {
                            // Suchfilter
                            let query = search_query.get().to_lowercase();
                            let name_match = if query.is_empty() { 
                                true 
                            } else {
                                match it {
                                    types::ListItem::Dir { name, .. } => name.to_lowercase().contains(&query),
                                    types::ListItem::File { name, .. } => name.to_lowercase().contains(&query),
                                }
                            };
                            
                            // Größenfilter
                            let size_match = match it {
                                types::ListItem::Dir { allocated_size, .. } => *allocated_size >= *min_size_filter.get(),
                                types::ListItem::File { allocated_size, .. } => *allocated_size >= *min_size_filter.get(),
                            };
                            
                            // Typfilter
                            let type_match = match file_type_filter.get().as_str() {
                                "dirs" => matches!(it, types::ListItem::Dir { .. }),
                                "files" => matches!(it, types::ListItem::File { .. }),
                                _ => true,
                            };
                            
                            // Versteckte Dateien Filter
                            let hidden_match = if !*show_hidden.get() {
                                match it {
                                    types::ListItem::Dir { name, .. } => !name.starts_with('.'),
                                    types::ListItem::File { name, .. } => !name.starts_with('.'),
                                }
                            } else {
                                true
                            };
                            
                            name_match && size_match && type_match && hidden_match
                        })
                        .map(|it| {
                        match it {
                            types::ListItem::Dir { name, path, allocated_size, logical_size, atime, .. } => {
                                let alloc = *allocated_size; let logical = *logical_size; let p = path.clone();
                                let percent = if max_alloc_list > 0 { ((alloc as f64) / (max_alloc_list as f64) * 100.0).clamp(1.0, 100.0) } else { 0.0 };
                                let bar_width = format!("width:{:.1}%;", percent);
                                rsx!{ tr {
                                    td { style: "padding:6px;border-bottom:1px solid #1b1e2a;cursor:pointer;color:#9cdcfe;", onclick: move |_| { 
                                        list_path.set(Some(p.clone())); 
                                        let mut hist = nav_history.get().clone();
                                        if !hist.contains(&p) { hist.push(p.clone()); }
                                        nav_history.set(hist);
                                    }, "{name}" }
                                    td { style: "padding:6px;border-bottom:1px solid #1b1e2a;", "Ordner" }
                                    td { style: "padding:6px;border-bottom:1px solid #1b1e2a;", "{fmt_ago_short(*atime)}" }
                                    td { style: "padding:6px;text-align:right;border-bottom:1px solid #1b1e2a;", "{fmt_bytes(alloc)}" }
                                    td { style: "padding:6px;text-align:right;border-bottom:1px solid #1b1e2a;", "{fmt_bytes(logical)}" }
                                    td { style: "padding:6px;border-bottom:1px solid #1b1e2a;min-width:160px;",
                                        div { class: "bar-shell", 
                                            div { class: "bar-fill-blue", style: "{bar_width}" }
                                        }
                                    }
                                } }
                            }
                            types::ListItem::File { name, allocated_size, logical_size, atime, .. } => {
                                let alloc = *allocated_size; let logical = *logical_size;
                                let percent = if max_alloc_list > 0 { ((alloc as f64) / (max_alloc_list as f64) * 100.0).clamp(1.0, 100.0) } else { 0.0 };
                                let bar_width = format!("width:{:.1}%;", percent);
                                rsx!{ tr {
                                    td { style: "padding:6px;border-bottom:1px solid #1b1e2a;", "{name}" }
                                    td { style: "padding:6px;border-bottom:1px solid #1b1e2a;", "Datei" }
                                    td { style: "padding:6px;border-bottom:1px solid #1b1e2a;", "{fmt_ago_short(*atime)}" }
                                    td { style: "padding:6px;text-align:right;border-bottom:1px solid #1b1e2a;", "{fmt_bytes(alloc)}" }
                                    td { style: "padding:6px;text-align:right;border-bottom:1px solid #1b1e2a;", "{fmt_bytes(logical)}" }
                                    td { style: "padding:6px;border-bottom:1px solid #1b1e2a;min-width:160px;",
                                        div { class: "bar-shell", 
                                            div { class: "bar-fill-green", style: "{bar_width}" }
                                        }
                                    }
                                } }
                            }
                        }
                    }) }
                }
            }  // table close
        }  // section close
    })
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

fn table_style() -> &'static str {
    "width:100%;border-collapse:collapse;margin-top:8px;background:#0f1117;border:1px solid #222533;border-radius:8px;"
}

// helper functions (fmt_bytes, copy_to_clipboard, show_toast, download_csv)
// are imported from ui_utils module
