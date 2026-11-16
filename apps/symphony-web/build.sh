#!/usr/bin/env bash
# The Symphony client's build: one Vite build of this package (its pinned Node and its lockfile's
# packages, npm.sh), then package.sh, which writes the shell and publishes the build into
# OUT/public (index.html, builds.json and the content-hashed files under assets/ that the server
# serves as they are).
#
#     apps/symphony-web/build.sh OUT        (`just symphony-ui-dist`, and the browser tests)
#
# It builds from the checkout it stands in, and no path, clock or environment enters the bytes.
# Two builds into one OUT take turns (tools/check_lock.sh, the lock in OUT/.build): Vite empties
# OUT/.modules/web before it writes. The published folder is left as it was until package.sh has a
# whole build to put in its place, so a build that fails publishes nothing.
set -euo pipefail

if [ "$#" != 1 ] || [ -z "$1" ]; then
    echo "usage: build.sh OUT" >&2
    exit 2
fi
here="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
root="$(cd -P "$here/../.." && pwd -P)"
mkdir -p "$1"
out="$(cd "$1" && pwd)"

# shellcheck source=../../tools/check_lock.sh
source "$root/tools/check_lock.sh"
check_lock "$out/.build" symphony-web "client"

# The publisher, built before the client: a build that cannot publish makes nothing.
publisher="$("$root/tools/rs-bin.sh" --build symphony-build)"
bash "$here/npm.sh" run --silent build -- --outDir "$out/.modules/web" --emptyOutDir >&2
exec bash "$here/package.sh" "$out" "$publisher"
