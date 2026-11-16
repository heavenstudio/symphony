#!/usr/bin/env bash
# The key of everything the client's build reads, printed as one line, `web-` and a hash: the web
# client's package (apps/symphony-web), each file git lists there but its guides, by its path in
# the package and its bytes, then the one file outside it the build reads, the notice styles the
# shell carries (apps/symphony-server/src/notices.css).
# The regenerator runs it before `just client-check` (symphony.toml, `[land] generators`): a tip
# whose key and committed distribution passed that check before builds no client.
set -euo pipefail
root="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
web="$(
    cd "$root/apps/symphony-web"
    {
        env -u GIT_DIR -u GIT_WORK_TREE -u GIT_COMMON_DIR -u GIT_INDEX_FILE \
            git ls-files --cached --others --exclude-standard -z -- . ':(exclude)*.md' \
            | LC_ALL=C sort -zu \
            | while IFS= read -r -d '' file; do
                [ ! -f "$file" ] || shasum -a 256 "$file"
            done
        shasum -a 256 ../symphony-server/src/notices.css
    } | shasum -a 256 | cut -c1-16
)"
printf 'web-%s\n' "$web"
