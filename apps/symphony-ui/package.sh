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
if [ -d "$out/public/assets" ]; then
    cp -R "$out/public/assets/." "$stage/assets/"
fi
manifest='{}'
for kind in board tasks docs review queue panel; do
    module="$out/.modules/$kind"
    cp "$module/public/assets/"* "$stage/assets/"
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
        echo "symphony-ui: $kind has no unique dx initializer; refusing to ship it" >&2
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
# The shell's hash includes the page module manifest: a new module changes Board.build, while
# returning to any unchanged kind still uses its immutable browser-cache entries.
jq -nr --arg source "$(cat assets/shell.js)" --argjson modules "$manifest" '$source | sub("__MODULES__"; ($modules | tojson))' > "$stage/shell.js"
hash="$(shasum -a 256 "$stage/shell.js" | cut -c1-16)"
shell="symphony-shell-$hash.js"
mv "$stage/shell.js" "$stage/assets/$shell"
cat > "$stage/index.html" <<HTML
<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Symphony</title><link rel="icon" href="data:,"></head><body><div id="main"></div>
<script type="module" src="$prefix/assets/$shell"></script></body></html>
HTML
"$here/../../tools/rs-bin.sh" symphony publish-client "$stage" "$out/public"
echo "symphony-ui: five page-kind modules and a deferred panel, one SPA shell: $out/public" >&2
