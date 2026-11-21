#!/usr/bin/env bash
# The bundle report of a build: each kind's first visit and what it defers, file by file with its
# bytes as built and gzipped, read from Vite's manifest in OUT/.modules/web (report.ts,
# src/bundle-report.ts). It reads a build and writes nothing.
#
#     apps/symphony-web/report.sh OUT       OUT as build.sh took it (`just symphony-ui-dist OUT`)
set -euo pipefail

if [ "$#" != 1 ] || [ -z "$1" ] || [ ! -d "$1" ]; then
    echo "usage: report.sh OUT" >&2
    exit 2
fi
here="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
out="$(cd "$1" && pwd)"
exec bash "$here/npm.sh" run --silent report -- "$out"
