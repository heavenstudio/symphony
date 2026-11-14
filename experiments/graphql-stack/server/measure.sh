#!/usr/bin/env bash
# Each build (rest, async-graphql, juniper) compiled from nothing in a target folder of its own,
# release as the shipped server is (codegen-units = 1), its CPU time and wall time printed with
# the machine's load, then its stripped binary's size, its bench and one query on a private port.
# The crates are fetched first, so no download is timed.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
cargo fetch --quiet
for build in rest async-graphql juniper; do
    rm -rf "target-$build"
    printf '== %s (load: %s)\n' "$build" "$(sysctl -n vm.loadavg)"
    /usr/bin/time -p cargo build --quiet --release --features "$build" --target-dir "target-$build" 2>&1 \
        | grep -E '^(real|user|sys)'
    bin="target-$build/release/graphql-stack-server"
    strip -o "target-$build/stripped" "$bin"
    printf 'stripped binary: %s bytes\n' "$(stat -f %z "target-$build/stripped")"
    "$bin" bench 15
    said="target-$build/address"
    rm -f "$said"
    "$bin" serve-once >"$said" &
    server=$!
    until [ -s "$said" ]; do sleep 0.1; done
    address="$(head -n 1 "$said")"
    if [ "$build" = rest ]; then
        curl -s "$address/api/column" | head -c 160
    else
        curl -s "$address/graphql" -H 'Content-Type: application/json' \
            -d '{"query":"{ board { version counts { ready } } column(status: \"ready\") { slug } }"}' |
            head -c 160
    fi
    printf '\n'
    wait "$server"
done
