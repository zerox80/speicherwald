use serde::Serialize;
use serde_json::Value as JsonValue;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{EventSource, MessageEvent};

use crate::types::*;

pub const BASE: &str = ""; // use same-origin relative URLs

fn url(path: &str) -> String { format!("{}{}", BASE, path) }

pub async fn list_scans() -> Result<Vec<ScanSummary>, String> {
    let resp = reqwasm::http::Request::get(&url("/scans")).send().await.map_err(map_net)?;
    if !resp.ok() { return Err(resp.text().await.unwrap_or_else(|_| "HTTP Fehler".into())); }
    resp.json().await.map_err(map_net)
}

pub async fn list_drives() -> Result<DrivesResponse, String> {
    let resp = reqwasm::http::Request::get(&url("/drives")).send().await.map_err(map_net)?;
    if !resp.ok() { return Err(resp.text().await.unwrap_or_else(|_| "HTTP Fehler".into())); }
    resp.json().await.map_err(map_net)
}

pub async fn healthz() -> Result<bool, String> {
    let resp = reqwasm::http::Request::get(&url("/healthz")).send().await.map_err(map_net)?;
    Ok(resp.ok())
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateScanReq {
    pub root_paths: Vec<String>,
    pub follow_symlinks: Option<bool>,
    pub include_hidden: Option<bool>,
    pub measure_logical: Option<bool>,
    pub measure_allocated: Option<bool>,
    pub excludes: Option<Vec<String>>,
    pub max_depth: Option<u32>,
    pub concurrency: Option<usize>,
}

pub async fn create_scan(req: &CreateScanReq) -> Result<CreateScanResp, String> {
    let resp = reqwasm::http::Request::post(&url("/scans"))
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(req).unwrap())
        .send().await.map_err(map_net)?;
    if !resp.ok() { return Err(resp.text().await.unwrap_or_else(|_| "HTTP Fehler".into())); }
    resp.json().await.map_err(map_net)
}

pub async fn get_scan(id: &str) -> Result<ScanSummary, String> {
    let resp = reqwasm::http::Request::get(&url(&format!("/scans/{}", id))).send().await.map_err(map_net)?;
    if !resp.ok() { return Err(resp.text().await.unwrap_or_else(|_| "HTTP Fehler".into())); }
    resp.json().await.map_err(map_net)
}

pub async fn cancel_scan(id: &str, purge: bool) -> Result<(), String> {
    let resp = reqwasm::http::Request::delete(&url(&format!("/scans/{}?purge={}", id, if purge {"true"} else {"false"})))
        .send().await.map_err(map_net)?;
    if !resp.ok() { return Err(resp.text().await.unwrap_or_else(|_| "HTTP Fehler".into())); }
    Ok(())
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TreeQuery { pub path: Option<String>, pub depth: Option<i64>, pub sort: Option<String>, pub limit: Option<i64> }

pub async fn get_tree(id: &str, q: &TreeQuery) -> Result<Vec<NodeDto>, String> {
    let mut qs = vec![];
    if let Some(p) = &q.path { qs.push(format!("path={}", urlencoding::encode(p))); }
    if let Some(d) = q.depth { qs.push(format!("depth={}", d)); }
    if let Some(s) = &q.sort { qs.push(format!("sort={}", urlencoding::encode(s))); }
    if let Some(l) = q.limit { qs.push(format!("limit={}", l)); }
    let qstr = if qs.is_empty() { String::new() } else { format!("?{}", qs.join("&")) };
    let resp = reqwasm::http::Request::get(&url(&format!("/scans/{}/tree{}", id, qstr))).send().await.map_err(map_net)?;
    if !resp.ok() { return Err(resp.text().await.unwrap_or_else(|_| "HTTP Fehler".into())); }
    resp.json().await.map_err(map_net)
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TopQuery { pub scope: Option<String>, pub limit: Option<i64> }

pub async fn get_top(id: &str, q: &TopQuery) -> Result<Vec<TopItem>, String> {
    let mut qs = vec![];
    if let Some(s) = &q.scope { qs.push(format!("scope={}", urlencoding::encode(s))); }
    if let Some(l) = q.limit { qs.push(format!("limit={}", l)); }
    let qstr = if qs.is_empty() { String::new() } else { format!("?{}", qs.join("&")) };
    let resp = reqwasm::http::Request::get(&url(&format!("/scans/{}/top{}", id, qstr))).send().await.map_err(map_net)?;
    if !resp.ok() { return Err(resp.text().await.unwrap_or_else(|_| "HTTP Fehler".into())); }
    resp.json().await.map_err(map_net)
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ListQuery { pub path: Option<String>, pub sort: Option<String>, pub order: Option<String>, pub limit: Option<i64>, pub offset: Option<i64> }

pub async fn get_list(id: &str, q: &ListQuery) -> Result<Vec<ListItem>, String> {
    let mut qs = vec![];
    if let Some(p) = &q.path { qs.push(format!("path={}", urlencoding::encode(p))); }
    if let Some(s) = &q.sort { qs.push(format!("sort={}", urlencoding::encode(s))); }
    if let Some(o) = &q.order { qs.push(format!("order={}", urlencoding::encode(o))); }
    if let Some(l) = q.limit { qs.push(format!("limit={}", l)); }
    if let Some(o) = q.offset { qs.push(format!("offset={}", o)); }
    let qstr = if qs.is_empty() { String::new() } else { format!("?{}", qs.join("&")) };
    let resp = reqwasm::http::Request::get(&url(&format!("/scans/{}/list{}", id, qstr))).send().await.map_err(map_net)?;
    if !resp.ok() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_else(|_| "HTTP Fehler".into());
        if status == 429 {
            // Try to parse retry_after_seconds from backend JSON
            if let Ok(v) = serde_json::from_str::<JsonValue>(&text) {
                if let Some(sec) = v.get("retry_after_seconds").and_then(|x| x.as_u64()) {
                    return Err(format!("Zu viele Anfragen (429). Bitte nach {} Sekunden erneut versuchen.", sec));
                }
                if let Some(obj) = v.get("error").and_then(|e| e.as_object()) {
                    if let Some(msg) = obj.get("message").and_then(|m| m.as_str()) {
                        return Err(format!("Zu viele Anfragen (429). {}", msg));
                    }
                }
            }
        }
        return Err(text);
    }
    resp.json().await.map_err(map_net)
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SearchQuery {
    pub query: String,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub min_size: Option<i64>,
    pub max_size: Option<i64>,
    pub file_type: Option<String>,
    pub include_files: Option<bool>,
    pub include_dirs: Option<bool>,
}

pub async fn search_scan(id: &str, q: &SearchQuery) -> Result<SearchResult, String> {
    let mut qs = vec![];
    qs.push(format!("query={}", urlencoding::encode(&q.query)));
    if let Some(l) = q.limit { qs.push(format!("limit={}", l)); }
    if let Some(o) = q.offset { qs.push(format!("offset={}", o)); }
    if let Some(s) = q.min_size { qs.push(format!("min_size={}", s)); }
    if let Some(s) = q.max_size { qs.push(format!("max_size={}", s)); }
    if let Some(t) = &q.file_type { qs.push(format!("file_type={}", urlencoding::encode(t))); }
    if let Some(f) = q.include_files { qs.push(format!("include_files={}", f)); }
    if let Some(d) = q.include_dirs { qs.push(format!("include_dirs={}", d)); }
    let qstr = format!("?{}", qs.join("&"));
    let resp = reqwasm::http::Request::get(&url(&format!("/scans/{}/search{}", id, qstr))).send().await.map_err(map_net)?;
    if !resp.ok() { return Err(resp.text().await.unwrap_or_else(|_| "HTTP Fehler".into())); }
    resp.json().await.map_err(map_net)
}

fn map_net(e: reqwasm::Error) -> String { format!("Netzwerkfehler: {}", e) }

pub async fn move_path(req: &MovePathRequest) -> Result<MovePathResponse, String> {
    let resp = reqwasm::http::Request::post(&url("/paths/move"))
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(req).unwrap())
        .send()
        .await
        .map_err(map_net)?;
    if !resp.ok() {
        return Err(resp.text().await.unwrap_or_else(|_| "HTTP Fehler".into()));
    }
    resp.json().await.map_err(map_net)
}

// SSE helper: open EventSource and wire message callback. Returns the EventSource to be kept alive.
pub fn sse_attach<F>(id: &str, mut on_message: F) -> Result<EventSource, String>
where F: 'static + FnMut(ScanEvent) {
    let es = EventSource::new(&url(&format!("/scans/{}/events", id))).map_err(|e| format!("SSE Fehler: {:?}", e))?;
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |ev: web_sys::Event| {
        if let Ok(me) = ev.dyn_into::<MessageEvent>() {
            if let Some(text) = me.data().as_string() {
                if let Ok(ev) = serde_json::from_str::<ScanEvent>(&text) {
                    on_message(ev);
                }
            }
        }
    });
    es.set_onmessage(Some(closure.as_ref().unchecked_ref()));
    // Leak the closure to keep it as long as the EventSource lives (we close ES on drop by the owner)
    closure.forget();
    Ok(es)
}
