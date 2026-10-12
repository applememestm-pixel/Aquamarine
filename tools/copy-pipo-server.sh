#!/usr/bin/env bash
# Copies the old Java server sources to a short path next to the repo so
# Claude can read them (the remote file bridge only reaches a few levels deep).
set -euo pipefail
VOL="$(cd "$(dirname "$(readlink -f "$0")")/../.." && pwd)"   # the drive that holds Aquamarine
SRC="$VOL/pipo-server"; DST="$VOL/pipo-srv"
[ -d "$SRC" ] || { echo "[X] $SRC not found"; exit 1; }
mkdir -p "$DST/java" "$DST/resources" "$DST/config"
( cd "$SRC/src/main/java/ru/pipo/server" && find . -name '*.java' -exec cp --parents -f {} "$DST/java/" \; )
[ -d "$SRC/src/main/resources" ] && cp -rf "$SRC/src/main/resources/." "$DST/resources/"
[ -f "$SRC/config/server-mode.properties" ] && cp -f "$SRC/config/server-mode.properties" "$DST/config/"
cp -f "$SRC/pom.xml" "$DST/pom.xml"
echo "Done. Tell Claude: \"$DST is ready\"."
