#!/usr/bin/env bash
# Launches Aquamarine. WebKitGTK on NVIDIA + Wayland often shows a blank or
# flickering window with its DMA-BUF renderer - switch that off.
#
# Diagnostics: everything the app, its web view (JS console) and GStreamer
# (warnings) print goes to logs/aquamarine-last.log next to this script, and
# logs/system.txt lists the media packages / GPU. Attach both to a bug report.
DIR="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
export WEBKIT_DISABLE_DMABUF_RENDERER=1
# Files copied from another disk/archive can lose the exec bit.
[ -f "$DIR/aquamarine" ] && [ ! -x "$DIR/aquamarine" ] && chmod +x "$DIR/aquamarine" 2>/dev/null
[ -f "$DIR/aqua-server" ] && [ ! -x "$DIR/aqua-server" ] && chmod +x "$DIR/aqua-server" 2>/dev/null
if [ ! -x "$DIR/aquamarine" ]; then
  echo "[X] $DIR/aquamarine is missing or not executable. Run: chmod +x \"$DIR/aquamarine\"" >&2
  exit 1
fi
# NVIDIA driver updated but not rebooted: the kernel module and the
# libraries differ, WebKit silently falls back to CPU drawing and lags.
if command -v nvidia-smi >/dev/null && nvidia-smi 2>&1 | grep -qi "version mismatch"; then
  msg="NVIDIA: driver/library version mismatch — restart the computer, otherwise Aquamarine is drawn without the GPU and lags."
  echo "[!] $msg" >&2
  command -v notify-send >/dev/null && notify-send -a Aquamarine "Aquamarine" "$msg" 2>/dev/null &
fi
LOGS="$DIR/logs"
if mkdir -p "$LOGS" 2>/dev/null && [ -w "$LOGS" ]; then
  {
    echo "date: $(date -Is)"
    echo "kernel: $(uname -r)"
    echo "session: ${XDG_SESSION_TYPE:-?} ${XDG_CURRENT_DESKTOP:-?}"
    command -v pacman >/dev/null && pacman -Q webkit2gtk-4.1 gstreamer gst-plugins-base gst-plugins-good \
      gst-plugins-bad gst-plugins-ugly gst-libav gst-plugin-pipewire gstreamer-vaapi ffmpeg pipewire \
      pipewire-pulse nvidia nvidia-open nvidia-utils libva-nvidia-driver mesa 2>&1
    command -v nvidia-smi >/dev/null && nvidia-smi --query-gpu=name,driver_version --format=csv,noheader 2>&1
    if command -v gst-inspect-1.0 >/dev/null; then
      echo "--- decoders / sinks"
      gst-inspect-1.0 2>/dev/null | grep -iE "h264|h265|mp3|mpg123|aac|opus|vorbis|vp9|av1|audiosink|pipewire|pulse" | head -60
    fi
  } > "$LOGS/system.txt" 2>&1
  [ -f "$LOGS/aquamarine-last.log" ] && mv -f "$LOGS/aquamarine-last.log" "$LOGS/aquamarine-prev.log"
  export GST_DEBUG="${GST_DEBUG:-2}" GST_DEBUG_NO_COLOR=1 AQUA_MEDIA_LOG=1
  exec "$DIR/aquamarine" "$@" > >(stdbuf -oL tee -a "$LOGS/aquamarine-last.log") 2>&1
fi
exec "$DIR/aquamarine" "$@"
