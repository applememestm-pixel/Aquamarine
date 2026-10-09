# Launch the FIRST (default) Aquamarine client instance. Uses the OS
# app-data dir for its vault, so this is the "normal" install.
#
# Usage (from the project root):
#     .\run-first.ps1
#
# For a second instance on the same machine, run .\run-second.ps1
# in another window.

$ErrorActionPreference = "Stop"

Push-Location (Join-Path $PSScriptRoot "crates\aqua-client-ui")
try {
    cargo tauri dev
} finally {
    Pop-Location
}
