#!/usr/bin/env bash
# COMMAND [ARGS...]: runs COMMAND (node, npm, npx) on the Node the web client pins
# (apps/symphony-web/.node-version), as rustup runs Cargo on the toolchain rust-toolchain.toml pins:
# the node on PATH when it is that version, else fnm's copy of it (`fnm exec`, installed by fnm the
# first time, which fetches it once), else it refuses, naming the version. npm is the one that Node
# ships, which the package's `engines` and `.npmrc` (`engine-strict`) hold to its version too.
set -euo pipefail
root="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
pin="$(tr -d '[:space:]' <"$root/apps/symphony-web/.node-version")"
if [ "$(node --version 2>/dev/null || true)" = "v$pin" ]; then
    exec "$@"
fi
if command -v fnm >/dev/null 2>&1; then
    if ! fnm exec --using="$pin" -- node --version >/dev/null 2>&1; then
        printf 'node: installing Node %s with fnm, once\n' "$pin" >&2
        fnm install "$pin" >&2
    fi
    exec fnm exec --using="$pin" -- "$@"
fi
printf 'node: the web client runs on Node %s (apps/symphony-web/.node-version), and PATH has %s: install it (with fnm: fnm install %s) and put it first on PATH\n' \
    "$pin" "$(node --version 2>/dev/null || echo 'no node')" "$pin" >&2
exit 2
