# Symphony

The app's workspace owns its server and Dioxus client under `apps/`, and the review
library, wire types, task board and working-directory scanner under `crates/`.
The app's formats and operating contract live in [docs/](docs/README.md).
The containing project's [guide](../AGENTS.md) owns landing and shared-service rules.

## Rust

`Cargo.toml` and `Cargo.lock` belong to this workspace. Members inherit its package
settings and dependency versions. The containing project's thin host and board-only
packages consume these libraries through path dependencies. A project's settings are
its `symphony.toml` (generic defaults in `crates/symphony/src/project.rs`), and its
game code is a plugin: the traits and block vocabulary in `crates/symphony-plugin`,
Star Foundry's implementation in `crates/symphony/src/star_foundry/`, selected by
the manifest's `[plugins] crates`. A plugin renders to generic blocks, never HTML,
and the client draws each block with one component. Only the plugins a project
selects answer for it: its card kinds, watches, pages and actions. A document is
named `docs/…` in every route, link and on the wire whatever its folder is called;
its reads, saves and commits name the file `Project::file` maps it to, and the
walks read the project's documents, history and artifacts folders.

Use `just lint-rs`, `just test-rs`, `just symphony-ui-dist` and `just symphony-ui-dev`
from this folder. `tools/rs-bin.sh` delegates to the containing project's source
launcher and slot wrapper. Each workspace builds in its own `.target/`; the root
gates compile, discover and run both sets of tests. Never run a shared-service
restart as a test: use a private port and a scratch board.

Read [the client guide](apps/symphony-ui/AGENTS.md) and
[the board guide](crates/tasks/AGENTS.md) before editing those packages. The Rust
architecture and test rules in the containing project's guide apply here too.
