#!/usr/bin/env bash
# COMMAND [ARGS...]: runs COMMAND (node, npm, npx) on the Node the web client pins
# (apps/symphony-web/.node-version), as rustup runs Cargo on the toolchain rust-toolchain.toml pins:
# the node on PATH when it is that version, else fnm's copy of it (installed by fnm the first time,
# which fetches it once), its folder first on PATH, else it refuses, naming the version. COMMAND
# takes this process's place either way, with no fnm process between (`fnm exec` passes no SIGTERM
# on), so a signal to this script's process reaches it. npm is the one that Node ships, which the
# package's `engines` and `.npmrc` (`engine-strict`) hold to its version too.
set -euo pipefail
root="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
pin="$(tr -d '[:space:]' <"$root/apps/symphony-web/.node-version")"
if [ "$(node --version 2>/dev/null || true)" = "v$pin" ]; then
    exec "$@"
fi
if command -v fnm >/dev/null 2>&1; then
    folder='require("path").dirname(process.execPath)'
    if ! bin="$(fnm exec --using="$pin" -- node -p "$folder" 2>/dev/null)"; then
        printf 'node: installing Node %s with fnm, once\n' "$pin" >&2
        fnm install "$pin" >&2
        bin="$(fnm exec --using="$pin" -- node -p "$folder")"
    fi
    export PATH="$bin:$PATH"
    exec "$@"
fi
printf 'node: the web client runs on Node %s (apps/symphony-web/.node-version), and PATH has %s: install it (with fnm: fnm install %s) and put it first on PATH\n' \
    "$pin" "$(node --version 2>/dev/null || echo 'no node')" "$pin" >&2
exit 2
