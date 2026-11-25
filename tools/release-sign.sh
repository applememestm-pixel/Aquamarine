#!/usr/bin/env bash
# Release signing (AQUA-SEC-012): one place for what release.yml and the
# signing check do with the Ed25519 key in $UPDATE_SIGNING_KEY (PEM).
#   release-sign.sh pub              → the key's public half, hex (64)
#   release-sign.sh sign SUMS SIGOUT → signature over the exact file, hex
# The private key goes to a 0600 temp file outside the workspace and is
# removed on every exit path; it is never echoed.
set -euo pipefail
: "${UPDATE_SIGNING_KEY:?UPDATE_SIGNING_KEY is not set}"
tmp=$(mktemp -d "${RUNNER_TEMP:-/tmp}/aqsign.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
umask 077
printf '%s\n' "$UPDATE_SIGNING_KEY" > "$tmp/key.pem"
hexof() { od -An -v -tx1 | tr -d ' \n'; }
case "${1:-}" in
  pub)
    openssl pkey -in "$tmp/key.pem" -pubout -outform DER | tail -c 32 | hexof
    echo ;;
  sign)
    openssl pkeyutl -sign -inkey "$tmp/key.pem" -rawin -in "$2" -out "$tmp/sig.bin"
    hexof < "$tmp/sig.bin" > "$3" ;;
  *) echo "usage: $0 pub | sign SUMS SIGOUT" >&2; exit 2 ;;
esac
