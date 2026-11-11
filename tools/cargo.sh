#!/usr/bin/env bash
# Ordinary Cargo in this workspace; no containing-project planner, pool or source cache.
# A build or test run records itself for the board's Build and Test columns (docs/MACHINE_WORK.md):
# one file in sf-runs/ in the git common dir, its start line now and its end line when Cargo ends.
set -euo pipefail
root="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
if [ ! -f "$root/Cargo.toml" ]; then
    printf 'symphony cargo: missing workspace manifest %s/Cargo.toml\n' "$root" >&2
    exit 2
fi
cd "$root"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/.target}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
export RUST_TEST_THREADS="${RUST_TEST_THREADS:-6}"
cargo="${CARGO:-cargo}"
kind=
case "${1:-}" in
    build | check | clippy) kind=build ;;
    test | nextest) kind=test ;;
    symphony-ui) case "${2:-}" in dist | clippy | size) kind=build ;; esac ;;
esac
# Anything else, a run inside a recorded one, or a folder in no checkout: Cargo itself, as it was.
git="$root/.git"
if [ -z "$kind" ] || [ -n "${SYMPHONY_RUN:-}" ] || [ ! -e "$git" ]; then exec "$cargo" "$@"; fi

# Where the record goes and what it says, from git's files alone: no process is started for it.
line=
if [ -f "$git" ]; then
    IFS= read -r line < "$git" || true
    git="${line#gitdir: }"
    case "$git" in /*) ;; *) git="$root/$git" ;; esac
fi
common="$git"
if [ -f "$git/commondir" ]; then
    IFS= read -r line < "$git/commondir" || true
    case "$line" in /*) common="$line" ;; *) common="$git/$line" ;; esac
fi
head= branch= tip=
IFS= read -r head < "$git/HEAD" 2>/dev/null || true
case "$head" in
    'ref: refs/heads/'*)
        branch="${head#ref: refs/heads/}"
        if [ -f "$common/refs/heads/$branch" ]; then
            IFS= read -r tip < "$common/refs/heads/$branch" || true
        elif [ -f "$common/packed-refs" ]; then
            while read -r line ref; do
                if [ "$ref" = "refs/heads/$branch" ]; then tip="$line"; break; fi
            done < "$common/packed-refs"
        fi
        ;;
    *) tip="$head" ;;
esac
# A JSON string's body: backslashes, quotes and the control characters a path or argument holds.
quote() {
    quoted="${1//\\/\\\\}"
    quoted="${quoted//\"/\\\"}"
    quoted="${quoted//$'\n'/\\n}"
    quoted="${quoted//$'\r'/\\r}"
    quoted="${quoted//$'\t'/\\t}"
}
command=cargo
for arg in "$@"; do command+=" $arg"; done
fields=
for field in "checkout=$root" "branch=$branch" "tip=$tip" "handle=${TASKS_SESSION:-}" \
    "command=$command" "launcher=${SYMPHONY_RUN_LAUNCHER:-}"; do
    quote "${field#*=}"
    fields+=",\"${field%%=*}\":\"$quoted\""
done
runs="$common/sf-runs"
record="$runs/$$-$RANDOM$RANDOM.run"
if ! { [ -d "$runs" ] || mkdir -p "$runs"; } 2>/dev/null ||
    ! printf '{"v":1,"kind":"%s","pid":%d%s}\n' "$kind" "$$" "$fields" 2>/dev/null > "$record"; then
    exec "$cargo" "$@"
fi

# Cargo runs as this script's child, in its process group, with its arguments, streams and limits;
# the end line records its status, and a death by signal is raised again here.
export SYMPHONY_RUN="$record"
unset SYMPHONY_RUN_LAUNCHER
status=0
"$cargo" "$@" || status=$?
printf '{"exit":%d,"seconds":%d}\n' "$status" "$SECONDS" 2>/dev/null >> "$record" || true
if [ "$status" -gt 128 ]; then
    case "$((status - 128))" in
        1 | 2 | 3 | 4 | 5 | 6 | 8 | 9 | 11 | 13 | 14 | 15) kill -"$((status - 128))" "$$" ;;
    esac
fi
exit "$status"
