#!/usr/bin/env bash
# [--list | --inputs | --alone LANES ARGS... | LANE...]: the release tests step. One nextest build
# and scheduler over the workspace, with Cargo only for doctests and the suites whose process-wide
# fixture must stay shared.
# Named LANEs run alone (`build`, `fixture`, `nextest`, a shared target, `libraries`, `doc`), as a
# release candidate's rerun asks for the lanes whose inputs a fix changed (docs/workflow/RELEASES.md, "The
# rerun"); the one build and the client are made whatever the lanes. Each lane that ran is listed
# in lanes.tsv beside the logs as it is told: its name, exit status and seconds, tab-separated, and
# a run that got through every lane it was asked for says so on its last line.
# `--inputs` says what each lane reads ([`reads`]) and runs nothing.
#
# `--alone LANES ARGS...` reruns one failure of a tests run alone, over that run's own build: LANES
# is the folder its lanes' logs went to (its binaries.json, and `client`, the client it served), and
# ARGS name the failure as a red's rerun line does (`-p PKG --test T -- NAME --exact`, `-p PKG --lib`
# for a target that named no failed test, a lane's own `--workspace --test T`). A test, or a target
# rerun whole, runs by nextest from that build's list, building nothing, under the release profile's
# own timeouts; a doctest or an ignored test (the runner fixture) by Cargo as its lane runs it, over
# the same build and feature graph; a lane's own line, or a shared-fixture target whole, as that
# lane alone, its logs in LANES/alone. It exits as the rerun did: 0 when the failure passed alone.
# The release run reruns its tests' failures so (docs/workflow/RELEASES.md, "The release run", step 5).
# The client's build test runs before scheduling because it writes the build folder. Browser
# tests receive one published client, and tests receive the build's tasks and other binaries.
# Every concurrent batch keeps a live log in SYMPHONY_TEST_LANE_LOGS (else build/test-lanes).
# Native batches keep Cargo's feature graph and build-script debug info, so none relinks while
# another runs, and each binary's copy is a link to its build before any starts, so none removes a
# binary another's test runs ([`link_binaries`]). A failed nextest case is a libtest-json-plus 0.1
# event; a native failure keeps Cargo's output. The release readers understand both in the same
# recorded tests step.
# No retries. Signals reach every batch through the foreground pipeline, whose processes are
# reaped before this script exits. Bash 3.2 (macOS's /bin/bash) runs it.
set -uo pipefail

root="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$root" || exit 2
list=''
inputs=''
only=()
usage() {
    printf 'usage: tools/test-lanes.sh [--list | --inputs | --alone LANES ARGS... | LANE...]\n' >&2
    exit 2
}
# The tests run a failure is rerun alone over (--alone), and the words that name the failure.
alone=''
alone_args=()
if [ "${1:-}" = --alone ]; then
    if [ "$#" -lt 3 ] || [ -z "$2" ]; then usage; fi
    alone="$2"
    shift 2
    alone_args=("$@")
    set --
fi
case "$#:${1:-}" in
    0:) ;;
    1:--list) list=1 ;;
    1:--inputs) inputs=1 ;;
    *)
        for arg in "$@"; do
            case "$arg" in -*) usage ;; esac
        done
        only=("$@")
        ;;
esac

# The process-scoped servers/templates (including transitive imports of store/common.rs),
# perf's generated board, and the browser's build gzip cache. Only these integration targets
# stay in Cargo; every other target, new targets included, belongs to nextest.
shared_tests=(perf sweep cli server store launch release after_commit after_publication
    after_worktrees hooks picks project after_http browser)
shared_libraries=(symphony-board symphony-app symphony-server)
live_fixture=release::nextest::tests::the_pinned_runner_reports_a_fixture_failure_and_a_terminated_test
filter='not (kind(test) & binary(=build))'
filter="$filter & not (kind(lib) & package(=symphony-build) & test(=$live_fixture))"
for target in "${shared_tests[@]}"; do
    filter="$filter & not (kind(test) & binary(=$target))"
done
for package in "${shared_libraries[@]}"; do
    filter="$filter & not (kind(lib) & package(=$package))"
done

# Every lane by name, in the order it runs.
every=(build fixture nextest "${shared_tests[@]}" libraries doc)
for lane in "${only[@]-}"; do
    [ -n "$lane" ] || continue
    case " ${every[*]} " in
        *" $lane "*) ;;
        *)
            printf 'test-lanes: no lane %s; the lanes are %s\n' "$lane" "${every[*]}" >&2
            exit 2
            ;;
    esac
done
# The failure --alone names, as a red's rerun line says it: its package (`-p`) or the workspace (a
# lane's own line), its kind (`lib`, `doc`, `test`, `bin`, `bench`, `example`) and target, and the
# test named in it (`-- [--ignored] NAME --exact`), none when the target is rerun whole.
a_package=''
a_workspace=''
a_kind=''
a_target=''
a_name=''
a_ignored=''
if [ -n "$alone" ]; then
    set -- "${alone_args[@]}"
    while [ "$#" -gt 0 ]; do
        case "$1" in
            -p)
                if [ "$#" -lt 2 ] || [ -z "$2" ] || [ -n "$a_package" ]; then usage; fi
                a_package="$2"
                shift
                ;;
            --workspace) a_workspace=1 ;;
            --lib | --doc)
                if [ -n "$a_kind" ]; then usage; fi
                a_kind="${1#--}"
                ;;
            --test | --bin | --bench | --example)
                if [ "$#" -lt 2 ] || [ -z "$2" ] || [ -n "$a_kind" ]; then usage; fi
                a_kind="${1#--}"
                a_target="$2"
                shift
                ;;
            --)
                shift
                if [ "${1:-}" = --ignored ]; then
                    a_ignored=1
                    shift
                fi
                if [ "$#" -ne 2 ] || [ -z "$1" ] || [ "$2" != --exact ]; then usage; fi
                a_name="$1"
                break
                ;;
            *) usage ;;
        esac
        shift
    done
    set --
    # One package or the workspace, and a target of it.
    if [ -z "$a_kind" ]; then usage; fi
    if [ -n "$a_package" ] && [ -n "$a_workspace" ]; then usage; fi
    if [ -z "$a_package" ] && [ -z "$a_workspace" ]; then usage; fi
    # A lane's own line, or a shared-fixture target rerun whole: that lane alone, as its lane runs
    # it (process-wide fixtures shared), over the same build.
    lane=''
    if [ -n "$a_workspace" ]; then
        if [ -n "$a_name" ]; then usage; fi
        case "$a_kind:$a_target" in
            test:build) lane=build ;;
            lib:) lane=libraries ;;
            doc:) lane=doc ;;
            test:*) lane="$a_target" ;;
            *) usage ;;
        esac
    elif [ "$a_kind" = test ] && [ -z "$a_name" ]; then
        case " ${shared_tests[*]} " in *" $a_target "*) lane="$a_target" ;; esac
    fi
    if [ -n "$lane" ]; then
        case " ${every[*]} " in
            *" $lane "*) ;;
            *) usage ;;
        esac
        only=("$lane")
    fi
    if [ ! -f "$alone/binaries.json" ] || [ ! -s "$alone/client" ]; then
        printf 'test-lanes: no tests run to rerun alone over in %s: it holds no binaries.json and client\n' \
            "$alone" >&2
        exit 2
    fi
    SYMPHONY_UI_DIST="$(cat "$alone/client")"
    export SYMPHONY_UI_DIST
    # A lane alone keeps its logs apart from the run's own.
    export SYMPHONY_TEST_LANE_LOGS="$alone/alone"
fi
# NAME: whether the lane NAME runs: every lane, or the ones named.
wanted() {
    [ "${#only[@]}" -eq 0 ] && return 0
    case " ${only[*]} " in *" $1 "*) return 0 ;; esac
    return 1
}

# NAME: what the lane NAME reads, a line each, for the digest a release candidate's rerun carries
# its green by (tools/check-inputs.sh): `test TARGET` and `package NAME` are derived, the folders
# of the packages that target or package builds with, from `cargo metadata`; `path PATH` is a path
# its tests read outside those; `tree` is the whole tree, which is never narrower than the commit.
# Every lane reads the tree outside apps/ and crates/ besides, which that script adds. A lane
# whose tests run another package's binary names that package; one that cannot be bounded says
# `tree`.
reads() {
    case "$1" in
        build) printf 'test build\npackage symphony-server\npath apps/symphony-web\n' ;;
        fixture) printf 'package symphony-build\n' ;;
        # Every other target of the workspace, and every doctest: the workspace.
        nextest | doc) printf 'tree\n' ;;
        # It reads every tracked file.
        hooks) printf 'tree\n' ;;
        # The server's binary over the client built from its sources.
        browser) printf 'test browser\npackage symphony-server\npath apps/symphony-web\n' ;;
        after_http) printf 'test after_http\npath apps/symphony-web\n' ;;
        # Its tests of this script read a log the release readers keep as a fixture.
        cli) printf 'test cli\npath crates/symphony-build/src/release/fixtures\n' ;;
        libraries)
            printf 'package %s\n' "${shared_libraries[@]}"
            printf 'path apps/symphony-web\n'
            ;;
        *) printf 'test %s\n' "$1" ;;
    esac
}
if [ -n "$inputs" ]; then
    for lane in "${every[@]}"; do
        reads "$lane" | while IFS= read -r line; do
            printf '%s\t%s\t%s\n' "$lane" "${line%% *}" "$([ "$line" = tree ] || printf '%s' "${line#* }")"
        done
    done
    exit 0
fi
names=()
picks=()
if wanted nextest; then
    names+=(nextest)
    picks+=("$filter")
fi
run=(test --config 'profile.dev.build-override.debug="line-tables-only"' --workspace --locked
    --no-fail-fast)
if [ -n "$list" ]; then
    printf 'nextest %s\n' "$filter"
    printf 'cargo --test build (before scheduling)\n'
    printf 'cargo --lib -- --ignored --exact %s (before scheduling)\n' "$live_fixture"
    for target in "${shared_tests[@]}"; do printf 'cargo --test %s\n' "$target"; done
    printf 'cargo --lib (exec adapter selects %s)\n' "${shared_libraries[*]}"
    printf 'doc --doc\n'
    exit 0
fi
# No setup, network access or alternate runner from PATH during a test run.
tools/nextest.sh --check || exit "$?"

# What every batch of tests runs with, once the build is made: the build's folder in `debug`.
environment() {
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
}

# A failure rerun alone (--alone) that is no lane's own: the test, or its target whole, from the
# build the tests run listed, nothing built.
if [ -n "$alone" ] && [ "${#only[@]}" -eq 0 ]; then
    environment
    if [ "$a_kind" = doc ] || [ -n "$a_ignored" ]; then
        # As its lane runs it, by Cargo over the one build: nextest runs no doctest, and the
        # runner fixture runs nextest of its own.
        selected=("--$a_kind")
        if [ -n "$a_target" ]; then selected+=("$a_target"); fi
        if [ -n "$a_name" ]; then
            selected+=(--)
            if [ -n "$a_ignored" ]; then selected+=(--ignored); fi
            selected+=("$a_name" --exact)
        fi
        exec tools/cargo.sh "${run[@]}" "${selected[@]}" </dev/null
    fi
    expression="package(=$a_package) & kind($a_kind)"
    if [ -n "$a_target" ]; then expression="$expression & binary(=$a_target)"; fi
    if [ -n "$a_name" ]; then expression="$expression & test(=$a_name)"; fi
    exec tools/nextest.sh run --profile release --binaries-metadata "$alone/binaries.json" \
        --no-fail-fast --retries 0 --no-tests fail --color never --failure-output immediate \
        --success-output never -E "$expression" </dev/null
fi

logs="${SYMPHONY_TEST_LANE_LOGS:-build/test-lanes}"
# No CDPATH: a cd it leads elsewhere prints the folder, which would make two lines of $logs.
if ! mkdir -p "$logs" || ! logs="$(CDPATH='' cd "$logs" && pwd)"; then
    printf "test-lanes: cannot make the folder for the lanes' logs: %s\n" "$logs" >&2
    exit 2
fi
# Said first, before any test runs, so a failure can be rerun alone over this run (--alone).
printf 'test-lanes: logs in %s\n' "$logs"

# The signal that interrupted the run, as the status it exits with.
interrupted=''
trap 'interrupted=130' INT
trap 'interrupted=143' TERM
trap 'interrupted=129' HUP

# NAME CODE SECONDS: the lane NAME, as it ended, listed.
listed() {
    printf '%s\t%s\t%s\n' "$1" "$2" "$3" >>"$logs/lanes.tsv"
}
if ! : >"$logs/lanes.tsv"; then
    printf 'test-lanes: cannot write %s\n' "$logs/lanes.tsv" >&2
    exit 2
fi

tools/nextest.sh list --config 'profile.dev.build-override.debug="line-tables-only"' \
    --workspace --locked --list-type binaries-only --message-format json >"$logs/binaries.json"
built=$?
if [ -n "$interrupted" ]; then exit "$interrupted"; fi
if [ "$built" -ne 0 ]; then exit "$built"; fi

environment

# Run this before any test that executes symphony-build or copies the build folder. It keeps
# Cargo's feature graph and build-override exactly the same as nextest's one build.
first=0
failed=()
if wanted build && jq -e '."rust-binaries"[] | select(."binary-name" == "build" and .kind == "test")' \
    "$logs/binaries.json" >/dev/null; then
    began="$SECONDS"
    tools/cargo.sh "${run[@]}" --test build
    first=$?
    if [ -n "$interrupted" ]; then exit "$interrupted"; fi
    listed build "$first" "$((SECONDS - began))"
    if [ "$first" -ne 0 ]; then failed+=('    `--workspace --test build`'); fi
fi
# This live tool regression is explicitly ignored in ordinary focused Cargo tests. The release
# owns it here, by its exact name, after provisioning; absence is a real failure, never a skip.
# Keep the workspace feature graph, profile and environment of the one build, before any readers.
if wanted fixture && jq -e '."rust-binaries" | to_entries[] | select(.key == "symphony-build" and .value.kind == "lib")' \
    "$logs/binaries.json" >/dev/null; then
    began="$SECONDS"
    tools/cargo.sh "${run[@]}" --lib -- --ignored --exact "$live_fixture"
    fixture_code=$?
    if [ -n "$interrupted" ]; then exit "$interrupted"; fi
    listed fixture "$fixture_code" "$((SECONDS - began))"
    if [ "$fixture_code" -ne 0 ]; then
        if [ "$first" -eq 0 ]; then first="$fixture_code"; fi
        printf 'error: the explicit runner fixture is red (exit %s), to rerun pass `-p symphony-build --lib -- --ignored %s --exact`\n' \
            "$fixture_code" "$live_fixture"
        failed+=('    `-p symphony-build --lib`')
    fi
fi
# Without a verified client, publish one once before any browser or build-folder reader starts.
if [ -z "${SYMPHONY_UI_DIST:-}" ]; then
    bash apps/symphony-web/build.sh build/symphony-ui || exit "$?"
    if [ -n "$interrupted" ]; then exit "$interrupted"; fi
    export SYMPHONY_UI_DIST="$root/build/symphony-ui/public"
fi
# The client the tests are served, for a failure rerun alone over this run (--alone).
printf '%s\n' "$SYMPHONY_UI_DIST" >|"$logs/client"

# Cargo prepares the environment and cwd for library tests. Its Rust exec adapter selects only
# the three shared-fixture libraries from authoritative artifact paths; nextest runs the rest.
shared_json="$(printf '%s\n' "${shared_libraries[@]}" | jq -Rsc 'split("\n")[:-1]')"
if ! jq --argjson shared "$shared_json" '."rust-binaries" | to_entries
    | map(select(.value.kind == "lib") | {binary: .value["binary-path"],
        shared: (.key as $id | $shared | index($id) != null)})' \
    "$logs/binaries.json" >"$logs/shared-libraries.json"; then exit 2; fi
for target in "${shared_tests[@]}"; do
    if wanted "$target" && jq -e --arg name "$target" '."rust-binaries"[]
        | select(."binary-name" == $name and .kind == "test")' "$logs/binaries.json" >/dev/null; then
        names+=("$target")
        picks+=("--test $target")
    fi
done
library_runner=''
if wanted libraries && jq -e 'any(.[]; .shared)' "$logs/shared-libraries.json" >/dev/null; then
    host="$(jq -er '."rust-build-meta".platforms.host.platform.triple' "$logs/binaries.json")" || exit 2
    runner="$(jq -cn --arg helper "$debug/symphony-build" --arg map "$logs/shared-libraries.json" \
        '[$helper, "exec-test-binary", $map]')" || exit 2
    library_runner="target.$host.runner=$runner"
    names+=(libraries)
    picks+=(--lib)
fi
if wanted doc; then
    names+=(doc)
    picks+=(--doc)
fi
started=()
if [ "${#names[@]}" -gt 0 ]; then
    for i in "${!names[@]}"; do
        rm -f "$logs/${names[i]}.log" "$logs/${names[i]}.status" "$logs/${names[i]}.seconds"
        started+=("$i")
    done
fi

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

# On macOS Cargo puts each binary of the workspace in the target folder as a copy of its build
# under deps/, and every Cargo call makes that copy again, even one that builds nothing: it removes
# the copy, then copies. Elsewhere it hard-links the binary to its build (copying only where it
# cannot link), which a later call finds is its build already and leaves in place. One batch's
# call that copies while another batch's test runs that binary (CARGO_BIN_EXE_NAME,
# SYMPHONY_TEST_BINARIES, the libraries' runner) leaves the test finding no file there. So each
# copy the build lists is made a hard link to the build it copies, renamed over it, and every later
# call finds the copy is its build already and leaves it in place. A copy that is no build's is
# left as Cargo made it, and said.
link_binaries() {
    local path copy name build
    jq -r '."rust-build-meta"."non-test-binaries" // {} | .[][] | .path' "$logs/binaries.json" |
        while IFS= read -r path; do
            copy="${debug%/debug}/$path"
            if [ -L "$copy" ] || [ ! -f "$copy" ]; then continue; fi
            name="${copy##*/}"
            for build in "$debug/deps/${name//-/_}-"*; do
                case "${build##*/}" in *.*) continue ;; esac
                if [ -L "$build" ] || [ ! -f "$build" ]; then continue; fi
                if [ "$build" -ef "$copy" ]; then continue 2; fi
                if cmp -s "$build" "$copy"; then
                    if ln -f "$build" "$copy.test-lanes" && mv -f "$copy.test-lanes" "$copy"; then
                        continue 2
                    fi
                    rm -f "$copy.test-lanes"
                    break
                fi
            done
            printf 'test-lanes: %s is no link to its build under deps/: a batch starting while a test runs it may leave it gone\n' \
                "$copy" >&2
        done
}

# I: the lane I, its output in its log, its exit status in NAME.status, which a signal that ends
# it leaves unwritten, and the seconds it took in NAME.seconds.
lane() {
    local began="$SECONDS" code
    # shellcheck disable=SC2086 # the lane's targets, each a word
    if [ "${names[$1]}" = nextest ]; then
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
    code=$?
    printf '%s\n' "$((SECONDS - began))" >|"$logs/${names[$1]}.seconds"
    printf '%s\n' "$code" >|"$logs/${names[$1]}.status"
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
    link_binaries
    # An interrupt since the client was published, the linking's among it, starts no lane.
    if [ -n "$interrupted" ]; then exit "$interrupted"; fi
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
    took=0
    if [ -f "$logs/$name.seconds" ]; then took="$(cat "$logs/$name.seconds")"; fi
    case "$took" in '' | *[!0-9]*) took=0 ;; esac
    rm -f "$logs/$name.status" "$logs/$name.seconds"
    if [ "$code" = 0 ]; then
        listed "$name" 0 "$took"
        continue
    fi
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
    listed "$name" "$code" "$took"
    if [ "$first" -eq 0 ]; then first="$code"; fi
    if [ "$name" = nextest ]; then
        printf 'test-lanes: nextest ended red (exit %s); case events name test failures, other errors remain in its log\n' "$code"
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
# Said last, and only here: a run that ended before (its build, its client) never says it, so
# whoever reads the log knows its red is its lanes' alone (crates/symphony-build/src/release/alone.rs).
printf 'test-lanes: ended: every lane asked for ran\n'
exit "$first"
