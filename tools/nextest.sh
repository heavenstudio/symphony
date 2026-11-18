#!/usr/bin/env bash
# Only --setup downloads. Other calls verify and use this target's pinned runner, else a linked
# worktree's main checkout target. Setup provisions that main cache once, never one per worktree.
# The release's explicit nextest-setup step owns provisioning; a test only reads a verified tool.
# Sourcing defines functions for the runner fixtures without starting anything. Bash 3.2.
set -euo pipefail

version=0.9.152

# Immutable upstream release assets, checked against the GitHub release API. Binary digests are
# of cargo-nextest extracted from those verified archives, so cached copies are checked too.
archive_sha256() {
    case "$1" in
    universal-apple-darwin) echo 2fadf57df4243e691f088ee3bff4f4ab9af16da0c95a1db84b5cb1685e098a76 ;;
    aarch64-unknown-linux-gnu) echo ca1f81a5b86533eb45ab97e0064aa88e925e53aab9a698ca6331a7cad1f85f07 ;;
    x86_64-unknown-linux-gnu) echo d039b43c3e0958441b6ad0a39c8884d018b3d2e8fb9928f209074b1100b5e17a ;;
    *) return 1 ;;
    esac
}
binary_sha256() {
    case "$1" in
    universal-apple-darwin) echo d570be1e78c20e13b1bcd697e07b032c1e1f79781388193239f63c6779731423 ;;
    aarch64-unknown-linux-gnu) echo 5c0dff45b1da8d252bc1316e521e2277acb6779f19a586103ea64756c7783b51 ;;
    x86_64-unknown-linux-gnu) echo eae075c1f810a15aa3ffa31d774d2a9e3dbbbab3d0a1b8c3abad4f2cdb64c2b3 ;;
    *) return 1 ;;
    esac
}
sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    else
        shasum -a 256 "$1" | cut -d ' ' -f 1
    fi
}
platform_name() {
    case "$1:$2" in
    Darwin:arm64 | Darwin:x86_64) echo universal-apple-darwin ;;
    Linux:aarch64 | Linux:arm64) echo aarch64-unknown-linux-gnu ;;
    Linux:x86_64 | Linux:amd64) echo x86_64-unknown-linux-gnu ;;
    *) printf 'nextest: no pinned cargo-nextest %s for %s %s; use macOS or glibc Linux\n' "$version" "$1" "$2" >&2; return 2 ;;
    esac
}
matches() {
    local got
    [ -f "$1" ] && [ ! -L "$1" ] && [ -x "$1" ] || return 1
    got="$(sha256 "$1")" || return 1
    [ "$got" = "$(binary_sha256 "$platform")" ] || return 1
    got="$("$1" --version 2>/dev/null)" || return 1
    # Official prebuilt executables add their commit and date after the exact release token.
    case "${got%%$'\n'*}" in
    "cargo-nextest $version" | "cargo-nextest $version ("*")") return 0 ;;
    *) return 1 ;;
    esac
}

# Only Git's own linked-worktree metadata authorizes a main-checkout fallback. A bare common
# directory has no main checkout. A nested workspace keeps its prefix under that checkout.
main_target() {
    local where git_dir common prefix main
    [ ! -d "$root/.git" ] || return 1
    where="$(unset GIT_DIR GIT_WORK_TREE GIT_COMMON_DIR; git -C "$root" rev-parse \
        --path-format=absolute --git-dir --git-common-dir --show-prefix 2>/dev/null)" || return 1
    { IFS= read -r git_dir; IFS= read -r common; IFS= read -r prefix; } <<< "$where" || true
    [ -n "$git_dir" ] && [ "$git_dir" != "$common" ] || return 1
    main="${common%/.git}"
    [ "$main" != "$common" ] || return 1
    printf '%s/%s.target\n' "$main" "$prefix"
}

# The subshell owns and cleans only its own scratch files. A verified executable is published by
# one rename; no other caller sees the download or extraction halfway through.
install_runner() (
    local folder="$1" scratch url got
    mkdir -p "$folder/bin" || return 2
    scratch="$(mktemp -d "$folder/fetch.XXXXXX")" || return 2
    trap 'rm -f "$scratch/tool.tar.gz" "$scratch/cargo-nextest" "$scratch/fetch-error"; rmdir "$scratch"' EXIT
    url="https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-$version/cargo-nextest-$version-$platform.tar.gz"
    if ! curl -fLsS --connect-timeout 15 --max-time 120 "$url" -o "$scratch/tool.tar.gz" 2>"$scratch/fetch-error"; then
        printf 'nextest: cannot fetch %s; allow HTTPS to GitHub and retry nextest-setup\n' "$url" >&2
        return 2
    fi
    got="$(sha256 "$scratch/tool.tar.gz")" || got=''
    if [ "$got" != "$(archive_sha256 "$platform")" ]; then
        printf 'nextest: archive SHA-256 %s differs from the pin; nothing installed\n' "$got" >&2
        return 2
    fi
    if ! tar -xzf "$scratch/tool.tar.gz" -C "$scratch" cargo-nextest \
        || ! chmod 755 "$scratch/cargo-nextest" || ! matches "$scratch/cargo-nextest"; then
        printf 'nextest: archive holds no cargo-nextest %s with the pinned binary SHA-256; nothing installed\n' "$version" >&2
        return 2
    fi
    mv -f "$scratch/cargo-nextest" "$folder/bin/cargo-nextest" || return 2
)

main() {
    local root tool_target platform main_cache='' folder binary candidate
    root="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
    platform="$(platform_name "$(uname -s)" "$(uname -m)")" || return 2
    tool_target="${CARGO_TARGET_DIR:-$root/.target}"
    case "$tool_target" in /*) ;; *) tool_target="$root/$tool_target" ;; esac
    folder="$tool_target/tools/nextest/$version"
    binary="$folder/bin/cargo-nextest"
    main_cache="$(main_target)" || main_cache=''
    if ! matches "$binary" && [ -n "$main_cache" ]; then
        candidate="$main_cache/tools/nextest/$version/bin/cargo-nextest"
        if matches "$candidate"; then binary="$candidate"; fi
    fi
    case "${1:-}" in
    --setup | --check | --where) [ "$#" -eq 1 ] || return 2 ;;
    esac
    if [ "${1:-}" = --setup ] && ! matches "$binary"; then
        # A linked worktree provisions the main cache, where all its sibling worktrees can read.
        if [ -n "$main_cache" ]; then folder="$main_cache/tools/nextest/$version"; fi
        install_runner "$folder" || return 2
        binary="$folder/bin/cargo-nextest"
    fi
    if ! matches "$binary"; then
        printf 'nextest: cargo-nextest %s is missing or differs; run just nextest-setup\n' "$version" >&2
        return 2
    fi
    case "${1:-}" in
    --setup) printf 'nextest: ready cargo-nextest %s at %s (SHA-256 verified)\n' "$version" "$binary" >&2; return ;;
    --check) return ;;
    --where) printf '%s\n' "$binary"; return ;;
    esac
    export PATH="$(dirname "$binary"):$PATH"
    exec "$root/tools/cargo.sh" nextest "$@"
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then main "$@"; fi
