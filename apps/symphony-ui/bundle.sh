#!/bin/bash
# The Symphony client's web build: `dx bundle` at the pinned version, into OUT/public (index.html and
# the content-hashed files under assets/ that the server serves as they are).
#
#     cargo symphony-ui dist OUT            (`just symphony-ui-dist`, and the browser tests: bin/cargo-symphony-ui)
#
# It runs through the cargo wrapper (`sf-slots cargo`, tools/gate), which names the tree's own build
# folder in CARGO_TARGET_DIR. wasm-fresh.sh first cleans another tree's wasm32 build of this workspace's
# crates from that folder (a new tree's folder starts from a seed another tree made), which the wrapper's
# own clean never reaches.
#
# The tools are dx, and wasm-bindgen at the version Cargo.lock pins, both built by cargo into
# sf-cargo/ in the git common dir by tools.sh the first time they are missing (minutes, once per
# machine). dx fetches its other two tools (wasm-opt, esbuild) at the versions it pins into
# DX_HOME, also there.
set -euo pipefail

out="${1:?usage: bundle.sh OUT}"
case "${2:-all}" in
    all) kinds=(board tasks docs review queue panel) ;;
    board|tasks|docs|review|queue|panel) kinds=("$2") ;;
    *) echo "symphony-ui: unknown page kind $2" >&2; exit 2 ;;
esac
case "${3:-z}" in
    z|s|2|3) export CARGO_PROFILE_WASM_RELEASE_OPT_LEVEL="${3:-z}" ;;
    *) echo "symphony-ui: unknown Rust optimization level $3" >&2; exit 2 ;;
esac
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:?bundle.sh runs under the cargo wrapper, sf-slots cargo, which names the build folder}"
common="${SF_GIT_COMMON_DIR:-$(git -C "$here" rev-parse --path-format=absolute --git-common-dir)}"
dx="$common/sf-cargo/dx-0.7.10/bin/dx"
export DX_HOME="$common/sf-cargo/dx-home"
export DX_TELEMETRY_ENABLED=false

# All dx bundles in this target folder use dx/symphony-ui/release/web, even with different
# --out-dir values. Hold the descriptor through cleaning, compilation and publication;
# children inherit it, and the kernel releases it when the last holder ends.
source "$root/tools/check_lock.sh"
check_lock "$target/symphony-ui-bundle" symphony-ui "client bundle"

bindgen_version="$(awk '$0 == "name = \"wasm-bindgen\"" { getline; gsub(/"/, "", $3); print $3; exit }' "$root/Cargo.lock")"
if [ ! -x "$dx" ] || [ ! -x "$DX_HOME/tools/wasm-bindgen-$bindgen_version/wasm-bindgen" ]; then
    bash "$here/tools.sh"
fi

bash "$here/wasm-fresh.sh"

# dx copies its own output folder whole, and keeps every hashed file an earlier build left there.
# Leave the published client alone throughout compilation; package.sh replaces it only when ready.
rm -rf "$target/dx/symphony-ui/release/web"
mkdir -p "$out"
out="$(cd "$out" && pwd)" # named from here, and dx runs from the crate
cd "$here"
# Dioxus's splitter keeps shared wasm-bindgen exports in the initial module. Compile each page
# kind independently: a board visit must not pay for document and thread decoders. The chat/task
# panel has its own module, loaded on opening and kept alive across page kinds. All assets are content hashed;
# identical files from separate kinds share one URL and one browser-cache entry.
for kind in "${kinds[@]}"; do
    rm -rf "$target/dx/symphony-ui/release/web"
    module="$out/.modules/$kind"
    rm -rf "$module/public"
    features="page-$kind"
    # --debug-symbols defaults to true in dx 0.7, release or not: wasm-opt would keep the DWARF.
    "$dx" bundle --web --bin symphony-ui --release --locked --no-default-features --features "$features" \
        --debug-symbols false --out-dir "$module" >&2
done
exec bash "$here/package.sh" "$out"
