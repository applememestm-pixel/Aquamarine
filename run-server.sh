#!/usr/bin/env bash
# Start aqua-server (relay) from source. Leave the terminal open; Ctrl+C stops.
#   ./run-server.sh          only this PC (+ Android emulator via 10.0.2.2)
#   ./run-server.sh --lan    also phones on the same Wi-Fi
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"

if [ "${1:-}" = "--lan" ] || [ "${1:-}" = "-Lan" ]; then
  export AQUA_LISTEN=0.0.0.0:7777
  printf '\033[36maqua-server starting for the whole local network (port 7777)\033[0m\n'
  skip='^(lo|docker|br-|veth|virbr|tun|tap|wg|tailscale|zt|hiddify|singbox|vmnet|vboxnet)'
  ip -4 -o addr show scope global 2>/dev/null | awk '{print $2, $4}' | while read -r ifc cidr; do
    [[ "$ifc" =~ $skip ]] && continue
    ipaddr="${cidr%/*}"
    [[ "$ipaddr" =~ ^(192\.168\.|10\.|172\.(1[6-9]|2[0-9]|3[01])\.) ]] && printf '  \033[32mphone: http://%s:7777\033[0m\n' "$ipaddr"
  done || true
  printf '  \033[32memulator: http://10.0.2.2:7777\033[0m\n'
  echo "If a firewall (ufw/firewalld) is on, allow TCP 7777 for the LAN."
else
  printf '\033[36maqua-server starting on http://127.0.0.1:7777 …\033[0m\n'
  echo "  (Android emulator: http://10.0.2.2:7777 ; for a phone run ./run-server.sh --lan)"
fi
export AQUA_OPEN_PANEL=1   # server opens its admin panel in the browser
echo "Press Ctrl+C to stop."
exec cargo run -p aqua-server
