@echo off
rem Opens the Android project in Android Studio together with the Tauri dev
rem session it builds through. Keep this window open while you work.
setlocal
cd /d "%~dp0..\crates\aqua-client-ui" || goto :fail
call "%~dp0env.bat" install || goto :fail
if not exist "gen\android\app\build.gradle.kts" (
  echo [i] First run - doing setup...
  call "%~dp01-setup.bat" || goto :fail
)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0patch-project.ps1" || goto :fail
echo.
echo [*] Android Studio opens the project crates\aqua-client-ui\gen\android.
echo     In it: pick your phone or an emulator at the top and press Run.
echo     Phone: Settings - About - tap "Build number" 7 times - Developer options - USB debugging.
echo.
if defined AQ_STUDIO set "ANDROID_STUDIO_PATH=%AQ_STUDIO%"
cargo tauri android dev --open
if errorlevel 1 if defined AQ_STUDIO (
  echo [i] Opening Android Studio directly and running the dev session...
  start "" "%AQ_STUDIO%" "%CD%\gen\android"
  cargo tauri android dev
)
exit /b 0

:fail
pause
exit /b 1
