# shellcheck shell=bash
# Finds Android SDK / NDK / Java on this Linux PC and exports what Tauri,
# Gradle and cargo need. SOURCE it, don't run it:
#     . android/env.sh            # find only
#     . android/env.sh install    # also install a missing SDK / NDK
# (Windows equivalent: env.bat + find-android.ps1)
#
# Note: an SDK copied from Windows (e.g. <drive>/Android/Sdk with adb.exe and
# an NDK built for windows-x86_64) does NOT work on Linux - a Linux SDK is
# needed; `install` puts one into ~/Android/Sdk.

_aq_env() {
  local install="${1:-}"
  local NDK_PKG='ndk;27.2.12479018'
  local CMDLINE_ZIP='https://dl.google.com/android/repository/commandlinetools-linux-13114758_latest.zip'
  local c sdk="" java="" ndk="" studio=""

  # ---- SDK: env vars, Android Studio's own setting, usual folders, adb on PATH
  local cands=("${ANDROID_HOME:-}" "${ANDROID_SDK_ROOT:-}" "$HOME/Android/Sdk" "/opt/android-sdk")
  while IFS= read -r c; do
    cands+=("$(grep -o 'android.sdk.path[^>]*value="[^"]*"' "$c" | sed 's/.*value="//; s/"$//' | head -1 || true)")
  done < <(ls -t "$HOME"/.config/Google/AndroidStudio*/options/other.xml 2>/dev/null || true)
  command -v adb >/dev/null && cands+=("$(dirname "$(dirname "$(readlink -f "$(command -v adb)")")")")
  for c in "${cands[@]}"; do
    [ -n "$c" ] && [ -x "$c/platform-tools/adb" ] && { sdk="$(readlink -f "$c")"; break; }
  done

  # ---- Java: Android Studio's bundled JBR first, then JAVA_HOME, then system JDKs.
  # Gradle/AGP want 17-21; a newer JDK (e.g. Arch's default java-27) is only a
  # last resort: Gradle may refuse it ("Unsupported class file major version").
  local studios=(/opt/android-studio "$HOME/android-studio" "$HOME/.local/share/JetBrains/Toolbox/apps/android-studio" /usr/share/android-studio)
  for c in "${studios[@]}"; do [ -x "$c/bin/studio.sh" ] && { studio="$c"; break; }; done
  local jcands=("${studio:+$studio/jbr}" "${JAVA_HOME:-}" /usr/lib/jvm/java-21-openjdk /usr/lib/jvm/java-17-openjdk /usr/lib/jvm/*/ /usr/lib/jvm/default)
  local newer="" v
  for c in "${jcands[@]}"; do
    [ -n "$c" ] && [ -x "$c/bin/java" ] || continue
    v="$("$c/bin/java" -version 2>&1 | head -1 | grep -o '"[0-9]*' | tr -d '"' || true)"
    if [ "${v:-0}" -ge 17 ] && [ "${v:-0}" -le 21 ]; then java="$(readlink -f "$c")"; break; fi
    [ "${v:-0}" -gt 21 ] && [ -z "$newer" ] && newer="$(readlink -f "$c")"
  done
  if [ -z "$java" ] && [ -n "$newer" ]; then
    java="$newer"
    echo "[!] Only a JDK newer than 21 found ($java). If Gradle fails: sudo pacman -S jdk21-openjdk" >&2
  fi
  if [ -z "$java" ]; then
    echo "[!] Java 17-21 not found - sudo pacman -S jdk21-openjdk (or install Android Studio: yay -S android-studio)" >&2
    return 1
  fi
  export JAVA_HOME="$java"

  # ---- Missing SDK / NDK → install through Google's command-line tools
  _aq_sdkmanager() {
    local sm; sm="$(ls "$1"/cmdline-tools/*/bin/sdkmanager 2>/dev/null | head -1)"
    if [ -z "$sm" ]; then
      echo "[*] Downloading Android command-line tools…" >&2
      command -v unzip >/dev/null || { echo "[!] unzip missing: sudo pacman -S unzip" >&2; return 1; }
      local tmp; tmp="$(mktemp -d)"
      curl -fL --progress-bar -o "$tmp/clt.zip" "$CMDLINE_ZIP" && unzip -q "$tmp/clt.zip" -d "$tmp" || { rm -rf "$tmp"; return 1; }
      mkdir -p "$1/cmdline-tools"; rm -rf "$1/cmdline-tools/latest"
      mv "$tmp/cmdline-tools" "$1/cmdline-tools/latest"; rm -rf "$tmp"
      sm="$1/cmdline-tools/latest/bin/sdkmanager"
    fi
    echo "$sm"
  }
  if [ -z "$sdk" ] && [ "$install" = install ]; then
    sdk="$HOME/Android/Sdk"; mkdir -p "$sdk"
    local sm; sm="$(_aq_sdkmanager "$sdk")" || { echo "[!] Could not get Android command-line tools." >&2; return 1; }
    echo "[*] Installing platform-tools into $sdk (accept the licenses with 'y')…" >&2
    yes | "$sm" --sdk_root="$sdk" --licenses >/dev/null 2>&1 || true
    "$sm" --sdk_root="$sdk" --install platform-tools || return 1
  fi
  if [ -z "$sdk" ]; then
    echo "[!] Android SDK not found. Open Android Studio once (it installs the SDK) or run: . android/env.sh install" >&2
    return 1
  fi

  _aq_find_ndk() {
    local n
    if [ -n "${NDK_HOME:-}" ] && [ -f "$NDK_HOME/source.properties" ] && [ -d "$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64" ]; then echo "$NDK_HOME"; return; fi
    for n in $(ls -d "$sdk"/ndk/*/ 2>/dev/null | sort -Vr) "$sdk/ndk-bundle/"; do
      [ -d "$n/toolchains/llvm/prebuilt/linux-x86_64" ] && { readlink -f "$n"; return 0; }
    done
    return 0
  }
  ndk="$(_aq_find_ndk)"
  if [ -z "$ndk" ] && [ "$install" = install ]; then
    local sm; sm="$(_aq_sdkmanager "$sdk")" || { echo "[!] Install the NDK via Android Studio → SDK Manager → SDK Tools → NDK (Side by side)." >&2; return 1; }
    echo "[*] Installing $NDK_PKG (~1 GB). Type 'y' if it asks to accept the license." >&2
    yes | "$sm" --sdk_root="$sdk" --licenses >/dev/null 2>&1 || true
    "$sm" --sdk_root="$sdk" --install "$NDK_PKG" || return 1
    ndk="$(_aq_find_ndk)"
  fi
  if [ -z "$ndk" ]; then
    echo "[!] Android NDK (Linux) not installed in $sdk. Run android/1-setup.sh - it installs it." >&2
    return 1
  fi

  export ANDROID_HOME="$sdk" ANDROID_SDK_ROOT="$sdk" NDK_HOME="$ndk"
  # Rust -> Android linkers (same as the .cargo/config.toml patch-project.sh writes)
  local bin="$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin"
  export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$bin/aarch64-linux-android24-clang"
  export CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER="$bin/armv7a-linux-androideabi24-clang"
  export CARGO_TARGET_I686_LINUX_ANDROID_LINKER="$bin/i686-linux-android24-clang"
  export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$bin/x86_64-linux-android24-clang"
  export PATH="$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$HOME/.cargo/bin:$PATH"
  [ -n "$studio" ] && export AQ_STUDIO="$studio/bin/studio.sh"
  if [ -z "${AQ_STUDIO:-}" ] && command -v android-studio >/dev/null; then AQ_STUDIO="$(command -v android-studio)"; export AQ_STUDIO; fi

  # LAN address for the phone to reach the dev server (skip VPN / virtual adapters)
  local lan
  lan="$(ip -4 -o addr show scope global 2>/dev/null | awk '{print $2, $4}' \
    | grep -Ev '^(docker|br-|veth|virbr|tun|tap|wg|tailscale|zt|hiddify|singbox|vmnet|vboxnet)' \
    | awk '{sub(/\/.*/,"",$2); print $2}' | grep -E '^(192\.168\.|10\.|172\.(1[6-9]|2[0-9]|3[01])\.)' | head -1 || true)"
  [ -n "$lan" ] && export AQ_LAN_IP="$lan"

  echo "[env] ANDROID_HOME = $ANDROID_HOME"
  echo "[env] NDK_HOME     = $NDK_HOME"
  echo "[env] JAVA_HOME    = $JAVA_HOME"
}
_aq_env "$@"
