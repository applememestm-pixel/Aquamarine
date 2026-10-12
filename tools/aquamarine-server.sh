#!/usr/bin/env bash
# Starts the relay and opens its admin panel in the browser.
# Data (accounts, aqua-admin.json) stays in this folder. Ctrl+C stops.
# (Windows: "Aquamarine Server.bat")
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"

# Stop a server that is already running so port 7777 is free.
pkill -x aqua-server 2>/dev/null && sleep 1 || true

if [ -f ./_find-server.sh ]; then . ./_find-server.sh; else
  EXE="$(pwd)/aqua-server"; [ -x "$EXE" ] || { echo "[X] aqua-server not found next to this script." >&2; exit 1; }
fi

export AQUA_LISTEN=0.0.0.0:7777
# Codes are shown (dev mode) until e-mail delivery is set up in the panel.
export AQUA_OPEN_PANEL=1
export RUST_LOG=info
clear 2>/dev/null || true
"$EXE" || true
echo
echo "*** Server stopped. ***"
