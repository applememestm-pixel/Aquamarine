# shellcheck shell=bash
# Sourced by aquamarine-server.sh / server-panel.sh: sets EXE to the freshest
# aqua-server. Works both from the repo's tools/ and from ../Aquamarine-apps.
HERE="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[1]}")")" && pwd)"
EXE=""
if [ -f "$HERE/../Cargo.toml" ] && command -v cargo >/dev/null; then
  echo "Building the server if the code changed…"
  ( cd "$HERE/.." && cargo build --release -p aqua-server -q )
fi
for c in "$HERE/../target/release/aqua-server" "$HERE/../../Aquamarine-apps/aqua-server" "$HERE/aqua-server"; do
  if [ -x "$c" ]; then EXE="$(readlink -f "$c")"; break; fi
done
if [ -z "$EXE" ]; then
  echo "[X] aqua-server not found. Run ./build-all.sh first." >&2
  exit 1
fi
