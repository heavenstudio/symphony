#!/bin/bash
# What a page's module costs on a first visit, and the named copy that says where its bytes come from.
#
#     cargo symphony-ui size OUT NAME...        (bin/cargo-symphony-ui, under the cargo wrapper)
#
# NAME is a page kind built on its own (board, docs, review), bundled with bundle.sh's own dx
# line for that kind (the program, src/split.rs, is a main module and chunks, which the
# canonical-entry browser test reports), or a size probe, a bin in this folder (floor_dioxus, floor_websys,
# entry_websys), bundled the same way (dx, the wasm-release profile, wasm-opt -Oz, --debug-symbols
# false) with the `size-probe` feature. Each goes into OUT/NAME/. For each it prints
# the shipped WASM's and JS glue's gzip -6 bytes and writes OUT/NAME-named.wasm: cargo's linked
# module without its DWARF, then the release's wasm-bindgen and wasm-opt -Oz with the same feature
# flags, keeping the name section (`--debuginfo` in place of the release's strip). Then:
#
#     cargo run -p symphony-ui --example wasm_size -- OUT/NAME-named.wasm       shares by group and kind
#     cargo run -p symphony-ui --example wasm_size -- --sections SHIPPED.wasm   the shipped sections
#
# The named copy's first line (custom sections excluded) is within a few hundred bytes of the shipped
# module's gzip, so its attribution is of the shipped code. Tests: tests/wasm_build.rs.
set -euo pipefail

usage="usage: cargo symphony-ui size OUT NAME..."
out="${1:?$usage}"
shift
if [ "$#" -eq 0 ]; then
    echo "$usage" >&2
    exit 2
fi
here="$(cd "$(dirname "$0")/.." && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:?size.sh runs under the workspace Cargo entrypoint, which names the build folder}"
# The common dir: SYMPHONY_GIT_COMMON_DIR, else git's.
common="${SYMPHONY_GIT_COMMON_DIR:-$(git -C "$here" rev-parse --path-format=absolute --git-common-dir)}"
# The tools' folder, as tools.sh finds it: symphony/tools in the common dir.
tools="$(SYMPHONY_GIT_COMMON_DIR="$common" bash "$here/tools.sh" --folder)"
dx="$tools/dx-0.7.10/bin/dx"
export DX_HOME="$tools/dx-home"
export DX_TELEMETRY_ENABLED=false
cargo="${CARGO:-cargo}"
bindgen_version="$(awk '$0 == "name = \"wasm-bindgen\"" { getline; gsub(/"/, "", $3); print $3; exit }' "$root/Cargo.lock")"
bindgen="$DX_HOME/tools/wasm-bindgen-$bindgen_version/wasm-bindgen"

for name in "$@"; do
    case "$name" in
        board|docs|review) ;;
        *)
            if [ ! -f "$here/probes/$name.rs" ]; then
                echo "symphony-ui size: $name is neither a page kind nor a probe in apps/symphony-ui/probes/" >&2
                exit 2
            fi
            ;;
    esac
done
mkdir -p "$out"
out="$(cd "$out" && pwd)"

# The one file under DIR whose name matches PATTERN, or a refusal naming what was found.
one() {
    local found
    found="$(find "$1" -name "$2" -type f)"
    if [ -z "$found" ] || [ "$(printf '%s\n' "$found" | wc -l)" -ne 1 ]; then
        echo "symphony-ui size: expected one $2 under $1, found: ${found:-none}" >&2
        return 1
    fi
    printf '%s\n' "$found"
}

gzipped() {
    gzip -6 -n -c "$1" | wc -c | tr -d ' '
}

# The lock every dx bundle in this build folder takes (bundle.sh), held to the end.
source "$root/tools/check_lock.sh"
check_lock "$target/symphony-ui-bundle" symphony-ui "client bundle"
if [ ! -x "$dx" ] || [ ! -x "$bindgen" ]; then
    SYMPHONY_GIT_COMMON_DIR="$common" bash "$here/tools.sh"
fi
bash "$here/wasm-fresh.sh"
export CARGO_PROFILE_WASM_RELEASE_OPT_LEVEL=z

for name in "$@"; do
    case "$name" in
        board|docs|review) stem=symphony-ui features="page-$name" ;;
        *) stem="$name" features=web,size-probe ;;
    esac
    rm -rf "${out:?}/$name" "$target/dx/$stem/release/web"
    # bundle.sh's own line for a page kind, without the packaging that joins every kind.
    (cd "$here" && "$dx" bundle --web --bin "$stem" --release --locked --no-default-features \
        --features "$features" --debug-symbols false --out-dir "$out/$name" >&2)
    assets="$out/$name/public/assets"
    wasm="$(one "$assets" "${stem}_bg-*.wasm")"
    glue="$(one "$assets" "$stem-*.js")"
    opt="$(one "$DX_HOME/tools" wasm-opt)"
    linked="$target/wasm32-unknown-unknown/wasm-release/$stem.wasm"
    "$cargo" run --quiet --locked -p symphony-ui --example wasm_size -- --strip-dwarf "$linked" "$out/$name-nodwarf.wasm"
    rm -rf "$out/$name-bindgen"
    "$bindgen" "$out/$name-nodwarf.wasm" --target web --no-demangle --keep-lld-exports \
        --out-name "$stem" --out-dir "$out/$name-bindgen"
    "$opt" "$out/$name-bindgen/${stem}_bg.wasm" -Oz -o "$out/$name-named.wasm" \
        --enable-reference-types --enable-bulk-memory --enable-mutable-globals \
        --enable-nontrapping-float-to-int --enable-threads --debuginfo
    cp "$wasm" "$out/$name.wasm"
    wasm_bytes="$(gzipped "$wasm")"
    glue_bytes="$(gzipped "$glue")"
    echo "$name: WASM $wasm_bytes, JS glue $glue_bytes, together $((wasm_bytes + glue_bytes)) gzip bytes; shipped module $out/$name.wasm, named copy $out/$name-named.wasm"
done
