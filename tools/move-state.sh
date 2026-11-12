#!/usr/bin/env bash
# Moves a repository's Symphony state, in its git common dir, into the one `symphony/` folder.
#
#     tools/move-state.sh [--dry-run] [CHECKOUT]     (CHECKOUT: the current folder when not given)
#
# Run once per repository, from any checkout of it, with the board and the shared server stopped
# (the owner's word, since both stop). Each move is a rename on one disk, never a copy:
#
#     sf-tasks/                         symphony/tasks/            the board, its keys and records
#     sf-surreal/                       symphony/surreal/          the pinned SurrealDB (VERSION/surreal)
#     sf-symphony/                      symphony/server/           the shared server's build and journals
#     sf-task-models/                   symphony/models/           the board's embedding model
#     sf-runs/                          symphony/runs/             the recorded runs and their logs
#     sf-cargo/dx-0.7.10/               symphony/tools/dx-0.7.10/  the client's build tools; the rest
#     sf-cargo/dx-home/                 symphony/tools/dx-home/      of that folder is the project's own
#     sf-docs/verdict-backlog/          symphony/server/…          the verdict journals and their locks
#     sf-docs/verdict-publications/                                  an older install kept; the rest of
#     sf-docs/verdict-*.lock                                         that folder is named safe to delete
#     worktrees/NAME/sf-sweep-remove.lock    worktrees/NAME/symphony-sweep-remove.lock
#     worktrees/NAME/sf-sweep-verdict.json   worktrees/NAME/symphony-sweep-verdict.json
#     worktrees/NAME/.sf-sweep-lock-*        worktrees/NAME/.symphony-sweep-lock-*
#
# An old copy the new path already replaces goes instead of moving: a pinned SurrealDB whose every
# file the new folder holds byte for byte (a build of the new code fetched it there first), and a
# linked worktree's sweep lock or saved verdict the new sweep has written again.
#
# Everything is checked before anything moves. It refuses, exit 1 with one line per reason on
# stderr and nothing moved, when any other new path exists beside the old one it would take, when a
# verdict entry is in both the older install and the server's folder, when a move would cross
# disks, or while anything of the repository runs: a process whose command line names a path under
# those folders (the board's SurrealDB, the shared server installed there, a board build kept
# there), the board's server its record names, or the machine's one Symphony server (its record in
# the machine folder: SYMPHONY_MACHINE_DIR, else the platform's), which serves every registered
# project. A run that finds nothing to move says so and exits 0, and a second run finishes what a
# stopped one left. --dry-run prints the moves and the refusals and changes nothing.
#
# Where sf-tasks, sf-surreal, sf-symphony or sf-task-models moved or went, it leaves a one-line
# file of that name saying where: a build of the old code then fails there, instead of starting an
# empty board, fetching SurrealDB again or installing the old server beside the moved state. A
# later run passes over these files.
#
# It never touches the board's database namespace (the store keeps it; renaming it needs an export
# and an import), the temporary folder or a build folder. Bash 3.2 is supported.
set -euo pipefail
shopt -s nullglob

say() { printf 'symphony move-state: %s\n' "$*" >&2; }
usage() { printf 'usage: tools/move-state.sh [--dry-run] [CHECKOUT]\n'; }

dry_run=no
checkout=""
for arg in "$@"; do
    case "$arg" in
        --dry-run) dry_run=yes ;;
        -h | --help) usage; exit 0 ;;
        -*) usage >&2; exit 2 ;;
        *)
            [ -z "$checkout" ] || { usage >&2; exit 2; }
            checkout="$arg"
            ;;
    esac
done
checkout="${checkout:-.}"

# The repository's common dir, as git names it and with every link resolved.
if ! given="$(unset GIT_DIR GIT_WORK_TREE GIT_COMMON_DIR
    git -C "$checkout" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)" ||
    [ -z "$given" ] || ! common="$(cd "$given" && pwd -P)"; then
    say "$checkout is not in a git repository"
    exit 1
fi

# What this run moves, as paths under the common dir: from[i] becomes to[i], in that order.
from=()
to=()
refusals=()
refuse() { refusals+=("$*"); }
# What this run removes instead of moving, as paths under the common dir, and why: drop[i], why[i].
drop=()
why=()
present() { [ -e "$1" ] || [ -L "$1" ]; }
# Whether OLD holds at least one file, and every entry under it is a plain file whose copy at its
# place under NEW holds the same bytes.
same_files() {
    local file found=no
    [ -d "$1" ] && [ ! -L "$1" ] || return 1
    while IFS= read -r -d '' file; do
        found=yes
        [ -f "$file" ] && [ ! -L "$file" ] && cmp -s "$file" "$2${file#"$1"}" || return 1
    done < <(find "$1" -mindepth 1 ! -type d -print0)
    [ "$found" = yes ]
}
# A real folder (not a link to one) holding nothing.
empty_folder() { [ -d "$1" ] && [ ! -L "$1" ] && [ -z "$(ls -A "$1")" ]; }
# The device a path is on, the link itself for a link: GNU stat, else the BSD one.
device() { stat -c %d "$1" 2>/dev/null || /usr/bin/stat -f %d "$1"; }

# Plan the move of OLD to NEW (both under the common dir), refusing what a rename cannot do there.
plan() {
    local old="$1" new="$2" above="$2"
    # The nearest folder above NEW that exists: NEW's folders are made there.
    while [[ "$above" == */* ]]; do
        above="${above%/*}"
        if present "$common/$above"; then break; fi
    done
    if [[ "$above" != */* ]] && ! present "$common/$above"; then above=""; fi
    if [ -n "$above" ] && { [ ! -d "$common/$above" ] || [ -L "$common/$above" ]; }; then
        refuse "$above is a file or a link, not a folder, so $old cannot move to $new: move it aside and run this again"
        return
    fi
    if [ "$(device "$common/$old")" != "$(device "$common/${above:-.}")" ]; then
        refuse "$old is on another disk than ${above:-the common dir}: its move would copy it; move it back and run this again"
        return
    fi
    from+=("$old")
    to+=("$new")
}

# Whole folders, each to its place under symphony/; a new one there already is a state to settle.
# The first four are fenced once they move.
folders=(
    "sf-tasks symphony/tasks"
    "sf-surreal symphony/surreal"
    "sf-symphony symphony/server"
    "sf-task-models symphony/models"
    "sf-runs symphony/runs"
    "sf-cargo/dx-0.7.10 symphony/tools/dx-0.7.10"
    "sf-cargo/dx-home symphony/tools/dx-home"
)
fenced=("${folders[@]:0:4}")
# Whether PATH is the file a run left where a folder moved.
is_fence() { [ -f "$1" ] && [ ! -L "$1" ] && grep -q '^moved by tools/move-state.sh to ' "$1"; }
for pair in "${folders[@]}"; do
    old="${pair% *}"
    new="${pair#* }"
    present "$common/$old" || continue
    if is_fence "$common/$old"; then continue; fi
    if present "$common/$new"; then
        if [ "$old" = sf-surreal ] && same_files "$common/$old" "$common/$new"; then
            drop+=("$old")
            why+=("$new holds the same pinned SurrealDB")
            continue
        fi
        refuse "both $old and $new exist: keep the one that holds the state, move the other aside, and run this again"
        continue
    fi
    plan "$old" "$new"
done

# The verdict journals and locks an older install kept, into the server's folder. One there already,
# where the server's folder is now or once it has moved, is refused unless it is an empty folder.
for entry in "$common/sf-docs/verdict-backlog" "$common/sf-docs/verdict-publications" \
    "$common/sf-docs"/verdict-*.lock; do
    present "$entry" || continue
    name="${entry##*/}"
    clash=""
    for there in "sf-symphony/$name" "symphony/server/$name"; do
        if present "$common/$there" && ! empty_folder "$common/$there"; then clash="$there"; fi
    done
    if [ -n "$clash" ]; then
        refuse "both sf-docs/$name and $clash exist: keep the one that holds the verdicts, move the other aside, and run this again"
        continue
    fi
    plan "sf-docs/$name" "symphony/server/$name"
done

# Each linked worktree's sweep files, renamed in its own folder of the common dir.
for admin in "$common"/worktrees/*/; do
    admin="${admin%/}"
    folder="worktrees/${admin##*/}"
    for entry in "$admin/sf-sweep-remove.lock" "$admin/sf-sweep-verdict.json" "$admin"/.sf-sweep-lock-*; do
        present "$entry" || continue
        name="${entry##*/}"
        case "$name" in
            sf-sweep-remove.lock) renamed=symphony-sweep-remove.lock ;;
            sf-sweep-verdict.json) renamed=symphony-sweep-verdict.json ;;
            *) renamed=".symphony-sweep-lock-${name#.sf-sweep-lock-}" ;;
        esac
        if present "$admin/$renamed"; then
            # A lock and a saved verdict the new sweep made again; a saved worktree lock is kept.
            case "$name" in
                sf-sweep-remove.lock | sf-sweep-verdict.json)
                    drop+=("$folder/$name")
                    why+=("the sweep's $folder/$renamed replaces it")
                    continue
                    ;;
            esac
            refuse "both $folder/$name and $folder/$renamed exist: keep one, move the other aside, and run this again"
            continue
        fi
        plan "$folder/$name" "$folder/$renamed"
    done
done

# Whether FOLDER holds nothing but entries this run moves out of it.
left_empty() {
    local folder="$1" entry name i moving
    [ -d "$common/$folder" ] && [ ! -L "$common/$folder" ] || return 1
    for entry in "$common/$folder"/* "$common/$folder"/.*; do
        name="${entry##*/}"
        case "$name" in . | ..) continue ;; esac
        moving=no
        for i in "${!from[@]}"; do
            if [ "${from[$i]}" = "$folder/$name" ]; then moving=yes; fi
        done
        [ "$moving" = yes ] || return 1
    done
}

# Whether this run moves or removes PATH itself.
planned() {
    local i
    for i in "${!from[@]}"; do [ "${from[$i]}" != "$1" ] || return 0; done
    for i in "${!drop[@]}"; do [ "${drop[$i]}" != "$1" ] || return 0; done
    return 1
}

# Whether this run moves anything out of FOLDER.
moves_from() {
    local i
    for i in "${!from[@]}"; do
        case "${from[$i]}" in "$1"/*) return 0 ;; esac
    done
    return 1
}

if [ "${#from[@]}" -eq 0 ] && [ "${#drop[@]}" -eq 0 ] && [ "${#refusals[@]}" -eq 0 ]; then
    printf 'nothing to move in %s\n' "$common"
    if present "$common/sf-docs"; then
        printf 'sf-docs holds an older install and nothing Symphony reads: it is safe to delete (%s)\n' "$common/sf-docs"
    fi
    exit 0
fi

# Nothing of the repository may run while its state moves. A process is named once, by the first
# reason found: the records' servers, then any process naming the old folders.
named=" "
# The pid a record names, by its "pid" field; nothing when the file or the field is not there.
recorded_pid() {
    local text pattern='"pid"[[:space:]]*:[[:space:]]*([0-9]+)'
    text="$(cat "$1" 2>/dev/null)" || return 1
    [[ "$text" =~ $pattern ]] || return 1
    printf '%s\n' "${BASH_REMATCH[1]}"
}
# The command line of process PID while it runs; failure when it does not.
command_of() {
    local line
    [ "$1" -gt 0 ] 2>/dev/null && [ "$1" != "$$" ] || return 1
    line="$(ps -ww -o command= -p "$1" 2>/dev/null)" || return 1
    [ -n "$line" ] || return 1
    printf '%s\n' "$line"
}

if pid="$(recorded_pid "$common/sf-tasks/server.json")" && line="$(command_of "$pid")"; then
    refuse "pid $pid ($line), the board's server sf-tasks/server.json records, runs: stop it and run this again"
    named="$named$pid "
fi

machine=""
if [ -n "${SYMPHONY_MACHINE_DIR:-}" ]; then
    case "$SYMPHONY_MACHINE_DIR" in
        /*) machine="$SYMPHONY_MACHINE_DIR" ;;
        *) refuse "SYMPHONY_MACHINE_DIR names $SYMPHONY_MACHINE_DIR, not an absolute folder, so the machine's Symphony server cannot be ruled out" ;;
    esac
elif [[ "$OSTYPE" == darwin* ]]; then
    if [[ "${HOME:-}" == /* ]]; then machine="$HOME/Library/Application Support/Symphony"; fi
elif [[ "${XDG_DATA_HOME:-}" == /* ]]; then
    machine="$XDG_DATA_HOME/symphony"
elif [[ "${HOME:-}" == /* ]]; then
    machine="$HOME/.local/share/symphony"
fi
if [ -z "$machine" ] && [ -z "${SYMPHONY_MACHINE_DIR:-}" ]; then
    refuse "the machine folder needs HOME, an absolute folder, so the machine's Symphony server cannot be ruled out"
elif [ -n "$machine" ] && pid="$(recorded_pid "$machine/server.json")" && line="$(command_of "$pid")"; then
    if [[ "$named" != *" $pid "* ]]; then
        refuse "pid $pid ($line), the machine's Symphony server ($machine/server.json), runs and serves every registered project: stop it (just symphony --stop) and run this again"
        named="$named$pid "
    fi
fi

# The old folders as a command line may name them: as git gave the common dir, resolved, and (on
# macOS, where /tmp and /var are links into /private) without /private.
prefixes=("$given/sf-" "$common/sf-")
case "$common" in /private/*) prefixes+=("${common#/private}/sf-") ;; esac
if table="$(ps -ww -axo pid=,command= 2>/dev/null)"; then
    while read -r pid line; do
        [ -n "$pid" ] && [ "$pid" != "$$" ] && [[ "$named" != *" $pid "* ]] || continue
        for prefix in "${prefixes[@]}"; do
            case "$line" in
                *"$prefix"*)
                    refuse "pid $pid ($line) runs on this repository's state: stop it and run this again"
                    named="$named$pid "
                    break
                    ;;
            esac
        done
    done <<<"$table"
else
    refuse "the process table could not be read (ps), so nothing running could be ruled out"
fi

if [ "$dry_run" = yes ]; then
    printf 'in %s (a dry run: nothing changes)\n' "$common"
    for i in "${!from[@]}"; do
        printf 'would move %s to %s\n' "${from[$i]}" "${to[$i]}"
    done
    for i in "${!drop[@]}"; do
        printf 'would remove %s: %s\n' "${drop[$i]}" "${why[$i]}"
    done
    for pair in "${fenced[@]}"; do
        if planned "${pair% *}"; then printf 'would fence %s\n' "${pair% *}"; fi
    done
    for folder in sf-cargo sf-docs; do
        if moves_from "$folder" && left_empty "$folder"; then
            printf 'would remove %s, left empty\n' "$folder"
        fi
    done
    for i in "${!refusals[@]}"; do say "${refusals[$i]}"; done
    [ "${#refusals[@]}" -eq 0 ] || exit 1
    exit 0
fi

if [ "${#refusals[@]}" -gt 0 ]; then
    for i in "${!refusals[@]}"; do say "${refusals[$i]}"; done
    exit 1
fi

printf 'in %s\n' "$common"
for i in "${!from[@]}"; do
    old="$common/${from[$i]}"
    new="$common/${to[$i]}"
    mkdir -p "${new%/*}"
    # An empty verdict folder the server's folder brought along gives way to the one moving in.
    if empty_folder "$new"; then rmdir "$new"; fi
    mv "$old" "$new"
    printf 'moved %s to %s\n' "${from[$i]}" "${to[$i]}"
done
for i in "${!drop[@]}"; do
    rm -rf "${common:?}/${drop[$i]}"
    printf 'removed %s: %s\n' "${drop[$i]}" "${why[$i]}"
done
for pair in "${fenced[@]}"; do
    old="${pair% *}"
    if planned "$old" && ! present "$common/$old"; then
        printf 'moved by tools/move-state.sh to %s: run only builds of the new code on this repository\n' \
            "${pair#* }" >"$common/$old"
        printf 'fenced %s: a build of the old code fails there instead of making it again\n' "$old"
    fi
done
for folder in sf-cargo sf-docs; do
    if moves_from "$folder" && empty_folder "$common/$folder"; then
        rmdir "$common/$folder"
        printf 'removed %s, left empty\n' "$folder"
    fi
done
if present "$common/sf-docs"; then
    printf 'sf-docs holds an older install and nothing Symphony reads: it is safe to delete (%s)\n' "$common/sf-docs"
fi
# shellcheck disable=SC2016 # the backquotes name commands for the reader; nothing runs them here
printf 'next: start the board and the shared server from builds of the new code (the next `just tasks` starts the board; `just symphony` the shared server); no older build may run on this repository again\n'
