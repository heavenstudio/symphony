#!/usr/bin/env bash
# The manifest step of the client's build (build.sh): OUT/.modules/web, one Vite build, made into
# one immutable, cached SPA shell and published as OUT/public.
#
#     package.sh OUT [PUBLISHER]        PUBLISHER is a built symphony-build; none given builds it
set -euo pipefail
out="$(cd "${1:?usage: package.sh OUT [PUBLISHER]}" && pwd)"
here="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
root="$(cd -P "$here/../.." && pwd -P)"
cd "$here"
# The path the server serves the client under, with no slash at its end: none, the origin's root.
prefix=""
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
web="$out/.modules/web"
vite="$web/.vite/manifest.json"
if [ ! -f "$vite" ]; then
    echo "symphony-web: the client's build is missing: $vite" >&2
    exit 2
fi
# The stylesheet every kind loads is one shared file, first in each kind's list: a first visit asks
# for it once, and every kind caches the same one. It is the build's entry for
# src/styles/shared.css, which writes its sheets' order; a build without it is refused.
shared_entry="src/styles/shared.css"
shared_file="$(jq -r --arg entry "$shared_entry" '.[$entry].file // empty' "$vite")"
if [ -z "$shared_file" ] || [ ! -s "$web/$shared_file" ]; then
    echo "symphony-web: the client's build has no shared stylesheet ($shared_entry): $vite" >&2
    exit 2
fi
shared="$(jq -cn --arg href "$prefix/$shared_file" '[$href]')"
bash "$here/assets-copy.sh" "$web/assets" "$stage/assets"
# The kinds: each other entry of Vite's manifest is a kind, its script the module the shell imports
# (it exports `run` and `stop`), its imports the chunks that script imports at any depth, which the
# shell preloads beside it, and its styles the shared stylesheet and then the CSS its entry and the
# chunks it imports carry, theirs first.
while IFS= read -r entry; do
    kind="$(jq -r --arg entry "$entry" '.[$entry].name' "$vite")"
    manifest="$(jq --arg kind "$kind" --arg entry "$entry" --arg prefix "$prefix/" --argjson shared "$shared" \
        --slurpfile vite "$vite" '
        $vite[0] as $chunks
        | def css($key): [($chunks[$key].imports // [])[] | css(.)[]] + ($chunks[$key].css // []);
        def imports($key): ($chunks[$key].imports // [])[] | (., imports(.));
        def once(items): reduce items as $item ([]; if any(.[]; . == $item) then . else . + [$item] end);
        . + {($kind): {
            script: ($prefix + $chunks[$entry].file),
            imports: (once(imports($entry)) | map($prefix + $chunks[.].file)),
            styles: ($shared + (once(css($entry)[]) | map($prefix + .)))
        }}' <<< "$manifest")"
done < <(jq -r --arg shared "$shared_entry" 'to_entries[] | select(.value.isEntry and .key != $shared) | .key' "$vite" | LC_ALL=C sort)
if [ "$manifest" = '{}' ]; then
    echo "symphony-web: the client's build has no kind: $vite" >&2
    exit 2
fi
# Notices travel with every retained build, in the same immutable asset list as its code. The
# shell names them without fetching them on a first visit; changing a notice changes its build.
bash "$here/notices.sh" "$web/.vite/licenses.json" "$stage/third-party.txt"
notice="third-party-$(shasum -a 256 "$stage/third-party.txt" | cut -c1-16).txt"
mv "$stage/third-party.txt" "$stage/assets/$notice"
# The shell's hash includes the page module manifest: a new module changes Board.build, while
# returning to any unchanged kind still uses its immutable browser-cache entries. The manifest
# keeps its own first line, `const modules = …;`, which readers of a build look for; the rest,
# the client cache's store (`shell/cache.ts`, which every page kind shares) and then the shell,
# each with its types stripped, is minified (every first visit loads it) by the minifier Vite
# ships (shell/minify.ts).
if [ "$(grep -cx 'const modules = __MODULES__;' shell/shell.ts)" != 1 ]; then
    echo "symphony-web: shell/shell.ts needs its manifest line, const modules = __MODULES__;" >&2
    exit 2
fi
grep -vx 'const modules = __MODULES__;' shell/shell.ts \
    | bash "$here/npm.sh" run --silent minify -- shell/cache.ts - > "$stage/shell.min.js"
{
    printf 'const modules = %s;\n' "$(jq -c . <<< "$manifest")"
    cat "$stage/shell.min.js"
} > "$stage/shell.js"
rm -f "$stage/shell.min.js"
# Eager notice styles are part of this shell's build identity too.
printf '\n// Third-party notices: %s/assets/%s\n' "$prefix" "$notice" >> "$stage/shell.js"
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
    "$root/tools/rs-bin.sh" symphony-build publish-client "$stage" "$out/public"
fi
kinds="$(jq -r 'keys | join(", ")' <<< "$manifest")"
echo "symphony-web: the client's kinds ($kinds) over one shared stylesheet, one SPA shell: $out/public" >&2
