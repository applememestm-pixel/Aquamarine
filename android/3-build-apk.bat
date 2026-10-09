@echo off
rem Builds an installable APK into android\out\Aquamarine-debug.apk
rem (debug-signed: installs on any phone, no keystore needed).
setlocal
cd /d "%~dp0..\crates\aqua-client-ui" || goto :fail
call "%~dp0env.bat" install || goto :fail
if not exist "gen\android\app\build.gradle.kts" (
  call "%~dp01-setup.bat" || goto :fail
)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0patch-project.ps1" || goto :fail

echo [*] Building APK (arm64 + armv7)...
cargo tauri android build --apk --debug --target aarch64 --target armv7 || goto :fail

if not exist "%~dp0out" mkdir "%~dp0out"
set "APK="
for /r "gen\android\app\build\outputs\apk" %%f in (*debug*.apk) do set "APK=%%f"
if not defined APK (echo [!] APK not found in gen\android\app\build\outputs & goto :fail)
copy /y "%APK%" "%~dp0out\Aquamarine-debug.apk" >nul || goto :fail

echo.
echo [OK] %~dp0out\Aquamarine-debug.apk
echo      Copy it to the phone and open it, or with USB debugging on:
echo      adb install -r "%~dp0out\Aquamarine-debug.apk"
explorer "%~dp0out"
pause
exit /b 0

:fail
echo.
echo [!] Build stopped - see the message above.
pause
exit /b 1
