@echo off
rem Finds Android SDK / NDK / Java wherever they are (any drive, Android
rem Studio's own SDK setting). Pass "install" to install a missing NDK.
set "_AQ_ERR="
set "_AQ_FLAG="
if /i "%~1"=="install" set "_AQ_FLAG=-InstallNdk"
for /f "usebackq tokens=1,* delims==" %%a in (`powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0find-android.ps1" %_AQ_FLAG%`) do (
  if /i "%%a"=="ERROR" (set "_AQ_ERR=%%b") else (set "%%a=%%b")
)
if defined _AQ_ERR goto :err
if not defined ANDROID_HOME goto :nosdk
set "ANDROID_SDK_ROOT=%ANDROID_HOME%"
set "PATH=%JAVA_HOME%\bin;%ANDROID_HOME%\platform-tools;%USERPROFILE%\.cargo\bin;%PATH%"
echo [env] ANDROID_HOME = %ANDROID_HOME%
echo [env] NDK_HOME     = %NDK_HOME%
echo [env] JAVA_HOME    = %JAVA_HOME%
exit /b 0

:nosdk
set "_AQ_ERR=Could not detect the Android SDK."
:err
echo [!] %_AQ_ERR%
exit /b 1
