#!/usr/bin/env bash
# ============================================================================
#  Aquamarine one-shot build for macOS / Linux.
#  Builds the relay server + the desktop client and drops the results into
#  ../Aquamarine-apps. The Windows equivalent is build-all.bat.
#
#  One-time deps:
#    - Rust (https://rustup.rs) and Node.js 18+.
#    - Linux: libwebkit2gtk-4.1-dev libgtk-3-dev libappindicator3-dev \
#             librsvg2-dev patchelf build-essential   (Debian/Ubuntu names)
#    - macOS: Xcode command-line tools.
#  The script installs the Tauri CLI for you if it's missing.
# ============================================================================
set -euo pipefail
cd "$(dirname "$0")"
ROOT="$(pwd)"
OUT="$ROOT/../Aquamarine-apps"
mkdir -p "$OUT"

say() { printf '\n\033[36m==>\033[0m %s\n' "$*"; }
die() { printf '\n\033[31m[X]\033[0m %s\n' "$*" >&2; exit 1; }

command -v cargo >/dev/null || die "Rust/cargo not found — install from https://rustup.rs"
command -v npm   >/dev/null || die "Node.js/npm not found — install from https://nodejs.org"

if ! cargo tauri --version >/dev/null 2>&1; then
  say "Installing Tauri CLI (one-time)…"
  cargo install tauri-cli --version '^2' --locked
fi

say "[1/3] Building the relay server…"
cargo build --release -p aqua-server

say "[2/3] Building the desktop client (Tauri)…"
( cd crates/aqua-client-ui && cargo tauri build ) || {
  # The app binary can be fine even if a bundler step fails — keep the binary.
  [ -f crates/aqua-client-ui/target/release/aqua-client-ui ] \
    && echo "[!] A bundle step failed, but the app binary built — continuing." \
    || die "Client build failed."
}

say "[3/3] Collecting into $OUT…"
cp -f target/release/aqua-server "$OUT/aqua-server" 2>/dev/null && echo "  + aqua-server"
BIN="crates/aqua-client-ui/target/release/aqua-client-ui"
[ -f "$BIN" ] && cp -f "$BIN" "$OUT/aquamarine" && echo "  + aquamarine"

# Native installers, if the bundler produced them.
BND="crates/aqua-client-ui/target/release/bundle"
if [ -d "$BND" ]; then
  find "$BND" -type f \( -name '*.dmg' -o -name '*.AppImage' -o -name '*.deb' -o -name '*.rpm' \) \
    -exec cp -f {} "$OUT/" \; -exec echo "  + {}" \;
fi

cat <<EOF

============================================================
  Done.  Apps: $OUT
    - aquamarine     (double-click / ./aquamarine to chat)
    - aqua-server    (./aqua-server to host a relay)
  Native installers (.dmg/.AppImage/.deb/.rpm) are there too if built.
============================================================
EOF
