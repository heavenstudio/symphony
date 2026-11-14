#!/usr/bin/env bash
# This workspace's ordinary Cargo binary. Every run builds first; --where only names its location.
# A board binary run from a linked worktree against this repository's live board runs the main
# checkout's launcher instead, so no branch's build of it runs there (crates/tasks/AGENTS.md).
set -euo pipefail
root="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
mode=run
case "${1:-}" in
    --build|--where|--link) mode="${1#--}"; shift ;;
esac
name="${1:-}"
board=no
case "$name" in
    symphony|symphony-build) package=symphony-app; member=crates/symphony ;;
    tasks|agent-send|tasks-copy|tasks-reclaim) package=symphony-board; member=crates/tasks; board=yes ;;
    fake-hc) package=symphony-board; member=crates/tasks ;;
    symphony-server|symphony-shelf|symphony-ui-dev) package=symphony-server; member=apps/symphony-server ;;
    *) printf 'symphony rs-bin: no declared binary %s in %s\n' "$name" "$root" >&2; exit 2 ;;
esac
shift
if [ ! -f "$root/Cargo.toml" ] || [ ! -f "$root/$member/Cargo.toml" ] || [ ! -f "$root/$member/src/bin/$name.rs" ]; then
    printf 'symphony rs-bin: missing owned source for %s under %s\n' "$name" "$root" >&2
    exit 2
fi
target="${CARGO_TARGET_DIR:-$root/.target}"
if [[ "$target" != /* ]]; then target="$root/$target"; fi
binary="$target/debug/$name"
if [ "$mode" = where ]; then printf '%s\n' "$binary"; exit 0; fi

# The port `$1` names as the binary reads it (Rust's `u16`: an optional `+`, then digits, zeros
# leading or not), without its leading zeros; failure for any other spelling.
port_number() {
    local digits="${1#+}"
    case "$digits" in
        '' | *[!0-9]*) return 1 ;;
    esac
    digits="${digits#"${digits%%[!0]*}"}"
    digits="${digits:-0}"
    [ "${#digits}" -le 5 ] && [ "$digits" -le 65535 ] || return 1
    printf '%s\n' "$digits"
}

# Whether this run may reach this repository's live board: the `tasks` database (the last of the
# leading `--database NAME` options, else TASKS_DATABASE) on the board's own server, which is the one
# the working directory's repository reaches (TASKS_SERVER unset) when that repository is this one,
# or a TASKS_SERVER on the port recorded in `$common/symphony/tasks/server.json`, compared as the number
# the binary reads. A record naming no address, or a port spelled any other way, counts as that
# port; with no record there is no live server to name.
live_board() {
    local database="${TASKS_DATABASE:-tasks}" record="" file addr here named recorded
    while [ "${1:-}" = --database ]; do
        database="${2:-}"
        [ $# -ge 2 ] || break
        shift 2
    done
    [ "$database" = tasks ] || return 1
    if [ -n "${TASKS_SERVER:-}" ]; then
        file="$common/symphony/tasks/server.json"
        [ -f "$file" ] || return 1
        IFS= read -r record < "$file" || true
        case "$record" in
            *'"addr":"'*:*) addr="${record#*\"addr\":\"}"; addr="${addr%%\"*}" ;;
            *) return 0 ;;
        esac
        named="$(port_number "${TASKS_SERVER##*:}")" || return 0
        recorded="$(port_number "${addr##*:}")" || return 0
        [ "$named" = "$recorded" ]
        return
    fi
    here="$(pwd -P)"
    [ "$here" = "$root" ] && return 0
    here="$(unset GIT_DIR GIT_WORK_TREE GIT_COMMON_DIR; git rev-parse --path-format=absolute --git-common-dir 2>/dev/null)" || return 1
    [ "$here" = "$common" ]
}

# A checkout whose `.git` is a folder is a main checkout. Otherwise git names this one's folders: a
# linked worktree's git dir is not the common dir, whose main checkout is that dir less `/.git`, as git
# itself finds it; one ending otherwise belongs to a bare repository, which has no main checkout.
if [ "$mode" = run ] && [ "$board" = yes ] && [ ! -d "$root/.git" ]; then
    where="$(unset GIT_DIR GIT_WORK_TREE GIT_COMMON_DIR; git -C "$root" rev-parse --path-format=absolute --git-dir --git-common-dir --show-prefix 2>/dev/null)" || where=""
    git_dir="" common="" prefix=""
    { IFS= read -r git_dir; IFS= read -r common; IFS= read -r prefix; } <<< "$where" || true
    if [ -n "$git_dir" ] && [ "$git_dir" != "$common" ] && live_board "$@"; then
        main="${common%/.git}"
        launcher="$main/${prefix}tools/rs-bin.sh"
        if [ "$main" = "$common" ] || [ ! -x "$launcher" ]; then
            printf 'symphony rs-bin: %s %s reaches the live board from the worktree %s, which only the main checkout'"'"'s build may do, and %s has no launcher at %s; name a scratch board with --database NAME\n' \
                "$name" "$*" "$root" "$main" "$launcher" >&2
            exit 2
        fi
        # Main's build in main's own target folder, run in the caller's folder and environment.
        exec env -u CARGO_TARGET_DIR "$launcher" "$name" "$@"
    fi
fi

# A freshness check before a board or server command is no build of a task's: it is not recorded.
TASKS_RECORDER=off "$root/tools/cargo.sh" build --quiet --locked -p "$package" --bin "$name" >&2
if [ ! -x "$binary" ]; then
    printf 'symphony rs-bin: Cargo produced no native executable at %s\n' "$binary" >&2
    exit 2
fi
# Cargo replaces `$binary` with a new copy on every build, even one that relinked nothing, and macOS
# assesses every new executable file on its first run, for seconds. Runs go through a hard link to
# that copy, `$target/rs-bin/NAME`, renewed only when the copy's modification time or size moved:
# when Cargo really relinked. The copy keeps the time of the build it copies.
stable="$target/rs-bin/$name"
case "$OSTYPE" in
    darwin*) identity=(/usr/bin/stat -f '%Fm %z') ;;
    *) identity=(stat -c '%y %s') ;;
esac
same=no
if [ -x "$stable" ]; then
    ids="$("${identity[@]}" "$binary" "$stable" 2>/dev/null)" || ids=""
    first="${ids%%$'\n'*}"
    if [ -n "$first" ] && [ "$ids" = "$first"$'\n'"$first" ]; then same=yes; fi
fi
if [ "$same" = no ]; then
    mkdir -p "$target/rs-bin"
    staged="$target/rs-bin/.$name.$$"
    rm -f "$staged"
    ln "$binary" "$staged" 2>/dev/null || cp -p "$binary" "$staged"
    mv -f "$staged" "$stable"
fi
if [ "$mode" = build ]; then printf '%s\n' "$stable"; exit 0; fi
# The hooks' link, which `--link` points at `$stable` by a rename, so a hook never finds it missing.
# A run points one naming Cargo's copy at `$stable`, the same build.
hook="$root/.target/bin/$name"
relink() { ln -sfn "$stable" "$hook.$$" && mv -f "$hook.$$" "$hook"; }
if [ "$mode" = link ]; then
    mkdir -p "$root/.target/bin"
    relink
    exit 0
fi
if [ -L "$hook" ] && [ "$(readlink "$hook")" = "$binary" ]; then relink; fi
exec "$stable" "$@"
