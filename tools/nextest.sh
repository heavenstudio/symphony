#!/usr/bin/env bash
# --setup installs the pinned runner under tools/.nextest; --check only checks it. Everything
# else runs that runner through tools/cargo.sh, preserving the recorder and the workspace limits.
# Only --setup may download. An already installed copy of exactly this version can seed it.
set -euo pipefail
root="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
version=0.9.152
folder="$root/tools/.nextest/$version"
binary="$folder/bin/cargo-nextest"

matches() {
    local got
    [ -x "$1" ] || return 1
    got="$("$1" --version 2>/dev/null)" || return 1
    [ "${got%%$'\n'*}" = "cargo-nextest $version" ]
}

if [ "${1:-}" = --setup ]; then
    if [ "$#" -ne 1 ]; then exit 2; fi
    if ! matches "$binary"; then
        installed="$(command -v cargo-nextest || true)"
        if [ -n "$installed" ] && matches "$installed"; then
            mkdir -p "$folder/bin"
            cp "$installed" "$binary"
        else
            cargo install cargo-nextest --locked --version "=$version" --root "$folder" \
                --target-dir "$folder/build"
        fi
    fi
    if ! matches "$binary"; then
        printf 'nextest: setup did not install cargo-nextest %s in %s\n' "$version" "$folder" >&2
        exit 2
    fi
    printf 'nextest: installed cargo-nextest %s in %s\n' "$version" "$folder" >&2
    exit 0
fi

if ! matches "$binary"; then
    printf 'nextest: cargo-nextest %s is missing or differs; run just nextest-setup\n' "$version" >&2
    exit 2
fi
if [ "${1:-}" = --check ]; then
    [ "$#" -eq 1 ]
    exit
fi
export PATH="$folder/bin:$PATH"
exec "$root/tools/cargo.sh" nextest "$@"
