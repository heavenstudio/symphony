set positional-arguments

# The containing project supplies the source launcher, test slots and shared dependency seeds.
fmt:
    @tools/rs-bin.sh sf-slots cargo fmt --all

lint-rs:
    @tools/rs-bin.sh sf-slots cargo fmt --all --check
    @tools/rs-bin.sh sf-slots cargo clippy --workspace --all-targets --locked -- -D warnings
    @PATH="$(pwd)/apps/symphony-ui/bin:$PATH" tools/rs-bin.sh sf-slots cargo symphony-ui clippy

test-rs *ARGS:
    @tools/rs-bin.sh sf-rust-tests "$@"

symphony-ui-dist OUT="build/symphony-ui" KIND="all" OPT="z":
    #!/usr/bin/env bash
    set -euo pipefail
    out={{quote(OUT)}}
    if [[ "$out" != /* ]]; then out="$PWD/$out"; fi
    PATH="$PWD/apps/symphony-ui/bin:$PATH" tools/rs-bin.sh sf-slots cargo symphony-ui dist "$out" {{quote(KIND)}} {{quote(OPT)}}

symphony-ui-dev PORT="0":
    @PATH="$(pwd)/apps/symphony-ui/bin:$PATH" tools/rs-bin.sh sf-slots cargo symphony-ui dev {{quote(PORT)}}
