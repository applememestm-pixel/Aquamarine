@echo off
rem Finds Android SDK / NDK / Java (Android Studio defaults) for the other scripts.
if not defined ANDROID_HOME if exist "%LOCALAPPDATA%\Android\Sdk" set "ANDROID_HOME=%LOCALAPPDATA%\Android\Sdk"
if not defined ANDROID_HOME (
  echo [!] Android SDK not found. Install Android Studio and finish its first-run setup.
  exit /b 1
)
if not defined NDK_HOME for /f "delims=" %%d in ('dir /b /ad /o:n "%ANDROID_HOME%\ndk" 2^>nul') do set "NDK_HOME=%ANDROID_HOME%\ndk\%%d"
if not defined NDK_HOME (
  echo [!] Android NDK not found.
  echo     Android Studio - Settings - Languages and Frameworks - Android SDK - SDK Tools
  echo     - tick "NDK Side by side" - Apply. Then run this again.
  exit /b 1
)
if not defined JAVA_HOME if exist "%ProgramFiles%\Android\Android Studio\jbr\bin\java.exe" set "JAVA_HOME=%ProgramFiles%\Android\Android Studio\jbr"
if not defined JAVA_HOME (
  echo [!] Java not found. Install Android Studio into the default folder or set JAVA_HOME.
  exit /b 1
)
set "PATH=%JAVA_HOME%\bin;%ANDROID_HOME%\platform-tools;%USERPROFILE%\.cargo\bin;%PATH%"
echo [env] ANDROID_HOME = %ANDROID_HOME%
echo [env] NDK_HOME     = %NDK_HOME%
echo [env] JAVA_HOME    = %JAVA_HOME%
exit /b 0
