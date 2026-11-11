#!/usr/bin/env bash
# `apt-get update` for CI runners. GitHub's Ubuntu runners read packages from
# Azure's mirror (azure.archive.ubuntu.com); when it stops answering, the
# step hangs for the whole job timeout (seen 40 minutes, twice) or ends with
# nothing downloaded. Try it briefly, then switch to archive.ubuntu.com.
set -u
APT=(apt-get -o Acquire::Retries=3 -o Acquire::http::Timeout=20 -o Acquire::https::Timeout=20)
log=$(mktemp)
if timeout 150 "${APT[@]}" update 2>&1 | tee "$log"; [ "${PIPESTATUS[0]}" -eq 0 ] && ! grep -qE "^(Ign|Err):.*azure\.archive|Failed to fetch" "$log"; then
  exit 0
fi
echo "::warning::Azure's Ubuntu mirror isn't answering - using archive.ubuntu.com"
[ -f /etc/apt/apt-mirrors.txt ] && printf 'http://archive.ubuntu.com/ubuntu/\tpriority:1\n' > /etc/apt/apt-mirrors.txt
for f in /etc/apt/sources.list /etc/apt/sources.list.d/*.list /etc/apt/sources.list.d/*.sources; do
  [ -f "$f" ] && sed -i -E 's#https?://azure\.archive\.ubuntu\.com#http://archive.ubuntu.com#g' "$f"
done
timeout 300 "${APT[@]}" update
