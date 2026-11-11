#!/bin/bash
# Assemble independently built page kinds into one immutable, cached SPA shell.
set -euo pipefail
out="$(cd "${1:?usage: package.sh OUT}" && pwd)"
here="$(cd "$(dirname "$0")" && pwd)"
cd "$here"
base="$(sed -n 's/^[[:space:]]*base_path[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' Dioxus.toml)"
base="${base#/}"
base="${base%/}"
prefix="${base:+/$base}"
# A SIGKILL can leave an unfinished assembly. Do not touch another live publisher's staging folder.
for stale in "$out"/.public-stage.*; do
    [ -d "$stale" ] || continue
    owner="${stale##*/.public-stage.}"
    owner="${owner%%.*}"
    case "$owner" in ''|*[!0-9]*) continue ;; esac
    if ! kill -0 "$owner" 2>/dev/null; then
        rm -rf "$stale"
    fi
done
stage="$(mktemp -d "$out/.public-stage.$$.XXXXXXXX")"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/assets"
# Open pages may still import a previous build's deferred modules after the swap.
if [ -n "${3:-}" ] && [ -d "$3/assets" ]; then
    bash "$here/assets-copy.sh" "$3/assets" "$stage/assets"
fi
if [ -d "$out/public/assets" ]; then
    bash "$here/assets-copy.sh" "$out/public/assets" "$stage/assets"
fi
manifest='{}'
for kind in board tasks docs review queue panel overlay; do
    module="$out/.modules/$kind"
    bash "$here/assets-copy.sh" "$module/public/assets" "$stage/assets"
    script="$(sed -n 's/.*<script type="module"[^>]*src="\([^"]*\)".*/\1/p' "$module/public/index.html")"
    original="$(basename "$script")"
    # dx starts the generated module itself. Expose that one promise to the shell, and make the
    # splitter's initSync reference local to this ESM instead of overwriting another kind's.
    {
        printf 'let __reviewInitSync;\n'
        sed -E -e 's/globalThis\.__wasm_split_main_initSync/__reviewInitSync/g' \
            -e "s/[[:alnum:]_\$]+\\(\\{module_or_path:\"[^\"]+\"\\}\\)\\.then\\(/globalThis.reviewModuleReady_$kind=&/" \
            "$module/public/assets/$original"
    } > "$stage/module-$kind.js"
    if [ "$(grep -o "globalThis.reviewModuleReady_$kind=" "$stage/module-$kind.js" | wc -l | tr -d ' ')" != 1 ]; then
        echo "symphony-ui: $kind has no unique dx initializer; refusing to package it" >&2
        exit 2
    fi
    script_hash="$(shasum -a 256 "$stage/module-$kind.js" | cut -c1-16)"
    script="$prefix/assets/$kind-$script_hash.js"
    mv "$stage/module-$kind.js" "$stage/assets/$kind-$script_hash.js"
    rm -f "$stage/assets/$original"
    wasm="$prefix/assets/$(find "$module/public/assets" -name 'symphony-ui_bg*.wasm' -exec basename {} \;)"
    styles="$(find "$module/public/assets" -name '*.css' -exec basename {} \; | sort | jq -R --arg prefix "$prefix/assets/" '$prefix + .' | jq -s .)"
    # The summary board has no media nodes. The persistent panel loads these styles when
    # opened; documents and the review page keep them with their visible pictures.
    if [ "$kind" = board ]; then
        styles="$(jq 'map(select(contains("/media-") | not))' <<< "$styles")"
    fi
    manifest="$(jq --arg kind "$kind" --arg script "$script" --arg wasm "$wasm" --argjson styles "$styles" '. + {($kind): {script: $script, wasm: $wasm, styles: $styles}}' <<< "$manifest")"
done
# The stylesheets every kind loads are one shared file, in their order, first in each kind's list:
# a first visit asks for it once instead of once per file, and every kind still caches the same
# one. A kind's own stylesheets follow it, the order a visit from another kind already loads them in.
shared="$(jq -c '[.[].styles] | reduce .[1:][] as $other (.[0]; map(select(. as $style | any($other[]; . == $style))))' <<< "$manifest")"
if [ "$(jq length <<< "$shared")" -gt 1 ]; then
    jq -r --arg prefix "$prefix/assets/" '.[] | ltrimstr($prefix)' <<< "$shared" | while IFS= read -r name; do
        cat "$stage/assets/$name"
        printf '\n'
    done > "$stage/shared.css"
    shared_name="shared-$(shasum -a 256 "$stage/shared.css" | cut -c1-16).css"
    mv "$stage/shared.css" "$stage/assets/$shared_name"
    manifest="$(jq --argjson shared "$shared" --arg href "$prefix/assets/$shared_name" 'map_values(.styles |= [$href] + (. - $shared))' <<< "$manifest")"
fi
# The shell's hash includes the page module manifest: a new module changes Board.build, while
# returning to any unchanged kind still uses its immutable browser-cache entries. The manifest
# keeps its own first line, `const modules = …;`, which readers of a build look for; the rest is
# minified by the esbuild dx fetched to minify each module's glue (every first visit loads it).
esbuild="$(find "${DX_HOME:?package.sh runs from bundle.sh, which names DX_HOME}/tools" -path '*/esbuild-*/esbuild')"
if [ -z "$esbuild" ] || [ "$(printf '%s\n' "$esbuild" | wc -l | tr -d ' ')" != 1 ] || [ ! -x "$esbuild" ]; then
    echo "symphony-ui: expected one esbuild under $DX_HOME/tools, found: ${esbuild:-none}" >&2
    exit 2
fi
if [ "$(grep -cx 'const modules = __MODULES__;' assets/shell.js)" != 1 ]; then
    echo "symphony-ui: assets/shell.js needs its manifest line, const modules = __MODULES__;" >&2
    exit 2
fi
{
    printf 'const modules = %s;\n' "$(jq -c . <<< "$manifest")"
    grep -vx 'const modules = __MODULES__;' assets/shell.js | "$esbuild" --minify --format=esm --log-level=warning
} > "$stage/shell.js"
# Eager notice styles are part of this shell's build identity too.
printf '\n// Notice styles: %s\n' "$(shasum -a 256 ../symphony-server/src/notices.css | cut -c1-16)" >> "$stage/shell.js"
hash="$(shasum -a 256 "$stage/shell.js" | cut -c1-16)"
shell="symphony-shell-$hash.js"
mv "$stage/shell.js" "$stage/assets/$shell"
cat > "$stage/index.html" <<HTML
<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Symphony</title><link rel="icon" href="data:,"><style id="shell-notices-style">$(cat ../symphony-server/src/notices.css)</style></head><body><div id="main"><section id="shell-notices" aria-label="Server notices"></section></div>
<script type="module" src="$prefix/assets/$shell"></script></body></html>
HTML
if [ -n "${2:-}" ]; then
    "$2" publish-client "$stage" "$out/public"
else
    "$here/../../tools/rs-bin.sh" symphony publish-client "$stage" "$out/public"
fi
echo "symphony-ui: five page-kind modules, a deferred panel and its overlay host, one SPA shell: $out/public" >&2
