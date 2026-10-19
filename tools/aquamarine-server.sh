#!/usr/bin/env bash
# Starts the relay and opens its admin panel in the browser.
# Data (accounts, aqua-admin.json) stays in this folder. Ctrl+C stops.
# (Windows: "Aquamarine Server.bat")
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"

# Stop a server that is already running so port 7777 is free.
pkill -x aqua-server 2>/dev/null && sleep 1 || true

# Disks mounted with noexec (NTFS/exFAT via fstab "user", USB sticks) can't run
# programs: chmod does nothing there. Then run a copy from ~/.cache instead -
# the data (database, logs, aqua-*.json) stays in this folder.
runnable() {
  local src="$1" dst dir
  [ -f "$src" ] || return 1
  [ -x "$src" ] || chmod +x "$src" 2>/dev/null || true
  if [ -x "$src" ]; then printf '%s\n' "$src"; return 0; fi
  dir="${XDG_CACHE_HOME:-$HOME/.cache}/aquamarine/bin"
  mkdir -p "$dir" || return 1
  dst="$dir/$(basename "$src")"
  if [ ! -x "$dst" ] || ! cmp -s "$src" "$dst"; then
    echo "[i] $(dirname "$src") is mounted without exec - running a copy from $dir" >&2
    cp -f "$src" "$dst.tmp" && chmod +x "$dst.tmp" && mv -f "$dst.tmp" "$dst" || return 1
  fi
  printf '%s\n' "$dst"
}
if [ -f ./_find-server.sh ]; then . ./_find-server.sh; else
  if [ ! -f "$(pwd)/aqua-server" ]; then echo "[X] aqua-server not found next to this script ($(pwd))." >&2; exit 1; fi
  EXE="$(runnable "$(pwd)/aqua-server")" || { echo "[X] can't make aqua-server runnable." >&2; exit 1; }
fi

export AQUA_LISTEN=0.0.0.0:7777
# Codes are shown (dev mode) until e-mail delivery is set up in the panel.
export AQUA_OPEN_PANEL=1
export RUST_LOG=info
clear 2>/dev/null || true
"$EXE" || true
echo
echo "*** Server stopped. ***"
