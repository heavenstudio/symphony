#!/usr/bin/env bash
# The inputs of each step of `just verify` and each lane of its tests at this checkout's commit,
# a line each: `verify/STEP` or `verify/tests/LANE`, a tab, and the digest of what it reads. The
# release run asks it before its checks (crates/symphony-build/src/release/inputs.rs), and a
# release candidate's rerun carries the green of a step or lane whose digest is the one it was
# green on (docs/workflow/RELEASES.md, "The rerun").
#
# What each reads is said beside it, in tools/verify.sh and tools/test-lanes.sh (`reads`, asked
# with `--inputs`). A digest holds, by its path and the name git gives its content at HEAD:
#
#   - the tree outside apps/ and crates/, each entry of the root: the check itself (this script,
#     tools/, the justfile, the lock file, the toolchain pin, the Cargo and nextest
#     configuration, symphony.toml) and every document, guide and skill, which any test may read;
#   - the toolchain installed, as `rustc -vV` and `cargo -vV` say it, which no path pins whole;
#   - for `package NAME` and `test TARGET`, the folder of each package it builds with: the
#     packages that hold the target, and every package of the workspace they depend on by path,
#     dev and build dependencies among them, from `cargo metadata`;
#   - for `path PATH`, that path;
#   - for `tree`, the whole tree, so a green is carried only on the tree it was green on.
#
# It reads the commit, never the working files: the run's checkout is clean. Anything it cannot
# tell (a package or a target no manifest names, a cargo that does not answer) fails it, exit 2,
# and nothing is carried by a digest. Bash 3.2 (macOS's /bin/bash) runs it.
set -uo pipefail

# The version of what a digest holds: a change to this script's reading changes every digest.
version=1
root="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$root" || exit 2

fail() {
    printf 'check-inputs: %s\n' "$1" >&2
    exit 2
}

git=(env -u GIT_DIR -u GIT_WORK_TREE -u GIT_COMMON_DIR -u GIT_INDEX_FILE git)
said="$(bash tools/verify.sh "$root/justfile" --inputs)" || fail 'tools/verify.sh --inputs failed'
[ -n "$said" ] || fail 'tools/verify.sh --inputs named no step'
metadata="$("${CARGO:-cargo}" metadata --no-deps --format-version 1 --locked --offline)" \
    || fail 'cargo metadata failed'
toolchain="$(rustc -vV && "${CARGO:-cargo}" -vV)" || fail 'the toolchain does not say its version'
whole="$("${git[@]}" rev-parse --verify --quiet 'HEAD^{tree}')" || fail 'HEAD names no tree'
# The root's entries but the packages' folders, each with the name of its content.
outside="$("${git[@]}" ls-tree HEAD | awk -F '\t' '$2 != "apps" && $2 != "crates"')" \
    || fail 'git ls-tree HEAD failed'

# Each name with what it reads resolved to `tree` or a path: NAME, a tab, `tree` or the path.
resolved="$(printf '%s\n' "$metadata" | jq -r --arg said "$said" '
    .workspace_root as $root
    | (.packages | map({
        key: .name,
        value: {
            folder: (.manifest_path | rtrimstr("/Cargo.toml") | ltrimstr($root + "/")),
            needs: [.dependencies[] | select(.path != null) | .name],
            tests: [.targets[] | select(.kind | index("test")) | .name]
        }
      }) | from_entries) as $packages
    | def closure:
        . as $start
        | {seen: [], todo: $start}
        | until(.todo | length == 0;
            .todo[0] as $name
            | if (.seen | index($name)) then .todo |= .[1:]
              elif $packages[$name] == null then error("no package \($name) in the workspace")
              else {seen: (.seen + [$name]), todo: (.todo[1:] + $packages[$name].needs)}
              end)
        | .seen;
      $said | split("\n")[] | select(length > 0) | split("\t")
    | .[0] as $name | .[1] as $kind | .[2] as $value
    | if $kind == "" then "\($name)\t"
      elif $kind == "tree" then "\($name)\ttree"
      elif $kind == "path" then "\($name)\t\($value)"
      elif $kind == "package" then ([$value] | closure[] | "\($name)\t\($packages[.].folder)")
      elif $kind == "test" then
        ([$packages | to_entries[] | select(.value.tests | index($value)) | .key]
         | if length == 0 then error("no test target \($value) in the workspace") else . end
         | closure[] | "\($name)\t\($packages[.].folder)")
      else error("\($name) reads by \($kind), which is no kind")
      end
')" || fail 'what a step reads does not resolve'

names="$(printf '%s\n' "$resolved" | cut -f 1 | awk '!seen[$0]++')"
tab="$(printf '\t')"
printf '%s\n' "$names" | while IFS= read -r name; do
    [ -n "$name" ] || continue
    reads="$(printf '%s\n' "$resolved" | awk -F '\t' -v name="$name" '$1 == name && $2 != "" { print $2 }' \
        | LC_ALL=C sort -u)"
    digest="$(
        {
            printf 'version %s\n%s\n' "$version" "$toolchain"
            if printf '%s\n' "$reads" | grep -qx tree; then
                printf 'tree %s\n' "$whole"
            else
                printf '%s\n' "$outside"
                printf '%s\n' "$reads" | while IFS= read -r path; do
                    [ -n "$path" ] || continue
                    # A path the commit does not hold is said as that: its absence is a state.
                    printf '%s %s\n' "$path" \
                        "$("${git[@]}" rev-parse --verify --quiet "HEAD:$path" || printf absent)"
                done
            fi
        } | shasum -a 256 | cut -c 1-16
    )"
    [ -n "$digest" ] || exit 2
    printf 'verify/%s%s%s\n' "$name" "$tab" "$digest"
done
