#!/usr/bin/env bash
# Retakes the product specs' pictures (docs/product/README.md, "Pictures"): every line of
# docs/product/pictures.txt, on a phone and wide, from a server of its own over a copy of the
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

finish() {
    if [ -n "$server" ]; then
        kill "$server" 2>/dev/null || true
        wait "$server" 2>/dev/null || true
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

# TASKS_RECORDER=off: nothing here is a build or a test of a task's.
mkdir "$work/machine"
(cd "$work/project" && SYMPHONY_MACHINE_DIR="$work/machine" TASKS_RECORDER=off \
    exec "$serve" --port 0 --host 127.0.0.1 --root "$work/project") >"$work/server.log" 2>&1 &
server=$!

base=""
for _ in $(seq 1 600); do
    base="$(grep --only-matching --max-count 1 'http://127\.0\.0\.1:[0-9]*' "$work/server.log" || true)"
    [ -n "$base" ] && break
    if ! kill -0 "$server" 2>/dev/null; then
        cat "$work/server.log" >&2
        echo "product-pictures: the server ended before it said its address" >&2
        exit 1
    fi
    sleep 0.1
done
if [ -z "$base" ]; then
    cat "$work/server.log" >&2
    echo "product-pictures: the server said no address in 60 s (load $(uptime | sed 's/.*averages*: //'))" >&2
    exit 1
fi

taken=0
while read -r name path steps; do
    case "$name" in '' | '#'*) continue ;; esac
    if [ "$#" -gt 0 ]; then
        case " $* " in *" $name "*) ;; *) continue ;; esac
    fi
    # shellcheck disable=SC2086 # a line's steps are shot's own options, split on spaces
    "$shot" shot "$base$path" --out "$out/$name-phone.png" --phone --scheme light --timeout 60 $steps
    # shellcheck disable=SC2086
    "$shot" shot "$base$path" --out "$out/$name-wide.png" --size 1280x800 --scheme light --timeout 60 $steps
    taken=$((taken + 1))
done <"$list"

if [ "$taken" -eq 0 ]; then
    echo "product-pictures: no line of $list is named ${*:-at all}" >&2
    exit 1
fi
echo "product-pictures: $taken pages retaken, on a phone and wide, in $out"
