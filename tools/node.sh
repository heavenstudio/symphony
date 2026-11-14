#!/usr/bin/env bash
# COMMAND [ARGS...]: runs COMMAND (node, npm, npx) on the Node the web client pins
# (apps/symphony-web/.node-version), as rustup runs Cargo on the toolchain rust-toolchain.toml pins:
# the node on PATH when it is that version, else fnm's copy of it (`fnm exec`, installed by fnm the
# first time, which fetches it once), else it refuses, naming the version.
#
# npm is the version the package's `engines` pins, which `.npmrc` (`engine-strict`) holds every run
# to: the one that Node ships when it is that version, else that version fetched once by the one
# Node ships (`npm pack`, which checks the file against the registry's integrity, from npm's cache
# when it holds it) into apps/symphony-web/.npm/VERSION, and run from there.
set -euo pipefail
root="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
web="$root/apps/symphony-web"
pin="$(tr -d '[:space:]' <"$web/.node-version")"
on=()
if [ "$(node --version 2>/dev/null || true)" != "v$pin" ]; then
    if ! command -v fnm >/dev/null 2>&1; then
        printf 'node: the web client runs on Node %s (apps/symphony-web/.node-version), and PATH has %s: install it (with fnm: fnm install %s) and put it first on PATH\n' \
            "$pin" "$(node --version 2>/dev/null || echo 'no node')" "$pin" >&2
        exit 2
    fi
    if ! fnm exec --using="$pin" -- node --version >/dev/null 2>&1; then
        printf 'node: installing Node %s with fnm, once\n' "$pin" >&2
        fnm install "$pin" >&2
    fi
    on=(fnm exec --using="$pin" --)
fi
if [ "${1:-}" = npm ]; then
    shift
    want="$(${on[@]+"${on[@]}"} node -p 'require(process.argv[1]).engines.npm' "$web/package.json")"
    if [ "$(${on[@]+"${on[@]}"} npm --version)" = "$want" ]; then
        set -- npm "$@"
    else
        cli="$web/.npm/$want/package/bin/npm-cli.js"
        if [ ! -f "$cli" ]; then
            printf 'node: fetching npm %s, once\n' "$want" >&2
            mkdir -p "$web/.npm"
            fresh="$(mktemp -d "$web/.npm/.fetch.XXXXXXXX")"
            # Outside the package, whose engines the npm Node ships does not meet.
            (cd "$fresh" && ${on[@]+"${on[@]}"} npm pack "npm@$want" --prefer-offline --silent >/dev/null)
            tar -xzf "$fresh/npm-$want.tgz" -C "$fresh"
            rm -f "$fresh/npm-$want.tgz"
            # Two first runs at once: the one that renames first wins, and the other's copy goes.
            [ -d "$web/.npm/$want" ] || mv "$fresh" "$web/.npm/$want"
            rm -rf "$fresh"
        fi
        set -- node "$cli" "$@"
    fi
fi
exec ${on[@]+"${on[@]}"} "$@"
