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
# The stage holds this build alone. Open pages may still import an earlier build's modules after
# the swap: publish-client keeps the builds published just before it over the folder, as that
# folder's builds.json names them (crates/symphony/src/cli/publish.rs).
manifest='{}'
# The minifier dx fetched to minify each module's glue: the program's chunk loader and the shell go
# through it too.
esbuild="$(find "${DX_HOME:?package.sh runs from bundle.sh, which names DX_HOME}/tools" -path '*/esbuild-*/esbuild')"
if [ -z "$esbuild" ] || [ "$(printf '%s\n' "$esbuild" | wc -l | tr -d ' ')" != 1 ] || [ ! -x "$esbuild" ]; then
    echo "symphony-ui: expected one esbuild under $DX_HOME/tools, found: ${esbuild:-none}" >&2
    exit 2
fi
# The program's kinds (src/split.rs): one script and main module for all of them, and a chunk each.
program_kinds=(tasks panel overlay timeline)
# Its chunks: the kinds' and the media layer's, which no kind is (the shell's manifest never names
# it) and a kind's page loads once it shows media.
program_chunks=("${program_kinds[@]}" media)
# Whether the wasm module $1 holds custom sections alone (a name and its bytes, never code, data or
# a table entry): past its 8-byte header, each section is its id, its size in LEB128 and its bytes.
custom_only() {
    local bytes at=8 size bits byte
    [ "$(($(wc -c < "$1")))" -le 4096 ] || return 1
    read -r -a bytes <<< "$(od -An -v -tu1 "$1" | tr -s ' \n' '  ')"
    [ "${bytes[*]:0:4}" = "0 97 115 109" ] || return 1
    while [ "$at" -lt "${#bytes[@]}" ]; do
        [ "${bytes[at]}" = 0 ] || return 1
        at=$((at + 1))
        size=0
        bits=0
        while :; do
            [ "$at" -lt "${#bytes[@]}" ] || return 1
            byte="${bytes[at]}"
            at=$((at + 1))
            size=$((size | (byte & 127) << bits))
            bits=$((bits + 7))
            [ "$byte" -ge 128 ] || break
        done
        at=$((at + size))
    done
    [ "$at" = "${#bytes[@]}" ]
}
# The program last: its styles are placed by what the other kinds load. The web client's kinds
# (apps/symphony-web) are placed after the shared stylesheet, below.
for module_kind in board docs program; do
    module="$out/.modules/$module_kind"
    # dx's splitter always writes a chunk the kinds would share, `chunk_0_split`, and never fills it
    # (wasm-split-cli 0.7.10's `build_split_chunks`): no kind loads it (chunks.js loads a kind's own
    # chunk alone), so it is not published. One that holds anything but custom sections fails the
    # build, as the kinds would then miss what it holds.
    if [ "$module_kind" = program ]; then
        for split in "$module/public/assets"/chunk_*_split-*.wasm; do
            [ -e "$split" ] || continue
            if ! custom_only "$split"; then
                echo "symphony-ui: the program's shared chunk $(basename "$split") is not empty, and no kind loads it; refusing to package it" >&2
                exit 2
            fi
            rm -f "$split"
        done
    fi
    bash "$here/assets-copy.sh" "$module/public/assets" "$stage/assets"
    script="$(sed -n 's/.*<script type="module"[^>]*src="\([^"]*\)".*/\1/p' "$module/public/index.html")"
    original="$(basename "$script")"
    wasm="$prefix/assets/$(find "$module/public/assets" -name 'symphony-ui_bg*.wasm' -exec basename {} \;)"
    ready="globalThis.reviewModuleReady_$module_kind="
    if [ "$module_kind" = program ]; then
        ready=""
        chunks='{}'
        for kind in "${program_kinds[@]}"; do
            ready="${ready}globalThis.reviewModuleReady_$kind="
        done
        for kind in "${program_chunks[@]}"; do
            chunk="$(find "$module/public/assets" -name "module_*_$kind-*.wasm" -exec basename {} \;)"
            if [ -z "$chunk" ] || [ "$(printf '%s\n' "$chunk" | wc -l | tr -d ' ')" != 1 ]; then
                echo "symphony-ui: expected one chunk of $kind in the program, found: ${chunk:-none}" >&2
                exit 2
            fi
            chunks="$(jq -c --arg kind "$kind" --arg chunk "$prefix/assets/$chunk" '. + {($kind): $chunk}' <<< "$chunks")"
        done
    fi
    # dx starts the generated module itself. Expose that one promise to the shell (to each of the
    # program's kinds), and make the splitter's initSync reference local to this ESM instead of
    # overwriting another kind's. The program's chunk loader follows, with its chunks' names.
    {
        printf 'let __reviewInitSync;\n'
        sed -E -e 's/globalThis\.__wasm_split_main_initSync/__reviewInitSync/g' \
            -e "s/[[:alnum:]_\$]+\\(\\{module_or_path:\"[^\"]+\"\\}\\)\\.then\\(/$ready&/" \
            "$module/public/assets/$original"
        if [ "$module_kind" = program ]; then
            printf '\n'
            sed "s#__CHUNKS__#$chunks#" assets/chunks.js | "$esbuild" --minify --format=esm --log-level=warning
        fi
    } > "$stage/module-$module_kind.js"
    if [ "$(grep -F -o "$ready" "$stage/module-$module_kind.js" | wc -l | tr -d ' ')" != 1 ]; then
        echo "symphony-ui: $module_kind has no unique dx initializer; refusing to package it" >&2
        exit 2
    fi
    script_hash="$(shasum -a 256 "$stage/module-$module_kind.js" | cut -c1-16)"
    script="$prefix/assets/$module_kind-$script_hash.js"
    mv "$stage/module-$module_kind.js" "$stage/assets/$module_kind-$script_hash.js"
    rm -f "$stage/assets/$original"
    styles="$(find "$module/public/assets" -name '*.css' -exec basename {} \; | sort | jq -R --arg prefix "$prefix/assets/" '$prefix + .' | jq -s .)"
    # The summary board has no media nodes. The persistent panel loads these styles when
    # opened; documents keep them with their visible pictures, and home has its own copy
    # (apps/symphony-web, its entry's CSS).
    if [ "$module_kind" = board ]; then
        styles="$(jq 'map(select(contains("/media-") | not))' <<< "$styles")"
    fi
    if [ "$module_kind" != program ]; then
        manifest="$(jq --arg kind "$module_kind" --arg script "$script" --arg wasm "$wasm" --argjson styles "$styles" '. + {($kind): {script: $script, wasm: $wasm, styles: $styles}}' <<< "$manifest")"
        continue
    fi
    # Each program kind's styles, of the program's: those another kind loads too (the frame's, the
    # pictures'), then its own: the timeline's by its name, the shared controls' (`style-`,
    # components/) for the chat and the menus. A style none of these places is refused.
    others="$(jq -c '[.[].styles[]] | unique' <<< "$manifest")"
    for kind in "${program_kinds[@]}"; do
        own="$(jq -c --arg kind "$kind" --argjson others "$others" '
            (({timeline: ["task-timeline-"], panel: ["style-"], overlay: ["style-"]})[$kind] // []) as $names
            | map(select(. as $style | ($others | index($style)) != null
                or any($names[]; . as $name | $style | split("/") | last | startswith($name))))' <<< "$styles")"
        manifest="$(jq --arg kind "$kind" --arg script "$script" --arg wasm "$wasm" --argjson styles "$own" --arg chunk "$(jq -r --arg kind "$kind" '.[$kind]' <<< "$chunks")" '. + {($kind): {script: $script, wasm: $wasm, styles: $styles, chunk: $chunk}}' <<< "$manifest")"
    done
    unplaced="$(jq -r --argjson styles "$styles" '$styles - [.[].styles[]] | .[]' <<< "$manifest")"
    if [ -n "$unplaced" ]; then
        echo "symphony-ui: no program kind takes these styles, refusing to package them: $unplaced" >&2
        exit 2
    fi
done
# The stylesheets every kind loads are one shared file, in their order, first in each kind's list:
# a first visit asks for it once instead of once per file, and every kind still caches the same
# one. A kind's own stylesheets follow it, the order a visit from another kind already loads them in.
# The task timeline's module only ever draws inside a task page, whose styles are in already: the
# shared file is the pages' (its own few styles do not narrow it), and it names that file too, so
# it loads only what the page has not.
shared="$(jq -c '[to_entries[] | select(.key != "timeline") | .value.styles] | reduce .[1:][] as $other (.[0]; map(select(. as $style | any($other[]; . == $style))))' <<< "$manifest")"
if [ "$(jq length <<< "$shared")" -gt 1 ]; then
    jq -r --arg prefix "$prefix/assets/" '.[] | ltrimstr($prefix)' <<< "$shared" | while IFS= read -r name; do
        cat "$stage/assets/$name"
        printf '\n'
    done > "$stage/shared.css"
    shared_name="shared-$(shasum -a 256 "$stage/shared.css" | cut -c1-16).css"
    mv "$stage/shared.css" "$stage/assets/$shared_name"
    manifest="$(jq --argjson shared "$shared" --arg href "$prefix/assets/$shared_name" 'map_values(.styles |= [$href] + (. - $shared))' <<< "$manifest")"
    shared="$(jq -cn --arg href "$prefix/assets/$shared_name" '[$href]')"
fi
# The web client's kinds, which bundle.sh built with Vite into .modules/web: each entry of Vite's
# manifest is a kind, its script the module the shell imports (it exports `run` and `stop`, and has
# no wasm), its styles the shared stylesheet and then the CSS its entry and the chunks it imports
# carry, theirs first. A kind is one client's: one both build is refused, and so is a build
# without the web client's.
web="$out/.modules/web"
if [ ! -f "$web/.vite/manifest.json" ]; then
    echo "symphony-ui: the web client's build is missing: $web/.vite/manifest.json" >&2
    exit 2
fi
bash "$here/assets-copy.sh" "$web/assets" "$stage/assets"
while IFS= read -r entry; do
    web_kind="$(jq -r --arg entry "$entry" '.[$entry].name' "$web/.vite/manifest.json")"
    if jq -e --arg kind "$web_kind" 'has($kind)' <<< "$manifest" > /dev/null; then
        echo "symphony-ui: $web_kind is a page kind of both clients; refusing to package it" >&2
        exit 2
    fi
    manifest="$(jq --arg kind "$web_kind" --arg entry "$entry" --arg prefix "$prefix/" --argjson shared "$shared" \
        --slurpfile vite "$web/.vite/manifest.json" '
        $vite[0] as $chunks
        | def css($key): [($chunks[$key].imports // [])[] | css(.)[]] + ($chunks[$key].css // []);
        . + {($kind): {
            script: ($prefix + $chunks[$entry].file),
            styles: ($shared + (reduce css($entry)[] as $style ([]; if any(.[]; . == $style) then . else . + [$style] end) | map($prefix + .)))
        }}' <<< "$manifest")"
done < <(jq -r 'to_entries[] | select(.value.isEntry) | .key' "$web/.vite/manifest.json" | LC_ALL=C sort)
# The shell's hash includes the page module manifest: a new module changes Board.build, while
# returning to any unchanged kind still uses its immutable browser-cache entries. The manifest
# keeps its own first line, `const modules = …;`, which readers of a build look for; the rest,
# the client cache's store (`assets/cache.js`, which every page kind shares) and then the shell,
# is minified by the esbuild dx fetched to minify each module's glue (every first visit loads it).
if [ "$(grep -cx 'const modules = __MODULES__;' assets/shell.js)" != 1 ]; then
    echo "symphony-ui: assets/shell.js needs its manifest line, const modules = __MODULES__;" >&2
    exit 2
fi
{
    printf 'const modules = %s;\n' "$(jq -c . <<< "$manifest")"
    {
        cat assets/cache.js
        grep -vx 'const modules = __MODULES__;' assets/shell.js
    } | "$esbuild" --minify --format=esm --log-level=warning
} > "$stage/shell.js"
# Eager notice styles are part of this shell's build identity too.
printf '\n// Notice styles: %s\n' "$(shasum -a 256 ../symphony-server/src/notices.css | cut -c1-16)" >> "$stage/shell.js"
hash="$(shasum -a 256 "$stage/shell.js" | cut -c1-16)"
shell="symphony-shell-$hash.js"
mv "$stage/shell.js" "$stage/assets/$shell"
# The build's files, every one the stage holds, named by its shell: the build its page loads.
find "$stage/assets" -mindepth 1 -maxdepth 1 -type f -exec basename {} \; | LC_ALL=C sort \
    | jq -R . | jq -s --arg shell "$shell" '{builds: [{shell: $shell, files: .}]}' > "$stage/builds.json"
cat > "$stage/index.html" <<HTML
<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Symphony</title><link rel="icon" href="data:,"><style id="shell-notices-style">$(cat ../symphony-server/src/notices.css)</style></head><body><div id="main"><section id="shell-notices" aria-label="Server notices"></section></div>
<script type="module" src="$prefix/assets/$shell"></script></body></html>
HTML
if [ -n "${2:-}" ]; then
    "$2" publish-client "$stage" "$out/public"
else
    "$here/../../tools/rs-bin.sh" symphony-build publish-client "$stage" "$out/public"
fi
echo "symphony-ui: the board and the documents as modules of their own, the web client's kinds (home, the queue, Reports) as its own, the program of the task pages, the chat, the menus and the task timeline as a main module and a chunk each, with the media layer's, one SPA shell: $out/public" >&2
