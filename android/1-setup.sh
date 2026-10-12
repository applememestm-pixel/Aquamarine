#!/usr/bin/env bash
# One-time setup: Rust Android targets, Tauri CLI, frontend deps and the
# Android Studio project in crates/aqua-client-ui/gen/android.
set -euo pipefail
HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
cd "$HERE/../crates/aqua-client-ui"
fail() { echo; echo "[!] Setup stopped - $*"; exit 1; }

command -v rustup >/dev/null || fail "Rust not found: sudo pacman -S rustup && rustup default stable"
command -v npm    >/dev/null || fail "Node.js not found: sudo pacman -S nodejs npm"
. "$HERE/env.sh" install || fail "see the message above."

echo "[*] Rust targets for Android…"
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android

if ! cargo tauri --version >/dev/null 2>&1; then
  echo "[*] Installing Tauri CLI - takes a few minutes once…"
  cargo install tauri-cli --version '^2' --locked
fi

# node_modules from a Windows install carries win32-only native binaries - reinstall then.
if ls -d ui/node_modules/@rollup/*win32* ui/node_modules/@esbuild/win32* >/dev/null 2>&1; then
  echo "[*] node_modules came from Windows - reinstalling…"
  rm -rf ui/node_modules
fi
if [ -x ui/node_modules/.bin/vite ]; then
  echo "[*] Frontend dependencies already installed."
else
  echo "[*] Frontend dependencies…"
  ( cd ui && npm ci )
fi

if [ ! -f gen/android/app/build.gradle.kts ]; then
  echo "[*] Generating the Android Studio project…"
  cargo tauri android init --ci --skip-targets-install
fi
bash "$HERE/patch-project.sh"

echo
echo "[OK] Ready. Next: android/2-android-studio.sh  (or android/3-build-apk.sh for a ready .apk)"
