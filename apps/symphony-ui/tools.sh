#!/bin/bash
# The Symphony client's build tools, built by cargo once per machine into symphony/tools/ in the git
# common dir: dx at the pinned version, built without its telemetry, and wasm-bindgen at the version
# Cargo.lock pins, in the tool folder dx reads (DX_HOME). Each build takes minutes the first time and
# nothing after.
#
#     tools.sh            installs what is missing (bundle.sh runs it when a tool is, dev.sh always)
#     tools.sh --folder   prints the folder a build runs them from, and installs nothing
#
# Until tools/move-state.sh moves this repository's state, the copies an older build installed stand
# in sf-cargo/, where every build of the old code runs them. A build of this code runs them there
# too and installs nothing beside them: what they lack it refuses, naming the move. Nothing here
# removes them, since another worktree's build may be running them; only the move takes them away.
set -euo pipefail

case "${1:-}" in
    "" | --folder) ;;
    *) echo "usage: tools.sh [--folder]" >&2; exit 2 ;;
esac
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
# The common dir: SYMPHONY_GIT_COMMON_DIR, else git's.
common="${SYMPHONY_GIT_COMMON_DIR:-$(git -C "$here" rev-parse --path-format=absolute --git-common-dir)}"
folder="$common/symphony/tools"
older="$common/sf-cargo"
dx_version=0.7.10
bindgen_version="$(awk '$0 == "name = \"wasm-bindgen\"" { getline; gsub(/"/, "", $3); print $3; exit }' "$root/Cargo.lock")"

# The first tool FOLDER lacks, as a build runs it; nothing when it holds both.
lacks() {
    if [ ! -x "$1/dx-$dx_version/bin/dx" ]; then
        echo "dx $dx_version"
    elif [ ! -x "$1/dx-home/tools/wasm-bindgen-$bindgen_version/wasm-bindgen" ]; then
        echo "wasm-bindgen $bindgen_version"
    fi
}
# Whether an older build's copies stand where it keeps them: this repository's state has not moved.
unmoved=no
for copy in "$older/dx-$dx_version" "$older/dx-home"; do
    if [ -e "$copy" ] || [ -L "$copy" ]; then unmoved=yes; fi
done
if [ "$unmoved" = yes ] && [ -n "$(lacks "$folder")" ] && [ -z "$(lacks "$older")" ]; then
    folder="$older"
fi

if [ "${1:-}" = --folder ]; then
    printf '%s\n' "$folder"
    exit 0
fi
[ -n "$(lacks "$folder")" ] || exit 0
if [ "$unmoved" = yes ]; then
    echo "symphony-ui tools: $older holds an older build's copies of the client's build tools," \
        "without $(lacks "$older"), and a build of this code installs none beside them: move" \
        "this repository's state (tools/move-state.sh, while nothing builds) and build again" >&2
    exit 1
fi

if [ ! -x "$folder/dx-$dx_version/bin/dx" ]; then
    echo "symphony-ui tools: building dx $dx_version into $folder (once per machine)" >&2
    env -u CARGO_TARGET_DIR cargo install dioxus-cli --version "=$dx_version" --locked \
        --features disable-telemetry --root "$folder/dx-$dx_version" >&2
fi

dest="$folder/dx-home/tools/wasm-bindgen-$bindgen_version"
if [ ! -x "$dest/wasm-bindgen" ]; then
    echo "symphony-ui tools: building wasm-bindgen $bindgen_version into $dest (once per version)" >&2
    scratch="$(mktemp -d "${TMPDIR:-/tmp}/symphony-ui-tools.XXXXXX")"
    trap 'rm -rf "$scratch"' EXIT
    env -u CARGO_TARGET_DIR cargo install wasm-bindgen-cli --version "=$bindgen_version" --locked \
        --bin wasm-bindgen --root "$scratch" >&2
    mkdir -p "$dest"
    cp "$scratch/bin/wasm-bindgen" "$dest/wasm-bindgen"
fi
