# Start the aqua-server (relay). Leave this window open - clients talk to it.
# Ctrl+C stops it.
#
# Usage (from the project root):
#     .\run-server.ps1          # only this PC (+ Android emulator via 10.0.2.2)
#     .\run-server.ps1 -Lan     # also phones on the same Wi-Fi

param([switch]$Lan)
$ErrorActionPreference = "Stop"

if ($Lan) {
    $env:AQUA_LISTEN = "0.0.0.0:7777"
    Write-Host "aqua-server starting for the whole local network (port 7777)" -ForegroundColor Cyan
    $ips = Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue |
        Where-Object { $_.IPAddress -match '^(192\.168\.|10\.|172\.(1[6-9]|2[0-9]|3[01])\.)' -and
                       $_.InterfaceAlias -notmatch 'Radmin|Hiddify|VPN|vEthernet|WSL|Docker|Loopback|TAP|TUN|Wintun' } |
        Select-Object -ExpandProperty IPAddress
    foreach ($ip in $ips) { Write-Host "  phone: http://${ip}:7777" -ForegroundColor Green }
    Write-Host "  emulator: http://10.0.2.2:7777" -ForegroundColor Green
    Write-Host "If Windows asks about network access for aqua-server - allow Private networks." -ForegroundColor DarkGray
} else {
    Write-Host "aqua-server starting on http://127.0.0.1:7777 ..." -ForegroundColor Cyan
    Write-Host "  (Android emulator: http://10.0.2.2:7777 ; for a phone run .\run-server.ps1 -Lan)" -ForegroundColor DarkGray
}
$env:AQUA_OPEN_PANEL = "1"   # server opens its admin panel in the browser
Write-Host "Press Ctrl+C to stop." -ForegroundColor DarkGray

cargo run -p aqua-server
