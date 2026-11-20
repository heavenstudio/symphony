#!/usr/bin/env bash
# The web client's checks (`just client-lint`, which `just lint-rs` runs): that the checkout's
# hand-written code is TypeScript (typescript-only.sh), its wire types against the Rust ones
# (wire.sh --check) and the tests of the example that writes them (its reader of the Rust the client
# carries), Biome's lint and format with every warning an error, TypeScript's types (`tsc --noEmit`,
# the browser's and then Node's), and its unit tests with their coverage (Vitest). Each runs
# whatever the ones before it did, and the script fails when any did.
set -uo pipefail
here="$(cd -P "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
root="$(cd -P "$here/../.." && pwd -P)"
red=()
step() {
    local name="$1"
    shift
    if ! "$@"; then red+=("$name"); fi
}
step "TypeScript only" bash "$here/typescript-only.sh" "$root"
step "wire types" bash "$here/wire.sh" --check
step "wire example's tests" "$root/tools/cargo.sh" test --quiet --locked -p symphony-api \
    --features typescript --example typescript
step "lint and types" "$here/npm.sh" run --silent lint
step "unit tests" "$here/npm.sh" run --silent test
if [ "${#red[@]}" -gt 0 ]; then
    printf 'client-lint: red in %s\n' "$(printf '%s, ' "${red[@]}" | sed 's/, $//')" >&2
    exit 1
fi
