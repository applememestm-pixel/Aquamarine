#!/usr/bin/env bash
# Removes OLD debug build caches. Release builds and packages are kept.
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"
for d in target/debug crates/aqua-client-ui/target/debug; do
  if [ -d "$d" ]; then echo "  deleting $d …"; rm -rf "$d"; fi
done
echo "Done. Release builds and installers are untouched."
