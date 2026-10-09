@echo off
rem Starts the Tauri dev session and opens the project in Android Studio.
rem Keep this window open while you work - Android Studio builds through it.
setlocal
cd /d "%~dp0..\crates\aqua-client-ui" || goto :fail
call "%~dp0env.bat" || goto :fail
if not exist "gen\android\app\build.gradle.kts" (
  echo [i] First run - doing setup...
  call "%~dp01-setup.bat" || goto :fail
)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0patch-project.ps1" || goto :fail
echo.
echo [*] Opening Android Studio. In it: pick your phone or an emulator at the top and press Run.
echo     Phone: Settings - About - tap "Build number" 7 times - Developer options - USB debugging.
echo.
cargo tauri android dev --open
exit /b 0

:fail
pause
exit /b 1
