#!/usr/bin/env bash
# ===================================================================
#  Pushes Aquamarine to https://github.com/applememestm-pixel/aquamarine
#  and tags v0.1.0 so GitHub builds installers for every platform.
#  Source of truth: ../aquamarine.bundle → working clone ../../Aquamarine-github.
#  On push git asks for your GitHub login; use a token with repo + workflow
#  rights as the password (or set up `gh auth login` once).
# ===================================================================
set -euo pipefail
URL=https://github.com/applememestm-pixel/aquamarine
HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
REPO="$(cd "$HERE/.." && pwd)"
SRC="$REPO/aquamarine.bundle"
DST="$(cd "$REPO/.." && pwd)/Aquamarine-github"
fail() { echo "[!] $* - see the message above."; exit 1; }

command -v git >/dev/null || fail "git not found (sudo pacman -S git)"
[ -f "$SRC" ] || fail "aquamarine.bundle not found in $REPO"
# NTFS volumes look foreign-owned to git ("dubious ownership").
g() { git -c safe.directory="$DST" -C "$DST" "$@"; }

if [ -d "$DST/.git" ]; then
  echo "[*] Updating $DST"
  g fetch "$SRC" main:refs/remotes/bundle/main --force || fail "fetch"
  g checkout -q main || fail "checkout"
  g reset -q --hard bundle/main || fail "reset"
else
  echo "[*] Cloning into $DST"
  git clone -q -b main "$SRC" "$DST" || fail "clone"
fi
g remote remove origin >/dev/null 2>&1 || true
g remote add origin "$URL.git"

echo "[*] Pushing code…"
g push -u origin main || fail "push"

g tag -f v0.1.0 >/dev/null
echo "[*] Pushing tag v0.1.0 (starts the all-platform build)…"
g push -f origin v0.1.0 || fail "tag push"

echo
echo "[OK] Done: $URL"
echo "  Site:   https://applememestm-pixel.github.io/aquamarine/"
echo "  Builds: $URL/actions"
command -v xdg-open >/dev/null && xdg-open "$URL/actions" >/dev/null 2>&1 &
