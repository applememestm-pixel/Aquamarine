#!/usr/bin/env bash
# Add these files' lines to the release's SHA256SUMS.txt right after a build
# job attaches them (release.yml). The apps install an update only once its
# file is in that list, and the `checksums` job writes it after *every*
# build — one slow runner (Intel macOS waits for a machine for half an hour)
# held back the update on Windows, Linux and Android. The `checksums` job
# still writes the whole list at the end.
#
# Jobs finishing at the same time may overwrite each other's upload: the
# list is read back and the upload repeated until our lines are in it.
#   tools/release_sums_add.sh <tag> <file>...
set -uo pipefail
tag=$1; shift
w=$(mktemp -d)
hash_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum -- "$1" | awk '{print $1}' | tr -d '\\'
  else shasum -a 256 -- "$1" | awk '{print $1}'; fi
}
: > "$w/mine"
for f in "$@"; do
  [ -f "$f" ] || continue
  printf '%s  %s\n' "$(hash_of "$f")" "$(basename "$f")" >> "$w/mine"
done
[ -s "$w/mine" ] || exit 0
cat "$w/mine"
for i in 1 2 3 4 5 6; do
  rm -f "$w/SHA256SUMS.txt"
  gh release download "$tag" --repo "$GITHUB_REPOSITORY" --pattern SHA256SUMS.txt --dir "$w" 2>/dev/null || true
  [ -f "$w/SHA256SUMS.txt" ] || : > "$w/SHA256SUMS.txt"
  if [ "$i" -gt 1 ] && awk 'NR==FNR { have[$0]=1; next } !($0 in have) { miss=1 } END { exit miss }' "$w/SHA256SUMS.txt" "$w/mine"; then
    echo "SHA256SUMS.txt has these files"
    # The site's updates.json too (phones read it when GitHub's API limits them).
    gh workflow run pages.yml --repo "$GITHUB_REPOSITORY" --ref main >/dev/null 2>&1 || true
    exit 0
  fi
  awk 'NR==FNR { mine[$2]=1; next } !($2 in mine) && NF' "$w/mine" "$w/SHA256SUMS.txt" > "$w/rest"
  cat "$w/rest" "$w/mine" | sort -k2 > "$w/out" && mv "$w/out" "$w/SHA256SUMS.txt"
  gh release upload "$tag" "$w/SHA256SUMS.txt" --repo "$GITHUB_REPOSITORY" --clobber || true
  sleep $((5 + RANDOM % 10))
done
echo "::warning::SHA256SUMS.txt not updated early — the checksums job writes it at the end"
exit 0
