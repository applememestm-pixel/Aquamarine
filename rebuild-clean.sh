#!/usr/bin/env bash
# Stop everything, wipe old test data, rebuild client + server, refresh
# ../Aquamarine-apps. (Windows: rebuild-clean.bat)
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"
echo "============================================================"
echo "  Aquamarine: stop, clean old data, rebuild, refresh apps"
echo "============================================================"

# 1. stop running instances
pkill -x aquamarine     2>/dev/null || true
pkill -x aqua-client-ui 2>/dev/null || true
pkill -x aqua-server    2>/dev/null || true
sleep 1

# 2. wipe old accounts / vaults / server DB (clean slate)
./reset-testdata.sh -y
echo "[ok] old data cleared."

# 3 + 4. build and collect into ../Aquamarine-apps
./build-all.sh
