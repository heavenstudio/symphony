#!/usr/bin/env bash
# Retain Cargo's compiler arguments and wrappers, adding only reproducible source diagnostics.
set -euo pipefail
compiler="${1:?missing compiler}"
shift
flags=(
    "--remap-path-prefix=${SYMPHONY_RELEASE_CARGO_HOME:?missing Cargo home}=/symphony/cargo"
    "--remap-path-prefix=${SYMPHONY_RELEASE_SOURCE:?missing source root}=/symphony/source"
    "--remap-path-prefix=${CARGO_TARGET_DIR:?missing target root}=/symphony/target"
)
if [ -n "${SYMPHONY_PARENT_RUSTC_WRAPPER:-}" ]; then
    exec "$SYMPHONY_PARENT_RUSTC_WRAPPER" "$compiler" "$@" "${flags[@]}"
fi
exec "$compiler" "$@" "${flags[@]}"
