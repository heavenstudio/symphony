#!/bin/bash
# The Symphony client's web build: `dx bundle` at the pinned version for the Dioxus client's kinds,
# and Vite for the web client's (apps/symphony-web, its pinned Node and its lockfile's packages),
# into OUT/public (index.html and the content-hashed files under assets/ that the server serves as
# they are).
#
#     cargo symphony-ui dist OUT            (`just symphony-ui-dist`, and the browser tests: bin/cargo-symphony-ui)
#
# It runs through the workspace Cargo entrypoint, which names the tree's own build
# folder in CARGO_TARGET_DIR. wasm-fresh.sh first cleans another tree's wasm32 build of this workspace's
# crates from that folder (a new tree's folder starts from a seed another tree made), which the wrapper's
# own clean never reaches.
#
# The tools are dx, and wasm-bindgen at the version Cargo.lock pins, both built by cargo into
# symphony/tools/ in the git common dir by tools.sh the first time they are missing (minutes, once per
# machine). dx fetches its other two tools (wasm-opt, esbuild) at the versions it pins into DX_HOME, also
# there. dx itself runs from /private/tmp/symphony-client-tools (dx-path.sh), the same path wherever
# it was installed.
set -euo pipefail

out="${1:?usage: bundle.sh OUT}"
case "${2:-all}" in
    all) kinds=(docs review program web) ;;
    docs|review|program|web) kinds=("$2") ;;
    *) echo "symphony-ui: unknown page kind $2" >&2; exit 2 ;;
esac
case "${3:-z}" in
    z|s|2|3) export CARGO_PROFILE_WASM_RELEASE_OPT_LEVEL="${3:-z}" ;;
    *) echo "symphony-ui: unknown Rust optimization level $3" >&2; exit 2 ;;
esac
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
owned_git() {
    env -u GIT_DIR -u GIT_WORK_TREE -u GIT_COMMON_DIR -u GIT_INDEX_FILE git "$@"
}
if [ ! -f "$root/.git" ] && [ ! -d "$root/.git" ]; then
    printf 'symphony-ui: release build requires its own repository: %s\n' "$root" >&2
    exit 2
fi
if ! top="$(owned_git -C "$root" rev-parse --show-toplevel)" || [ "$(cd -P "$top" && pwd -P)" != "$(cd -P "$root" && pwd -P)" ]; then
    printf 'symphony-ui: release repository does not belong to %s\n' "$root" >&2
    exit 2
fi
target="${CARGO_TARGET_DIR:?bundle.sh runs under the workspace Cargo entrypoint, which names the build folder}"
# The common dir: SYMPHONY_GIT_COMMON_DIR, else git's.
common="${SYMPHONY_GIT_COMMON_DIR:-$(git -C "$here" rev-parse --path-format=absolute --git-common-dir)}"
# The tools' folder, as tools.sh finds it: symphony/tools in the common dir.
tools="$(SYMPHONY_GIT_COMMON_DIR="$common" bash "$here/tools.sh" --folder)"
dx="$tools/dx-0.7.10/bin/dx"
export DX_HOME="$tools/dx-home"
export DX_TELEMETRY_ENABLED=false

# All dx bundles in this target folder use dx/symphony-ui/release/web, even with different
# --out-dir values. Hold the descriptor through cleaning, compilation and publication;
# children inherit it, and the kernel releases it when the last holder ends.
source "$root/tools/check_lock.sh"
check_lock "$target/symphony-ui-bundle" symphony-ui "client bundle"

bindgen_version="$(awk '$0 == "name = \"wasm-bindgen\"" { getline; gsub(/"/, "", $3); print $3; exit }' "$root/Cargo.lock")"
if [ ! -x "$dx" ] || [ ! -x "$DX_HOME/tools/wasm-bindgen-$bindgen_version/wasm-bindgen" ]; then
    SYMPHONY_GIT_COMMON_DIR="$common" bash "$here/tools.sh"
fi
# dx's own path is in every workspace crate's Cargo hash (it is their RUSTC_WORKSPACE_WRAPPER), and
# so in every byte of the client: run it from one path in every checkout and common dir.
dx="$(bash "$here/dx-path.sh" "$tools" dx-0.7.10/bin/dx)"

bash "$here/wasm-fresh.sh"

# Manganis scopes CSS and serializes asset metadata using canonical source paths.
# Build from the same verified paths on every host; the public lock and inputs stay intact.
# The checkout's listing goes in whole; release-inputs keeps what the client's build reads,
# and names the snapshot by those bytes alone.
inputs="$(mktemp -d "${TMPDIR:-/tmp}/symphony-release-inputs.XXXXXXXX")"
trap 'rm -rf "$inputs"' EXIT
"$root/tools/cargo.sh" metadata --format-version 1 --locked \
    --filter-platform wasm32-unknown-unknown > "$inputs/metadata.json"
owned_git -C "$root" ls-files --cached --others --exclude-standard -z > "$inputs/source"
components="$(jq -er '.packages[] | select(.name == "dioxus-primitives") | .manifest_path' "$inputs/metadata.json")"
components="$(cd "$(dirname "$components")/.." && pwd -P)"
component_pin="$(jq -er '.packages[] | select(.name == "dioxus-primitives") | .source' "$inputs/metadata.json")"
if [ "$(owned_git -C "$components" rev-parse HEAD)" != "${component_pin##*#}" ]; then
    printf 'symphony-ui: component checkout does not match its locked commit\n' >&2
    exit 2
fi
owned_git -C "$components" ls-files --cached -z > "$inputs/components"
snapshot_args=("$root" "$inputs/metadata.json" "$inputs/source" "$inputs/components")
publisher="$("$root/tools/rs-bin.sh" --build symphony-build)"
lock_folder="$("$publisher" release-inputs "${snapshot_args[@]}" --key)"
snapshot="$(
    # The parent retains fd 9 for the target while this subshell takes the input lock.
    exec 9>&-
    check_lock "$lock_folder" symphony-ui "release inputs"
    "$publisher" release-inputs "${snapshot_args[@]}" "$lock_folder"
)"
export SYMPHONY_GIT_COMMON_DIR="$common"
# The web client builds from the checkout: the snapshot is the Rust build's.
checkout="$root"
root="$snapshot"
here="$root/apps/symphony-ui"
# Rust diagnostics in dependencies must not encode a developer's registry or checkout path.
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
cargo_home="$(cd "$cargo_home" && pwd -P)"
export SYMPHONY_RELEASE_CARGO_HOME="$cargo_home"
export SYMPHONY_RELEASE_SOURCE="$root"
export SYMPHONY_PARENT_RUSTC_WRAPPER="${RUSTC_WRAPPER:-}"
export RUSTC_WRAPPER="$here/remap-rustc.sh"
# A wrapper is not Cargo's dependency fingerprint. Keep its dependency products separate
# from builds that did not remap paths; all output stays under the caller's held target.
wrapper_hash="$(shasum -a 256 "$RUSTC_WRAPPER" | cut -c1-16)"
target="$target/symphony-client-$wrapper_hash"
export CARGO_TARGET_DIR="$target"
bash "$here/wasm-fresh.sh" release-snapshot
rm -rf "$inputs"
trap - EXIT

# dx copies its own output folder whole, and keeps every hashed file an earlier build left there.
# Leave the published client alone throughout compilation; package.sh replaces it only when ready.
rm -rf "$target/dx/symphony-ui/release/web"
mkdir -p "$out"
out="$(cd "$out" && pwd)" # named from here, and dx runs from the crate
cd "$here"
# Home and the documents are each compiled on their own: a first visit of home must not pay
# for the task pages' code, which a shared main module would hold
# (docs/analysis/client-shared-runtime.md). The program, the task pages with the chat, the menus
# and the task timeline, is one program that dx's splitter cuts into a main module and a chunk for
# each (src/split.rs), and `symphony-build split-repair` makes that output safe and repeatable, from
# the program dx linked, before anything is packaged; a failed repair fails the build. The web
# client's kinds (the queue, Reports, the task board) are one Vite build, `web`. All assets are
# content hashed; identical files from separate kinds share one URL and one browser-cache entry.
for kind in "${kinds[@]}"; do
    module="$out/.modules/$kind"
    # The web client's kinds, one Vite build, which no path or clock enters.
    if [ "$kind" = web ]; then
        bash "$checkout/apps/symphony-web/npm.sh" run --silent build -- --outDir "$module" --emptyOutDir >&2
        continue
    fi
    rm -rf "$target/dx/symphony-ui/release/web"
    rm -rf "$module/public"
    # --debug-symbols defaults to true in dx 0.7, release or not: wasm-opt would keep the DWARF.
    flags=(--features "page-$kind" --debug-symbols false)
    if [ "$kind" = program ]; then
        flags+=(--wasm-split)
    fi
    "$dx" bundle --web --bin symphony-ui --release --locked --no-default-features "${flags[@]}" \
        --out-dir "$module" >&2
    if [ "$kind" = program ]; then
        "$publisher" split-repair "$target/wasm32-unknown-unknown/wasm-release/symphony-ui.wasm" \
            "$module/public" >&2
    fi
done
exec bash "$here/package.sh" "$out" "$publisher"
