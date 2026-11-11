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
trap 'rm -f "$inventory"' EXIT
if ! find "$built" "$committed" -mindepth 1 -print0 > "$inventory"; then
    printf 'client-check: could not inventory the distributions\n' >&2
    exit 2
fi
checked=0
while IFS= read -r -d '' file; do
    if [ -L "$file" ] || { [ ! -f "$file" ] && [ ! -d "$file" ]; }; then
        printf 'client-check: client entries must be regular files or directories: %s\n' "$file" >&2
        exit 2
    fi
    [ -f "$file" ] || continue
    case "$file" in
        "$built"/*)
            relative="${file#"$built"/}"
            if [ ! -f "$committed/$relative" ] || ! cmp -s "$file" "$committed/$relative"; then
                printf 'client-check: rebuild and commit the client asset: %s\n' "$relative" >&2
                exit 1
            fi
            checked=$((checked + 1))
            ;;
    esac
done < "$inventory"
# Older immutable hashes may remain for pages that deferred an import across a publication.
printf 'client-check: %s freshly built files match the committed client\n' "$checked"
