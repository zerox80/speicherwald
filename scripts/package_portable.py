#!/usr/bin/env python3
"""
Package SpeicherWald as a portable ZIP (Server + WebUI, optional Desktop GUI) for releases.

This script will:
- Build the WebUI (Trunk/Dioxus) into the repository-level `ui/` directory
- Build the backend binary in release mode
- Optionally build the Desktop GUI (Tauri) on Windows and include it
- Stage the portable layout (speicherwald.exe next to `ui/`)
- Create a versioned ZIP under `dist/`

Requirements:
- Rust toolchain (cargo, rustc)
- On first run for UI build: `rustup target add wasm32-unknown-unknown`
- Trunk for building the WASM UI: `cargo install trunk --locked`
- For Desktop build: Windows environment, Rust toolchain; Tauri crate will be built via cargo

Usage:
  python scripts/package_portable.py [--include-desktop]

Optional environment variables:
  SPEICHERWALD_EXTRA_FILES: semicolon-separated paths to include in the ZIP (e.g. "README.md;LICENSE")

The backend was patched to resolve the UI folder relative to the executable at runtime,
so the extracted ZIP is portable. Start `speicherwald.exe` and open http://127.0.0.1:8080/.
If packaged with `--include-desktop` on Windows, you can also run `speicherwald-desktop.exe`.
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import argparse
from datetime import datetime
from pathlib import Path
from zipfile import ZipFile, ZIP_DEFLATED
from typing import Optional, List, Dict

REPO_ROOT = Path(__file__).resolve().parents[1]
WEBUI_DIR = REPO_ROOT / "webui"
UI_OUT_DIR = REPO_ROOT / "ui"
TARGET_DIR = REPO_ROOT / "target" / "release"
EXE_NAME = "speicherwald.exe" if os.name == "nt" else "speicherwald"
DIST_DIR = REPO_ROOT / "dist"


def run(cmd: List[str], cwd: Optional[Path] = None, env: Optional[Dict[str, str]] = None) -> None:
    print(f"\n$ {' '.join(cmd)} (cwd={cwd or REPO_ROOT})")
    subprocess.run(cmd, cwd=str(cwd) if cwd else None, env=env, check=True)


def which(name: str) -> bool:
    from shutil import which as _which
    return _which(name) is not None


def read_version_from_cargo() -> str:
    cargo_toml = (REPO_ROOT / "Cargo.toml").read_text(encoding="utf-8", errors="ignore")
    # naive parse
    m = re.search(r"^\s*version\s*=\s*\"([^\"]+)\"", cargo_toml, re.MULTILINE)
    return m.group(1) if m else "0.0.0"


def ensure_prereqs() -> None:
    if not which("cargo"):
        raise RuntimeError("cargo not found in PATH. Please install Rust (https://rustup.rs)")

    # UI toolchain checks
    need_wasm_target = True
    try:
        out = subprocess.run(["rustup", "target", "list", "--installed"], capture_output=True, text=True, check=True)
        need_wasm_target = "wasm32-unknown-unknown" not in out.stdout
    except Exception:
        # rustup may not be available if using a custom toolchain install
        print("warning: could not verify rustup target; will try to add wasm target if available")

    if need_wasm_target and which("rustup"):
        run(["rustup", "target", "add", "wasm32-unknown-unknown"])  # may be a no-op if already present

    if not which("trunk"):
        print("'trunk' not found. Installing via 'cargo install trunk --locked' (first time only)...")
        run(["cargo", "install", "trunk", "--locked"])  # network install


def build_webui() -> None:
    if not WEBUI_DIR.exists():
        print(f"skip: {WEBUI_DIR} does not exist – nothing to build for UI")
        return
    # Build UI (Trunk outputs to ../ui per webui/Trunk.toml)
    run(["trunk", "build", "--release"], cwd=WEBUI_DIR)
    if not (UI_OUT_DIR / "index.html").exists():
        raise RuntimeError("UI build did not produce ui/index.html – please check Trunk errors above")


def build_backend() -> Path:
    run(["cargo", "build", "--release"], cwd=REPO_ROOT)
    exe = TARGET_DIR / EXE_NAME
    if not exe.exists():
        raise RuntimeError(f"backend binary not found at {exe}")
    return exe


def build_desktop() -> Optional[Path]:
    """Build the Tauri desktop app (Windows) and return path to the exe if successful."""
    tauri_dir = REPO_ROOT / "desktop" / "src-tauri"
    if not tauri_dir.exists():
        print(f"skip: {tauri_dir} does not exist – desktop not available")
        return None
    if os.name != "nt":
        print("skip: desktop build is only supported on Windows in this script")
        return None
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(tauri_dir / "target-tauri")
    # -j 1 to avoid file lock issues on Windows
    run(["cargo", "build", "--release", "-j", "1"], cwd=tauri_dir, env=env)
    exe = tauri_dir / "target-tauri" / "release" / "speicherwald-desktop.exe"
    if not exe.exists():
        raise RuntimeError(f"desktop binary not found at {exe}")
    return exe


def stage_and_zip(exe_path: Path, version: str, desktop_exe: Optional[Path] = None) -> Path:
    timestamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    out_name = f"speicherwald-portable-v{version}-windows-x64-{timestamp}.zip"
    DIST_DIR.mkdir(parents=True, exist_ok=True)
    zip_path = DIST_DIR / out_name

    # Staging layout (in-memory via zipfile):
    #   speicherwald.exe
    #   ui/**
    #   RUN-SpeicherWald.cmd
    #   README-PORTABLE.txt
    #   LICENSE, README.md (optional)

    readme_portable = (
        "SpeicherWald – Portable Build\n\n"
        "Start: RUN-SpeicherWald.cmd (öffnet Browser) oder direkt speicherwald.exe.\n"
        "Web-UI: http://127.0.0.1:8080/\n\n"
        "Optional (falls enthalten): speicherwald-desktop.exe oder RUN-Desktop.cmd für die Desktop-GUI.\n\n"
        "Datenbank: ./data/speicherwald.db (wird beim Start angelegt).\n"
        "Konfiguration (optional): speicherwald.toml im gleichen Ordner.\n"
    )
    run_cmd_bat = ("\r\n".join([
        "@echo off",
        "setlocal",
        ":: Starte Server im Hintergrund und öffne Browser",
        f'start "" "%~dp0{EXE_NAME}"',
        "timeout /t 1 >nul",
        'start "" http://127.0.0.1:8080/',
    ]) + "\r\n")

    # Create ZIP
    with ZipFile(zip_path, "w", compression=ZIP_DEFLATED, compresslevel=9) as z:
        # exe at root
        z.write(exe_path, arcname=EXE_NAME)
        # desktop exe at root (optional)
        if desktop_exe and desktop_exe.exists():
            z.write(desktop_exe, arcname="speicherwald-desktop.exe")
        # ui folder
        if UI_OUT_DIR.exists():
            for p in UI_OUT_DIR.rglob("*"):
                if p.is_file():
                    rel = UI_OUT_DIR.name + "/" + str(p.relative_to(UI_OUT_DIR)).replace("\\", "/")
                    z.write(p, arcname=rel)
        # run helper
        z.writestr("RUN-SpeicherWald.cmd", run_cmd_bat)
        if desktop_exe and desktop_exe.exists():
            run_desktop_bat = ("\r\n".join([
                "@echo off",
                "setlocal",
                ":: Starte Desktop-GUI",
                "start \"\" \"%~dp0speicherwald-desktop.exe\"",
            ]) + "\r\n")
            z.writestr("RUN-Desktop.cmd", run_desktop_bat)
        z.writestr("README-PORTABLE.txt", readme_portable)
        # optional extras
        extras_env = os.environ.get("SPEICHERWALD_EXTRA_FILES", "").strip()
        if extras_env:
            for pth in extras_env.split(";"):
                p = (REPO_ROOT / pth).resolve()
                if p.exists() and p.is_file():
                    z.write(p, arcname=p.name)
        # try auto-include LICENSE/README if present
        for fname in ("LICENSE", "LICENSE.txt", "README.md"):
            f = REPO_ROOT / fname
            if f.exists() and f.is_file():
                z.write(f, arcname=f.name)

    print(f"\nCreated: {zip_path}")
    return zip_path


def main() -> None:
    parser = argparse.ArgumentParser(description="Package SpeicherWald portable ZIP")
    parser.add_argument("--include-desktop", action="store_true", help="Include Tauri Desktop GUI (Windows only)")
    args = parser.parse_args()

    print("SpeicherWald portable packager\n")
    version = read_version_from_cargo()
    print(f"Detected version: v{version}")

    ensure_prereqs()
    build_webui()
    exe = build_backend()
    desktop_exe: Optional[Path] = None
    if args.include_desktop:
        try:
            desktop_exe = build_desktop()
        except Exception as e:
            print(f"warning: desktop build failed: {e}")
    zip_path = stage_and_zip(exe, version, desktop_exe)

    print("\nAll done.")
    print(f"Portable ZIP: {zip_path}")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as e:
        print(f"ERROR: command failed with exit code {e.returncode}")
        sys.exit(e.returncode)
    except Exception as e:
        print(f"ERROR: {e}")
        sys.exit(1)
