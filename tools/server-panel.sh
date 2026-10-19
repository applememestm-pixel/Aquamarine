#!/usr/bin/env bash
# Aquamarine relay with a tiny terminal "panel": connect address, live account
# stats from /v1/stats and the log (dev-mode e-mail codes show up here).
# Ctrl+C stops the server. (Windows: "Aquamarine Server (panel).bat" + server-gui.ps1)
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"
PORT=7777

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

# LAN IPv4 so others on the network can connect (skip VPN / virtual adapters).
LAN="$(ip -4 -o addr show scope global 2>/dev/null | awk '{print $2, $4}' \
  | grep -Ev '^(docker|br-|veth|virbr|tun|tap|wg|tailscale|zt|hiddify|singbox|vmnet|vboxnet)' \
  | awk '{sub(/\/.*/,"",$2); print $2}' | head -1 || true)"
ADDR="http://${LAN:-127.0.0.1}:$PORT"

T=$'\033[1;38;2;0;229;199m'; V=$'\033[38;2;123;92;255m'; D=$'\033[38;2;140;150;165m'; N=$'\033[0m'
printf '\n  %sAquamarine · relay%s\n' "$T" "$N"
printf '  %sconnect address:%s %s\n' "$D" "$N" "$ADDR"
printf '  %sGive this address to people on your network (Server field on login).%s\n' "$D" "$N"
if command -v wl-copy >/dev/null; then printf '%s' "$ADDR" | wl-copy && echo "  (address copied to clipboard)"
elif command -v xclip >/dev/null; then printf '%s' "$ADDR" | xclip -selection clipboard && echo "  (address copied to clipboard)"; fi
echo

LOG="$(pwd)/server.log"
AQUA_LISTEN="0.0.0.0:$PORT" AQUA_DEV_CODES=1 RUST_LOG=info "$EXE" >"$LOG" 2>&1 &
PID=$!
tail -n +1 -f "$LOG" --pid="$PID" &
TAILPID=$!
cleanup() { kill "$PID" 2>/dev/null || true; kill "$TAILPID" 2>/dev/null || true; echo; echo "[*] server stopped."; }
trap cleanup EXIT INT TERM

num() { grep -o "\"$1\":[0-9]*" <<<"$2" | head -1 | cut -d: -f2; }
while kill -0 "$PID" 2>/dev/null; do
  sleep 10
  if r="$(curl -fsS -m 2 "http://127.0.0.1:$PORT/v1/stats" 2>/dev/null)"; then
    printf '%s[stats] Registered: %s   Online: %s   Bots: %s%s\n' "$V" "$(num users "$r")" "$(num online "$r")" "$(num bots "$r")" "$N"
  fi
done
echo "[*] server exited (log: $LOG)."
