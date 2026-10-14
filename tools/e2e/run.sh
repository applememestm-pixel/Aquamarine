#!/usr/bin/env bash
# End-to-end check of the real Linux client (WebKitGTK) — no mocks.
#   tools/e2e/run.sh            article with a video + music, playback,
#                               reader; prints main-thread stalls, CPU and fps
#   tools/e2e/run.sh x.py ...   run these scripts (after registering) instead
# Needs: Xvfb, webkit2gtk-driver (WebKitWebDriver), tauri-driver
# (cargo install tauri-driver), python3 + selenium, ffmpeg, and release builds
# of aqua-server and the client (cargo build --release --features custom-protocol).
set -euo pipefail
HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
export E2E_WORK="${E2E_WORK:-$(mktemp -d /tmp/aqua-e2e.XXXX)}"
cd "$E2E_WORK"; mkdir -p srv data
[ -f clip.mp4 ] || ffmpeg -loglevel error -y -f lavfi -i testsrc2=size=1280x720:rate=30 -f lavfi -i sine=frequency=500 -t 60 -c:v libx264 -preset ultrafast -pix_fmt yuv420p -c:a aac -shortest -movflags +faststart clip.mp4
[ -f big.mp4 ] || ffmpeg -loglevel error -y -f lavfi -i testsrc2=size=1920x1080:rate=30 -t 120 -c:v libx264 -preset ultrafast -b:v 20M -pix_fmt yuv420p big.mp4
[ -f song.mp3 ] || ffmpeg -loglevel error -y -f lavfi -i "sine=frequency=330:duration=176" -b:a 192k song.mp3
Xvfb :99 -screen 0 1600x1000x24 >/dev/null 2>&1 & XV=$!
(cd srv && AQUA_DEV_CODES=1 AQUA_LISTEN=127.0.0.1:7777 AQUA_DIR_DB="$E2E_WORK/srv/dir.sqlite" "$ROOT/target/release/aqua-server" > server.log 2>&1) & SRV=$!
sleep 2
DISPLAY=:99 AQUA_DATA_DIR="$E2E_WORK/data" WEBKIT_DISABLE_DMABUF_RENDERER=1 tauri-driver --native-driver "$(command -v WebKitWebDriver)" > td.log 2>&1 & TD=$!
trap 'kill $TD $SRV $XV 2>/dev/null || true' EXIT
sleep 1
python3 "$HERE/register.py"
if [ $# -gt 0 ]; then
  for s in "$@"; do python3 "$HERE/$s"; done   # e.g. run.sh audio_probe.py
else
  python3 "$HERE/article_media.py"
fi
echo "screenshots: $E2E_WORK"
