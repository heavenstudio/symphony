# Symphony

A local document, review and task workspace with a Rust server, headless-testable browser client,
and project plugins. The server binary embeds its client; running it needs no client compiler.

Install the Rust toolchain named in `rust-toolchain.toml` and `just`, then run:

```sh
just symphony --port 0
```

The server prints its loopback URL. `just symphony-cli` prints the document CLI usage, and
`just tasks --help` lists the board commands. Workspace commands use ordinary Cargo through `tools/cargo.sh`;
they require no containing-project gate executable. See [the workspace guide](AGENTS.md) for
command ownership and [the server guide](apps/symphony-server/README.md) for project configuration.

`just verify` checks formatting, host and wasm clippy, rebuilds all eight real client modules,
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

Consumers take the libraries at a release tag of this repository and commit its revision in their
Cargo lockfile ([RELEASES](docs/RELEASES.md)); the plugin kit and the helpers are made to be
published on crates.io by version as well
([RELEASES](docs/RELEASES.md#packages-on-cratesio)). A consuming project's host supplies its plugins and its own verification,
approval and landing policy. Symphony supplies no standalone landing/stamp backend.

## License

MIT. Copyright (c) 2026 Stefano Benatti. See [LICENSE](LICENSE).
