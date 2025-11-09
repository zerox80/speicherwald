//! SpeicherWald Desktop Application - Tauri Frontend
//!
//! This is the desktop application wrapper that manages the backend server
//! and provides a native desktop interface for SpeicherWald. The application
//! automatically finds, spawns, and manages the backend process while providing
//! a seamless user experience.
//!
//! ## Architecture
//!
//! - **Backend Management**: Automatically locates and spawns the backend server
//! - **Process Lifecycle**: Manages backend process startup, health checks, and cleanup
//! - **Window Management**: Creates and manages the desktop window interface
//! - **Error Handling**: Provides informative error displays when backend fails to start
//!
//! ## Features
//!
//! - Automatic backend discovery in multiple locations
//! - Dynamic port allocation for avoiding conflicts
//! - Health check verification before opening main window
//! - Proper cleanup on application exit
//! - User-friendly error messages in German

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

/// Application state for managing the backend process.
///
/// Holds a mutex-protected reference to the spawned backend child process
/// and the port number on which the backend is running.
struct BackendState {
  /// The spawned backend process handle
  child: Mutex<Option<Child>>,
  /// The port number the backend is running on
  port: u16,
}

/// Finds an available TCP port on the localhost interface.
///
/// Binds to port 0 which automatically selects an available port,
/// then returns that port number and closes the listener.
///
/// # Returns
///
/// An available port number on 127.0.0.1
fn find_free_port() -> u16 {
  let listener = TcpListener::bind("127.0.0.1:0").expect("bind 0");
  let port = listener.local_addr().unwrap().port();
  drop(listener);
  port
}

/// Generates a list of candidate paths where the backend executable might be located.
///
/// Searches multiple common locations for the backend executable, including:
/// - Environment variable override
/// - Same directory as the desktop executable
/// - Development build directories
/// - Packaged resource locations
///
/// # Returns
///
/// A vector of candidate paths, deduplicated and case-insensitive on Windows
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

/// Spawns the backend server process on the specified port.
///
/// Attempts to find and execute the backend server from one of the candidate
/// locations. Configures the backend with the appropriate port, host, and
/// environment variables for database storage.
///
/// # Arguments
///
/// * `port` - The port number on which the backend should listen
///
/// # Returns
///
/// * `anyhow::Result<Child>` - The spawned process handle or an error if spawning failed
///
/// # Notes
///
/// - Avoids spawning the desktop executable itself (recursion prevention)
/// - Sets up environment for user-writable database location
/// - In debug mode, inherits stdout/stderr for development visibility
/// - In release mode, suppresses output for cleaner user experience
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

/// Waits for the backend server to become ready and responsive.
///
/// Periodically checks if the backend server is responding to HTTP requests
/// on the specified port. Used to ensure the backend is fully started before
/// opening the main application window.
///
/// # Arguments
///
/// * `port` - The port number on which the backend should be listening
/// * `timeout_ms` - Maximum time to wait in milliseconds
///
/// # Returns
///
/// `true` if the backend becomes ready within the timeout, `false` otherwise
///
/// # Notes
///
/// - Checks the /healthz endpoint for responsiveness
/// - Returns immediately on first successful health check
/// - Waits 150ms between attempts to avoid excessive polling
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

/// Terminates the backend server process gracefully.
///
/// Sends a termination signal to the backend process and waits for it to exit.
/// This is called when the desktop application is closing to ensure proper cleanup.
///
/// # Arguments
///
/// * `child` - A mutable reference to the optional child process handle
///
/// # Notes
///
/// - Uses platform-appropriate termination signals
/// - Waits for the process to exit before returning
/// - Sets the child handle to None after termination
/// - Gracefully handles the case where no child process exists
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

/// Generates environment variables for user-writable locations.
///
/// Sets up environment variables to ensure the SQLite database is stored
/// in a user-writable directory, avoiding permission issues when the
/// application is installed in protected locations like Program Files.
///
/// # Returns
///
/// A vector of environment variable name-value pairs
///
/// # Notes
///
/// - Places the database in %LOCALAPPDATA%\SpeicherWald on Windows
/// - Creates the directory if it doesn't exist
/// - Converts Windows paths to forward slashes for SQLite URI format
/// - Ensures proper URI format with sqlite:// prefix
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

/// Percent-encodes a string for use in a data URL.
///
/// Converts a string into a properly percent-encoded format suitable for
/// use in data URLs. Handles HTML content and special characters safely.
///
/// # Arguments
///
/// * `input` - The string to percent-encode
///
/// # Returns
///
/// A percent-encoded string safe for use in data URLs
///
/// # Notes
///
/// - Preserves alphanumeric characters and common URL-safe symbols
/// - Encodes spaces as %20, special characters as %XX sequences
/// - Handles newlines and other control characters
/// - Safe for HTML content and German text
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

/// Main entry point for the SpeicherWald desktop application.
///
/// Sets up and runs the Tauri application, handling backend process management,
/// window creation, and error handling. This is the primary entry point that
/// orchestrates the entire desktop application lifecycle.
///
/// # Application Flow
///
/// 1. Find an available port for the backend
/// 2. Attempt to spawn the backend server process
/// 3. If successful: wait for backend to be ready, then open main window
/// 4. If failed: show error window with troubleshooting information
/// 5. Handle window close events by properly cleaning up the backend
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
