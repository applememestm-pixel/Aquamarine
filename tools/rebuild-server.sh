#!/usr/bin/env bash
# Force-fresh server rebuild from the current sources → ../Aquamarine-apps.
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")/.."
APPS="$(cd .. && pwd)/Aquamarine-apps"
echo "Stopping any running server…"
pkill -x aqua-server 2>/dev/null && sleep 1 || true
echo "Forcing a CLEAN server rebuild from the current sources…"
cargo clean -p aqua-server
cargo build --release -p aqua-server || { echo; echo "*** BUILD FAILED - scroll up for the error ***"; exit 1; }
mkdir -p "$APPS"
cp -f target/release/aqua-server "$APPS/aqua-server"
echo
echo "============================================================"
echo "  DONE. Fresh aqua-server copied to $APPS"
echo "  Now start it with tools/aquamarine-server.sh"
echo "============================================================"
