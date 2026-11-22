#!/usr/bin/env bash
# JUSTFILE [STEP...]: the full verification, `just verify [STEP...]` (docs/workflow/CHECKS.md, "The
# release run"), run inside the verify run that recipe records. Every step runs whatever the ones before
# it did, and it is red when any step is, naming each red step and its log. The steps, in order:
#
#     lint          the lint-rs recipe
#     client-check  the client-check recipe into build/verified-client: the client built fresh and
#                   compared with the committed distribution
#     size-check    the size-check recipe: the shipped binaries' size budgets
#     docs-audit    the docs audit, `symphony docs-audit --check`
#     crates-check  the crates-check recipe: each published package whose version crates.io holds
#                   packaged from the same sources (docs/workflow/RELEASES.md, "Packages on crates.io")
#     semver-check  the kit/helper APIs against crates.io, with their actual version moves
#     hosted        the hosted-check recipe: the board built with its hosted feature, its tests and
#                   its matrix on PostgreSQL 16 and 18, which no other step builds
#     nextest-setup provision the checksum-pinned runner, outside the tests step
#     tests         every test target of the workspace, the browser tests among them, built once and
#                   scheduled by nextest with shared-fixture Cargo batches (tools/test-lanes.sh), logs in tests-lanes/
#                   beside the step's log (SYMPHONY_TEST_LANE_LOGS), named in it as the lanes start
#
# Each is recorded as a step of the verify run (docs/workflow/MACHINE_WORK.md) and writes its output to a
# log of its own, STEP.log in the folder SYMPHONY_CHECK_STEPS names (the release run names one for
# each check it runs), else build/verify, which steps.tsv there lists as each step ends: its name, exit
# status (`-` for the tests when a red nextest-setup kept them from starting), seconds, log and command, and the machine's load as it ended (its load averages over 1, 5
# and 15 minutes and its processors, space-separated; empty where the system does not say),
# tab-separated (crates/symphony-build/src/release/steps.rs reads it). Named STEPs run alone, in the
# order above. An interrupt (Ctrl-C) stops it once the step it reached ends, exiting 130.
#
# The tests step lists each lane that ran after its own line, as `tests/LANE`, a part of the step,
# and `tests/LANE...` in place of `tests` runs those lanes alone (tools/test-lanes.sh), which is
# how a release candidate's rerun runs the lanes whose inputs a fix changed and carries the green
# of the rest (docs/workflow/RELEASES.md, "The rerun"). `--inputs` in place of the steps says what each
# step and lane reads ([`reads`]) and runs nothing.
#
# The tests serve the client the client check built in this same verify (SYMPHONY_UI_DIST). The
# check's folder is cleared before it runs, so a client an earlier check left there is never served.
# With none (the client check built nothing, or did not run), SYMPHONY_UI_DIST is unset and the
# tests runner publishes one client before scheduling, red if that build fails; its log and the
# summary say what the tests ran against.
#
# Bash 3.2 (macOS's /bin/bash) runs it.
set -uo pipefail

justfile="${1:?usage: tools/verify.sh JUSTFILE [STEP...]}"
shift
CDPATH='' cd "$(dirname "$justfile")" || exit 2

# A just a step runs exits 130 on an interrupt rather than dying by it, which leaves bash going on to
# the next step: the interrupt is kept, and the run stops once the step it reached has ended.
interrupted=''
trap 'interrupted=1' INT

steps=(lint client-check size-check docs-audit crates-check semver-check hosted nextest-setup tests)

# STEP: what the step STEP reads, a line each, in the words of tools/test-lanes.sh's `reads`:
# `package NAME` (the folders of the packages it builds with), `path PATH`, or `tree`, the whole
# tree. Every step reads the tree outside apps/ and crates/ besides, the check itself among it
# (this script, the justfile, tools/, the lock file and the toolchain pin), which
# tools/check-inputs.sh adds. The tests step has no inputs of its own: its lanes have.
reads() {
    case "$1" in
        # Lint reads the branch's commits for what they tell a consumer, the docs audit every
        # document, guide and recipe, and the crates check every published package.
        lint | docs-audit | crates-check) printf 'tree\n' ;;
        client-check) printf 'path apps/symphony-web\npath apps/symphony-server\n' ;;
        size-check) printf 'package symphony-server\npackage symphony-app\npackage symphony-board\n' ;;
        semver-check) printf 'path crates\n' ;;
        # The board with its hosted feature, and the settings table it judges.
        hosted) printf 'package symphony-board\n' ;;
        nextest-setup) ;;
    esac
}
if [ "$#:${1:-}" = 1:--inputs ]; then
    for step in "${steps[@]}"; do
        if [ "$step" = tests ]; then
            bash tools/test-lanes.sh --inputs | sed 's|^|tests/|' || exit 2
            continue
        fi
        # A step that reads nothing of its own is named still, by a line with no word.
        printf '%s\t\t\n' "$step"
        reads "$step" | while IFS= read -r line; do
            printf '%s\t%s\t%s\n' "$step" "${line%% *}" "$([ "$line" = tree ] || printf '%s' "${line#* }")"
        done
    done
    exit 0
fi

chosen=()
# The lanes of the tests step asked for alone, as `tests/LANE`.
only_lanes=()
for arg in "$@"; do
    known=''
    case "$arg" in
    tests/?*)
        only_lanes+=("${arg#tests/}")
        continue
        ;;
    esac
    for step in "${steps[@]}"; do
        if [ "$step" = "$arg" ]; then known=1; fi
    done
    if [ -z "$known" ]; then
        printf 'verify: no step %s; the steps are %s\n' "$arg" "${steps[*]}" >&2
        exit 2
    fi
done
for step in "${steps[@]}"; do
    if [ "$#" -eq 0 ]; then
        chosen+=("$step")
        continue
    fi
    for arg in "$@"; do
        case "$arg" in tests/?*) arg=tests ;; esac
        # Even a tests-only release rerun has its explicit setup prerequisite. A setup failure
        # is red at that step; tests never starts and cannot file a misleading missing-tool red.
        if [ "$step" = "$arg" ] || { [ "$step" = nextest-setup ] && [ "$arg" = tests ]; }; then
            chosen+=("$step")
            break
        fi
    done
done

logs="${SYMPHONY_CHECK_STEPS:-build/verify}"
# The folder is this verify's alone: nothing a step runs writes its steps there.
unset SYMPHONY_CHECK_STEPS
# No CDPATH: a cd it leads elsewhere prints the folder, which would make two lines of $logs.
if ! mkdir -p "$logs" || ! logs="$(CDPATH='' cd "$logs" && pwd)"; then
    printf 'verify: cannot make the folder for its steps: %s\n' "$logs" >&2
    exit 2
fi
summary="$logs/steps.tsv"
if ! : >"$summary"; then
    printf 'verify: cannot write %s\n' "$summary" >&2
    exit 2
fi

client=build/verified-client
# The client the client check built in this verify, once it has.
fresh=''
said=()
red=()

# The machine's load averages over 1, 5 and 15 minutes and its processors, space-separated, as
# steps.tsv's last field says them; nothing where the system does not say.
load() {
    local averages processors
    if [ -r /proc/loadavg ]; then
        averages="$(cut -d ' ' -f 1-3 /proc/loadavg)"
    else
        averages="$(sysctl -n vm.loadavg 2>/dev/null | tr -d '{}')"
    fi
    processors="$(getconf _NPROCESSORS_ONLN 2>/dev/null)"
    # shellcheck disable=SC2086 # the averages, split into their words
    set -- $averages
    if [ "$#" -eq 3 ] && [ -n "$processors" ]; then
        printf '%s %s %s %s' "$1" "$2" "$3" "$processors"
    fi
}

# NAME COMMAND ARGV...: the step NAME, said as COMMAND, running ARGV with its output in its log.
run() {
    local name="$1" command="$2" log="$logs/$1.log" start="$SECONDS" code seconds
    shift 2
    printf 'verify: %s: %s (log: %s)\n' "$name" "$command" "$log"
    "$@" >"$log" 2>&1
    code=$?
    seconds=$((SECONDS - start))
    printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$name" "$code" "$seconds" "$log" "$command" "$(load)" >>"$summary"
    if [ "$code" -eq 0 ]; then
        printf 'verify: %s: green (%s s)\n' "$name" "$seconds"
        said+=("$name green ($seconds s)")
    else
        printf 'verify: %s: red, exit %s (%s s); its last lines:\n' "$name" "$code" "$seconds"
        tail -n 20 "$log" | sed 's/^/    /'
        said+=("$name red, exit $code ($seconds s); log $log")
        red+=("$name")
    fi
}

checked=''
reached=''
for step in "${chosen[@]}"; do
    if [ -n "$interrupted" ]; then break; fi
    reached="$step"
    case "$step" in
    lint)
        run lint "just lint-rs" just --justfile "$justfile" lint-rs
        ;;
    client-check)
        checked=1
        rm -rf "$client/public"
        run client-check "just client-check $client" just --justfile "$justfile" client-check "$client"
        if [ -f "$client/public/index.html" ]; then fresh="$PWD/$client/public"; fi
        ;;
    size-check)
        run size-check "just size-check" just --justfile "$justfile" size-check
        ;;
    docs-audit)
        run docs-audit "just symphony-cli docs-audit --check" \
            tools/record.sh --step docs-audit test "docs audit" \
            just --justfile "$justfile" symphony-cli docs-audit --check
        ;;
    crates-check)
        run crates-check "just crates-check" \
            tools/record.sh --step crates-check test "crates check" \
            just --justfile "$justfile" crates-check
        ;;
    semver-check)
        run semver-check "just semver-check" \
            tools/record.sh --step semver-check test "semver check" \
            just --justfile "$justfile" semver-check
        ;;
    hosted)
        run hosted "just hosted-check" \
            tools/record.sh --step hosted test "hosted check" \
            just --justfile "$justfile" hosted-check
        ;;
    nextest-setup)
        run nextest-setup "just nextest-setup" \
            tools/record.sh --step nextest-setup build "nextest setup" \
            just --justfile "$justfile" nextest-setup
        ;;
    tests)
        case " ${red[*]-} " in
        *" nextest-setup "*)
            printf 'verify: tests not started: nextest-setup is red; see %s/nextest-setup.log\n' "$logs"
            # Listed as not run (`-`), never as a red of its own: whoever reads the list knows
            # the step is there and did not run, so no rerun of the setup alone passes for it.
            printf 'tests\t-\t0\t%s\t%s\t%s\n' "$logs/nextest-setup.log" tools/test-lanes.sh "$(load)" >>"$summary"
            continue
            ;;
        esac
        tests=(tools/test-lanes.sh)
        # The whole step when it was asked for by its name, whatever lanes were named beside it.
        whole=''
        for arg in "$@"; do
            if [ "$arg" = tests ]; then whole=1; fi
        done
        if [ -z "$whole" ] && [ "${#only_lanes[@]}" -gt 0 ]; then tests+=("${only_lanes[@]}"); fi
        lanes="$logs/tests-lanes"
        rm -f "$lanes/lanes.tsv"
        if [ -n "$fresh" ]; then
            how="the tests ran against the client the client check built in this verify: $fresh"
            case " ${red[*]-} " in
            *" client-check "*)
                how="the client check is red: the tests ran against the client it built from this tree, $fresh, not the committed one"
                ;;
            esac
            run tests "SYMPHONY_UI_DIST=\"\$PWD/$client/public\" ${tests[*]}" \
                env SYMPHONY_UI_DIST="$fresh" SYMPHONY_TEST_LANE_LOGS="$lanes" \
                tools/record.sh --step tests test tests "${tests[@]}"
        else
            if [ -n "$checked" ]; then
                why='the client check built no client'
            else
                why='the client check did not run in this verify'
            fi
            how="$why, so SYMPHONY_UI_DIST was unset: the tests runner published one client before scheduling (build/symphony-ui), red if that build failed"
            run tests "${tests[*]}" \
                env -u SYMPHONY_UI_DIST SYMPHONY_TEST_LANE_LOGS="$lanes" \
                tools/record.sh --step tests test tests "${tests[@]}"
        fi
        # Each lane that ran, a part of the step: its log the lane's own, else the step's.
        if [ -f "$lanes/lanes.tsv" ]; then
            while IFS="$(printf '\t')" read -r lane code seconds; do
                log="$logs/tests.log"
                if [ -f "$lanes/$lane.log" ]; then log="$lanes/$lane.log"; fi
                printf 'tests/%s\t%s\t%s\t%s\t%s\t%s\n' "$lane" "$code" "$seconds" "$log" \
                    "tools/test-lanes.sh $lane" "$(load)" >>"$summary"
            done <"$lanes/lanes.tsv"
        fi
        printf 'verify: tests: %s\n' "$how"
        printf '\nverify: %s\n' "$how" >>"$logs/tests.log"
        last=$((${#said[@]} - 1))
        said[$last]="${said[$last]}; $how"
        ;;
    esac
done

printf '\n'
for line in "${said[@]-}"; do
    if [ -n "$line" ]; then printf 'verify: %s\n' "$line"; fi
done
if [ -n "$interrupted" ]; then
    printf 'verify: interrupted in %s: no step after it ran\n' "${reached:-its start}"
    exit 130
fi
if [ "${#red[@]}" -gt 0 ]; then
    printf 'verify: red in %s\n' "$(printf '%s, ' "${red[@]}" | sed 's/, $//')"
    exit 1
fi
printf 'verify: green: %s\n' "${chosen[*]}"
