#!/usr/bin/env bash
# A mixed federation on one machine: this build (NEW), a release with node
# keys (KEYED) and one without (OLD), each its own process and data folder.
# Then: crates/aqua-server/tests/mixed_binaries.rs (see its header).
#   tools/fed-mixed.sh <new-binary> <keyed-binary> <old-binary> [data-root]
set -euo pipefail
NEW=$1 KEYED=$2 OLD=$3 ROOT=${4:-$(mktemp -d)}
SECRET=${AQUA_MIXED_SECRET:-mixed-network-secret-0123456789}
TOKEN=${AQUA_MIXED_ADMIN:-mixed-admin-token-0123456789abcdef}
P=(17771 17772 17773)
start() { # name bin port peers...
  local name=$1 bin=$2 port=$3; shift 3
  mkdir -p "$ROOT/$name"
  (cd "$ROOT/$name" && AQUA_LISTEN=127.0.0.1:$port AQUA_PUBLIC_URL=http://127.0.0.1:$port \
    AQUA_FED_SECRET=$SECRET AQUA_FED_PEERS=$(IFS=,; echo "$*") AQUA_ADMIN_TOKEN=$TOKEN \
    AQUA_DEV_CODES=1 AQUA_OPEN_SIGNUP=1 nohup "$bin" > "$ROOT/$name.log" 2>&1 & echo $! > "$ROOT/$name.pid")
}
u() { echo "http://127.0.0.1:$1"; }
start new "$NEW" ${P[0]} "$(u ${P[1]})" "$(u ${P[2]})"
start keyed "$KEYED" ${P[1]} "$(u ${P[0]})" "$(u ${P[2]})"
start old "$OLD" ${P[2]} "$(u ${P[0]})" "$(u ${P[1]})"
for p in "${P[@]}"; do for _ in $(seq 60); do curl -sf "$(u $p)/v1/stats" >/dev/null && break; sleep 0.5; done; done
echo "data: $ROOT"
echo "AQUA_MIXED=NEW=$(u ${P[0]}),KEYED=$(u ${P[1]}),OLD=$(u ${P[2]}) AQUA_MIXED_ADMIN=$TOKEN AQUA_MIXED_SECRET=$SECRET"
echo "stop: kill \$(cat $ROOT/*.pid)"
