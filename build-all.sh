#!/usr/bin/env bash
# ============================================================================
#  Aquamarine one-shot build for Linux / macOS (Windows: build-all.bat).
#  Builds the relay server + the desktop client, drops runnable apps into the
#  neighbouring ../Aquamarine-apps and copies the Linux packages into
#  site/downloads so the website can hand them out.
#
#  One-time deps:
#    Arch:   sudo pacman -S --needed rustup nodejs npm base-devel webkit2gtk-4.1 \
#              curl wget file openssl appmenu-gtk-module libappindicator-gtk3 \
#              librsvg xdotool patchelf  &&  rustup default stable
#    Debian: libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
#              librsvg2-dev patchelf build-essential
#    macOS:  Xcode command-line tools.
#  The script installs the Tauri CLI for you if it's missing.
# ============================================================================
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"
ROOT="$(pwd)"
OUT="$(cd .. && pwd)/Aquamarine-apps"
DL="$ROOT/site/downloads"
mkdir -p "$OUT" "$DL"

say() { printf '\n\033[36m==>\033[0m %s\n' "$*"; }
die() { printf '\n\033[31m[X]\033[0m %s\n' "$*" >&2; exit 1; }

command -v cargo >/dev/null || die "Rust/cargo not found — sudo pacman -S rustup && rustup default stable"
command -v npm   >/dev/null || die "Node.js/npm not found — sudo pacman -S nodejs npm"

if ! cargo tauri --version >/dev/null 2>&1; then
  say "Installing Tauri CLI (one-time)…"
  cargo install tauri-cli --version '^2' --locked
fi

# Frontend deps. node_modules installed on Windows carry win32-only native
# binaries (rollup/esbuild) — Vite can't run with them, so reinstall.
UI=crates/aqua-client-ui/ui
if [ ! -d "$UI/node_modules" ] || ls -d "$UI"/node_modules/@rollup/*win32* "$UI"/node_modules/@esbuild/win32* >/dev/null 2>&1; then
  say "Installing frontend dependencies (npm ci)…"
  rm -rf "$UI/node_modules"
  ( cd "$UI" && npm ci )
fi

say "[1/3] Building the relay server…"
cargo build --release -p aqua-server

say "[2/3] Building the desktop client (Tauri)…"
BUNDLE_WARN=
( cd crates/aqua-client-ui && cargo tauri build ) || {
  # The app binary can be fine even if a bundler step fails — keep the binary,
  # otherwise you'd keep launching yesterday's build.
  [ -f crates/aqua-client-ui/target/release/aqua-client-ui ] || die "Client build failed."
  BUNDLE_WARN=1
  echo "[!] A bundle step failed, but the app binary built — continuing."
}

say "[3/3] Collecting into $OUT…"
# A running copy would keep the old binary busy ("Text file busy").
if pgrep -x aquamarine >/dev/null 2>&1; then
  echo "  [*] Closing the running Aquamarine so it can be replaced…"
  pkill -x aquamarine || true; sleep 1
fi
pkill -x aqua-server 2>/dev/null && sleep 1 || true

cp -f target/release/aqua-server "$OUT/aqua-server" && echo "  + aqua-server"
BIN="crates/aqua-client-ui/target/release/aqua-client-ui"
[ -f "$BIN" ] && cp -f "$BIN" "$OUT/aquamarine" && echo "  + aquamarine"
for f in aquamarine.sh aquamarine-server.sh server-panel.sh aquamarine-site.sh; do
  cp -f "tools/$f" "$OUT/$f" && chmod +x "$OUT/$f" && echo "  + $f"
done

# Native packages, if the bundler produced them → apps folder + website.
BND="crates/aqua-client-ui/target/release/bundle"
if [ -d "$BND" ]; then
  while IFS= read -r f; do
    cp -f "$f" "$OUT/" && echo "  + $(basename "$f")"
    case "$f" in
      *.AppImage) cp -f "$f" "$DL/Aquamarine.AppImage" ;;
      *.deb)      cp -f "$f" "$DL/Aquamarine.deb" ;;
      *.rpm)      cp -f "$f" "$DL/Aquamarine.rpm" ;;
      *.dmg)      cp -f "$f" "$DL/Aquamarine.dmg" ;;
    esac
  done < <(find "$BND" -type f \( -name '*.dmg' -o -name '*.AppImage' -o -name '*.deb' -o -name '*.rpm' \))
fi

cat <<EOT

============================================================
  Done.  Apps: $OUT
    - aquamarine.sh         (./aquamarine.sh to chat)
    - aquamarine-server.sh  (host a relay; opens its admin panel)
    - server-panel.sh       (relay with live stats in the terminal)
    - aquamarine-site.sh    (the website on :8080; also runs in Termux / Docker)
  Packages (.AppImage/.deb/.rpm) are there and in site/downloads too.
============================================================
EOT
[ -z "$BUNDLE_WARN" ] || echo "[!] Packages were NOT rebuilt (see the error above); the app binary is fresh."
