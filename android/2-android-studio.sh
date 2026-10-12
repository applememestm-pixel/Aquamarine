#!/usr/bin/env bash
# Opens the Android project in Android Studio together with the Tauri dev
# session it builds through. Keep this terminal open while you work.
set -euo pipefail
HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
cd "$HERE/../crates/aqua-client-ui"
. "$HERE/env.sh" install || exit 1
if [ ! -f gen/android/app/build.gradle.kts ]; then
  echo "[i] First run - doing setup…"
  bash "$HERE/1-setup.sh"
fi
bash "$HERE/patch-project.sh"
echo
echo "[*] Android Studio opens the project crates/aqua-client-ui/gen/android."
echo "    In it: pick your phone or an emulator at the top and press Run."
echo "    Phone over USB: Settings - About - tap \"Build number\" 7 times - Developer options - USB debugging."
echo "    The phone must be on the same Wi-Fi as this PC."
HOSTARG=()
if [ -n "${AQ_LAN_IP:-}" ]; then
  HOSTARG=(--host "$AQ_LAN_IP")
  echo "[env] dev server address = $AQ_LAN_IP"
fi
echo
[ -n "${AQ_STUDIO:-}" ] && export ANDROID_STUDIO_PATH="$AQ_STUDIO"
if ! cargo tauri android dev --open "${HOSTARG[@]}"; then
  if [ -n "${AQ_STUDIO:-}" ]; then
    echo "[i] Opening Android Studio directly…"
    nohup "$AQ_STUDIO" "$PWD/gen/android" >/dev/null 2>&1 &
  fi
fi
