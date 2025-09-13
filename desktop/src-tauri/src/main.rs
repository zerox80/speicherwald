#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
  env,
  collections::HashSet,
  io::{Read, Write},
  net::{TcpListener, TcpStream},
  path::PathBuf,
  process::{Child, Command, Stdio},
  sync::Mutex,
  thread,
  time::Duration,
};
use tauri::{Manager, WindowUrl};

struct BackendState {
  child: Mutex<Option<Child>>,
  port: u16,
}

fn find_free_port() -> u16 {
  let listener = TcpListener::bind("127.0.0.1:0").expect("bind 0");
  let port = listener.local_addr().unwrap().port();
  drop(listener);
  port
}

fn candidate_backend_paths() -> Vec<PathBuf> {
  let mut v = Vec::new();
  if let Ok(envp) = env::var("SPEICHERWALD_BACKEND_PATH") {
    v.push(PathBuf::from(envp));
  }
  if let Ok(exe) = env::current_exe() {
    if let Some(dir) = exe.parent() {
      // Prefer distinct backend names first
      v.push(dir.join("speicherwald-backend.exe"));
      v.push(dir.join("speicherwald-backend"));
      // Legacy/common names
      v.push(dir.join("speicherwald.exe"));
      v.push(dir.join("speicherwald"));
      // packaged resource convention
      v.push(dir.join("..\\resources\\speicherwald-backend.exe"));
      v.push(dir.join("..\\resources\\speicherwald.exe"));
      v.push(dir.join("..\\resources\\speicherwald"));
    }
  }
  // repo-relative common dev paths
  if let Ok(mut cwd) = env::current_dir() {
    // desktop/ -> repo root
    if cwd.ends_with("desktop") { cwd.pop(); }
    v.push(cwd.join("target\\release\\speicherwald.exe"));
    v.push(cwd.join("target\\debug\\speicherwald.exe"));
    v.push(cwd.join("speicherwald.exe"));
    v.push(cwd.join("target\\release\\speicherwald-backend.exe"));
    v.push(cwd.join("target\\debug\\speicherwald-backend.exe"));
  }
  // de-duplicate candidates (case-insensitive on Windows)
  let mut seen = HashSet::<String>::new();
  v.retain(|p| seen.insert(p.to_string_lossy().to_lowercase()));
  v
}

fn spawn_backend(port: u16) -> anyhow::Result<Child> {
  let mut last_err: Option<anyhow::Error> = None;
  let self_path = env::current_exe().ok().and_then(|p| p.canonicalize().ok());
  for cand in candidate_backend_paths() {
    if let Ok(cc) = cand.canonicalize() {
      if let Some(ref sp) = self_path {
        if &cc == sp {
          // avoid spawning ourselves -> recursion
          eprintln!("[desktop] skip self executable as backend: {}", cc.display());
          continue;
        }
      }
    }
    if cand.exists() {
      let mut cmd = Command::new(&cand);
      if let Some(dir) = cand.parent() { cmd.current_dir(dir); }
      cmd.env("SPEICHERWALD__SERVER__PORT", format!("{}", port))
        .env("SPEICHERWALD__SERVER__HOST", "127.0.0.1")
        .envs(user_writable_envs());
      #[cfg(debug_assertions)]
      { cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit()); }
      #[cfg(not(debug_assertions))]
      { cmd.stdout(Stdio::null()).stderr(Stdio::null()); }
      match cmd.spawn() {
        Ok(child) => return Ok(child),
        Err(e) => { eprintln!("[desktop] failed to spawn {:?}: {}", cand, e); last_err = Some(anyhow::anyhow!(e)); }
      }
    }
  }
  Err(last_err.unwrap_or_else(|| anyhow::anyhow!("speicherwald executable not found")))
}

fn wait_until_ready(port: u16, timeout_ms: u64) -> bool {
  let start = std::time::Instant::now();
  while start.elapsed() < Duration::from_millis(timeout_ms) {
    if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
      let _ = s.write_all(b"GET /healthz HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
      let mut buf = [0u8; 64];
      if let Ok(n) = s.read(&mut buf) {
        if n >= 12 && &buf[..12] == b"HTTP/1.1 200" { return true; }
      }
    }
    thread::sleep(Duration::from_millis(150));
  }
  false
}

fn kill_backend(child: &mut Option<Child>) {
  if let Some(ch) = child.as_mut() {
    #[cfg(windows)]
    {
      let _ = ch.kill();
    }
    #[cfg(not(windows))]
    {
      let _ = ch.kill();
    }
    let _ = ch.wait();
  }
  *child = None;
}

fn user_writable_envs() -> Vec<(String, String)> {
  // Place the SQLite DB in a user-writable directory to avoid Program Files write restrictions
  let mut envs: Vec<(String, String)> = Vec::new();
  if let Ok(lapp) = env::var("LOCALAPPDATA") {
    let db_dir = std::path::Path::new(&lapp).join("SpeicherWald");
    let _ = std::fs::create_dir_all(&db_dir);
    let db_path = db_dir.join("speicherwald.db");
    // Normalize to forward slashes and absolute style: sqlite:///C:/...
    let mut p = db_path.to_string_lossy().replace('\\', "/");
    if !p.starts_with('/') { p = format!("/{}", p); }
    let db_url = format!("sqlite://{}", p);
    envs.push(("SPEICHERWALD__DATABASE__URL".to_string(), db_url));
  }
  envs
}

fn percent_encode_for_data_url(input: &str) -> String {
  let mut s = String::with_capacity(input.len() * 2);
  for ch in input.chars() {
    match ch {
      'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' | '/' | ':' | '=' | '&' | ';' | ',' | '\'' | '(' | ')' | '!' | '*' => s.push(ch),
      ' ' => s.push_str("%20"),
      '<' => s.push_str("%3C"),
      '>' => s.push_str("%3E"),
      '"' => s.push_str("%22"),
      '#' => s.push_str("%23"),
      '%' => s.push_str("%25"),
      '\n' => s.push_str("%0A"),
      '\r' => s.push_str("%0D"),
      _ => s.push(ch),
    }
  }
  s
}

fn main() {
  let port = find_free_port();

  tauri::Builder::default()
    .setup(move |app| {
      // launch backend
      let child_res = spawn_backend(port);

      match child_res {
        Ok(child) => {
          let state = BackendState { child: Mutex::new(Some(child)), port };
          app.manage(state);

          // wait until ready and then open window
          {
            let app_handle = app.handle();
            thread::spawn(move || {
              if wait_until_ready(port, 10_000) {
                let _ = tauri::WindowBuilder::new(
                  &app_handle,
                  "main",
                  WindowUrl::External(format!("http://127.0.0.1:{}/", port).parse().unwrap())
                )
                .title("SpeicherWald")
                .inner_size(1200.0, 800.0)
                .build();
              } else {
                // fallback: open /healthz anyway so user sees something
                let _ = tauri::WindowBuilder::new(
                  &app_handle,
                  "main",
                  WindowUrl::External(format!("http://127.0.0.1:{}/healthz", port).parse().unwrap())
                )
                .title("SpeicherWald – Backend nicht erreichbar")
                .inner_size(900.0, 600.0)
                .build();
              }
            });
          }

          Ok(())
        }
        Err(e) => {
          // Show an informative window instead of exiting silently
          let app_handle = app.handle();
          let html = format!(r#"<html><head><meta charset='utf-8'><title>SpeicherWald – Fehler</title></head>
<body style='font-family:Segoe UI, sans-serif; padding:20px;'>
  <h2>SpeicherWald – Backend konnte nicht gestartet werden</h2>
  <p style='color:#b00020;'>Fehler: {}</p>
  <p>Bitte prüfen Sie:</p>
  <ul>
    <li>Liegt <code>speicherwald.exe</code> im selben Ordner wie <code>SpeicherWald.exe</code>?</li>
    <li>Wurde die Datei ggf. von SmartScreen blockiert? Rechtsklick → Eigenschaften → Zulassen.</li>
    <li>Test: Starten Sie <code>speicherwald.exe</code> in PowerShell und öffnen Sie dann <a href='http://127.0.0.1:8080/'>http://127.0.0.1:8080/</a>.</li>
  </ul>
</body></html>"#, e);
          let url = WindowUrl::External(
            format!("data:text/html,{}", percent_encode_for_data_url(&html)).parse().unwrap()
          );
          let _ = tauri::WindowBuilder::new(&app_handle, "main", url)
            .title("SpeicherWald – Fehler")
            .inner_size(900.0, 600.0)
            .build();
          Ok(())
        }
      }
    })
    .on_window_event(|event| {
      if let tauri::WindowEvent::CloseRequested { .. } = event.event() {
        if let Some(state) = event.window().try_state::<BackendState>() {
          let mut guard = state.child.lock().unwrap();
          kill_backend(&mut *guard);
        }
      }
    })
    .build(tauri::generate_context!())
    .expect("error while running tauri application")
    .run(|_app_handle, _event| {});
}
