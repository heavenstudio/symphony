#!/bin/bash
# Observe the real recipe interpreter without replacing its commands or argument handling.
set -euo pipefail
# The first PATH entry is the fixture's observer directory. Resolve the interpreter production
# would find without that observer, then restore PATH for the recipe's nested commands.
recipe_path="$PATH"
PATH="${PATH#*:}"
recipe_bash="$(command -v bash)"
PATH="$recipe_path"
export PATH
executable=no
if [ -x "$1" ]; then executable=yes; fi
printf '· symphony recipe bash entered at %s s since Unix epoch; script executable=%s\n' "$(date +%s)" "$executable" >&2
export PS4='+ symphony recipe ${SECONDS}s line ${LINENO}: '
exec "$recipe_bash" -x "$@"
