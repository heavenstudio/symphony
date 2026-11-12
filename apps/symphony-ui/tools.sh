#!/bin/bash
# The Symphony client's build tools, built by cargo once per machine into sf-cargo/ in the git common
# dir (bundle.sh runs this when one is missing): dx at the pinned version, built without its
# telemetry, and wasm-bindgen at the version Cargo.lock pins, in the tool folder dx reads (DX_HOME).
# Each build takes minutes the first time and nothing after.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
# The common dir: SYMPHONY_GIT_COMMON_DIR, else its old name (said once, during the names' bridge),
# else git's.
if [ -z "${SYMPHONY_GIT_COMMON_DIR:-}" ] && [ -n "${SF_GIT_COMMON_DIR:-}" ]; then
    echo "SF_GIT_COMMON_DIR is the old name of SYMPHONY_GIT_COMMON_DIR and will stop being read: set SYMPHONY_GIT_COMMON_DIR instead" >&2
    export SYMPHONY_GIT_COMMON_DIR="$SF_GIT_COMMON_DIR"
fi
common="${SYMPHONY_GIT_COMMON_DIR:-$(git -C "$here" rev-parse --path-format=absolute --git-common-dir)}"
folder="$common/sf-cargo"
dx_version=0.7.10

if [ ! -x "$folder/dx-$dx_version/bin/dx" ]; then
    echo "symphony-ui tools: building dx $dx_version into $folder (once per machine)" >&2
    env -u CARGO_TARGET_DIR cargo install dioxus-cli --version "=$dx_version" --locked \
        --features disable-telemetry --root "$folder/dx-$dx_version"
fi

bindgen_version="$(awk '$0 == "name = \"wasm-bindgen\"" { getline; gsub(/"/, "", $3); print $3; exit }' "$root/Cargo.lock")"
dest="$folder/dx-home/tools/wasm-bindgen-$bindgen_version"
if [ ! -x "$dest/wasm-bindgen" ]; then
    echo "symphony-ui tools: building wasm-bindgen $bindgen_version into $dest (once per version)" >&2
    scratch="$(mktemp -d "${TMPDIR:-/tmp}/symphony-ui-tools.XXXXXX")"
    trap 'rm -rf "$scratch"' EXIT
    env -u CARGO_TARGET_DIR cargo install wasm-bindgen-cli --version "=$bindgen_version" --locked \
        --bin wasm-bindgen --root "$scratch"
    mkdir -p "$dest"
    cp "$scratch/bin/wasm-bindgen" "$dest/wasm-bindgen"
fi
