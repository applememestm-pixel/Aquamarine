#!/usr/bin/env bash
# Launch a SECOND Aquamarine client with its own vault, to chat with yourself
# from one window to another on the same machine.
#
# Run ./run-first.sh FIRST and wait for its window: it starts the Vite dev
# server and builds the Rust binary; this script only relaunches that binary
# with a different AQUA_DATA_DIR. (Two `cargo tauri dev` in parallel fight
# over target/ and Vite's port 5173.)
set -euo pipefail
ROOT="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
CLIENT="$ROOT/crates/aqua-client-ui"
EXE="$CLIENT/target/debug/aqua-client-ui"
DATA="$ROOT/second-instance-data"

if [ ! -x "$EXE" ]; then
  printf '\n\033[33mThe compiled client binary is not there yet:\033[0m\n  %s\n\n' "$EXE"
  echo "Launch ./run-first.sh first and wait for the first Aquamarine window to open,"
  echo "then run ./run-second.sh again."
  echo "(Or: cd $CLIENT && cargo build — but Vite dev must still run on :5173.)"
  exit 1
fi

mkdir -p "$DATA"
export AQUA_DATA_DIR="$DATA"
printf '\n\033[36mLaunching SECOND Aquamarine instance\033[0m\n'
echo "  vault:  $DATA"
echo "  binary: $EXE"
echo "  reusing Vite dev server from the first instance."
exec "$EXE"
