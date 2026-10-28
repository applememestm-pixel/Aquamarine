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
    '<uses-permission android:name="android.permission.ACCESS_LOCAL_NETWORK" />',
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

# 1b. Android 17 local-network permission for projects patched before it existed.
$x = [IO.File]::ReadAllText($m)
if ($x -notmatch 'ACCESS_LOCAL_NETWORK') {
  $x = ([regex]'<uses-permission android:name="android.permission.RECORD_AUDIO" />').Replace($x, "<uses-permission android:name=`"android.permission.ACCESS_LOCAL_NETWORK`" />`n    <uses-permission android:name=`"android.permission.RECORD_AUDIO`" />", 1)
  [IO.File]::WriteAllText($m, $x)
  Write-Host '[patch] ACCESS_LOCAL_NETWORK added'
}
# 1d. Plain HTTP to the app's own loopback media server (audio / video
#     playback); debug builds also to the dev server.
$xmlMain = Join-Path $app 'src\main\res\xml'; $xmlDebug = Join-Path $app 'src\debug\res\xml'
New-Item -ItemType Directory -Force $xmlMain, $xmlDebug | Out-Null
Copy-Item (Join-Path $PSScriptRoot 'netsec\release.xml') (Join-Path $xmlMain 'network_security_config.xml') -Force
Copy-Item (Join-Path $PSScriptRoot 'netsec\debug.xml') (Join-Path $xmlDebug 'network_security_config.xml') -Force
$mt = Get-Content $m -Raw
if ($mt -notmatch 'networkSecurityConfig') {
  $mt = $mt -replace '<application', '<application android:networkSecurityConfig="@xml/network_security_config"'
  Set-Content -Path $m -Value $mt -NoNewline -Encoding UTF8
  Write-Host '[patch] network security config (loopback media server)'
}

# 1c. MainActivity that asks for local-network access on Android 17+.
$ma = Get-ChildItem -Path (Join-Path $app 'src\main\java') -Recurse -Filter MainActivity.kt | Select-Object -First 1
if ($ma) {
  Copy-Item (Join-Path $PSScriptRoot 'MainActivity.kt') $ma.FullName -Force
  Write-Host '[patch] MainActivity requests local network access'
}
# 1c'. Nearby delivery over Bluetooth (crates\aqua-client-ui\src\mesh_android.rs).
if ($ma) {
  Copy-Item (Join-Path $PSScriptRoot 'AquaMesh.kt') (Join-Path $ma.DirectoryName 'AquaMesh.kt') -Force
  Write-Host '[patch] AquaMesh.kt (Bluetooth nearby delivery)'
  # Server on this device (crates\aqua-client-ui\src\relay_host.rs).
  Copy-Item (Join-Path $PSScriptRoot 'AquaRelay.kt') (Join-Path $ma.DirectoryName 'AquaRelay.kt') -Force
  Write-Host '[patch] AquaRelay.kt (server on this device)'
  # Home-screen widget (android\widget).
  Copy-Item (Join-Path $PSScriptRoot 'widget\AquaWidget.kt') (Join-Path $ma.DirectoryName 'AquaWidget.kt') -Force
  Copy-Item (Join-Path $PSScriptRoot 'widget\res\*') (Join-Path $app 'src\main\res') -Recurse -Force
  Write-Host '[patch] AquaWidget.kt (home-screen widget)'
}
$x = [IO.File]::ReadAllText($m)
if ($x -notmatch 'AquaWidget') {
  $x = $x.Replace('</application>', '<receiver android:name=".AquaWidget" android:exported="false"><intent-filter><action android:name="android.appwidget.action.APPWIDGET_UPDATE" /></intent-filter><meta-data android:name="android.appwidget.provider" android:resource="@xml/aqua_widget_info" /></receiver>' + "`n    </application>")
  [IO.File]::WriteAllText($m, $x)
  Write-Host '[patch] home-screen widget receiver added'
}
$x = [IO.File]::ReadAllText($m)
if ($x -notmatch 'AquaRelayService') {
  $fgs = '<uses-permission android:name="android.permission.FOREGROUND_SERVICE" />' + "`n    " +
         '<uses-permission android:name="android.permission.FOREGROUND_SERVICE_SPECIAL_USE" />' + "`n    " +
         '<uses-permission android:name="android.permission.WAKE_LOCK" />' + "`n    " +
         '<uses-permission android:name="android.permission.READ_MEDIA_IMAGES" />' + "`n    " +
         '<uses-permission android:name="android.permission.READ_MEDIA_VIDEO" />' + "`n    " +
         '<uses-permission android:name="android.permission.READ_MEDIA_VISUAL_USER_SELECTED" />' + "`n    " +
         '<uses-permission android:name="android.permission.READ_EXTERNAL_STORAGE" android:maxSdkVersion="32" />' + "`n    <application"
  $x = ([regex]'<application').Replace($x, $fgs, 1)
  $svc = '<service android:name=".AquaRelayService" android:exported="false" android:foregroundServiceType="specialUse"><property android:name="android.app.PROPERTY_SPECIAL_USE_FGS_SUBTYPE" android:value="Hosts the Aquamarine relay server for the user and their contacts" /></service>' + "`n    </application>"
  $x = $x.Replace('</application>', $svc)
  [IO.File]::WriteAllText($m, $x)
  Write-Host '[patch] foreground service for the server added'
}
# In-app updates (crates/aqua-client-ui/src/updates.rs): installing the
# downloaded APK needs this permission.
if ($x -notmatch 'REQUEST_INSTALL_PACKAGES') {
  $x = ([regex]'<application').Replace($x, '<uses-permission android:name="android.permission.REQUEST_INSTALL_PACKAGES" />' + "`n    <application", 1)
  [IO.File]::WriteAllText($m, $x)
  Write-Host '[patch] REQUEST_INSTALL_PACKAGES (in-app updates)'
}
if ($x -notmatch 'BLUETOOTH_CONNECT') {
  $bt = @(
    '<uses-permission android:name="android.permission.BLUETOOTH" android:maxSdkVersion="30" />',
    '<uses-permission android:name="android.permission.BLUETOOTH_ADMIN" android:maxSdkVersion="30" />',
    '<uses-permission android:name="android.permission.ACCESS_FINE_LOCATION" android:maxSdkVersion="30" />',
    '<uses-permission android:name="android.permission.BLUETOOTH_CONNECT" />',
    '<uses-permission android:name="android.permission.BLUETOOTH_SCAN" android:usesPermissionFlags="neverForLocation" />',
    '<uses-feature android:name="android.hardware.bluetooth" android:required="false" />'
  ) -join "`n    "
  $x = ([regex]'<application').Replace($x, "$bt`n    <application", 1)
  [IO.File]::WriteAllText($m, $x)
  Write-Host '[patch] Bluetooth permissions added'
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
# 4. Android Studio default build variant: "universal" with only the ABIs that
#    matter (phones = arm64, emulator = x86_64) instead of the alphabetical
#    first one ("arm" = 32-bit only, which no modern phone/emulator wants).
$gp = Join-Path $proj 'gradle.properties'
$g = [IO.File]::ReadAllText($gp)
if ($g -notmatch '(?m)^abiList=') {
  $g = $g.TrimEnd() + "`n`n# Aquamarine: ABIs built when running from Android Studio`nabiList=arm64-v8a,x86_64`narchList=arm64,x86_64`ntargetList=aarch64,x86_64`n"
  [IO.File]::WriteAllText($gp, $g)
  Write-Host '[patch] Studio builds arm64 + x86_64'
}
$bg = Join-Path $app 'build.gradle.kts'
$b = [IO.File]::ReadAllText($bg)
if ($b -notmatch 'aquamarine-default-variant') {
  $b = $b.TrimEnd() + "`n`n// aquamarine-default-variant`nandroid {`n    productFlavors {`n        getByName(`"universal`") { isDefault = true }`n    }`n}`n"
  [IO.File]::WriteAllText($bg, $b)
  Write-Host '[patch] default build variant: universalDebug'
}

# 5. Rust -> Android linkers from the NDK. Without this cargo falls back to
#    `cc` ("linker `cc` not found") when Android Studio builds the Rust part.
if ($env:NDK_HOME) {
  $bin = (Join-Path $env:NDK_HOME 'toolchains\llvm\prebuilt\windows-x86_64\bin').Replace('\', '/')
  if (Test-Path $bin) {
    $api = 24   # = minSdk in app/build.gradle.kts
    $cfg = @(
      '# Generated by android/patch-project.ps1 for this PC (NDK paths). Not in git.',
      '[target.aarch64-linux-android]',
      "linker = `"$bin/aarch64-linux-android$api-clang.cmd`"",
      '[target.armv7-linux-androideabi]',
      "linker = `"$bin/armv7a-linux-androideabi$api-clang.cmd`"",
      '[target.i686-linux-android]',
      "linker = `"$bin/i686-linux-android$api-clang.cmd`"",
      '[target.x86_64-linux-android]',
      "linker = `"$bin/x86_64-linux-android$api-clang.cmd`""
    ) -join "`n"
    $cargoDir = Join-Path $root 'crates\aqua-client-ui\.cargo'
    New-Item -ItemType Directory -Force $cargoDir | Out-Null
    [IO.File]::WriteAllText((Join-Path $cargoDir 'config.toml'), $cfg + "`n")
    Write-Host '[patch] .cargo/config.toml -> NDK linkers'
  }
}

# 6. Tell Gradle / Android Studio where the SDK is.
if ($env:ANDROID_HOME) {
  $lp = Join-Path $proj 'local.properties'
  $sdkEsc = $env:ANDROID_HOME.Replace('\', '\\').Replace(':', '\:')
  [IO.File]::WriteAllText($lp, "sdk.dir=$sdkEsc`n")
  Write-Host "[patch] local.properties -> $($env:ANDROID_HOME)"
}
Write-Host '[patch] done'
