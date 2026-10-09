# Start the aqua-server (relay) on 127.0.0.1:7777. Leave this window
# open — both clients talk to it. Ctrl+C stops it.
#
# Usage (from the project root):
#     .\run-server.ps1

$ErrorActionPreference = "Stop"

Write-Host "aqua-server starting on http://127.0.0.1:7777 ..." -ForegroundColor Cyan
Write-Host "Press Ctrl+C to stop." -ForegroundColor DarkGray

cargo run -p aqua-server
