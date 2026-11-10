#!/usr/bin/env bash
# The checksum-pinned SurrealDB executable used by the board fixtures.
#
#     bash tools/surreal.sh     prints the path of the pinned `surreal`, fetching it first if needed
#
# SurrealDB's own release binary, never built here: version VERSION, fetched once per machine into
# `sf-surreal/` in the git common dir (every worktree shares it, as `sf-cargo/`; outside a repository
# it goes beside the workspace's own build) and installed only when the tarball matches the SHA-256
# SurrealDB publishes for this platform, pinned below in `pinned` (`<URL without .tgz>.txt` beside
# each tarball). The Rust tests start it (`crates/tasks/tests/store/common.rs`), unless `SF_SURREAL`
# names another. Of callers at once, one fetches (`lock.d` in the folder, naming its holder; one
# whose holder is gone, or that names none a minute after it was made, is taken over); nothing but
# the checked binary is ever left in the folder.
# A refusal is one line on stderr, `❌ surreal: …`, and exit 2.
#
# Sourced, it defines its functions and runs nothing. Test harnesses can supply fetch and pinned
# at the same function boundary. Bash 3.2 is supported.

VERSION=3.3.0
FOLDER=sf-surreal

# SurrealDB's name for a build, the tarball's URL.
url() {
    printf 'https://download.surrealdb.com/v%s/surreal-v%s.%s.tgz\n' "$VERSION" "$VERSION" "$1"
}

# The SHA-256 SurrealDB publishes for build $1's tarball of VERSION; exit 1 for a build not pinned.
pinned() {
    case "$1" in
    darwin-arm64) echo 75e37adf5f9aacfa1308df71193b2d0e0727bc6532105541c083e71271cd7fc9 ;;
    darwin-amd64) echo c8e560d37c9f6f04b95f22791723122b5579f1fa5ac5102d6d82c36e38b72710 ;;
    linux-amd64) echo 44aeab565f7e7e39d2d0bf0583c8aae648babc91373c70b2658288d95bbbcd55 ;;
    linux-arm64) echo f03356497f875057126641f06757671542e0a7b75f18fd13820c2dab29349d74 ;;
    *) return 1 ;;
    esac
}

# Every pinned build.
PINNED="darwin-arm64 darwin-amd64 linux-amd64 linux-arm64"

refuse() {
    printf '❌ surreal: %s\n' "$*" >&2
}

# SurrealDB's name for the build of system $1 (`uname -s`) on machine $2 (`uname -m`), one pinned.
platform_name() {
    local system machine
    case "$1" in
    Darwin) system=darwin ;;
    Linux) system=linux ;;
    *) system='?' ;;
    esac
    case "$2" in
    arm64 | aarch64) machine=arm64 ;;
    x86_64 | amd64) machine=amd64 ;;
    *) machine='?' ;;
    esac
    if ! pinned "$system-$machine" >/dev/null; then
        refuse "no SurrealDB $VERSION build is pinned for $1 $2 (tools/surreal.sh pinned)"
        return 1
    fi
    printf '%s\n' "$system-$machine"
}

# The folder $1, every link in it followed.
resolved() {
    (cd "$1" 2>/dev/null && pwd -P)
}

# `SF_GIT_COMMON_DIR`, by the rule the test slots take it by (an absolute path git takes as
# --git-dir, else refused), else the git common dir of the repository holding workspace $1, else
# nothing: as `sf-slots cargo` finds its build folder.
common_dir() {
    local common=""
    if [ -n "${SF_GIT_COMMON_DIR:-}" ]; then
        case "$SF_GIT_COMMON_DIR" in
        /*)
            if [ -d "$SF_GIT_COMMON_DIR" ]; then
                common="$(git --git-dir="$SF_GIT_COMMON_DIR" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)" || common=""
            fi
            ;;
        esac
        case "$common" in
        /*) resolved "$common" ;;
        *)
            refuse "SF_GIT_COMMON_DIR=$SF_GIT_COMMON_DIR is not a git dir (an absolute path git takes as --git-dir)"
            return 1
            ;;
        esac
        return
    fi
    if common="$(git -C "$1" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)" && [ -n "$common" ]; then
        resolved "$common"
    fi
}

# `sf-surreal/` in the common dir every worktree of workspace $1 shares, else beside its own build.
folder() {
    local common
    common="$(common_dir "$1")" || return 1
    if [ -n "$common" ]; then
        printf '%s/%s\n' "$common" "$FOLDER"
    else
        printf '%s/.target/%s\n' "$1" "$FOLDER"
    fi
}

# Download URL $1 into file $2; on failure, why, on stdout.
fetch() {
    curl -fsS --connect-timeout 60 --max-time 900 -o "$2" "$1" 2>&1
}

sha256() {
    local digest
    if command -v sha256sum >/dev/null 2>&1; then
        digest="$(sha256sum "$1")" || return 1
    else
        digest="$(shasum -a 256 "$1")" || return 1
    fi
    printf '%s\n' "${digest%% *}"
}

# Minutes a lock may name no holder before it counts as gone: its maker writes its pid an instant
# after making it, so one with none this long after was left by a process that ended in between.
OWNERLESS_MINUTES=1

# Whether folder $1's lock has a holder: a pid a process has, or, naming none, made under
# OWNERLESS_MINUTES ago (`find -mmin` is in minutes on BSD and GNU alike).
held() {
    local holder
    holder="$(cat "$1/lock.d/pid" 2>/dev/null || true)"
    if [ -n "$holder" ]; then
        kill -0 "$holder" 2>/dev/null
    else
        [ -d "$1/lock.d" ] && [ -z "$(find "$1/lock.d" -maxdepth 0 -mmin +"$OWNERLESS_MINUTES" 2>/dev/null)" ]
    fi
}

# Wait for folder $1's lock, taking over one whose holder is gone (its pid dead, or none named in time).
lock() {
    until mkdir "$1/lock.d" 2>/dev/null; do
        if ! held "$1"; then
            mv "$1/lock.d" "$1/lock.stale.$$" 2>/dev/null && rm -rf "$1/lock.stale.$$"
            continue
        fi
        sleep 0.2
    done
    printf '%s\n' "$$" >"$1/lock.d/pid"
}

unlock() {
    rm -rf "$1/lock.d"
}

# Fetch build $2 into folder $1, check it and install it, under the folder's lock: its path.
install() {
    local where="$1" name="$2" binary="$1/$VERSION/surreal" from scratch why got
    from="$(url "$name")"
    printf 'surreal: fetching SurrealDB %s for %s once for this machine, into %s\n' "$VERSION" "$name" "$where" >&2
    mkdir -p "$where/$VERSION" || return 1
    scratch="$(mktemp -d "$where/fetch.XXXXXX")" || return 1
    if ! why="$(fetch "$from" "$scratch/surreal.tgz")"; then
        rm -rf "$scratch"
        refuse "could not fetch $from: ${why:-the download failed}"
        return 1
    fi
    got="$(sha256 "$scratch/surreal.tgz")" || got=""
    if [ "$got" != "$(pinned "$name")" ]; then
        rm -rf "$scratch"
        refuse "$from has SHA-256 $got, not the pinned $(pinned "$name"); nothing was installed"
        return 1
    fi
    if ! tar -xzf "$scratch/surreal.tgz" -C "$scratch" surreal 2>/dev/null \
        || [ -L "$scratch/surreal" ] || [ ! -f "$scratch/surreal" ]; then
        rm -rf "$scratch"
        refuse "$from holds no \`surreal\` binary; nothing was installed"
        return 1
    fi
    chmod 755 "$scratch/surreal" && mv -f "$scratch/surreal" "$binary"
    local moved=$?
    rm -rf "$scratch"
    [ "$moved" -eq 0 ] || return 1
    printf '%s\n' "$binary"
}

# The pinned `surreal` of build $2 under folder $1, fetched and checked first if it is not there.
ensure() {
    local where="$1" binary="$1/$VERSION/surreal" done
    if [ -x "$binary" ]; then
        printf '%s\n' "$binary"
        return
    fi
    mkdir -p "$where" || return 1
    lock "$where"
    if [ -x "$binary" ]; then
        unlock "$where"
        printf '%s\n' "$binary"
        return
    fi
    install "$where" "$2"
    done=$?
    unlock "$where"
    return "$done"
}

main() {
    local workspace where name
    workspace="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
    where="$(folder "$workspace")" || return 2
    name="$(platform_name "$(uname -s)" "$(uname -m)")" || return 2
    ensure "$where" "$name" || return 2
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    main "$@"
    exit
fi
