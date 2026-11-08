# Symphony

The app's workspace owns its server and Dioxus client under `apps/`, and the review
library, wire types, task board and working-directory scanner under `crates/`.
The app's formats and operating contract live in [docs/](docs/README.md).
The containing project's [guide](../AGENTS.md) owns landing and shared-service rules.

## Rust

`Cargo.toml` and `Cargo.lock` belong to this workspace. Members inherit its package
settings and dependency versions. The containing project's thin host and board-only
packages consume these libraries through path dependencies. A project's settings are
its `symphony.toml` (generic defaults in `crates/symphony/src/project.rs`; the
board itself reads `[board] on_review`, the command a task moving to
review starts detached from the main checkout, in `crates/tasks/src/on_review.rs`), and its
own code is a plugin: the traits and block vocabulary in `crates/symphony-plugin`,
the implementation in a crate of the project's that its host links, selected by
the manifest's `[plugins] crates`. A plugin renders to generic blocks, never HTML,
and the client draws each block with one component. Only the plugins a project
selects answer for it: its card kinds, watches, pages and actions. A project's
documents are served from its `[[entrypoints]]` (`crates/symphony/src/entrypoint.rs`;
none written is `/docs/` over `docs/`). A document is named by its route, its
entrypoint's URL and its path in the folder (`docs/…` by the convention), in every
link and on the wire; its reads, saves and commits name the file `Project::file`
maps it to, and every walk goes through `Project::walk`, so what an entrypoint's
patterns and renderers leave out is never listed or read. The Symphony's own routes
(`symphony_api::SYMPHONY_ROUTES`, which `assets/shell.js` repeats) win over every
entrypoint.

One server serves every project the machine folder's registry holds
(`crates/symphony/src/machine/registry.rs`: `projects.json` beside the server's record,
in `SYMPHONY_MACHINE_DIR` when set), each under `/<id>/` with its own watcher, feed,
board snapshot and branch scan. A project's state stays in its checkout: paths under
its artifacts folder go through `Project::artifact`, never an `artifacts/` literal.
The projects share the server's origin, so the client keys what it keeps in the
browser by the project's id (`project::stored` in `apps/symphony-ui`).
The host rule (`crates/symphony/src/machine/host.rs`) decides whether a start's build
serves, waits for or replaces the running one. A project's look is its theme
package (`crates/symphony/src/theme.rs`), served as `/<id>/theme.css` and inlined in
its pages. Tests use a temporary machine folder.

Use `just lint-rs`, `just test-rs`, `just symphony-ui-dist` and `just symphony-ui-dev`
from this folder. `tools/rs-bin.sh` delegates to the containing project's source
launcher and slot wrapper. Each workspace builds in its own `.target/`; the root
gates compile, discover and run both sets of tests. Never run a shared-service
restart as a test: use a private port and a scratch board.

Read [the client guide](apps/symphony-ui/AGENTS.md) and
[the board guide](crates/tasks/AGENTS.md) before editing those packages. The Rust
architecture and test rules in the containing project's guide apply here too.
