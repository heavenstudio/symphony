#!/usr/bin/env bash
# PHASE LABEL COMMAND [ARGS...]: COMMAND through the board's recorder (`tasks run`), so the board's
# Build or Test column and its machine panel show it while it runs and its work folder keeps how it
# ended (docs/MACHINE_WORK.md); its arguments, streams, status and limits are its own either way.
# The recorder is TASKS_RECORDER's, else this checkout's prepared .target/bin/tasks, else that of
# the main checkout this worktree belongs to; never a containing project's. Run inside a recorded
# command, with TASKS_RECORDER=off, or with no recorder that answers `run --probe` (none built
# yet, or one older than the command), COMMAND is exec'd as it is.
set -euo pipefail
if [ "$#" -lt 3 ] || { [ "$1" != build ] && [ "$1" != test ]; }; then
    printf 'usage: tools/record.sh build|test LABEL COMMAND [ARGS...]\n' >&2
    exit 2
fi
phase="$1"
label="$2"
shift 2
# The outermost recorded command is the one operation: its steps are not counted again.
if [ -n "${TASKS_RUN_PARENT:-}" ] || [ "${TASKS_RECORDER:-}" = off ]; then
    exec "$@"
fi
root="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
recorder="${TASKS_RECORDER:-$root/.target/bin/tasks}"
if [ -z "${TASKS_RECORDER:-}" ] && [ ! -x "$recorder" ]; then
    # A worktree of this repository records with its main checkout's prepared recorder.
    top="$(git -C "$root" rev-parse --show-toplevel 2>/dev/null || true)"
    common="$(git -C "$root" rev-parse --path-format=absolute --git-common-dir 2>/dev/null || true)"
    if [ -n "$top" ] && [ "$(cd -P "$top" && pwd -P)" = "$root" ] && [ "${common##*/}" = .git ] \
        && [ -f "${common%/.git}/tools/record.sh" ]; then
        recorder="${common%/.git}/.target/bin/tasks"
    fi
fi
if [ -x "$recorder" ] && "$recorder" run --probe </dev/null >/dev/null 2>&1; then
    exec "$recorder" run --phase "$phase" --label "$label" -- "$@"
fi
exec "$@"
