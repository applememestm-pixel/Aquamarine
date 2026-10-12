#!/usr/bin/env bash
# Same as tools/publish-github.sh (kept here for convenience).
exec bash "$(cd "$(dirname "$(readlink -f "$0")")" && pwd)/../tools/publish-github.sh" "$@"
