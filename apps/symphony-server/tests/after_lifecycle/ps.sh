#!/bin/bash
# The native callers and real process rows are exercised without scanning other agents' processes.
set -euo pipefail
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
# The server's census of leftovers: a case may hold its first round on the FIFO census-hold until
# it writes a line there, and give it rows of its own, before the real ones, in census-rows.
if [ "$fields" = pid=,ppid=,command= ]; then
    if [ -p "$fixture_root/census-hold" ]; then
        printf '%s\n' "$$" > "$fixture_root/census-pid"
        /bin/mv "$fixture_root/census-hold" "$fixture_root/census-held"
        read -r _ < "$fixture_root/census-held"
    fi
    if [ -f "$fixture_root/census-rows" ]; then /bin/cat "$fixture_root/census-rows"; fi
fi
# Successful native starts register their exact pid; command observations also register the
# invoking pid, so an interrupted starter still leaves its server discoverable to cleanup.
pids="$PPID"
for observed in "$fixture_root"/owned.* "$fixture_root"/env.*; do
    pid="${observed##*.}"
    if [[ "$pid" =~ ^[1-9][0-9]*$ ]]; then pids="$pids,$pid"; fi
done
exec /bin/ps -ww -p "$pids" -o "$fields"
