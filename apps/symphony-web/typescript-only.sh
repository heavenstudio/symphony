#!/usr/bin/env bash
# Hand-written code here is TypeScript (check.sh's step, `just client-lint`): this refuses every
# JavaScript file (.js, .jsx, .mjs or .cjs, in any case) git tracks in the checkout, or would track
# (written and not ignored, so a file not added yet is refused before its commit, not after),
# naming each, outside the one folder whose JavaScript nobody writes by hand:
#
#   apps/symphony-server/client/public/   the client's distribution, built from its TypeScript
#                                         (`just client-update`) and never edited
#
# A script a page runs as written is TypeScript with no syntax of its own, which `tsc` checks
# (apps/symphony-desktop/src/notification.ts); one Node runs is TypeScript Node strips (dev.ts).
#
#     apps/symphony-web/typescript-only.sh [ROOT]     ROOT: the checkout, this one's by default
set -euo pipefail
root="${1:-$(cd -P "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)}"
found="$(git -C "$root" ls-files --cached --others --exclude-standard --deduplicate -- \
    ':(icase)*.js' ':(icase)*.jsx' ':(icase)*.mjs' ':(icase)*.cjs' \
    ':(exclude)apps/symphony-server/client/public/')"
if [ -n "$found" ]; then
    printf 'typescript-only: hand-written code here is TypeScript (apps/symphony-web/AGENTS.md), and these are JavaScript:\n' >&2
    sed 's/^/  /' <<<"$found" >&2
    exit 1
fi
