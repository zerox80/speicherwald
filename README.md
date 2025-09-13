# SpeicherWald 🌲

> A high-performance disk space analyzer for Windows

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=flat&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Windows](https://img.shields.io/badge/Windows-0078D6?style=flat&logo=windows&logoColor=white)](https://www.microsoft.com/windows)

SpeicherWald is a powerful, open-source disk space analyzer built with Rust and modern web technologies. It provides fast directory size analysis for both local and network drives on Windows systems.

## 🌟 Highlights

- **⚡ Lightning Fast**: Multi-threaded scanning with intelligent caching
- **🎯 Accurate**: Measures both logical and allocated disk space
- **🌐 Web-Based UI**: Modern, responsive interface using Dioxus/WASM
- **🖥️ Desktop App**: Native experience via Tauri
- **📊 Real-time Updates**: Live progress tracking with Server-Sent Events
- **💾 Persistent Storage**: SQLite database for scan history
- **🔍 Smart Filtering**: Exclude patterns, hidden files handling

## 📋 System Requirements

- **OS**: Windows 11, Windows Server 2019 or later
- **Runtime**: Microsoft Edge WebView2 (for desktop app)
- **Network**: Supports already-connected UNC paths (no credential management in v0.1)

## 🎯 Target Audience

System administrators and power users who need to quickly identify storage-intensive directories and manage disk space efficiently.

---

## Deutsche Beschreibung

Axum-basiertes Open-Source-Backend (Rust) zur Größenanalyse von Verzeichnissen (lokal und Netzwerk/UNC) auf Windows 11 und Windows Server 2019 – mit minimalistischer Dioxus-Web-UI (WASM) und Desktop-App via Tauri. Web und Desktop nutzen dieselbe HTTP-API über `localhost`.

- Lizenz: GPLv3
- Sprache: Deutsch (API-Feldnamen Englisch)
- Plattform: Windows 11, Windows Server 2019
- Netzlaufwerke: Es werden nur bereits verbundene/zugängliche UNC-Pfade unterstützt (keine Credential-Verwaltung in v1)

Zielgruppe: Admins, die schnell einen Überblick über Laufwerke erhalten und speicherintensive Ordner identifizieren möchten. Fokus: Performance, Stabilität, klare UI.

## ✨ Features v0.1
- Scans für lokale Laufwerke und zugängliche UNC-Pfade
- Messwerte: logische Größe und belegter Speicher (Cluster-basiert via `GetCompressedFileSizeW`)
- Optionen: `follow_symlinks` (default false), `include_hidden` (default true), `excludes` (Glob), `max_depth`, `concurrency`
- Persistenz: SQLite (Scans + Metadaten)
- Streaming: Server-Sent Events (SSE) für Fortschritt/Warnungen/Fertigstellung
- Endpunkte für Laufwerksübersicht (`/drives`), Baum-Abfrage (`/scans/{id}/tree`) und Top-N (`/scans/{id}/top`)
- Statische Web-UI (Dioxus) wird vom Backend unter `/` ausgeliefert (SPA-Fallback)

## 📁 Project Structure
- Backend (Axum): `src/`, Einstieg `src/main.rs`
- Web-UI (Dioxus, WASM): `webui/` mit Build-Ausgabe nach `ui/` (siehe `webui/Trunk.toml`)
- Statische UI-Artefakte: `ui/` (vom Backend via `ServeDir` ausgeliefert)
- Desktop (Tauri): `desktop/src-tauri/`, Einstieg `desktop/src-tauri/src/main.rs`
- Defaults: `config/default.toml`
- Installer-Skripte: `scripts/`

## 🚀 Quick Start
Voraussetzungen: Rust (stable), Cargo. Für die UI wird Trunk benötigt (wird vom Installer automatisch installiert; für manuellen Build siehe unten).

```powershell
# Release-Build des Backends
cargo build --release

# Server starten (Host/Port via Config oder Env)
./target/release/speicherwald.exe

# Web-UI öffnen (vom Backend ausgeliefert)
start http://localhost:8080/
```

Hinweis: Standard-Host/Port sind `127.0.0.1:8080` (siehe `config/default.toml`).

## 📦 Installation (Recommended)
Die Skripte bauen Backend, Web-UI (Dioxus/Trunk) und Desktop (Tauri) und kopieren die Artefakte an den Zielort. WebView2 wird geprüft und bei Bedarf installiert.

- Benutzerinstallation (keine Adminrechte, nach `%LocalAppData%\Programs\SpeicherWald`):
  - `scripts\install_user.cmd`
- Admin-Installation (z. B. nach `%ProgramFiles%\Speicherwald`):
  - In einer Administrator-Eingabeaufforderung: `scripts\install_admin.cmd`

Nach erfolgreicher Installation kann die Desktop-App über `SpeicherWald.exe` im Installationsverzeichnis gestartet werden. Die Desktop-App startet den lokalen HTTP-Server automatisch auf einem freien Port und öffnet die UI.

## 🔨 Manual Builds
### Web-UI (Dioxus/WASM)
Die Web-UI wird per Trunk gebaut und direkt nach `ui/` ausgegeben (siehe `webui/Trunk.toml`).

```powershell
# Voraussetzungen
rustup target add wasm32-unknown-unknown
cargo install trunk --locked

# Build (Release) – Artefakte landen in ../ui
cd webui
trunk build --release
```

Hinweis zu wasm-opt/Validator-Flags:
- Die für den wasm-Validator benötigten WASM-Feature-Flags werden über die Trunk-Asset-Direktiven in `webui/index.html` gesetzt.
- Konkret: `data-wasm-opt="z"` (Größen-Optimierung) und `data-wasm-opt-params="--enable-bulk-memory --enable-nontrapping-float-to-int"` sind bereits hinterlegt.
- Vorteil: Andere Nutzer können einfach `trunk build --release` ausführen – ohne zusätzliche Umgebungsvariablen.
- Umschalten der Optimierung: In `webui/index.html` den Wert von `data-wasm-opt` anpassen (`1|2|3|4` für Speed, `s|z` für Größe) oder zum Deaktivieren `data-wasm-opt="0"` setzen.

Für Live-Entwicklung kannst du z. B. nutzen:

```powershell
cd webui
trunk watch --release    # baut bei Änderungen fortlaufend nach ../ui
# In zweitem Terminal das Backend starten, welches / (ui) ausliefert
cargo run
```

### Backend (Axum)
```powershell
# Entwicklung
cargo run

# Produktion
cargo build --release
```

### Desktop (Tauri)
```powershell
# Release-Build der Tauri-App
cd desktop/src-tauri
# Wichtiger Hinweis: Unter Windows ggf. mit einem Job (-j 1) bauen, um File-Locks zu vermeiden
cargo build --release -j 1

# Ergebnis (typisch):
# desktop/src-tauri/target/release/speicherwald-desktop.exe
```

Die Desktop-App (`desktop/src-tauri/src/main.rs`) sucht beim Start das Backend-Binary (z. B. `speicherwald.exe`), startet es auf einem freien Port (`127.0.0.1:<port>`) und öffnet dann das Fenster mit der Web-UI. Für verpackte Releases wird die UI und das Backend zusammen ausgeliefert. WebView2 (Edge) wird benötigt.

## 🐳 Docker/Compose Quick Start

Schnellstart mit Docker (UI wird im Image mit ausgeliefert):

```bash
# Image bauen
docker build -t speicherwald:latest .

# Container starten (DB unter ./data, Port 8080)
docker run --rm -p 8080:8080 -v %cd%/data:/app/data speicherwald:latest

# Öffnen
start http://localhost:8080/
```

Mit Docker Compose (empfohlen während der Entwicklung):

```bash
docker compose up -d
start http://localhost:8080/
```

Hinweise:

- Volumes: Standardmäßig wird die SQLite-DB unter `./data` persistiert.
- Ports: Der Container exponiert Port `8080`.
- Env: Konfiguration via `SPEICHERWALD__*` (siehe unten) oder `docker-compose.yml`.

## ⚙️ Configuration
Reihenfolge (höchste Priorität zuerst):
- Umgebungsvariablen mit Prefix `SPEICHERWALD__` (z. B. `SPEICHERWALD__SERVER__PORT=9090`)
- Optional: spezifische Datei via `SPEICHERWALD_CONFIG` (Pfad ohne Erweiterung; z. B. `C:/cfg/prod` lädt `prod.toml`)
- Lokale Datei `speicherwald.toml` im Arbeitsverzeichnis (optional)
- Eingebettete Defaults aus `config/default.toml` (im Binary mitkompiliert)

Beispiele (PowerShell):

```powershell
# Port per Env-Var
$env:SPEICHERWALD__SERVER__PORT = "9090"
# Host per Env-Var (Standard 127.0.0.1)
$env:SPEICHERWALD__SERVER__HOST = "127.0.0.1"

# SQLite-URL per Env-Var
$env:SPEICHERWALD__DATABASE__URL = "sqlite://data/dev.db"

# Scan-Defaults (für POST /scans, wenn Felder weggelassen werden)
$env:SPEICHERWALD__SCAN_DEFAULTS__EXCLUDES = '["**/target","**/.git"]'

# Alternative Config-Datei verwenden
$env:SPEICHERWALD_CONFIG = "C:/pfad/zu/prod"  # lädt z. B. prod.toml

Standard-Defaults (siehe `config/default.toml`):

```toml
[server]
host = "127.0.0.1"
port = 8080

[database]
url = "sqlite://data/speicherwald.db"

[scan_defaults]
follow_symlinks = false
include_hidden = true
measure_logical = true
measure_allocated = true
excludes = []

[scanner]
batch_size = 4000
flush_threshold = 8000
flush_interval_ms = 750
dir_concurrency = 12
# handle_limit optional – weglassen bedeutet kein explizites Limit
#handle_limit = 2048
```

Besonderheit Desktop: Die Desktop-App setzt zur Laufzeit die Datenbank in ein benutzerbeschreibbares Verzeichnis (`%LocalAppData%\SpeicherWald\speicherwald.db`) via Env-Var `SPEICHERWALD__DATABASE__URL`, um Schreibrechte-Probleme zu vermeiden.

## 🔒 Rate Limiting
Das Backend schützt sich vor Missbrauch durch Rate Limiting:

- Globale Begrenzung (konfigurierbar via Umgebungsvariablen):
  - `SPEICHERWALD_RATE_LIMIT_MAX_REQUESTS` (Standard: `100`)
  - `SPEICHERWALD_RATE_LIMIT_WINDOW_SECONDS` (Standard: `60`)
  - Gilt IP-basiert über alle Endpunkte (Header `X-Forwarded-For`/`X-Real-IP` werden respektiert).
- Per-Endpunkt-Limits (fest verdrahtet in `src/state.rs`):
  - `POST /scans`: 10/minute/IP
  - `GET /scans/:id/search`: 30/minute/IP
  - `GET /drives`: 20/minute/IP

Hinweis: Alte Einträge werden periodisch bereinigt (alle 5 Minuten), um Speicherverbrauch gering zu halten.

### UI & Pagination Verhalten
- Die Web-UI reduziert automatische Aktualisierungen (SSE-getriggerte Tabellen-Reloads) auf ca. alle 5 Sekunden, um unnötige API-Last zu vermeiden.
- Die Explorer-(Liste)-Pagination ist defensiv:
  - „Vorherige Seite“ ist deaktiviert, wenn `offset == 0` oder während eines Ladevorgangs.
  - „Nächste Seite“ ist nur aktiv, wenn die letzte Abfrage mindestens so viele Einträge wie das `limit` geliefert hat (Heuristik „es gibt vermutlich noch mehr“), und wird während des Ladens deaktiviert.
  - Beim Navigieren in einen neuen Pfad wird `offset` automatisch auf `0` zurückgesetzt.
  - Während eines Ladevorgangs werden weitere gleichzeitige Abfragen übersprungen.

Wenn trotzdem `429 Too Many Requests` auftritt (z. B. bei schneller manueller Navigation), bitte den in der Fehlermeldung genannten `retry_after_seconds` abwarten und anschließend erneut versuchen.

## 💡 Usage
- Laufwerksüberblick unter `GET /drives` und auf der Startseite der Web-UI
- Scans können aus der Web-UI („Neuer Scan“) oder per `POST /scans` gestartet werden
- Während eines Scans liefert `GET /scans/{id}/events` Fortschritt und Warnungen (SSE). Die UI zeigt dies live an.
- Nach Abschluss stehen Baum- und Top-N-Ansichten zur Verfügung (`/tree`, `/top`)

## 🔌 API Reference

### Health & Monitoring
- `GET /healthz` — Liveness check
- `GET /readyz` — Readiness check (validates DB connectivity)
- `GET /metrics` — JSON metrics snapshot (scans, files, directories, bytes, uptime)
- `GET /metrics/prometheus` — Prometheus-compatible metrics
- `GET /version` — Build- und Paketinformationen (Name, Version, Build-Profil, OS/Arch)

### Drive Management
- `GET /drives` — List all available drives with capacity information

### Scan Operations
- `POST /scans` — Start a new scan
- `GET /scans` — List all scans
- `GET /scans/{id}` — Get scan details
- `DELETE /scans/{id}` — Cancel running scan
  - Optional: `?purge=true` — Delete scan and all associated data
- `GET /scans/{id}/events` — SSE stream for real-time progress

### Data Queries
- `GET /scans/{id}/tree` — Query directory tree
  - Query params: `path`, `depth`, `sort`, `limit`
- `GET /scans/{id}/top` — Get largest items
  - Query params: `scope=dirs|files`, `limit`
- `GET /scans/{id}/list` — List directory contents
  - Query params: `path`, `sort`, `order`, `limit`, `offset`
- `GET /scans/{id}/search` — Search within scan results
  - Query params: `query`, `path`, `type`, `min_size`, `max_size`

### Export
- `GET /scans/{id}/export` — Export scan data
  - Query params: `format=csv|json`, `scope=nodes|files|all`, `limit`
- `GET /scans/{id}/statistics` — Get detailed scan statistics

### Beispiel: Scan starten
```bash
curl -X POST http://localhost:8080/scans \
  -H "Content-Type: application/json" \
  -d '{
        "root_paths": ["C:/Users", "\\\\server\\share"],
        "follow_symlinks": false,
        "include_hidden": true,
        "measure_logical": true,
        "measure_allocated": true,
        "excludes": ["**/.git", "**/node_modules"],
        "max_depth": null,
        "concurrency": null
      }'
```

### SSE-Stream öffnen
```bash
curl -N http://localhost:8080/scans/<SCAN_ID>/events
```

### Beispiel: Teilbaum laden
```bash
curl "http://localhost:8080/scans/<SCAN_ID>/tree?path=C:/Users&depth=2&sort=size&limit=200"
```

### Beispiel: Top-N laden
```bash
curl "http://localhost:8080/scans/<SCAN_ID>/top?scope=dirs&limit=100"
```

## 🔧 Troubleshooting
- WebView2 fehlt: Installiere die Microsoft Edge WebView2 Runtime. Der Installer (`scripts/install*.cmd` → `install.ps1`) versucht dies automatisch. Manuell: https://developer.microsoft.com/microsoft-edge/webview2/
- Port belegt: Setze `SPEICHERWALD__SERVER__PORT` auf einen freien Port oder passe `speicherwald.toml` an. Die Desktop-App wählt automatisch einen freien Port.
- Schreibrechte/DB: Beim Start im Arbeitsverzeichnis wird die SQLite-DB unter `data/` angelegt. Die Desktop-App nutzt `%LocalAppData%\SpeicherWald\speicherwald.db`.
- Lange Pfade: Pfade mit `\\?\`-Präfix werden soweit möglich unterstützt.
- Reparse Points / Symlinks: Standard wird nicht gefolgt; optional per Flag (`follow_symlinks`) aktivierbar.
- Hidden/System: Standard einbeziehen; per Flag (`include_hidden`) deaktivierbar.
- UNC-Pfade: Es werden nur bereits verbundene/zugängliche Ressourcen gescannt (keine Credential-Verwaltung in v1).


## 🔒 Security
- Keine Speicherung von Zugangsdaten in v0.1. Es werden nur bereits verbundene Ressourcen gescannt.

## 🤝 Contributing
Feedback und PRs sind willkommen. Bitte beachte Code-Stil, Tests (falls relevant) und die Projektziele (Performance, Stabilität, minimalistische UI).

## 📄 License
GPLv3 – siehe `LICENSE`. Stelle sicher, dass in Releases der vollständige Lizenztext enthalten ist.
