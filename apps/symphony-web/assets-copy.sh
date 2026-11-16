#!/usr/bin/env bash
# Merge regular immutable asset files into an unpublished assembly.
set -euo pipefail
source_dir="${1:?missing source assets}"
destination="${2:?missing destination assets}"
for directory in "$source_dir" "$destination"; do
    if [ -L "$directory" ] || [ ! -d "$directory" ]; then
        printf 'symphony-web: missing regular asset directory: %s\n' "$directory" >&2
        exit 2
    fi
done
shopt -s nullglob dotglob
for file in "$source_dir"/*; do
    if [ -L "$file" ] || [ ! -f "$file" ]; then
        printf 'symphony-web: asset must be a regular file: %s\n' "$file" >&2
        exit 2
    fi
    target="$destination/${file##*/}"
    if [ -L "$target" ] || { [ -e "$target" ] && ! cmp -s "$file" "$target"; }; then
        printf 'symphony-web: immutable asset collision: %s -> %s\n' "$file" "$target" >&2
        exit 2
    fi
    cp "$file" "$target"
done
