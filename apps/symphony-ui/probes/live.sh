#!/bin/bash
# The replies a first visit of `/` and `/tasks` reads from a running Symphony server, saved for
# examples/ssr_size.rs, each printed with its gzip -6 bytes. Read-only GETs, the requests the
# client makes with its defaults (the last day, no search, the first page of each column).
#
#     bash apps/symphony-ui/probes/live.sh BASE OUT
#
# BASE is the server's address as `just symphony` prints it (http://127.0.0.1:8765 for the shared one).
# Then: cargo run -p symphony-ui --example ssr_size -- OUT/board.json OUT/counts.json OUT/ready.json
# OUT/in_progress.json OUT/review.json OUT/blocked.json OUT/done.json
set -euo pipefail

usage="usage: live.sh BASE OUT"
base="${1:?$usage}"
out="${2:?$usage}"
mkdir -p "$out"

fetch() { # NAME PATH
    curl --silent --show-error --fail "$base$2" -o "$out/$1.json"
    printf '%8d %s\n' "$(gzip -6 -n -c "$out/$1.json" | wc -c)" "$2"
}

fetch board /api/board
fetch activity /api/activity
fetch pending /api/pending
fetch counts '/api/task-board?counts=1&q=&days=1'
for status in ready in_progress review blocked done; do
    fetch "$status" "/api/task-board?q=&status=$status&days=1&older=0"
done
