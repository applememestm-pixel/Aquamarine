#!/usr/bin/env bash
# Launch a clean client instance "B" with its own identity/vault, so you can
# register a fresh account and chat with instance A on one machine.
set -euo pipefail
ROOT="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
APP="$(cd "$ROOT/.." && pwd)/Aquamarine-apps/aquamarine"
if [ ! -x "$APP" ]; then
  echo "Built client not found at:"
  echo "  $APP"
  echo "Run ./build-all.sh first."
  exit 1
fi
export AQUA_DATA_DIR="$ROOT/.data-B"
mkdir -p "$AQUA_DATA_DIR"
nohup "$APP" >/dev/null 2>&1 &
disown
echo "Client B started (vault: $AQUA_DATA_DIR)"
