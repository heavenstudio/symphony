#!/usr/bin/env bash
# The containing project's launcher supplies source identities, build seeds and slot ownership.
exec "$(cd -P "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)/tools/rs-bin.sh" "$@"
