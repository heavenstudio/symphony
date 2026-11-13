#!/bin/bash
# Before a wasm32 build: clean this workspace's own crates from the build folder's wasm32 builds, both
# profiles (debug for clippy, wasm-release for dx), when another tree built there last.
#
#     apps/symphony-ui/wasm-fresh.sh        (bundle.sh and `cargo symphony-ui clippy` run it)
#
# It runs under the workspace Cargo entrypoint, which names the tree's own build
# folder in CARGO_TARGET_DIR. Cargo names a workspace crate's artifacts by its path inside the workspace,
# the same in every worktree, and judges them fresh by mtime, so another tree's newer build would pass
# for this one's. The wrapper starts a new tree's folder from a seed another tree made, its wasm32 builds
# among it, crates and all. The wrapper cleans every seeded target/profile and writes this record
# after successful setup. Standalone builds, and folders with no matching record, clean both wasm
# profiles here. The record stays inside the folder beside the wrapper's records, which a seed never
# holds (the tree's root would be a file the tree does not track, written under a verify).
# Tests: tests/wasm_build.rs.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="${CARGO_TARGET_DIR:?wasm-fresh.sh runs under the workspace Cargo entrypoint, which names the build folder}"
cargo="${CARGO:-cargo}"
case "${1:-}" in
    '') release_snapshot=false ;;
    release-snapshot) release_snapshot=true ;;
    *) printf 'wasm-fresh: unknown mode %s\n' "$1" >&2; exit 2 ;;
esac

# The tree: the workspace's path and its manifest's inode and birth time, or its ctime where the file
# system records no birth time (GNU stat's %W reads 0 then), as the wrapper keys it; neither moves when
# the manifest's metadata alone is written. GNU stat first: BSD's -f is GNU's file-system mode, which
# would answer with the wrong numbers.
if made="$(stat -c '%i %W %Z' "$root/Cargo.toml" 2>/dev/null)"; then
    read -r inode born changed <<<"$made"
    case "$born" in 0 | -) born="$changed" ;; esac
else
    read -r inode born changed <<<"$(stat -f '%i %B %c' "$root/Cargo.toml")"
fi
key="$root $inode $born"
record="$target/symphony-tree-wasm32-unknown-unknown"
if $release_snapshot; then record="$target/symphony-release-tree"; fi
recorded="$(cat "$record" 2>/dev/null || true)"
if [ "$recorded" != "$key" ]; then
    # A record may still hold the key from before birth times, the manifest's inode and ctime: one that
    # names the manifest as it is now names this tree, so its builds stay and the record is rewritten.
    if [ "$recorded" != "$root $inode $changed" ]; then
        if $release_snapshot; then
            # Private path-patched components carry asset metadata too. A different snapshot
            # must not inherit any wasm release product from another canonical source path.
            (cd "$root" && "$cargo" clean --target wasm32-unknown-unknown --profile wasm-release --quiet --locked)
        else
            for profile in dev wasm-release; do
                (cd "$root" && "$cargo" clean --workspace --target wasm32-unknown-unknown --profile "$profile" --quiet --locked)
            done
        fi
    fi
    mkdir -p "$target"
    printf '%s' "$key" > "$record"
fi
