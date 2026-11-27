#!/usr/bin/env bash
# Test of tools/aquamarine-site.sh::verify_release (AQUA-SEC-012, 27.11):
# the site is taken only from a release whose SHA256SUMS.txt is signed by
# the built-in key, names that release (VERSION) and lists the archive.
# A key of its own here; the script's functions are loaded, not run.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
t=$(mktemp -d); trap 'rm -rf "$t"' EXIT
# The script's helpers, without running it.
sed -n '/^say()/,/^die()/p; /^unhex()/,/^sha256()/p; /^verify_release()/,/^}/p' "$HERE/aquamarine-site.sh" > "$t/fns.sh"
die() { printf 'DIE %s\n' "$*"; exit 1; }
# shellcheck disable=SC1091
source "$t/fns.sh"
die() { printf 'DIE %s\n' "$*"; exit 1; }
openssl genpkey -algorithm ed25519 -out "$t/key.pem" 2>/dev/null
PUBKEY=$(openssl pkey -in "$t/key.pem" -pubout -outform DER | tail -c 32 | od -An -v -tx1 | tr -d ' \n')
release() { # dir version
  mkdir -p "$1"; printf 'site' > "$1/aquamarine-site.tar.gz"; printf '%s' "$2" > "$1/VERSION"
  (cd "$1" && for f in aquamarine-site.tar.gz VERSION; do printf '%s  %s\n' "$(openssl dgst -sha256 -r "$f" | cut -d' ' -f1)" "$f"; done) > "$1/SHA256SUMS.txt"
  openssl pkeyutl -sign -inkey "$t/key.pem" -rawin -in "$1/SHA256SUMS.txt" -out "$1/s.bin"
  od -An -v -tx1 < "$1/s.bin" | tr -d ' \n' > "$1/SHA256SUMS.txt.sig"
}
ok() { if (verify_release "$@") >/dev/null; then echo "ok: $*"; else echo "FAIL (refused): $*"; exit 1; fi; }
no() { if (verify_release "$@") >/dev/null; then echo "FAIL (accepted): $*"; exit 1; else echo "ok, refused: $*"; fi; }
release "$t/good" 1.21.0-26
ok "$t/good" v1.21.0-26 aquamarine-site.tar.gz
no "$t/good" v1.21.0-27 aquamarine-site.tar.gz            # signed sums of another release
cp -r "$t/good" "$t/swapped"; printf 'evil' > "$t/swapped/aquamarine-site.tar.gz"
no "$t/swapped" v1.21.0-26 aquamarine-site.tar.gz         # archive swapped
cp -r "$t/good" "$t/resigned"; sed -i 's/^[0-9a-f]*  aquamarine/0000  aquamarine/' "$t/resigned/SHA256SUMS.txt"
no "$t/resigned" v1.21.0-26 aquamarine-site.tar.gz        # sums changed after signing
cp -r "$t/good" "$t/nosig"; rm "$t/nosig/SHA256SUMS.txt.sig"
no "$t/nosig" v1.21.0-26 aquamarine-site.tar.gz           # unsigned
PUBKEY=$(printf '%064d' 0 | tr 0 1)
no "$t/good" v1.21.0-26 aquamarine-site.tar.gz            # another key
echo "site verify: all good"
