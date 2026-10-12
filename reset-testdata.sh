#!/usr/bin/env bash
# Resets Aquamarine to a CLEAN state for testing: deletes all LOCAL accounts,
# chats and keys on THIS machine so registration can be tested from zero.
# Source code and built apps are NOT touched.
#   ./reset-testdata.sh        asks first
#   ./reset-testdata.sh -y     no question (used by rebuild-clean.sh)
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"
ROOT="$(pwd)"
APPS="$(cd .. && pwd)/Aquamarine-apps"
ID=app.aquamarine.desktop

if [ "${1:-}" != "-y" ]; then
  cat <<EOT
============================================================
  Reset Aquamarine to a CLEAN state for testing
============================================================
This deletes:
  - the server's account database (all registered users)
  - your client vault in ~/.local/share/$ID
  - the dev/test data folders (.data-*, second-instance-data)
EOT
  read -rp "Delete all local Aquamarine data now? [y/N] " a
  case "$a" in [yYдД]*) ;; *) echo "Cancelled - nothing was deleted."; exit 0 ;; esac
fi

echo "Removing server account databases…"
for dir in "$ROOT" "$ROOT/tools" "$APPS"; do
  rm -f "$dir"/aqua-directory.sqlite "$dir"/aqua-directory.sqlite-wal "$dir"/aqua-directory.sqlite-shm
done

echo "Removing client vaults / chats…"
for d in .data-test .data-vlad second-instance-data .data-A .data-B; do
  rm -rf "${ROOT:?}/$d"
done
for base in "${XDG_DATA_HOME:-$HOME/.local/share}" "${XDG_CONFIG_HOME:-$HOME/.config}" "${XDG_CACHE_HOME:-$HOME/.cache}"; do
  rm -rf "${base:?}/$ID"
done

echo
echo "Done - everything is clean."
echo "Next: start tools/aquamarine-server.sh, then launch a client and register fresh."
echo "For two accounts on one PC use ./test-client-A.sh and ./test-client-B.sh."
