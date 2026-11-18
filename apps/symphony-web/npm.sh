#!/usr/bin/env bash
# ARGS...: npm in the web client's folder on its pinned Node (tools/node.sh), once its packages are
# the lockfile's: `npm ci` first when node_modules was not installed from this very package-lock.json
# (the copy of it an install leaves as node_modules/.symphony-lock). npm's cache holds every package
# an install fetched, so an install after the first is offline (`--prefer-offline`). Two runs in one
# checkout take turns at the install (tools/check_lock.sh, the lock beside node_modules). An install
# that succeeds says nothing, so a first run's output is the command's alone, as every later run's
# is; one that fails says all it said on stderr and exits as it did, and npm is not run.
#
#     apps/symphony-web/npm.sh run build     apps/symphony-web/npm.sh run lint     … run test
set -euo pipefail
here="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
root="$(cd -P "$here/../.." && pwd -P)"
cd "$here"
if ! cmp -s package-lock.json node_modules/.symphony-lock 2>/dev/null; then
    # shellcheck source=../../tools/check_lock.sh
    source "$root/tools/check_lock.sh"
    check_lock "$here/.install" symphony-web "package install"
    if ! cmp -s package-lock.json node_modules/.symphony-lock 2>/dev/null; then
        installed=0
        said="$("$root/tools/node.sh" npm ci --prefer-offline --no-audit --no-fund --loglevel=error 2>&1)" || installed=$?
        if [ "$installed" != 0 ]; then
            printf '%s\n' "$said" >&2
            exit "$installed"
        fi
        cp package-lock.json node_modules/.symphony-lock
    fi
    exec 9>&-
fi
exec "$root/tools/node.sh" npm "$@"
