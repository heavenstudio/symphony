#!/usr/bin/env bash
# The key of everything the client's build reads, printed as the last line: the release
# snapshot's name, `symphony-build release-inputs ... --key`, of the inputs captured as
# apps/symphony-ui/bundle.sh captures them (the lines below, which a test holds to bundle.sh's),
# then `web-` and the web client's package's hash.
# The regenerator runs it before `just client-check` (symphony.toml, `[land] generators`): a tip
# whose key and committed distribution passed that check before builds no client.
set -euo pipefail
root="$(cd -P "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
inputs="$(mktemp -d "${TMPDIR:-/tmp}/symphony-client-key.XXXXXXXX")"
trap 'rm -rf "$inputs"' EXIT
# bundle.sh's capture, line for line:
owned_git() {
    env -u GIT_DIR -u GIT_WORK_TREE -u GIT_COMMON_DIR -u GIT_INDEX_FILE git "$@"
}
"$root/tools/cargo.sh" metadata --format-version 1 --locked \
    --filter-platform wasm32-unknown-unknown > "$inputs/metadata.json"
owned_git -C "$root" ls-files --cached --others --exclude-standard -z > "$inputs/source"
components="$(jq -er '.packages[] | select(.name == "dioxus-primitives") | .manifest_path' "$inputs/metadata.json")"
components="$(cd "$(dirname "$components")/.." && pwd -P)"
component_pin="$(jq -er '.packages[] | select(.name == "dioxus-primitives") | .source' "$inputs/metadata.json")"
if [ "$(owned_git -C "$components" rev-parse HEAD)" != "${component_pin##*#}" ]; then
    printf 'symphony-ui: component checkout does not match its locked commit\n' >&2
    exit 2
fi
owned_git -C "$components" ls-files --cached -z > "$inputs/components"
snapshot_args=("$root" "$inputs/metadata.json" "$inputs/source" "$inputs/components")
publisher="$("$root/tools/rs-bin.sh" --build symphony-build)"
# The web client's build reads its package (apps/symphony-web) from the checkout: the hash of each
# file git lists there but its guides, by its path in the package and its bytes.
web="$(
    cd "$root/apps/symphony-web"
    owned_git ls-files --cached --others --exclude-standard -z -- . ':(exclude)*.md' \
        | LC_ALL=C sort -zu \
        | while IFS= read -r -d '' file; do
            [ ! -f "$file" ] || shasum -a 256 "$file"
        done \
        | shasum -a 256 | cut -c1-16
)"
# Its key, as bundle.sh names its snapshot's lock:
printf '%s web-%s\n' "$("$publisher" release-inputs "${snapshot_args[@]}" --key)" "$web"
