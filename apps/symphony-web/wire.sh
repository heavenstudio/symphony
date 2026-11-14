#!/usr/bin/env bash
# [--check]: the web client's wire types (src/wire.ts) and the cases its unit tests hold its
# TypeScript to (src/wire.cases.json), as crates/symphony-api's `typescript` example prints them and
# Biome formats them: written over the committed files (`just client-wire`), or, with --check,
# compared with them, failing on the first that differs (`just client-lint`).
set -euo pipefail
here="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
root="$(cd -P "$here/../.." && pwd -P)"
check=''
case "$#:${1:-}" in
    0:) ;;
    1:--check) check=1 ;;
    *)
        printf 'usage: apps/symphony-web/wire.sh [--check]\n' >&2
        exit 2
        ;;
esac
fresh="$(mktemp -d "${TMPDIR:-/tmp}/symphony-web-wire.XXXXXXXX")"
trap 'rm -rf "$fresh"' EXIT
for pair in "wire.ts:" "wire.cases.json:cases"; do
    file="${pair%%:*}"
    mode="${pair#*:}"
    # shellcheck disable=SC2086 # the example's one argument, or none
    "$root/tools/cargo.sh" run --quiet --locked -p symphony-api --features typescript \
        --example typescript -- $mode >"$fresh/raw"
    "$here/npm.sh" exec --silent -- biome format --stdin-file-path="src/$file" <"$fresh/raw" >"$fresh/$file"
    if [ -z "$check" ]; then
        cp "$fresh/$file" "$here/src/$file"
    elif ! cmp -s "$fresh/$file" "$here/src/$file"; then
        printf 'client-lint: apps/symphony-web/src/%s is not what crates/symphony-api writes: run just client-wire\n' "$file" >&2
        diff -u "$here/src/$file" "$fresh/$file" | head -40 >&2 || true
        exit 1
    fi
done
