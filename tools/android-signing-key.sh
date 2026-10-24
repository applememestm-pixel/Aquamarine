#!/usr/bin/env bash
# Makes the one signing key every Android build is signed with (release.yml,
# "Sign APK"). With it each new APK installs over the previous one and the
# in-app updates work; without it CI signs every build with a new throwaway
# key and Android refuses to update ("package conflicts" / reinstall).
#
#   tools/android-signing-key.sh [file.jks]
#
# Prints the three GitHub secrets to add (Settings → Secrets and variables →
# Actions → New repository secret). Keep the .jks file and the password
# somewhere safe and private: whoever has them can sign "updates" of the app,
# and losing them means everyone reinstalls once more.
set -euo pipefail
out="${1:-aquamarine-release.jks}"
[ -e "$out" ] && { echo "$out already exists — not overwriting it." >&2; exit 1; }
command -v keytool >/dev/null || { echo "keytool not found: install a JDK (Android Studio ships one: <studio>/jbr/bin)." >&2; exit 1; }
pass="$(head -c 32 /dev/urandom | base64 | tr -dc 'A-Za-z0-9' | head -c 28)"
alias=aquamarine
keytool -genkeypair -keystore "$out" -storetype PKCS12 -storepass "$pass" -keypass "$pass" \
  -alias "$alias" -keyalg RSA -keysize 4096 -validity 36500 \
  -dname "CN=Aquamarine, O=Aquamarine" >/dev/null
echo
echo "Key: $out — keep it and the password private, and back them up."
echo
echo "Add these repository secrets on GitHub:"
echo "  ANDROID_KEY_ALIAS          $alias"
echo "  ANDROID_KEYSTORE_PASSWORD  $pass"
echo "  ANDROID_KEYSTORE_B64       (the line below)"
echo
base64 -w0 "$out" 2>/dev/null || base64 "$out" | tr -d '\n'
echo
