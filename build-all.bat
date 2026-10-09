@echo off
setlocal enabledelayedexpansion
title Aquamarine - build everything

REM ============================================================================
REM  Aquamarine one-click build.
REM  Double-click this. It builds the desktop client + the relay server and
REM  drops double-clickable apps into a NEIGHBORING folder (..\Aquamarine-apps),
REM  and copies the Windows installer into site\downloads so the website can
REM  hand it out directly.
REM
REM  Needs (one-time): Rust (rustup), Node.js, and WebView2 (preinstalled on
REM  Win10/11). The script installs the Tauri CLI for you if it's missing.
REM ============================================================================

cd /d "%~dp0"
set "ROOT=%~dp0"
set "OUT=%~dp0..\Aquamarine-apps"
set "DL=%~dp0site\downloads"

echo.
echo ============================================================
echo   Aquamarine - building client + server
echo   This can take several minutes the first time. Grab a tea.
echo ============================================================
echo.

REM ---- sanity: toolchains -----------------------------------------------------
where cargo >nul 2>nul || (echo [X] Rust/cargo not found. Install from https://rustup.rs && goto :fail)
where npm   >nul 2>nul || (echo [X] Node.js/npm not found. Install from https://nodejs.org && goto :fail)

REM ---- Tauri CLI --------------------------------------------------------------
cargo tauri --version >nul 2>nul
if errorlevel 1 (
  echo [*] Installing Tauri CLI ^(one-time^)...
  cargo install tauri-cli --locked || goto :fail
)

REM ---- 1/3  server ------------------------------------------------------------
echo.
echo [1/3] Building the relay server...
cargo build --release -p aqua-server || goto :fail

REM ---- 2/3  desktop client ----------------------------------------------------
echo.
echo [2/3] Building the desktop client ^(Tauri^)...
set "BUNDLE_WARN="
pushd "crates\aqua-client-ui"
cargo tauri build
if errorlevel 1 (
  REM The exe can be fine even when only the installer step fails (NSIS
  REM download, a locked file...). Keep going so the new app still gets
  REM copied -- otherwise you'd keep launching yesterday's Aquamarine.exe.
  if exist "target\release\aqua-client-ui.exe" (
    set "BUNDLE_WARN=1"
    echo.
    echo [!] Installer step failed, but the app itself was built - continuing.
  ) else (
    popd & goto :fail
  )
)
popd

REM ---- 3/3  collect -----------------------------------------------------------
echo.
echo [3/3] Collecting apps into "%OUT%"...
if not exist "%OUT%" mkdir "%OUT%"
REM A running copy locks the exe and the copy would silently keep the old one.
tasklist /FI "IMAGENAME eq Aquamarine.exe" 2>nul | find /I "Aquamarine.exe" >nul && (
  echo   [*] Closing running Aquamarine.exe so it can be replaced...
  taskkill /IM Aquamarine.exe /F >nul 2>nul
  timeout /t 1 /nobreak >nul
)
if not exist "%DL%"  mkdir "%DL%"

set "CLIENT_EXE=crates\aqua-client-ui\target\release\aqua-client-ui.exe"
set "SERVER_EXE=target\release\aqua-server.exe"

if exist "%CLIENT_EXE%" (
  copy /Y "%CLIENT_EXE%" "%OUT%\Aquamarine.exe" >nul && echo   + Aquamarine.exe
) else (
  echo   [!] client exe not found at %CLIENT_EXE%
)

if exist "%SERVER_EXE%" (
  copy /Y "%SERVER_EXE%" "%OUT%\aqua-server.exe" >nul && echo   + aqua-server.exe
) else (
  echo   [!] server exe not found at %SERVER_EXE%
)

REM server launchers (reliable console + optional windowed panel)
copy /Y "tools\Aquamarine Server.bat"          "%OUT%\Aquamarine Server.bat"          >nul
copy /Y "tools\Aquamarine Server (panel).bat"  "%OUT%\Aquamarine Server (panel).bat"  >nul
copy /Y "tools\server-gui.ps1"                 "%OUT%\server-gui.ps1"                 >nul
echo   + Aquamarine Server.bat ^(console^) + (panel).bat ^(windowed^)

REM installer -> website downloads folder (direct download from the site)
for %%F in ("crates\aqua-client-ui\target\release\bundle\nsis\*-setup.exe") do (
  copy /Y "%%F" "%DL%\Aquamarine-Setup.exe" >nul && echo   + site\downloads\Aquamarine-Setup.exe
)

echo.
echo ============================================================
if defined BUNDLE_WARN echo   [!] Installer was NOT rebuilt (see the error above); the app is fresh.
echo   Done.
echo   Apps:      %OUT%
echo     - Aquamarine.exe           (double-click to chat)
echo     - Aquamarine Server.bat    (double-click to host a relay)
echo   Website:   site\index.html  (downloads wired to site\downloads)
echo ============================================================
echo.
echo Tip: macOS / Linux / Android installers only build on those systems.
echo      Run this same repo's build there to fill those download slots.
echo.
pause
exit /b 0

:fail
echo.
echo ************************************************************
echo   Build failed. Scroll up for the first red error line.
echo ************************************************************
echo.
pause
exit /b 1
