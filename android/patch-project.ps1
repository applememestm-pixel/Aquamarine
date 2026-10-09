# Brings the generated Tauri Android project (crates/aqua-client-ui/gen/android)
# up to what Aquamarine needs. Safe to run any number of times.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$proj = Join-Path $root 'crates\aqua-client-ui\gen\android'
$app  = Join-Path $proj 'app'
if (-not (Test-Path $app)) { throw "Android project not found: $proj (run 1-setup.bat)" }

# 1. Permissions for calls (mic, camera), notifications, legacy Downloads write.
$m = Join-Path $app 'src\main\AndroidManifest.xml'
$x = [IO.File]::ReadAllText($m)
if ($x -notmatch 'RECORD_AUDIO') {
  $perms = @(
    '<uses-permission android:name="android.permission.RECORD_AUDIO" />',
    '<uses-permission android:name="android.permission.CAMERA" />',
    '<uses-permission android:name="android.permission.MODIFY_AUDIO_SETTINGS" />',
    '<uses-permission android:name="android.permission.POST_NOTIFICATIONS" />',
    '<uses-permission android:name="android.permission.WRITE_EXTERNAL_STORAGE" android:maxSdkVersion="28" />',
    '<uses-feature android:name="android.hardware.camera" android:required="false" />'
  ) -join "`n    "
  $x = ([regex]'<application').Replace($x, "$perms`n    <application", 1)
  [IO.File]::WriteAllText($m, $x)
  Write-Host '[patch] permissions added to AndroidManifest.xml'
}

# 2. App icon.
$icons = Join-Path $root 'crates\aqua-client-ui\icons\android'
if (Test-Path $icons) {
  Copy-Item -Path (Join-Path $icons '*') -Destination (Join-Path $app 'src\main\res') -Recurse -Force
  Write-Host '[patch] launcher icons copied'
}

# 3. Gradle must call the Rust build through cargo's Tauri CLI.
Get-ChildItem -Path (Join-Path $proj 'buildSrc') -Recurse -Filter BuildTask.kt | ForEach-Object {
  $t = [IO.File]::ReadAllText($_.FullName)
  if ($t -match '"""node"""') {
    [IO.File]::WriteAllText($_.FullName, $t.Replace('"""node"""', '"""cargo"""'))
    Write-Host '[patch] BuildTask now uses cargo'
  }
}
Write-Host '[patch] done'
