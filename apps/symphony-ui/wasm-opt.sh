#!/bin/bash
# wasm-opt as dx runs it on each module of the program's split, then `symphony-build split-repair
# --chunk` on what it wrote: a chunk's segments put in order, a main module left as it is. dx names
# each module by its bytes as soon as wasm-opt has written it, and the script it then minifies names
# the chunks so. The splitter writes a chunk's segments in the order of a hash map, and esbuild
# picks the script's short names by how often each character appears in it, so chunks named before
# they are in order would give the script other bytes on some builds, which renaming the chunks
# after dx cannot undo. bundle.sh runs dx with a DX_HOME of its own whose wasm-opt this is,
# SYMPHONY_WASM_OPT naming the real one and SYMPHONY_SPLIT_REPAIR the symphony-build that orders.
#
# It exits as wasm-opt did. A chunk it cannot put in order stays as wasm-opt wrote it: the repair
# after dx reads the same bytes and fails the build on them, where a failure here would only have dx
# put the module it was given, unoptimized, in wasm-opt's place.
set -euo pipefail
real="${SYMPHONY_WASM_OPT:?wasm-opt.sh runs under bundle.sh, which names the real wasm-opt}"
repair="${SYMPHONY_SPLIT_REPAIR:?wasm-opt.sh runs under bundle.sh, which names symphony-build}"
"$real" "$@"
written=
while [ "$#" -gt 0 ]; do
    if [ "$1" = -o ] && [ "$#" -gt 1 ]; then
        written="$2"
    fi
    shift
done
if [ -n "$written" ]; then
    "$repair" split-repair --chunk "$written" || true
fi
