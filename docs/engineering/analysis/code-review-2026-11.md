# Code and architecture review, November 2026

A reading of the workspace's structure, its architecture risks, its code smells and its developer
documentation, as ranked findings a maintainer can act on. Each finding names its evidence, what it
costs today, the smallest change that would fix it and a size: small (an hour or two), a task, or a
project (several tasks). Nothing here is built; each finding is a task to file or a choice to put
in [DECISIONS](../../DECISIONS.md).

Read at main's `3780edd9` on 2026-11-15, with no build and no test run: the numbers are line
counts, `cargo tree`, `git rev-list`, `git cat-file` and grep, and a number a choice rests on is
given with its command. The Dioxus client (`apps/symphony-ui`) has handed every page to the React
one (`apps/symphony-web`) and keeps the chat panel and the menus, so it is read only for what its
removal needs.

## The numbers

| Package | Source lines | Test lines |
|---|---|---|
| `crates/tasks` (the board) | 85,100 | 112,500 |
| `crates/symphony` (the library) | 60,900 | 65,400 |
| `apps/symphony-server` | 21,000 | 32,500 |
| `apps/symphony-ui` (Dioxus, leaving) | 7,900 | 34,500 |
| `apps/symphony-web` (React) | 24,700 | 24,500 |
| the eleven smaller crates | 21,500 | 12,200 |

Rust is 454,000 lines in all, with 6,249 tests. Comment lines are 13 % of the board's source lines
and 11 % of the library's. The developer guides and documents are 163,900 words. The repository
holds 1,560 commits that are not merges, from 2026-11-02 on.

The package boundaries hold. The dependency graph has no cycle and one direction: the server on
the library and the board, the library on the board, both on the wire types, the plugin kit on the
blocks and the document reader alone. The server holds no SurrealQL and no table name. What crosses
a boundary by the back door is named in findings 3, 8 and 9.

## The top ten, ranked by what they would save

### 1. The board's guide is 1,628 lines under one heading, and each change to the board edits two documents

- **Evidence.** `crates/tasks/AGENTS.md` is 20,600 words with one heading. `docs/workflow/TASKS.md` is
  39,400 words. They share almost no text (about one in a hundred of the guide's eight-word runs is
  in TASKS), so neither repeats the other: both are the board's reference, split by no rule a
  reader can tell. Of the 1,560 commits, 233 change `docs/workflow/TASKS.md`, 209 the guide and 129 both
  (`git rev-list --count --no-merges --full-history main -- PATH`); they are the two most changed
  files in the repository, where the most changed source file has 80. The root `AGENTS.md` (5,800
  words, in every session's context) has 118.
- **Cost.** An agent told to read the guide before it edits the board reads 20,600 words it
  cannot skim, for a change that needs a tenth of them; so does its reviewer. Two branches that
  each add a paragraph to the same unbroken text conflict at the land.
- **Smallest change.** Make the guide what an editor needs before touching the crate (where each
  job lives, the invariants a change must keep, how to test against a scratch board), a few hundred
  lines under headings. What it says beyond that is cut where `docs/workflow/TASKS.md` or the code already
  says it, and moved to the section of TASKS that owns it only where nothing else does: the saving
  is the cut, as TASKS is long already. The root guide keeps what every session needs and links
  the rest: how `tools/rs-bin.sh` decides freshness, what the memory guard misses and what `init`
  retires are reference, read by the few tasks that touch them.
- **Size.** A task for the board's guide, a task for the root guide.

### 2. The built client is committed, and is 85 % of the repository

- **Evidence.** One walk of everything main reaches, each blob counted once at its size on disk
  and by the path it is first met under:

  ```
  git rev-list --objects main \
    | git cat-file --batch-check='%(objecttype) %(objectsize:disk) %(rest)' \
    | awk '$1=="blob"{n++; d+=$2; if(index($3,"apps/symphony-server/client/public/")==1){pn++; pd+=$2}}
           END{print n, d/1048576, pn, pd/1048576}'
  ```

  It gives 20,402 blobs taking 156.8 MiB, of which 5,895 blobs and 133.3 MiB are under
  `apps/symphony-server/client/public/`: 85 %. The folder's first commit is of 2026-11-10
  (`git log --full-history --reverse --format=%ad --date=short main -- apps/symphony-server/client/public`),
  so that is five days' growth, about 27 MiB a day. In the tree it is 7.0 MB in 109 files. Of
  main's commits, 91 only regenerate it
  (`git log --oneline main --grep='Regenerate the client distribution'`). Keeping it right costs
  `crates/tasks/src/land/regenerate.rs` (1,674 lines) and its tests
  (`crates/tasks/tests/sweep/land_regenerate.rs`, 1,962), the three kept builds and their check,
  the rule that a branch never commits it, and the land that keeps main's side of it.
- **Cost.** Every clone, every worktree's fetch and every consuming project's pin carries that
  history, which at this rate nears a gigabyte within a month; a commit of main is usable as a pin
  only once the regenerator has caught up with it.
- **Smallest change.** A choice for the owner, as it changes how a pin is taken. Once the Dioxus
  client is gone the client is one Vite build of seconds: main could commit no distribution, a
  checkout building its own as a source kit's preview already does, and only a release commit the
  one it verified, so a pin is a release. That removes the regenerator from every land and nearly
  all of the growth. The lighter option keeps today's design and regenerates on the release
  cadence alone, one commit of it in twenty landings. Neither shrinks the history there is: that
  takes a rewrite, a separate choice.
- **Size.** A project, after the choice.

### 3. This repository's own release machinery is compiled into the library every project links

- **Evidence.** `crates/symphony/src/release.rs` and `src/release/` are 21,767 lines (about 11,400
  of source, the rest tests beside it), with 6,881 more in `tests/release/`. With `split_repair`,
  `release_inputs`, `size_budget`, `crates_check` and `cli/publish.rs` it is about 14,100 lines of
  source that only the `symphony-build` binary runs. The package has no features and publishes
  `src/**` (`crates/symphony/Cargo.toml:21`), so the server, a consuming host and the crates.io
  package compile all of it, and one of the four tests of `crates/symphony/tests/links.rs`
  (`the_binary_links_none_of_the_build_only_commands`) exists to prove the linker then drops it
  from the shipped binary. The board repeats two of its constants
  (`crates/tasks/src/sweep/holder.rs:17`), held equal by a test.
- **Cost.** Compile time in every worktree and every consumer; the release's unit tests inside
  the library's own unit-test binary; a public surface a published library has to keep.
- **Smallest change.** Move those modules to a workspace package of their own, unpublished, that
  owns `symphony-build`. Nearly all they use of the library is public already. What is not goes
  with them or is opened: `init::BUILDS` (`init.rs:1065`, read by the size budget), the
  `build_main` dispatch and its help (`cli/commands.rs:177-260`), `cadence::landed`
  (`release/cadence.rs:25`, which `reports/git.rs:317` calls and so stays in the library) and the
  test-only `test_tree` the release tests build on. It removes that one linker test, the manifest's
  exclusions and the repeated constants.
- **Size.** A task: a move, no behaviour changes.

### 4. Eight tenths of the board's dependencies serve a search mode nothing calls

- **Evidence.** `cargo tree -p symphony-board -e normal,build --prefix none --no-dedupe | sort -u`
  lists 189 packages; with `--prune tract-onnx --prune tract-linalg --prune tokenizers` before the
  sort it lists 36. So 153 of the 189 are there only for the local embedding model behind
  `tasks search --mode vector` and `hybrid`. The default mode is keyword
  (`crates/tasks/src/cli/args.rs:197`), and no skill or page names another; the board's guide
  describes them. The size budgets (`tools/size-budget.toml`, each a size plus 4 %) hold `tasks`
  to 20.9 MB, more than the server's own code at 16.2 MB, which with the client it embeds is the
  largest binary. The server runs the embedding catch-up for every project with a board
  (`apps/symphony-server/src/front/embeddings.rs`). The code itself is about 1,500 lines.
- **Cost.** The board's cold build and its binary's size, a model download, and a background
  inference pass per project, for results no agent is told to read.
- **Smallest change.** The owner's choice first, as the two answers differ. If search by meaning
  goes, delete it: the two modes, `tasks embed`, the server's catch-up, the model's tables and the
  three dependencies, which is smaller than any switch. If it stays, put it behind a Cargo feature
  that is off by default, as [plugin delivery](plugin-delivery.md) proposes; that keeps a second
  configuration whose tests still have to run somewhere.
- **Size.** A task either way.

### 5. The board's command line imitates a Python program that is gone

- **Evidence.** `crates/tasks/src/pyjson.rs` (298 lines) writes JSON as Python's `json.dumps`
  did, with Python's float and string `repr`; `Py::` is on 245 source lines in ten other files, and
  `crates/symphony/src/taskview.rs:494` prints one to text to parse it straight back with
  `serde_json`. `cli/args.rs` reads arguments as argparse did, and `cli/usage.rs` (1,166 lines) is
  argparse's help written by hand: each command is declared in `args.rs:145`, summarised in
  `usage.rs:60` and written out in `usage.rs:308`, its usage block twice, 87 blocks in all, with
  the status list typed out twelve times. The code reads its flags back by name, 127 times, and
  `Parsed::get` (`args.rs:498`) answers a misspelt name with nothing. No Python file is tracked;
  `schema.rs:1` still cites one.
- **Cost.** A new flag or status edits up to twelve strings by hand; a typo in a flag's name is
  silent; `pyjson.rs` exists for byte fidelity with output no program compares any more.
- **Smallest change.** First print `serde_json` values (the crate already keeps key order) and
  drop `pyjson.rs`. `cli/shape.rs` (212 lines) goes only as far as its rows get `Serialize` impls
  written for them, since it computes fields such as a task's age; that part is a rewrite, not a
  deletion. A task. Then generate each help text from the one table in `args.rs` rather than keep
  its hand-written twin, which needs no new dependency: a task. A move to a parser crate is a
  project and not needed for either saving.
- **Size.** Two tasks.

### 6. The server's live hub does one project's work on one thread, under two wide locks

Three faults in `apps/symphony-server/src/live.rs`, read from the code and not measured: how much
of a slow page each explains is not known until one of the proofs below is run.

- **A document once opened is followed until the server stops.** Reading a document inserts it in
  its page's `documents` (`live.rs:621`); nothing removes one, so the set grows to at most the
  project's documents. Each refresh of main's page renders every followed document again
  (`live.rs:970-990`) under the `rendering` lock a document read also takes (`live.rs:606`), and a
  refresh runs on every watcher event, a ref written in the common dir among them. Follow a
  document only while a feed names it, which the feed's address already does and so needs a count
  of feeds per document and no new protocol, and drop the rest at the start of a refresh.
  *Proof:* a hub test that reads two documents with no feed open, changes one file, refreshes for
  a ref's path and finds both still followed; or a server of one's own with `SYMPHONY_PROFILE=1`,
  whose "document event" line grows as documents are opened.
- **The board's clock waits behind document rendering.** One thread per project runs the
  one-second board tick and then the document refresh (`live.rs:1638-1690`), so callback dispatch
  and task giving wait for a render; and `update_board` holds the `tasks` lock from `live.rs:1173`
  to `1289` across three kinds of board read while every board page's read waits. Give the tick
  its own thread, and take the lock after the reads. *Proof:* under the same profile, the "board
  tick" lines come apart by the time a render takes.
- **A board search opens a new board connection per request** (`api.rs:399`,
  `symphony::taskview::connect`, which may even start a board), finding the board's server with
  `ps` and `lsof` and reading its schema, where the hub's shared reader exists to avoid exactly
  that (`live.rs:428`). Use the shared reader. *Proof:* count the board server's connections, or
  the `ps` and `lsof` children, per search request against a server of one's own.

- **Cost.** A likely source of pages that slow as the shared server's uptime and the machine's
  load grow.
- **Size.** Small, small, and a task for the documents.

### 7. The React client reads and applies the task feed in three places

- **Evidence.** The task board, the agents page and the task page each switch over the same task
  regions of the feed, once to read them and once to apply them: `boardlive.tsx:174` and `840`,
  `agents-live.ts:413` and `696`, `task-live.ts:247` and `631`. Compared line for line, a pair of
  them has some 50 to 60 lines alike. What reads one item is shared already, in `task.ts`
  (`readTaskChange`, `readItemChange`, `merge`, `holderHealth`); what repeats is the switch around
  them. The regions are untyped on the wire: the server writes them as `json!` with a region
  string (`live.rs:1198`), and `wire.ts:278` types only the home page's five. Beside it, the feed's
  inner loop is written twice (`live.tsx:144`, `document-live.ts:584`), and the document's copy
  assumes the server's ping interval in a constant (`document-live.ts:574` against
  `crates/symphony-api/src/lib.rs:138`).
- **Cost.** A change to how a task region is read or applied is made and tested three times.
- **Smallest change.** One reader of a task region in `task.ts` that the three switches call,
  each page keeping its own regions and its own way of applying them (the task page holds one task
  and its children, the others a list). The ping interval exported through the wire. Each page's
  model reaching React differently (a redraw callback on three, an external store on the
  documents) is a difference of style this does not need to settle.
- **Size.** A task, and typing the regions in `crates/symphony-api` a second.

### 8. Fifteen places start git with the caller's repository still in the environment

- **Evidence.** Outside tests git is started at some twenty-five places in the board, the library
  and the server. The board's shared helper drops the eight variables by which a caller names its
  own repository and index (`crates/tasks/src/sweep/git/command.rs:13-29`), and its comment says
  why; the land goes through it or drops the same eight (`land.rs:170`, `land/moving.rs:405`,
  `land/regenerate.rs:1222`). Eight in the board drop none: `git.rs:120`, `live.rs:558`,
  `runs.rs:332`, `launch/mod.rs:704` and `717`, `cli/commands.rs:90`, `cli/probes.rs:391`,
  `store/codex_delivery.rs:112`. Seven in the library drop none: `branches.rs:180` and `269`,
  `landing.rs:15`, `36`, `99` and `156`, `asks.rs:627`. Three in the library and the server's one
  drop four of the eight (`reload.rs:65`, `reports/git.rs:129`, `release/git.rs:27`,
  `apps/symphony-server/src/repo.rs:78`). `commit.rs` keeps them on purpose and says so.
  `git worktree list --porcelain` is parsed in nine places: `launch/mod.rs:729`,
  `sweep/git/worktrees.rs:45`, `commit.rs:1195`, `branches.rs:201`, `reload.rs:82`,
  `release/place.rs:175`, `release/candidate.rs:1124`, `release/refresh.rs:1232` and the server's
  `repo.rs:118`.
- **Cost.** Under a git hook, which sets `GIT_DIR` and `GIT_INDEX_FILE`, the branch scan, the
  landing's card checks, a launch and the recorder's probes read the caller's repository where the
  sweep and the land read their own folder's.
- **Smallest change.** One builder in `symphony_board::sys` with the eight names that every start
  uses; `commit.rs` keeps its exception. One reader of the worktree list, which the board already
  has.
- **Size.** Small for the builder, a task with the readers.

### 9. Verdicts and board errors are text, read by matching it

- **Evidence.** A verdict is the start of a message's text, read by four parsers: `stages.rs:224`
  and `reports.rs:173` are the same function, `alarm.rs:172` wants a colon and a space after the
  word, `store/work.rs:182` is a fourth. The finished pair of statuses is `FINISHED` in
  `health.rs:17` and `CLOSED` in `stages.rs:67`, constants again in the library
  (`branches.rs:81`, `verdicts.rs:31`), and written inline in nine more places across the three
  packages. A board error reaches the server as a `String` (about fifty functions there return
  one) and is classified by substring: slow or not decides 504 or 503
  (`crates/tasks/src/rpc/read.rs:104`). A status is a `String` in sixteen of the board's structs
  though `symphony_api::TaskStatus` exists (`task_queries.rs:15`).
- **Cost.** A reworded board message changes an HTTP status; the four verdict readers can
  disagree on the same message.
- **Smallest change.** One `Verdict::of(text)` and one finished constant: small. Keep the board's
  `Error` to the server's edge with `is_slow()` on it: a task. The status enum through the store
  is left out: it touches some sixty files for a set that rarely changes.
- **Size.** Small, then a task.

### 10. The Dioxus client cannot be deleted when the panel and the menus move

- **Evidence.** `apps/symphony-ui` is now the chat panel and the menus alone (`package.sh:37`),
  7,900 lines of source, yet the React client and the server still take from it:
  - the shell every page boots from and the cache script (`assets/shell.js`, 704 lines, and
    `cache.js`), which `package.sh:30` minifies with the esbuild that dx ships, so they need
    another minifier, Vite's;
  - the shared stylesheet, which `package.sh:158` derives from what the program's page links and
    which becomes a fixed list (the frame's, the components' theme and the board's sheets);
  - two stylesheets the React client imports by path (`media-layer.ts:11`, `attach.tsx:15`);
  - the packaging the Vite build runs through (`bundle.sh`, `package.sh`,
    `bin/cargo-symphony-ui`) and its tests (`tests/bundle_concurrent.rs`, `tests/client_release`);
  - the web dev server's paths to the shell and the cache script (`dev-server.ts:18-23`);
  - the test that holds the shell's routes equal to the wire types' (`src/project.rs:399`);
  - the regenerator's key, which reaches the shell and those styles only through the Dioxus
    package's release inputs (`tools/client-key.sh`);
  - the 42 browser test files, 28,800 lines, that are the React client's only integration tests.

  Going with it: the server's `development.rs` (696 lines), `split_repair` (a workaround for the
  Dioxus splitter) and the `wasm` and `_dioxus` routes (`crates/symphony-api/src/project.rs:182`).
  Dead already: no client reads a section or an ask without `format=blocks`, so
  `docview/legacy.rs` and its arms in `api.rs:715-822` answer nobody here.
- **Cost.** The removal is planned as a deletion and is a move; found late, it holds 42,000 lines
  in the tree after their last user is gone.
- **Smallest change.** Move each item of that list to the web package while the panel and the
  menus move, the key and the shell's minifier among them; then the deletion is a deletion. One
  `PAGES` constant in the wire types replaces the lists of page kinds that remain.
- **Size.** A project.

## Further findings

- **`symphony shot` aims clicks through features no page here has.** About half of
  `crates/symphony/src/cli/commands/shot.rs` (2,348 lines, with 6,353 of tests) reaches into closed
  shadow roots, slots and clip-path shapes; no client source attaches a shadow root. Aim at the
  middle of the element's box, check it with `elementFromPoint` and fail naming what is on top. A
  task.
- **`init.rs` carries paths nothing reaches.** `Shipped.recipes` is an `Option` that
  `builtin()` always fills (`init.rs:233`, `273`), so `retire_recipes` (`init.rs:737`) runs only
  in tests; `RETIRED_ASK` and the justfile-setting migration undo what an init wrote for about a
  day. Ten of its 77 tests hold only those states. Small, once each project has run init once.
  The skill list kept by hand (`skill!`, `init.rs:253`) is not the weight: embedding the folder
  whole would remove those twenty lines and add a dependency. Its one risk, a new skill folder
  that is not shipped, has no test today; one that lists `shipped/skills/` against the list is
  proposed here. A change of a skill's words needs a compile because the installed copy in
  `.agents/skills/` carries a digest the binary computes.
- **The plugin loopback measures what is now real.** `plugins/measure.rs` (1,011 lines) sends
  each call through a JSON round trip inside the process to estimate a protocol's cost; the process
  protocol exists and is measured itself. Each seam is written in seven places; dropping the
  loopback removes one. A task.
- **Nothing ties the schema's text to its version.** A change to `DEFINE`
  (`crates/tasks/src/schema.rs:276`) with no bump of `VERSION` (`schema.rs:81`) never runs on an
  existing board. A test that pins a digest of the text per version catches that. It does not catch
  a changed field written without `OVERWRITE`, which keeps its old definition: that stays a
  reviewer's to see. Small.
- **The landing drops errors that explain a refusal.** `land.rs:1777` drops a failed post, so a
  refusal can be missing from the thread; `stamps.rs:367` reads a failed board read as no subtasks,
  which hides their red runs from the tests stamp; `land.rs:2620` drops a record write a resumed
  land then refuses on. A line on stderr at each. Small.
- **`symphony.toml` has two readers.** The board reads its keys by hand from a `toml::Table`
  (`crates/tasks/src/project.rs:131` on, and four more files) and the library mirrors the board's
  tables to refuse a misspelt key; where the file does not parse, one answers nothing and the other
  names the fault. One serde struct the board exports and the library embeds. A task.
- **Reads are dispatched through six lists of paths** in `apps/symphony-server/src/api.rs`, from
  `READS` at line 28 on; writes have one. One table of a path and what it needs. A task.
- **`Project::of` runs on async workers** (`front.rs:418`, on every request), and can wait ten
  seconds for a plugin process to declare itself. Resolve it once per request off the runtime.
  Small.
- **A few tests assert that nothing happened after a fixed sleep**
  (`crates/tasks/tests/sweep/land_signals.rs:206`,
  `crates/symphony/tests/release/regenerated.rs:186`), which a slow machine passes either way, and
  each of the release tests builds an origin, a checkout and a worktree.
- **Comments record measurements and incidents** with their dates and loads
  (`schema.rs:18-22`, `sweep/trigger.rs:94-96`), which belong in `docs/history/`.

## The architecture risks the brief named

- **The schema rule** is sound: one cumulative script, a guard on every write, a live board moved
  only by the default branch's schema. Its gap is the missing tie above.
- **One shared server** is the right shape for one machine; its risk is finding 6, where one
  project's render delays its own board.
- **State in the git common dir** is named once (`crates/tasks/src/folders.rs`) and no other
  package repeats the names.
- **The live feed** is sound: nothing is queued per subscriber, a subscriber 128 events behind is
  told to read again, a dead socket shows at the next ping. Its weakness is the untyped regions of
  finding 7.
- **The client's module split** leaves with the Dioxus client; the React client is one Vite entry
  per kind with React as its only runtime dependency.

## Read and sound

So a later review does not read them again:

- The board's queries bind every value; the statements built with `format!` interpolate only
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
restart keeps handing its sockets over (about 530 lines of source and 549 of tests, of the update's
2,046 and 2,920, where the clients already reconnect and a plain restart would do).
