#!/usr/bin/env bash
# Builds an installable APK into android/out/Aquamarine-debug.apk
# (debug-signed: installs on any phone, no keystore needed).
set -euo pipefail
HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
cd "$HERE/../crates/aqua-client-ui"
fail() { echo; echo "[!] Build stopped - $*"; exit 1; }
. "$HERE/env.sh" install || fail "see the message above."
[ -f gen/android/app/build.gradle.kts ] || bash "$HERE/1-setup.sh"
bash "$HERE/patch-project.sh"

echo "[*] Building APK (arm64 + armv7)…"
cargo tauri android build --apk --debug --target aarch64 --target armv7

mkdir -p "$HERE/out"
APK="$(find gen/android/app/build/outputs/apk -name '*debug*.apk' -printf '%T@ %p\n' 2>/dev/null | sort -n | tail -1 | cut -d' ' -f2-)"
[ -n "$APK" ] || fail "APK not found in gen/android/app/build/outputs"
cp -f "$APK" "$HERE/out/Aquamarine-debug.apk"

echo
echo "[OK] $HERE/out/Aquamarine-debug.apk"
echo "     Copy it to the phone and open it, or with USB debugging on:"
echo "     adb install -r \"$HERE/out/Aquamarine-debug.apk\""
command -v xdg-open >/dev/null && xdg-open "$HERE/out" >/dev/null 2>&1 &
