#!/usr/bin/env bash
# This workspace's ordinary Cargo binary. A run builds first unless nothing Cargo would read changed
# since the last build this launcher asked it for (`fresh`); --build and --link always build, and
# --where only names Cargo's copy. A board binary run from a linked worktree against this
# repository's live board runs the main checkout's launcher instead, so no branch's build of it runs
# there (crates/tasks/AGENTS.md).
set -euo pipefail
root="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
mode=run
case "${1:-}" in
    --build|--where|--link) mode="${1#--}"; shift ;;
esac
name="${1:-}"
board=no
case "$name" in
    symphony) package=symphony-app; member=crates/symphony ;;
    symphony-build) package=symphony-build; member=crates/symphony-build ;;
    tasks|agent-send|tasks-copy|tasks-reclaim|tasks-move) package=symphony-board; member=crates/tasks; board=yes ;;
    fake-hc) package=symphony-board; member=crates/tasks ;;
    symphony-server|symphony-shelf) package=symphony-server; member=apps/symphony-server ;;
    symphony-desktop) package=symphony-desktop; member=apps/symphony-desktop ;;
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

# The folder the socket of the board's PostgreSQL on the data folder `$1` is in, as the board names
# it (`server::postgres::socket_folder`): `/tmp/symphony-pg-` and the first 16 hex digits of the
# SHA-256 of the data's path; failure where no SHA-256 program is found.
socket_folder() {
    local digest
    if command -v shasum > /dev/null; then
        digest="$(printf '%s' "$1" | shasum -a 256)" || return 1
    elif command -v sha256sum > /dev/null; then
        digest="$(printf '%s' "$1" | sha256sum)" || return 1
    else
        return 1
    fi
    printf '/tmp/symphony-pg-%s\n' "${digest:0:16}"
}

# The folder `$1` with its links followed, so `/tmp` and `/private/tmp` on macOS are one; `$1` as
# it is where it cannot be entered.
resolved() {
    (cd "$1" 2> /dev/null && pwd -P) || printf '%s\n' "$1"
}

# Whether this run may reach this repository's live board: the `tasks` database (the last of the
# leading `--database NAME` options, else TASKS_DATABASE) on the board's own server, which is the one
# the working directory's repository reaches (TASKS_SERVER unset) when that repository is this one,
# or a TASKS_SERVER naming the socket folder of the PostgreSQL on that board's data
# (`$common/symphony/tasks/postgresql`), compared with links followed, as the board compares it
# (`live::names_address`). A TASKS_SERVER of any other form names no board a board binary opens, as
# every one refuses it, and where the socket folder cannot be named it counts as the live one.
live_board() {
    local database="${TASKS_DATABASE:-tasks}" here expected
    while [ "${1:-}" = --database ]; do
        database="${2:-}"
        [ $# -ge 2 ] || break
        shift 2
    done
    [ "$database" = tasks ] || return 1
    if [ -n "${TASKS_SERVER:-}" ]; then
        case "$TASKS_SERVER" in
            postgresql:*) ;;
            *) return 1 ;;
        esac
        # The board names its data by the common dir with its links followed (`git::read`).
        expected="$(socket_folder "$(resolved "$common")/symphony/tasks/postgresql")" || return 0
        [ "$(resolved "${TASKS_SERVER#postgresql:}")" = "$(resolved "$expected")" ]
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

# Cargo replaces `$binary` with a new copy on every build, even one that relinked nothing, and macOS
# assesses every new executable file on its first run, for seconds. Runs go through a hard link to
# that copy, `$target/rs-bin/NAME`, renewed only when the copy's modification time or size moved:
# when Cargo really relinked. The copy keeps the time of the build it copies.
stable="$target/rs-bin/$name"
# The hooks' link, which `--link` points at `$stable` by a rename, so a hook never finds it missing.
# A run points one naming Cargo's copy at `$stable`, the same build.
hook="$root/.target/bin/$name"
relink() { ln -sfn "$stable" "$hook.$$" && mv -f "$hook.$$" "$hook"; }
run_stable() {
    if [ -L "$hook" ] && [ "$hook" -ef "$binary" ] && [ "$(readlink "$hook")" = "$binary" ]; then
        relink
    fi
    exec "$stable" "$@"
}

# What the last build this launcher asked Cargo for read, and under what (`write_fresh`), with the
# second taken before Cargo started. A run whose build `fresh` finds as it was runs `$stable` without
# asking Cargo, whose no-op check costs a board command more than the rest of the launcher does.
depinfo="$target/debug/$name.d"
fresh_file="$target/rs-bin/.$name.fresh"
fresh_format='symphony rs-bin fresh 2'
# A file's identity as the relink compares it (time and size); and, for each of the files named,
# links followed, its status change time in seconds before that identity: it moves whenever anything
# at all is written to the file, renamed onto its path or done to its times, and nothing but the
# clock sets it back.
case "$OSTYPE" in
    darwin*) identity=(/usr/bin/stat -f '%Fm %z'); changed=(/usr/bin/stat -L -f '%c %Fm %z') ;;
    *) identity=(stat -c '%y %s'); changed=(stat -L -c '%Z %y %s') ;;
esac

# The program `$1` as Cargo's wrapper finds it from the checkout's root, in `found`: as written when
# it holds a slash, else the first in PATH (an empty or relative entry read from the root), else
# `none`. No child process.
found=""
find_program() {
    local rest="${PATH-}:" dir
    found="$1"
    case "$1" in */*) return 0 ;; esac
    found=none
    while [ -n "$rest" ]; do
        dir="${rest%%:*}"
        rest="${rest#*:}"
        case "$dir" in /*) ;; *) dir="$root/$dir" ;; esac
        if [ -f "$dir/$1" ] && [ -x "$dir/$1" ]; then
            found="$dir/$1"
            return 0
        fi
    done
}

# The environment Cargo builds under, as one quoted word in `key`: what it builds from and where, the
# cargo and rustc it runs, HOME (where Cargo's and rustup's own settings are), and each variable Cargo,
# rustc or rustup reads (CARGO*, RUSTC*, RUSTDOC*, RUSTFLAGS*, RUSTUP*, __CARGO*) or `$1` names (those
# the target's build scripts declared and its sources read), set or not. A name bash cannot read, as
# a target triple's hyphens make one, is left out.
key=""
build_key() {
    local word
    key="$root $target $package $name"
    find_program "${CARGO:-cargo}"
    key+=" cargo=$found"
    find_program rustc
    key+=" rustc=$found"
    for word in HOME ${!CARGO*} ${!RUSTC*} ${!RUSTDOC*} ${!RUSTFLAGS*} ${!RUSTUP*} ${!__CARGO*} $1; do
        case "$word" in [!A-Za-z_]* | *[!A-Za-z0-9_]*) continue ;; esac
        if [ -n "${!word+set}" ]; then key+=" $word=${!word}"; else key+=" $word"; fi
    done
    printf -v key '%q' "$key"
}

# Whether `$stable` is what Cargo would build now, as the stamp says: one this launcher wrote, under
# this environment, at a second not after now (a clock set back since counts as a change), for the
# build `$stable` is (its identity, so a build another run linked since counts too), each path it
# names as it was: `- PATH` still absent, and `+ PATH` there with its status last changed in a second
# before the stamp's, so a change in the stamp's own second counts. What the stamp's own file times
# say counts for nothing, so a stamp copied, restored or touched vouches for no change made after its
# second. Anything else asks Cargo. It cannot see a toolchain a channel pin names updated in place
# (this workspace pins a version), or a file changed while the clock stood before the stamp's second
# and set right since, which fools Cargo too.
fresh() {
    local line count=0 at="" names="" built="" now
    local -a paths=()
    [ -x "$stable" ] && [ -f "$fresh_file" ] && [ -r "$fresh_file" ] || return 1
    # Line by line: bash reads a regular file in blocks only up to newlines.
    while IFS= read -r line; do
        count=$((count + 1))
        case "$count:$line" in
            "1:$fresh_format") ;;
            "2:at "*) at="${line#at }" ;;
            "3:env "*) names="${line#env }" ;;
            "4:key "*)
                build_key "$names"
                [ "$line" = "key $key" ] || return 1
                ;;
            "5:built "*) built="${line#built }" ;;
            [1-5]:*) return 1 ;;
            *:'+ /'*) paths+=("${line#+ }") ;;
            *:'- /'*) [[ ! -e ${line#- } && ! -L ${line#- } ]] || return 1 ;;
            *) return 1 ;;
        esac
    done 2>/dev/null < "$fresh_file"
    # A last line with no newline is no line the launcher wrote.
    [ "$count" -gt 5 ] && [ -z "$line" ] && [ "${#paths[@]}" -gt 0 ] || return 1
    case "$at" in '' | *[!0-9]*) return 1 ;; esac
    now="$(date +%s)" || return 1
    [ "$now" -ge "$at" ] 2>/dev/null || return 1
    # One stat for the build and every path, a line each; one gone or unreadable fails it.
    line="$("${changed[@]}" "$stable" "${paths[@]}" 2>/dev/null)" || return 1
    local IFS=$'\n'
    set -f
    # shellcheck disable=SC2086 # one line a word
    set -- $line
    set +f
    [ "$#" -eq $((${#paths[@]} + 1)) ] && [ "${1#* }" = "$built" ] || return 1
    shift
    for line; do
        line="${line%% *}"
        case "$line" in '' | *[!0-9]*) return 1 ;; esac
        [ "$line" -lt "$at" ] || return 1
    done
}

# `+ PATH` when there is something at PATH, else `- PATH`, on `lines`.
fresh_mark() {
    if [ -e "$1" ]; then lines+="+ $1"$'\n'; else lines+="- $1"$'\n'; fi
}

# The stamp `fresh` reads, after a build this launcher asked Cargo for, which started in the second
# `$at`. It names Cargo's own account of what the build read (`debug/NAME.d`: each source of the
# workspace's packages it compiled, each file a source includes and each file or folder a local build
# script watches, with everything under such a folder, as Cargo looks at all of it) and the manifest
# of each package holding one; the workspace's manifest and lock, this launcher and Cargo's wrapper;
# each Cargo configuration and toolchain pin Cargo or rustup would read, from the root up and in
# CARGO_HOME, and rustup's settings, there or not; and the environment, with each variable a build
# script in the target declared and each one a source read as rustc recorded it (`# env-dep:` in the
# dep-info of each unit in the target); and the identity of `$stable`, the build it vouches for. None
# when that account is not plain absolute paths of files there.
write_fresh() {
    local content path dir last="" seen=$'\n' lines="" names walked header="" stamped="" was=""
    local recorded built
    local -a folders=() tops=()
    [ -f "$depinfo" ] && [ -r "$depinfo" ] || return 1
    # Its trailing newlines dropped by the substitution: bash 3.2 matches a pattern against each end
    # of a long string in time that grows with its square.
    content="$(< "$depinfo")" || return 1
    case "${content%%: *}" in */"$name") ;; *) return 1 ;; esac
    content="${content#*: }"
    case "$content" in '' | *\\* | *$'\n'* | *$'\t'*) return 1 ;; esac
    set -f
    for path in $content; do
        if [[ "$path" != /* ]] || [ ! -e "$path" ]; then
            set +f
            return 1
        fi
        lines+="+ $path"$'\n'
        [ ! -d "$path" ] || folders+=("$path")
        dir="${path%/*}"
        [ "$dir" != "$last" ] || continue
        last="$dir"
        while [ -n "$dir" ]; do
            case "$seen" in *$'\n'"$dir"$'\n'*) break ;; esac
            seen+="$dir"$'\n'
            if [ -f "$dir/Cargo.toml" ]; then
                lines+="+ $dir/Cargo.toml"$'\n'
                break
            fi
            dir="${dir%/*}"
        done
    done
    set +f
    if [ "${#folders[@]}" -gt 0 ]; then
        # Each folder under none of the others, walked once.
        for path in "${folders[@]}"; do
            for dir in "${folders[@]}"; do
                case "$path" in "$dir"/*) continue 2 ;; esac
            done
            tops+=("$path")
        done
        walked="$(find -L "${tops[@]}" -mindepth 1 -print)" || return 1
        case "$walked" in *\\* | *$'\t'*) return 1 ;; esac
        while IFS= read -r path; do
            [ -n "$path" ] || continue
            [[ "$path" == /* ]] && [ -e "$path" ] || return 1
            lines+="+ $path"$'\n'
        done <<< "$walked"
    fi
    lines+="+ $root/Cargo.toml"$'\n'"+ $root/tools/rs-bin.sh"$'\n'"+ $root/tools/cargo.sh"$'\n'
    fresh_mark "$root/Cargo.lock"
    dir="$root"
    while :; do
        for path in "$dir/.cargo/config.toml" "$dir/.cargo/config" "$dir/rust-toolchain.toml" "$dir/rust-toolchain"; do
            fresh_mark "$path"
        done
        [ -n "$dir" ] || break
        dir="${dir%/*}"
    done
    fresh_mark "${CARGO_HOME:-${HOME-}/.cargo}/config.toml"
    fresh_mark "${CARGO_HOME:-${HOME-}/.cargo}/config"
    fresh_mark "${RUSTUP_HOME:-${HOME-}/.rustup}/settings.toml"
    # The variables rustc recorded: those of the stamp there when it was written for this same build
    # (no unit of it has compiled since, and a relink removes the stamp first), else what each unit's
    # dep-info in the target says.
    built="$("${identity[@]}" "$stable" 2>/dev/null)" || return 1
    if [ -f "$fresh_file" ]; then
        {
            IFS= read -r header && IFS= read -r stamped && IFS= read -r stamped &&
                IFS= read -r was && IFS= read -r was
        } < "$fresh_file" 2>/dev/null || header=""
    fi
    if [ "$header" = "$fresh_format" ] && [ "$was" = "built $built" ] && [[ "$stamped" == "env "* ]] &&
        [[ "${stamped#env }" != *[!A-Za-z0-9_\ ]* ]]; then
        recorded="${stamped#env }"
    else
        recorded="$(grep -h -s '^# env-dep:' "$target"/debug/deps/*.d "$target"/debug/build/*/*.d |
            sed -n 's/^# env-dep:\([A-Za-z_][A-Za-z0-9_]*\)=.*/\1/p; s/^# env-dep:\([A-Za-z_][A-Za-z0-9_]*\)$/\1/p')" || true
    fi
    names="$({
        cat "$target"/debug/build/*/output 2>/dev/null |
            sed -n 's/^cargo::\{0,1\}rerun-if-env-changed=\([A-Za-z_][A-Za-z0-9_]*\)$/\1/p'
        printf '%s\n' "$recorded"
    } | tr ' ' '\n' | sort -u | tr '\n' ' ')" || true
    build_key "$names"
    printf '%s\nat %s\nenv %s\nkey %s\nbuilt %s\n%s' "$fresh_format" "$at" "$names" "$key" "$built" "$lines" > "$fresh_file.$$" &&
        mv -f "$fresh_file.$$" "$fresh_file"
}

if [ "$mode" = run ] && fresh; then run_stable "$@"; fi

# A build: the stamp's second is taken first, once the target folder exists (one that does not yet is
# the seed's to make, tools/seed-target.sh, so the first build of a checkout leaves no stamp and the
# next asks Cargo once more).
at=""
if [ -d "$target" ]; then
    at="$(date +%s)"
    trap 'rm -f "$fresh_file.$$"' EXIT
fi
# A freshness check before a board or server command is no build of a task's: it is not recorded.
TASKS_RECORDER=off "$root/tools/cargo.sh" build --quiet --locked -p "$package" --bin "$name" >&2
if [ ! -x "$binary" ]; then
    printf 'symphony rs-bin: Cargo produced no native executable at %s\n' "$binary" >&2
    exit 2
fi
same=no
if [ -x "$stable" ]; then
    ids="$("${identity[@]}" "$binary" "$stable" 2>/dev/null)" || ids=""
    first="${ids%%$'\n'*}"
    if [ -n "$first" ] && [ "$ids" = "$first"$'\n'"$first" ]; then same=yes; fi
fi
if [ "$same" = no ]; then
    mkdir -p "$target/rs-bin"
    # No stamp outlives the build it vouched for, even when this run ends before it writes one.
    rm -f "$fresh_file"
    staged="$target/rs-bin/.$name.$$"
    rm -f "$staged"
    ln "$binary" "$staged" 2>/dev/null || cp -p "$binary" "$staged"
    mv -f "$staged" "$stable"
fi
if [ -n "$at" ]; then
    write_fresh || rm -f "$fresh_file"
    rm -f "$fresh_file.$$"
fi
if [ "$mode" = build ]; then printf '%s\n' "$stable"; exit 0; fi
if [ "$mode" = link ]; then
    mkdir -p "$root/.target/bin"
    relink
    exit 0
fi
run_stable "$@"
