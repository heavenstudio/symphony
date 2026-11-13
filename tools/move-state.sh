#!/usr/bin/env bash
# Moves a repository's Symphony state, in its git common dir, into the one `symphony/` folder.
#
#     tools/move-state.sh [--dry-run] [CHECKOUT]     (CHECKOUT: the current folder when not given)
#
# Run once per repository, from any checkout of it, with the board and the shared server stopped
# (the owner's word, since both stop): the board by its hold (`tasks database hold`), whose record
# moves with the board's folder and holds the new build's board until its holder releases it. So do
# the board's other records of processes, each naming one by its pid and when it started: a private
# replay's (replay.json) and each ask to end a server (ending/PID). The new build reads them as its
# own, and as the move refuses while a server they name runs, none is adopted or asked twice.
# Each move is a rename on one disk, never a copy:
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
# file the new folder holds byte for byte (a build of the new code fetched it there first; a lock
# no fetch holds aside), the client's build tools where the new folder holds a whole install of
# them (dx's bin/dx, a wasm-bindgen in dx-home's tools: an install of the new code's beside them,
# and the old ones a cache), and a linked worktree's sweep lock or saved verdict the new sweep has
# written again. Only this command moves or removes an older build's copies of the client's build
# tools: a build of the new code runs them where they stand until then and installs none beside
# them (apps/symphony-ui/tools.sh), since no build may delete tools another worktree's build may
# be running.
#
# Everything is checked before anything moves. It refuses, exit 1 with one line per reason on
# stderr and nothing moved, when any other new path exists beside the old one it would take, when a
# verdict entry is in both the older install and the server's folder, when a move would cross
# disks, or while anything of the repository runs: a process whose command line names a path under
# those folders (the board's SurrealDB, the shared server installed there, a board build kept
# there), a process with a file open under the board's or the server's folder or under the
# client's build tools it moves or removes, or working there, by whatever path it opened them (a
# board command or a server by a relative path, a client build running the tools: by lsof), the
# board's server its record names, the machine's one Symphony server (its record in the machine
# folder: SYMPHONY_MACHINE_DIR, else the platform's), which serves every registered project, a land
# (its lock sf-tasks/land.lock held, or the process group sf-tasks/land/running names alive; the
# board's record and these in symphony/tasks/ too, once a stopped run moved the folder there), a
# fetch of SurrealDB holding its lock, or any program of the board or the server (tasks,
# agent-send, tasks-copy, tasks-reclaim, symphony, symphony-server, surreal) working in a checkout
# of the repository or its common dir, or naming one.
#
# A SurrealDB server (`surreal start … DATA`) whose data folder and working folder both lie
# outside the repository is passed and named on stdout, though it runs the pinned program from
# sf-surreal: a test's private board left running, which holds nothing of this repository's state
# but that program's file, and keeps running it once it moves. Its data folder, the last word of its command
# line (`memory`, or a path after `surrealkv://`, `rocksdb://` or `file://`), is made absolute
# against its working folder and resolved; a folder lies in the repository when it is under the
# common dir or a checkout git lists, or where git, asked there, finds this repository (one it
# finds nested in a checkout's ignored files is another), and a working folder since removed lies
# in none. A server with another shape, or one lsof finds with a file open under the board's or the
# server's folder, is refused like any other process.
#
# A run that finds nothing to move says so and exits 0, and a second run finishes what a stopped
# one left. --dry-run prints the moves, the passes and the refusals and changes nothing. It reads
# the process table with ps, working folders and the open files under those folders with lsof, the
# repositories with git and the land's lock with perl. Its last line says what comes next: with a
# hold moved, releasing it first by a build of the new code, as no board command starts the board
# while it stands.
#
# Where sf-tasks, sf-surreal, sf-symphony or sf-task-models moved or went, it leaves a one-line
# file of that name saying where, right after that folder's own move: a build of the old code then
# fails there, instead of starting an empty board, fetching SurrealDB again or installing the old
# server beside the moved state. A later run passes over these files, and writes the one a stopped
# run left unwritten: wherever such a folder is gone and its new one stands.
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
# The pid a live fetch of SurrealDB names in FOLDER's lock (`lock.d/pid`, tools/surreal.sh's).
fetching() {
    local holder
    holder="$(cat "$1/lock.d/pid" 2>/dev/null)" || return 1
    [[ "$holder" =~ ^[0-9]+$ ]] && kill -0 "$holder" 2>/dev/null || return 1
    printf '%s\n' "$holder"
}
# Whether FILE, under the pinned SurrealDB's FOLDER, is a fetch's lock, no state: the empty `lock`
# an older fetch script took, or what `lock.d` holds, whose live holder is refused below and whose
# gone one the next fetch takes over.
fetch_lock() {
    case "${2#"$1"/}" in
        lock) [ -f "$2" ] && [ ! -L "$2" ] && [ ! -s "$2" ] ;;
        lock.d/*) return 0 ;;
        *) return 1 ;;
    esac
}
# Whether OLD holds at least one file, and every entry under it but a fetch's lock is a plain file
# whose copy at its place under NEW holds the same bytes.
same_files() {
    local file found=no
    [ -d "$1" ] && [ ! -L "$1" ] || return 1
    while IFS= read -r -d '' file; do
        if fetch_lock "$1" "$file"; then continue; fi
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
# Where the fenced folder OLD goes; failure for a folder no fence stands for.
fence_of() {
    local pair
    for pair in "${fenced[@]}"; do
        if [ "${pair% *}" = "$1" ]; then
            printf '%s\n' "${pair#* }"
            return 0
        fi
    done
    return 1
}
# Whether FOLDER, the client's build tools under the common dir, holds a whole install of them: dx's
# own program, or a wasm-bindgen in dx's tool folder, each a plain executable file.
whole_tools() {
    local tool
    case "$1" in
        */dx-home)
            for tool in "$common/$1"/tools/wasm-bindgen-*/wasm-bindgen; do
                if [ -f "$tool" ] && [ ! -L "$tool" ] && [ -x "$tool" ]; then return 0; fi
            done
            return 1
            ;;
        *) [ -f "$common/$1/bin/dx" ] && [ ! -L "$common/$1/bin/dx" ] && [ -x "$common/$1/bin/dx" ] ;;
    esac
}
# The fenced folders a stopped run moved or removed before it could fence them: gone from their old
# name, their new folder standing.
unfenced=()
for pair in "${folders[@]}"; do
    old="${pair% *}"
    new="${pair#* }"
    if ! present "$common/$old"; then
        if fence_of "$old" >/dev/null && present "$common/$new"; then unfenced+=("$old"); fi
        continue
    fi
    if is_fence "$common/$old"; then continue; fi
    if present "$common/$new"; then
        if [ "$old" = sf-surreal ] && same_files "$common/$old" "$common/$new"; then
            drop+=("$old")
            why+=("$new holds the same pinned SurrealDB")
            continue
        fi
        case "$old" in
            sf-cargo/*)
                if whole_tools "$new"; then
                    drop+=("$old")
                    why+=("$new holds the client's build tools whole, which a build of the new code installed: the old folder is a cache")
                    continue
                fi
                ;;
        esac
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

# Whether this run moves or removes PATH itself.
planned() {
    local i
    for i in "${!from[@]}"; do [ "${from[$i]}" != "$1" ] || return 0; done
    for i in "${!drop[@]}"; do [ "${drop[$i]}" != "$1" ] || return 0; done
    return 1
}

# Whether FOLDER holds nothing but entries this run moves out of it or removes.
left_empty() {
    local folder="$1" entry name
    [ -d "$common/$folder" ] && [ ! -L "$common/$folder" ] || return 1
    for entry in "$common/$folder"/* "$common/$folder"/.*; do
        name="${entry##*/}"
        case "$name" in . | ..) continue ;; esac
        planned "$folder/$name" || return 1
    done
}

# Whether this run moves or removes anything out of FOLDER.
moves_from() {
    local i
    for i in "${!from[@]}"; do
        case "${from[$i]}" in "$1"/*) return 0 ;; esac
    done
    for i in "${!drop[@]}"; do
        case "${drop[$i]}" in "$1"/*) return 0 ;; esac
    done
    return 1
}

if [ "${#from[@]}" -eq 0 ] && [ "${#drop[@]}" -eq 0 ] && [ "${#unfenced[@]}" -eq 0 ] &&
    [ "${#refusals[@]}" -eq 0 ]; then
    printf 'nothing to move in %s\n' "$common"
    if present "$common/sf-docs"; then
        printf 'sf-docs holds an older install and nothing Symphony reads: it is safe to delete (%s)\n' "$common/sf-docs"
    fi
    exit 0
fi

# Nothing of the repository may run while its state moves. A process is named once, by the first
# reason found: the records' servers, any process naming the old folders, a land, a fetch of
# SurrealDB, then any program of the board or the server working in or naming a folder of the
# repository.
named=" "
# The pid a record names, by its "pid" field; nothing when the file or the field is not there.
recorded_pid() {
    local text pattern='"pid"[[:space:]]*:[[:space:]]*([0-9]+)'
    text="$(cat "$1" 2>/dev/null)" || return 1
    [[ "$text" =~ $pattern ]] || return 1
    printf '%s\n' "${BASH_REMATCH[1]}"
}
# The command line of process PID while it runs, less the blanks a process that renamed itself
# leaves at its end; failure when it does not run.
command_of() {
    local line
    [ "$1" -gt 0 ] 2>/dev/null && [ "$1" != "$$" ] || return 1
    line="$(ps -ww -o command= -p "$1" 2>/dev/null)" || return 1
    line="${line%"${line##*[![:space:]]}"}"
    [ -n "$line" ] || return 1
    printf '%s\n' "$line"
}

# The board's server, recorded in its old folder or, once a stopped run moved that, in its new one.
for record in sf-tasks/server.json symphony/tasks/server.json; do
    if pid="$(recorded_pid "$common/$record")" && [[ "$named" != *" $pid "* ]] &&
        line="$(command_of "$pid")"; then
        refuse "pid $pid ($line), the board's server $record records, runs: stop it and run this again"
        named="$named$pid "
    fi
done

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

# The folders of this repository: its common dir and each checkout git lists, as named, resolved,
# and (on macOS, where /tmp and /var are links into /private) without /private.
roots=()
add_root() {
    roots+=("$1")
    case "$1" in /private/*) roots+=("${1#/private}") ;; esac
}
add_root "$common"
if [ "$given" != "$common" ]; then add_root "$given"; fi
while IFS= read -r entry; do
    case "$entry" in
        "worktree "*)
            path="${entry#worktree }"
            add_root "$path"
            if real="$(cd "$path" 2>/dev/null && pwd -P)" && [ "$real" != "$path" ]; then add_root "$real"; fi
            ;;
    esac
done < <(unset GIT_DIR GIT_WORK_TREE GIT_COMMON_DIR
    git -C "$common" worktree list --porcelain 2>/dev/null)

# PATH made absolute against the folder FROM when it is relative, its `.` and `..` parts folded.
absolute() {
    local path="$1" part out="" parts=() folded=()
    case "$path" in /*) ;; *) path="$2/$path" ;; esac
    IFS=/ read -r -a parts <<<"$path"
    for part in ${parts[@]+"${parts[@]}"}; do
        case "$part" in
            "" | .) ;;
            ..)
                if [ "${#folded[@]}" -gt 0 ]; then
                    unset "folded[$((${#folded[@]} - 1))]"
                    folded=(${folded[@]+"${folded[@]}"})
                fi
                ;;
            *) folded+=("$part") ;;
        esac
    done
    for part in ${folded[@]+"${folded[@]}"}; do out="$out/$part"; done
    printf '%s\n' "${out:-/}"
}
# Whether the absolute, folded FOLDER lies in this repository: under its common dir or a checkout
# git lists, or where git, asked from it or from the nearest folder above it that exists, finds
# this repository. Where git finds another repository (one nested in a checkout's ignored files),
# it lies outside; where it finds none, it lies in a checkout only by its path.
in_repository() {
    local folder="$1" real="$1" found root
    while [ "$real" != / ] && [ ! -d "$real" ]; do
        real="${real%/*}"
        real="${real:-/}"
    done
    real="$(cd "$real" 2>/dev/null && pwd -P)" || return 0
    for root in "$common" "${common#/private}" "$given"; do
        case "$folder/" in "$root"/*) return 0 ;; esac
        case "$real/" in "$root"/*) return 0 ;; esac
    done
    if found="$(unset GIT_DIR GIT_WORK_TREE GIT_COMMON_DIR GIT_CEILING_DIRECTORIES
        git -C "$real" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)" &&
        [ -n "$found" ]; then
        found="$(cd "$found" 2>/dev/null && pwd -P)" || return 0
        [ "$found" = "$common" ]
        return
    fi
    for root in "${roots[@]}"; do
        case "$folder/" in "$root"/*) return 0 ;; esac
        case "$real/" in "$root"/*) return 0 ;; esac
    done
    return 1
}
# Whether process PID, whose command line is LINE, is a SurrealDB server whose data folder and
# working folder both lie outside this repository, as `passing` then says. Its data folder is the
# path its command line ends with, made absolute against its working folder (none for `memory`);
# a working folder that no longer exists is in no repository. A server whose program, data or
# working folder cannot be told is not one.
passing=""
outside_server() {
    local pid="$1" line="$2" program last data cwd where
    program="${line%% *}"
    [ "${program##*/}" = surreal ] || return 1
    case "${line#"$program"}" in " start "*) ;; *) return 1 ;; esac
    command -v lsof >/dev/null 2>&1 || return 1
    cwd="$(lsof -a -p "$pid" -d cwd -Fn 2>/dev/null | sed -n 's/^n//p' | head -n 1)"
    [ -n "$cwd" ] || return 1
    last="${line##* }"
    case "$last" in
        memory) data="" ;;
        surrealkv://?* | surrealkv+versioned://?* | rocksdb://?* | file://?*)
            data="$(absolute "${last#*://}" "$cwd")"
            if in_repository "$data"; then return 1; fi
            ;;
        *) return 1 ;;
    esac
    if [ -d "$cwd" ]; then
        if in_repository "$(absolute "$cwd" /)"; then return 1; fi
        where="$cwd"
    else
        where="$cwd, gone"
    fi
    passing="pid $pid ($line): a SurrealDB server whose data (${data:-in memory}) and working folder ($where) lie outside this repository"
}
# The servers passed, their pids in `passes` too: lsof below still refuses one with a file open
# under the board's or the server's folder.
passed=()
passed_pid=()
passes=" "
pass() {
    passed+=("$passing")
    passed_pid+=("$1")
    passes="$passes$1 "
}

# The old folders as a command line may name them: as git gave the common dir, resolved, and (on
# macOS) without /private. A server outside the repository that names only the old SurrealDB's
# program is passed: it holds nothing of the repository's state but that program's file.
prefixes=("$given/sf-" "$common/sf-")
case "$common" in /private/*) prefixes+=("${common#/private}/sf-") ;; esac
if table="$(ps -ww -axo pid=,command= 2>/dev/null)"; then
    while read -r pid line; do
        [ -n "$pid" ] && [ "$pid" != "$$" ] && [[ "$named" != *" $pid "* ]] || continue
        for prefix in "${prefixes[@]}"; do
            case "$line" in
                *"$prefix"*)
                    if outside_server "$pid" "$line"; then
                        pass "$pid"
                    else
                        refuse "pid $pid ($line) runs on this repository's state: stop it and run this again"
                        named="$named$pid "
                    fi
                    break
                    ;;
            esac
        done
    done <<<"$table"
else
    refuse "the process table could not be read (ps), so nothing running could be ruled out"
fi

# A process with a file open under the board's or the server's folder this run moves, or under an
# older build's copy of the client's build tools it moves or removes, or working there, as lsof
# finds it, by whatever path it opened them: a board command or a server by a relative path, a
# build running the tools, whatever its command line names. No build removes the tools, since
# another worktree's may be running them; only this run does.
for pair in "${folders[@]}"; do
    old="${pair% *}"
    case "$old" in
        sf-tasks | sf-symphony)
            what_it_is="this repository's state"
            then_what="stop it and run this again"
            ;;
        sf-cargo/*)
            what_it_is="an older build's copy of the client's build tools"
            then_what="let it end and run this again"
            ;;
        *) continue ;;
    esac
    planned "$old" || continue
    if ! command -v lsof >/dev/null 2>&1; then
        case "$old" in
            sf-cargo/*) refuse "lsof is needed to rule out a process with a file open under $old, the client's build tools" ;;
            *) refuse "lsof is needed to rule out a process with a file open under $old, $what_it_is" ;;
        esac
        continue
    fi
    while IFS=$'\t' read -r pid fd file; do
        [ -n "$pid" ] && [ "$pid" != "$$" ] && [[ "$named" != *" $pid "* ]] || continue
        line="$(command_of "$pid")" || continue
        if [ "$fd" = cwd ]; then what="works in ${file#"$common"/}"; else what="has ${file#"$common"/} open"; fi
        refuse "pid $pid ($line) $what, $what_it_is: $then_what"
        named="$named$pid "
    done < <(lsof -nP -w -Fpfn +D "$common/$old" 2>/dev/null |
        awk '/^p/ { pid = substr($0, 2); told = 0; next }
            /^f/ { fd = substr($0, 2); next }
            /^n/ && !told { printf "%s\t%s\t%s\n", pid, fd, substr($0, 2); told = 1 }')
done

# A land at work: the lock two lands take in turn held, or the process group a land killed outright
# names in its record still alive, in the board's old folder or, once a stopped run moved that, in
# its new one. A record whose group is gone moves with the folder, and the next land forgets it.
for board in sf-tasks symphony/tasks; do
    [ -d "$common/$board" ] && [ ! -L "$common/$board" ] || continue
    lock="$common/$board/land.lock"
    if [ -f "$lock" ]; then
        held=0
        perl -MFcntl=:flock -e 'open(my $f, "<", $ARGV[0]) or exit 2; exit(flock($f, LOCK_EX | LOCK_NB) ? 0 : 1)' \
            "$lock" 2>/dev/null || held=$?
        case "$held" in
            0) ;;
            1)
                holder="$(tr -d '\n' <"$lock" 2>/dev/null || true)"
                refuse "${holder:-a land} holds $board/land.lock: let it end and run this again"
                ;;
            *) refuse "whether a land holds $board/land.lock could not be asked (perl, flock): end every land of this repository and run this again" ;;
        esac
    fi
    record="$common/$board/land/running"
    group=""
    if [ -f "$record" ]; then IFS=$'\t' read -r group ran <"$record" || true; fi
    if [[ "$group" =~ ^[0-9]+$ ]] &&
        ps -axo pgid= 2>/dev/null | awk -v group="$group" '$1 == group { found = 1 } END { exit !found }'; then
        refuse "the process group $group a land started (${ran:-its command}) still runs: end it (kill -TERM -$group) and run this again"
    fi
done

# A fetch of SurrealDB into the old folder, under its lock.
if pid="$(fetching "$common/sf-surreal")" && [[ "$named" != *" $pid "* ]]; then
    refuse "pid $pid fetches SurrealDB into sf-surreal (its lock.d names it): let it end and run this again"
    named="$named$pid "
fi

# Every program of the board and the server, by its name (the program ps names, or its command
# line's first word), working in a folder of the repository or naming one: a board command,
# monitor, dispatcher, sweep or embedding pass, a server.
programs=" tasks agent-send tasks-copy tasks-reclaim symphony symphony-server surreal "
candidates=" "
consider() {
    [ -n "$1" ] && [ "$1" != "$$" ] && [[ "$named" != *" $1 "* ]] && [[ "$passes" != *" $1 "* ]] &&
        [[ "$candidates" != *" $1 "* ]] || return 0
    case "$programs" in *" ${2##*/} "*) candidates="$candidates$1 " ;; esac
}
if comms="$(ps -axo pid=,comm= 2>/dev/null)"; then
    while read -r pid comm; do consider "$pid" "$comm"; done <<<"$comms"
fi
if [ -n "${table:-}" ]; then
    while read -r pid line; do consider "$pid" "${line%% *}"; done <<<"$table"
fi
if [ "$candidates" != " " ]; then
    cwds=""
    if command -v lsof >/dev/null 2>&1; then
        # shellcheck disable=SC2086 # the pids, split to be joined with commas
        cwds="$(lsof -a -d cwd -p "$(echo $candidates | tr ' ' ,)" -Fpn 2>/dev/null || true)"
    else
        refuse "lsof is needed to rule out a program of the board or the server working in this repository"
    fi
    for pid in $candidates; do
        line="$(command_of "$pid")" || continue
        cwd="$(printf '%s\n' "$cwds" |
            awk -v pid="p$pid" '$0 == pid { on = 1; next } /^p/ { on = 0 } on && /^n/ { print substr($0, 2); exit }')"
        where=""
        for root in "${roots[@]}"; do
            if [ -n "$cwd" ]; then
                case "$cwd/" in "$root"/*) where="works in $cwd" && break ;; esac
            fi
            case "$line" in *"$root"/* | *"$root" | *"$root "*) where="names $root" && break ;; esac
        done
        if [ -n "$where" ]; then
            if outside_server "$pid" "$line"; then
                pass "$pid"
            else
                refuse "pid $pid ($line), a program of the board or the server, $where, a folder of this repository: stop it and run this again"
                named="$named$pid "
            fi
        fi
    done
fi

# The servers passed and refused by nothing after, each on a line of its own.
tell_passed() {
    local i
    for i in "${!passed[@]}"; do
        [[ "$named" != *" ${passed_pid[$i]} "* ]] || continue
        printf 'passed %s; it keeps running\n' "${passed[$i]}"
    done
}

if [ "$dry_run" = yes ]; then
    printf 'in %s (a dry run: nothing changes)\n' "$common"
    tell_passed
    for i in "${!from[@]}"; do
        printf 'would move %s to %s\n' "${from[$i]}" "${to[$i]}"
    done
    for i in "${!drop[@]}"; do
        printf 'would remove %s: %s\n' "${drop[$i]}" "${why[$i]}"
    done
    for pair in "${fenced[@]}"; do
        if planned "${pair% *}"; then printf 'would fence %s\n' "${pair% *}"; fi
    done
    for i in "${!unfenced[@]}"; do
        printf 'would fence %s, which a stopped run left unfenced\n' "${unfenced[$i]}"
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

# The fence at OLD, a fenced folder gone from there, once nothing is at its name; nothing for
# another folder. A run stopped between a folder's move and its fence leaves it to the next run.
fence() {
    local new
    new="$(fence_of "$1")" || return 0
    present "$common/$1" && return 0
    printf 'moved by tools/move-state.sh to %s: run only builds of the new code on this repository\n' \
        "$new" >"$common/$1"
    printf 'fenced %s: a build of the old code fails there instead of making it again\n' "$1"
}

printf 'in %s\n' "$common"
tell_passed
for i in "${!unfenced[@]}"; do
    fence "${unfenced[$i]}"
done
for i in "${!from[@]}"; do
    old="$common/${from[$i]}"
    new="$common/${to[$i]}"
    mkdir -p "${new%/*}"
    # An empty verdict folder the server's folder brought along gives way to the one moving in.
    if empty_folder "$new"; then rmdir "$new"; fi
    mv "$old" "$new"
    printf 'moved %s to %s\n' "${from[$i]}" "${to[$i]}"
    fence "${from[$i]}"
done
for i in "${!drop[@]}"; do
    rm -rf "${common:?}/${drop[$i]}"
    printf 'removed %s: %s\n' "${drop[$i]}" "${why[$i]}"
    fence "${drop[$i]}"
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
# A hold that moved with the board's folder still holds it: no board command starts it until its
# holder releases it.
# shellcheck disable=SC2016 # the backquotes name commands for the reader; nothing runs them here
if [ -f "$common/symphony/tasks/hold" ]; then
    printf 'next: release the hold first, with a build of the new code (`just tasks database release --as HANDLE`, by the handle that holds it), then start the board and the shared server from builds of the new code (the next `just tasks` starts the board; `just symphony` the shared server); no older build may run on this repository again\n'
else
    printf 'next: start the board and the shared server from builds of the new code (the next `just tasks` starts the board; `just symphony` the shared server); no older build may run on this repository again\n'
fi
