#!/usr/bin/env bash
# Ordinary Cargo in this workspace; no containing-project planner, pool or source cache.
set -euo pipefail
root="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
if [ ! -f "$root/Cargo.toml" ]; then
    printf 'symphony cargo: missing workspace manifest %s/Cargo.toml\n' "$root" >&2
    exit 2
fi
cd "$root"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/.target}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
export RUST_TEST_THREADS="${RUST_TEST_THREADS:-6}"
exec "${CARGO:-cargo}" "$@"
