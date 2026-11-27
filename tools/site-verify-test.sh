#!/usr/bin/env bash
# Test of tools/aquamarine-site.sh::verify_release (AQUA-SEC-012, 27.11):
# the site is taken only from a release whose SHA256SUMS.txt is signed by
# the built-in key, names that release (VERSION) and lists the archive.
# A key of its own here; the script's functions are loaded, not run.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
t=$(mktemp -d); trap 'rm -rf "$t"' EXIT
# The script's helpers, without running it.
sed -n '/^say()/,/^die()/p; /^unhex()/,/^sha256()/p; /^verify_release()/,/^}/p; /^image_ref()/,/^}/p' "$HERE/aquamarine-site.sh" > "$t/fns.sh"
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
# Docker: only a signed IMAGE-site.txt naming the image by digest.
img="ghcr.io/applememestm-pixel/aquamarine-site@sha256:$(printf '%064d' 0 | tr 0 a)"
PUBKEY=$(openssl pkey -in "$t/key.pem" -pubout -outform DER | tail -c 32 | od -An -v -tx1 | tr -d ' \n')
mkdir -p "$t/img"; printf '%s\n' "$img" > "$t/img/IMAGE-site.txt"; printf '%s' 1.21.0-26 > "$t/img/VERSION"
(cd "$t/img" && for f in IMAGE-site.txt VERSION; do printf '%s  %s\n' "$(openssl dgst -sha256 -r "$f" | cut -d' ' -f1)" "$f"; done) > "$t/img/SHA256SUMS.txt"
openssl pkeyutl -sign -inkey "$t/key.pem" -rawin -in "$t/img/SHA256SUMS.txt" -out "$t/img/s.bin"
od -An -v -tx1 < "$t/img/s.bin" | tr -d ' \n' > "$t/img/SHA256SUMS.txt.sig"
ok "$t/img" v1.21.0-26 IMAGE-site.txt
[ "$(image_ref "$t/img/IMAGE-site.txt" site)" = "$img" ] && echo "ok: image by digest" || { echo "FAIL: image ref"; exit 1; }
for bad in "ghcr.io/applememestm-pixel/aquamarine-server@sha256:$(printf '%064d' 0)" "ghcr.io/applememestm-pixel/aquamarine-site:latest" "ghcr.io/evil/aquamarine-site@sha256:$(printf '%064d' 0)" "ghcr.io/applememestm-pixel/aquamarine-site@sha256:abc"; do
  printf '%s\n' "$bad" > "$t/bad.txt"
  if (image_ref "$t/bad.txt" site) >/dev/null; then echo "FAIL (accepted): $bad"; exit 1; else echo "ok, refused: $bad"; fi
done
cp -r "$t/img" "$t/img2"; printf '%s\n' "${img%a}b" > "$t/img2/IMAGE-site.txt"
no "$t/img2" v1.21.0-26 IMAGE-site.txt                    # digest changed after signing
echo "site verify: all good"
