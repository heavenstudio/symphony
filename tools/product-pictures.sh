#!/usr/bin/env bash
# Retakes the product specs' pictures (docs/product/README.md, "Pictures"): every line of
# docs/product/pictures.txt, at the widths it names, from a server of its own over a copy of the
# fixture project (tools/product-pictures/fixture), with a machine folder of its own and a board
# of its own, the copy's, which tools/product-pictures/board.sh fills, and the runs and the hours
# tools/product-pictures/records.sh says its machine recorded. Every picture is taken at
# one moment (NOW below, on the board's clock and the page's), in UTC, so a page that draws an age
# or a time is the same picture on any day. It touches no shared server and no project's board,
# and ends the one server it started, the copy's board server with it.
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
# The moment of every picture: 2026-03-02 09:30 UTC, a Monday morning.
NOW=1772443800
export TZ=UTC
server=""
reader=""
board=""

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
    # The copy's board server, which the Symphony's started, ends once its starter has: it is no
    # child of this script's to wait for, so it is ended by the pid it recorded and looked at until
    # it is gone (ten seconds at most), as it writes in the folder removed next.
    board="$(jq -r '.pid // empty' "$work/project/.git/symphony/tasks/server.json" 2>/dev/null || true)"
    if [ -n "$board" ]; then
        kill "$board" 2>/dev/null || true
        for _ in $(seq 100); do
            kill -0 "$board" 2>/dev/null || break
            sleep 0.1
        done
    fi
    rm -rf "$work"
}
trap finish EXIT

# The fixture as a repository of its own: the server serves a git checkout.
cp -R "$root/tools/product-pictures/fixture" "$work/project"
git -C "$work/project" init --quiet --initial-branch main
git -C "$work/project" add --all
# Made at a moment of its own, so a commit's id is the same in every copy.
GIT_AUTHOR_DATE="@$((NOW - 30 * 86400)) +0000" GIT_COMMITTER_DATE="@$((NOW - 30 * 86400)) +0000" \
    git -C "$work/project" -c user.name=Orchard -c user.email=orchard@example.invalid \
    commit --quiet --message "The fixture"

shot="$("$root/tools/rs-bin.sh" --build symphony)"
serve="$("$root/tools/rs-bin.sh" --build symphony-server)"
tasks="$("$root/tools/rs-bin.sh" --build tasks)"
copy="$("$root/tools/rs-bin.sh" --build tasks-copy)"
# The board's database is the pinned SurrealDB the board's own tests run.
SYMPHONY_SURREAL="${SYMPHONY_SURREAL:-$(bash "$root/tools/surreal.sh")}"
export SYMPHONY_SURREAL
# TASKS_RECORDER=off: nothing here is a build or a test of a task's. No hook, sweep or terminal of
# this machine's is the copy's: a board command there starts none.
export SYMPHONY_MACHINE_DIR="$work/machine" TASKS_RECORDER=off TASKS_SWEEP=off HC_BIN=/usr/bin/false
unset TASKS_SESSION TASKS_TASK TASKS_SERVER TASKS_DATABASE
mkdir "$work/machine"

# The copy's board is made by its first command, as a project's is; the server makes none. The
# board's own server ends with the command that started it, and the Symphony's then holds it.
(cd "$work/project" && TASKS_NOW=$((NOW - 30 * 86400)) "$tasks" list >/dev/null 2>"$work/board.log") || {
    cat "$work/board.log" >&2
    echo "product-pictures: the fixture's board was not made" >&2
    exit 1
}

# The server's output goes through a FIFO to a reader that keeps it whole in its log and says the
# server's address on a second FIFO the first time the server prints it, or an empty line once the
# output ends without one, the server having ended. The script waits for that line, `read -t` its
# deadline. The second FIFO is held open both ways (fd 3), so neither side's open waits.
mkfifo "$work/output" "$work/address"
exec 3<>"$work/address"
(cd "$work/project" && TASKS_NOW=$NOW \
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

# The board: its tasks, written by the board's own commands, then every session given the name
# the fixture has for it in place of the handle the board drew at random, and the folder it
# registered from, this run's, a fixed one: the board's dump with those words exchanged (through a
# word no handle is, so a name that is another session's drawn handle is not exchanged twice), read
# back in whole.
if ! (cd "$work/project" && TASKS="$tasks" NOW=$NOW bash "$root/tools/product-pictures/board.sh") \
    >"$work/names" 2>"$work/board.log"; then
    cat "$work/board.log" >&2
    echo "product-pictures: the fixture's board was not filled" >&2
    exit 1
fi
drawn=()
named=()
count=0
while read -r name handle; do
    count=$((count + 1))
    drawn+=(-e "s/$handle/@session-$count@/g")
    named+=(-e "s/@session-$count@/$name/g")
done <"$work/names"
(
    cd "$work/project"
    TASKS_NOW=$NOW "$copy" --board --out |
        sed "${drawn[@]}" | sed "${named[@]}" |
        jq -c '.sessions |= map(.worktree = "/orchard")' >"$work/board.json"
    TASKS_NOW=$NOW "$copy" --board <"$work/board.json" >>"$work/board.log" 2>&1
) || {
    cat "$work/board.log" >&2
    echo "product-pictures: the fixture's board was not given its sessions' names" >&2
    exit 1
}

# What the machine recorded of the project, for Reports and Runs: no page has asked for it yet.
if ! (cd "$work/project" && NOW=$NOW bash "$root/tools/product-pictures/records.sh") \
    >>"$work/board.log" 2>&1; then
    cat "$work/board.log" >&2
    echo "product-pictures: the fixture's runs and hours were not written" >&2
    exit 1
fi

taken=0
while read -r name widths path steps || [ -n "$name" ]; do
    case "$name" in '' | '#'*) continue ;; esac
    # From the list's `=theme` line on, the project has the theme of its own
    # (tools/product-pictures/theme.toml), which its pages take at their next load: whichever
    # pictures are retaken, those after the line are of the themed project.
    if [ "$name" = "=theme" ]; then
        cat "$root/tools/product-pictures/theme.toml" >>"$work/project/symphony.toml"
        continue
    fi
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
        "$shot" shot "$base$path" --out "$out/$name-phone.png" --phone --scheme light --clock "$NOW" --timeout 60 $steps
    fi
    if [ "$widths" != phone ]; then
        # shellcheck disable=SC2086
        "$shot" shot "$base$path" --out "$out/$name-wide.png" --size 1280x800 --scheme light --clock "$NOW" --timeout 60 $steps
    fi
    taken=$((taken + 1))
done <"$list"

if [ "$taken" -eq 0 ]; then
    echo "product-pictures: no line of $list is named ${*:-at all}" >&2
    exit 1
fi
echo "product-pictures: $taken pages retaken in $out"
