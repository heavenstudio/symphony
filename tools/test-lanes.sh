#!/usr/bin/env bash
# [--list]: every test of the workspace, as the tests step of `just verify` runs them (tools/verify.sh;
# docs/CHECKS.md, "The tests side by side"): built once, then run in lanes side by side.
#
# The build is `cargo test --workspace --locked --no-fail-fast --no-run`, what `cargo test
# --workspace` builds, through tools/cargo.sh as every cargo here. Each lane is then one `cargo test
# --workspace --locked --no-fail-fast` over the targets it names (LANES below), never a package of
# its own, so its targets keep the build's features. The build and every lane give build scripts,
# proc macros and their dependencies the debug info of the rest of the build (`--config
# profile.dev.build-override.debug="line-tables-only"`): without it a dependency that a lane's
# targets use only through those is another unit than the build's, which shares it with a normal
# dependency, and the lane would build its crates again and link its binaries anew while other
# lanes run them. With it every lane's units are the build's, and no lane builds anything.
#
# At most half the processors online run lanes at once (`getconf _NPROCESSORS_ONLN`, at least one;
# one when the system says no number): each of that many slots takes the next lane no slot has
# taken, in LANES' order, until none is left.
#
# Each lane writes its output as it runs to a log of its own, NAME.log in the folder
# SYMPHONY_TEST_LANE_LOGS names (verify names one beside the step's log; a relative one is the
# workspace's), else build/test-lanes, where a run first removes the logs of an earlier one. Before
# any lane starts it says where each lane's log is, so a run stopped before its end (the release
# run's stop ends with SIGKILL) leaves every lane's output named; the red's reader takes the lanes
# and how many run at once from those lines, to say files of the build found gone went while those
# lanes ran, not while the target before in the log's order did. Once every lane has ended their
# logs follow each other whole on stdout in LANES' order, each target's output in one piece as a
# red's reader reads it, a target at a time (crates/symphony-build/src/release/rerun.rs), then
# every red lane's targets that failed as one list in cargo's words, `error: N targets failed:`:
# the last list that reader keeps (crates/symphony-build/src/release/failures.rs).
#
# A lane is green only when it said the exit status 0. A red lane's targets that failed are cargo's
# list ending its log when cargo ended as it does on failed targets (101); any other red lane
# (killed by a signal, cargo failing before any target, a lane that said no status or one that is
# no number) is named by a line after its output in cargo's words, `error: the lane NAME is red
# (…), to rerun pass `--workspace TARGETS``, and its targets are one entry of that list. It exits as
# the build did when that failed, running no lane; else as the first red lane did, in LANES' order
# (1 for a status it cannot read); else 0. An interrupt (Ctrl-C), SIGTERM or SIGHUP reaches the lanes
# too: it waits for them to end, prints what they wrote and exits 128 and the signal's number.
#
# The lanes share no port or store: every server, store and browser a test starts takes a port the
# system picks and a folder of its own. The one test writing the build folder, wasm_build, whose
# `tools/rs-bin.sh --build symphony-build` links that binary there again, is in `rest` with
# standalone, which copies the build folder whole, and with every target of symphony-build and
# symphony-app, whose tests run that binary and whose binaries a lane's cargo links again as it
# starts: no other lane starts one. The browser's lane writes the build folder too when
# SYMPHONY_UI_DIST names no client: its own client build (`cargo symphony-ui dist`,
# apps/symphony-ui/bundle.sh) runs `tools/rs-bin.sh --build symphony-build`, which can link that
# binary there again while `rest` runs it. Verify names the client its client check built, so only
# a verify whose client check built none leaves it to the browser's lane. The binaries the tests would otherwise build for themselves, each under
# its case's clock, are the build's own: SYMPHONY_TEST_TASKS_BINARY names its `tasks`,
# SYMPHONY_TEST_BINARIES the folder of its binaries, unless the caller names them.
#
# `--list` prints each lane that would run, its name and then its targets as cargo takes them, and
# runs nothing but `cargo metadata`. Bash 3.2 (macOS's /bin/bash) runs it.
set -uo pipefail

root="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$root" || exit 2
list=''
case "$#:${1:-}" in
    0:) ;;
    1:--list) list=1 ;;
    *)
        printf 'usage: tools/test-lanes.sh [--list]\n' >&2
        exit 2
        ;;
esac

# LANES: each lane's name, then the test targets it runs, by the names cargo's `--test` takes. A name
# no package has is left out, so a target renamed runs in `rest`, which runs every test target no
# other lane names; `units` runs the libraries' and binaries' own tests and each example and bench
# cargo tests, `doc` the doc tests. A lane left with nothing to run is not started.
lanes=(
    'perf perf'
    'sweep sweep'
    'browser browser'
    'board cli server store launch launcher child_guard'
    'server after_access after_host_origin after_http after_lifecycle after_reload'
    'units'
    'rest'
    'doc'
)
run=(test --config 'profile.dev.build-override.debug="line-tables-only"' --workspace --locked
    --no-fail-fast)

# The workspace's targets that `cargo test` runs besides its libraries and binaries: KIND NAME a
# line, KIND test, example or bench, each with `test = true` (an integration test's default).
if ! targets="$(tools/cargo.sh metadata --no-deps --format-version 1 --locked | jq -r '
        .packages[].targets[]
        | select(.test and (.kind == ["test"] or .kind == ["example"] or .kind == ["bench"]))
        | "\(.kind[0]) \(.name)"' | sort -u)"; then
    printf "test-lanes: cannot list the workspace's test targets (cargo metadata, jq)\n" >&2
    exit 2
fi

# Each lane's name and its targets as cargo takes them, in LANES' order; `rest`'s filled last.
names=()
picks=()
claimed=' '
rest=-1
for lane in "${lanes[@]}"; do
    # shellcheck disable=SC2086 # a lane's words
    set -- $lane
    name="$1"
    shift
    pick=''
    case "$name" in
        units)
            pick='--lib --bins'
            while read -r kind target; do
                case "$kind" in example | bench) pick="$pick --$kind $target" ;; esac
            done <<<"$targets"
            ;;
        doc) pick='--doc' ;;
        rest) rest="${#names[@]}" ;;
        *)
            for target in "$@"; do
                if printf '%s\n' "$targets" | grep -qxF "test $target"; then
                    pick="$pick --test $target"
                    claimed="$claimed$target "
                fi
            done
            ;;
    esac
    names+=("$name")
    picks+=("${pick# }")
done
if [ "$rest" -ge 0 ]; then
    pick=''
    while read -r kind target; do
        case "$claimed" in *" $target "*) continue ;; esac
        if [ "$kind" = test ]; then pick="$pick --test $target"; fi
    done <<<"$targets"
    picks[rest]="${pick# }"
fi

if [ -n "$list" ]; then
    for i in "${!names[@]}"; do
        if [ -n "${picks[i]}" ]; then printf '%s %s\n' "${names[i]}" "${picks[i]}"; fi
    done
    exit 0
fi

logs="${SYMPHONY_TEST_LANE_LOGS:-build/test-lanes}"
# No CDPATH: a cd it leads elsewhere prints the folder, which would make two lines of $logs.
if ! mkdir -p "$logs" || ! logs="$(CDPATH='' cd "$logs" && pwd)"; then
    printf "test-lanes: cannot make the folder for the lanes' logs: %s\n" "$logs" >&2
    exit 2
fi

# The signal that interrupted the run, as the status it exits with.
interrupted=''
trap 'interrupted=130' INT
trap 'interrupted=143' TERM
trap 'interrupted=129' HUP

tools/cargo.sh "${run[@]}" --no-run
built=$?
if [ -n "$interrupted" ]; then exit "$interrupted"; fi
if [ "$built" -ne 0 ]; then exit "$built"; fi

# The build's binaries, for the tests that would build their own.
debug="${CARGO_TARGET_DIR:-$root/.target}"
case "$debug" in /*) ;; *) debug="$root/$debug" ;; esac
debug="$debug/debug"
export SYMPHONY_TEST_BINARIES="${SYMPHONY_TEST_BINARIES:-$debug}"
if [ -z "${SYMPHONY_TEST_TASKS_BINARY:-}" ] && [ -f "$debug/tasks" ]; then
    export SYMPHONY_TEST_TASKS_BINARY="$debug/tasks"
fi

started=()
for i in "${!names[@]}"; do
    rm -f "$logs/${names[i]}.log" "$logs/${names[i]}.status"
    if [ -n "${picks[i]}" ]; then started+=("$i"); fi
done

processors="$(getconf _NPROCESSORS_ONLN 2>/dev/null)"
case "$processors" in
    '' | *[!0-9]*)
        at_once=1
        why='no number of processors online'
        ;;
    *)
        at_once=$((10#$processors / 2))
        why="$processors processors online"
        ;;
esac
if [ "$at_once" -lt 1 ]; then at_once=1; fi

# I: the lane I, its output in its log and its exit status in NAME.status, which a signal that ends
# it leaves unwritten.
lane() {
    # shellcheck disable=SC2086 # the lane's targets, each a word
    tools/cargo.sh "${run[@]}" ${picks[$1]} </dev/null >>"$logs/${names[$1]}.log" 2>&1
    printf '%s\n' "$?" >|"$logs/${names[$1]}.status"
}

# A slot: each lane no slot has taken, one after another in LANES' order, each in a shell of its
# own, so a lane that ends its shell ends no other. A slot takes a lane by making its log, which no
# other slot then can (noclobber).
slot() (
    set -C
    for i in "${started[@]}"; do
        if { true >"$logs/${names[i]}.log"; } 2>/dev/null; then (lane "$i"); fi
    done
)

# N: N slots side by side as one pipeline, which ends once every slot has. Never as jobs of this
# shell (`&`): a script's job starts with Ctrl-C ignored, and so would every process it runs, a
# test's children among them, as an ignored signal outlives exec and bash 3.2 cannot undo it, so a
# test interrupting a child of its own would fail here alone. A pipeline's commands run in this
# shell's process group, as any command it runs, and a signal sent to the group reaches each.
slots() {
    if [ "$1" -le 1 ]; then
        slot
    else
        slot | slots "$(($1 - 1))"
    fi
}

if [ "${#started[@]}" -gt 0 ]; then
    printf 'test-lanes: lanes run at most %s at once (%s), each writing its log as it runs:\n' \
        "$at_once" "$why"
    for i in "${started[@]}"; do
        printf 'test-lanes:     %s (%s)\n' "$logs/${names[i]}.log" "${picks[i]}"
    done
    if [ "$at_once" -gt "${#started[@]}" ]; then at_once="${#started[@]}"; fi
    slots "$at_once"
fi

first=0
failed=()
for i in "${started[@]-}"; do
    if [ -z "$i" ]; then continue; fi
    name="${names[i]}"
    log="$logs/$name.log"
    if [ -f "$log" ]; then cat "$log"; fi
    # A lane ended mid-line (a test killed as it ran) has its line ended, so the next starts a line.
    if [ -s "$log" ] && [ -n "$(tail -c 1 "$log")" ]; then printf '\n'; fi
    code=''
    told=''
    if [ -f "$logs/$name.status" ]; then
        code="$(cat "$logs/$name.status")"
        told=1
    fi
    rm -f "$logs/$name.status"
    if [ "$code" = 0 ]; then continue; fi
    # A lane the interrupt cut short, or never let start, failed in nothing of its own.
    if [ -z "$told" ] && [ -n "$interrupted" ]; then continue; fi
    case "$code" in
        [1-9] | [1-9][0-9] | 1[0-9][0-9] | 2[0-4][0-9] | 25[0-5]) said="exit status $code" ;;
        *)
            if [ -n "$told" ]; then
                printf "test-lanes: the lane %s said its exit status as '%s', which is none\n" \
                    "$name" "$code"
            elif [ -f "$log" ]; then
                printf 'test-lanes: the lane %s ended before it could say how\n' "$name"
            else
                printf 'test-lanes: the lane %s never started\n' "$name"
            fi
            said='no exit status it could read'
            code=1
            ;;
    esac
    if [ "$first" -eq 0 ]; then first="$code"; fi
    # The lane's own list: the lines of the `error: N target(s) failed:` that ends its log, which
    # cargo prints last when it ends on failed targets (101), never a list a test printed.
    own=()
    if [ "$code" = 101 ]; then
        while IFS= read -r line; do
            own+=("$line")
        done < <(awk '/^error: [0-9]+ targets? failed:$/ { n = 0; listing = 1; next }
            listing && /^[ \t]+`/ { kept[++n] = $0; next }
            /^[ \t\r]*$/ { listing = 0; next }
            { listing = 0; n = 0 }
            END { for (i = 1; i <= n; i++) print kept[i] }' "$log" 2>/dev/null)
    fi
    if [ "${#own[@]}" -gt 0 ]; then
        failed+=("${own[@]}")
    else
        # shellcheck disable=SC2016 # cargo's backquotes, as they are
        printf 'error: the lane %s is red (%s) and listed no target that failed, to rerun pass `%s`\n' \
            "$name" "$said" "--workspace ${picks[i]}"
        failed+=("    \`--workspace ${picks[i]}\`")
    fi
done
if [ "${#failed[@]}" -eq 1 ]; then
    printf 'error: 1 target failed:\n%s\n' "${failed[0]}"
elif [ "${#failed[@]}" -gt 1 ]; then
    printf 'error: %s targets failed:\n' "${#failed[@]}"
    printf '%s\n' "${failed[@]}"
fi
if [ -n "$interrupted" ]; then
    printf 'test-lanes: interrupted (exit %s): a lane it cut short ended there, its log as far as it got\n' \
        "$interrupted"
    exit "$interrupted"
fi
exit "$first"
