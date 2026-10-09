# Locates Android SDK / NDK / Java for the .bat scripts and prints
# NAME=value lines. -InstallNdk installs the NDK through sdkmanager
# (fetching Android's command-line tools first) when it is missing.
param([switch]$InstallNdk)
$ErrorActionPreference = 'SilentlyContinue'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$NdkPackage = 'ndk;27.2.12479018'
$CmdlineZip = 'https://dl.google.com/android/repository/commandlinetools-win-13114758_latest.zip'

function Test-Sdk($p)  { $p -and (Test-Path (Join-Path $p 'platform-tools')) }
function Test-Java($p) { $p -and (Test-Path (Join-Path $p 'bin\java.exe')) }
$drives = (Get-PSDrive -PSProvider FileSystem).Root

# ---- SDK: env vars, Android Studio's own setting, usual folders, every drive
$sdkCandidates = @($env:ANDROID_HOME, $env:ANDROID_SDK_ROOT, "$env:LOCALAPPDATA\Android\Sdk")
Get-ChildItem "$env:APPDATA\Google\AndroidStudio*\options\other.xml" | Sort-Object LastWriteTime -Descending | ForEach-Object {
  $m = [regex]::Match((Get-Content $_.FullName -Raw), 'android\.sdk\.path.{0,24}?([A-Za-z]:[\\/][^"<&]+)')
  if ($m.Success) { $sdkCandidates += $m.Groups[1].Value.Replace('\\', '\').Trim() }
}
foreach ($d in $drives) { foreach ($s in 'Android\Sdk','Android\sdk','AndroidSDK','Android-SDK','Android') { $sdkCandidates += (Join-Path $d $s) } }
$adb = (Get-Command adb.exe).Source
if ($adb) { $sdkCandidates += (Split-Path (Split-Path $adb)) }
$sdk = $sdkCandidates | Where-Object { Test-Sdk $_ } | Select-Object -First 1
if (-not $sdk) { Write-Output 'ERROR=Android SDK not found. Open Android Studio once: it installs the SDK.'; exit 1 }
$sdk = (Resolve-Path $sdk).Path

# ---- Java: JAVA_HOME, Android Studio's bundled JBR (newest install first)
$studios = foreach ($d in $drives) { foreach ($s in 'AndroidStudio','Android Studio','Program Files\Android\Android Studio','AndroidSt') { Join-Path $d $s } }
$studios = $studios | Where-Object { Test-Path (Join-Path $_ 'build.txt') } | Sort-Object { (Get-Item (Join-Path $_ 'build.txt')).LastWriteTime } -Descending
# Android Gradle Plugin needs JDK 17+: prefer Studio's JBR over a random JAVA_HOME.
$javaCandidates = @($studios | ForEach-Object { Join-Path $_ 'jbr' }) + @($env:JAVA_HOME)
$java = $javaCandidates | Where-Object { Test-Java $_ } | Select-Object -First 1
if (-not $java) { Write-Output 'ERROR=Java not found - install Android Studio or set JAVA_HOME to a JDK 17+.'; exit 1 }

# ---- NDK
function Find-Ndk {
  if ($env:NDK_HOME -and (Test-Path (Join-Path $env:NDK_HOME 'source.properties'))) { return $env:NDK_HOME }
  $n = Get-ChildItem (Join-Path $sdk 'ndk') -Directory | Where-Object { Test-Path (Join-Path $_.FullName 'source.properties') } |
       Sort-Object { [version]($_.Name -replace '[^0-9.]', '') } -Descending | Select-Object -First 1
  if ($n) { return $n.FullName }
  if (Test-Path (Join-Path $sdk 'ndk-bundle\source.properties')) { return (Join-Path $sdk 'ndk-bundle') }
  return $null
}
$ndk = Find-Ndk
if (-not $ndk -and $InstallNdk) {
  $env:JAVA_HOME = $java
  $sdkm = Get-ChildItem (Join-Path $sdk 'cmdline-tools') -Recurse -Filter sdkmanager.bat | Select-Object -First 1
  if (-not $sdkm) {
    [Console]::Error.WriteLine('[*] Downloading Android command-line tools...')
    $zip = Join-Path $env:TEMP 'android-cmdline-tools.zip'
    $tmp = Join-Path $env:TEMP 'android-cmdline-tools'
    Invoke-WebRequest -Uri $CmdlineZip -OutFile $zip -UseBasicParsing
    Remove-Item $tmp -Recurse -Force; Expand-Archive $zip $tmp -Force
    $dest = Join-Path $sdk 'cmdline-tools\latest'
    New-Item -ItemType Directory -Force (Split-Path $dest) | Out-Null
    Remove-Item $dest -Recurse -Force; Move-Item (Join-Path $tmp 'cmdline-tools') $dest
    $sdkm = Get-Item (Join-Path $dest 'bin\sdkmanager.bat')
  }
  if (-not $sdkm) { Write-Output 'ERROR=Could not download Android command-line tools from dl.google.com - install the NDK via Android Studio - SDK Manager - SDK Tools.'; exit 1 }
  [Console]::Error.WriteLine("[*] Installing $NdkPackage (~1 GB) in a separate window - type 'y' there if it asks to accept the license.")
  Start-Process -FilePath $sdkm.FullName -ArgumentList "--sdk_root=`"$sdk`"", '--install', $NdkPackage -Wait
  $ndk = Find-Ndk
}
if (-not $ndk) { Write-Output "ERROR=Android NDK not installed in $sdk. Run 1-setup.bat - it installs it - or Android Studio - SDK Manager - SDK Tools - NDK Side by side."; exit 1 }

Write-Output "ANDROID_HOME=$sdk"
Write-Output "NDK_HOME=$ndk"
Write-Output "JAVA_HOME=$java"
$studioExe = $studios | ForEach-Object { Join-Path $_ 'bin\studio64.exe' } | Where-Object { Test-Path $_ } | Select-Object -First 1
if ($studioExe) { Write-Output "AQ_STUDIO=$studioExe" }
