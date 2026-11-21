#!/usr/bin/env bash
# Ordinary Cargo in this workspace; no containing-project planner, pool or source cache. A build or
# a test goes through tools/record.sh (sourced: one shell fewer), which records it for the board when
# a recorder answers and otherwise execs it as it is; anything else is Cargo exec'd directly. Run by
# itself, not inside a recipe's run, a build is a run of the pipeline build (a clippy one of lint)
# and a test one of test (docs/workflow/RUNS.md).
set -euo pipefail
root="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
if [ ! -f "$root/Cargo.toml" ]; then
    printf 'symphony cargo: missing workspace manifest %s/Cargo.toml\n' "$root" >&2
    exit 2
fi
cd "$root"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/.target}"
# A linked worktree's new build folder starts as clones of the main checkout's (tools/seed-target.sh),
# which never fails the build; a checkout without it (a launcher fixture) builds as before.
if [ -x "$root/tools/seed-target.sh" ]; then
    "$root/tools/seed-target.sh" "$CARGO_TARGET_DIR" || true
fi
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
export RUST_TEST_THREADS="${RUST_TEST_THREADS:-6}"
cargo="${CARGO:-cargo}"
phase=
pipeline=
case "${1:-}" in
    test|nextest|bench) phase="test" pipeline="test" ;;
    clippy) phase="build" pipeline="lint" ;;
    build|check|doc|rustc) phase="build" pipeline="build" ;;
esac
if [ -n "$phase" ] && [ -f "$root/tools/record.sh" ]; then
    # shellcheck source=tools/record.sh
    . "$root/tools/record.sh"
    record --pipeline "$pipeline" "$phase" "cargo $*" "$cargo" "$@"
fi
exec "$cargo" "$@"
