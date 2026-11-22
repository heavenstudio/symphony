# Symphony behind a versioned contract

Whether Symphony's packages can be released and versioned one by one, so that a consuming project
takes each when it is ready, and what that asks of Cargo, of the one machine every project shares
and of the release. This is an investigation: nothing here is built. The direction is B, the
process boundary and a few packages with version numbers of their own
([its card](#consumer-contract-versioning)); each further choice B leaves open is a
decision of its own, on a card of its own.

- **Today there is one version of everything.** The consuming project names ten Symphony packages
  from one Git pin, and its lock binds them all to one commit. The machine's one server is built
  from that same pin. So the project updates everything at once or nothing, and every release
  builds and runs its whole suite ([below](#today-one-pin-for-everything)).
- **What it touches** is far more than the plugin seams: the library's internals and the board's
  library in its own tools, the board's schema as a writer, `tasks api`, the settings file, hook
  names, and Symphony's own source files read by its tests. The seams and the block vocabulary
  broke it in none of the last weeks' breaks; the library's internals, the board's library and
  the board's rules did ([below](#what-broke-it)).
- **Cargo:** a Git pin names one commit, and two references to the same repository are two
  sources, so two packages taken at two tags bring two copies of every package they share, whose
  types do not match. Only a registry gives version ranges that unify. Tried here, resolving only
  ([below](#what-cargo-allows)).
- **The machine** shares one server and one board schema per board. The server serves a project
  registered at an older pin or schema, and leaves out a setting it does not know; what it refuses
  is a board behind its schema. A contract can let a project's own code lag; it cannot let it keep
  an older server or board than the machine's without new work that costs more than it gives
  ([below](#what-one-machine-shares)).
- **Three options:** A, the process boundary only; B, A plus a small set of packages with their
  own version numbers; C, every package versioned on its own. The direction: **B**
  ([below](#the-options)).

How facts are marked: a fact about the code names the file and the item it was read in; a
measurement says how it was taken; a fact from outside names its source by URL, quoted where its
words matter; **Not verified** marks what could not be checked. "Read 2026-11-13" is this machine
that day: Symphony's main at `22bdb4c4`, the consuming project's checkout read in place at its main
(`ce64816ed`), changed in nothing.

## Today: one pin for everything

- **The pin.** The consuming project's workspace manifest names `process-cwd`, `repository-read`,
  `symphony`, `symphony-api`, `symphony-args`, `symphony-board`, `symphony-docs`,
  `symphony-plugin`, `symphony-schema` and `symphony-server`, each `{ git = …, branch = "main" }`,
  and its `Cargo.lock` binds all twelve Symphony packages it locks (those ten, `symphony-env` and
  `chrome-devtools` through them) to `b8e59102`, a commit of 2026-11-13 08:28, 129 commits behind
  main at `22bdb4c4` (`git rev-list --count`). No release is tagged yet (`git tag -l 'release-*'`
  is empty), so the pin is a commit of main moved by hand.
- **Every package is 0.1.0 and unpublished.** Each member's manifest says `version = "0.1.0"`;
  the workspace says `publish = false` (`Cargo.toml`, `[workspace.package]`). Members depend on
  each other by `path` alone. No version number carries meaning.
- **The release is one version too.** A release is one tag on one commit of main
  ([RELEASES](../RELEASES.md#a-release)); its refresh moves a consumer's whole pin to it in a commit
  that may change only `Cargo.toml` and `Cargo.lock`
  ([the refresh](../RELEASES.md#the-refresh)), and its conciliation builds and tests the
  consumer's packages that use Symphony against the candidate
  ([step 4](../RELEASES.md#the-release-run)).
- **The version numbers that do exist** are four counters, each raised by hand: the board's schema
  (`VERSION = 17`, `crates/tasks/src/schema.rs`), the `tasks api` methods (1.14:
  `MAJOR = 1` and `MINOR = 14`, `crates/tasks/src/cli/api.rs`), the plugin protocol (`PROTOCOL`, 0.2,
  `crates/symphony-plugin/src/protocol.rs`) and the machine's pin (`PIN = 1`,
  `crates/symphony/src/machine.rs`: "raised with each change to the wire contract or the plugin
  seams a host and its projects share", 1 since it came on 2026-11-05).

## Where a consuming project touches Symphony

### The packages it links

Counted in the consuming project's `apps/`, `crates/` and `tools/`, tests included, by the Rust
files that name each crate (`grep -rlw` of its Rust name, the library's by a word-bounded
`symphony::`, less two files of its gate, which links no library: one declares a module of its own
named `symphony`, the other names the library's `symphony::commit` in a doc comment alone; read 2026-11-13). "Guard" is the test here that compiles what a consumer calls
(`tests/consumer_api.rs` in five packages: [RELEASES](../RELEASES.md#the-release-run), step 4).

| Package | Its packages that link it | Files | What it uses | Stated as a contract? | Guard |
|---|---|---|---|---|---|
| `symphony-plugin` | its host, its plugin crate | 18 | the `Plugin` trait and the seams its plugin implements, linked into its host | yes: "Each seam below takes and returns plain data … never a Symphony library type" (`lib.rs`, the crate's doc) | `crates/symphony/tests/consumer_api.rs`, the traits by the methods a plugin writes |
| `symphony-api` | its host's tests, its plugin crate | 13 | the block vocabulary and the seams' data, re-exported as `symphony_plugin::vocabulary` | the crate is "the Symphony's wire types: the JSON the server … sends and the client … reads" (`lib.rs`); the blocks are part of it | not held: the blocks it builds are caught only by the conciliation (that guard's doc) |
| `symphony-docs` | its plugin crate, three crates and three tools | 15 | `DocCheck`, `Docs`, `Problem`, the register's syntax | re-exported by the plugin crate (`pub use symphony_docs::{DocCheck, Docs, Problem}`) | `crates/symphony` guard |
| `symphony` (the library) | its host (4 files), its plugin crate (27), an art tool (9) and its review tool (4) | 44 | the host's `cli::main` and `plugins::install`; in its plugin and tools, internals: `Project`, `register::parse_decisions`, `landing::generators`, `text`, `markup::escape`, `images::reserve`, `edit::heading_anchor`, `render::Render` and more | no | `crates/symphony` guard, by signature |
| `symphony-board` | its `tasks` package, its host's tests, its review tool, its plugin crate | 73 | its `tasks` and `agent-send` are the board's own `main`s; its review tool opens the live board as a writer (`Store::open` in `tools/gate-review/src/nightly/board.rs`) and links `assessment_publication`, `sweep`, `red`, `slots`, `on_review`, `alarm` | no | `crates/tasks/tests/consumer_api.rs` |
| `symphony-server` | its host, its plugin crate | 7 | `binary::run`: its host builds the machine's one server, its plugin linked in | no | `apps/symphony-server/tests/consumer_api.rs` |
| `process-cwd` | five packages | 132 | starting child processes | no | `crates/process-cwd/tests/consumer_api.rs` |
| `symphony-schema` | two tools, a crate its plugin links | 7 | JSON Schema checks | no | `crates/symphony` guard |
| `repository-read` | its gate, a content crate | 7 | the repository reader | no | `crates/repository-read/tests/consumer_api.rs` |
| `symphony-args` | two tools | 2 | argument parsing | no | `crates/symphony` guard |

The guards compile under `just lint-rs`, so a change that breaks a name they hold fails its own
task's gate. What they do not hold, by their own account: the blocks a consumer builds, the
signature of a function they name alone, and a field read or a method called on a value no guard
names (`crates/symphony/tests/consumer_api.rs`, the file's doc).

### The other surfaces

| Surface | What the consuming project does with it | Stated? | Versioned? |
|---|---|---|---|
| The plugin process protocol ([PLUGIN_PROTOCOL](../PLUGIN_PROTOCOL.md)) | nothing yet: its plugin is linked (its `symphony.toml` has no `[plugins.process]`), and it declares seams a process does not answer (`unanswered` in `crates/symphony/src/plugins/process.rs`: renderers, routes, navigation, home regions, board actions, watches, doc checks, commands, a theme), with card kinds that would cost about eight round trips per card | yes | yes: 0.2, every call naming its minor; another major is refused by name (`Version::agreed`) |
| The board's schema | its own `tasks` build and its review tool write its live board at the pin's schema | yes (`schema.rs`, its doc) | yes, 17: a writer opens a board only at its own version, a reader at its own or one ahead, read-only (`decide`); it went from 9 on 2026-11-02 to 17 on 2026-11-12 |
| `tasks api` (JSON in, JSON out) | its notices are told through it (`tools/gate/src/cargo/notice/delivery.rs`), and its board launcher reads the schema's and the api's `VERSION` from Symphony's committed source (`tools/tasks.sh`, `contract`) | yes (`api.rs`, its doc) | yes, 1.14, its minor raised each time a method takes or does something an earlier build did otherwise |
| `symphony.toml` | the machine's one server reads it for every project | in `crates/symphony/src/project.rs` | no. The server reads it through `Project::of`, which leaves out a table or key the build does not know, says so on stderr and reads the rest, "so a file a newer Symphony wrote keeps its settings"; only `Project::load` refuses the file, as registration (`project add`, a server started from that checkout), `status --lint` and `init` read it (`project/file.rs`, `refusal`). Three tables are strict wherever the file is read (`deny_unknown_fields`): an unknown key in an `[[entrypoints]]` entry (`project/file.rs`, `FileEntrypoint`), in a landing generator (`[land]` or `[verdicts] generators`, the board's `land::Generator`) or in a `[board] machine_wip` entry (`project/board.rs`, `WipGuidance`) fails the file, and the server then serves that project on the defaults. The board's own reading of `[land]`, which `tasks land`, `tasks done --covered-by` and the regenerator make, refuses a key it does not know anywhere in that table (`crates/tasks/src/land.rs`, `Settings::of`) |
| Recipes, skills and hooks (`symphony init`) | it keeps recipes and skills of its own; its Claude settings run its own build's `symphony hook tasks-beat` | the names, in AGENTS.md | no |
| The client | embedded in the server its host builds, from the pin's committed distribution (`apps/symphony-server/build.rs`) | yes | rides with the server |
| Symphony's source tree | its tests find Symphony's checkout through `cargo metadata --locked --offline` and copy its justfile, scripts and manifests into fixtures (`tools/gate/tests/gate/symphony_source.rs`, which also asserts the pin is a `branch = "main"` source); one test counts words in Symphony's own text (`tools/symphony/tests/vocabulary.rs`) | no | no |
| The machine | its registry entry holds its pin and board schema (`projects.json`); the machine's `server.json` names its checkout, so the one server serving every project, this repository's included, is its host's build at its pin | yes (`crates/symphony/src/machine/host.rs`) | by `PIN` and the schema: a host refuses a project registered at a newer one (`Build::lacks`) |

### What broke it

- **2026-11-12, the first release run.** Two public names of the board's library that its code
  calls: `cli::board_client` removed, and a field `key` added to `land::Generator`, which its test
  builds by its four fields. Both were put back and the guards added (the board tasks of that
  run's conciliation red).
- **Its four pin moves before 2026-11-12** broke only its tests of Symphony's internals:
  `verdicts::BoardTask` gained a field, `docview::folder` an argument, and a usage text changed
  ([the plugin delivery analysis](plugin-delivery.md#what-changed-in-the-last-twenty-landings)).
- **2026-11-13, the release candidate's first round:** 51 tests failed. 19 were this repository's
  own: the conciliation's patch reached only the first cargo, while the consumer's tests start
  cargo runs of their own that resolved an older Symphony; fixed by moving the copy's lock instead
  (the round's conciliation red, on the board). The other 32 fail at the consumer's own pin with no
  Symphony change: 8 of them on rules of the board already in that pin (a subtask is never moved to
  review, a reviewer cannot judge a task it filed, a draft card is not listed), one counting words
  in Symphony's text, the rest the project's own. The second round (15:29) failed on 31, all red at
  the project's own pin too.
- **The plugin seams and the block vocabulary** broke nothing: the seams gained two methods with
  defaults (2026-11-11 and 12), the block vocabulary (`crates/symphony-api/src/blocks.rs`) has not
  changed since 2026-11-04, and the sources of `symphony-docs` and `symphony-schema` not since
  2026-11-05 (`git log`).

So what breaks a consumer is its links to the internals of the library and the board, and the
board's behaviour, not the seams. Last week (994 commits since 2026-11-06, merges left out),
`crates/tasks/src` changed in 270 commits, `crates/symphony/src` in 198, `apps/symphony-server/src`
in 125 and `crates/symphony-api/src` in 46; the plugin crate's `lib.rs` in 6, its `protocol.rs` in
7, `process-cwd` in 13, `symphony-env` in 4, `repository-read` and `symphony-args` in 3 each,
`symphony-docs`, `symphony-schema` and the blocks in none (`git log --name-only`, counted per path).

## What Cargo allows

### A Git pin names a commit, not a version

"Cargo fetches the `git` repository at that location and traverses the file tree to find
`Cargo.toml` file for the requested crate anywhere inside the `git` repository", with `branch`,
`tag` or `rev` choosing the commit, and "Cargo locks the commits of `git` dependencies in
`Cargo.lock`" ([specifying dependencies](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html)).
A `version` beside `git` "does _not_ affect which commit is used" (same page): it is only checked.
So a Git dependency has no version range: Cargo takes the commit named and nothing else, and the
release's conciliation moves the lock with `cargo update -p 'URL#NAME' --precise COMMIT`, which
moves every package taken from that source at once ([step 4](../RELEASES.md#the-release-run)).

### Two references, two copies

The Cargo pages do not say what happens when one graph takes the same repository at two
references, so it was tried: a scratch manifest outside every checkout, resolved offline with the
pinned toolchain against commits Cargo's cache already held (`cargo generate-lockfile --offline`,
then `cargo tree -d -e normal`); nothing was compiled.

| The manifest takes | `symphony-api` packages in the lock |
|---|---|
| `symphony-plugin` and `symphony-api`, both at `rev = 7f229c54…` | 1 |
| `symphony-plugin` at `7f229c54…`, `symphony-api` at `b8e59102…` | 2: the plugin's own `symphony-api` from `7f229c54`, and the manifest's from `b8e59102` |
| `symphony-plugin` at `rev = "7f229c54a2d16c7bc9a67fbf18732dda63457334"`, `symphony-api` at `rev = "7f229c54a2d1"`: the same commit, written two ways | 2, both `v0.1.0`, both at `7f229c54` |

Cargo accepts all three. A package's path dependencies come from its own source, so the plugin
brings its own `symphony-api`. And a source is its URL with its reference as written: the same
commit named twice is two sources, so two tags on one commit (one per package) are two sources too.

Two copies are two crates to the compiler: "the types and items are considered different by the
Rust compiler, even if they have the same name", which "usually manifest as a compile-time error,
but sometimes they will only appear as a runtime misbehavior", such as a failed `downcast_ref`
([the resolver](https://doc.rust-lang.org/cargo/reference/resolver.html#version-incompatibility-hazards)).
The plugin's trait returns `symphony_plugin::vocabulary::Block`, which is the plugin's own copy of
`symphony_api::Block`; a consumer building blocks with another copy cannot return them. So over Git,
packages whose types cross (here: the plugin seams, the blocks, the doc checks; the library and
everything it re-exports) can only be taken at one reference together. Packages whose types cross
nothing may sit at references of their own, at the cost of one more checkout of the whole
repository in Cargo's cache each: 217 MB for one today, 171 MB of it its `.git` (`du` of
`~/.cargo/git/checkouts/symphony-…/b8e5910`).

### A registry unifies by version

A registry is the other kind of source: an index of published packages and their versions.
"Cargo assumes packages follow SemVer and will unify dependency versions if they are SemVer
compatible", and two compatible versions that cannot be unified because of conflicting
requirements are an error ([the resolver](https://doc.rust-lang.org/cargo/reference/resolver.html#semver-compatibility)).
So with a registry, a plugin kit at 1.4 asking for blocks `"1.2"` and a consumer asking for
blocks `"1.3"` share one copy, the newest 1.x; asking for `0.2` and `0.3` brings two, since "Versions
are considered compatible if their left-most non-zero major/minor/patch component is the same"
([specifying dependencies](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#default-requirements)).

- **Where.** crates.io is public: "crates.io does not accept packages that depend on crates from
  other registries" and does not permit path dependencies ([registries](https://doc.rust-lang.org/cargo/reference/registries.html),
  [specifying dependencies](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#local-paths-in-published-crates)).
  A private one can be small: "A minimal registry can be implemented by having a git repository
  that contains an index, and a server that contains the compressed `.crate` files created by
  `cargo package`. Users won't be able to use Cargo to publish to it, but this may be sufficient
  for closed environments" ([running a registry](https://doc.rust-lang.org/cargo/reference/running-a-registry.html)),
  and its downloads may be "https, http, and file URLs" ([the index](https://doc.rust-lang.org/cargo/reference/registry-index.html)).
  So a release publishing to a minimal one writes the index entries and uploads the files itself;
  `cargo publish` needs a registry that also serves the web API the same page describes.
  A consumer names it in `.cargo/config.toml` (`[registries]`) and each dependency with
  `registry = "NAME"`; publishing to it is allowed by `package.publish` listing it.
- **Exact pins conflict.** This workspace takes `tokenizers = "=0.22.1"` and the `tract` crates at
  `=0.22.5` (`Cargo.toml`, `[workspace.dependencies]`). Two versions of the board in one graph that
  pin different patch releases of these cannot resolve at all, as the resolver page's `log
  =0.4.11` against `=0.4.8` example says.
- **One native library per graph.** "The resolver will attempt to find a graph where there is only
  one instance of each `links` name" (the resolver, `links`): two versions of a package that link
  the same native library through incompatible releases are refused.
- **Two versions on purpose.** A consumer may name both, under two names: "Depend on multiple
  versions of a crate" with `package = "…"` ([renaming dependencies](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#renaming-dependencies-in-cargotoml)).
  Their types still do not mix.
- **Sharing types across a major.** An old major can re-export the new one's types, so the two
  are "precisely the same type": the semver trick ([semver-trick](https://github.com/dtolnay/semver-trick)),
  "not the silver bullet that solves all occurrences of dependency hell".

### Which packages could version apart

What each package's public items expose of another's, read from its `pub use` lines and its
signatures:

| Package | Its types reach the public items of | So it can version apart |
|---|---|---|
| `process-cwd` | `symphony-board` (`sys.rs` re-exports `exit::Exit` and its `spawn` items) | yes, once no consumer links the board |
| `symphony-args` | `symphony` (`cli.rs` re-exports `Flag`, `Parsed`, `parse`) | yes, once no consumer links the library |
| `symphony-schema` | `symphony` (`lib.rs`: `pub use symphony_schema as schema`; `text::repr`) | the same |
| `repository-read` | none (a dev-dependency of `symphony-api`) | yes, today |
| `symphony-env` | variable names in the board, the reader and the server | no consumer names it; it moves with them |
| `symphony-docs` | `symphony-plugin` (`DocCheck`, `Docs`, `Problem`), `symphony` (the register's syntax) | only with the plugin seams |
| `symphony-api` | `symphony-plugin` (the vocabulary), `symphony`, the board, the server, the client | its blocks only with the plugin seams; its app wire types with the server and the client |
| `symphony-plugin` | `symphony`, the server | the seams, the blocks and the doc checks are one unit |
| `symphony-board`, `symphony`, `symphony-server`, `symphony-ui`, `chrome-devtools` | one another | no: the library re-exports the board, the plugin seams, the wire types, the doc checks, the arguments and the schema checks; the server links the library and embeds the client |

So two units could carry version numbers that mean something: the plugin kit (the seams, the
blocks, the doc checks) and each leaf helper (`process-cwd`, `symphony-schema`, `symphony-args`,
`repository-read`). The library, the board and the server move together, and with the machine.

### What Cargo does not check

- **A leaked type.** Marking a dependency public or private, so that rustc warns when a private
  one's type shows in a public item, is still unstable: "The 'public-dependency' feature allows
  marking dependencies as 'public' or 'private'", enabled with `-Zpublic-dependency`
  ([unstable features](https://doc.rust-lang.org/cargo/reference/unstable.html#public-dependency),
  tracking issue 44663). On this toolchain a kit that re-exports a helper's type is caught only by
  reading.
- **A breaking change.** Which changes need a new major is written down
  ([SemVer compatibility](https://doc.rust-lang.org/cargo/reference/semver.html)): "Major: adding a
  public field when no private field exists", "Major: adding new enum variants (without
  `non_exhaustive`)", "Possibly-breaking: adding a defaulted trait item", "Major:
  renaming/moving/removing any public items"; and for 0.x, changes in "y" count as major.
  `cargo-semver-checks` compares a package with an earlier one (`--baseline-rev`, "Git revision to
  lookup for a baseline"), "does not have to be published anywhere", but "doesn't yet have lints for
  all of them" ([cargo-semver-checks](https://github.com/obi1kenobi/cargo-semver-checks)), and no
  tool sees a change of behaviour, such as a board rule.

## What one machine shares

### Today

- **One server for every project.** The machine runs one server (`crates/symphony/src/machine.rs`),
  and with linked plugins it must be a build that links every registered project's plugins
  (`machine/host.rs`). Today that is the consuming project's host, at its pin: `server.json` names
  its checkout. A host serves a project registered at its pin and board schema or older ones, and
  refuses a newer one (`Build::lacks`). So this repository's own pages are served at the consuming
  project's pin.
- **Each board's schema.** Each project has its own board, but every program that opens it must
  be at its schema to write, and the server reads a board at its own version or one ahead, read
  only; a board behind the server is refused before it is read (`schema.rs`, `decide`). The schema
  moves forward only, by the project's `just tasks migrate`, or by a refresh after a snapshot
  ([the refresh](../RELEASES.md#the-refresh)). The rule chosen for later moves each board at the
  machine's update once every program that writes it is on the release
  ([the plugin delivery analysis](plugin-delivery.md#the-board)).
- **The settings file** of every project is read by that one server, which leaves out a table or
  key it does not know and reads the rest (`Project::of`), so a file written for another release
  is served with what this one knows. Registration, a start from that checkout and the lint refuse
  such a key (`Project::load`). Three tables are strict wherever the file is read: a misspelt key
  in an `[[entrypoints]]` entry, a landing generator or a `[board] machine_wip` entry fails the
  file, and the server serves that project on the defaults. The board's own reading of `[land]`
  (`tasks land`, a covered close and the regenerator) refuses a key it does not know anywhere in
  that table, so a release's `tasks` refuses an older project's `[land]` that holds a key it retired.
- **The client** is the server's own, embedded.

### One release behind

What a consuming project one release behind the machine would need from each, and what it costs:

| Shared | A project one release behind needs | Cost |
|---|---|---|
| The server | a server of release N serving a project whose code is of N-1 | with linked plugins, none: the server is the build that links the plugin, so it is the project's version; with plugin processes, the protocol below |
| The plugin protocol | the host answering a plugin built at an older minor, and at the previous major for a window | built for minors: the host speaks every minor of its major (`Version::agreed`); a kept major is one more set of request and answer types beside the current, as Zed keeps one set of bindings per API version ([the plugin delivery analysis](plugin-delivery.md#webassembly-components)) |
| The board's schema | a server at schema N reading and writing a board a project's own programs keep at N-1 | high: every write the server makes in two shapes, or that project's pages read-only. None at all when no program of the project's writes its board but the machine's own `tasks` |
| `tasks api` | the methods of N-1 answered as they were | low: within a major, add and never remove or change |
| `symphony.toml` | a file written for N-1 still loading at N | low: the server already leaves a retired key out and reads the rest, the default taking its place; what is left is the lint and registration warning of it for a window rather than refusing it, and the same in the three strict tables and in the board's reading of `[land]`, which refuse it today |
| Recipes, skills, hooks | the names N-1's settings call still answering | low: keep each name for the window |
| The client | nothing | it rides with the server |

The schema is the one where a window costs more than it gives. A board behind the server comes
only from a project's own program writing it at an older schema; the cheaper way out is that no
project program writes its live board but the machine's `tasks`, which `tasks api --lines` already
serves as JSON.

## The release under a contract

### The conciliation becomes contract checks

Today the conciliation runs `cargo test --no-fail-fast` over every package of the project that
takes a Symphony package, in a scratch copy whose lock is moved to the candidate
([step 4](../RELEASES.md#the-release-run)): 13, 90 and 23 minutes in the last three runs, and red
on the project's own reds as much as on Symphony's (above). Under a contract it is what this
repository promises, checked by tests it owns:

- **The Rust names promised:** the guards that exist, narrowed to the packages that carry a
  version, and `cargo-semver-checks` of each such package against its last released tag, which
  says whether the change is a minor or needs a new major.
- **The plugin protocol:** each registered project's plugin exchanges, requests and answers kept
  from its process (a request log beside the plugin's own log, as the plugin delivery design
  describes; not built), replayed against the candidate's host: each answer read, each block
  drawn. A plugin of each protocol major still kept is started and declared.
- **The board's interface:** `tasks api` calls of each kept version answered as before, and a
  board at the previous schema moved by the candidate's migrate on a scratch board.
- **Settings:** each registered project's `symphony.toml` loaded by the candidate as the server,
  the lint and the board's `[land]` reading load it, so a key the candidate no longer knows is
  named before the release rather than left out by the server after it, and a file the candidate
  would serve on the defaults or whose `[land]` it would refuse is red.

What it gives up: the project's own tests no longer run in Symphony's release. A break the
contract does not cover, such as a changed board rule a project's fixture relies on, is found when
that project moves its own pins, by its own gate. The project's suite can leave the release before
any of these checks is new: the consumer-API guards, the protocol's tests and `tasks api`'s already
run in the full verification, and hold the Rust names until the kit's semver check does.

### The refresh becomes the machine's install

Today the refresh moves each consumer's pin to the release, fast-forwards its main and migrates its
board ([the refresh](../RELEASES.md#the-refresh)). Under A or B it installs the release on the
machine (server, board commands, document command line, client), moves each board under the rule
already chosen, and moves no project's pin: each project moves its kit and its helpers through its
own landing, when it chooses, and its own gate runs its own suite then. That is the phased update.

### What RELEASES would say

Under B, in the words RELEASES would then use:

- **A release** is the machine's: one tag for the server, the board commands, the document
  command line and the client, installed on the machine by the update; its message names the
  protocol majors and minors it serves, the `tasks api` version and the board schema.
- **A kit release** is a version of its own published on crates.io (such as `1.4.0` of the kit's
  packages), by the same run when the kit changed, its number set by the semver check against the
  previous published version; each helper the same, on its own line
  ([where versions live](#open-choices)).
- **The conciliation** is the contract checks above; no project's suite runs in the release.
- **The refresh** installs the release on the machine; a project's pins are its own to move.
- **A project still on a protocol major the release no longer serves** is named before the
  release. The protocol is on its first major, so what a release does with such a project is
  asked when a second is planned.

## The options

| | A. The process boundary only | B. A small versioned kit and helpers, and A | C. Every package versioned |
|---|---|---|---|
| A consumer links | nothing of Symphony but the kit its plugin binary serves with, at any release | the kit (seams, blocks, doc checks) and the helpers it wants, each at its own version | what it links today, each package at its own version |
| It can stay behind on | its plugin binary, while the machine speaks that protocol major; its `tasks api` and command clients, within their versions | A's, plus the kit's version, and each helper for as long as it likes | any package, within what the others' version ranges allow |
| It cannot stay behind on | the server, the board commands, its board's schema, the client, the recipes | the same | the board's schema and the machine's server: a board writer of its own holds the schema, and the server is still one build |
| Symphony keeps compatible | the protocol's minors and a kept major; `tasks api`; command names and outputs; settings keys | A's, plus the kit's and each helper's Rust names within a major | every package's public names within its major, the library's and the board's included |
| Distribution | one release tag; the kit at any tag | one release tag, one tag line for the kit, one per helper; Git is enough, though the versions live on crates.io ([below](#open-choices)) | a registry, private or crates.io |
| The release judges | contract checks | contract checks and semver checks | semver checks of every package and their combinations; the project's suite or a matrix |
| A break of a promised name shows | as a refused call or a default answer at run time | at the kit's or helper's semver check, at its task's gate | at each package's semver check |

### A. The process boundary only

The contract is what crosses a process: the plugin protocol, `tasks api`, the commands, their
output, the settings file and the hook names. No Rust name is promised. The machine runs one
Symphony release: the server, `tasks`, `agent-send`, `symphony` and the client, as the plugin
delivery design's fifth slice installs it.

- **A consumer stays behind on** its plugin binary, built with whichever `symphony-plugin` it last
  took, for as long as the machine's server speaks that protocol major; and on its scripts and
  tools that call `tasks api` or the commands, within their versions. How long a major lasts is asked
  when a second one is planned.
- **Symphony keeps compatible** every minor of the protocol's major, the previous major for that
  window, the `tasks api` methods, the command names and outputs a project parses, and the
  settings keys: a retired one the server already leaves out, and the lint, registration, the
  three strict tables and the board's `[land]` reading would warn of for the window rather than
  refuse.
- **The work here, in order:**
  1. Every seam answered from a process: renderers, routes, navigation, home regions, board
     actions and fields, gates, watches, doc checks, prose rules, generators, commands, transports
     and a theme, each refused today (`unanswered`).
  2. A register's card kinds asked in one request a read, not about eight round trips a card.
  3. The release installed on the machine and the server no longer a project's build: the host
     rule becomes the protocol's (the plugin delivery design's fifth slice, and its update
     command).
  4. The windows written down and tested: a plugin built at an older minor and at the kept major
     served by the candidate's host.
  5. The release run: the contract checks in place of the conciliation, the refresh as the
     machine's install; RELEASES rewritten.
  6. The Rust guards dropped, since nothing Rust is promised.
- **The work on the consuming project's board:** its host no longer built; its plugin a process;
  its review tool's board writes through `tasks api --lines`; its uses of the library's internals
  in its plugin, its art tool and its review tool's landing (register parsing, landing checks,
  text helpers, `Project`, status and triage; 40 of the 44 files that name the library) copied
  into its own code or reached through commands; and the helpers it links (`process-cwd` alone in 132 of its files, `symphony-docs`,
  `symphony-schema`, `symphony-args`, `repository-read`) copied, since nothing of Symphony's is
  linked.
- **What it cannot give:** a compile error when a plugin and the host disagree (a refused call or
  the seam's default answer instead); a promise that rebuilding the plugin with a newer kit needs
  no code change; a project keeping an older server or board than the machine's.

### B. A small versioned kit and helpers, and A (recommended)

A's contract, and a few packages whose Rust names carry a semver promise, each on a version line
of its own:

- **The kit:** the plugin seams, the block vocabulary and the seams' data, and the doc checks, one
  version for the three, since their types cross. The block vocabulary leaves `symphony-api`,
  whose app wire types (46 commits last week) stay with the server and the client; the blocks
  have not changed since 2026-11-04.
- **The helpers:** `process-cwd`, `symphony-schema`, `symphony-args` and `repository-read`, each
  with its own version, provided no promised package's public items show their types.

Over Git this works without a registry: a consumer names the kit's three packages at one kit tag
(one source, one copy each), and each helper at its own tag (two sources, but they share no
types). A registry is needed only once version ranges must unify, such as two kit-dependent crates
of a consumer each asking for a different minor. One is used all the same, after B: the versions
live on crates.io ([below](#open-choices)).

- **A consumer stays behind on** its kit version, and so its plugin, while the machine's server
  speaks that kit's protocol major; on each helper as long as it likes, since nothing on the machine
  shares them; on its `tasks api` clients within the api's version.
- **Symphony keeps compatible** A's list, plus the kit's and each helper's Rust names within a
  major. A deliberate break is a new major, the old name kept beside the new for a release, as was
  chosen for breaks of a consumer's API.
- **The work here, in order:**
  1. The block vocabulary and the seams' data out of `symphony-api` into the kit, so the app's
     wire types move without touching it.
  2. Version lines: a real `version` in each promised package's manifest (0.1.0 everywhere when
     this was read), each version published on crates.io ([below](#open-choices)), and
     `cargo-semver-checks` against the previous version in the release run and at a task's gate
     when the package changed. The guards narrow to these packages and keep what semver checks
     miss (a generic function named alone).
  3. No promised package shows another's type unless they share a line; the library's and the
     board's re-exports of the helpers stop mattering once no consumer links those two.
  4. A's steps 1 to 5.
- **The work on the consuming project's board:** as under A, its host is no longer built, its
  plugin is a process and its review tool writes the board through `tasks api --lines`; it keeps
  linking the kit and the helpers at the versions it names. What its plugin takes from the
  library's internals (27 of the 44 files that name the library) moves into the kit or comes in
  the protocol's answers (the plugin delivery design's fourth slice, the plugin on the kit alone).
  Two other users of the library need an answer of their own:
  - **Its art tool** (9 files) installs its plugin into the library and runs the library's status,
    triage and page rendering (`tools/art/src/cards.rs`, `review_triage.rs`,
    `tests/golden_card.rs`). Its routes: the machine's commands, so the tool checks what the
    machine shows, or copies of its own. Its register parsing and text helpers are small enough
    to copy whichever route the rest takes.
  - **Its review tool's landing** (three files of `tools/gate-review/src/land/` and a test) takes
    the landing's checks (`landing::regenerates`, `is_doc`, `owner_lines_left`), `Project::of`
    and the lock the server's document writes take (`commit::write_lock`). Its routes: commands
    of the machine's, or copies of its own. Whichever it takes, that lock is a file two programs
    must agree on, so its place and rule join the contract.

  Neither keeps the library, which B does not let a project link. At any tag but the kit's it
  would also be a second source, which brings a second copy of the kit: a plugin built on the kit
  no longer fits that library's plugin list (inferred from the resolver page, as above; not
  compiled).
- **What it cannot give:** a project keeping an older server or board than the machine's; a
  semver check that sees every break, or any change of behaviour, such as a board rule its fixtures
  rely on (8 of the 31 reds of 2026-11-13); and the library's internals as a promise, which stay
  free to move.

### C. Every package versioned on its own

Each of the 14 packages on its own version line, published to a registry, a consumer linking what
it links today, each at the version it names.

- **A consumer stays behind on** any package within what the others' ranges allow: the board at
  0.4 beside the library at 0.7 only if the library 0.7 accepts board 0.4.
- **Symphony keeps compatible** the public names of every package within its major. That is every
  public item of the library and the board, whose sources 198 and 270 commits touched last week
  (commits, not counted changes to public names); a change to any public name is a version
  decision, and the packages' combinations a consumer may hold are
  tested together or not at all.
- **The work here:** a registry (Git pins cannot do it: two references are two copies, above),
  with its publishing written by hand for a minimal one or its web API served;
  `publish` opened and a real version in each manifest; semver checks for every package;
  compatibility tests across the versions a consumer may combine; and, for the phase to reach the
  board, a server that reads and writes a board one schema behind.
- **The work on the consuming project's board:** none at first: it keeps its links.
- **What it cannot give:** the machine's server and each board's schema stay one per machine. With
  linked plugins the server is still a build of one project's pins, so it lags with that project
  and serves every other project at its pins, as today; and a board library kept behind by a
  project's own tool still holds that board's schema, so the server refuses the board or the tool
  is refused. The exact pins of this workspace (`=0.22.1`, `=0.22.5`) make two board versions in one
  graph fail to resolve whenever they differ.

### What none of them gives

A project keeping an older server, older board commands or an older board schema than the
machine's. Under each option those are one per machine and move together for every project at
the machine's update, as each board's schema does
([its card](plugin-delivery.md#board-schema-at-update)). What a contract can phase is a project's own
code: its plugin, its tools and the helpers it links.

- [r] **consumer-contract-versioning** — A consuming project could take Symphony's parts one at a time, each when it is ready, and Symphony's releases would stop waiting on that project's whole test suite; how much can be phased depends on how far the contract goes. Today the project takes all of Symphony at one commit, and the machine's one server is built from it. How far should the contract go? A: the process boundary only: a project links nothing of Symphony, its plugin is a program of its own speaking the versioned plugin protocol, and the machine runs one Symphony release for the server, the board commands and the client. B: A, plus a few packages with version numbers of their own that a project may keep linking at the version it names: the plugin kit (its seams, blocks and doc checks) as one, and each small helper (child processes, schema checks, arguments, the repository reader). C: every one of the 14 packages versioned on its own, a project linking what it links today. Recommended: B. Approve and say A, B or C in the note, or comment. [[analysis/consumer-contract#The options]]
  [2026-11-13 18:42] agent — Posted for the board task `docs/consumer-contract-analysis`, from your comment on **conciliation-consumer-own-reds**.

    **What a project can stay behind on:**
    - A: its plugin program, while the machine's server speaks that program's protocol major (how many releases a major lasts is a number to set), and its scripts and tools that call `tasks api` or Symphony's commands, within those versions.
    - B: A's, plus its plugin kit's version, and each helper for as long as it likes, since nothing on the machine shares them.
    - C: any package, within what the others' versions accept.
    - None of them: a server, board commands or a board schema older than the machine's. Those stay one per machine and move together at its update, as you chose for the board: "C makes sense to improve upon A. while we don't have a dedicated build service."

    **What Symphony must keep working:**
    - A: every minor of the plugin protocol, and the previous major for that window; the board's `tasks api` calls; command names and outputs; settings keys (the server already leaves out one it does not know and reads the rest; the lint and registration would warn of a retired one for the window rather than refuse it).
    - B: A's, plus the kit's and the helpers' Rust names within a major, a check against the previous version's tag saying when a change needs a new one. Last week 0 to 13 commits touched these parts' sources.
    - C: the Rust names of every package, the library and the board included, whose sources 198 and 270 commits touched last week (commits, not counted changes to public names); and a package registry, since with Git pins two versions of this repository bring two copies of every package they share (tried here), whose types do not match (from Cargo's documentation; not compiled).

    **The work:**
    - Here, A and B: finish the plugin path you chose ("Let's go with B first, measure results against the current approach for a post change comparison."): every seam answered from a process, and the release installed on the machine instead of built by a project; then checks of the contract in place of the conciliation. B adds the kit split out of the wire types, and its version tags. C adds the registry, version checks for every package, and tests across the versions a project may combine.
    - On the consuming project's board, A and B: its host is no longer built (4 files); its plugin becomes a process and leaves the library's internals (27 of the 44 of its files that name the library); its review tool writes the board through `tasks api` rather than the board's library. Two other users of the library need an answer: an art tool (9 files), which runs Symphony's status, triage and page rendering with its plugin installed, and its review tool's landing (4 files), which takes the landing's checks and the lock it shares with the server. Under A both reach those through the machine's commands or copy them, and the project also copies the helpers it links, one of them in 132 of its files. Under B the analysis proposes the art tool through the machine's commands, and the landing on the library at the kit's own tag, which promises nothing, so a break there is the project's own, found by its gate.

    **What changes for releases, under A and B:** a release is judged by checks this repository owns (the plugin protocol, `tasks api`, the kit's version check), never by a project's whole suite, so a project's own reds stay on its own board. The release is installed on the machine and moves no project's pin; each project moves its kit and helpers through its own landing. Until that is built, **conciliation-consumer-own-reds** decides what holds a release.

    **Why B.** It phases what a project's own code can phase, and promises stability only where little changes. A makes the project copy code it links today, and possibly change its plugin at whatever kit it rebuilds with, for no promise in return. C promises stability on the packages that change most, needs a registry, and still cannot phase the server or a board.

    **Earlier verdicts checked.** **plugin-delivery-choice** (B), **release-delivery** (C) and **board-schema-at-update** (C) chose how plugins, releases and boards reach the machine: A and B build on them, and C leaves them as they are. **symphony-update-command** (B) has projects follow release tags, which every option keeps. **consumer-api-break-release** (A) keeps an old name beside the new for a release, which B keeps for its kit. None says whether packages are versioned apart, so none decides this card.

    This card is the only change. The build is filed after your verdict. [[analysis/consumer-contract#Why B]]
  [2026-11-13 19:39] owner · approved — There are multiple A/B/Cs here. No way to decide on all things. Those are separate decisions. For the original question, I agree with approach B
  [2026-11-13 20:27] agent — Recorded: B, for the question this card asked. A project links nothing of Symphony but the plugin kit (its seams, blocks and doc checks) and the small helpers (child processes, schema checks, arguments, the repository reader), each at the version it names; its plugin is a program of its own speaking the versioned plugin protocol; and the machine runs one Symphony release for the server, the board commands and the client.

    **What B decides, so none of it is asked:**
    - **Where versions live:** this card gives B "its version tags" and the registry to C, so the kit and each helper get a line of tags in this repository, cut by the release run; Git is enough. The kit and the helpers start at 1.0.0: for a pinned tag Cargo reads 0.x and 1.0 alike.
    - **The board's rules:** under every option the board commands "stay one per machine and move together at its update", so a changed rule reaches every project at the machine's update, the release's notes saying so, and a project's test that relied on the old rule is that project's to update. `tasks api` promises what its calls take and answer, not the rules behind them.
    - **An old protocol major:** the protocol is on its first major and no setting or command is retired, so how long an old one is kept is asked by the task that first plans a second.

    **The one choice left** is a card of its own: **contract-tool-route**, how the project's art tool and landing tool get what they use of the library today.

    **The build, in order on the board:** the release candidate's red already under way takes the consuming project's suite out of the release run, so its own reds no longer hold a release. Then the refresh leaves a project's own refusals to it; `tasks api` answers its version; the kit leaves the wire types, carries what a plugin uses and gets its tags; every seam a plugin declares answers from a process; and last the release is built and run on the machine, from which a release moves no project's pin.
  [2026-11-13 20:43] agent · resolved
  [2026-11-13 21:25] agent — The owner, 2026-11-13, after this verdict: "I would like to publish crates (MIT license) to crates.io as the pacakge manager". So under B the kit's and the helpers' versions live on crates.io, not on tag lines in this repository: this replaces the record's "Git is enough" (board task `tools/publish-to-crates-io`). The record's "The kit and the helpers start at 1.0.0" was the agent's, not your words; it holds only because the kit's blocks have now left the app's wire types (board task `tools/kit-vocabulary-leaves-the-wire-types`), as with them inside it nearly every release would need a new major. So the kit, its three packages at one version, and each helper start at 1.0.0, and the app's wire types (`symphony-api`) at 0.1.0, which promises no name.

## Why B

- **It phases what can be phased.** A project moves its plugin kit and each helper when it is
  ready, through its own landing and its own gate; the release never moves its pins.
- **It promises little, where little changes.** The sources of the kit's parts and the helpers
  were touched by 0 to 13 commits last week; those of the library and the board, which it leaves
  free, by 198 and 270 (commits, not counted changes to public names).
- **It builds on what was chosen.** Plugin delivery goes the process way first, measured against
  today's ([its card](plugin-delivery.md#plugin-delivery-choice)). B finishes that path (every seam
  from a process, the release installed on the machine) and adds the version lines a phased update
  needs. That design's target split already has the plugin SDK as a crate
  rebuilt when "a seam changes, by semver"
  ([the plugin delivery analysis](plugin-delivery.md#the-target-split)); B gives it the version
  line and the check.
- **A** costs the project copies of code it links today, and possibly a code change to its plugin
  at whatever kit release it rebuilds with, for no promise in return.
- **C** promises compatibility on the packages that change most, needs a registry, and still cannot
  phase the server or the board's schema.

The release keeps its full verification and its conciliation
([its card](symphony-update.md#symphony-update-command)), with the conciliation become checks of what
Symphony promises, so a project's own reds stay on its own board and no longer hold Symphony's
release.

## What stands of the tasks in flight

- **The per-project update command** (the board task that moves a project's pin to the newest
  release tag) moves every package at once. Under B it shrinks to moving the kit's and the
  helpers' tags, and the machine's update takes the rest.
- **Every seam from a process** (A's steps 1 and 2) is B's step 4: the seams a registered plugin
  declares, card kinds asked in one request a read first.
- **The settings and plugin checks** above wait for a first break of either: the server already
  reads a settings file leniently, and no project's plugin is a process yet.
- **The conciliation** is the first part of the release B's build replaces: the project's suite
  leaves the run before any new contract check exists, the consumer-API guards and the tests of
  the protocol and of `tasks api` judging it meanwhile.

## Open choices

One choice is the owner's, a card of its own in [DECISIONS](../DECISIONS.md): how the art tool and
the review tool's landing get what they use of the library today, through the machine's commands
or through copies of their own. Whichever, the write lock the landing shares with the server joins
the contract.

B decides the rest:

- **Where versions live:** on crates.io, the package manager, where the packages are published
  under MIT ([its card](#consumer-contract-versioning)). So not on lines of tags in
  this repository, which the options' table (Distribution) found enough for B: the kit and the helpers
  are published there, each version once ([RELEASES](../RELEASES.md#packages-on-cratesio)).
- **The board's rules:** the board commands stay one per machine and move at its update, so a
  changed rule reaches every project at once, the release's notes saying so. `tasks api` promises
  what its calls take and answer, not the rules behind them.
- **An old protocol major:** the protocol is on its first, and no settings key or command is
  retired, so how long an old major is kept is asked when a second is planned.

The first numbers: the kit (`symphony-plugin`, `symphony-blocks` and `symphony-docs`, one version
for the three) and each helper start at 1.0.0, the number whose majors and minors B's promise is
written in. The kit could take it only once its blocks had left the app's wire types for
`symphony-blocks`: with them inside it, nearly every release, changing a wire type, would have
needed a new major, which says nothing. `symphony-api`, the app's wire types, stays at 0.1.0 and
promises no name.

- [a] **contract-tool-route** — Under B a consuming project links nothing of Symphony but the plugin kit and the small helpers. Two of its tools use more of Symphony's library today, so each needs another way to get what it uses. Its art tool, which checks its art cards, sorts the register's cards with the project's plugin inside the library, runs Symphony's status and triage, and draws a card as a page draws it. Its landing tool uses the checks a landing makes (whether generated files must be made again, what counts as a document, whether your open lines are left) and the reader of the project's settings. A: Symphony's commands: both tools run the machine's own commands, which start the project's plugin as the machine does; Symphony adds the commands it lacks (one that draws a single card, one that answers the landing's checks for a branch) and keeps those commands' output stable. B: the project's own copies: it copies what the two tools use into its own code, and Symphony builds nothing. Recommended: A. Approve and say A or B in the note, or comment. [[analysis/consumer-contract#Open choices]]
  [2026-11-13 20:27] agent — Posted for the board task `design/contract-b-breakdown`: the one choice B leaves open, a card of its own as your note on **consumer-contract-versioning** asks.

    **What they use:** the art tool, in 9 files; the landing tool, in three files and a test. Both also read the register and use small text helpers, which are copied under either option.

    **The lock.** The landing tool also takes the lock the machine's server takes while it writes the main checkout. Under either option the lock's place and rule are written down as part of what Symphony promises, and the landing takes it itself, so it does not decide this card.

    **A** costs Symphony two new commands, and keeping their output and status's and triage's stable for the project to read. **B** costs Symphony nothing. The project keeps copies of how a card is sorted and drawn and of the landing's checks. They keep passing while the machine does otherwise, until someone copies again.

    **Why A.** Both tools exist to check what the machine does: the card you will see, and whether the machine would hold a land. Only the machine's own commands answer that.

    **When.** No Symphony task waits on this. It is needed when the project moves these two tools off the library, after its plugin.

    **Earlier verdicts checked.** **consumer-contract-versioning** (B) lets a project link only the kit and the helpers, so neither tool keeps the library, and it leaves open how they get what they use. No other card speaks of either tool. Symphony's side is filed after your answer.
  [2026-11-13 20:43] agent · auto-approved — A: the owner, 2026-11-13, in the terminal: "contract-tool-route reads like it should be auto triaged". The card's own evidence leaves A standing: both tools exist to check what the machine does (the card you will see, whether the machine would hold a land), and under B the project's copies "keep passing while the machine does otherwise, until someone copies again", so they would stop checking it. Symphony adds the two commands (one that draws a single card, one that answers the landing's checks for a branch) and keeps their output, and status's and triage's, stable; its side is filed on the board now. Any verdict here overrides this.

## Verified and inferred

- **Read in code:** each fact that names a file and an item, in Symphony at `22bdb4c4`; and, read
  only, the consuming project's workspace manifest, its packages' manifests, its `Cargo.lock`, its
  host and `tasks` binaries, `symphony.toml`, Claude settings, `tools/tasks.sh`,
  `tools/gate-review/src/nightly/board.rs`, `tools/gate/tests/gate/symphony_source.rs`, and the
  library's uses in its art tool and in `tools/gate-review/src/land/`.
- **Measured:** its uses of each crate (`grep -rlw` over its `apps/`, `crates/`, `tools/`); commits
  per package since 2026-11-06 (`git log --no-merges --name-only`); the pin's distance from main
  (`git rev-list --count`); one Cargo checkout's size (`du`); the machine's registry and server
  record (read in place).
- **Tried:** the three resolutions above, offline, toolchain 1.99.0, nothing compiled.
- **Read on the board:** the conciliation reds of 2026-11-12 and of the release candidate's first
  round, with their triage; the open and ready tasks named above.
- **From outside:** each claim beside its URL, read 2026-11-13.
- **Inferred:** that the plugin seams would need one major in the period read, from their history
  (two methods with defaults); that a registry is needed under C only, from the trials; the
  combinations under C that would need testing, not counted.
- **Not verified:** that two copies of the blocks, or of the plugin seams, fail to compile in the
  consumer's plugin or its art tool (it follows from the resolver page's words; nothing was
  compiled); what a private registry costs to
  host; whether `[patch]` could force one copy of a package taken at two references.
