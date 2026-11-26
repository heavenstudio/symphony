set positional-arguments
# The Symphony's recipes as `symphony init` installs them in a project, `land` among them; the ones
# below of the same name win (symphony.just sets `allow-duplicate-recipes`, which just refuses
# twice), building this workspace's binaries before they run.

import 'symphony.just'

# A recipe that builds or tests runs as one operation through tools/record.sh, which records it
# for the board's Build and Test columns when a recorder answers (docs/workflow/MACHINE_WORK.md), as a run
# of the pipeline it names (docs/workflow/RUNS.md); inside it, a recipe recorded with a --step is a step of
# that run, and any other command runs unrecorded. TASKS_SESSION=HANDLE names the session whose
# task it is, and TASKS_TASK=SLUG the task itself. A tree without tools/record.sh runs the command
# as it is: no recorder, present or not, fails a recipe.
record := if path_exists(justfile_directory() / "tools/record.sh") == "true" { "tools/record.sh" } else { "sh -c 'while [ \"$1\" = --pipeline ] || [ \"$1\" = --step ]; do shift 2; done; shift 2; exec \"$@\"' unrecorded" }

# Independent workspace commands use ordinary Cargo. The containing project keeps its own gates.
fmt:
    @tools/cargo.sh fmt --all

# After clippy it runs the board tests' raw-statement fence (crates/tasks/tests/raw_fence.rs), a
# target of its own that reads text and starts nothing: a branch runs the targets of the tests it
# touched, and the fence judges the files of every board target, so every branch's lint runs it.
# Its last Rust step reads the branch against the default branch: red when it changes what a
# consumer must do (the board schema, a consumer-API guard's call, a settings key) and no commit of
# it says so in a `Consumer-Note` trailer (docs/workflow/RELEASES.md, "Its notes"). The step after it reads
# the branch the same way for the published packages it changes, and runs the crates check on
# those alone: red when one's sources moved while crates.io holds its version (docs/workflow/RELEASES.md,
# "Packages on crates.io"). A branch that changes none reads nothing, and a crates.io that gives no
# answer is said, the packages named as not checked, and no red.
lint-rs:
    @{{record}} --pipeline lint --step lint build "just lint-rs" just --justfile {{quote(justfile())}} _lint-rs

[private]
_lint-rs:
    @tools/cargo.sh fmt --all --check
    @tools/cargo.sh clippy --workspace --all-targets --locked -- -D warnings
    @tools/cargo.sh clippy -p symphony-api --features typescript --all-targets --locked -- -D warnings
    @tools/cargo.sh clippy -p symphony-board --features hosted-postgresql --all-targets --locked -- -D warnings
    @tools/cargo.sh test -p symphony-board --test raw_fence --locked
    @tools/rs-bin.sh symphony-build consumer-notes --check
    @tools/rs-bin.sh symphony-build crates-check --branch .
    @just --justfile {{quote(justfile())}} client-lint

# The web client's checks (apps/symphony-web/check.sh): its wire types against crates/symphony-api's
# and the tests of the example that writes them, Biome's lint and format and TypeScript's types,
# every warning an error, and its unit tests. Part of `just lint-rs`.
client-lint:
    @{{record}} --pipeline lint --step client-lint build "just client-lint" bash apps/symphony-web/check.sh

# Write the web client's wire types (apps/symphony-web/src/wire.ts) and the cases its tests hold its
# TypeScript to again, from crates/symphony-api.
client-wire:
    @bash apps/symphony-web/wire.sh

# No test reads this machine's terminals: a test that names no hc of its own (HC_BIN) is given one
# that answers nothing, as on a machine with none, so a terminal here named as a fixture's session
# is never taken for its pane. The recorder keeps the caller's.
test-rs *ARGS:
    @{{record}} --pipeline test test "just test-rs $*" env HC_BIN="${HC_BIN:-/usr/bin/false}" tools/cargo.sh test --locked "$@"

# Install the release test runner once. Test runs only use this pinned local copy and never fetch it.
nextest-setup:
    @bash tools/nextest.sh --setup

# The settings check (`symphony settings-check`, docs/operations/PROJECT_FILE.md): symphony.toml read as the
# Symphony reads it and judged on its shape alone, recorded as a test run, which is what a change
# of settings' values lands by in a reviewer's place (docs/workflow/TASKS.md, "Needed stamps").
settings-check:
    @{{record}} --pipeline settings test "just settings-check" tools/rs-bin.sh symphony settings-check

# The client's build (apps/symphony-web/build.sh): one Vite build of the web client and its shell,
# published into OUT/public, OUT named from this folder.
symphony-ui-dist OUT="build/symphony-ui":
    @{{record}} --pipeline client build "just symphony-ui-dist" bash apps/symphony-web/build.sh {{quote(OUT)}}

# apps/symphony-web/dev.ts (its guide's "The dev server"), on loopback port PORT (0 for any), over
# a server of your own (`just symphony --port 0`) on port SERVER; 8765 it refuses as either.
# The web client's dev server: its kinds from their sources, the page loaded again on each save.
[no-exit-message]
symphony-ui-dev SERVER PORT="0":
    @bash apps/symphony-web/npm.sh run --silent dev -- {{quote(SERVER)}} {{quote(PORT)}}

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
    @tools/rs-bin.sh symphony-build publish-client build/shipped-client/public apps/symphony-server/client/public

# The generic document CLI and board binaries belong to this workspace. The docs checks a doc's
# evidence names, `docs-audit --check` and `status --lint` as typed, are each a test run of the
# pipeline `docs` through tools/record.sh, so a task's recorded runs at its tip hold them and the
# task page's check gate reads the docs audit, the project's [verdicts] check; every other command
# runs as it is.
[no-exit-message]
symphony-cli *ARGS:
    @{{ if ARGS =~ '^(docs-audit --check|status --lint)$' { record + ' --pipeline docs test "just symphony-cli $*"' } else { '' } }} tools/rs-bin.sh symphony "$@"

# Both document checks with fixed arguments, each recorded by symphony-cli. The no-argument
# recipe lets the project's Codex policy permit these checks without permitting extra CLI flags.
docs-check:
    @just --justfile {{quote(justfile())}} symphony-cli docs-audit --check
    @just --justfile {{quote(justfile())}} symphony-cli status --lint

# The board but its recorder, which the Symphony's recipe refuses too (`_symphony_board`): a build
# records itself by the binary, through tools/record.sh. Like the Symphony's, it writes only where
# `_symphony_unasked` admits (its checkout, from a worktree main's artifacts), as an agent runs it
# unasked.
[no-exit-message]
tasks *ARGS:
    @{{ _symphony_board }}{{ _symphony_unasked }}tools/rs-bin.sh tasks "$@"

# Like the Symphony's, it sets no marker, and an agent's run of it asks first, as no allow rule
# passes it.
# A change to a board's people or a profile, on this workspace's build of the board.
[no-exit-message]
tasks-people *ARGS:
    @tools/rs-bin.sh tasks people "$@"

# Symphony's releases (docs/workflow/RELEASES.md), a release run: main's tip, fetched, verified in full
# (`just verify`) in a clean checkout outside this folder, building and testing nothing of the
# projects this machine serves; each red check filed on the board, a green tip tagged
# release-YYYY-MM-DD and that tag pushed. `--commit COMMIT` releases a commit of main out of cycle,
# `--status` runs nothing, and `--schedule` prints the entry that runs it each day. Scheduling is
# the owner's. The board's binary is built and linked first: the run files its reds and records
# its checks through it. So is `symphony-build`, whose link `[board] on_land` runs (symphony.toml):
# any `just release`, even `--status`, makes that link or renews it.
[no-exit-message]
release *ARGS:
    @tools/rs-bin.sh --link tasks >/dev/null
    @tools/rs-bin.sh --link symphony-build >/dev/null
    @tools/rs-bin.sh symphony-build release "$@"

[no-exit-message]
agent-send *ARGS:
    @tools/rs-bin.sh agent-send "$@"

# Rebuild the client and refuse drift from the client embedded at this pin.
client-check OUT="build/verified-client":
    @{{record}} --pipeline client --step client-check build "just client-check" just --justfile {{quote(justfile())}} _client-check {{quote(OUT)}}

[private]
_client-check OUT:
    @just symphony-ui-dist {{quote(OUT)}}
    @bash tools/client-check.sh {{quote(OUT)}}/public

# The shipped binaries, built as `cargo install` builds them (the release profile), and the client
# the server embeds, each against its line in tools/size-budget.toml (docs/workflow/CHECKS.md, "Size
# budgets"). The release run's verify runs it: one release build, which no task's gate pays for.
size-check:
    @{{record}} --pipeline size --step size-check build "just size-check" just --justfile {{quote(justfile())}} _size-check

[private]
_size-check:
    @tools/cargo.sh build --release --locked -p symphony-server -p symphony-app -p symphony-board --bin symphony-server --bin symphony --bin tasks --bin agent-send
    @tools/rs-bin.sh symphony-build size-budget . "${CARGO_TARGET_DIR:-.target}/release"

# Each published package whose version crates.io holds, packaged here and compared with what
# crates.io holds of that version: red, naming the package, where the sources moved and the version
# did not (docs/workflow/RELEASES.md, "Packages on crates.io"). It reads crates.io and publishes nothing.
crates-check:
    @{{record}} --pipeline crates test "just crates-check" tools/rs-bin.sh symphony-build crates-check .

# Each kit/helper API against its last published version, with its actual version bump and all
# features. The launcher fetches the checksum-pinned tool into this checkout's build folder.
semver-check:
    @{{record}} --pipeline semver test "just semver-check" bash tools/semver.sh --all-features -p symphony-plugin -p symphony-blocks -p symphony-docs -p symphony-env -p symphony-args -p symphony-schema -p process-cwd -p repository-read

# The hosted board's build (docs/operations/POSTGRESQL.md, "A hosted board"), which no default build has: the
# board's own tests with the `hosted-postgresql` feature and its matrix against PostgreSQL 16 and
# 18 servers of its own (crates/tasks/tests/hosted), red where either is not installed.
hosted-check:
    @{{record}} --pipeline hosted test "just hosted-check" tools/cargo.sh test --locked -p symphony-board --features hosted-postgresql --lib --test hosted

# The full verification (tools/verify.sh): lint, the client check, the size budgets, the docs audit,
# the crates and semver checks, the hosted board's check and every test target, each a step with a log of its own, run
# whatever the ones before it did; red when any step is, naming each. `just verify STEP...` runs
# those steps alone. The native browser harness runs headless Chrome; no game engine or display is
# needed.
verify *STEPS:
    @{{record}} --pipeline verify test "just verify${*:+ $*}" just --justfile {{quote(justfile())}} _verify "$@"

[private]
_verify *STEPS:
    @bash tools/verify.sh {{quote(justfile())}} "$@"
