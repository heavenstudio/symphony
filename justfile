set positional-arguments
# The Symphony's recipes as `symphony init` installs them in a project; the ones below of the same
# name win, building this workspace's binaries before they run.
set allow-duplicate-recipes

import 'symphony.just'

# A recipe that builds or tests runs as one operation through tools/record.sh, which records it
# for the board's Build and Test columns when a recorder answers (docs/MACHINE_WORK.md), as a run
# of the pipeline it names (docs/RUNS.md); inside it, a recipe recorded with a --step is a step of
# that run, and any other command runs unrecorded. TASKS_SESSION=HANDLE names the session whose
# task it is, and TASKS_TASK=SLUG the task itself. A tree without tools/record.sh runs the command
# as it is: no recorder, present or not, fails a recipe.
record := if path_exists(justfile_directory() / "tools/record.sh") == "true" { "tools/record.sh" } else { "sh -c 'while [ \"$1\" = --pipeline ] || [ \"$1\" = --step ]; do shift 2; done; shift 2; exec \"$@\"' unrecorded" }

# Independent workspace commands use ordinary Cargo. The containing project keeps its own gates.
fmt:
    @tools/cargo.sh fmt --all

lint-rs:
    @{{record}} --pipeline lint --step lint build "just lint-rs" just --justfile {{quote(justfile())}} _lint-rs

[private]
_lint-rs:
    @tools/cargo.sh fmt --all --check
    @tools/cargo.sh clippy --workspace --all-targets --locked -- -D warnings
    @PATH="$(pwd)/apps/symphony-ui/bin:$PATH" tools/cargo.sh symphony-ui clippy

test-rs *ARGS:
    @{{record}} --pipeline test test "just test-rs $*" tools/cargo.sh test --locked "$@"

symphony-ui-dist OUT="build/symphony-ui" KIND="all" OPT="z":
    #!/usr/bin/env bash
    set -euo pipefail
    out={{quote(OUT)}}
    if [[ "$out" != /* ]]; then out="$PWD/$out"; fi
    PATH="$PWD/apps/symphony-ui/bin:$PATH" tools/cargo.sh symphony-ui dist "$out" {{quote(KIND)}} {{quote(OPT)}}

symphony-ui-dev PORT="0":
    @PATH="$(pwd)/apps/symphony-ui/bin:$PATH" tools/cargo.sh symphony-ui dev {{quote(PORT)}}

# A server of its own uses --port; the server CLI owns shared-server selection. These commands
# say their own errors, with no recipe-failed line after them.
[no-exit-message]
symphony *ARGS:
    @tools/rs-bin.sh symphony-server "$@"

# Refresh the actual release assets compiled into the server package.
client-update:
    @{{record}} --pipeline client build "just client-update" just --justfile {{quote(justfile())}} _client-update

[private]
_client-update:
    @just symphony-ui-dist build/shipped-client
    @tools/rs-bin.sh symphony publish-client build/shipped-client/public apps/symphony-server/client/public

# The generic document CLI and board binaries belong to this workspace.
[no-exit-message]
symphony-cli *ARGS:
    @tools/rs-bin.sh symphony "$@"

[no-exit-message]
tasks *ARGS:
    @tools/rs-bin.sh tasks "$@"

[no-exit-message]
agent-send *ARGS:
    @tools/rs-bin.sh agent-send "$@"

# Rebuild the seven real modules and refuse drift from the client embedded at this pin.
client-check OUT="build/verified-client":
    @{{record}} --pipeline client --step client-check build "just client-check" just --justfile {{quote(justfile())}} _client-check {{quote(OUT)}}

[private]
_client-check OUT:
    @just symphony-ui-dist {{quote(OUT)}}
    @bash tools/client-check.sh {{quote(OUT)}}/public

# The native browser harness runs headless Chrome; no game engine or display is needed.
verify:
    @{{record}} --pipeline verify test "just verify" just --justfile {{quote(justfile())}} _verify

[private]
_verify: lint-rs client-check
    @SF_SYMPHONY_UI_DIST="$PWD/build/verified-client/public" {{record}} --step tests test "tests" tools/cargo.sh test --workspace --locked --no-fail-fast
