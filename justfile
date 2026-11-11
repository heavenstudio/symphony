set positional-arguments

# Independent workspace commands use ordinary Cargo. The containing project keeps its own gates.
fmt:
    @tools/cargo.sh fmt --all

lint-rs:
    @tools/cargo.sh fmt --all --check
    @tools/cargo.sh clippy --workspace --all-targets --locked -- -D warnings
    @PATH="$(pwd)/apps/symphony-ui/bin:$PATH" tools/cargo.sh symphony-ui clippy

test-rs *ARGS:
    @tools/cargo.sh test --locked "$@"

symphony-ui-dist OUT="build/symphony-ui" KIND="all" OPT="z":
    #!/usr/bin/env bash
    set -euo pipefail
    out={{quote(OUT)}}
    if [[ "$out" != /* ]]; then out="$PWD/$out"; fi
    PATH="$PWD/apps/symphony-ui/bin:$PATH" tools/cargo.sh symphony-ui dist "$out" {{quote(KIND)}} {{quote(OPT)}}

symphony-ui-dev PORT="0":
    @PATH="$(pwd)/apps/symphony-ui/bin:$PATH" tools/cargo.sh symphony-ui dev {{quote(PORT)}}

# A server of its own uses --port; the server CLI owns shared-server selection.
symphony *ARGS:
    @tools/rs-bin.sh symphony-server "$@"

# Refresh the actual release assets compiled into the server package.
client-update:
    @just symphony-ui-dist build/shipped-client
    @tools/rs-bin.sh symphony publish-client build/shipped-client/public apps/symphony-server/client/public

# The generic document CLI and board binaries belong to this workspace.
symphony-cli *ARGS:
    @tools/rs-bin.sh symphony "$@"

tasks *ARGS:
    @tools/rs-bin.sh tasks "$@"

agent-send *ARGS:
    @tools/rs-bin.sh agent-send "$@"

# Rebuild the seven real modules and refuse drift from the client embedded at this pin.
client-check OUT="build/verified-client":
    @just symphony-ui-dist {{quote(OUT)}}
    @bash tools/client-check.sh {{quote(OUT)}}/public

# The native browser harness runs headless Chrome; no game engine or display is needed.
verify: lint-rs client-check
    @SF_SYMPHONY_UI_DIST="$PWD/build/verified-client/public" tools/cargo.sh test --workspace --locked
