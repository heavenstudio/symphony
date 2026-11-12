#!/bin/bash
# Run only through cargo symphony-ui dev (the cargo wrapper owns this tree's incremental folder).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:?dev.sh needs CARGO_TARGET_DIR from the workspace Cargo entrypoint}"
# The common dir: SYMPHONY_GIT_COMMON_DIR, else git's.
common="${SYMPHONY_GIT_COMMON_DIR:-$(git -C "$here" rev-parse --path-format=absolute --git-common-dir)}"
export DX_HOME="$common/sf-cargo/dx-home"
dx="$common/sf-cargo/dx-0.7.10/bin/dx"
bash "$here/tools.sh"
"${CARGO:-cargo}" build --locked -p symphony-server --bins
exec "$target/debug/symphony-ui-dev" "${2:-$root}" "$dx" "${1:-0}"
