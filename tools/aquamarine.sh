#!/usr/bin/env bash
# Launches Aquamarine. WebKitGTK on NVIDIA + Wayland often shows a blank or
# flickering window with its DMA-BUF renderer - switch that off.
DIR="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
export WEBKIT_DISABLE_DMABUF_RENDERER=1
exec "$DIR/aquamarine" "$@"
