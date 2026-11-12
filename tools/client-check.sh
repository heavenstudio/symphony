#!/usr/bin/env bash
# Compare a fresh real build with the assets committed for this server pin.
set -euo pipefail
root="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
built="${1:?usage: client-check.sh BUILT_PUBLIC [COMMITTED_PUBLIC]}"
committed="${2:-$root/apps/symphony-server/client/public}"
for directory in "$built" "$committed"; do
    if [ -L "$directory" ] || [ ! -d "$directory" ] || [ ! -f "$directory/index.html" ]; then
        printf 'client-check: missing regular client distribution: %s\n' "$directory" >&2
        exit 2
    fi
done
built="$(cd -P "$built" && pwd -P)"
committed="$(cd -P "$committed" && pwd -P)"
if [[ "$built" = "$committed" || "$built" = "$committed/"* || "$committed" = "$built/"* ]]; then
    printf 'client-check: build output and committed client must be separate directories\n' >&2
    exit 2
fi
inventory="$(mktemp "${TMPDIR:-/tmp}/symphony-client-check.XXXXXXXX")"
listed="$(mktemp "${TMPDIR:-/tmp}/symphony-client-check.XXXXXXXX")"
present="$(mktemp "${TMPDIR:-/tmp}/symphony-client-check.XXXXXXXX")"
trap 'rm -f "$inventory" "$listed" "$present"' EXIT
if ! find "$built" "$committed" -mindepth 1 -print0 > "$inventory"; then
    printf 'client-check: could not inventory the distributions\n' >&2
    exit 2
fi
while IFS= read -r -d '' file; do
    if [ -L "$file" ] || { [ ! -f "$file" ] && [ ! -d "$file" ]; }; then
        printf 'client-check: client entries must be regular files or directories: %s\n' "$file" >&2
        exit 2
    fi
done < "$inventory"
# A published folder's builds.json names its builds, newest first, and the bound it was published
# under (keep: KEEP in crates/symphony/src/cli/publish.rs). The fresh build is the built folder's
# newest, whatever older builds that folder keeps from an earlier check.
shape='.builds | type == "array" and length > 0
    and all(.[]; (.shell | type == "string") and (.files | type == "array") and all(.files[]; type == "string"))'
if ! keep="$(jq -er "select($shape) | .keep | select(type == \"number\" and . > 0)" "$built/builds.json" 2>/dev/null)"; then
    printf 'client-check: the fresh build names no build or bound in its builds.json\n' >&2
    exit 2
fi
checked=0
while IFS= read -r relative; do
    if [ ! -f "$committed/$relative" ] || ! cmp -s "$built/$relative" "$committed/$relative"; then
        printf 'client-check: rebuild and commit the client asset: %s\n' "$relative" >&2
        exit 1
    fi
    checked=$((checked + 1))
done < <(printf 'index.html\n'; jq -r '.builds[0].files[] | "assets/\(.)"' "$built/builds.json")
# The committed client keeps the fresh build first, then older builds a page open on one of them
# may still import from, as many in all as the bound, and no file none of them names.
if ! jq -e --slurpfile fresh "$built/builds.json" "($shape) and .builds[0] == \$fresh[0].builds[0]" \
    "$committed/builds.json" > /dev/null 2>&1; then
    printf 'client-check: rebuild and commit the client asset: builds.json\n' >&2
    exit 1
fi
kept="$(jq -r '.builds | length' "$committed/builds.json")"
if [ "$kept" -gt "$keep" ]; then
    printf 'client-check: the committed client keeps %s builds, more than the %s it is published with: run just client-update\n' "$kept" "$keep" >&2
    exit 1
fi
jq -r '.builds[].files[]' "$committed/builds.json" | LC_ALL=C sort -u > "$listed"
if [ -d "$committed/assets" ]; then
    (cd "$committed/assets" && find . -type f | sed 's#^\./##') | LC_ALL=C sort > "$present"
fi
stray="$(LC_ALL=C comm -13 "$listed" "$present" | sed -n 1p)"
if [ -n "$stray" ]; then
    printf 'client-check: no build the committed client keeps names assets/%s: run just client-update\n' "$stray" >&2
    exit 1
fi
lost="$(LC_ALL=C comm -23 "$listed" "$present" | sed -n 1p)"
if [ -n "$lost" ]; then
    printf 'client-check: the committed client lacks assets/%s, which a build it keeps names: run just client-update\n' "$lost" >&2
    exit 1
fi
printf 'client-check: %s freshly built files match the committed client, which keeps %s builds\n' "$checked" "$kept"
