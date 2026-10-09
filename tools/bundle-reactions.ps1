# Bake the "eltaller47" reaction pack into the app build so every copy of
# Aquamarine has it - no Telegram token needed on other PCs.
#
# 1. In Aquamarine: Emoji panel -> "+" -> "Get El Taller reactions" (once).
# 2. Run this script (or bundle-reactions.bat).
# 3. Rebuild the app.
param(
  [string]$Repo = (Split-Path -Parent $PSScriptRoot),
  [string]$Pack = "eltaller47"
)
$ErrorActionPreference = "Stop"
$data = if ($env:AQUA_DATA_DIR) { $env:AQUA_DATA_DIR } else { Join-Path $env:APPDATA "app.aquamarine.desktop" }
$stickers = Join-Path $data "stickers"
if (-not (Test-Path $stickers)) { Write-Host "No sticker library at $stickers - start Aquamarine once first."; exit 1 }

$src = Get-ChildItem $stickers -Directory | Where-Object {
  $meta = Join-Path $_.FullName "pack.json"
  (Test-Path $meta) -and (([System.IO.File]::ReadAllText($meta) | ConvertFrom-Json).name -eq $Pack)
} | Select-Object -First 1
if (-not $src) {
  Write-Host "Pack '$Pack' is not imported yet."
  Write-Host "In Aquamarine open the emoji panel -> '+' -> 'Get El Taller reactions', then run this again."
  exit 1
}

$dest = Join-Path $Repo "crates\aqua-client-ui\ui\public\reactions\$Pack"
New-Item -ItemType Directory -Force $dest | Out-Null
Get-ChildItem $dest -File | Remove-Item -Force

$mimes = @{ ".tgs" = "application/x-tgsticker"; ".webm" = "video/webm"; ".webp" = "image/webp"; ".png" = "image/png"; ".gif" = "image/gif" }
$files = @()
Get-ChildItem $src.FullName -File | Where-Object { $mimes.ContainsKey($_.Extension.ToLower()) } | ForEach-Object {
  Copy-Item -LiteralPath $_.FullName -Destination $dest
  $name = $_.Name
  $emoji = if ($name.Contains("_")) { $name.Substring(0, $name.IndexOf("_")) } else { "" }
  $files += [pscustomobject]@{ file = $name; emoji = $emoji; mime = $mimes[$_.Extension.ToLower()] }
}
$json = [pscustomobject]@{ pack = $Pack; title = $src.Name; files = @($files) } | ConvertTo-Json -Depth 4
[System.IO.File]::WriteAllText((Join-Path $dest "manifest.json"), $json, (New-Object System.Text.UTF8Encoding $false))
Write-Host "Bundled $($files.Count) reactions from '$($src.Name)' into:"
Write-Host "  $dest"
Write-Host "Now rebuild Aquamarine - the pack ships with the app."
