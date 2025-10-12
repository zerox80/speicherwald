#!/usr/bin/env python3
"""
Entwicklungs-Launcher für SpeicherWald: Startet Backend + öffnet Browser

Usage:
  python scripts/run_dev.py [--port PORT] [--no-browser]
"""

import argparse
import os
import subprocess
import sys
import time
import webbrowser
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
EXE_NAME = "speicherwald.exe" if os.name == "nt" else "speicherwald"


def check_backend_ready(port: int = 8080, timeout: int = 30) -> bool:
    """Wartet darauf, dass das Backend bereit ist"""
    try:
        import urllib.request
    except ImportError:
        print("urllib nicht verfügbar, überspringe Health-Check")
        return True
    
    url = f"http://127.0.0.1:{port}/healthz"
    start_time = time.time()
    
    print(f"Warte auf Backend auf Port {port}...", end="", flush=True)
    while time.time() - start_time < timeout:
        try:
            with urllib.request.urlopen(url, timeout=1) as response:
                if response.status == 200:
                    print(" ✓ Backend ist bereit!")
                    return True
        except Exception:
            pass
        print(".", end="", flush=True)
        time.sleep(0.5)
    
    print(" ✗ Timeout!")
    return False


def main():
    parser = argparse.ArgumentParser(description="Startet SpeicherWald Backend und öffnet Browser")
    parser.add_argument("--port", type=int, default=8080, help="Backend Port (Standard: 8080)")
    parser.add_argument("--no-browser", action="store_true", help="Browser nicht automatisch öffnen")
    parser.add_argument("--release", action="store_true", help="Release-Build verwenden")
    args = parser.parse_args()

    # Finde Backend-Binary
    build_type = "release" if args.release else "debug"
    target_dir = REPO_ROOT / "target" / build_type
    backend_exe = target_dir / EXE_NAME
    
    if not backend_exe.exists():
        print(f"Backend nicht gefunden: {backend_exe}")
        print(f"Bitte erst 'cargo build {'--release' if args.release else ''}' ausführen")
        sys.exit(1)

    # Setze Port per Umgebungsvariable (falls Backend das unterstützt)
    env = os.environ.copy()
    # Alternativ: config-Datei anpassen oder als Argument übergeben

    # Starte Backend
    print(f"\n==> Starte Backend: {backend_exe}")
    backend_process = subprocess.Popen(
        [str(backend_exe)],
        cwd=str(REPO_ROOT),
        env=env
    )

    try:
        # Warte auf Backend
        if not check_backend_ready(args.port):
            print("\nBackend konnte nicht gestartet werden!")
            backend_process.terminate()
            sys.exit(1)

        # Öffne Browser
        url = f"http://127.0.0.1:{args.port}/"
        if not args.no_browser:
            print(f"\n==> Öffne Browser: {url}")
            webbrowser.open(url)
        else:
            print(f"\n==> Backend läuft auf: {url}")

        print("\n✓ SpeicherWald läuft!")
        print("  Drücke Ctrl+C zum Beenden\n")

        # Warte bis der Prozess beendet wird
        backend_process.wait()

    except KeyboardInterrupt:
        print("\n\n==> Beende Backend...")
        backend_process.terminate()
        try:
            backend_process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            backend_process.kill()
        print("✓ Backend beendet")


if __name__ == "__main__":
    main()
