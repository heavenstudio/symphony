# Symphony

A local document, review and task workspace with a Rust server, headless-testable browser client,
and project plugins. The server binary embeds its client; running it needs no client compiler.
To put it on a project of your own, see [On your own project](#on-your-own-project); the rest of
this page is about working on Symphony itself.

Install the Rust toolchain named in `rust-toolchain.toml` and `just`, then run:

```sh
just symphony --port 0
```

The server prints its loopback URL. `just symphony-cli` prints the document CLI usage, and
`just tasks --help` lists the board commands. Workspace commands use ordinary Cargo through `tools/cargo.sh`;
they require no containing-project gate executable. See [the workspace guide](AGENTS.md) for
command ownership and [the server guide](apps/symphony-server/README.md) for project configuration.

`just verify` checks formatting, host and wasm clippy, rebuilds the client's modules and program,
compares their paths and bytes with the committed client, then runs the workspace's native tests
and headless Chrome acceptance. The toolchain includes the wasm target. The client build needs
Git, Bash, jq, curl and a C/C++ build toolchain; native board fixtures need lsof. Install Chrome
or name it with `SYMPHONY_CHROME`. Local acceptance requires Chrome explicitly so a missing browser
cannot pass as unexecuted acceptance. No game engine, display server or window is required.
Verification is local; hosted GitHub CI is not required. It is the release run's full verification
of main; a task proves its own change with the tests closest to it ([CHECKS](docs/CHECKS.md)).

After changing SDK source inputs, run `just client-update` and commit
`apps/symphony-server/client/public` with the source. `just client-check` rebuilds into an
independent output directory and refuses changed current asset names, bytes or shell HTML.
The distribution keeps the two builds published before the current one, for pages that deferred
an import across a publication, and the check refuses any build or file beyond them.

Release builds copy verified inputs and the already locked component source into a private,
content-addressed snapshot under `/private/tmp`. Compiler path remapping makes the result independent
of the original checkout and Cargo home, while preserving existing flags, wrappers and resource
settings. Linux build hosts create that parent directory (`sudo install -d -m 1777 /private/tmp`).
Private snapshot patches retain the existing dependency versions and features;
the source workspace's manifests and lockfile stay intact. A publication keeps the two builds
before it whole, and a same-name byte collision refuses it.

Consumers take the libraries from this repository by Git, at a release tag, and commit the selected
revision in their Cargo lockfile. The plugin kit, the helpers and the product (the library as
`symphony-app`, the board, the server, the Chrome driver as `symphony-chrome` and the app's wire
types, `symphony-api`) are made to be published on crates.io by version as well, though none is
published yet ([RELEASES](docs/RELEASES.md#packages-on-cratesio)). A consuming project's host supplies its
plugins and its own verification, approval and landing policy. Symphony supplies no standalone
landing/stamp backend.

## On your own project

A project runs four commands: `symphony`, `tasks`, `agent-send` and `symphony-server`. Their
packages are made to be installed from crates.io with the line
[RELEASES](docs/RELEASES.md#packages-on-cratesio) gives, but none is published yet. Until then,
build them in a clone of this repository:

```sh
cargo build --locked --release --bin symphony --bin tasks --bin agent-send --bin symphony-server
```

They are left in `.target/release/`: put that folder on `PATH`, or link them where `symphony init`
says. Each answers `--version`.

In the project's main checkout, a git repository, run `symphony init`. It installs the agents'
skills, the recipes (`symphony.just`, which the project's justfile imports), the allow rules and
the hooks, and ends with `Next:`, what it cannot do for you: the project file (its tables and keys
are in [PROJECT_FILE](docs/PROJECT_FILE.md)), the documents' folder, the links to the builds, the
SurrealDB the board runs and the project's registration with the machine's server. Do each, then
run `symphony init` again until it says `Next: nothing`.

The first task, by hand: `just tasks register` prints a handle, say `calm-swift`; `just tasks
create ui/first-task "The first task" --as calm-swift` files the task and `just tasks claim
ui/first-task --as calm-swift` takes it. Work on a branch named after the slug (`git switch -c
ui/first-task`) and commit, then post what you did (`just tasks say ui/first-task "TEXT" --as
calm-swift`) and `just tasks move ui/first-task review --as calm-swift`. Someone else reviews it, a
second handle from `just tasks register`, say `ember-sparrow`: `just tasks role ui/first-task
reviewer --as ember-sparrow`, then `just tasks verdict ui/first-task approve "FULL_SHA: why" --as
ember-sparrow`, naming the branch's tip. Then, with the main checkout back on main (`git switch
main`), `just land ui/first-task --as calm-swift` merges the branch, closes the task and pushes main
when there is an `origin`. `just symphony` serves the project's documents and board at the address
it prints.

## License

MIT. Copyright (c) 2026 Stefano Benatti. See [LICENSE](LICENSE).
