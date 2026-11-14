#!/usr/bin/env bash
# JUSTFILE [STEP...]: the full verification, `just verify [STEP...]` (docs/CHECKS.md, "The
# release run"), run inside the verify run that recipe records. Every step runs whatever the ones before
# it did, and it is red when any step is, naming each red step and its log. The steps, in order:
#
#     lint          the lint-rs recipe
#     client-check  the client-check recipe into build/verified-client: the client built fresh and
#                   compared with the committed distribution
#     size-check    the size-check recipe: the shipped binaries' size budgets
#     docs-audit    the docs audit, `symphony docs-audit --check`
#     tests         every test target of the workspace, the browser tests among them, built once and
#                   run in lanes side by side (tools/test-lanes.sh)
#
# Each is recorded as a step of the verify run (docs/MACHINE_WORK.md) and writes its output to a
# log of its own, STEP.log in the folder SYMPHONY_CHECK_STEPS names (the release run names one for
# each check it runs), else build/verify, which steps.tsv there lists as each step ends: its name, exit
# status, seconds, log and command, and the machine's load as it ended (its load averages over 1, 5
# and 15 minutes and its processors, space-separated; empty where the system does not say),
# tab-separated (crates/symphony/src/release/steps.rs reads it). Named STEPs run alone, in the
# order above. An interrupt (Ctrl-C) stops it once the step it reached ends, exiting 130.
#
# The tests serve the client the client check built in this same verify (SYMPHONY_UI_DIST). The
# check's folder is cleared before it runs, so a client an earlier check left there is never served.
# With none (the client check built nothing, or did not run), SYMPHONY_UI_DIST is unset and the
# browser tests build the client themselves, red if that build fails; the tests' log and the
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

steps=(lint client-check size-check docs-audit tests)
chosen=()
for arg in "$@"; do
    known=''
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
        if [ "$step" = "$arg" ]; then
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
    tests)
        tests=(tools/test-lanes.sh)
        if [ -n "$fresh" ]; then
            how="the tests ran against the client the client check built in this verify: $fresh"
            case " ${red[*]-} " in
            *" client-check "*)
                how="the client check is red: the tests ran against the client it built from this tree, $fresh, not the committed one"
                ;;
            esac
            run tests "SYMPHONY_UI_DIST=\"\$PWD/$client/public\" ${tests[*]}" \
                env SYMPHONY_UI_DIST="$fresh" tools/record.sh --step tests test tests "${tests[@]}"
        else
            if [ -n "$checked" ]; then
                why='the client check built no client'
            else
                why='the client check did not run in this verify'
            fi
            how="$why, so SYMPHONY_UI_DIST was unset: the browser tests built the client themselves (build/symphony-ui), red if that build failed"
            run tests "${tests[*]}" \
                env -u SYMPHONY_UI_DIST tools/record.sh --step tests test tests "${tests[@]}"
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
