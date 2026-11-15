# Code and architecture review, November 2026

A reading of the workspace's structure, its architecture risks, its code smells and its developer
documentation, as ranked findings a maintainer can act on. Each finding names its evidence, what it
costs today, the smallest change that would fix it and a size: small (an hour or two), a task, or a
project (several tasks). Nothing here is built; each finding is a task to file or a choice to put
in [DECISIONS](../DECISIONS.md).

Read at main's `f2381520` on 2026-11-15, with no build and no test run: the numbers are line
counts, `cargo metadata` and `cargo tree`, `git log`, `git cat-file` and grep. The Dioxus client
(`apps/symphony-ui`) is being replaced a page at a time by the React one (`apps/symphony-web`), so
it is read only for what its removal needs.

## The numbers

| Package | Source lines | Test lines |
|---|---|---|
| `crates/tasks` (the board) | 84,500 | 111,500 |
| `crates/symphony` (the library) | 60,900 | 65,400 |
| `apps/symphony-server` | 21,000 | 32,500 |
| `apps/symphony-ui` (Dioxus, leaving) | 15,400 | 37,200 |
| `apps/symphony-web` (React) | 36,200 lines of TypeScript, about half of them tests | |
| the eleven smaller crates | 21,300 | 11,900 |

Rust is 462,000 lines in all, with 6,338 tests. Comment lines are 13 % of the board's source lines
and 11 % of the library's, and about a third of the board's source by characters. The developer
guides and documents are 161,600 words. The repository holds 1,534 commits that are not merges,
from 2026-11-02 on.

The package boundaries hold. The dependency graph has no cycle and one direction: the server on
the library and the board, the library on the board, both on the wire types, the plugin kit on the
blocks and the document reader alone. The server holds no SurrealQL and no table name, and runs
git in one file. What crosses a boundary by the back door is named in findings 3, 8 and 9.

## The top ten, ranked by what they would save

### 1. The board's guide is 1,621 lines under one heading, and each change to the board edits two documents

- **Evidence.** `crates/tasks/AGENTS.md` is 20,500 words with one heading and 28 blank lines.
  `docs/TASKS.md` is 39,100 words. They share almost no text (262 of the guide's 20,525 eight-word
  runs are in TASKS), so neither repeats the other: both are the board's reference, split by no
  rule a reader can tell. Of the 1,534 commits, 230 change `docs/TASKS.md`, 208 the guide and 128
  both; they are the two most changed files in the repository, ahead of any source file. The root
  `AGENTS.md` (5,700 words, in every session's context) is third among the guides with 116.
- **Cost.** An agent told to read the guide before it edits the board reads 20,500 words it
  cannot skim, for a change that needs a tenth of them; so does its reviewer. Two branches that
  each add a paragraph to the same unbroken text conflict at the land.
- **Smallest change.** Make the guide what an editor needs before touching the crate (where each
  job lives, the invariants a change must keep, how to test against a scratch board), a few hundred
  lines under headings, and move each behaviour it describes to the section of `docs/TASKS.md` that
  owns it. The root guide keeps what every session needs and links the rest: how `tools/rs-bin.sh`
  decides freshness, what the memory guard misses and what `init` retires are reference, read by
  the few tasks that touch them.
- **Size.** A task for the board's guide, a task for the root guide.

### 2. The built client is committed, and is 62 % of the repository

- **Evidence.** `apps/symphony-server/client/public/` is 8.2 MB and 97 files in the tree. Its
  history is 4,316 blobs, 96 MiB of the 155 MiB all blobs take on disk, after thirteen days; 89
  commits exist only to regenerate it. Keeping it right costs `crates/tasks/src/land/regenerate.rs`
  (1,674 lines) and its tests (1,962), the three kept builds and their check, the rule that a
  branch never commits it, and the land that keeps main's side of it.
- **Cost.** Every clone, every worktree's fetch and every consuming project's pin carries a
  history that grows about 7 MiB a day; a commit of main is usable as a pin only once the
  regenerator has caught up with it.
- **Smallest change.** A choice for the owner, as it changes how a pin is taken. Once the Dioxus
  client is gone the client is one Vite build of seconds: main could commit no distribution, a
  checkout building its own as a source kit's preview already does, and only a release commit the
  one it verified, so a pin is a release. That removes the regenerator from every land and most of
  the history's growth. The lighter option keeps today's design and regenerates on the release
  cadence alone.
- **Size.** A project, after the choice.

### 3. This repository's own release machinery is compiled into the library every project links

- **Evidence.** `crates/symphony/src/release.rs` and `src/release/` are 21,767 lines (11,400 of
  source, the rest tests beside it), with 6,881 more in `tests/release/`. With `split_repair`
  (1,400), `release_inputs`, `size_budget`, `crates_check` and `cli/publish.rs` it is about 14,100
  lines of source that only the `symphony-build` binary runs. The package has no features and
  publishes `src/**` (`crates/symphony/Cargo.toml:21`), so the server, a consuming host and the
  crates.io package compile all of it, and `crates/symphony/tests/links.rs` exists to prove the
  linker then drops it from the shipped binary. The one outside caller is `reports/git.rs:317`, of
  a five-line function. The board repeats two of its constants
  (`crates/tasks/src/sweep/holder.rs:17`), held equal by a test.
- **Cost.** Compile time in every worktree and every consumer; 239 release tests inside the
  library's own unit-test binary; a public surface a published library has to keep.
- **Smallest change.** Move those modules to a workspace package of their own, unpublished, that
  owns `symphony-build`. Everything they use of the library is already public. It removes the
  linker test, the manifest's exclusions and the repeated constants.
- **Size.** A task: a move, no behaviour changes.

### 4. Nine tenths of the board's dependencies serve a search mode nothing asks for

- **Evidence.** `cargo tree -p symphony-board -e normal,build` lists 189 packages; 172 of them are
  in the trees of `tract-onnx`, `tract-linalg` and `tokenizers`, the local embedding model behind
  `tasks search --mode vector` and `hybrid`. The default mode is keyword
  (`crates/tasks/src/cli/args.rs:197`), and no skill, guide or page names another. `tasks` is the
  largest shipped binary, 20.9 MB against the server's 16.2 MB (`tools/size-budget.toml`). The
  server runs the embedding catch-up for every project with a board
  (`apps/symphony-server/src/front/embeddings.rs`). The code itself is 1,474 lines.
- **Cost.** The board's cold build and its binary's size, a model download, and a background
  inference pass per project, for results no agent is told to read.
- **Smallest change.** Put the model and `tasks embed` behind a Cargo feature that is off by
  default, as [plugin delivery](plugin-delivery.md) already proposes, and have the server skip the
  catch-up for a build without it. Whether search by meaning is kept at all is the owner's choice;
  the feature makes either answer cheap.
- **Size.** A task.

### 5. The board's command line imitates a Python program that is gone

- **Evidence.** `crates/tasks/src/pyjson.rs` writes JSON as Python's `json.dumps` did, with
  Python's float and string `repr`; `Py::` is on 260 source lines in ten files, and
  `crates/symphony/src/taskview.rs:494` prints one to text to parse it straight back with
  `serde_json`. `cli/args.rs` reads arguments as argparse did, and `cli/usage.rs` (1,166 lines) is
  argparse's help written by hand: each command is declared in `args.rs:145`, summarised in
  `usage.rs:60` and written out in `usage.rs:308`, its usage block twice, 87 blocks in all, with
  the status list typed out twelve times. The code reads its flags back by name through 73 calls,
  and `Parsed::get` (`args.rs:498`) answers a misspelt name with nothing. No Python file is
  tracked; `schema.rs:1` still cites one.
- **Cost.** A new flag or status edits up to twelve strings by hand; a typo in a flag's name is
  silent; about 400 lines exist for byte fidelity with output no program compares any more.
- **Smallest change.** First print `serde_json` values (the crate already keeps key order) and
  drop `pyjson.rs` and `cli/shape.rs`: a task. Then generate each help text from the one table in
  `args.rs` rather than keep its hand-written twin, which needs no new dependency: a task. A move
  to a parser crate is a project and not needed for either saving.
- **Size.** Two tasks.

### 6. The server's live hub does one project's work on one thread, under two wide locks

Three faults in `apps/symphony-server/src/live.rs`, each small to fix:

- **A document once opened is followed until the server stops.** Reading a document inserts it in
  its page's `documents` (`live.rs:621`); nothing removes one. Each refresh of main's page renders
  every followed document again (`live.rs:974-990`) under the `rendering` lock a document read
  also takes (`live.rs:606`), and a refresh runs on every watcher event, a ref written in the
  common dir among them. Refresh time grows with every document opened since the server started.
  Follow a document only while a feed names it, and drop the rest at the start of a refresh.
- **The board's clock waits behind document rendering.** One thread per project runs the
  one-second board tick and then the document refresh (`live.rs:1638-1690`), so callback dispatch
  and task giving wait for a render; and `update_board` holds the `tasks` lock from `live.rs:1173`
  to `1289` across three board reads, one of them a thread read per open task, while every board
  page's read waits. Give the tick its own thread, and take the lock after the reads.
- **A board search opens a new board connection per request** (`api.rs:399`,
  `symphony::taskview::connect`), which finds the board's server with `ps` and `lsof` and reads its
  schema, where the hub's shared reader exists to avoid exactly that (`live.rs:424-439`). The
  React board sends several reads per query. Use the shared reader.

- **Cost.** The shared server slows as its uptime and the machine's load grow; this is where a
  slow page comes from.
- **Size.** Small, small, and a task for the documents.

### 7. The React client has two copies of the task feed and three ways for a model to reach a component

- **Evidence.** The board and agents pages each read and apply the task regions of the feed:
  `boardlive.tsx:174-209` against `agents-live.ts:416-451`, `boardlive.tsx:860-906` against
  `agents-live.ts:705-753`, about 75 lines alike but for a name, each with its own tests. The
  regions are untyped on the wire: the server writes them as `json!` with a region string
  (`live.rs:1198`), and `wire.ts:273` types only the home page's five. A model reaches React
  through `useState` (`live.tsx:276`), through a mutable class and a `redraw()` called at thirteen
  sites (`boardlive.tsx:917`, `agents.tsx:40`), or through `useSyncExternalStore`
  (`document-live.ts:196`). The feed loop is written twice (`live.tsx:144`,
  `document-live.ts:584`), and the document's copy assumes the server's ping interval in a
  constant (`document-live.ts:574` against `crates/symphony-api/src/lib.rs:143`).
- **Cost.** A change to a task region is made and tested twice, and the task page, the next kind
  to move, makes it three times. A forgotten `redraw()` leaves a page stale where no unit test
  sees it.
- **Smallest change.** Before the task page lands: one `readTaskRegion` and `applyTaskRegion` in
  `task.ts`, every live model on the one `Model` and a single `useLive` hook, and the stream's
  inner loop in `feed.ts` with the ping interval exported through the wire. It removes about 150
  lines and two host interfaces, and gives the next page one pattern to follow.
- **Size.** A task, and typing the regions in `crates/symphony-api` a second.

### 8. Seventeen private git runners disagree on the caller's repository

- **Evidence.** Git is started through eight private helpers in the board (`land.rs:1251` and
  `1538`, `launch/mod.rs:658`, `cli/probes.rs:390`, `cli/commands.rs:89` and three more) beside the
  shared one (`sweep/git/command.rs:8`), and nine in the library. They drop different inherited
  variables: eight names in `sweep/git/command.rs:17-26`, the same eight typed again in
  `land.rs:170-179`, four in `reload.rs`, another four in `reports/git.rs`, none in
  `branches.rs:178`, `landing.rs:14` or the launch and probe helpers. `git worktree list` is parsed
  in six places across the three packages.
- **Cost.** Under a git hook, which sets `GIT_DIR` and `GIT_INDEX_FILE`, the branch scan and the
  landing checks read the caller's repository while the sweep and the release run read their own
  folder's. The shared helper's own comment names this hazard.
- **Smallest change.** One builder in `symphony_board::sys` that every helper starts from, with
  the eight names; `commit.rs`, which honours them on purpose, says so. One worktree-list reader.
- **Size.** Small for the builder, a task with the readers.

### 9. Status, verdicts and board errors are strings, read by text

- **Evidence.** `symphony_api::TaskStatus` exists (`task_queries.rs:15`) and the board uses it
  nowhere: `status: String` is in nine structs, a quoted status in 243 source lines of 45 files.
  The finished pair is `FINISHED` in `health.rs:17`, `CLOSED` in `stages.rs:67`, inline in seven
  more places and again in the server. A verdict is the start of a message's text, read by four
  parsers: `stages.rs:224` and `reports.rs:173` are the same function, `alarm.rs:172` wants a
  colon after the word, `store/work.rs:182` is a fourth. A board error reaches the server as a
  `String` (68 such signatures there) and is classified by substring: slow or not decides 504 or
  503 (`crates/tasks/src/rpc/read.rs:103`).
- **Cost.** A reworded board message changes an HTTP status; the four verdict readers can
  disagree on the same message; a new status has no compiler to find its uses.
- **Smallest change.** One `Verdict::of(text)` and one finished constant: small. Keep the board's
  `Error` to the server's edge with `is_slow()` on it: a task. The status enum through the store:
  a task.
- **Size.** Small, then two tasks.

### 10. The Dioxus client cannot be deleted when its last page moves

- **Evidence.** `apps/symphony-ui` still owns the shell every page boots from (`assets/shell.js`,
  624 lines, and `cache.js`), the shared stylesheet (`package.sh:150` builds it as what the Dioxus
  kinds have in common), the packaging the Vite build runs through, a stylesheet the React client
  imports by path (`media-layer.ts:11`) and the 46 browser test files that are the React client's
  only integration tests. The page kinds are listed in six places and the routes in a JavaScript
  and a TypeScript copy. Going with it: the server's `development.rs` (696 lines), `split_repair`
  (1,400 lines, a workaround for the Dioxus splitter), the `wasm` routes and the
  `skip-board-fields` feature. Dead already: no client reads a section or an ask without
  `format=blocks`, so `docview/legacy.rs` and its arms in `api.rs:714-825` answer nobody here.
- **Cost.** The removal is planned as a deletion and is a move; found late, it holds 52,000 lines
  in the tree after their last user is gone.
- **Smallest change.** Move the shell, the cache script, the shared styles, the packaging and the
  browser tests to the web package before the last kind moves, with the shared stylesheet a fixed
  list; then the deletion is a deletion. One `PAGES` constant in the wire types replaces the kind
  lists that remain.
- **Size.** A project, best started while the panel and overlay move.

## Further findings

- **`symphony shot` aims clicks through features no page here has.** About half of
  `crates/symphony/src/cli/commands/shot.rs` (2,348 lines, with 6,353 of tests) reaches into closed
  shadow roots, slots and clip-path shapes; no client source attaches a shadow root. Aim at the
  middle of the element's box, check it with `elementFromPoint` and fail naming what is on top. A
  task.
- **`init.rs` carries paths nothing reaches.** `Shipped.recipes` is an `Option` that
  `builtin()` always fills (`init.rs:228`, `273`), so `retire_recipes` runs only in tests;
  `RETIRED_ASK` and the justfile-setting migration undo what an init wrote for about a day. Ten of
  its 77 tests hold only those states. Small, once each project has run init once. The skill list
  kept by hand (`skill!`, `init.rs:253`) is not the weight: embedding the folder whole would remove
  those twenty lines and add a dependency, and a test that lists `shipped/skills/` against it holds
  the one risk. A change of a skill's words needs a compile because the installed copy in
  `.agents/skills/` carries a digest the binary computes; the evidence for such a change could be the
  hooks test alone.
- **The plugin loopback measures what is now real.** `plugins/measure.rs` (1,011 lines) sends
  each call through a JSON round trip inside the process to estimate a protocol's cost; the process
  protocol exists and is measured itself. Each seam is written in seven places; dropping the
  loopback removes one. A task.
- **Nothing ties the schema's text to its version.** A change to `DEFINE`
  (`crates/tasks/src/schema.rs:276`) with no bump of `VERSION` (`schema.rs:81`) never runs on an
  existing board, and a changed field keeps its old definition unless written `OVERWRITE`. A test
  that pins a digest of the text per version catches both. Small.
- **The landing drops errors that explain a refusal.** `land.rs:1777` drops a failed post, so a
  refusal can be missing from the thread; `stamps.rs:367` reads a failed board read as no subtasks,
  which hides their red runs from the tests stamp; `land.rs:2620` drops a record write a resumed
  land then refuses on. A line on stderr at each. Small.
- **`symphony.toml` has two readers.** The board reads its keys by hand from a `toml::Table`
  (`crates/tasks/src/project.rs:131-262` and four more files) and the library mirrors the board's
  tables to refuse a misspelt key; where the file does not parse, one answers nothing and the other
  names the fault. One serde struct the board exports and the library embeds. A task.
- **Reads are dispatched through six lists of paths** in `apps/symphony-server/src/api.rs:28-606`;
  writes have one. One table of a path and what it needs. A task.
- **`Project::of` runs on async workers** (`front.rs:418` on every request, `api.rs:109`), and
  can wait ten seconds for a plugin process to declare itself. Resolve it once per request off the
  runtime. Small.
- **Two tests pass without running** unless a variable is set
  (`crates/symphony/tests/after_commit.rs:2153`, `tests/edit/message.rs:388`), and a handful assert
  that nothing happened after a fixed sleep (`crates/tasks/tests/sweep/land_signals.rs:206`,
  `crates/symphony/tests/release/regenerated.rs:186`). Each of the 82 release tests builds an
  origin, a checkout and a worktree.
- **Comments record measurements and incidents** with their dates and loads
  (`schema.rs:18-22`, `sweep/trigger.rs:94`), which belong in `docs/history/`.

## The architecture risks the brief named

- **The schema rule** is sound: one cumulative script, a guard on every write, a live board moved
  only by the default branch's schema. Its gap is the missing tie above.
- **One shared server** is the right shape for one machine; its risk is finding 6, where one
  project's slow render delays its own board, and uptime is a cost.
- **State in the git common dir** is named once (`crates/tasks/src/folders.rs`) and no other
  package repeats the names.
- **The live feed** is sound: nothing is queued per subscriber, a subscriber 128 events behind is
  told to read again, a dead socket shows at the next ping. Its weakness is the untyped regions of
  finding 7.
- **The client's module split** leaves with the Dioxus client; the React client is one Vite entry
  per kind with React as its only runtime dependency.

## Read and sound

So a later review does not read them again:

- The board's queries bind every value; the nine statements built with `format!` interpolate only
  names the code chooses.
- `cbor.rs`, the hand-written board client, `crates/chrome-devtools` and `crates/process-cwd` each
  do a job no maintained crate does as they need it (the leash, CPU-bounded waits, a spawn without
  fork), and say why.
- The React client has one request helper, no `any`, no suppressed effect dependencies, no
  snapshot tests, and a hand-drawn chart that is the simpler choice for what it shows.
- The documents do not repeat each other: no pair shares more than 4 % of its eight-word runs.
  Their fault is length and structure, finding 1.
- The small crates each have a user outside the workspace or a reason to publish alone; none is
  worth folding while a consumer calls it.
- The server's tests wait on conditions, not on the clock, with few exceptions.

## Choices that are the owner's

Three findings need a decision before a task: whether main commits the built client or only a
release does (finding 2), whether search by meaning is kept (finding 4), and whether the server's
restart keeps handing its sockets over (1,600 lines of source and 3,800 of tests, where the clients
already reconnect and a plain restart would do).
