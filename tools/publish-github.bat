@echo off
rem ===================================================================
rem  Pushes Aquamarine to https://github.com/applememestm-pixel/aquamarine
rem  and tags v0.1.0 so GitHub builds installers for every platform.
rem  Needs git (https://git-scm.com). On first push a GitHub sign-in
rem  window opens (Git Credential Manager) - just log in there.
rem ===================================================================
setlocal
set URL=https://github.com/applememestm-pixel/aquamarine
set SRC=%~dp0..\aquamarine.bundle
set DST=%~dp0..\..\Aquamarine-github

where git >nul 2>nul || (echo [!] git not found. Install it from https://git-scm.com & pause & exit /b 1)
if not exist "%SRC%" (echo [!] aquamarine.bundle not found in D:\Aquamarine & pause & exit /b 1)

if exist "%DST%\.git" (
  echo [*] Updating %DST%
  git -C "%DST%" fetch "%SRC%" main:refs/remotes/bundle/main --force || goto :fail
  git -C "%DST%" checkout -q main || goto :fail
  git -C "%DST%" reset -q --hard bundle/main || goto :fail
) else (
  echo [*] Cloning into %DST%
  git clone -q -b main "%SRC%" "%DST%" || goto :fail
)
cd /d "%DST%"
git remote remove origin >nul 2>nul
git remote add origin %URL%.git

echo [*] Pushing code...
git push -u origin main || goto :fail

git tag -f v0.1.0 >nul
echo [*] Pushing tag v0.1.0 (starts the all-platform build)...
git push -f origin v0.1.0 || goto :fail

echo.
echo [OK] Done: %URL%
echo   Last step: in the page that opens set  Source = GitHub Actions
echo   Site:   https://applememestm-pixel.github.io/aquamarine/
echo   Builds: %URL%/actions
start %URL%/settings/pages
start %URL%/actions
pause
exit /b 0

:fail
echo [!] Something failed - see the message above.
pause
exit /b 1
