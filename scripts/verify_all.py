#!/usr/bin/env python3
"""
verify_all.py

Führt eine vollständige lokale Verifikation wie in der CI durch:
- cargo fmt -- --check
- cargo clippy -- -D warnings
- cargo test --verbose
- cargo test --all-features --verbose
- cargo build --release --verbose
- (optional) WebUI-Build mit Trunk (webui/)
- (optional) Desktop-Build (desktop/src-tauri)
- (optional) cargo bench --no-run (Windows)

Benutzung (Beispiele):
  py -3 scripts/verify_all.py                    # alles (falls Trunk/Target vorhanden)
  py -3 scripts/verify_all.py --ensure-deps      # fehlende wasm-Target/Trunk installieren
  py -3 scripts/verify_all.py --skip-ui          # UI-Build überspringen
  py -3 scripts/verify_all.py --skip-desktop     # Desktop-Build überspringen
  py -3 scripts/verify_all.py --keep-going       # bei Fehlern andere Schritte weiter ausführen

Exit-Codes:
  0 = alle gewählten Schritte erfolgreich
  1 = mind. ein Schritt fehlgeschlagen
  2 = Umgebungsvoraussetzungen fehlen (cargo/rustc)
"""
from __future__ import annotations

import argparse
import os
import platform
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import List, Optional, Tuple


REPO_ROOT = Path(__file__).resolve().parent.parent
WEBUI_DIR = REPO_ROOT / "webui"
UI_DIR = REPO_ROOT / "ui"
DESKTOP_TAURI_DIR = REPO_ROOT / "desktop" / "src-tauri"


class StepResult:
    def __init__(self, name: str, ok: bool, returncode: int = 0, duration_s: float = 0.0):
        self.name = name
        self.ok = ok
        self.returncode = returncode
        self.duration_s = duration_s

    def __repr__(self) -> str:
        status = "OK" if self.ok else f"FAIL({self.returncode})"
        return f"<StepResult {self.name}: {status} in {self.duration_s:.1f}s>"


def which(tool: str) -> Optional[str]:
    return shutil.which(tool)


def run_cmd(
    cmd: List[str],
    cwd: Optional[Path] = None,
    env: Optional[dict] = None,
    verbose: bool = True,
) -> Tuple[int, float]:
    if verbose:
        here = f" (cwd={cwd})" if cwd else ""
        print(f"\n==> Running: {' '.join(cmd)}{here}")
    t0 = time.time()
    proc = subprocess.run(cmd, cwd=str(cwd) if cwd else None, env=env)
    dt = time.time() - t0
    return proc.returncode, dt


def ensure_base_tools() -> None:
    if not which("cargo") or not which("rustc"):
        print("ERROR: cargo/rustc nicht im PATH gefunden. Bitte Rust installieren: https://rustup.rs/")
        sys.exit(2)


def wasm_target_installed() -> bool:
    try:
        proc = subprocess.run(
            ["rustup", "target", "list", "--installed"],
            check=False,
            capture_output=True,
            text=True,
        )
        if proc.returncode != 0:
            return False
        return "wasm32-unknown-unknown" in proc.stdout
    except FileNotFoundError:
        return False


def ensure_wasm_target(ensure_deps: bool) -> bool:
    if wasm_target_installed():
        return True
    if not ensure_deps:
        print(
            "WARN: wasm32-unknown-unknown ist nicht installiert. UI-Build wird fehlschlagen. "
            "Starte erneut mit --ensure-deps oder nutze --skip-ui."
        )
        return False
    print("Installing rust target: wasm32-unknown-unknown ...")
    rc, _ = run_cmd(["rustup", "target", "add", "wasm32-unknown-unknown"])
    return rc == 0


def trunk_installed() -> bool:
    return which("trunk") is not None


def ensure_trunk(ensure_deps: bool) -> bool:
    if trunk_installed():
        return True
    if not ensure_deps:
        print(
            "WARN: 'trunk' ist nicht installiert. UI-Build wird fehlschlagen. "
            "Starte erneut mit --ensure-deps oder nutze --skip-ui."
        )
        return False
    print("Installing trunk (cargo install trunk --locked) ...")
    rc, _ = run_cmd(["cargo", "install", "trunk", "--locked"])
    return rc == 0


def add_common_env() -> dict:
    env = os.environ.copy()
    env.setdefault("CARGO_TERM_COLOR", "always")
    env.setdefault("RUST_BACKTRACE", "1")
    return env


def run_steps(args: argparse.Namespace) -> int:
    results: List[StepResult] = []
    env = add_common_env()

    def do(name: str, cmd: List[str], cwd: Optional[Path] = None, extra_env: Optional[dict] = None) -> None:
        step_env = env if extra_env is None else {**env, **extra_env}
        rc, dt = run_cmd(cmd, cwd=cwd, env=step_env, verbose=not args.quiet)
        ok = (rc == 0)
        results.append(StepResult(name, ok, rc, dt))
        if not ok and not args.keep_going:
            summarize_and_exit(results)

    # 1) Lints
    if not args.skip_lints:
        do("fmt --check", ["cargo", "fmt", "--", "--check"], cwd=REPO_ROOT)
        do("clippy -D warnings", ["cargo", "clippy", "--", "-D", "warnings"], cwd=REPO_ROOT)

    # 2) Tests
    if not args.skip_tests:
        do("test", ["cargo", "test", "--verbose"], cwd=REPO_ROOT)
        do("test --all-features", ["cargo", "test", "--all-features", "--verbose"], cwd=REPO_ROOT)

    # 3) Release-Build
    if not args.skip_build:
        do("build --release", ["cargo", "build", "--release", "--verbose"], cwd=REPO_ROOT)

    # 4) WebUI (Trunk)
    if not args.skip_ui:
        if not WEBUI_DIR.exists():
            print("WARN: webui/-Verzeichnis nicht gefunden. Überspringe UI-Build.")
        else:
            ok_target = ensure_wasm_target(args.ensure_deps)
            ok_trunk = ensure_trunk(args.ensure_deps)
            if ok_target and ok_trunk:
                do("webui trunk build --release", ["trunk", "build", "--release"], cwd=WEBUI_DIR)
            else:
                results.append(StepResult("webui trunk build --release", False, 1, 0.0))
                if not args.keep_going:
                    summarize_and_exit(results)

    # 5) Desktop (Tauri)
    if not args.skip_desktop:
        if not DESKTOP_TAURI_DIR.exists():
            print("WARN: desktop/src-tauri/-Verzeichnis nicht gefunden. Überspringe Desktop-Build.")
        else:
            # Hinweis aus README: -j 1 unter Windows, um File-Locks zu vermeiden
            # Zusätzlich: separates TARGET-Verzeichnis, um Lock-Kollisionen zu vermeiden
            desktop_target = DESKTOP_TAURI_DIR / "target-tauri"
            extra_env = {"CARGO_TARGET_DIR": str(desktop_target)}
            do(
                "desktop cargo build --release -j 1",
                ["cargo", "build", "--release", "-j", "1"],
                cwd=DESKTOP_TAURI_DIR,
                extra_env=extra_env,
            )

    # 6) Benches (nur Windows sinnvoll entsprechend CI)
    if not args.skip_bench and platform.system().lower().startswith("win"):
        do("bench --no-run", ["cargo", "bench", "--no-run"], cwd=REPO_ROOT)

    return summarize_and_exit(results, exit_only=False)


def summarize_and_exit(results: List[StepResult], exit_only: bool = True) -> int:
    print("\n================ SUMMARY ================")
    ok_all = True
    total_time = 0.0
    for res in results:
        total_time += res.duration_s
        status = "OK" if res.ok else f"FAIL ({res.returncode})"
        print(f"- {res.name:<32} : {status:>10}  [{res.duration_s:.1f}s]")
        ok_all = ok_all and res.ok
    print(f"----------------------------------------\nTotal: {total_time:.1f}s\nResult: {'SUCCESS' if ok_all else 'FAILURE'}\n")
    code = 0 if ok_all else 1
    if exit_only:
        sys.exit(code)
    return code


def parse_args(argv: Optional[List[str]] = None) -> argparse.Namespace:
    p = argparse.ArgumentParser(description="Lokale CI-ähnliche Verifikation für SpeicherWald")
    p.add_argument("--skip-lints", action="store_true", help="fmt/clippy überspringen")
    p.add_argument("--skip-tests", action="store_true", help="cargo test überspringen")
    p.add_argument("--skip-build", action="store_true", help="cargo build --release überspringen")
    p.add_argument("--skip-ui", action="store_true", help="UI-Build (Trunk) überspringen")
    p.add_argument("--skip-desktop", action="store_true", help="Desktop-Build (Tauri) überspringen")
    p.add_argument("--skip-bench", action="store_true", help="Benches überspringen (Windows)")
    p.add_argument("--ensure-deps", action="store_true", help="fehlende wasm-Target/Trunk automatisch installieren")
    p.add_argument("--keep-going", action="store_true", help="bei Fehlern weitere Schritte trotzdem ausführen")
    p.add_argument("--quiet", action="store_true", help="Befehle nicht ausgeben")
    return p.parse_args(argv)


def main(argv: Optional[List[str]] = None) -> int:
    args = parse_args(argv)
    ensure_base_tools()
    return run_steps(args)


if __name__ == "__main__":
    sys.exit(main())
