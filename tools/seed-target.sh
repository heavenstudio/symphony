#!/usr/bin/env bash
# A linked worktree's first build starts from clones of the main checkout's compiled dependencies.
#
# tools/seed-target.sh TARGET, which tools/cargo.sh runs before Cargo: when TARGET does not exist
# yet and this checkout is a linked worktree whose main checkout has built `.target/debug`, TARGET
# is made with `debug/` a copy-on-write clone of that folder (clonefile on APFS, a reflink on
# Linux), so its compiled dependencies take no disk until a build replaces them. The clone is taken
# under Cargo's own lock on the main checkout's build folder, so no build there is caught halfway.
# While it holds that lock, a build there and every board command from any worktree (each builds
# the main checkout's launcher) waits. On macOS it is held for one clonefile call of the whole
# folder (tools/seed-clone.rs, which rustc compiles here first: under a second), else for `cp -c`
# cloning each file (seconds to a minute). Once it is released, the fingerprints of the workspace's
# own packages are left out, so the worktree compiles those from its own sources; Cargo checks every
# dependency it reuses as it always does.
#
# The clone is staged as `TARGET.seed.PID` beside TARGET (git ignores it) and renamed into place. A
# signal that ends the seed ends the clone and removes the staging. One left by a seed killed
# outright, or a sweep's `TARGET.clear.PID`, goes on a later run once its process is gone.
#
# Nothing is ever copied byte for byte: across filesystems, on one that cannot clone, without a
# lock tool, or when the main checkout's build holds its lock past SYMPHONY_SEED_WAIT seconds (120),
# it does nothing and Cargo builds from nothing. It never fails the build it runs before.
# SYMPHONY_SEED_TARGET=off turns it off. `du` counts each clone in full: `df` says what the disk
# holds.
set -uo pipefail
target="${1:-}"
say() { printf 'symphony seed: %s\n' "$*" >&2; }
[ -n "$target" ] || exit 0
[ "${SYMPHONY_SEED_TARGET:-on}" != off ] || exit 0

# What a seed or a clear left beside TARGET, once its process is gone; nothing when `ps` cannot say.
if ps -p $$ >/dev/null 2>&1; then
    for left in "$target".seed.* "$target".clear.*; do
        pid="${left##*.}"
        case "$pid" in '' | *[!0-9]*) continue ;; esac
        [ -d "$left" ] && [ ! -L "$left" ] || continue
        ps -p "$pid" >/dev/null 2>&1 || rm -rf "$left"
    done
fi

[ ! -e "$target" ] && [ ! -L "$target" ] || exit 0
root="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)" || exit 0

# A linked worktree's git dir is not its common dir; the main checkout is the common dir less `/.git`.
where="$(unset GIT_DIR GIT_WORK_TREE GIT_COMMON_DIR; git -C "$root" rev-parse --path-format=absolute --git-dir --git-common-dir 2>/dev/null)" || exit 0
git_dir="${where%%$'\n'*}"
common="${where#*$'\n'}"
[ -n "$git_dir" ] && [ "$git_dir" != "$common" ] || exit 0
main="${common%/.git}"
[ "$main" != "$common" ] || exit 0
source="$main/.target/debug"
[ -d "$source/deps" ] && [ -d "$source/.fingerprint" ] && [ -f "$source/.cargo-lock" ] || exit 0

parent="$(dirname "$target")"
[ -d "$parent" ] || exit 0
whole=""
case "$OSTYPE" in
    darwin*)
        device() { /usr/bin/stat -f %d "$1"; }
        # `cp -c` falls back to a byte copy where it cannot clone: only an APFS volume clones.
        mount_point="$(/bin/df -P "$source" 2>/dev/null | awk 'NR == 2 { print $NF }')"
        [ -n "$mount_point" ] && /sbin/mount | grep -qF " on $mount_point (apfs," || exit 0
        clone=(/bin/cp -c -R -p)
        whole="$root/tools/seed-clone.rs"
        command -v lockf >/dev/null || exit 0
        # What lockf exits with when the lock was not to be had in time (EX_TEMPFAIL).
        timed_out=75
        locked() { local wait="$1" file="$2"; shift 2; lockf -k -n -s -t "$wait" "$file" "$@"; }
        ;;
    linux*)
        device() { stat -c %d "$1"; }
        # A reflink or nothing: `--reflink=always` fails rather than copying.
        clone=(cp -a --reflink=always)
        command -v flock >/dev/null || exit 0
        timed_out=1
        locked() { local wait="$1" file="$2"; shift 2; flock -w "$wait" "$file" "$@"; }
        ;;
    *) exit 0 ;;
esac
[ "$(device "$source")" = "$(device "$parent")" ] || exit 0

# The workspace's packages, as Cargo names them: `…/crates/tasks#symphony-board@0.1.0` or
# `…/crates/symphony#0.1.0`.
members="$(cd "$root" && "${CARGO:-cargo}" metadata --no-deps --format-version 1 --offline 2>/dev/null |
    grep -o '"workspace_members":\[[^]]*\]')" || exit 0
names=()
while IFS= read -r id; do
    case "$id" in
        *'#'*@*) name="${id#*#}"; name="${name%@*}" ;;
        *'#'*) name="${id%#*}"; name="${name##*/}" ;;
        *) continue ;;
    esac
    [ -n "$name" ] && names+=("$name")
done < <(printf '%s\n' "$members" | grep -o '"[^"]*"' | tr -d '"' | grep -v '^workspace_members$')
[ "${#names[@]}" -gt 0 ] || exit 0

wait="${SYMPHONY_SEED_WAIT:-120}"
case "$wait" in '' | *[!0-9]*) wait=120 ;; esac

# Whatever ends this run ends the clone it started and removes what it staged. On a signal the run
# then ends by that signal, so a Ctrl-C stops the build it came before as well. A trap runs between
# two commands, so one run between the clone's fork and `job=$!` would find no job to end and leave
# it running, waiting on the lock and then cloning: a signal while the job starts waits until the
# job is recorded and job control is off again (`starting`, `pending`).
staged=""
job=""
starting=""
pending=""
# `until_ended`: once the read end of the job's FIFO (6) reads its end of file. macOS can lose a
# FIFO's end for a read already blocked on it as its last writer goes, while a read begun after
# answers it at once (crates/process-cwd/src/spawn.rs; measured here with the job killed as the
# read blocks: 9 ends lost in 1.2 million reads at loads of 70 to 145, and each of the 7 asked
# again answered at once): so each read is given a second and asked again once it times out, a
# check for an event its watcher can drop (RULES.md code/no-timed-waits). A read that times out
# leaves REPLY as it was in macOS's bash (3.2) and answers above 128 from bash 4 on; an end of
# file sets it empty. Nothing writes to the FIFO, so a line read ends the wait too. A read end not
# open (a second signal's `finish` run inside the first's once it closed it, or an open of it that
# failed) leaves nothing to wait on: a read of it would fail as a timeout does, so it is looked
# for first, and its absence ends the wait.
until_ended() {
    local status
    while :; do
        { : <&6; } 2>/dev/null || return 0
        REPLY=unended
        read -r -t 1 <&6
        status=$?
        [ "$status" -gt 128 ] || [ "$REPLY" = unended ] || return 0
    done
}
# shellcheck disable=SC2329 # the traps below run it
finish() {
    if [ -n "$job" ]; then
        kill -TERM -- "-$job" 2>/dev/null
        wait "$job" 2>/dev/null
        # The rest of its group, the lock tool and the copy, can outlive the job's shell: it is
        # ended outright and waited for, so the staging never goes under a copy still writing it.
        # Each member holds the write end of the job's FIFO (`under_lock`), so the read end's end
        # of file is the last of them gone.
        kill -KILL -- "-$job" 2>/dev/null
        until_ended
        exec 6<&-
        job=""
    fi
    if [ -n "$staged" ]; then
        rm -rf "$staged"
        staged=""
    fi
}
# `ended SIGNAL`: this run ends by SIGNAL, at once, or while the job starts once it is recorded.
ended() {
    if [ -n "$starting" ]; then
        pending="$1"
        return
    fi
    finish
    trap - "$1"
    kill -"$1" $$
}
trap finish EXIT
for signal in HUP INT TERM; do
    # shellcheck disable=SC2064 # each trap names its own signal
    trap "ended $signal" "$signal"
done

# `under_lock COMMAND…`: COMMAND's status, run holding the main checkout's build lock. It runs as a
# job of its own process group, which `finish` ends whole, the lock tool and the copy with it. Bash
# puts the job in its group from both sides of the fork; the two calls race, macOS at times refuses
# one though the other has made the group, and bash says so for the job's own call (`child setpgid
# (PID to PID): Operation not permitted`) before the job's own redirection: so the job is forked
# with stderr at /dev/null already. The job is forked holding a FIFO open to read and write (5),
# which every member of its group inherits; this run then keeps the read end alone (6), whose end of
# file says that every member has gone, which is what `finish` waits for. Opened both ways first,
# neither open waits for the other side; an open that fails forks nothing, as the read end alone
# would wait for a writer for ever.
under_lock() {
    local held="$staged/tool/held"
    rm -f "$held"
    mkfifo "$held" || return 1
    exec 5<>"$held" || return 1
    starting=1
    set -m
    { locked "$wait" "$source/.cargo-lock" "$@" & } 2>/dev/null
    job=$!
    set +m
    exec 6<"$held" 5<&-
    starting=""
    [ -z "$pending" ] || ended "$pending"
    wait "$job"
    local status=$?
    job=""
    exec 6<&-
    return "$status"
}

staged="$target.seed.$$"
rm -rf "$staged"
mkdir -p "$staged/tool" || exit 0
# One clonefile call where rustc builds the helper; `cp` where it does not, or the call fails.
how=""
if [ -n "$whole" ] && [ -f "$whole" ] &&
    "${RUSTC:-rustc}" --edition 2024 -o "$staged/tool/seed-clone" "$whole" >/dev/null 2>&1; then
    how="one clonefile call"
    under_lock "$staged/tool/seed-clone" "$source" "$staged/debug"
    status=$?
    if [ "$status" -ne 0 ] && [ "$status" -ne "$timed_out" ]; then
        rm -rf "$staged/debug"
        how=""
    fi
fi
if [ -z "$how" ]; then
    how="${clone[*]}"
    under_lock "${clone[@]}" "$source" "$staged/debug"
    status=$?
fi
rm -rf "$staged/tool"
if [ "$status" -ne 0 ]; then
    say "no clone of $source (its build held the lock past ${wait}s, or the volume refused); building from nothing"
    exit 0
fi
# The workspace's own units go: Cargo would take them for the worktree's though built from the main
# checkout's sources, and each clone of one would come to hold disk of its own once the main
# checkout rebuilt it. A unit is its fingerprint folder `NAME-HASH`, its outputs in deps/ and
# examples/ named `…-HASH` or `…-HASH.…`, a build script's folder in build/, and a binary's or an
# example's copy under its own name.
debug="$staged/debug"
hex16='[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]'
for name in "${names[@]}"; do
    for fingerprint in "$debug/.fingerprint/$name"-*; do
        hash="${fingerprint##*/"$name"-}"
        # shellcheck disable=SC2254 # the pattern is sixteen hex digits
        case "$hash" in $hex16) ;; *) continue ;; esac
        for unit in "$fingerprint"/bin-* "$fingerprint"/example-*; do
            [ -f "$unit" ] || continue
            case "${unit##*/}" in
                *.json) ;;
                bin-*) rm -f "$debug/${unit##*/bin-}" "$debug/${unit##*/bin-}.d" ;;
                example-*) rm -f "$debug/examples/${unit##*/example-}" "$debug/examples/${unit##*/example-}.d" ;;
            esac
        done
        find "$debug/deps" "$debug/examples" -maxdepth 1 -type f \( -name "*-$hash" -o -name "*-$hash.*" \) -exec rm -f {} + 2>/dev/null
        rm -rf "$debug/build/$name-$hash" "$fingerprint"
    done
done
printf 'cloned from %s by %s at %s\n' "$source" "$how" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$staged/SEEDED"
# Another run's folder made since the test stays; this staging goes with the run (`finish`).
if [ -e "$target" ] || ! mv "$staged" "$target" 2>/dev/null; then
    exit 0
fi
moved="$staged"
staged=""
# Another run made the folder between the test and the rename, and the clone went inside it.
if [ -e "$target/${moved##*/}" ]; then
    rm -rf "${target:?}/${moved##*/}"
    exit 0
fi
say "$target/debug starts as a clone of $source (no disk until a build changes it)"
exit 0
