#!/usr/bin/env bash
# This workspace's ordinary Cargo binary. Every run builds first; --where only names its location.
set -euo pipefail
root="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
mode=run
case "${1:-}" in
    --build|--where|--link) mode="${1#--}"; shift ;;
esac
name="${1:-}"
case "$name" in
    symphony) package=symphony; member=crates/symphony ;;
    tasks|agent-send|tasks-copy|tasks-reclaim|tasks-cutover|fake-hc) package=symphony-board; member=crates/tasks ;;
    symphony-server|symphony-shelf|symphony-ui-dev) package=symphony-server; member=apps/symphony-server ;;
    *) printf 'symphony rs-bin: no declared binary %s in %s\n' "$name" "$root" >&2; exit 2 ;;
esac
shift
if [ ! -f "$root/Cargo.toml" ] || [ ! -f "$root/$member/Cargo.toml" ] || [ ! -f "$root/$member/src/bin/$name.rs" ]; then
    printf 'symphony rs-bin: missing owned source for %s under %s\n' "$name" "$root" >&2
    exit 2
fi
target="${CARGO_TARGET_DIR:-$root/.target}"
if [[ "$target" != /* ]]; then target="$root/$target"; fi
binary="$target/debug/$name"
if [ "$mode" = where ]; then printf '%s\n' "$binary"; exit 0; fi
# Cargo checks freshness on every call. The target pathname alone proves no prepared image.
"$root/tools/cargo.sh" build --quiet --locked -p "$package" --bin "$name" >&2
if [ ! -x "$binary" ]; then
    printf 'symphony rs-bin: Cargo produced no native executable at %s\n' "$binary" >&2
    exit 2
fi
if [ "$mode" = build ]; then printf '%s\n' "$binary"; exit 0; fi
if [ "$mode" = link ]; then
    mkdir -p "$root/.target/bin"
    ln -sfn "$binary" "$root/.target/bin/$name"
    exit 0
fi
exec "$binary" "$@"
