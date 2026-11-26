#!/usr/bin/env bash
# The release APK (shrunk by R8) on an emulator: the vault key moves into the
# Android Keystore, the plain file goes only after a restart of the phone,
# and a key that can't be had leaves the data alone (src/vault_key.rs).
# Needs an image where `adb root` works (not a Play Store one).
set -euo pipefail
apk="$1"
aapt="$(ls -d "$ANDROID_HOME"/build-tools/*/aapt2 | sort -V | tail -1)"
pkg="$("$aapt" dump packagename "$apk")"
echo "package: $pkg"

manifest="$("$aapt" dump xmltree --file AndroidManifest.xml "$apk")"
# (older aapt2 prints a boolean as "(type 0x12)0x0")
for attr in 'allowBackup[^=]*=(false|\(type 0x12\)0x0)' dataExtractionRules fullBackupContent; do
  grep -qE "$attr" <<<"$manifest" || { echo "::error::the release manifest has no $attr"; exit 1; }
done

fail() {
  echo "::error::$1"
  adb logcat -d | grep -iE 'AquaKeystore|aquamarine|AndroidRuntime|RustStdoutStderr|FATAL' | tail -150 || true
  exit 1
}
root() { adb root >/dev/null || fail "adb root refused: use an image without Play"; adb wait-for-device; sleep 2; }
start() {
  adb shell am force-stop "$pkg"
  adb logcat -c
  adb shell monkey -p "$pkg" -c android.intent.category.LAUNCHER 1 >/dev/null
}
note() {
  for _ in $(seq 1 90); do
    adb logcat -d -s AquaKeystore:I | grep -qE "AquaKeystore.*: $1" && { sleep 3; alive; echo "ok: $1"; return; }
    sleep 2
  done
  fail "no '$1' from the Keystore"
}
alive() {
  adb logcat -d | grep -qE 'FATAL EXCEPTION|Fatal signal' && fail "the app crashed"
  adb shell pidof "$pkg" >/dev/null || fail "the app isn't running"
}
has() { adb shell "[ -f '$dir/$1' ] && echo yes || echo no" | tr -d '\r'; }
expect() { [ "$(has "$1")" = "$2" ] || { adb shell ls -la "$dir"; fail "$1 present: expected $2"; }; }

root
adb install -g "$apk" >/dev/null

echo "== first start: wrapped, the plain file kept"
start
note pending
dir="$(adb shell "find /data/data/$pkg -name master.key.ks" | tr -d '\r' | head -1)"
[ -n "$dir" ] || fail "no master.key.ks"
dir="${dir%/*}"
expect master.key yes
expect vault.sqlite yes

echo "== an account from the previous release (the plain key file only): moved in, the same key"
adb shell am force-stop "$pkg"
before="$(adb shell "sha256sum '$dir/master.key'" | tr -d '\r')"
adb shell "rm '$dir/master.key.ks' '$dir/keystore.json'"
start
note pending
expect master.key yes
expect master.key.ks yes
[ "$(adb shell "sha256sum '$dir/master.key'" | tr -d '\r')" = "$before" ] || fail "the key changed"

echo "== the app again, the same start of the phone: still kept"
start
note pending
expect master.key yes

echo "== after a restart of the phone: the plain file goes"
adb reboot
adb wait-for-device
for _ in $(seq 1 150); do
  [ "$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ] && break
  sleep 2
done
sleep 10
root
start
note confirmed
expect master.key no
expect master.key.ks yes

echo "== and opens with the wrapped key alone"
start
note confirmed

echo "== a key the Keystore can't open: the recovery screen, nothing removed, no new key"
adb shell am force-stop "$pkg"
adb shell "echo QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE= > '$dir/master.key.ks'"
size_before="$(adb shell "stat -c %s '$dir/vault.sqlite'" | tr -d '\r')"
start
note locked
expect master.key no
expect master.key.ks yes
expect vault.sqlite yes
[ "$(adb shell "stat -c %s '$dir/vault.sqlite'" | tr -d '\r')" = "$size_before" ] || fail "the vault changed"
echo "all Keystore checks passed"
