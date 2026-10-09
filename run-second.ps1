# Launch a SECOND Aquamarine client with its own vault, so you can
# chat with yourself from one window to another on the same machine.
#
# IMPORTANT: run .\run-first.ps1 FIRST and wait for its window to
# appear. The first instance starts the Vite dev server + builds the
# Rust binary; this script just launches that already-built .exe
# with a different AQUA_DATA_DIR. (Running `cargo tauri dev` twice
# in parallel fails: both lock the target/ dir and fight for Vite's
# port 5173.)
#
# Usage (from the project root):
#     .\run-second.ps1

$ErrorActionPreference = "Stop"

$projectRoot = $PSScriptRoot
$clientDir   = Join-Path $projectRoot "crates\aqua-client-ui"
$exe         = Join-Path $clientDir   "target\debug\aqua-client-ui.exe"
$dataDir     = Join-Path $projectRoot "second-instance-data"

if (-not (Test-Path $exe)) {
    Write-Host ""
    Write-Host "The compiled client binary is not there yet:" -ForegroundColor Yellow
    Write-Host "  $exe" -ForegroundColor DarkGray
    Write-Host ""
    Write-Host "Launch .\run-first.ps1 first and wait for the first Aquamarine window to open." -ForegroundColor Yellow
    Write-Host "Then come back here and run .\run-second.ps1 again." -ForegroundColor Yellow
    Write-Host ""
    Write-Host "(If you'd rather build without opening the first window, run:" -ForegroundColor DarkGray
    Write-Host "   cd $clientDir ; cargo build" -ForegroundColor DarkGray
    Write-Host " and then .\run-second.ps1 — but you still need Vite dev running on :5173.)" -ForegroundColor DarkGray
    exit 1
}

if (-not (Test-Path $dataDir)) {
    New-Item -ItemType Directory -Path $dataDir | Out-Null
}

$env:AQUA_DATA_DIR = $dataDir
Write-Host ""
Write-Host "Launching SECOND Aquamarine instance" -ForegroundColor Cyan
Write-Host "  vault:  $dataDir" -ForegroundColor DarkGray
Write-Host "  binary: $exe" -ForegroundColor DarkGray
Write-Host "  reusing Vite dev server from the first instance." -ForegroundColor DarkGray
Write-Host ""

& $exe
