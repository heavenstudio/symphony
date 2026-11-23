#!/usr/bin/env bash
# Retakes the product specs' pictures (docs/product/README.md, "Pictures"): every line of
# docs/product/pictures.txt, at the widths it names, from a server of its own over a copy of the
# fixture project (tools/product-pictures/fixture), with a machine folder of its own and a board
# of its own, the copy's, which tools/product-pictures/board.sh fills, and the runs and the hours
# tools/product-pictures/records.sh says its machine recorded. From the list's `=machine` line on,
# they are the machine's one server's instead, started then over that machine folder with the copy
# and a second project with no board (tools/product-pictures/pantry) registered in it, so its root
# is the project picker; from its `=theme` line on, the copy's own server's again, the copy with a
# theme of its own. Every picture is taken at one moment (NOW below, on the board's clock and the
# page's), in UTC, so a page that draws an age or a time is the same picture on any day. It touches
# no shared server, no project's board and no terminal, and ends the servers it started, the
# board's PostgreSQL they started with them. It works in one folder, /tmp/symphony-product-pictures,
# one run at a time, as the Settings page draws its paths, and puts on its servers' PATH false for
# the agents' and Herdr's programs and its own build of the GitHub exchange's, so no page names a
# program or a folder of this machine's.
#
#   tools/product-pictures.sh          retake every picture
#   tools/product-pictures.sh NAME...  retake those named (the list's first word)
#
# A picture that differs afterwards (`git status docs/product`) had gone stale.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
list="$root/docs/product/pictures.txt"
out="$root/docs/product/pictures"
# The moment of every picture: 2026-03-02 09:30 UTC, a Monday morning.
NOW=1772443800
export TZ=UTC

# The programs, this checkout's own builds, built before the lock below is waited for, so a run
# that builds holds no other run up; the run that holds it finds each fresh again in seconds.
shot="$("$root/tools/rs-bin.sh" --build symphony)"
serve="$("$root/tools/rs-bin.sh" --build symphony-server)"
tasks="$("$root/tools/rs-bin.sh" --build tasks)"
copy="$("$root/tools/rs-bin.sh" --build tasks-copy)"
# The GitHub exchange's program is its package's own binary, which tools/rs-bin.sh does not run:
# Cargo builds it, as a build of it is made for the main checkout (AGENTS.md), and records no run.
TASKS_RECORDER=off "$root/tools/cargo.sh" build --quiet --locked -p symphony-github
github="${CARGO_TARGET_DIR:-$root/.target}"
[[ "$github" = /* ]] || github="$root/$github"
github="$github/debug/symphony-github"

# Every run works in the one folder WORK: the Settings page draws the paths of the copy's files and
# of the programs it finds, so a folder of each run's own would be other words in each picture. One
# run at a time holds it: the script runs again holding LOCK, the kernel's lock on that file, which
# lockf (macOS) or flock (Linux) waits for, half an hour at most, and which a run that ends in any
# way, killed outright too, lets go.
work=/tmp/symphony-product-pictures
lock="$work.lock"
if [ "${SYMPHONY_PICTURES_LOCK:-}" != "$lock" ]; then
    export SYMPHONY_PICTURES_LOCK="$lock"
    if command -v lockf >/dev/null; then
        exec lockf -k -t 1800 "$lock" "$BASH" "$0" "$@"
    fi
    if command -v flock >/dev/null; then
        exec flock -w 1800 "$lock" "$BASH" "$0" "$@"
    fi
    echo "product-pictures: neither lockf nor flock is here to hold $lock" >&2
    exit 1
fi

# Every server started, each followed by its output's reader.
started=()
board=""

# `clear_work`: WORK removed, the copy's board server ended first. That server, the PostgreSQL its
# last starter, the Symphony's, started, ends once its starters have: it is no child of this
# script's to wait for, so it is ended by the pid its data records (the first line of
# postmaster.pid) and looked at until it is gone (ten seconds at most), as it writes in the folder
# removed next. The second project holds no board, and the one server's read of it makes and starts
# none.
clear_work() {
    board="$(head -n 1 "$work/project/.git/symphony/tasks/postgresql/postmaster.pid" 2>/dev/null || true)"
    if [ -n "$board" ]; then
        kill "$board" 2>/dev/null || true
        for _ in $(seq 100); do
            kill -0 "$board" 2>/dev/null || break
            sleep 0.1
        done
    fi
    rm -rf "$work"
}

finish() {
    # Each server, then its reader, though a child of the server's still held its output.
    for pid in ${started[@]+"${started[@]}"}; do
        kill "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
    done
    clear_work
}
trap finish EXIT
# What a run killed outright left there goes first, its board server with it.
clear_work
mkdir "$work"
# WORK as the servers name it: on macOS, /tmp is /private/tmp.
work="$(cd "$work" && pwd -P)"

# `repository FOLDER TO`: tools/product-pictures/FOLDER as a repository of its own at TO, as the
# server serves a git checkout, its one commit made at a moment of its own, so its id is the same
# in every copy.
repository() {
    cp -R "$root/tools/product-pictures/$1" "$2"
    git -C "$2" init --quiet --initial-branch main
    git -C "$2" add --all
    GIT_AUTHOR_DATE="@$((NOW - 30 * 86400)) +0000" GIT_COMMITTER_DATE="@$((NOW - 30 * 86400)) +0000" \
        git -C "$2" -c user.name=Orchard -c user.email=orchard@example.invalid \
        commit --quiet --message "The fixture"
}
repository fixture "$work/project"

# The board's database is the machine's PostgreSQL, 18 or later, on which the copy's first board
# command makes its board, as a project's is (docs/operations/PROJECT_FILE.md#board).
# TASKS_RECORDER=off: nothing here is a build or a test of a task's. No hook, sweep or terminal of
# this machine's is the copy's: a board command there starts none, and the terminals' two
# programs are false (the servers' Herdr program, `hc`, the false in their folder below), so the
# one server's own sweep of the copy's board reaches none either.
export SYMPHONY_MACHINE_DIR="$work/machine" TASKS_RECORDER=off TASKS_SWEEP=off \
    HC_BIN=/usr/bin/false HERDR_APP_BIN=/usr/bin/false
unset TASKS_SESSION TASKS_TASK TASKS_SERVER TASKS_DATABASE
mkdir "$work/machine"
# The servers' own programs, in a folder put first on their PATH: the board build a server's sweep
# of the copy's board runs (a project's checkout links its own, `just symphony`, and a sweep that
# finds none fails, which every page would say); the GitHub exchange's, which the Settings page asks
# to describe itself; and false in the place of the agents' and Herdr's, which that page finds by
# name, so it names no program of this machine's and a launch from here would start nothing.
mkdir "$work/bin"
ln -s "$tasks" "$work/bin/tasks"
ln -s "$github" "$work/bin/symphony-github"
for program in claude codex hc; do
    ln -s /usr/bin/false "$work/bin/$program"
done

# The copy's board is made by its first command, on PostgreSQL, as a project's is; the server
# makes none. The board's own server ends with the command that started it, and the Symphony's
# then holds it.
(cd "$work/project" && TASKS_NOW=$((NOW - 30 * 86400)) "$tasks" list >/dev/null 2>"$work/board.log") || {
    cat "$work/board.log" >&2
    echo "product-pictures: the fixture's board was not made" >&2
    exit 1
}

# `start NAME FD ARGUMENT...`: the Symphony server with ARGUMENT..., run in the copy with its board
# clock held at NOW and its own programs first on its PATH, Herdr's found there by its name, its
# address left in `base`. Its output goes through a FIFO to a reader that
# keeps it whole in NAME.log and says the server's address on a second FIFO the first time the
# server prints it, or an empty line once the output ends without one, the server having ended.
# The script waits for that line, `read -t` its deadline. The second FIFO is held open both ways
# on FD, so neither side's open waits; FD is given, through `eval`, as macOS's own bash (3.2)
# cannot pick one itself (`exec {fd}<>`, bash 4.1).
start() {
    local name=$1 fd=$2
    shift 2
    mkfifo "$work/$name.output" "$work/$name.address"
    eval "exec $fd<>\"\$work/\$name.address\""
    (cd "$work/project" && PATH="$work/bin:$PATH" HC_BIN=hc TASKS_NOW=$NOW exec "$serve" "$@") \
        >"$work/$name.output" 2>&1 &
    started+=("$!")
    {
        said=""
        while IFS= read -r line || [ -n "$line" ]; do
            printf '%s\n' "$line"
            if [ -z "$said" ] && [[ $line =~ http://127\.0\.0\.1:[0-9]+ ]]; then
                said=1
                printf '%s\n' "${BASH_REMATCH[0]}" >&"$fd"
            fi
        done <"$work/$name.output" >"$work/$name.log"
        [ -n "$said" ] || echo >&"$fd"
    } &
    started+=("$!")
    base=""
    if ! read -r -t 60 -u "$fd" base; then
        cat "$work/$name.log" >&2
        echo "product-pictures: the $name server said no address in 60 s (load $(uptime | sed 's/.*averages*: //'))" >&2
        exit 1
    fi
    if [ -z "$base" ]; then
        cat "$work/$name.log" >&2
        echo "product-pictures: the $name server ended before it said its address" >&2
        exit 1
    fi
}

start own 3 --port 0 --host 127.0.0.1 --root "$work/project"

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

# The machine's one server, for the list's lines from `=machine` on, its address left in `machine`:
# the copy and the second project registered in the machine folder, as `just symphony` in each
# checkout registers it, the copy first, and the server started from the copy, as that start runs
# it, with the client this checkout commits, which it would otherwise look for in the machine
# folder. The copy's board is read before the server says its address; the second project's first
# read is asked for and answered before any picture, so the project picker counts both. It rolls
# the hour up into the copy's records and records hours of its own there, which the copy's own
# server leaves alone: the pictures that read them come before the line.
machine=""
machine() {
    repository pantry "$work/pantry"
    (cd "$work" && "$serve" project add "$work/project" && "$serve" project add "$work/pantry") \
        >"$work/projects.log" 2>&1 || {
        cat "$work/projects.log" >&2
        echo "product-pictures: the projects were not registered" >&2
        exit 1
    }
    start machine 4 serve --home "$work/machine" --install "$root/apps/symphony-server" \
        --root "$work/project" --port 0 --host 127.0.0.1
    machine="$base"
    curl --silent --show-error --max-time 60 --output /dev/null "$machine/pantry/api/board" || {
        cat "$work/machine.log" >&2
        echo "product-pictures: the second project's board was not read" >&2
        exit 1
    }
}

own="$base"
# Which server the line read next is taken from: the copy's own, or the machine's one server.
from=own
taken=0
while read -r name widths path steps || [ -n "$name" ]; do
    case "$name" in '' | '#'*) continue ;; esac
    # From the list's `=machine` line on, the pictures are the one server's, started at the first of
    # them retaken, so a retake of none starts it.
    if [ "$name" = "=machine" ]; then
        from=machine
        continue
    fi
    # From the list's `=theme` line on, the project has the theme of its own
    # (tools/product-pictures/theme.toml), which its pages take at their next load, from its own
    # server again: whichever pictures are retaken, those after the line are of the themed project.
    if [ "$name" = "=theme" ]; then
        cat "$root/tools/product-pictures/theme.toml" >>"$work/project/symphony.toml"
        from=own
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
    if [ "$from" = machine ]; then
        [ -n "$machine" ] || machine
        base="$machine"
    else
        base="$own"
    fi
    # What shot prints is a line's `--eval` value, the page set up for its picture and nothing to
    # read: only its errors are said.
    if [ "$widths" != wide ]; then
        # shellcheck disable=SC2086 # a line's steps are shot's own options, split on spaces
        "$shot" shot "$base$path" --out "$out/$name-phone.png" --phone --scheme light --clock "$NOW" --timeout 60 $steps >/dev/null
    fi
    if [ "$widths" != phone ]; then
        # shellcheck disable=SC2086
        "$shot" shot "$base$path" --out "$out/$name-wide.png" --size 1280x800 --scheme light --clock "$NOW" --timeout 60 $steps >/dev/null
    fi
    taken=$((taken + 1))
done <"$list"

if [ "$taken" -eq 0 ]; then
    echo "product-pictures: no line of $list is named ${*:-at all}" >&2
    exit 1
fi
echo "product-pictures: $taken pages retaken in $out"
