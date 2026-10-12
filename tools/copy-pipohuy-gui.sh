#!/usr/bin/env bash
# Copies the old PipoHuy UI sources to a short path for Aquamarine.
set -euo pipefail
VOL="$(cd "$(dirname "$(readlink -f "$0")")/../.." && pwd)"
SRC="$VOL/IdeaProjects/PipoHuyYaHochuUmeret/src/main/java/com/messenger/gui"
DST="$VOL/pipo-gui"
[ -d "$SRC" ] || { echo "[X] $SRC not found"; exit 1; }
mkdir -p "$DST"
( cd "$SRC" && find . -name '*.java' -exec cp --parents -f {} "$DST/" \; )
echo "Done: sources copied to $DST"
