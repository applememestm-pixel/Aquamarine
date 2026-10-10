@echo off
title Aquamarine Server
rem Starts the relay and opens its admin panel in the browser.
rem Data (accounts, aqua-admin.json) stays in this folder, as before.
cd /d "%~dp0"

rem Stop a server that is already running so port 7777 is free.
taskkill /f /im aqua-server.exe >nul 2>nul
timeout /t 1 >nul

rem Use the freshest build: rebuild from source when Rust is installed.
set "EXE="
where cargo >nul 2>nul
if not errorlevel 1 (
  echo Building the server if the code changed...
  pushd "%~dp0.."
  cargo build --release -p aqua-server -q
  popd
)
if exist "%~dp0..\target\release\aqua-server.exe" set "EXE=%~dp0..\target\release\aqua-server.exe"
if not defined EXE if exist "%~dp0..\..\Aquamarine-apps\aqua-server.exe" set "EXE=%~dp0..\..\Aquamarine-apps\aqua-server.exe"
if not defined EXE if exist "%~dp0aqua-server.exe" set "EXE=%~dp0aqua-server.exe"
if not defined EXE (
  echo [X] aqua-server.exe not found. Run build-all.bat first.
  pause
  exit /b 1
)

set "AQUA_LISTEN=0.0.0.0:7777"
set "AQUA_DEV_CODES=1"
set "AQUA_OPEN_PANEL=1"
set "RUST_LOG=info"
cls
"%EXE%"
echo.
echo *** Server stopped. ***
pause
