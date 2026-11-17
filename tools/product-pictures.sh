#!/usr/bin/env bash
# Retakes the product specs' pictures (docs/product/README.md, "Pictures"): every line of
# docs/product/pictures.txt, at the widths it names, from a server of its own over a copy of the
# fixture project (tools/product-pictures/fixture), with a machine folder of its own. It touches
# no shared server and no board, and ends the one server it started.
#
#   tools/product-pictures.sh          retake every picture
#   tools/product-pictures.sh NAME...  retake those named (the list's first word)
#
# A picture that differs afterwards (`git status docs/product`) had gone stale.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
list="$root/docs/product/pictures.txt"
out="$root/docs/product/pictures"
work="$(mktemp -d "${TMPDIR:-/tmp}/symphony-product-pictures.XXXXXXXX")"
server=""
reader=""

finish() {
    if [ -n "$server" ]; then
        kill "$server" 2>/dev/null || true
        wait "$server" 2>/dev/null || true
    fi
    # Its reader goes too, though a child of the server's still held its output.
    if [ -n "$reader" ]; then
        kill "$reader" 2>/dev/null || true
        wait "$reader" 2>/dev/null || true
    fi
    rm -rf "$work"
}
trap finish EXIT

# The fixture as a repository of its own: the server serves a git checkout.
cp -R "$root/tools/product-pictures/fixture" "$work/project"
git -C "$work/project" init --quiet --initial-branch main
git -C "$work/project" add --all
git -C "$work/project" -c user.name=Orchard -c user.email=orchard@example.invalid \
    commit --quiet --message "The fixture"

shot="$("$root/tools/rs-bin.sh" --build symphony)"
serve="$("$root/tools/rs-bin.sh" --build symphony-server)"

# The server's output goes through a FIFO to a reader that keeps it whole in its log and says the
# server's address on a second FIFO the first time the server prints it, or an empty line once the
# output ends without one, the server having ended. The script waits for that line, `read -t` its
# deadline. The second FIFO is held open both ways (fd 3), so neither side's open waits.
mkfifo "$work/output" "$work/address"
exec 3<>"$work/address"
# TASKS_RECORDER=off: nothing here is a build or a test of a task's.
mkdir "$work/machine"
(cd "$work/project" && SYMPHONY_MACHINE_DIR="$work/machine" TASKS_RECORDER=off \
    exec "$serve" --port 0 --host 127.0.0.1 --root "$work/project") >"$work/output" 2>&1 &
server=$!
{
    said=""
    while IFS= read -r line || [ -n "$line" ]; do
        printf '%s\n' "$line"
        if [ -z "$said" ] && [[ $line =~ http://127\.0\.0\.1:[0-9]+ ]]; then
            said=1
            printf '%s\n' "${BASH_REMATCH[0]}" >&3
        fi
    done <"$work/output" >"$work/server.log"
    [ -n "$said" ] || echo >&3
} &
reader=$!

base=""
if ! read -r -t 60 base <&3; then
    cat "$work/server.log" >&2
    echo "product-pictures: the server said no address in 60 s (load $(uptime | sed 's/.*averages*: //'))" >&2
    exit 1
fi
if [ -z "$base" ]; then
    cat "$work/server.log" >&2
    echo "product-pictures: the server ended before it said its address" >&2
    exit 1
fi

taken=0
while read -r name widths path steps || [ -n "$name" ]; do
    case "$name" in '' | '#'*) continue ;; esac
    if [ "$#" -gt 0 ]; then
        case " $* " in *" $name "*) ;; *) continue ;; esac
    fi
    case "$widths" in
    phone | wide | both) ;;
    *)
        echo "product-pictures: $name: widths are phone, wide or both, not '$widths'" >&2
        exit 1
        ;;
    esac
    if [ "$widths" != wide ]; then
        # shellcheck disable=SC2086 # a line's steps are shot's own options, split on spaces
        "$shot" shot "$base$path" --out "$out/$name-phone.png" --phone --scheme light --timeout 60 $steps
    fi
    if [ "$widths" != phone ]; then
        # shellcheck disable=SC2086
        "$shot" shot "$base$path" --out "$out/$name-wide.png" --size 1280x800 --scheme light --timeout 60 $steps
    fi
    taken=$((taken + 1))
done <"$list"

if [ "$taken" -eq 0 ]; then
    echo "product-pictures: no line of $list is named ${*:-at all}" >&2
    exit 1
fi
echo "product-pictures: $taken pages retaken in $out"
