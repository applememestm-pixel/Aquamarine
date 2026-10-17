#!/usr/bin/env bash
# Arch Linux: everything for the Android build in one go.
#
#   bash android/arch-all.sh          # deps + setup + APK, then offers Android Studio
#   bash android/arch-all.sh apk      # deps + setup + APK only
#   bash android/arch-all.sh studio   # deps + setup, then Android Studio (run on phone/emulator)
#
# 1. pacman: JDK 21, Rust, Node.js, unzip, base-devel…
# 2. Android Studio from the AUR (if yay / paru is installed; optional —
#    the APK builds without it, the SDK comes from Google's command-line tools)
# 3. android/1-setup.sh: Android SDK + NDK (~1.5 GB into ~/Android/Sdk),
#    Rust Android targets, Tauri CLI, npm packages, the Gradle project
# 4. android/3-build-apk.sh → android/out/Aquamarine-debug.apk
#    and/or android/2-android-studio.sh
set -euo pipefail
HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
MODE="${1:-all}"
say() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
fail() { printf '\n\033[1;31m[!] %s\033[0m\n' "$*" >&2; exit 1; }

command -v pacman >/dev/null || fail "This script is for Arch Linux (pacman). On other systems run android/1-setup.sh."

# ---- 1. system packages ------------------------------------------------------
say "System packages (sudo password may be asked)"
PKGS=(base-devel git curl unzip perl rustup nodejs npm jdk21-openjdk)
MISSING=()
for p in "${PKGS[@]}"; do pacman -Qq "$p" >/dev/null 2>&1 || MISSING+=("$p"); done
# rustup conflicts with the "rust" package: keep rust if it's already there.
if pacman -Qq rust >/dev/null 2>&1; then
  MISSING=("${MISSING[@]/rustup}")
  command -v rustup >/dev/null || fail "The 'rust' package has no rustup (Android targets need it). Replace it: sudo pacman -S rustup  (answer y to remove rust)"
fi
MISSING=($(printf '%s\n' "${MISSING[@]}" | grep -v '^$' || true))
if [ "${#MISSING[@]}" -gt 0 ]; then
  sudo pacman -S --needed "${MISSING[@]}"
else
  echo "all installed"
fi

# Gradle wants Java 17–21; Arch may default to something newer.
if [ -x /usr/lib/jvm/java-21-openjdk/bin/java ]; then
  export JAVA_HOME=/usr/lib/jvm/java-21-openjdk
fi

# ---- 2. Rust toolchain ---------------------------------------------------------
say "Rust"
if ! rustup show active-toolchain >/dev/null 2>&1; then
  rustup default stable
fi
rustup --version | head -1
export PATH="$HOME/.cargo/bin:$PATH"

# ---- 3. Android Studio (optional) ----------------------------------------------
say "Android Studio"
STUDIO=""
for c in /opt/android-studio/bin/studio.sh "$HOME/android-studio/bin/studio.sh" "$(command -v android-studio 2>/dev/null || true)"; do
  [ -n "$c" ] && [ -x "$c" ] && { STUDIO="$c"; break; }
done
if [ -n "$STUDIO" ]; then
  echo "found: $STUDIO"
elif [ "$MODE" != apk ]; then
  AUR=""
  for h in yay paru; do command -v "$h" >/dev/null && { AUR="$h"; break; }; done
  if [ -n "$AUR" ]; then
    echo "installing android-studio from the AUR with $AUR (big download)…"
    "$AUR" -S --needed android-studio || echo "[i] Android Studio not installed - the APK still builds without it."
  else
    echo "[i] No yay/paru - skipping Android Studio. The APK builds without it."
    echo "    To get it later: install yay (https://github.com/Jguer/yay), then: yay -S android-studio"
  fi
fi

# ---- 4. SDK, NDK, targets, Tauri CLI, npm, Gradle project -----------------------
say "Android SDK / NDK, Tauri CLI, project (first run: 10-20 minutes)"
bash "$HERE/1-setup.sh"

# ---- 5. build and/or open ---------------------------------------------------------
if [ "$MODE" = all ] || [ "$MODE" = apk ]; then
  say "Building the APK"
  bash "$HERE/3-build-apk.sh"
fi
if [ "$MODE" = studio ]; then
  say "Android Studio + dev session"
  exec bash "$HERE/2-android-studio.sh"
fi
if [ "$MODE" = all ]; then
  echo
  read -r -p "Open the project in Android Studio now (phone / emulator, live reload)? [y/N] " a || a=""
  case "$a" in [yYдД]*) exec bash "$HERE/2-android-studio.sh" ;; esac
  echo "Later: bash android/2-android-studio.sh"
fi
