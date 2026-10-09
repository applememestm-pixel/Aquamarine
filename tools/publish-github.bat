@echo off
rem ===================================================================
rem  Publishes Aquamarine to https://github.com/vladstalker2027/aquamarine
rem  Needs: git (https://git-scm.com). Optional: GitHub CLI "gh"
rem  (https://cli.github.com) - with it the repo is created automatically.
rem ===================================================================
setlocal
set OWNER=vladstalker2027
set REPO=aquamarine
set SRC=%~dp0..\aquamarine.bundle
set DST=%~dp0..\..\Aquamarine-github

where git >nul 2>nul || (echo [!] git not found. Install it from https://git-scm.com & pause & exit /b 1)
if not exist "%SRC%" (echo [!] aquamarine.bundle not found next to build-all.bat & pause & exit /b 1)

if exist "%DST%\.git" (
  echo [*] Updating %DST%
  git -C "%DST%" pull "%SRC%" main || (pause & exit /b 1)
) else (
  echo [*] Cloning into %DST%
  git clone -b main "%SRC%" "%DST%" || (pause & exit /b 1)
)
cd /d "%DST%"
git remote remove origin >nul 2>nul

where gh >nul 2>nul
if %errorlevel%==0 (
  gh auth status >nul 2>nul || gh auth login
  gh repo view %OWNER%/%REPO% >nul 2>nul
  if errorlevel 1 (
    echo [*] Creating github.com/%OWNER%/%REPO%
    gh repo create %OWNER%/%REPO% --public --description "End-to-end encrypted messenger: Telegram chats + Discord servers and calls" --homepage "https://%OWNER%.github.io/%REPO%/" || (pause & exit /b 1)
  )
) else (
  echo.
  echo [i] GitHub CLI not found. Create an EMPTY public repo first:
  echo     https://github.com/new  ^(name: %REPO%, no README^)
  start https://github.com/new
  pause
)

git remote add origin https://github.com/%OWNER%/%REPO%.git
git push -u origin main || (echo [!] push failed & pause & exit /b 1)

echo.
echo [OK] Code is on https://github.com/%OWNER%/%REPO%
echo  1. Settings - Pages - Source: GitHub Actions  ^(site goes live^)
echo  2. To build installers for all platforms:  git tag v0.1.0 ^&^& git push origin v0.1.0
start https://github.com/%OWNER%/%REPO%/settings/pages
pause
