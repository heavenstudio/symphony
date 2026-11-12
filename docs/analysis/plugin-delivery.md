# Delivering Symphony without rebuilds

How a project could take a Symphony release without compiling Symphony, with its own plugin code
delivered apart from Symphony's, and how Symphony's crates, binaries and client could be split so
that a change rebuilds and redistributes only what it touches. Three cards in
[DECISIONS](../DECISIONS.md) carry the choices. This is a design: nothing here is built.

The owner, 2026-11-12, after Star Foundry's pin had to be moved and its host rebuilt to bring the
app up to date: "seeing how problematic the current approach that requires building client's board
often for them to even work is, there needs to be design a better approach for more automatic type
of updates, by keeping client specific code separate from the board's one, in order full rebuilds to
be avoided when binaries are updated?" and "I feel that this is more of a problem around "rust's
crate and build archiectural design issue" around server and client architecture with custom plugin
capabilities." Later the same morning: "It feels to me we should really have separate crates that
have lower binary sizes, in order to be able to distribute things quickly and with smaller builds,
so that only things that needs to be rebuilt are rebuilt and distributed. And that is rust's crate
design architecture pains that we need to really solve well. 30MB is still really big for a binary".

How facts are marked: a fact about the code names the file and function it was read in; a
measurement says how it was taken; a fact from outside names its source by URL beside it, with the
words quoted where they could be disputed; **Not verified** marks what could not be checked. "Read
2026-11-12" is the state of this machine that day: Symphony's main at `c760118`, Star Foundry's
checkout read in place and changed in nothing.

![Today: each project compiles Symphony into binaries of its own](plugin-delivery/today.svg)

## What an update costs today

- **The pin.** Star Foundry names ten Symphony crates in its `Cargo.toml` on `branch = "main"`,
  and its `Cargo.lock` binds the commit (read 2026-11-12: `19ea97f`, 29 commits behind main). It
  moved the pin four times in 19 hours, from 2026-11-11 12:30 to 2026-11-12 07:18 (its `git log`
  of `Cargo.lock`); two of the four needed code changes in Star Foundry before they built.
- **The last move, 2026-11-12.** From Star Foundry's machine-work records: the host server built
  in the pin's worktree at 07:00, the host and plugin tests ran red at 07:15 (exit 101), the pin
  and its two test fixes were committed at 07:18, and `cargo install` of the server ran four times
  on main from 07:33 to 07:35. The installed `symphony-server` is 295,428,896 bytes, written at
  07:33:53 (its `installed.json`). The orchestrator measured the whole move at about 30 minutes at
  load ~110 on 16 cores: a debug build of 10 to 15 minutes, the tests, a commit hook's workspace
  lint of about 2 minutes a pass, and a release install of about 11 minutes.
- **The server's size.** About 266 MB of the 295 MB is the embedded client distribution:
  `apps/symphony-server/client/public` is 266 MB in 1,587 files, 322 of them `.wasm`, every build
  ever published (measured on main, `du` and `find`). The build script embeds every file
  (`apps/symphony-server/build.rs`, `generate`; served by `client.rs`, `asset`). The board task
  `perf/client-distribution-keeps-every-build` bounds what is kept, and `perf/binary-size-budget`
  measures the rest and adds a size check; their numbers belong in this document when they post.
  Neither had posted a measurement when this was written (2026-11-12, 09:45), so the sizes below
  that come from them are marked as estimates.
- **The host rule.** The machine runs one server, and it must compile every registered project's
  plugins (`crates/symphony/src/machine/host.rs`, `judge`). A project with no plugins never hosts,
  so Symphony's own project is served by Star Foundry's build, at Star Foundry's pin: the
  machine's `server.json` names Star Foundry's checkout. A second project with plugins of its own
  could not be hosted beside Star Foundry at all, since no build holds both.
- **The board.** Star Foundry builds its own `tasks` and `agent-send`, though neither holds a line
  of its code: its `tasks` is `symphony_board::cli::process_main()` and nothing else, its
  `agent-send` the board's `agent_send::main` with a list of variables (`apps/tasks/src/bin/`). A
  build writes only a board at its own schema, an older writer is refused, and a reader one
  version behind still reads (`crates/tasks/src/schema.rs`, `decide`). A consuming project's live
  board moves forward only by `just tasks migrate` in its main checkout (`crates/tasks/src/live.rs`,
  `may_move`). The schema went from 9 on 2026-11-02 to 16 on 2026-11-11 (`git log -G` of
  `VERSION`), so a project whose board command lags a schema cannot write its board, and one two
  behind cannot read it.

## Where the coupling is

### What a project compiles against

Counted in Star Foundry's `apps/`, `tools/` and `crates/`, tests included, by the files that name
each crate (`grep`, read 2026-11-12):

| Symphony crate | What Star Foundry uses | Files |
|---|---|---|
| `symphony-plugin` | the `Plugin` trait and 11 of its 16 seams | 18 |
| `symphony-api` | the block vocabulary and the seams' data (`Block`, `TaskOffer`, `NavItem`, …) | 10 |
| `symphony` | the plugin: text helpers, register parsing (`parse_decisions`, `Decision`), schema validation, markup escaping, `images::sips_available`, `edit::heading_anchor`, `children::command`, `blocks::vector_shapes`, `project::Project`; the host: `cli::main`, `plugins::install`; its tools: `landing`, `register`, `verdicts`, `taskcli`, `clock`, `commit` | 45 |
| `symphony-server` | `binary::run`, `development::main` | 5 |
| `symphony-board` | its `tasks` and `agent-send` (above); its review gate links `sys`, `sweep`, `red`, `assessment_publication`, `slots`, `on_review`, `alarm`, `rpc`, `port` | 72, 57 of them in `tools/gate-review` |
| `symphony-docs` | `DocCheck`, `Docs`, `Problem` | 15 |
| `process-cwd` | its child-process helpers, everywhere | 132 |

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

What did break Star Foundry, in its last four pin moves, was all in its tests of Symphony's
internals, never the seams: `symphony::verdicts::BoardTask` gained `links` (a struct literal in
`tools/art`'s test), `docview::folder` took a new argument (`tools/symphony`'s renderer test), and
the CLI's usage text changed (an exact-text test). So the seams are stable; what breaks is linking
the library's internals. And with nothing broken at all, every update is still a compile of the
server, the CLIs and the plugin, an install and a restart.

![The crates a consumer compiles, and what a change rebuilds](plugin-delivery/crates.svg)

### What a project customises

Star Foundry's plugin (`tools/symphony/src/lib.rs`), seam by seam:

| Seam | What it adds | How it runs |
|---|---|---|
| components | 14: ship, ships, art, screen, ship-matrix, shape-matrix, subfaction-ships, relationships, five icon families, the hull gallery | reads the game's data under the checkout, renders blocks, gives a cache version per page; used 726 times in its docs |
| card kinds | 4: hull model, art, golden frame, hull mentions | reads a card's text and thread: pictures, slot, markers, triage category, what it gates, the letter chosen; 842 cards in 39 registers |
| renderers | `.glb` hull models | draws a viewer, a custom element whose script is a file of the project |
| routes | `/x/hulls/<id>.glb` | answers a file from the models folder |
| nav | "⬡ hulls" | a fixed entry |
| home regions | ship art | a region computed from the checkout |
| board actions | approve to land | already out of process: it runs `tools/rs-bin.sh sf-land api` with JSON on stdin (`land_call`) |
| commands | `relationships`, `screens`, `ship-art` | `symphony NAME ARGS`, writing to stdout and stderr |
| watches | the gallery's models, concepts and outlines folders | drops and warms a cache in the server's memory |
| theme | the name and the icon | fixed data |
| doc checks | the design data, the faction pages, the screen catalogue | read `docs/` and return problems |

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
  the trade Star Foundry's tests met.
- **Neovim** runs remote plugins over MessagePack-RPC and states its API's level and the oldest it
  is compatible with ([API](https://neovim.io/doc/user/api/)).
- **Helix** has not chosen this: its plugin work is a Scheme, Steel, still an experimental draft
  ([PR 8675](https://github.com/helix-editor/helix/pull/8675)); a maintainer wrote of WebAssembly
  that "we had three separate attempts at integrating WASM that didn't lead anywhere"
  ([discussion 3806](https://github.com/helix-editor/helix/discussions/3806)). **Not verified:** that
  it turned RPC down as too slow; only a third party's summary on that page says so.

**Costs.** A call is a round trip through the kernel: about 6 to 8 µs on Unix sockets and pipes
in one old benchmark, before encoding ([ipc-bench](https://github.com/goldsborough/ipc-bench); none
of the projects above publishes a figure). A process per call is slow to start, which is why
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
wasmtime ships a new major every month and supports one for 2 months, an LTS for 24
([release policy](https://docs.wasmtime.dev/stability-release.html)), and compiled plugins load
only into the same wasmtime version
([Module](https://docs.rs/wasmtime/latest/wasmtime/struct.Module.html)). The runtime adds about
19 MB to a release build, 2.1 MB without its compiler, in 2024 figures
([minimal](https://docs.wasmtime.dev/examples-minimal.html)).

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
  needs no code. Of Star Foundry's seams, the nav entry, the theme, the generators (already
  configuration) and a route that answers files from a folder by extension need none.
- **Scripting.** Rhai is maintained (1.27.0, 2026-10-21), runs "roughly 2-3x slower than Python
  3", and its limits on work and size are off until the host sets them
  ([benchmarks](https://rhai.rs/book/about/benchmarks.html),
  [max operations](https://rhai.rs/book/safety/max-operations.html)). Lua through mlua is
  maintained; its real sandbox is Luau's ([mlua](https://github.com/mlua-rs/mlua),
  [Luau sandbox](https://luau.org/sandbox)). Steel, Koto, Rune and starlark-rust are before 1.0,
  and two of them say their API will change
  ([starlark-rust](https://github.com/facebook/starlark-rust), [Steel](https://github.com/mattwparas/steel)).

Star Foundry's components read typed game data with its own crates (`relationships`,
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
- **On macOS** a file `curl` downloads is not quarantined, so Gatekeeper never sees it
  ([Apple forums](https://developer.apple.com/forums/thread/706442)); a binary built on the
  machine is not either (**not verified**: inferred from the same rules). A running binary is
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
| A Symphony update costs a project | the pin, its fixes, a compile of the server and CLIs, a restart: about 30 min | one command, no compile, a restart | one command, no compile, a restart | the pin, a compile of the plugin with the release's exact toolchain, a restart |
| What the plugin keeps | everything | everything (below) | the data seams; a process only through the host | everything, in stable-ABI types |
| The board | one prebuilt `tasks` | one prebuilt `tasks` | one prebuilt `tasks` | one prebuilt `tasks` |
| A plugin that crashes or hangs | an abort or a stack overflow ends the one server; a hang holds its thread | its seams show an error; restarted; a timeout per call | a trap is an error; epochs bound it | a panic aborts the one server |
| Versions | Rust semver of the SDK, at compile time | negotiated at start; additive minors | WIT semver; the host keeps old worlds | a layout check at load, or none |
| Security | the user's rights, inside the server | the user's rights, in its own process | a sandbox: the checkout, no network, host functions only | the user's rights, inside the server |
| Two projects with plugins on one machine | impossible: no build holds both | yes | yes | yes, while both match the toolchain |
| Symphony builds | an SDK crate; prebuilt CLIs | a protocol, an SDK loop, a host adapter, process supervision | a WIT world, a wasmtime embedding, host functions, a wasm SDK, a monthly runtime upgrade | ABI-stable seams |

### A. Linked, with a stable SDK

The plugin keeps linking into a server the project builds, but against a small `symphony-plugin`
SDK with its own semver, never the library's internals; the board and the CLIs come prebuilt.

- **An update** still moves the pin, compiles the server (the 11-minute release install) and
  restarts. What it saves is the fixing: the SDK breaks only on a major version.
- **The host rule stays**: one server must link every project's plugins, so a second project with
  plugins cannot share the machine's server.
- **Symphony builds**: the SDK's boundary (what of `text`, `register` and the rest a plugin may use,
  re-exported there), and the prebuilt board below, which every option shares.

### B. A plugin process (recommended)

![Proposed: one Symphony release per machine, each project's plugin a process of its own](plugin-delivery/proposed.svg)

The server is Symphony's release build. A project's `symphony.toml` names its plugin as a
command (`[plugins] command = ["target/release/star-foundry-plugin"]` or similar); the server starts
it once per project it serves, keeps it running, and talks to it in JSON lines on its stdin and
stdout. The project builds that binary from its own code and the SDK; its `main` is one call,
`symphony_plugin::serve(&PLUGIN)`, over the same trait implementations it has today.

**What the plugin can still do, seam by seam:**

| Seam | In B |
|---|---|
| components | one request per document render carries every component of the page, so `Page::once` keeps working inside the plugin; `cache_version` is a request too |
| card kinds | one request per register read carries every card that changed (842 in Star Foundry's 39 registers), answered with each card's kind, slot, markers, pictures, category, gates and letter; the server keeps answers by the card's text |
| renderers | extensions and content types declared at start; `draw` a request |
| routes | the paths declared at start; `serve` answers JSON, or a file path under the checkout, which the server sends |
| nav, theme, generators | declared at start, or moved into `symphony.toml` |
| home regions, board actions, board fields, gates, prose rules, doc checks | a request each; the gates seam's 10 s wait and its bound on calls (`crates/symphony/src/gates.rs`) apply to every call |
| watches | the paths declared at start; `changed` tells the plugin, whose cache lives in its own process now |
| commands | `symphony NAME ARGS` runs the plugin binary with `command NAME ARGS`, its streams passed through, as Cargo runs `cargo-NAME` |
| transports | unused today; the board's built-in transports stay |

Nothing is lost. Approve to land already works this way: it calls `sf-land api` with JSON on stdin.

**Costs and how they are bounded.** A call costs a pipe round trip, microseconds, plus JSON for its
data: one request per page render and per register read, not per component or card. The plugin
starts once and stays up, as a language server does; a crash restarts it, with a cap, as editors
cap a crashing language server. **Not measured:** the JSON cost of Star Foundry's largest register
read; that is the first slice's measurement.

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

**What Symphony builds:**

1. The protocol in `symphony-plugin`: each seam's request and answer as serde types (most already
   are), the handshake and its version rules, written down in a document of its own.
2. The SDK's `serve()`: reads requests, calls the plugin's trait methods, writes answers.
3. The host side: a `Plugin` in the server that forwards each seam to the process (the adapter the
   crate's doc foresees), with start, handshake, batching, a timeout per call, restart with a cap,
   and a stop with the server.
4. The CLI: a plugin's command runs the plugin binary.
5. A test plugin run both linked and as a process through the same tests, so the two cannot differ.
6. The host rule shrinks to the protocol: any release serves every project whose plugin speaks its
   major.

### C. A WebAssembly plugin

The server embeds wasmtime and loads each project's plugin as a component of a `symphony:plugin`
WIT world, the project's checkout preopened.

- **An update** costs the project nothing until the server drops the WIT version its plugin
  targets.
- **What it keeps**: the data seams, as in B. A process only through a host function: Star
  Foundry's approve to land runs `sf-land api`, and its pictures run `sips`; each becomes a host
  function the server grants, which is the sandbox's door. A command (`ship-art`) runs in wasm with
  the folders it is granted.
- **Star Foundry's plugin cannot compile to wasm as it is**: it links the whole `symphony` library
  (the git library, the Markdown parser, the board with its embedding model) and `process-cwd`. It
  must first move to the SDK alone, which in B is optional.
- **What it gains**: a sandbox. Today every plugin is the owner's own code, so the risk it removes
  is an accident's, not an attacker's.
- **What Symphony builds**: the WIT world, the wasmtime embedding (about 19 MB more server), the
  host functions, a wasm SDK, and a wasmtime upgrade every month or two.

### D. A dynamic library

The server loads each project's plugin as a shared library through `abi_stable` or `stabby`.

- **An update** still compiles the plugin, with exactly the release's compiler and shared crates.
- **Every seam's types** become the ABI crate's (`RString`, `RVec`, its trait objects), so every
  plugin is rewritten.
- **No isolation**: a panic across the boundary aborts the one server.
- Rust has no stable ABI on the way, abi_stable has not been released since 2023, and Bevy removed
  the same idea as unsound.

### Not chosen

- **Scripting** as the plugin: it would rewrite Star Foundry's data rules in a second language.
  Configuration takes what needs no code, under any option.
- **Two servers**, one per project with plugins: two ports, two hosts and two sets of sockets for
  the owner to know, and every project still compiles its own.

## The crates, the binaries and what rebuilds

### What exists today

From `cargo metadata` and `cargo tree` (normal and build edges, read 2026-11-12):

| Package | Binaries | Links (crates) | Heaviest of what it links |
|---|---|---|---|
| `symphony-server` | `symphony-server`, `symphony-shelf`, `symphony-ui-dev` | 357 | the library below, `axum`, `tokio`, `notify`; the whole client, embedded |
| `symphony` | `symphony` | 322 | the board below, `gix` (74 crates of the 322), `comrak`, `chrome-devtools` |
| `symphony-board` | `tasks`, `agent-send`, `tasks-copy`, `tasks-cutover`, `tasks-reclaim` | 189 | `tract-onnx` and `tokenizers` for search by meaning, `rusqlite` for the old board's move: 159 of the 189 |
| `symphony-ui` | the client: eight wasm modules | 187 | Dioxus |
| `chrome-devtools` | none | 35 | the page-shot driver |
| `symphony-plugin` | none | 10 | the seams |
| `symphony-api` | none | 8 | the wire types and blocks |

So `agent-send`, which sends a message, compiles against the embedding model; the document CLI
compiles the board, the git library and the Chrome driver; and Star Foundry's plugin, by linking
`symphony`, compiles all 322. A crate rebuilds when it or anything it links changes, and the
change propagates to everything above it
([fingerprint](https://github.com/rust-lang/cargo/blob/master/src/compiler/fingerprint/mod.rs));
the diagram above says what each kind of change rebuilds. In a consuming project every one of
those rebuilds is the project's.

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
  recompiles the embedding crate and relinks what is above it. Helix ships a `runtime/` folder
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
  One binary keeps one copy of the shared code but ships whole at every change; separate binaries
  repeat the shared code and ship only the one that changed. For a release fetched per artifact,
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

**Sizes.** Today the one artifact a project gets is a 295 MB server. In the split the server is
its code alone, about 30 MB before the size task's reductions, and the client one build of its
eight modules: about 6 MB of wasm if the 322 files are 40 builds of the same size, an estimate the
client task's measurement replaces. The board, the CLI and the client are each smaller again;
their sizes are the size task's table.

### How a release gets there

The second card's choice: built on the machine by the nightly, or downloaded.

- **Built on the machine.** The nightly already checks the release's commit out in a checkout of
  its own and verifies it there (`docs/RELEASES.md`). After it tags, it builds the release profile
  once and installs it in the machine folder. That is the 11-minute release build, once per
  release per machine, never per project, and nothing is signed or downloaded. A second machine
  has nothing to fetch.
- **Downloaded.** A hosted CI builds each platform's artifacts on a tag and publishes them as the
  Git host's release, through dist or a workflow of the project's; `symphony update` fetches the
  manifest, then only the artifacts whose hash it lacks, and checks each one. A `curl` download is
  not quarantined on macOS, so nothing needs signing to run; signing and notarising come with an
  app the owner hands to someone else. This repository has no hosted CI today (AGENTS.md).

### One update command

`symphony update` on the machine: fetch or build the release's changed artifacts and check each
one's SHA-256; dump every registered project's board and move each to the release's schema (the
third card); point `current` at the release; restart the server. A project's plugin is rebuilt by
the project, when the project changes it or wants a newer SDK, never by the update.

It replaces the per-project command of [the update analysis](symphony-update.md): a project no
longer moves a pin to take a release, only its SDK's version, when it wants a new seam.

## The board

- **One `tasks` for every project.** It holds no project code today, so the release's `tasks` and
  `agent-send` serve every project on the machine, and Star Foundry's `apps/tasks` package goes.
- **Schema moves are Symphony's.** Today only a project's own `just tasks migrate` moves its
  board. With one release per machine, the update moves every registered board, each after a dump
  (`tasks-copy --board --out`), so the server, the board command and every board are at one
  schema at once. A board that cannot be moved stops the update before the restart, naming it.
- **Symphony's own board** keeps moving with this checkout's main, as it does today, so a schema
  lands on Symphony's board before any release carries it. The release's server reads it while
  main's schema is at most one ahead (`schema.rs`, `decide`), which a nightly release keeps true
  unless main raises the schema twice in a day.
- **Star Foundry's tools** that link the board's internals (`tools/gate-review` and the others)
  are its own builds and pin Symphony's crates at their own pace; nothing waits on them. The board's
  `tasks api --lines` JSON interface is the way off that pin, Star Foundry's to take.

## The recommendation: B, the board first

B keeps every capability Star Foundry's plugin has, needs no rewrite of it (its plugin crate
becomes a process with a one-line `main`), takes Symphony out of every project's compile, isolates
a failing plugin from the server every project shares, and lets two projects with plugins share
one machine. C's sandbox is not worth a rewrite of the plugin and a monthly runtime upgrade while
every plugin is the owner's own; nothing in B rules C out later, behind the same seams.

### The slices

Each step leaves Star Foundry working.

1. **The board and the CLIs from the release** (days). The nightly builds `tasks`, `agent-send`
   and `symphony` at the tag into the machine folder with their checksums; `builds.rs` finds the
   current release before `PATH`; `symphony update` installs it and moves every registered board,
   refusing to move one more than a schema ahead of the running server, which is still Star
   Foundry's build until slice 4. Star Foundry drops `apps/tasks` (a note for its board).
2. **The protocol and the host side** (a week). The protocol document, `serve()`, the host adapter,
   the plugin's commands through its binary, and the test plugin run both ways. Symphony's
   `[plugins]` takes a `command` beside `crates`.
3. **Star Foundry's plugin as a process.** It adds a binary whose `main` is `serve(&PLUGIN)` and
   names it in its `symphony.toml`; its plugin crate and tests stay as they are. Its own build
   still links the server meanwhile, so it can switch back by one line.
4. **The server from the release, the client beside it.** The release's server serves every
   project, the host rule becomes the protocol's, the client is installed as `client/HASH/` and no
   longer embedded, and Star Foundry's `apps/symphony-host` goes.
5. **The plugin on the SDK alone.** Star Foundry's plugin moves off the library's internals to what
   the SDK re-exports; the Symphony side of its build is then the SDK's 10 crates, not the
   library's 322.
6. **Per-artifact fetch**, once the second card says downloads.

### What would be measured

- **A Symphony update on a project** is one command and no compile of project code; on this
  machine, at the same load, it takes minutes, where it took about 30 on 2026-11-12. Measured by
  the update's own log of each step's time.
- **After slice 1**, Star Foundry's `just tasks` runs the release's build with no Star Foundry
  build, and every registered board reaches a new release's schema at the update.
- **A plugin change** rebuilds only the plugin's binary, and the server starts it again without a
  restart of its own.
- **Isolation**: a plugin process killed, or made to hang for 60 s, leaves `/symphony/` answering
  and Star Foundry's pages showing the seam's error within the call's wait.
- **Versions**: a plugin built at protocol 1.0 is still served by a server at 1.n, by a test.
- **Size**: the server within the budget the size task records, with the client beside it at one
  build.
- **The call cost**: Star Foundry's largest register read and its heaviest page render, linked and
  as a process, in slice 3; the process must not make either noticeably slower.

## Open questions

1. How many releases does a protocol major outlive its successor? Until a first major ends, none
   needs to.
2. Should `symphony update` run unattended after the nightly tags, or wait to be run? Moving every
   board is an act on live state, so until the owner says otherwise it waits, and the line that
   says a project is behind ([the update analysis](symphony-update.md#knowing-a-project-is-behind))
   says so.
3. Star Foundry's tools that link the board's internals: stay pinned, or move to `tasks api`?
   Star Foundry's to decide, on its board.

## Verified and inferred

- **Read in code:** each fact that names a file and a function, in Symphony at `c760118` and in
  Star Foundry's checkout in place.
- **Measured:** the twenty landings' changes (`git log` and each merge's diff); crate counts
  (`cargo tree`); the client distribution's size and files (`du`, `find`); Star Foundry's uses of
  each crate, its cards and component uses (`grep`); its pin moves (`git log` of its `Cargo.lock`)
  and the last move's times (its machine-work records, read and decoded in place; that their
  fields mean start and duration is inferred from their values). The 30-minute figure and its
  parts are the orchestrator's measurement of 2026-11-12.
- **From outside:** each claim beside its URL, read 2026-11-12.
- **Not verified:** that Helix rejected RPC as too slow; that WASI's preview 2 refuses a process
  as preview 1 does; that a binary built on the machine is never quarantined; a JSON cost per
  register read; the size of one client build (an estimate from the file count).
- **Not tried:** any of the options; nothing here was built.
