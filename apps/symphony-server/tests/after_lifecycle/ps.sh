#!/bin/bash
# The native callers and real process rows are exercised without scanning other agents' processes.
set -euo pipefail
# Before any write, hold the stand-in lifetime in the case's scratch (the fixture's HOME), which
# everything this script starts inherits. A server may start it without a fork (its board's
# lookup, say), handing this shell none of the case's endpoints, and kill the shell alone, so a
# child already started (env, mv) can still write after the server has ended. The case waits
# until no stand-in holds the lifetime before removing its scratch. It is opened for reading, which never creates a path,
# and found still in place after: once the case has withdrawn it, the open or that check fails and
# the script writes nothing.
exec 9<"$HOME/stand-ins/lifetime"
[ -p "$HOME/stand-ins/lifetime" ] || exit 1
fixture_root="${0%/*}/.."
{
    printf 'parent=%s\ncwd=%s\n' "$PPID" "$PWD"
    /usr/bin/env
} > "$fixture_root/env.$PPID.$$.partial"
/bin/mv "$fixture_root/env.$PPID.$$.partial" "$fixture_root/env.$PPID"
case " $* " in
    *' -p '*) exec /bin/ps "$@" ;;
esac
fields="${!#}"
# A case may hold one read of the whole table on the FIFO ps-hold until it writes a line there.
if [ -p "$fixture_root/ps-hold" ]; then
    /bin/mv "$fixture_root/ps-hold" "$fixture_root/ps-held"
    read -r _ < "$fixture_root/ps-held"
fi
# Successful native starts register their exact pid; command observations also register the
# invoking pid, so an interrupted starter still leaves its server discoverable to cleanup. This
# run's own env record names $PPID again: each pid is taken once.
pids=("$PPID")
for observed in "$fixture_root"/owned.* "$fixture_root"/env.*; do
    pid="${observed##*.}"
    if [[ "$pid" =~ ^[1-9][0-9]*$ && " ${pids[*]} " != *" $pid "* ]]; then pids+=("$pid"); fi
done
# Given one pid, /bin/ps asks for that process alone; given two, even the same one twice, it reads
# the whole process table, seconds under load. So each pid is asked of it alone, and the rows are
# printed as one call over them all prints them: the header once, when a field has a title, then
# by terminal device and pid, the order ps sorts in, a dead pid without a row and, with none
# alive, a failure. Each call prints its process's terminal first, under a title wider than any
# device's name (?? or 16/68), so that column's nine characters are cut from both lines. A column
# ps sizes to its widest row (user, etime) is as wide as each row's own; pid and ppid are not.
header=
rows=()
for pid in "${pids[@]}"; do
    if asked=$(/bin/ps -ww -p "$pid" -o tdev=TERMINAL -o "$fields"); then
        row="${asked#*$'\n'}"
        terminal="${row:0:8}"
        terminal="${terminal// /}"
        device=-1
        if [ "$terminal" != '??' ]; then device=$(((${terminal%/*} << 24) | ${terminal#*/})); fi
        rows+=("$device $pid ${row:9}")
    fi
    header="${asked%%$'\n'*}"
    header="${header:9}"
done
if [[ "$header" = *[![:space:]]* ]]; then printf '%s\n' "$header"; fi
[ ${#rows[@]} -gt 0 ] || exit 1
printf '%s\n' "${rows[@]}" | /usr/bin/sort -n -k1,1 -k2,2 | while IFS= read -r row; do
    row="${row#* }"
    printf '%s\n' "${row#* }"
done
