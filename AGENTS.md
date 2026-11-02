# Symphony

The app's workspace owns its server and Dioxus client under `apps/`, and the review
library, wire types, task board and working-directory scanner under `crates/`.
The app's formats and operating contract live in [docs/](docs/README.md).
The containing project's [guide](../AGENTS.md) owns landing and shared-service rules.

## Rust

`Cargo.toml` and `Cargo.lock` belong to this workspace. Members inherit its package
settings and dependency versions. The containing project's thin host and board-only
packages consume these libraries through path dependencies; game-specific modules
remain here until the plugin boundary is cut.

Use `just lint-rs`, `just test-rs`, `just symphony-ui-dist` and `just symphony-ui-dev`
from this folder. `tools/rs-bin.sh` delegates to the containing project's source
launcher and slot wrapper. Each workspace builds in its own `.target/`; the root
gates compile, discover and run both sets of tests. Never run a shared-service
restart as a test: use a private port and a scratch board.

Read [the client guide](apps/symphony-ui/AGENTS.md) and
[the board guide](crates/tasks/AGENTS.md) before editing those packages. The Rust
architecture and test rules in the containing project's guide apply here too.
