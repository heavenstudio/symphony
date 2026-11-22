#!/usr/bin/env bash
# [--pipeline NAME] [--step NAME] PHASE LABEL COMMAND [ARGS...]: COMMAND through the board's
# recorder (`tasks run`), so the board's Build or Test column and its machine panel show it while it
# runs and its work folder keeps how it ended (docs/workflow/MACHINE_WORK.md); its arguments, streams, status
# and limits are its own either way. At top level, `--pipeline` makes it a run of that pipeline, of
# which the day files keep a line (docs/workflow/RUNS.md). Inside a recorded run, only a command `--step`
# names is recorded again, as a step of that run; any other command recorded inside a recorded
# command is exec'd as it is, as is every one with TASKS_RECORDER=off or with no recorder that
# answers `run --probe` with `run 1` within two seconds (none built yet, one older than the command,
# or one hung or slow; TASKS_RECORDER_PROBE_WAIT sets other seconds, a test's). A recorder that
# does not answer `run 2` too records no run or step: the command is recorded at top level as
# before, and exec'd as it is inside a recorded command.
# The recorder is TASKS_RECORDER's, else this checkout's prepared .target/bin/tasks, else that of
# the main checkout this worktree belongs to; never a containing project's. Finding the recorder
# starts no process: only the probe and COMMAND run.
#
# Run, or sourced (tools/cargo.sh) for its `record` function, which takes the same arguments and
# never returns: one shell fewer before each build.
set -euo pipefail

record() {
    local pipeline='' step='' runs=() line='' first='' second=''
    while [ "$#" -ge 2 ] && { [ "$1" = --pipeline ] || [ "$1" = --step ]; }; do
        if [ "$1" = --pipeline ]; then pipeline="$2"; else step="$2"; fi
        shift 2
    done
    if [ "$#" -lt 3 ] || { [ "$1" != build ] && [ "$1" != test ]; }; then
        printf 'usage: tools/record.sh [--pipeline NAME] [--step NAME] build|test LABEL COMMAND [ARGS...]\n' >&2
        exit 2
    fi
    local phase="$1" label="$2" root recorder gitdir='' common=''
    shift 2
    if [ "${TASKS_RECORDER:-}" = off ]; then
        exec "$@"
    fi
    # Inside a recorded command, only a step of a recorded run is counted again.
    if [ -n "${TASKS_RUN_PARENT:-}" ] && { [ -z "$step" ] || [ -z "${SYMPHONY_RUN_PIPELINE:-}" ]; }; then
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
    # The probe answers each version of `tasks run` it takes, a line each.
    if [ -x "$recorder" ]; then
        while IFS= read -r -t "${TASKS_RECORDER_PROBE_WAIT:-2}" line; do
            case "$line" in
                "run 1") first=1 ;;
                "run 2") second=1 ;;
            esac
        done < <(exec "$recorder" run --probe </dev/null 2>/dev/null) || true
    fi
    if [ -n "$second" ]; then
        if [ -n "$pipeline" ]; then runs+=(--pipeline "$pipeline"); fi
        if [ -n "$step" ]; then runs+=(--step "$step"); fi
        exec "$recorder" run --phase "$phase" --label "$label" ${runs[@]+"${runs[@]}"} -- "$@"
    fi
    if [ -n "$first" ] && [ -z "${TASKS_RUN_PARENT:-}" ]; then
        exec "$recorder" run --phase "$phase" --label "$label" -- "$@"
    fi
    exec "$@"
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    record "$@"
fi
