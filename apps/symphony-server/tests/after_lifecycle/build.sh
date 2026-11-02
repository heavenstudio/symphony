#!/bin/bash
# Only expensive build commands are replaced. The recipe and installed server are real.
set -euo pipefail
# A scratch installation has no real agent panes to reap, including its startup sweep.
if [ "${1:-}" = tasks ]; then
    [ "$*" = 'tasks reap' ] || exit 93
    exit 0
fi
printf '%s\t%s\t%s\n' "$1" "$PWD" "$*" >> "$SF_LIFECYCLE_LOG"
case "$1" in
    ui)
        out="${2:-build/symphony-ui}"
        mkdir -p "$out/public"
        printf '<!doctype html><html>lifecycle client</html>\n' > "$out/public/index.html"
        ;;
    sf-slots)
        shift
        [ "$1" = cargo ]
        shift
        case "$1" in
            build) out=.target/debug ;;
            install)
                while [ "$1" != --root ]; do shift; done
                out="$2/bin"
                ;;
            *) exit 91 ;;
        esac
        mkdir -p "$out"
        # Cargo is the stand-in here, not executable copying or cold-image validation. Reuse
        # the built inode at the real install path so parallel cases do not restage a large
        # debug image before every exec. Never write through a link to the original binary.
        if [ ! "$SF_LIFECYCLE_BINARY" -ef "$out/symphony-server" ]; then
            ln -f "$SF_LIFECYCLE_BINARY" "$out/symphony-server"
        fi
        ;;
    *) exit 92 ;;
esac
