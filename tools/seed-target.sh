#!/usr/bin/env bash
# A linked worktree's first build starts from clones of the main checkout's compiled dependencies.
#
# tools/seed-target.sh TARGET, which tools/cargo.sh runs before Cargo: when TARGET does not exist
# yet and this checkout is a linked worktree whose main checkout has built `.target/debug`, TARGET
# is made with `debug/` a copy-on-write clone of that folder (clonefile on APFS, a reflink on
# Linux), so its compiled dependencies take no disk until a build replaces them. The clone is taken
# under Cargo's own lock on the main checkout's build folder, so no build there is caught halfway.
# The fingerprints of the workspace's own packages are left out, so the worktree compiles those from
# its own sources; Cargo checks every dependency it reuses as it always does.
#
# Nothing is ever copied byte for byte: across filesystems, on one that cannot clone, without a
# lock tool, or when the main checkout's build holds its lock past SF_SEED_WAIT seconds (120), it
# does nothing and Cargo builds from nothing. It never fails the build it runs before.
# SF_SEED_TARGET=off turns it off. `du` counts each clone in full: `df` says what the disk holds.
set -uo pipefail
target="${1:-}"
say() { printf 'symphony seed: %s\n' "$*" >&2; }
[ -n "$target" ] || exit 0
[ "${SF_SEED_TARGET:-on}" != off ] || exit 0
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
case "$OSTYPE" in
    darwin*)
        device() { /usr/bin/stat -f %d "$1"; }
        # `cp -c` falls back to a byte copy where it cannot clone: only an APFS volume clones.
        mount_point="$(/bin/df -P "$source" 2>/dev/null | awk 'NR == 2 { print $NF }')"
        [ -n "$mount_point" ] && /sbin/mount | grep -qF " on $mount_point (apfs," || exit 0
        clone=(/bin/cp -c -R -p)
        command -v lockf >/dev/null || exit 0
        locked() { local wait="$1" file="$2"; shift 2; lockf -k -n -s -t "$wait" "$file" "$@"; }
        ;;
    linux*)
        device() { stat -c %d "$1"; }
        # A reflink or nothing: `--reflink=always` fails rather than copying.
        clone=(cp -a --reflink=always)
        command -v flock >/dev/null || exit 0
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

wait="${SF_SEED_WAIT:-120}"
case "$wait" in '' | *[!0-9]*) wait=120 ;; esac
staged="$target.seed.$$"
rm -rf "$staged"
mkdir -p "$staged" || exit 0
if ! locked "$wait" "$source/.cargo-lock" "${clone[@]}" "$source" "$staged/debug" 2>/dev/null; then
    rm -rf "$staged"
    say "no clone of $source (its build held the lock past ${wait}s, or the volume refused); building from nothing"
    exit 0
fi
# What Cargo would take for the worktree's own packages is the main checkout's build of other sources.
for name in "${names[@]}"; do
    for fingerprint in "$staged/debug/.fingerprint/$name"-*; do
        hash="${fingerprint##*/"$name"-}"
        case "$hash" in
            [0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f])
                rm -rf "$fingerprint" ;;
        esac
    done
done
printf 'cloned from %s at %s\n' "$source" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$staged/SEEDED"
if [ -e "$target" ] || ! mv "$staged" "$target" 2>/dev/null; then
    rm -rf "$staged"
    exit 0
fi
# Another run made the folder between the test and the rename, and the clone went inside it.
if [ -e "$target/${staged##*/}" ]; then
    rm -rf "${target:?}/${staged##*/}"
    exit 0
fi
say "$target/debug starts as a clone of $source (no disk until a build changes it)"
exit 0
