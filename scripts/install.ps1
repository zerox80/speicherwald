[CmdletBinding()]
Param(
  [ValidateSet('User','Admin')]
  [string]$Scope = 'User',
  [string]$InstallDir,
  [switch]$IncludeDesktop,
  [switch]$SkipUI,
  [switch]$EnsureDeps
)

$ErrorActionPreference = 'Stop'

function Assert-Admin {
  $current = [Security.Principal.WindowsIdentity]::GetCurrent()
  $principal = New-Object Security.Principal.WindowsPrincipal($current)
  if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Administrative privileges are required for -Scope Admin. Please run PowerShell as Administrator.'
  }
}

function Ensure-Tool($name) {
  if (-not (Get-Command $name -ErrorAction SilentlyContinue)) {
    throw "Required tool '$name' not found in PATH. Please install it first."
  }
}

function Exec($cmd, $cwd) {
  Write-Host "`n==> $cmd (cwd=$cwd)" -ForegroundColor Cyan
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = 'powershell'
  $psi.Arguments = "-NoProfile -ExecutionPolicy Bypass -Command $cmd"
  if ($cwd) { $psi.WorkingDirectory = $cwd }
  $psi.RedirectStandardOutput = $false
  $psi.RedirectStandardError = $false
  $psi.UseShellExecute = $true
  $p = [System.Diagnostics.Process]::Start($psi)
  $p.WaitForExit()
  if ($p.ExitCode -ne 0) { throw "Command failed with exit code $($p.ExitCode): $cmd" }
}

# Resolve paths
$ScriptDir = Split-Path -Parent $PSCommandPath
$RepoRoot  = Split-Path -Parent $ScriptDir
$WebUiDir  = Join-Path $RepoRoot 'webui'
$UiOutDir  = Join-Path $RepoRoot 'ui'
$TargetRel = Join-Path $RepoRoot 'target\release'
$ExeName   = 'speicherwald.exe'

if (-not $InstallDir) {
  if ($Scope -eq 'Admin') {
    $InstallDir = Join-Path ${env:ProgramFiles} 'SpeicherWald'
  } else {
    $InstallDir = Join-Path ${env:LOCALAPPDATA} 'Programs\SpeicherWald'
  }
}

if ($Scope -eq 'Admin') { Assert-Admin }

Write-Host "Installing to: $InstallDir" -ForegroundColor Green

# Prerequisites
Ensure-Tool 'cargo'
Ensure-Tool 'rustc'

if (-not $SkipUI) {
  if ($EnsureDeps) {
    try {
      if (Get-Command rustup -ErrorAction SilentlyContinue) {
        Exec "rustup target add wasm32-unknown-unknown" $RepoRoot
      } else {
        Write-Warning "'rustup' not found; skipping target installation."
      }
    } catch { Write-Warning $_ }

    if (-not (Get-Command trunk -ErrorAction SilentlyContinue)) {
      Write-Host "Installing Trunk (first time only)..." -ForegroundColor Yellow
      Exec "cargo install trunk --locked" $RepoRoot
    }
  }
}

# Build UI (Trunk outputs to ../ui per webui/Trunk.toml)
if (-not $SkipUI) {
  if (Test-Path $WebUiDir) {
    Exec "trunk build --release" $WebUiDir
  } else {
    Write-Warning "webui/ not found. Skipping UI build."
  }
}

# Build backend
Exec "cargo build --release -q" $RepoRoot
$ExePath = Join-Path $TargetRel $ExeName
if (-not (Test-Path $ExePath)) {
  throw "Backend binary not found at $ExePath"
}

# Stage files
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item -Force $ExePath (Join-Path $InstallDir $ExeName)
if (Test-Path $UiOutDir) {
  Copy-Item -Recurse -Force $UiOutDir (Join-Path $InstallDir 'ui')
} else {
  Write-Warning "ui/ directory not found in repo. The server will fall back to embedded build-time assets if available."
}

# Create helper CMD to start server and open browser
$runCmd = @(
  '@echo off',
  'setlocal',
  'cd /d %~dp0',
  'start "" "%~dp0' + $ExeName + '"',
  'timeout /t 1 >nul',
  'start "" http://127.0.0.1:8080/'
) -join "`r`n"
[IO.File]::WriteAllText((Join-Path $InstallDir 'RUN-SpeicherWald.cmd'), $runCmd, [Text.Encoding]::ASCII)

# Optional: Desktop build (Tauri)
if ($IncludeDesktop) {
  $TauriDir = Join-Path $RepoRoot 'desktop\src-tauri'
  if (Test-Path $TauriDir) {
    $env:CARGO_TARGET_DIR = Join-Path $TauriDir 'target-tauri'
    try {
      Exec "cargo build --release -j 1" $TauriDir
      $desktopExe = Join-Path $TauriDir 'target-tauri\release\speicherwald-desktop.exe'
      if (Test-Path $desktopExe) {
        Copy-Item -Force $desktopExe (Join-Path $InstallDir 'speicherwald-desktop.exe')
      } else {
        Write-Warning "Desktop binary not found at $desktopExe"
      }
    } catch {
      Write-Warning "Desktop build failed: $_"
    }
  } else {
    Write-Warning "desktop/src-tauri not found. Skipping desktop build."
  }
}

Write-Host "`nInstallation completed." -ForegroundColor Green
Write-Host "Location: $InstallDir"
Write-Host "Start via: RUN-SpeicherWald.cmd or $ExeName"
Write-Host "Note: The Desktop app requires Microsoft Edge WebView2 Runtime. See README if needed."
