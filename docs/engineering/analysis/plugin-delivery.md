# Delivering Symphony without rebuilds

How a project takes a Symphony release without compiling Symphony, with its own plugin code
delivered apart from Symphony's, and how Symphony's crates, binaries and client split so that a
change rebuilds and redistributes only what it touches. The path is chosen, on three cards in
[DECISIONS](../../DECISIONS.md). This is a design: nothing here is built yet.

- **The path:** each project's plugin becomes a program of its own that the server starts and
  talks to in JSON lines (option B), and one Symphony release per machine, built here by the
  nightly and installed by one command, serves every project; an update then compiles nothing of a
  project's. Hosted builds that the update downloads are the later direction, not yet.
- **First slice, an estimated 2 to 3 days:** measure today's approach (an update end to end, what
  it rebuilds and redistributes, each plugin call's cost), then what JSON adds to each call, by
  sending the plugin's calls through a JSON round trip inside today's server. Every later slice is
  compared with those numbers; B goes on only if the JSON cost is small.
- **Board schemas** move at the update, each board only once every program that writes it is on
  the release's schema: an improvement on moving every board at once, while there is no dedicated
  build service.
- **Not measured yet:** all of the first slice's numbers, how long each kind of change takes to
  rebuild, and one client build's size.

The ask, after the consuming project's pin had to be moved and its host rebuilt to bring the app up
to date: today's approach makes a project build the board often just for it to work. Updates should
be more automatic, with a project's own code kept apart from the board's, so that an update of the
binaries needs no full rebuild. The problem is one of Rust's crates and build architecture, around a
server and a client with plugins: the crates should be separate and the binaries smaller, so that
things are distributed quickly and with smaller builds, and only what needs rebuilding is rebuilt
and distributed. 30 MB is still very big for a binary.

How facts are marked: a fact about the code names the file and function it was read in; a
measurement says how it was taken; a fact from outside names its source by URL beside it, with the
words quoted where they could be disputed; **Not verified** marks what could not be checked. "Read
2026-11-12" is the state of this machine that day: Symphony's main at `c760118`, the consuming project's
checkout read in place and changed in nothing.

![Today: each project compiles Symphony into binaries of its own](plugin-delivery/today.svg)

## What an update costs today

- **The pin.** The consuming project names ten Symphony crates in its `Cargo.toml` on `branch = "main"`,
  and its `Cargo.lock` binds the commit (read 2026-11-12: `19ea97f`, 29 commits behind main). It
  moved the pin four times in 19 hours, from 2026-11-11 12:30 to 2026-11-12 07:18 (its `git log`
  of `Cargo.lock`); two of the four needed code changes in the consuming project before they built.
- **The last move, 2026-11-12.** From the consuming project's machine-work records: the host server built
  in the pin's worktree at 07:00, the host and plugin tests ran red at 07:15 (exit 101), the pin
  and its two test fixes were committed at 07:18, and `cargo install` of the server ran four times
  on main from 07:33 to 07:35. The installed `symphony-server` is 295,428,896 bytes, written at
  07:33:53 (its `installed.json`). The orchestrator measured the whole move at about 30 minutes at
  load ~110 on 16 cores: a debug build of 10 to 15 minutes, the tests, a commit hook's workspace
  lint of about 2 minutes a pass, and a release install of about 11 minutes.
- **The sizes, today.** The consuming project's installed `symphony-server` is 295,428,896 bytes. About
  266 MB of it is the embedded client distribution: `apps/symphony-server/client/public` is 266 MB
  in 1,587 files, 322 of them `.wasm`, every build ever published (measured on main, `du` and
  `find`). The build script embeds every file (`apps/symphony-server/build.rs`, `generate`; served
  by `client.rs`, `asset`). The rest, about 30 MB, is an estimate by subtraction, not a measurement
  of the code. The consuming project's other builds, linked at its `.target/bin/` and built by its
  `tools/rs-bin.sh` with `cargo build --release --locked` on Cargo's default release profile, are
  `tasks` 31,675,888 bytes, `symphony` 22,668,544 and `agent-send` 5,601,824 (`stat`, read
  2026-11-12). The board task `perf/client-distribution-keeps-every-build` bounds what the
  distribution keeps, and `perf/binary-size-budget` measures where each binary's bytes are and adds
  a size check; neither had posted a measurement by 10:05 on 2026-11-12, so every size below that
  depends on them is marked as an estimate.
- **The host rule.** The machine runs one server, and it must compile every registered project's
  plugins (`crates/symphony/src/machine/host.rs`, `judge`). "A project with no plugins never
  replaces a running host and is served by any host holding its pin" (the module's doc), so
  Symphony's own project is served by the consuming project's build, at the consuming project's pin: the machine's
  `server.json` names the consuming project's checkout. A second project with plugins of its own could be
  served beside the consuming project only by one build that links both projects' plugins, which neither
  project builds today.
- **The board.** The consuming project builds its own `tasks` and `agent-send`, though neither holds a line
  of its code: its `tasks` is `symphony_board::cli::process_main()` and nothing else, its
  `agent-send` the board's `agent_send::main` with a list of variables (`apps/tasks/src/bin/`). A
  build writes only a board at its own schema, an older writer is refused, and a reader one
  version behind still reads (`crates/tasks/src/schema.rs`, `decide`). Every board command opens
  its board to write (`crates/tasks/src/cli/mod.rs`, `open`), so one behind its board's schema is
  refused outright, reads included. A consuming project's live board moves forward only by `just
  tasks migrate` in its main checkout (`crates/tasks/src/live.rs`, `may_move`). The schema went
  from 9 on 2026-11-02 to 16 on 2026-11-11, and to 17 when the owner's asks landed on main at
  09:28 on 2026-11-12 (`git log -G` of `VERSION`).

## Where the coupling is

### What a project compiles against

the consuming project's `Cargo.toml` names ten Symphony crates. Counted in its `apps/`, `tools/` and
`crates/`, tests included, by the Rust files that name each crate (`grep -w` of the crate's Rust
name, the library's by `symphony::`; read 2026-11-12 at the consuming project's `62d91ce93`):

| Symphony crate | What the consuming project uses | Files |
|---|---|---|
| `symphony-plugin` | the `Plugin` trait and 11 of its 16 seams | 18 |
| `symphony-api` | the block vocabulary and the seams' data (`Block`, `TaskOffer`, `NavItem`, …) | 13 |
| `symphony` | the plugin: text helpers, register parsing (`parse_decisions`, `Decision`), schema validation, `markup::escape`, `images::reserve` and `images::sips_available`, `edit::heading_anchor`, `children::command`, `blocks::vector_shapes`, `project::Project`; the host: `cli::main`, `plugins::install`; its tools: `landing`, `register`, `verdicts`, `taskcli`, `clock`, `commit` | 45 |
| `symphony-server` | `binary::run`, `development::main` | 7 |
| `symphony-board` | its `tasks` and `agent-send` (above); its review gate links `sys`, `sweep`, `red`, `assessment_publication`, `slots`, `on_review`, `alarm`, `rpc`, `port`, and its nightly opens the board to write (`tools/gate-review/src/nightly/board.rs`, `Store::open`) | 73, 57 of them in `tools/gate-review` |
| `symphony-docs` | `DocCheck`, `Docs`, `Problem` | 15 |
| `process-cwd` | its child-process helpers, everywhere | 132 |
| `symphony-schema` | JSON Schema validation (`validate`, `Schema`, `text::repr`), in its content tools and in `crates/relationships`, which the plugin links | 7 |
| `repository-read` | the repository reader its gate's audits use (`Reader`, `Reader::process`) | 7 |
| `symphony-args` | argument parsing in two of its tools (`parse`, `Flag`) | 2 |

The seams are data in and data out by design: "Each seam below takes and returns plain data
(paths, text and the wire types of `symphony-api` …), never a Symphony library type, so a sidecar
adapter (a process answering a line protocol) can implement any of them later"
(`crates/symphony-plugin/src/lib.rs`, the crate's doc). The block vocabulary is already serde
JSON, the client's wire format (`crates/symphony-api/src/blocks.rs`). The exceptions are
`Page::once`, a per-render cache of `Any` values, and `Watch::changed`, which tells the plugin to
drop a cache kept in the server's memory.

### What changed in the last twenty landings

The last twenty merges on main, from `461d9ead` (2026-11-12 02:21) to `329c9864` (08:12), six
hours (`git log --first-parent --merges`, each merge's diff against its first parent):

- **The seams**: no change. The two in the week before were both a new method with a default (the
  gates seam on 2026-11-11, `Command::summary` on 2026-11-12), which breaks no plugin.
- **The block vocabulary**: no change since 2026-11-05.
- **`symphony-api`**: changed by 4 of 20, every change an addition: the task timeline's types,
  the page names, `GatesSeen` with a field `gates` on a wire struct, a field `under` on a machine
  work struct. A field added to a public struct breaks a consumer that builds the struct by a
  literal, and no other.
- **Public items of the library, the board and the server**: changed by 7 of 20 (93, 39, 19,
  16, 10, 8 and 3 changed `pub` lines, tests left out).
- **The board schema and the pin**: no change.

What did break the consuming project, in its last four pin moves, was all in its tests of Symphony's
internals, never the seams: `symphony::verdicts::BoardTask` gained `links` (a struct literal in
a tool's test), `docview::folder` took a new argument (`tools/symphony`'s renderer test), and
the CLI's usage text changed (an exact-text test). So the seams are stable; what breaks is linking
the library's internals. And with nothing broken at all, every update is still a compile of the
server, the CLIs and the plugin, an install and a restart.

![The crates a consumer compiles, and what a change rebuilds](plugin-delivery/crates.svg)

### What a project customises

the consuming project's plugin (`tools/symphony/src/lib.rs`), seam by seam:

| Seam | What it adds | How it runs |
|---|---|---|
| components | 14: an entry, the catalogue, pictures, screen, a matrix, shape-matrix, a group listing, relationships, five icon families, the model gallery | reads the game's data under the checkout, renders blocks, gives a cache version per page; used 726 times in its docs |
| card kinds | 4: model, picture, golden frame, mentions | reads a card's text and thread: pictures, slot, markers, triage category, what it gates, the letter chosen; 842 cards in 39 registers |
| renderers | its 3D model files | draws a viewer, a custom element whose script is a file of the project |
| routes | `/x/PAGE/ID.EXT`, a model by its id | answers a file from the models folder |
| nav | "⬡" and its page's name | a fixed entry |
| home regions | the pictures' progress | a region computed from the checkout |
| board actions | approve to land | already out of process: it runs the project's landing tool's `api` with JSON on stdin (`land_call`) |
| commands | `relationships`, `screens` and one that draws pictures | `symphony NAME ARGS`, writing to stdout and stderr |
| watches | the gallery's models, concepts and outlines folders | drops and warms a cache in the server's memory |
| theme | the name and the icon | fixed data |
| doc checks | the design data, the group pages, the screen catalogue | read `docs/` and return problems |

It uses none of board fields, gates, prose rules, generators or transports. Its generators are
configuration already: `[verdicts] generators` in its `symphony.toml`, with its check, land
command, transports, worktrees, launch defaults, review command, slots and alarm. Its landing is
its own `just land` family of recipes.

## How Rust programs deliver plugins without relinking

### Out of process, over a versioned protocol

The plugin is a program of its own; the host starts it and talks to it over pipes or a socket.

- **Language servers.** JSON-RPC behind a `Content-Length` header, over stdio, a pipe or a
  socket; capabilities are exchanged once in `initialize`, and a reader "should ignore" properties
  it does not know, which is how the protocol stays compatible
  ([LSP 3.17](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)).
  A crashed server is restarted, within a limit: "VS Code, for example, doesn't restart a server
  which has crashed 5 times in the last 180 seconds" (same page).
- **Terraform providers** (HashiCorp's go-plugin). The plugin prints one line naming its protocol
  versions and a local address, and the host picks a version both support
  ([internals](https://raw.githubusercontent.com/hashicorp/go-plugin/main/docs/internals.md),
  [go-plugin](https://pkg.go.dev/github.com/hashicorp/go-plugin)). "Plugins can't crash your host
  process: A panic in a plugin doesn't panic the plugin user"
  ([go-plugin](https://github.com/hashicorp/go-plugin)). Terraform's protocol 5 serves Terraform
  0.12 on and 6 serves 1.0 on, and "Minor versions of the protocol are additive"
  ([protocol](https://developer.hashicorp.com/terraform/plugin/terraform-plugin-protocol)).
- **Nushell plugins.** Stdio, JSON or MessagePack; a plugin's version "must be semver compatible
  with the engine's version", and since Nushell is 0.x every minor release breaks every plugin
  ([protocol](https://www.nushell.sh/contributor-book/plugin_protocol_reference.html)): "they do
  need to be recompiled with the latest version of `nu-plugin`"
  ([0.91](https://www.nushell.sh/blog/2024-03-05-nushell_0_91_0.html)). Idle plugins are stopped
  after 10 s by default ([plugins](https://www.nushell.sh/book/plugins.html)). The lesson: a
  strict version rule brings the rebuild back.
- **mdBook preprocessors**, the closest to Symphony's case: a command named in `book.toml`, asked
  first `supports <renderer>`, then handed the book as JSON on stdin and answering it on stdout
  ([preprocessors](https://rust-lang.github.io/mdBook/for_developers/preprocessors.html)). A version
  mismatch only prints a warning
  ([example](https://raw.githubusercontent.com/rust-lang/mdBook/master/examples/nop-preprocessor.rs)).
- **Cargo's external commands.** `cargo foo` runs `cargo-foo` from `PATH` with its arguments
  unchanged, and Cargo advises against linking Cargo as a library because it "is unstable: the API
  may change without deprecation" ([external tools](https://doc.rust-lang.org/cargo/reference/external-tools.html)),
  the trade the consuming project's tests met.
- **Neovim** runs remote plugins over MessagePack-RPC and states its API's level and the oldest it
  is compatible with ([API](https://neovim.io/doc/user/api/)).
- **Helix** has not chosen this: its plugin work is a Scheme, Steel, still an experimental draft
  ([PR 8675](https://github.com/helix-editor/helix/pull/8675)); a maintainer wrote of WebAssembly
  that "we had three separate attempts at integrating WASM that didn't lead anywhere"
  ([discussion 3806](https://github.com/helix-editor/helix/discussions/3806)). **Not verified:** that
  it turned RPC down as too slow; only a third party's summary on that page says so.

**Costs.** A call is a round trip through the kernel. One old benchmark reports throughput, not
latency: 130,372 messages a second on Unix domain sockets and 162,441 on pipes, 100-byte messages
sent back and forth in turn, on an Intel i5-4590S
([ipc-bench](https://github.com/goldsborough/ipc-bench)). That is 6 to 8 µs a message, a figure
derived here from the throughput, before any encoding; whether its count takes a round trip as one
message or two, the page does not say. None of the projects above publishes a figure. A process
per call is slow to start, which is why
Bazel keeps its workers running, 2 to 4 times faster on Java
([persistent workers](https://bazel.build/remote/persistent)). The host needs a handshake, a
timeout per call, a restart and a kill (go-plugin's `Kill` and `CleanupClients`). A plugin runs
with the user's rights: no sandbox, but no shared memory with the host either.

### WebAssembly components

The plugin is compiled to WebAssembly and run inside the host by a runtime such as wasmtime.

- **The interface.** A WIT world describes "the functionality a component provides, and the
  functionality it requires" ([worlds](https://component-model.bytecodealliance.org/design/worlds.html)).
  WIT packages carry full semver; an item marked `@since` "is not modified incompatibly going
  forward", and adding a case to a variant is "a _breaking change_"
  ([WIT](https://github.com/WebAssembly/component-model/blob/main/design/mvp/WIT.md)). wasmtime's
  linker matches semver-compatible versions
  ([Linker](https://docs.rs/wasmtime/latest/wasmtime/component/struct.Linker.html)).
- **The toolchain.** Rust's `wasm32-wasip2` has been tier 2 since 1.82
  ([announcement](https://blog.rust-lang.org/2024/11/26/wasip2-tier-2/)); `cargo build --target
  wasm32-wasip2` with `wit-bindgen` replaces `cargo-component`, which is "in the process of being
  deprecated" ([Rust](https://component-model.bytecodealliance.org/language-support/building-a-simple-component/rust.html)).
  "WASI 0.3.0 was released on June 11, 2026", with async, and "Wasmtime 46 is the first release to
  implement the final WASI 0.3.0 specification" ([WASI 0.3](https://wasi.dev/releases/wasi-p3)).
- **Zed**, the closest model: only extensions that need code are Rust compiled to `wasm32-wasip2`;
  themes, languages and snippets are files
  ([developing extensions](https://zed.dev/docs/extensions/developing-extensions)). Zed keeps old
  extensions working by compiling in one set of bindings per API version, ten of them from
  `since_v0_0_1` to `since_v0_8_0`, and reads which one a plugin targets from the wasm file
  ([wit.rs](https://raw.githubusercontent.com/zed-industries/zed/main/crates/extension_host/src/wasm_host/wit.rs)).
  Running a command is a capability the host grants (`process:exec`)
  ([capabilities](https://zed.dev/docs/extensions/capabilities)).
- **Others.** Lapce runs WASI plugins that speak JSON-RPC over their stdio
  ([wasi.rs](https://raw.githubusercontent.com/lapce/lapce/master/lapce-proxy/src/plugin/wasi.rs)).
  Zellij moved from wasmtime to wasmi in 0.44 for size and portability
  ([0.44](https://zellij.dev/news/remote-sessions-windows-cli/)), and says "The plugin API however
  might break every now and then" ([upgrading](https://zellij.dev/documentation/plugin-upgrading.html)).
  Envoy's proxy-wasm ABI is specified apart from any host, with SDKs in four languages
  ([spec](https://github.com/proxy-wasm/spec)). Extism wraps wasmtime with bytes in and bytes out
  and no component model ([memory](https://extism.org/docs/concepts/memory)).

**Costs.** A plugin starts with no files: "By default WebAssembly does not have access to the
filesystem because there are no preopened directories", and a host grants one folder at a time
([WasiCtxBuilder](https://docs.rs/wasmtime-wasi/latest/wasmtime_wasi/struct.WasiCtxBuilder.html)).
It cannot start a process or a thread: "spawning a process will always return an error"
([wasm32-wasip1](https://doc.rust-lang.org/rustc/platform-support/wasm32-wasip1.html); **not
verified** for wasip2, whose page does not say), so running a command is a host function.
tokio builds only with a few features there ([tokio](https://docs.rs/tokio/latest/tokio/)). A slow
plugin is bounded by epochs, about a 10% slowdown, or fuel, more
([interrupting](https://docs.wasmtime.dev/examples-interrupting-wasm.html)); a trap comes back as an
error ([Trap](https://docs.rs/wasmtime/latest/wasmtime/enum.Trap.html)); memory is unlimited until
the host limits it ([StoreLimits](https://docs.rs/wasmtime/latest/wasmtime/struct.StoreLimitsBuilder.html)).
Values are copied across every call, and synchronous calls between components carry "roughly
3.5x overhead" ([road to 1.0](https://bytecodealliance.org/articles/the-road-to-component-model-1-0)).
wasmtime releases a new major every month and supports one for 2 months, an LTS for 24
([release policy](https://docs.wasmtime.dev/stability-release.html)), and compiled plugins load
only into the same wasmtime version
([Module](https://docs.rs/wasmtime/latest/wasmtime/struct.Module.html)). wasmtime's own C library,
`libwasmtime.so`, was 19 MB in a release build with its default features and 2.1 MB without them,
measured on Linux x86_64 on 2024-12-12
([minimal](https://docs.wasmtime.dev/examples-minimal.html)); what the runtime would add to this
server is not measured.

### Dynamic libraries

The plugin is a shared library the host loads into its own process.

- **No stable ABI.** "Type layout can be changed with each compilation"
  ([type layout](https://doc.rust-lang.org/reference/type-layout.html)). The proposed stable ABI,
  crABI, is an open RFC ([RFC 3470](https://github.com/rust-lang/rfcs/pull/3470)) whose tracking
  issue has not started its implementation
  ([#111423](https://github.com/rust-lang/rust/issues/111423)).
- **libloading** loads symbols and checks nothing: "Users of this API must specify the correct
  type" ([Library](https://docs.rs/libloading/latest/libloading/struct.Library.html)).
- **abi_stable** checks layouts when a library loads, but each `0.y` release "defines its own ABI"
  and its last release was 0.11.3 on 2023-10-12
  ([abi_stable](https://github.com/rodrimati1992/abi_stable_crates),
  [crates.io](https://crates.io/api/v1/crates/abi_stable)). **stabby** is maintained (72.1.16,
  2026-07-20) and checks the compiler and the functions at load, at the cost of its own types and
  no pattern matching on them ([stabby](https://github.com/ZettaScaleLabs/stabby)).
- **Bevy** removed its dynamic plugins: "The current implementation for dynamic plugins is
  unsound" ([PR 13080](https://github.com/bevyengine/bevy/pull/13080)), removed in 0.15
  ([migration](https://bevy.org/learn/migration-guides/0-14-to-0-15/)).

**Costs.** The same compiler, and in practice the same build of every shared crate. No
isolation: since 1.81 a panic across a C ABI aborts the process
([1.81](https://blog.rust-lang.org/2024/09/05/Rust-1.81.0/)), which here is the one server every
project shares. Unloading is unreliable ([LocalKey](https://doc.rust-lang.org/std/thread/struct.LocalKey.html)).

### Configuration and scripting, for what needs no code

- **A manifest.** Zed's themes and languages, Starship's modules
  ([config](https://starship.rs/config/)) and mdBook's `book.toml` move into configuration what
  needs no code. Of the consuming project's seams, the nav entry, the theme, the generators (already
  configuration) and a route that answers files from a folder by extension need none.
- **Scripting.** Rhai is maintained (1.27.0, 2026-10-21), runs "roughly 2-3x slower than Python
  3", and its limits on work and size are off until the host sets them
  ([benchmarks](https://rhai.rs/book/about/benchmarks.html),
  [max operations](https://rhai.rs/book/safety/max-operations.html)). Lua through mlua is
  maintained; its real sandbox is Luau's ([mlua](https://github.com/mlua-rs/mlua),
  [Luau sandbox](https://luau.org/sandbox)). Steel, Koto, Rune and starlark-rust are before 1.0,
  and two of them say their API will change
  ([starlark-rust](https://github.com/facebook/starlark-rust), [Steel](https://github.com/mattwparas/steel)).

The consuming project's components read typed game data with its own crates (`relationships`,
`picture-selection`); rewriting them in a script would be a second implementation of that data's
rules. Scripting fits glue, not this.

### A prebuilt Symphony

- **dist** (formerly cargo-dist) writes its own release CI, per-platform archives, a `.sha256`
  for each and installers, and an updater that is "currently experimental"
  ([dist](https://github.com/axodotdev/cargo-dist),
  [checksums](https://axodotdev.github.io/cargo-dist/book/artifacts/checksums.html),
  [updater](https://axodotdev.github.io/cargo-dist/book/installers/updater.html)); v0.34.0 shipped
  on 2026-11-01 ([releases](https://github.com/axodotdev/cargo-dist/releases)).
- **cargo-binstall** finds release archives by convention or by
  `[package.metadata.binstall]`, and checks minisign signatures in an "initial, limited" way
  ([binstall](https://github.com/cargo-bins/cargo-binstall),
  [signing](https://github.com/cargo-bins/cargo-binstall/blob/main/SIGNING.md)).
- **On macOS** "Unix-y networking tools, like `curl` and `scp`, don't quarantine the files they
  download", and running a quarantined program always invokes Gatekeeper; but "The system may run
  Gatekeeper at other times as well. The exact circumstances under which it runs Gatekeeper is not
  documented and changes over time" ([Apple forums](https://developer.apple.com/forums/thread/706442)).
  On Apple silicon "any executable must be signed before it's allowed to run", an ad-hoc signature
  is enough and Apple's linker applies one at link time, but "binaries signed this way cannot pass
  through Gatekeeper"
  ([Big Sur release notes](https://developer.apple.com/documentation/macos-release-notes/macos-big-sur-11_0_1-universal-apps-release-notes)).
  So an ad-hoc-signed binary fetched by `curl` runs today with nothing promising it always will;
  what Gatekeeper is built to accept is Developer ID-signed software with a notarisation ticket
  ([notarizing](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)).
  A binary built on the
  machine carries the same ad-hoc signature (**not verified** that Gatekeeper never checks one). A
  running binary is
  replaced by writing a new file and renaming it over the old, never by writing into it
  ([updating Mac software](https://developer.apple.com/tutorials/data/documentation/security/updating-mac-software.json)).
- **Version skew** between a server and its clients is a stated rule elsewhere: kubectl "is
  supported within one minor version (older or newer) of `kube-apiserver`"
  ([skew policy](https://kubernetes.io/releases/version-skew-policy/)), and Docker's client and
  daemon negotiate "the highest version of the API supported by both"
  ([Engine API](https://docs.docker.com/reference/api/engine/)).

## The options

Four ways for Symphony, each with a prebuilt Symphony where it allows one. The table first, then
each.

| | A. Linked, with an SDK | B. A plugin process | C. A WebAssembly plugin | D. A dynamic library |
|---|---|---|---|---|
| A Symphony update costs a project | the pin, its fixes, a compile of the server and CLIs, a restart: about 30 min | one command, no compile of its code, a restart; the minutes not estimated | as B | the pin, a compile of the plugin with the release's exact toolchain, a restart |
| What the plugin keeps | everything | everything (below), at the cost of a JSON round trip per call, not measured | the data seams; a process, a picture resize or a written file only through the host | everything, in stable-ABI types |
| The board | one prebuilt `tasks` | one prebuilt `tasks` | one prebuilt `tasks` | one prebuilt `tasks` |
| A plugin that crashes or hangs | an abort or a stack overflow ends the one server; a hang holds its thread | its seams show an error after the call's wait; a hung call holds a worker of its process; a process that stops answering is killed and restarted, its warmed caches lost | a trap is an error; epochs bound it | a panic aborts the one server |
| Versions | Rust semver of the SDK, at compile time | negotiated at start; additive minors | WIT semver; the host keeps old worlds | a layout check at load, or none |
| A newer plugin on an older server | the compiler refuses it | the SDK speaks down to the server's minor, or the server refuses a newer major by name | the server refuses a world it does not have, by name | abi_stable refuses a mismatched layout at load; with libloading nothing checks |
| Security | the user's rights, inside the server | the user's rights, in its own process | a sandbox: the checkout, no network, host functions only | the user's rights, inside the server |
| Two projects with plugins on one machine | only from one build that links both projects' plugins | yes | yes | yes, while both match the toolchain |
| Symphony builds | an SDK crate; prebuilt CLIs | a protocol, an SDK loop, a host adapter with a two-pass page render and a card cache, process supervision, the CLI's seams: about two weeks, estimated | a WIT world, a wasmtime embedding, host functions, a wasm SDK, a monthly runtime upgrade | ABI-stable seams |

### A. Linked, with a stable SDK

The plugin keeps linking into a server the project builds, but against a small `symphony-plugin`
SDK with its own semver, never the library's internals; the board and the CLIs come prebuilt.

- **An update** still moves the pin, compiles the server (the 11-minute release install) and
  restarts. What it saves is the fixing: the SDK breaks only on a major version.
- **The host rule stays**: one server must link every project's plugins, so a second project with
  plugins shares the machine's server only through one build that links both projects' plugins.
- **Symphony builds**: the SDK's boundary (what of `text`, `register` and the rest a plugin may use,
  re-exported there), and the prebuilt board below, which every option shares.

### B. A plugin process (chosen)

![Proposed: one Symphony release per machine, each project's plugin a process of its own](plugin-delivery/proposed.svg)

The server is Symphony's release build. A project's `symphony.toml` names its plugin as a
command (`[plugins] command = ["target/release/PROJECT-plugin"]` or similar); the server starts
it once per project it serves, keeps it running, and talks to it in JSON lines on its stdin and
stdout. The project builds that binary from its own code and the SDK; its `main` is one call,
`symphony_plugin::serve(&PLUGIN)`, over the same trait implementations it has today.

**What the plugin can still do, seam by seam:**

| Seam | In B |
|---|---|
| components | one request per document render carries every component of the page, so `Page::once` keeps working inside the plugin; `cache_version` is a request too |
| card kinds | one request per register read carries every card that changed (842 in the consuming project's 39 registers), answered with each card's kind, slot, markers, pictures, category, gates and letter; the server keeps answers by the card's text |
| renderers | extensions and content types declared at start; `draw` a request |
| routes | the paths declared at start; `serve` answers JSON, or a file path under the checkout, which the server sends |
| nav, theme, generators | declared at start, or moved into `symphony.toml` |
| home regions, board actions, board fields, gates, prose rules, doc checks | a request each; the gates seam's 10 s wait and its bound on calls (`crates/symphony/src/gates.rs`) apply to every call |
| watches | the paths declared at start; `changed` tells the plugin, whose cache lives in its own process now |
| commands | `symphony NAME ARGS` runs the plugin binary with `command NAME ARGS`, its streams passed through, as Cargo runs `cargo-NAME` |
| transports | unused today; the board's built-in transports stay |

Nothing is lost. Approve to land already works this way: it calls the project's landing tool's `api` with JSON on stdin.
The `symphony` command line uses the plugin as much as the server does: the consuming project's `symphony`
installs its plugin (`apps/symphony-host/src/bin/symphony.rs`), so its three commands, its doc
checks in `status --lint`, its card kinds in triage and status, and its components in
`docs-audit` all go through the process too.

**Costs, stated as plainly as C's and D's.**

- **One pipe per project.** Every seam of a project goes through its process's stdin and stdout.
  The protocol carries a request id on every line, and `serve()` answers each request on a worker
  of a small pool, so concurrent renders do not queue behind one another and answers may come back
  out of order; the plugin's code already takes concurrent calls (`Plugin: Send + Sync`, called
  from many requests at once today). A call that hangs holds its worker until the call's wait ends,
  and its seam shows the error. When every worker is held, or one call outlives a hard limit, the
  server kills the process and starts it again, and the kill drops every cache the plugin had
  warmed (the gallery's, each page's `once` values): the next renders are cold.
- **Batching needs a two-pass render.** Today the library calls a component where it meets it in
  the page (`crates/symphony/src/plugins.rs`, `render`). One request per page means a first pass
  that collects the page's component calls, one request with all of them, and a second pass that
  places the answers.
- **The card cache needs more than the text.** A card's pictures depend on the tree they are read
  from: the dashboard asks for them from a branch's tree when the card is on a branch
  (`crates/symphony/src/dashboard.rs`), so answers are kept by tree and card text, and dropped when
  a watched picture folder changes.
- **A compile error becomes a silent mismatch.** Until the plugin uses the SDK alone, it links the
  library's internals at its own pin: `register::parse_decisions`, `edit::heading_anchor`,
  `markup::escape`, `images::reserve`, `Project::artifact`. Under a newer server it would compute
  those with older code than the server runs: an anchor or an escaping that changed shows up as a
  broken link or a wrong path, where today the pin move fails to compile. The slices below move the
  plugin to the SDK before the server comes from the release, so that window never opens.
- **Debugging moves to a log and a replay.** The plugin's stderr goes to a log of its own in the
  project's artifacts folder (`Project::artifact`), each line stamped; `serve()` catches a panic
  in a request and answers it as an error carrying the panic's message; a process that exits is
  logged with its status and its last lines. The server keeps the last requests of each project
  beside that log, and `symphony plugin replay FILE` runs the plugin binary on one of them, under
  a debugger when needed. The plugin's own tests keep running linked, in one process.
- **Not measured:** the JSON cost of a call, which the first slice measures before anything else
  is built, and how long an update takes under B (fetching, dumps, moving boards, the restart),
  which the update's own log will give.

The plugin starts once and stays up, as a language server does; a crash restarts it, with a cap,
as editors cap a crashing language server.

**Versions.** The handshake names the protocol's major and minor on both sides, and the plugin's
answer declares what it implements, as LSP's capabilities do.

- Within a major, changes are additive: a new field has a default and a reader ignores one it does
  not know, as LSP and Nushell require; a new seam is one a plugin declares or not.
- An older plugin on a newer server works: the server speaks every minor of its major.
- A newer plugin on an older server: the SDK speaks down to the server's minor when it can, and
  the server refuses a newer major by name, saying which release to install.
- A new block kind is the breaking case, as a new variant is for WIT: the SDK never sends one newer
  than the minor agreed, and the server draws an unknown block as an error in its place.
- A major is kept for some releases after the next, as Zed keeps its old API versions; how many is
  the project's to say when the first major ends.
- Today's `PIN` (`crates/symphony/src/machine.rs`) becomes the protocol's major.

**Failure isolation.** A crash, a hang or a runaway allocation is the plugin process's: that
project's seams show their error within the call's wait, every other project's pages are served,
and the server restarts the process. Today a plugin that aborts or overflows its stack takes down
the one server for every project.

**Security.** The plugin runs with the user's rights, as its code does inside the server today: no
gain and no loss. It is started without the session's variables (`children::command`) and in its
checkout.

**What Symphony builds**, each with an estimate in one executor's working days. The estimates come
from the size of each part, not from a measurement:

1. The protocol in `symphony-plugin`: each seam's request and answer as serde types (most data
   already is; the borrowed `Page` and `Card` need owned forms), request ids, the handshake and its
   version rules, written down in a document of its own. 1 to 2 days.
2. The SDK's `serve()`: reads requests, answers each on a worker of a pool, catches a panic per
   request, writes answers. 1 to 2 days.
3. The host side: a `Plugin` in the server that forwards each seam to the process (the adapter the
   crate's doc foresees), with start, handshake, the two-pass page render, the card cache keyed by
   tree and text, a timeout per call, kill and restart with a cap, the plugin's log and its record
   of requests, and a stop with the server. 4 to 6 days, the two-pass render the largest part.
4. The CLI: every seam the `symphony` command uses goes through the process: the plugin's
   commands, its doc checks in `status --lint`, its card kinds in triage and status, its
   components in `docs-audit`. 1 to 2 days.
5. A test plugin run both linked and as a process through the same tests, so the two cannot
   differ. 1 to 2 days.
6. The host rule shrinks to the protocol: any release serves every project whose plugin speaks its
   major. 1 day.

In all 9 to 15 days, about two to three weeks, before the consuming project's own steps.

### C. A WebAssembly plugin

The server embeds wasmtime and loads each project's plugin as a component of a `symphony:plugin`
WIT world, the project's checkout preopened.

- **An update** costs the project nothing until the server drops the WIT version its plugin
  targets.
- **What it keeps**: the data seams, as in B. What changes for the consuming project, seam by seam:
  - **approve to land** runs the project's landing tool's `api`, a process: it becomes a host function
    the server grants for that one program, which is the sandbox's door.
  - **pictures**: the model cards' small copies go through the library's `images::reserve`, which
    runs `sips`; that becomes a host function too, or the server resizes after the plugin answers.
  - **commands**: its picture command writes under the checkout, so it needs a folder granted for
    writing; `symphony NAME` runs a command inside a runtime, so the command line embeds wasmtime
    as well as the server.
  - **watches**: the server watches the paths and calls `changed`; the cache lives in the
    instance's memory and is lost when the instance is replaced, as B's is when its process is.
  - **components** read the game's data from the checkout, preopened read-only.
- **The consuming project's plugin cannot compile to wasm as it is**: it links the whole `symphony` library
  (the git library, the Markdown parser, the board with its embedding model) and `process-cwd`. It
  must first move to the SDK alone, which in B is a later step, not a precondition.
- **A newer plugin on an older server**: the server refuses a component built for a world it does
  not have, and says which; Zed reads the version a plugin targets from its file and keeps one set
  of bindings per version, so an older plugin keeps working.
- **What it gains**: a sandbox. Today every plugin is the owner's own code, so the risk it removes
  is an accident's, not an attacker's.
- **What Symphony builds**: the WIT world, the wasmtime embedding in the server and the command
  line (what it adds to their size is not measured; the runtime's C library alone was 19 MB in
  2024), the host functions, a wasm SDK, and a wasmtime upgrade every month or two.

### D. A dynamic library

The server loads each project's plugin as a shared library through `abi_stable` or `stabby`.

- **An update** still compiles the plugin, with exactly the release's compiler and shared crates.
- **Every seam's types** become the ABI crate's (`RString`, `RVec`, its trait objects), so every
  plugin is rewritten.
- **No isolation**: a panic across the boundary aborts the one server.
- **A newer plugin on an older server**: abi_stable checks the layouts when it loads the library
  and refuses one that does not match; through libloading alone nothing checks, and a mismatch is
  undefined behaviour inside the server.
- Rust has no stable ABI on the way, abi_stable has not been released since 2023, and Bevy removed
  the same idea as unsound.

### Not chosen

- **Scripting** as the plugin: it would rewrite the consuming project's data rules in a second language.
  Configuration takes what needs no code, under any option.
- **Two servers**, one per project with plugins: two ports, two hosts and two sets of sockets for
  the owner to know, and every project still compiles its own.

## The crates, the binaries and what rebuilds

### What exists today

From `cargo metadata` and `cargo tree` (normal and build edges, on this machine's target,
aarch64-apple-darwin, unless the row says otherwise; read 2026-11-12):

| Package | Binaries | Links (crates) | Heaviest of what it links |
|---|---|---|---|
| `symphony-server` | `symphony-server`, `symphony-shelf`, `symphony-ui-dev` | 357 | the library below, `axum`, `tokio`, `notify`; the whole client, embedded |
| `symphony` | `symphony` | 322 | the board below, `gix` (74 crates of the 322), `comrak`, `chrome-devtools` |
| `symphony-board` | `tasks`, `agent-send`, `tasks-copy`, `tasks-cutover`, `tasks-reclaim` | 189 | `tract-onnx` and `tokenizers` for search by meaning, `rusqlite` for the old board's move: 159 of the 189 |
| `symphony-ui` | the client: eight wasm modules | 148 on `wasm32-unknown-unknown` with its default features, the build macros included (173 on this machine's target) | Dioxus |
| `chrome-devtools` | none | 35 | the page-shot driver |
| `symphony-plugin` | none | 10 | the seams |
| `symphony-api` | none | 8 | the wire types and blocks |

So `agent-send`, which sends a message, compiles against the embedding model; the document CLI
compiles the board, the git library and the Chrome driver; and the consuming project's plugin, by linking
`symphony`, compiles all 322. A crate rebuilds when it or anything it links changes, and the
change propagates to everything above it
([fingerprint](https://github.com/rust-lang/cargo/blob/master/src/compiler/fingerprint/mod.rs));
the diagram above says what each kind of change rebuilds. In a consuming project every one of
those rebuilds is the project's.

The crate counts say what rebuilds, not how long it takes. Rebuild time per kind of change is not
measured here. The one time on record is the whole move of 2026-11-12: a debug build of 10 to 15
minutes and a release install of about 11, at load ~110 on 16 cores. The client task measures the
server's release build cold and after touching one Symphony crate, with `cargo build --timings`,
and its report names the ten slowest units; the size task measures each binary. Their numbers are
the first to put beside the target split below.

The release profile is Cargo's default (`opt-level = 3`, no LTO, 16 codegen units); the dev
profile sets `debug = "line-tables-only"` and `incremental = false`; the client's
`wasm-release` profile is `opt-level = "z"`, LTO, one codegen unit, `panic = "abort"`
(`Cargo.toml`).

### The target split

Each artifact is built by Symphony once per release, never by a project, and redistributed only
when its own bytes change:

| Artifact | What it holds | Rebuilt and fetched again when |
|---|---|---|
| `symphony-server` | the server's code, with no client in it | the server, the library or the board changes |
| `client/HASH/` | one build of the client, its hash the client's release inputs (`release-inputs` already computes them) | the client or the wire types change |
| `tasks`, `agent-send` | the board; `agent-send` the board's messaging alone | the board changes |
| search by meaning | the embedding model and `tasks embed` behind a feature of their own or a binary of their own | that code changes |
| `symphony` | the document CLI | the library changes |
| page shots | the Chrome driver, a binary or a feature of its own | the driver changes |
| the plugin SDK | a crate: `symphony-plugin`, `symphony-api`, `symphony-docs` | a seam changes, by semver |
| a project's plugin | the project's code and the SDK | the project's code changes: the project's own build, and the only one it makes |

What each split saves in bytes and seconds is `perf/binary-size-budget`'s table to give, and the
first measure of this design once it posts.

### How Rust projects keep binaries small and builds incremental

- **A size profile.** `opt-level = "z"` (or `"s"`, which can be smaller), `lto`, `codegen-units =
  1`, `panic = "abort"` and `strip`
  ([min-sized-rust](https://github.com/johnthagen/min-sized-rust),
  [profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)). Since 1.77 a profile
  without debug info strips it by default ([1.77](https://blog.rust-lang.org/2024/03/21/Rust-1.77.0/)).
  The server catches panics in its handlers, so `panic = "abort"` is the size task's to weigh, not
  a default. `upx` packs a binary but disabled macOS support in 4.2.0
  ([NEWS](https://raw.githubusercontent.com/upx/upx/devel/NEWS)).
- **Finding the bytes.** `cargo bloat --crates` for native code
  ([cargo-bloat](https://github.com/RazrFalcon/cargo-bloat)), `cargo llvm-lines` for generic code
  that weighs on both size and compile time ([cargo-llvm-lines](https://github.com/dtolnay/cargo-llvm-lines)).
- **Assets beside the binary, not in it.** `include_bytes!` makes the file part of the crate's
  inputs ([include_bytes](https://doc.rust-lang.org/std/macro.include_bytes.html)), so a new client
  recompiles the embedding crate and relinks what is above it. Helix delivers a `runtime/` folder
  beside its executable ([building](https://docs.helix-editor.com/building-from-source.html)), and
  Vite writes a manifest naming each hashed file for a backend to read
  ([backend integration](https://vite.dev/guide/backend-integration)). The server already serves an
  installed `client/public` folder and an override (`apps/symphony-server/src/client.rs`, `select`);
  a consuming host is what forces the embedded copy today.
- **Layering.** Split wide, not deep, and keep generic code off the boundaries between crates,
  which each user compiles again ([Fast Rust Builds](https://matklad.github.io/2021/09/04/fast-rust-builds.html));
  read `cargo build --timings` before splitting
  ([timings](https://doc.rust-lang.org/cargo/reference/timings.html)).
- **Features.** An optional dependency is not compiled unless asked for, and a binary can require
  features ([features](https://doc.rust-lang.org/cargo/reference/features.html),
  [required-features](https://doc.rust-lang.org/cargo/reference/cargo-targets.html#the-required-features-field)).
  The catch: features unify over what is built together, so building one package after the
  workspace can rebuild a dependency "and everything that depends on it"
  ([FAQ](https://doc.rust-lang.org/cargo/faq.html)); the fix in Cargo is still unstable
  ([feature-unification](https://doc.rust-lang.org/cargo/reference/unstable.html#feature-unification)),
  and a workspace-hack crate (`cargo-hakari`) is the workaround
  ([hakari](https://docs.rs/cargo-hakari/latest/cargo_hakari/about/index.html)).
- **Several binaries or one.** rustup's `cargo` and `rustc` are links to one binary that looks at
  its own name ([self_update.rs](https://github.com/rust-lang/rustup/blob/master/src/cli/self_update.rs));
  uutils builds either one binary or one per tool ([coreutils](https://github.com/uutils/coreutils)).
  One binary keeps one copy of the shared code but is sent whole at every change; separate binaries
  repeat the shared code and send only the one that changed. For a release fetched per artifact,
  separate binaries win.
- **Dynamic linking of a shared core** speeds a developer's rebuilds, not a release: Bevy's
  `dynamic_linking` is "not recommended" for shipping
  ([Bevy setup](https://bevy.org/learn/quick-start/getting-started/setup/)), and it gives no
  plugin ABI (above).
- **Linkers.** x86_64 Linux links with lld by default since 1.90
  ([1.90](https://blog.rust-lang.org/2025/09/18/Rust-1.90.0/)); macOS's own linker is the fast one
  there (same Bevy page). Cranelift for debug builds is still a nightly preview
  ([cg_clif](https://github.com/rust-lang/rustc_codegen_cranelift)).

## Distribution

### A release on a machine

A release is a folder per platform, each artifact hashed:

```
releases/release-2026-11-20/
  manifest.json        the release, the board schema, the plugin protocol's major and minors,
                       each artifact's path, size and SHA-256
  SHA256SUMS
  bin/symphony-server
  bin/tasks
  bin/agent-send
  bin/symphony
  client/HASH/         one build of the client
```

It is installed in the machine folder (`~/Library/Application Support/Symphony/` on macOS), beside
the registry, with a `current` link; the next release goes in a new folder and the link is renamed
over, never written into, as Apple asks of a running binary. The previous release stays for a
rollback. An artifact whose hash is unchanged is linked from the previous release instead of being
fetched or built again: most releases change one or two artifacts.

The recipes `symphony init` installs already look for a build in a fixed order: the checkout's
`.target/bin/NAME`, the main checkout's, then `PATH` (`crates/symphony/src/builds.rs`). The
machine's `current` release goes before `PATH`, so a project with no build of its own runs the
release, and Symphony's own checkout keeps running its own builds.

**Sizes.** Measured today, as the consuming project builds them on Cargo's default release profile: the
server 295,428,896 bytes, `tasks` 31,675,888, `symphony` 22,668,544, `agent-send` 5,601,824. In the
split the server is its code alone, about 30 MB by subtraction (an estimate) before the size
task's reductions, and the client one build of its eight modules: about 6 MB of wasm if the 322
files are 40 builds of the same size (an estimate the client task's measurement replaces). `tasks`
is the second largest: 159 of the 189 crates it links are the embedding model's runtime, its
tokenizer and the SQLite reader, which the target split above moves out of it and out of
`agent-send`. The size task's table gives where each binary's bytes are.

### How a release gets there

Built on the machine by the nightly, for now; downloaded from a hosted build later, B, which the owner
puts off for now and starts when they say so.

- **Built on the machine, now.** The nightly already checks the release's commit out in a checkout
  of its own and verifies it there (`docs/workflow/RELEASES.md`). After it tags, it builds the release
  profile once and installs it in the machine folder. That is the 11-minute release build, once
  per release per machine, never per project, and nothing is signed or downloaded. A second
  machine has nothing to fetch.
- **A project that builds its own server while the machine runs a release's board commands**, as
  every project would if plugins stayed linked, must have its pin at the tag of the installed
  release. The release's board commands and the server the project builds then share one board
  schema; a pin behind the installed tag leaves that server a schema behind its board, reading it
  and writing nothing. Under B the case does not arise: nothing comes from a release before the
  fifth slice below, when the server and the board commands come together, and until then the consuming
  project builds both at its pin, which the per-project update command below moves to the newest
  release tag.
- **Downloaded, later.** A hosted CI builds each platform's artifacts on a tag and publishes them as the
  Git host's release, through dist or a workflow of the project's; `symphony update` fetches the
  manifest, then only the artifacts whose hash it lacks, and checks each one. A `curl` download is
  not quarantined on macOS and runs with the linker's ad-hoc signature, but Apple documents no
  promise that Gatekeeper never checks it (above); a Developer ID signature and notarisation are
  the safe form, a step of the release of their own. This repository has no hosted CI today
  (AGENTS.md).

### One update command

`symphony update` on the machine: fetch or build the release's changed artifacts and check each
one's SHA-256; dump every registered project's board through its running database and move it to
the release's schema under the rule "The board" sets out below, never stopping, signalling or
restarting a board's database; point `current` at the release; restart the server. A project's
plugin is rebuilt by the project, when the project changes it or wants a newer SDK, never by the
update.

**What it replaces.** A per-project command, the one chosen before this design, moves a project's
pin to the newest release tag ([the update analysis](../../workflow/analysis/symphony-update.md)); its board task is
ready and not started, and `tools/symphony-behind-signal` (the line that says a project is behind)
depends on it. Under B it is still how the consuming project takes a release until the release's server
serves every project (the fifth slice below), so it stays worth building as filed. At the fifth
slice the machine's command replaces it, a project moves a pin only for its SDK's version when it
wants a new seam, and the behind signal reads the machine's installed release instead of a
project's pin.

## The board

- **One `tasks` for every project, from the same release as the server.** `tasks` and
  `agent-send` hold no project code today, so the release's serve every project on the machine,
  and the consuming project's `apps/tasks` package goes. They come with the server, in one release, never
  apart: the server opens a board to read (`crates/symphony/src/taskview/connect.rs`,
  `Opening::read`), and a board one schema ahead of it is read and never written (`schema.rs`,
  `decide`), so the owner's messages, priorities, fields and cancels from the app would fail; a
  board behind the server is not even read.
- **Every writer of a board must be on its schema.** A project may have programs of its own that
  open its board to write through Symphony's crates. The consuming project has two: its nightly's review
  gate opens the board as a writer at the consuming project's pin (`tools/gate-review/src/nightly/board.rs`,
  `Store::open`), and its landing reaches the board through `tools/tasks.sh`, which runs the consuming
  project's own `tasks` build. A writer behind its board's schema is refused outright. So a
  schema move is safe only once every writer of that board is at the new schema; moving every
  board at each update, whatever writes it, would refuse those writers the moment their board
  moved.
- **A board moves when every writer is on the release.** The board crate records, on the board,
  each program that opens it to write and the schema it was built at. The update moves a board
  only when every writer seen there in the last week is the release's own or built at the
  release's schema; otherwise it installs no release that raises the schema, and names each
  writer to rebuild. A release that keeps the schema installs at once. This improves on moving
  every board at once, and holds while there is no dedicated build service; that such a service
  would rebuild a project's writers with each release is the agent's reading, not the owner's.
- **Writers built before the record.** A build older than the record never writes it, so the
  record alone cannot see it, and the first schema move after the record lands would refuse it as
  moving every board would. The record therefore lands with a schema raise of its own, and that
  one move is made as today, by each project's `just tasks migrate` once its own writers are
  rebuilt. A writer behind its board's schema is refused (`decide`), so from that move on no
  build older than the record can write the board, and every writer that can is one that records
  itself. The update never moves a board whose schema is older than the record's; it names the
  project, and the project's migrate brings it up.
- **A copy first, and no database stopped.** On 2026-11-12 the consuming project's board came back three
  days old. Its launcher's 60-second readiness wait expired while the board's database replayed a
  large log, the launcher ended the database, and the next start skipped the rest of the log; the
  server's refusal to read a board behind its schema is what kept anyone from writing on the old
  state (the board task `tools/board-database-lost-its-log`). So the update never stops, signals
  or restarts a board's database. It dumps each board through its running database
  (`tasks-copy --board --out`) and keeps the dump with the release, moves the board by the board's
  own migrate, and checks that the board reads at the release's schema before it restarts the
  server. The snapshot the launcher is to take before each start (the board task
  `tools/board-rollback-alarm-and-snapshot`, in progress) is a second copy. Today only `tasks
  migrate` in a project's main checkout may move its live board (`live.rs`, `may_move`); the
  update needs that rule to admit it.
- **Symphony's own board** keeps moving with this checkout's main, as today, since Symphony's own
  work runs main's board commands. When main raises the schema before the installed release
  carries it, the machine's server reads Symphony's board and writes nothing to it; when main
  raises it twice, the server cannot read it at all. That lasts until a release with main's schema
  is installed, which is not bounded by the nightly: the update waits to be run, and under the rule
  above a lagging writer of any project on the machine holds every release that raises the
  schema. It is today's "Symphony's own pages wait for the consumer's pin" in another form, and
  the cost of a held release falls on Symphony's own pages first. What shortens it: the update
  names the writer that holds it, the line that says a project is behind says so on Symphony's
  pages, and the hold ends when that writer is rebuilt or moves to `tasks api`.
- **The consuming project's tools that link the board** move to the release's `tasks` or to the board's
  JSON interface, `tasks api --lines`, the consuming project's choice; until they do, they are the writers
  that hold a release that raises the schema.

## The chosen path: B, measured first

B is chosen, measured first: its results are measured against the current approach, for a
comparison after the change. B keeps every capability the consuming project's plugin has, needs
no rewrite of it (its plugin crate becomes a process with a one-line `main`), takes Symphony out of every project's compile, isolates
a failing plugin from the server every project shares, and lets two projects with plugins share
one machine. Its costs are real: the pipe and its workers, a cold cache after every kill, a
two-pass page render, a card cache keyed by tree, debugging through a log and a replay, and about
two to three weeks of Symphony's work by estimate. C's sandbox is not worth a rewrite of the
plugin, host functions for its processes, its pictures and its writes, and a monthly runtime
upgrade, while every plugin is the owner's own; nothing in B rules C out later, behind the same
seams. B's one unknown that could sink it is the JSON cost, so that is measured before anything
else is built.

### The slices

1. **Measure today's approach, then the JSON cost** (an estimated 2 to 3 days). First the
   "before" of the owner's comparison, taken on today's approach before anything changes (the
   three measures in "Before and after" below). Then each seam's call gets its serde request and
   answer (the first part of the protocol), and the library gains a loopback: a setting under
   which every plugin call goes through a JSON round trip inside the process, each call's time
   recorded beside the same call without it. It runs on Symphony's test plugin, then on the consuming
   project's at its next pin move, over its 39 registers and its heaviest pages. Nothing else
   changes, so it costs little if the answer is no. B goes on when a register read and a page
   render grow by little (a bound the owner sets with the numbers in hand); otherwise the protocol
   batches harder or takes a binary encoding first. C copies every value across its boundary too,
   so a high cost would not make C cheaper.
2. **The protocol and the host side** (the six parts above, an estimated 7 to 12 days after the
   first slice's share). Symphony's `[plugins]` takes a `command` beside `crates`.
3. **The consuming project's plugin as a process.** It adds a binary whose `main` is `serve(&PLUGIN)` and
   names it in its `symphony.toml`; its plugin crate and tests stay as they are. Its server is
   still its own build at the same pin as the plugin, so the two agree on every library function,
   and one line switches back.
4. **The plugin on the SDK alone.** The consuming project's plugin moves off the library's internals (the
   five functions above and the rest of the `symphony` row in the coupling table) to what the SDK
   re-exports or the protocol answers; the Symphony side of its build is then the SDK's 10 crates,
   not the library's 322. This comes before the server leaves the project, so the plugin never
   computes with older library code than the server runs.
5. **The release on the machine.** The server, `tasks`, `agent-send`, `symphony` and the client,
   from one release, together, built here by the nightly and installed by `symphony update`: the
   client as `client/HASH/` beside the server, no longer embedded; the host rule the protocol's;
   boards moved under the rule of "The board", once the writer record has landed. The consuming project
   drops `apps/symphony-host` and `apps/tasks`, its `tools/tasks.sh` runs the release's `tasks`,
   and the machine's command replaces the per-project pin move.
6. **Per-artifact fetch**, when the owner moves releases to hosted builds; not yet.

Until the fifth slice the consuming project takes Symphony as it does today, by moving its pin to the
newest release tag, so each of the first four leaves it working. From the fifth, the server
and the board commands always come from one release, so no board is ever a schema ahead of the
server that serves it, and a board moves only when every program that writes it is on the
release. The writer record and its schema raise can land before the fifth slice, on their own:
with them a project's own `just tasks migrate` can name a writer that lags before it moves the
board. The size tasks' work, a bounded client distribution and a
size check, runs beside these slices and waits on none of them.

### Before and after

The owner asked for B's results measured against the current approach. The first slice takes the
"before" on today's approach; the same three measures are taken again after the change.

| Measure | Before: today's approach, taken in the first slice | After |
|---|---|---|
| **An update, end to end** | on the consuming project's next pin move: the wall time of each step (moving the pin, each fix, each build, each test run, each lint pass, the release install, the restart) and of the whole, from the first command to the restarted server answering a page, with the load average; read from the build and test records the board's recorder keeps for each recorded run (`docs/workflow/MACHINE_WORK.md`) and the server's start | the first `symphony update` that installs a release, from the command to the restarted server answering, by the update's own log of each step: fetching or building, each board's dump and move, the restart |
| **What is rebuilt and redistributed** | the units Cargo compiles for that move (`cargo build --timings` on the release install: how many, each one's time, and whose: Symphony's crates, the project's, third-party), and the bytes it installs (each installed binary's size, the 295 MB server included) | the same, for the update: the units the project compiles (none of its code expected) and the bytes fetched and installed (only the artifacts whose hash changed) |
| **Each plugin call** | every seam call's time, linked, over one read of the consuming project's 39 registers and a render of its heaviest pages (those with the most components) | the same calls through the loopback in the first slice, then through the process in the third |

The table's numbers go into this document as each is taken.

### What would be measured

- **A Symphony update on a project** is one command and no compile of project code; on this
  machine, at the same load, it takes minutes, where it took about 30 on 2026-11-12, by the table
  above.
- **After the fifth slice**, the consuming project's `just tasks` runs the release's build with no build of the
  consuming project's, and every registered board whose writers are on the release reaches a new
  release's schema at the update.
- **A plugin change** rebuilds only the plugin's binary, and the server starts it again without a
  restart of its own.
- **Isolation**: a plugin process killed, or made to hang for 60 s, leaves `/symphony/` answering
  and the consuming project's pages showing the seam's error within the call's wait; concurrent renders of
  two of its pages both answer while a third call hangs.
- **Versions**: a plugin built at protocol 1.0 is still served by a server at 1.n, by a test.
- **Size**: the server within the budget the size task records, with the client beside it at one
  build.
- **The call cost through the process**: the consuming project's largest register read and its heaviest
  page render, linked and as a process, in the third slice; the process must not make either
  noticeably slower than the first slice's numbers said.

## Open questions

1. How many releases does a protocol major outlive its successor? Until a first major ends, none
   needs to.
2. Should `symphony update` run unattended after the nightly tags, or wait to be run? Moving every
   board is an act on live state, so until the owner says otherwise it waits, and the line that
   says a project is behind ([the update analysis](../../workflow/analysis/symphony-update.md#knowing-a-project-is-behind))
   says so. While it waits, Symphony's own pages may read a board they cannot write ("The board").
3. The consuming project's tools that link the board's internals: stay pinned, or move to `tasks api`?
   The consuming project's to decide, on its board. Until they move, they hold every release that raises
   the schema.
4. How long a writer counts as live in the board's record of its writers: a week is proposed, so
   that a nightly or a weekly tool is seen; a shorter window lets a release through sooner and
   risks refusing a tool that runs rarely.
5. What JSON cost is small enough for B to go on: the owner's to set once the first slice has
   numbers.

## Verified and inferred

- **Read in code:** each fact that names a file and a function, in Symphony at `c760118` (and the
  review's points at `a615bfba`) and in the consuming project's checkout in place, at its `62d91ce93`.
- **Measured:** the twenty landings' changes (`git log` and each merge's diff); crate counts
  (`cargo tree`, the target named); the client distribution's size and files (`du`, `find`); the
  sizes of the consuming project's builds (`stat`); the consuming project's uses of each crate, its cards and
  component uses (`grep`); its pin moves (`git log` of its `Cargo.lock`) and the last move's times
  (its machine-work records, read and decoded in place; that their fields mean start and duration
  is inferred from their values). The 30-minute figure and its parts are the orchestrator's
  measurement of 2026-11-12.
- **Estimated, not measured:** the server's code at about 30 MB (295 MB less the 266 MB client);
  one client build at about 6 MB of wasm; each part of B's work in days, and the first slice's 2 to
  3 days; the 6 to 8 µs a message, derived from a benchmark's throughput.
- **From outside:** each claim beside its URL, read 2026-11-12.
- **Not verified:** that Helix rejected RPC as too slow; that WASI's preview 2 refuses a process
  as preview 1 does; that Gatekeeper never checks a binary built on the machine or fetched by
  `curl`; the JSON cost of a call; how long an update takes under B; the rebuild time of each kind
  of change.
- **Not tried:** any of the options; nothing here was built.
