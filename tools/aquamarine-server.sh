#!/usr/bin/env bash
# Starts the relay and opens its admin panel in the browser.
# Data (accounts, aqua-admin.json) stays in this folder. Ctrl+C stops.
# (Windows: "Aquamarine Server.bat")
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"

# Stop a server that is already running so port 7777 is free.
pkill -x aqua-server 2>/dev/null && sleep 1 || true

if [ -f ./_find-server.sh ]; then . ./_find-server.sh; else
  EXE="$(pwd)/aqua-server"
  # Files copied from another disk/archive can lose the exec bit.
  [ -f "$EXE" ] && [ ! -x "$EXE" ] && chmod +x "$EXE" 2>/dev/null
  if [ ! -f "$EXE" ]; then echo "[X] aqua-server not found next to this script ($(pwd))." >&2; exit 1; fi
  if [ ! -x "$EXE" ]; then
    echo "[X] aqua-server is not executable and chmod failed (exFAT/NTFS mounted without exec?)." >&2
    echo "    Run: chmod +x \"$EXE\"  or copy the folder to your home directory." >&2
    exit 1
  fi
fi

export AQUA_LISTEN=0.0.0.0:7777
# Codes are shown (dev mode) until e-mail delivery is set up in the panel.
export AQUA_OPEN_PANEL=1
export RUST_LOG=info
clear 2>/dev/null || true
"$EXE" || true
echo
echo "*** Server stopped. ***"
