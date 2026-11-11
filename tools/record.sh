#!/usr/bin/env bash
# PHASE LABEL COMMAND [ARGS...]: COMMAND through the board's recorder (`tasks run`), so the board's
# Build or Test column and its machine panel show it while it runs and its work folder keeps how it
# ended (docs/MACHINE_WORK.md); its arguments, streams, status and limits are its own either way.
# The recorder is TASKS_RECORDER's, else this checkout's prepared .target/bin/tasks, else that of
# the main checkout this worktree belongs to; never a containing project's. Run inside a recorded
# command, with TASKS_RECORDER=off, or with no recorder that answers `run --probe` with `run 1`
# within two seconds (none built yet, one older than the command, or one hung or slow), COMMAND is
# exec'd as it is. Finding the recorder starts no process: only the probe and COMMAND run.
#
# Run, or sourced (tools/cargo.sh) for its `record` function, which takes the same arguments and
# never returns: one shell fewer before each build.
set -euo pipefail

record() {
    if [ "$#" -lt 3 ] || { [ "$1" != build ] && [ "$1" != test ]; }; then
        printf 'usage: tools/record.sh build|test LABEL COMMAND [ARGS...]\n' >&2
        exit 2
    fi
    local phase="$1" label="$2" root recorder gitdir='' common='' answer=''
    shift 2
    # The outermost recorded command is the one operation: its steps are not counted again.
    if [ -n "${TASKS_RUN_PARENT:-}" ] || [ "${TASKS_RECORDER:-}" = off ]; then
        exec "$@"
    fi
    case "${BASH_SOURCE[0]}" in
        */*) root="${BASH_SOURCE[0]%/*}/.." ;;
        *) root=.. ;;
    esac
    recorder="${TASKS_RECORDER:-$root/.target/bin/tasks}"
    if [ -z "${TASKS_RECORDER:-}" ] && [ ! -x "$recorder" ] && [ -f "$root/.git" ]; then
        # A worktree of this repository records with its main checkout's prepared recorder: the
        # checkout whose .git is the worktree's common dir.
        IFS= read -r gitdir <"$root/.git" || true
        gitdir="${gitdir#gitdir: }"
        case "$gitdir" in /*) ;; *) gitdir="$root/$gitdir" ;; esac
        if [ -f "$gitdir/commondir" ]; then
            IFS= read -r common <"$gitdir/commondir" || true
            case "$common" in /*) ;; *) common="$gitdir/$common" ;; esac
        fi
        if [ -n "$common" ] && [ "$common/../.git" -ef "$common" ] \
            && [ -f "$common/../tools/record.sh" ]; then
            recorder="$common/../.target/bin/tasks"
        fi
    fi
    if [ -x "$recorder" ]; then
        IFS= read -r -t 2 answer < <(exec "$recorder" run --probe </dev/null 2>/dev/null) || true
    fi
    if [ "$answer" = "run 1" ]; then
        exec "$recorder" run --phase "$phase" --label "$label" -- "$@"
    fi
    exec "$@"
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    record "$@"
fi
