#!/usr/bin/env bash
# Launch the FIRST (default) Aquamarine client in dev mode (hot reload).
# Uses the normal app-data dir (~/.local/share/app.aquamarine.desktop).
# For a second instance on the same machine run ./run-second.sh in another
# terminal once this window is up.
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")/crates/aqua-client-ui"
exec cargo tauri dev
