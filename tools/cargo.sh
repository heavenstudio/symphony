#!/usr/bin/env bash
# Ordinary Cargo in this workspace; no containing-project planner, pool or source cache. A build or
# a test goes through tools/record.sh, which records it for the board when a recorder answers and
# otherwise execs it as it is; anything else is Cargo exec'd directly.
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
cargo="${CARGO:-cargo}"
phase=
case "${1:-}" in
    test|nextest|bench) phase="test" ;;
    build|check|clippy|doc|rustc) phase="build" ;;
    # The client's dev server runs until stopped: no build to record.
    symphony-ui) if [ "${2:-}" != dev ]; then phase="build"; fi ;;
esac
if [ -n "$phase" ] && [ -x "$root/tools/record.sh" ]; then
    exec "$root/tools/record.sh" "$phase" "cargo $*" "$cargo" "$@"
fi
exec "$cargo" "$@"
