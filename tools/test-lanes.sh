#!/usr/bin/env bash
# [--list]: the release tests step. One nextest build and scheduler over the workspace, with
# Cargo only for doctests and the suites whose process-wide fixture must stay shared.
# The client's build test runs before scheduling because it writes the build folder. Browser
# tests receive one published client, and tests receive the build's tasks and other binaries.
# Every concurrent batch keeps a live log in SYMPHONY_TEST_LANE_LOGS (else build/test-lanes).
# Native batches keep Cargo's feature graph and build-script debug info, so none relinks while
# another runs. A failed nextest case is a libtest-json-plus 0.1 event; a native failure keeps
# Cargo's output. The release readers understand both in the same recorded tests step.
# No retries. Signals reach every batch through the foreground pipeline, whose processes are
# reaped before this script exits. Bash 3.2 (macOS's /bin/bash) runs it.
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

# The process-scoped servers/templates (including transitive imports of store/common.rs),
# perf's generated board, and the browser's build gzip cache. Only these integration targets
# stay in Cargo; every other target, new targets included, belongs to nextest.
shared_tests=(perf sweep cli server store launch release after_commit after_publication
    after_worktrees hooks picks project after_http browser)
shared_libraries=(symphony-board symphony-app symphony-server)
filter='not (kind(test) & binary(=build))'
for target in "${shared_tests[@]}"; do
    filter="$filter & not (kind(test) & binary(=$target))"
done
for package in "${shared_libraries[@]}"; do
    filter="$filter & not (kind(lib) & package(=$package))"
done
names=(nextest)
picks=("$filter")
run=(test --config 'profile.dev.build-override.debug="line-tables-only"' --workspace --locked
    --no-fail-fast)
if [ -n "$list" ]; then
    printf 'nextest %s\n' "$filter"
    printf 'cargo --test build (before scheduling)\n'
    for target in "${shared_tests[@]}"; do printf 'cargo --test %s\n' "$target"; done
    printf 'cargo --lib (exec adapter selects %s)\n' "${shared_libraries[*]}"
    printf 'doc --doc\n'
    exit 0
fi
# No setup, network access or alternate runner from PATH during a test run.
tools/nextest.sh --check || exit "$?"

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

tools/nextest.sh list --config 'profile.dev.build-override.debug="line-tables-only"' \
    --workspace --locked --list-type binaries-only --message-format json >"$logs/binaries.json"
built=$?
if [ -n "$interrupted" ]; then exit "$interrupted"; fi
if [ "$built" -ne 0 ]; then exit "$built"; fi

# A test names its own terminal stand-in, or sees no terminals of this machine.
export HC_BIN="${HC_BIN:-/usr/bin/false}"

# The build's binaries, for the tests that would build their own.
debug="${CARGO_TARGET_DIR:-$root/.target}"
case "$debug" in /*) ;; *) debug="$root/$debug" ;; esac
debug="$debug/debug"
export SYMPHONY_TEST_BINARIES="${SYMPHONY_TEST_BINARIES:-$debug}"
if [ -z "${SYMPHONY_TEST_TASKS_BINARY:-}" ] && [ -f "$debug/tasks" ]; then
    export SYMPHONY_TEST_TASKS_BINARY="$debug/tasks"
fi

# Run this before any test that executes symphony-build or copies the build folder. It keeps
# Cargo's feature graph and build-override exactly the same as nextest's one build.
first=0
failed=()
if jq -e '."rust-binaries"[] | select(."binary-name" == "build" and .kind == "test")' \
    "$logs/binaries.json" >/dev/null; then
    tools/cargo.sh "${run[@]}" --test build
    first=$?
    if [ -n "$interrupted" ]; then exit "$interrupted"; fi
    if [ "$first" -ne 0 ]; then failed+=('    `--workspace --test build`'); fi
fi
# Without a verified client, publish one once before any browser or build-folder reader starts.
if [ -z "${SYMPHONY_UI_DIST:-}" ]; then
    bash apps/symphony-web/build.sh build/symphony-ui || exit "$?"
    if [ -n "$interrupted" ]; then exit "$interrupted"; fi
    export SYMPHONY_UI_DIST="$root/build/symphony-ui/public"
fi

# Cargo prepares the environment and cwd for library tests. Its Rust exec adapter selects only
# the three shared-fixture libraries from authoritative artifact paths; nextest runs the rest.
shared_json="$(printf '%s\n' "${shared_libraries[@]}" | jq -Rsc 'split("\n")[:-1]')"
if ! jq --argjson shared "$shared_json" '."rust-binaries" | to_entries
    | map(select(.value.kind == "lib") | {binary: .value["binary-path"],
        shared: (.key as $id | $shared | index($id) != null)})' \
    "$logs/binaries.json" >"$logs/shared-libraries.json"; then exit 2; fi
for target in "${shared_tests[@]}"; do
    if jq -e --arg name "$target" '."rust-binaries"[]
        | select(."binary-name" == $name and .kind == "test")' "$logs/binaries.json" >/dev/null; then
        names+=("$target")
        picks+=("--test $target")
    fi
done
library_runner=''
if jq -e 'any(.[]; .shared)' "$logs/shared-libraries.json" >/dev/null; then
    host="$(jq -er '."rust-build-meta".platforms.host.platform.triple' "$logs/binaries.json")" || exit 2
    runner="$(jq -cn --arg helper "$debug/symphony-build" --arg map "$logs/shared-libraries.json" \
        '[$helper, "exec-test-binary", $map]')" || exit 2
    library_runner="target.$host.runner=$runner"
    names+=(libraries)
    picks+=(--lib)
fi
names+=(doc)
picks+=(--doc)
started=()
for i in "${!names[@]}"; do
    rm -f "$logs/${names[i]}.log" "$logs/${names[i]}.status"
    started+=("$i")
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
    if [ "$1" -eq 0 ]; then
        NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 tools/nextest.sh run --profile release \
            --binaries-metadata "$logs/binaries.json" --no-fail-fast --retries 0 \
            --test-threads "${RUST_TEST_THREADS:-6}" --color never \
            --message-format libtest-json-plus --message-format-version 0.1 \
            --failure-output never --success-output never -E "$filter" \
            </dev/null 2>&1 | "$debug/symphony-build" test-stalls "$root/.config/nextest.toml" \
            >>"$logs/${names[$1]}.log" 2>>"$logs/${names[$1]}.log"
    elif [ "${names[$1]}" = libraries ]; then
        tools/cargo.sh "${run[@]}" --config "$library_runner" --lib </dev/null >>"$logs/${names[$1]}.log" 2>&1
    else
        tools/cargo.sh "${run[@]}" ${picks[$1]} </dev/null >>"$logs/${names[$1]}.log" 2>&1
    fi
    printf '%s\n' "$?" >|"$logs/${names[$1]}.status"
}

# A slot: each batch no slot has taken, in the exceptions' listed order, each in a shell of its
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
    if [ "$name" = nextest ]; then
        printf 'test-lanes: nextest ended red (exit %s); its JSON events name each failure\n' "$code"
        continue
    fi
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
