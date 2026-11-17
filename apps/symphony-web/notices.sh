#!/usr/bin/env bash
# Vite's inventory of packages in emitted chunks, plus the copied controls' provenance and
# selected MIT text. No clock or checkout path enters the notice. An incomplete inventory or a
# newly bundled license requiring a different selection is a build error, never an empty notice.
set -euo pipefail
inventory="${1:?usage: notices.sh INVENTORY OUT}"
destination="${2:?usage: notices.sh INVENTORY OUT}"
here="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
if [ ! -f "$inventory" ] || ! jq -e '
    type == "array" and length > 0 and all(.[];
        (.name | type == "string" and length > 0) and
        (.version | type == "string" and length > 0) and
        .identifier == "MIT" and
        (.text | type == "string" and length > 0))
' "$inventory" >/dev/null 2>&1; then
    echo "symphony-web: missing or incomplete bundled MIT license inventory: $inventory" >&2
    exit 2
fi
{
    printf 'Third-party notices for the Symphony client\n\n'
    jq -r 'sort_by(.name, .version)[] | "\(.name)@\(.version) (\(.identifier))\n\n\(.text)\n"' "$inventory"
    cat "$here/copied-controls.txt"
    printf '\n'
    cat "$here/src/styles/LICENSE-MIT"
    printf '\n'
} > "$destination"
