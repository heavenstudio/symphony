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
# A lane's output goes to a file of its own. Once every lane has ended they follow each other whole
# on stdout in LANES' order, each target's output in one piece as a red's reader reads it, a target
# at a time (crates/symphony/src/release/rerun.rs), then every red lane's own list of the targets
# that failed (the last it printed) as one list in cargo's words, `error: N targets failed:`: the
# last list that reader keeps (crates/symphony/src/release/failures.rs). It exits as the build did
# when that failed, running no lane; else as the first red lane did, in LANES' order; else 0. An
# interrupt (Ctrl-C) reaches the lanes too: it waits for them to end, prints what they wrote and
# exits 130.
#
# The lanes share no port, store or build folder they write: every server, store and browser a test
# starts takes a port the system picks and a folder of its own, and no test writes the build folder
# but wasm_build, whose `tools/rs-bin.sh --build symphony-build` links that binary there again. The
# lane `rest` holds wasm_build with standalone, which copies the build folder whole, and with every
# target of symphony-app, whose tests run that binary and whose binaries a lane's cargo links again
# as it starts: no other lane starts one. The binaries the tests would otherwise build for
# themselves, each under its case's clock, are the build's own: SYMPHONY_TEST_TASKS_BINARY names its
# `tasks`, SYMPHONY_TEST_BINARIES the folder of its binaries, unless the caller names them.
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

interrupted=''
trap 'interrupted=1' INT

tools/cargo.sh "${run[@]}" --no-run
built=$?
if [ "$built" -ne 0 ]; then exit "$built"; fi
if [ -n "$interrupted" ]; then exit 130; fi

# The build's binaries, for the tests that would build their own.
debug="${CARGO_TARGET_DIR:-$root/.target}"
case "$debug" in /*) ;; *) debug="$root/$debug" ;; esac
debug="$debug/debug"
export SYMPHONY_TEST_BINARIES="${SYMPHONY_TEST_BINARIES:-$debug}"
if [ -z "${SYMPHONY_TEST_TASKS_BINARY:-}" ] && [ -f "$debug/tasks" ]; then
    export SYMPHONY_TEST_TASKS_BINARY="$debug/tasks"
fi

outputs="$(mktemp -d "${TMPDIR:-/tmp}/symphony-test-lanes.XXXXXX")" || exit 2

# I: the lane I, its output in the file I.log of its own and its exit status in I.status, which an
# interrupt that ends it leaves unwritten.
lane() {
    # shellcheck disable=SC2086 # the lane's targets, each a word
    tools/cargo.sh "${run[@]}" ${picks[$1]} </dev/null >"$outputs/$1.log" 2>&1
    printf '%s\n' "$?" >"$outputs/$1.status"
}

# I...: those lanes side by side as one pipeline, which ends once every lane has. Never as jobs of
# this shell (`&`): a script's job starts with Ctrl-C ignored, and so would every process it runs,
# a test's children among them, as an ignored signal outlives exec and bash 3.2 cannot undo it, so
# a test interrupting a child of its own would fail here alone. A pipeline's commands run in this
# shell's process group, as any command it runs, and an interrupt reaches each of them.
side_by_side() {
    local first="$1"
    shift
    if [ "$#" -eq 0 ]; then
        lane "$first"
    else
        lane "$first" | side_by_side "$@"
    fi
}

started=()
for i in "${!names[@]}"; do
    if [ -n "${picks[i]}" ]; then started+=("$i"); fi
done
if [ "${#started[@]}" -gt 0 ]; then side_by_side "${started[@]}"; fi

first=0
failed=()
for i in "${started[@]-}"; do
    if [ -z "$i" ]; then continue; fi
    if [ -f "$outputs/$i.log" ]; then cat "$outputs/$i.log"; fi
    if [ -f "$outputs/$i.status" ]; then
        code="$(cat "$outputs/$i.status")"
    else
        code=1
        if [ -z "$interrupted" ]; then
            printf 'test-lanes: the lane %s ended before it could say how\n' "${names[i]}"
        fi
    fi
    if [ "$code" -ne 0 ] && [ "$first" -eq 0 ]; then first="$code"; fi
    # The lane's own list: the lines of the last `error: N target(s) failed:` it printed.
    while IFS= read -r line; do
        failed+=("$line")
    done < <(awk '/^error: [0-9]+ targets? failed:$/ { n = 0; listing = 1; next }
        listing && /^[ \t]+`/ { kept[++n] = $0; next }
        { listing = 0 }
        END { for (i = 1; i <= n; i++) print kept[i] }' "$outputs/$i.log" 2>/dev/null)
    rm -f "$outputs/$i.log" "$outputs/$i.status"
done
rmdir "$outputs"
if [ "${#failed[@]}" -eq 1 ]; then
    printf 'error: 1 target failed:\n%s\n' "${failed[0]}"
elif [ "${#failed[@]}" -gt 1 ]; then
    printf 'error: %s targets failed:\n' "${#failed[@]}"
    printf '%s\n' "${failed[@]}"
fi
if [ -n "$interrupted" ]; then exit 130; fi
exit "$first"
