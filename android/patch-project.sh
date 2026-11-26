#!/usr/bin/env bash
# Brings the generated Tauri Android project (crates/aqua-client-ui/gen/android)
# up to what Aquamarine needs. Safe to run any number of times.
# Expects android/env.sh to have been sourced (NDK_HOME, ANDROID_HOME).
set -euo pipefail
HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
PROJ="$ROOT/crates/aqua-client-ui/gen/android"
APP="$PROJ/app"
[ -d "$APP" ] || { echo "Android project not found: $PROJ (run android/1-setup.sh)" >&2; exit 1; }

# 1. Permissions for calls (mic, camera), notifications, legacy Downloads write.
M="$APP/src/main/AndroidManifest.xml"
if ! grep -q RECORD_AUDIO "$M"; then
  AQ_PERMS='<uses-permission android:name="android.permission.ACCESS_LOCAL_NETWORK" />
    <uses-permission android:name="android.permission.RECORD_AUDIO" />
    <uses-permission android:name="android.permission.CAMERA" />
    <uses-permission android:name="android.permission.MODIFY_AUDIO_SETTINGS" />
    <uses-permission android:name="android.permission.POST_NOTIFICATIONS" />
    <uses-permission android:name="android.permission.WRITE_EXTERNAL_STORAGE" android:maxSdkVersion="28" />
    <uses-feature android:name="android.hardware.camera" android:required="false" />
    ' perl -0pi -e 's/<application/$ENV{AQ_PERMS}<application/' "$M"
  echo '[patch] permissions added to AndroidManifest.xml'
fi
# 1b. Android 17 local-network permission for projects patched before it existed.
if ! grep -q ACCESS_LOCAL_NETWORK "$M"; then
  perl -0pi -e 's|(<uses-permission android:name="android.permission.RECORD_AUDIO" />)|<uses-permission android:name="android.permission.ACCESS_LOCAL_NETWORK" />\n    $1|' "$M"
  echo '[patch] ACCESS_LOCAL_NETWORK added'
fi
# 1b2. Videos in picture-in-picture (MediaPlayer → AquaRelay.enterPip).
if ! grep -q supportsPictureInPicture "$M"; then
  perl -0pi -e 's/<activity/<activity android:supportsPictureInPicture="true"/' "$M"
  echo '[patch] picture-in-picture allowed'
fi
for permission in ACCESS_NETWORK_STATE BLUETOOTH_ADVERTISE; do
  if ! grep -q "android.permission.$permission" "$M"; then
    AQ_PERM="$permission" perl -0pi -e 's|<application|<uses-permission android:name="android.permission.$ENV{AQ_PERM}" />\n    <application|' "$M"
  fi
done
# 1c. MainActivity that asks for local-network access on Android 17+.
MA="$(find "$APP/src/main/java" -name MainActivity.kt 2>/dev/null | head -1)"
if [ -n "$MA" ]; then
  cp -f "$HERE/MainActivity.kt" "$MA"
  echo '[patch] MainActivity requests local network access'
fi

# 1c'. Nearby delivery over Bluetooth (crates/aqua-client-ui/src/mesh_android.rs).
if [ -n "$MA" ]; then
  cp -f "$HERE/AquaMesh.kt" "$(dirname "$MA")/AquaMesh.kt"
  echo '[patch] AquaMesh.kt (Bluetooth nearby delivery)'
fi
# 1c''. Server on this device (crates/aqua-client-ui/src/relay_host.rs).
if [ -n "$MA" ]; then
  cp -f "$HERE/AquaRelay.kt" "$(dirname "$MA")/AquaRelay.kt"
  mkdir -p "$APP/src/main/res/drawable"
  cp -f "$HERE/res/drawable/ic_stat_aquamarine.xml" "$APP/src/main/res/drawable/ic_stat_aquamarine.xml"
  echo '[patch] AquaRelay.kt (server on this device)'
fi
if ! grep -q AquaRelayService "$M"; then
  AQ_FGS='<uses-permission android:name="android.permission.FOREGROUND_SERVICE" />
    <uses-permission android:name="android.permission.FOREGROUND_SERVICE_SPECIAL_USE" />
    <uses-permission android:name="android.permission.WAKE_LOCK" />
    <uses-permission android:name="android.permission.READ_MEDIA_IMAGES" />
    <uses-permission android:name="android.permission.READ_MEDIA_VIDEO" />
    <uses-permission android:name="android.permission.READ_MEDIA_VISUAL_USER_SELECTED" />
    <uses-permission android:name="android.permission.READ_EXTERNAL_STORAGE" android:maxSdkVersion="32" />
    ' perl -0pi -e 's/<application/$ENV{AQ_FGS}<application/' "$M"
  AQ_SVC='<service android:name=".AquaRelayService" android:exported="false" android:foregroundServiceType="specialUse"><property android:name="android.app.PROPERTY_SPECIAL_USE_FGS_SUBTYPE" android:value="Hosts the Aquamarine relay server for the user and their contacts" /></service>
    </application>' perl -0pi -e 's|</application>|$ENV{AQ_SVC}|' "$M"
  echo '[patch] foreground service for the server added'
fi
# 1c''. Screen sharing in calls (MainActivity.kt: AquaScreenService) and
#       nearby delivery (Wi-Fi broadcasts, Bluetooth visibility).
if ! grep -q AquaScreenService "$M"; then
  AQ_SCR='<uses-permission android:name="android.permission.FOREGROUND_SERVICE_MEDIA_PROJECTION" />
    <uses-permission android:name="android.permission.CHANGE_WIFI_MULTICAST_STATE" />
    <uses-permission android:name="android.permission.ACCESS_WIFI_STATE" />
    ' perl -0pi -e 's/<application/$ENV{AQ_SCR}<application/' "$M"
  AQ_SCS='<service android:name=".AquaScreenService" android:exported="false" android:foregroundServiceType="mediaProjection" />
    </application>' perl -0pi -e 's|</application>|$ENV{AQ_SCS}|' "$M"
  echo '[patch] screen sharing service + nearby permissions added'
fi
# 1c-sync. Background messages, calls in the background, the reminder after a
#          reboot (AquaRelay.kt: AquaSyncService, AquaCallService, AquaBootReceiver).
if ! grep -q AquaSyncService "$M"; then
  AQ_SYP='<uses-permission android:name="android.permission.FOREGROUND_SERVICE_REMOTE_MESSAGING" />
    <uses-permission android:name="android.permission.REQUEST_IGNORE_BATTERY_OPTIMIZATIONS" />
    ' perl -0pi -e 's/<application/$ENV{AQ_SYP}<application/' "$M"
  AQ_SYS='<service android:name=".AquaSyncService" android:exported="false" android:foregroundServiceType="remoteMessaging" />
    </application>' perl -0pi -e 's|</application>|$ENV{AQ_SYS}|' "$M"
  echo '[patch] background connection service added'
fi
if ! grep -q AquaCallService "$M"; then
  AQ_CAP='<uses-permission android:name="android.permission.FOREGROUND_SERVICE_MICROPHONE" />
    <uses-permission android:name="android.permission.FOREGROUND_SERVICE_CAMERA" />
    <uses-permission android:name="android.permission.RECEIVE_BOOT_COMPLETED" />
    ' perl -0pi -e 's/<application/$ENV{AQ_CAP}<application/' "$M"
  AQ_CAS='<service android:name=".AquaCallService" android:exported="false" android:foregroundServiceType="microphone|camera" />
    <receiver android:name=".AquaBootReceiver" android:exported="false"><intent-filter><action android:name="android.intent.action.BOOT_COMPLETED" /><action android:name="android.intent.action.MY_PACKAGE_REPLACED" /></intent-filter></receiver>
    </application>' perl -0pi -e 's#</application>#$ENV{AQ_CAS}#' "$M"
  echo '[patch] call service and boot reminder added'
fi
# 1c'''. Home-screen widget (android/widget).
if [ -n "$MA" ]; then
  cp -f "$HERE/widget/AquaWidget.kt" "$(dirname "$MA")/AquaWidget.kt"
  mkdir -p "$APP/src/main/res"
  cp -rf "$HERE/widget/res/." "$APP/src/main/res/"
  echo '[patch] AquaWidget.kt (home-screen widget)'
fi
if ! grep -q AquaWidget "$M"; then
  AQ_WDG='<receiver android:name=".AquaWidget" android:exported="false"><intent-filter><action android:name="android.appwidget.action.APPWIDGET_UPDATE" /></intent-filter><meta-data android:name="android.appwidget.provider" android:resource="@xml/aqua_widget_info" /></receiver>
    </application>' perl -0pi -e 's|</application>|$ENV{AQ_WDG}|' "$M"
  echo '[patch] home-screen widget receiver added'
fi
if ! grep -q BLUETOOTH_CONNECT "$M"; then
  AQ_BT='<uses-permission android:name="android.permission.BLUETOOTH" android:maxSdkVersion="30" />
    <uses-permission android:name="android.permission.BLUETOOTH_ADMIN" android:maxSdkVersion="30" />
    <uses-permission android:name="android.permission.ACCESS_FINE_LOCATION" android:maxSdkVersion="30" />
    <uses-permission android:name="android.permission.BLUETOOTH_CONNECT" />
    <uses-permission android:name="android.permission.BLUETOOTH_SCAN" android:usesPermissionFlags="neverForLocation" />
    <uses-feature android:name="android.hardware.bluetooth" android:required="false" />
    ' perl -0pi -e 's/<application/$ENV{AQ_BT}<application/' "$M"
  echo '[patch] Bluetooth permissions added'
fi

# 1e. In-app updates (crates/aqua-client-ui/src/updates.rs): installing
#      the downloaded APK needs this permission.
if ! grep -q REQUEST_INSTALL_PACKAGES "$M"; then
  perl -0pi -e 's|<application|<uses-permission android:name="android.permission.REQUEST_INSTALL_PACKAGES" />\n    <application|' "$M"
  echo '[patch] REQUEST_INSTALL_PACKAGES (in-app updates)'
fi

# 1d. Plain HTTP to the app's own loopback media server (audio / video
#     playback); debug builds also to the dev server.
mkdir -p "$APP/src/main/res/xml" "$APP/src/debug/res/xml"
cp -f "$HERE/netsec/release.xml" "$APP/src/main/res/xml/network_security_config.xml"
cp -f "$HERE/netsec/debug.xml" "$APP/src/debug/res/xml/network_security_config.xml"
if ! grep -q networkSecurityConfig "$M"; then
  perl -0pi -e 's|<application|<application android:networkSecurityConfig="\@xml/network_security_config"|' "$M"
  echo '[patch] network security config (loopback media server)'
fi

# 1f. Nothing of the app in Android's backup or phone-to-phone copy
#     (keys, sessions, the vault).
cp -f "$HERE/backup/backup_rules.xml" "$HERE/backup/data_extraction_rules.xml" "$APP/src/main/res/xml/"
if ! grep -q dataExtractionRules "$M"; then
  perl -0pi -e 's|<application|<application android:allowBackup="false" android:fullBackupContent="\@xml/backup_rules" android:dataExtractionRules="\@xml/data_extraction_rules"|' "$M"
  echo '[patch] Android backup excluded'
fi

# 2. App icon.
ICONS="$ROOT/crates/aqua-client-ui/icons/android"
if [ -d "$ICONS" ]; then
  cp -rf "$ICONS/." "$APP/src/main/res/"
  echo '[patch] launcher icons copied'
fi

# 3. Gradle must call the Rust build through cargo's Tauri CLI.
while IFS= read -r f; do
  if grep -q '"""node"""' "$f"; then
    sed -i 's/"""node"""/"""cargo"""/g' "$f"
    echo '[patch] BuildTask now uses cargo'
  fi
done < <(find "$PROJ/buildSrc" -name BuildTask.kt 2>/dev/null)

# 4. Android Studio default build variant: "universal" with only the ABIs that
#    matter (phones = arm64, emulator = x86_64).
GP="$PROJ/gradle.properties"
if ! grep -q '^abiList=' "$GP"; then
  printf '\n# Aquamarine: ABIs built when running from Android Studio\nabiList=arm64-v8a,x86_64\narchList=arm64,x86_64\ntargetList=aarch64,x86_64\n' >> "$GP"
  echo '[patch] Studio builds arm64 + x86_64'
fi
BG="$APP/build.gradle.kts"
if ! grep -q aquamarine-default-variant "$BG"; then
  printf '\n// aquamarine-default-variant\nandroid {\n    productFlavors {\n        getByName("universal") { isDefault = true }\n    }\n}\n' >> "$BG"
  echo '[patch] default build variant: universalDebug'
fi

# 5. Rust -> Android linkers from the NDK. Without this cargo falls back to
#    `cc` ("linker `cc` not found") when Android Studio builds the Rust part.
#    Rewritten every run, so a config left over from Windows (.cmd linkers) is replaced.
if [ -n "${NDK_HOME:-}" ]; then
  BIN="$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
  if [ -d "$BIN" ]; then
    API=24   # = minSdk in app/build.gradle.kts
    mkdir -p "$ROOT/crates/aqua-client-ui/.cargo"
    cat > "$ROOT/crates/aqua-client-ui/.cargo/config.toml" <<EOT
# Generated by android/patch-project.sh for this PC (NDK paths). Not in git.
[target.aarch64-linux-android]
linker = "$BIN/aarch64-linux-android$API-clang"
[target.armv7-linux-androideabi]
linker = "$BIN/armv7a-linux-androideabi$API-clang"
[target.i686-linux-android]
linker = "$BIN/i686-linux-android$API-clang"
[target.x86_64-linux-android]
linker = "$BIN/x86_64-linux-android$API-clang"
EOT
    echo '[patch] .cargo/config.toml -> NDK linkers'
  fi
fi

# 6. Tell Gradle / Android Studio where the SDK is.
if [ -n "${ANDROID_HOME:-}" ]; then
  printf 'sdk.dir=%s\n' "$ANDROID_HOME" > "$PROJ/local.properties"
  echo "[patch] local.properties -> $ANDROID_HOME"
fi
# gradlew copied over from Windows may have lost its exec bit.
[ -f "$PROJ/gradlew" ] && chmod +x "$PROJ/gradlew" 2>/dev/null || true
echo '[patch] done'
