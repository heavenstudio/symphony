#!/bin/bash
# The path a release build runs dx from, printed: the same in every checkout and common dir, never
# the folder tools.sh installed dx in.
#
#     dx-path.sh TOOLS NAME        (bundle.sh: TOOLS the folder tools.sh names, NAME dx-0.7.10/bin/dx)
#
# dx makes itself the rustc wrapper of the workspace's own crates (RUSTC_WORKSPACE_WRAPPER), by its
# canonical path, and Cargo mixes that path into each such crate's -C metadata: their symbol names
# follow it, and so every byte of the client's wasm and every asset name after them. Run from
# another folder, as once tools/move-state.sh had moved the common dir's tools, the same sources
# made another client than the one committed. So a build runs STAND/NAME, STAND being
# /private/tmp/symphony-client-tools, a private folder of this user's beside the release snapshots:
# a hard link of TOOLS/NAME, or a copy of it where no link can be made (another volume), renamed
# into place whole whenever what stands there is not TOOLS/NAME's file or bytes, so a build that
# finds it so writes nothing. A link there is replaced, since dx would follow it to another path.
# SYMPHONY_CLIENT_TOOLS names another folder, for a test's stand-in dx; no flag sets it.
# Tests: tests/tools.rs.
set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "usage: dx-path.sh TOOLS NAME" >&2
    exit 2
fi
installed="$1/$2"
case "/$2/" in
    *//* | */./* | */../*)
        printf 'symphony-ui dx path: NAME must be a path inside TOOLS: %s\n' "$2" >&2
        exit 2
        ;;
esac
if [ ! -f "$installed" ] || [ ! -x "$installed" ]; then
    printf 'symphony-ui dx path: no executable %s\n' "$installed" >&2
    exit 2
fi

# The stand: a folder of this user's that no one else can write, as release-inputs keeps its
# snapshots, since every build runs what stands in it; named by its canonical path, the one dx
# finds itself at.
stand="${SYMPHONY_CLIENT_TOOLS:-/private/tmp/symphony-client-tools}"
mkdir -m 700 "$stand" 2>/dev/null || true
private=no
if [ -d "$stand" ] && [ ! -L "$stand" ] && [ -O "$stand" ]; then
    if mode="$(stat -c '%a' "$stand" 2>/dev/null)"; then :; else mode="$(stat -f '%Lp' "$stand")"; fi
    if (( (8#$mode & 8#077) == 0 )); then private=yes; fi
fi
if [ "$private" != yes ]; then
    printf 'symphony-ui dx path: %s is not a private folder of this user'"'"'s\n' "$stand" >&2
    exit 2
fi
stand="$(cd -P "$stand" && pwd -P)"
target="$stand/$2"
folder="${target%/*}"
mkdir -p "$folder"
if [ "$(cd -P "$folder" && pwd -P)" != "$folder" ]; then
    printf 'symphony-ui dx path: %s leads elsewhere through a link\n' "$folder" >&2
    exit 2
fi

# Whether the target is TOOLS/NAME's own file, or a file of the same bytes, and no link.
current() {
    [ ! -L "$target" ] && [ -f "$target" ] && [ -x "$target" ] \
        && { [ "$target" -ef "$installed" ] || cmp -s "$target" "$installed"; }
}
if ! current; then
    if [ -L "$target" ]; then
        rm -f "$target"
    elif [ -e "$target" ] && [ ! -f "$target" ]; then
        printf 'symphony-ui dx path: %s is no file\n' "$target" >&2
        exit 2
    fi
    staged="$folder/.dx-path.$$"
    trap 'rm -f "$staged"' EXIT
    rm -f "$staged"
    if ! ln -L "$installed" "$staged" 2>/dev/null; then
        cp "$installed" "$staged"
        chmod 755 "$staged"
    fi
    mv -f "$staged" "$target"
    trap - EXIT
fi
printf '%s\n' "$target"
