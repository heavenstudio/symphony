#!/usr/bin/env bash
# cargo-semver-checks, pinned for rust-toolchain.toml's Rust. With arguments, run its check;
# without them, print the executable's path. Sourcing defines functions without running them.
# Each worktree keeps its own tool under CARGO_TARGET_DIR (else .target), never on the owner's
# PATH. Downloads are checksum-verified before an atomic rename; concurrent callers may fetch
# the same immutable archive, but none can observe a partial executable. Bash 3.2 is supported.
set -euo pipefail

VERSION=0.51.0

# SHA-256 of the upstream release assets (GitHub release API, v0.51.0).
pinned() {
    case "$1" in
    aarch64-apple-darwin) echo 3cec5da41eee9a3336d75ad43d1f30a5c9680818f4ba209024786f452cabab5b ;;
    x86_64-apple-darwin) echo 542f9518fd5360f46c1027a6ac2c9e9ee7431518c42495346df84922a050ce5b ;;
    aarch64-unknown-linux-gnu) echo 58531fa492d237c71b97d113a6d2a36209d96105956569cd0406aa66226ac928 ;;
    x86_64-unknown-linux-gnu) echo cacadafaa5cd27ae6d05a74a103266917ebab4c5e43c898360e177d4fbf06a65 ;;
    *) return 1 ;;
    esac
}

refuse() {
    printf 'semver: %s\n' "$*" >&2
}

platform_name() {
    local machine system
    case "$1" in
    Darwin) system=apple-darwin ;;
    Linux) system=unknown-linux-gnu ;;
    *) system=unknown ;;
    esac
    case "$2" in
    arm64 | aarch64) machine=aarch64 ;;
    x86_64 | amd64) machine=x86_64 ;;
    *) machine=unknown ;;
    esac
    if ! pinned "$machine-$system" >/dev/null; then
        refuse "no cargo-semver-checks $VERSION build pinned for $1 $2; use a supported macOS or glibc Linux host (tools/semver.sh)"
        return 2
    fi
    printf '%s\n' "$machine-$system"
}

fetch() {
    curl -fLsS --connect-timeout 15 --max-time 120 "$1" -o "$2"
}

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    else
        shasum -a 256 "$1" | cut -d ' ' -f 1
    fi
}

# Fetch once into this build folder. The subshell owns and cleans only its own scratch files.
ensure() (
    local where="$1/$VERSION/$2" binary="$1/$VERSION/$2/cargo-semver-checks" scratch url got
    if [ -x "$binary" ]; then printf '%s\n' "$binary"; return; fi
    mkdir -p "$where" || return 2
    scratch="$(mktemp -d "$where/fetch.XXXXXX")" || return 2
    trap 'rm -f "$scratch/tool.tar.gz" "$scratch/cargo-semver-checks"; rmdir "$scratch"' EXIT
    url="https://github.com/obi1kenobi/cargo-semver-checks/releases/download/v$VERSION/cargo-semver-checks-$2.tar.gz"
    printf 'semver: fetching cargo-semver-checks %s into %s\n' "$VERSION" "$where" >&2
    if ! fetch "$url" "$scratch/tool.tar.gz"; then
        refuse "cannot fetch $url; allow HTTPS to GitHub and retry (or populate this project tool cache while online)"
        return 2
    fi
    got="$(sha256 "$scratch/tool.tar.gz")" || got=''
    if [ "$got" != "$(pinned "$2")" ]; then
        refuse "$url has SHA-256 $got, not the pinned $(pinned "$2"); nothing installed"
        return 2
    fi
    if ! tar -xzf "$scratch/tool.tar.gz" -C "$scratch" cargo-semver-checks \
        || [ -L "$scratch/cargo-semver-checks" ] || [ ! -f "$scratch/cargo-semver-checks" ]; then
        refuse "$url holds no cargo-semver-checks executable; nothing installed"
        return 2
    fi
    chmod 755 "$scratch/cargo-semver-checks" || return 2
    mv -f "$scratch/cargo-semver-checks" "$binary" || return 2
    printf '%s\n' "$binary"
)

main() {
    local workspace target name binary
    workspace="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
    target="${CARGO_TARGET_DIR:-$workspace/.target}"
    case "$target" in /*) ;; *) target="$PWD/$target" ;; esac
    name="$(platform_name "$(uname -s)" "$(uname -m)")" || return 2
    binary="$(ensure "$target/tools/cargo-semver-checks" "$name")" || return 2
    if [ "$#" -eq 0 ]; then printf '%s\n' "$binary"; return; fi
    exec "$binary" semver-checks "$@"
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then main "$@"; fi
