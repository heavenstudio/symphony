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
# The minifier dx fetched to minify a module's glue, which minifies the shell.
esbuild="$(find "${DX_HOME:?package.sh runs from bundle.sh, which names DX_HOME}/tools" -path '*/esbuild-*/esbuild')"
if [ -z "$esbuild" ] || [ "$(printf '%s\n' "$esbuild" | wc -l | tr -d ' ')" != 1 ] || [ ! -x "$esbuild" ]; then
    echo "symphony-ui: expected one esbuild under $DX_HOME/tools, found: ${esbuild:-none}" >&2
    exit 2
fi
# The stylesheets every kind loads are one shared file, first in each kind's list: a first visit
# asks for it once instead of once per file, and every kind caches the same one. They are those the
# page dx built for src/main.rs links in its head, in the order of their names, each minified and
# named by its bytes by dx: the frame's, the controls' theme, the board's and the shared controls'.
# Nothing else of that build is published: its script and its wasm draw nothing.
styles_page="$out/.modules/styles/public/index.html"
if [ ! -f "$styles_page" ]; then
    echo "symphony-ui: the shared stylesheets' build is missing: $styles_page" >&2
    exit 2
fi
names="$({ grep -o '<link rel="stylesheet" href="[^"]*"' "$styles_page" || true; } | sed 's#.*/##; s#"$##' | LC_ALL=C sort)"
if [ -z "$names" ]; then
    echo "symphony-ui: the shared stylesheets' build links no stylesheet: $styles_page" >&2
    exit 2
fi
while IFS= read -r name; do
    cat "$out/.modules/styles/public/assets/$name"
    printf '\n'
done <<< "$names" > "$stage/shared.css"
shared_name="shared-$(shasum -a 256 "$stage/shared.css" | cut -c1-16).css"
mv "$stage/shared.css" "$stage/assets/$shared_name"
shared="$(jq -cn --arg href "$prefix/assets/$shared_name" '[$href]')"
# The web client's kinds, which bundle.sh built with Vite into .modules/web: each entry of Vite's
# manifest is a kind, its script the module the shell imports (it exports `run` and `stop`, and has
# no wasm), its imports the chunks that script imports at any depth, which the shell preloads beside
# it, and its styles the shared stylesheet and then the CSS its entry and the chunks it imports
# carry, theirs first. A build without the web client's is refused.
web="$out/.modules/web"
if [ ! -f "$web/.vite/manifest.json" ]; then
    echo "symphony-ui: the web client's build is missing: $web/.vite/manifest.json" >&2
    exit 2
fi
bash "$here/assets-copy.sh" "$web/assets" "$stage/assets"
while IFS= read -r entry; do
    web_kind="$(jq -r --arg entry "$entry" '.[$entry].name' "$web/.vite/manifest.json")"
    manifest="$(jq --arg kind "$web_kind" --arg entry "$entry" --arg prefix "$prefix/" --argjson shared "$shared" \
        --slurpfile vite "$web/.vite/manifest.json" '
        $vite[0] as $chunks
        | def css($key): [($chunks[$key].imports // [])[] | css(.)[]] + ($chunks[$key].css // []);
        def imports($key): ($chunks[$key].imports // [])[] | (., imports(.));
        def once(items): reduce items as $item ([]; if any(.[]; . == $item) then . else . + [$item] end);
        . + {($kind): {
            script: ($prefix + $chunks[$entry].file),
            imports: (once(imports($entry)) | map($prefix + $chunks[.].file)),
            styles: ($shared + (once(css($entry)[]) | map($prefix + .)))
        }}' <<< "$manifest")"
done < <(jq -r 'to_entries[] | select(.value.isEntry) | .key' "$web/.vite/manifest.json" | LC_ALL=C sort)
# The shell's hash includes the page module manifest: a new module changes Board.build, while
# returning to any unchanged kind still uses its immutable browser-cache entries. The manifest
# keeps its own first line, `const modules = …;`, which readers of a build look for; the rest,
# the client cache's store (`assets/cache.js`, which every page kind shares) and then the shell,
# is minified (every first visit loads it).
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
echo "symphony-ui: the web client's kinds (home, the queue, Reports, the task board, the agents page, the documents, a task's page, the chat, the menus) over one shared stylesheet, one SPA shell: $out/public" >&2
