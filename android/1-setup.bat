@echo off
rem One-time setup: Rust Android targets, Tauri CLI, frontend deps and the
rem Android Studio project in crates\aqua-client-ui\gen\android.
setlocal
cd /d "%~dp0..\crates\aqua-client-ui" || goto :fail
call "%~dp0env.bat" install || goto :fail

where rustup >nul 2>nul || (echo [!] Rust not found - install from https://rustup.rs & goto :fail)
where npm >nul 2>nul || (echo [!] Node.js not found - install from https://nodejs.org & goto :fail)

echo [*] Rust targets for Android...
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android || goto :fail

cargo tauri --version >nul 2>nul
if errorlevel 1 (
  echo [*] Installing Tauri CLI - takes a few minutes once...
  cargo install tauri-cli --version "^2" --locked || goto :fail
)

echo [*] Frontend dependencies...
pushd ui
call npm ci
if errorlevel 1 (popd & goto :fail)
popd

if not exist "gen\android\app\build.gradle.kts" (
  echo [*] Generating the Android Studio project...
  cargo tauri android init --ci --skip-targets-install || goto :fail
)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0patch-project.ps1" || goto :fail

echo.
echo [OK] Ready. Next: 2-android-studio.bat  (or 3-build-apk.bat for a ready .apk)
pause
exit /b 0

:fail
echo.
echo [!] Setup stopped - see the message above.
pause
exit /b 1
