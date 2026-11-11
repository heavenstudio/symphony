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

`just verify` checks formatting, host and wasm clippy, rebuilds all seven real client modules,
compares their paths and bytes with the committed client, then runs the workspace's native tests
and headless Chrome acceptance. The toolchain includes the wasm target. The client build needs
Git, Bash, jq, curl and a C/C++ build toolchain; native board fixtures need lsof. Install Chrome
or name it with `SF_CHROME`. Local acceptance requires Chrome explicitly so a missing browser
cannot pass as unexecuted acceptance. No game engine, display server or window is required.
Verification is local; hosted GitHub CI is not required.

After changing SDK source inputs, run `just client-update` and commit
`apps/symphony-server/client/public` with the source. `just client-check` rebuilds into an
independent output directory and refuses changed current asset names, bytes or shell HTML.
Older immutable assets may remain for pages that deferred an import across a publication.

Release builds copy verified inputs and the already locked component source into a private,
content-addressed snapshot under `/private/tmp`. Compiler path remapping makes the result independent
of the original checkout and Cargo home, while preserving existing flags, wrappers and resource
settings. Linux build hosts create that parent directory (`sudo install -d -m 1777 /private/tmp`).
Private snapshot patches retain the existing dependency versions and features;
the source workspace's manifests and lockfile stay intact. Old immutable assets are retained,
and a same-name byte collision refuses publication.

Consumers take the libraries from this repository's `main` branch and commit the selected revision
in their Cargo lockfile. A consuming project's host supplies its plugins and its own verification,
approval and landing policy. Symphony supplies no standalone landing/stamp backend.
