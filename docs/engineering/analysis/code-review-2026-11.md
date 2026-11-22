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

- [-] **client-distribution-on-main** — Each land that changes the web client is followed by a background run that commits a fresh build of the client to main. Those builds are about 85 % of what git holds for main's history and grow about 27 MiB a day, which every clone downloads. Nothing on the pages changes under any option. How often should the built client be committed? A: only by a release, the one it verified, on a commit of its own under its tag that main never holds; a checkout that needs pages builds its own. B: once a release: the release run starts the run that commits it to main, and no land does. C: after every land that changes it, as today. None of them shrinks the history already there: that takes rewriting it, a choice of its own. Recommended: B. Approve and say A, B or C in the note, or comment. [[analysis/code-review-2026-11#2. The built client is committed, and is 85 % of the repository]]
  [2026-11-15 03:46] agent — Posted for the board task `docs/code-review-owner-choices`, from the codebase review.

    **Today**, read at main `3f3d9186` with the review's command (in its finding): git holds 156.2 MiB for main's history, 134.0 MiB of it the client under `apps/symphony-server/client/public`, first committed on 2026-11-10. 95 of main's commits regenerate it, 12 to 39 a day, and all but `e3314c18` (which also restates figures in two documents) do nothing else; in the tree it is 110 files, 6.3 MB. Each of those rounds is also main's only build of the client after a land, and between releases the only audit of the documents on main's tip: a round that fails files a red task within minutes, as one did on 2026-11-15.

    **What A and B both change:**
    - *A fresh clone* fetches every release tag, so under either it gets one client a release. Since the first release (2026-11-13), main gained 1,918 client files, 247 MiB before git compresses them; the five releases since hold 545 of them, 71 MiB: about three tenths, at one release every 20 landings.
    - *No round after a land.* A land that breaks the client's build, or the documents, is found by the next task that builds the client, or by the release run. While a release candidate is open, up to 24 hours, main's landings have only their own evidence. A build after each land that commits nothing would keep that check, at a build a land.

    **A, only a release:**
    - *Main holds no client.* A pin to a commit of main would build a server with no pages, so it is no longer possible. A checkout that serves pages (an agent's server of its own, for pictures) builds the client first, as a task already does for its tests; while the Dioxus client remains, that takes minutes.
    - *The release* builds the client in its own checkout, commits it on top of the tip it verified and tags that commit. `just client-check` then has no client on main to compare a fresh build with: the release's check becomes the build it commits, and the two older builds a page may still be open on (`KEEP` in `crates/symphony/src/cli/publish.rs`) come from the previous release's tag.
    - *What goes:* the regenerator, 1,674 lines (`crates/tasks/src/land/regenerate.rs`) and 1,962 of tests, with the rule that a branch never commits the client, the land keeping main's side of it and a pin's wait for it.
    - *What changes:* 32 tracked code, script and settings files name the folder (`git grep -l client/public`, less the documents, test fixtures and one experiment): 26 Rust files, among them the server's build script, its client and their tests, the size budget's client line, the release's verified client and the packaging test, and the server's manifest, the justfile, `symphony.toml`, the size budget, `tools/client-check.sh` and `tools/verify.sh`. The review sizes it as a project.

    **B, once a release.** The release run starts the regenerator's round on main's tip and waits for it before it verifies, as it waits for one today; no land starts one. The regenerator, its check and those 32 files stay as they are. Between releases main's client is older than its sources, so a pin to a commit of main waits for the next release, and main's tree keeps its 6.3 MB of client. A task.

    **C, as today.** About 27 MiB a day, some 0.8 GiB more in a month at this pace.

    **Why B.** A fresh clone gets the same bytes under A and B, so B takes nearly all of A's saving for a task rather than a project: the release run starts the round a land starts today, and the rest stays as it is. What A adds, a main with no client in it and the regenerator's removal, is cheaper once the Dioxus client is gone and the client is one quick build.

    **Earlier verdicts checked.** **symphony-update-command** (B, projects follow release tags) already makes every pin a release, which A and B rely on. **pin-waits-for-current-client** (A) keeps a pin to a commit of main off a client older than its sources; A leaves such a pin no client and B makes it wait for a release, so neither changes that choice. **release-main-while-candidate-open** (A) counts on "the regenerator's client check and docs audit each round" to check main's landings while a candidate is open; under A or B those landings would have only their own evidence for up to 24 hours, a consequence of either, not a change to that choice. **release-delivery** (C) ships a release's client beside the server once `symphony update` is built; it does not say whether main commits one before then. **pictures-in-every-clone** (F) commits the documents' pictures, not the client. None of them answers this card.
  [2026-11-15 06:46] owner · rejected — we should push those releases as crates to crates.io and not commit then at all if possible.
  [2026-11-15 11:13] agent — Replaced by **client-in-the-crate**. The server's package for crates.io already carries the built client, so what you ask is possible for a server taken from crates.io, and not yet for one built from a Git tag, as the consuming project builds it: nothing is published so far. The new card asks when the commits stop, with what each way costs and what has not been tried.

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

- [r] **board-search-by-meaning** — The board can find tasks by what they mean as well as by their words, with a small language model that runs on this machine: you asked for tasks to be searchable by meaning, and chose the model. Nothing calls it today: the board page searches by words, no skill or document names it but the board's own guide and help, and the consuming project's tools never call it. It costs 153 of the 189 packages the board is built from, a 133 MB download for each project, and a pass every minute on the server for each project, 13 hours of it on this repository's board since 2026-11-14. What becomes of it? A: delete it; the board searches by words alone. B: move the model into a program of its own, as an approved branch that never landed does: the board and the server build without it, and search by meaning works where that program is built. C: keep it in the board behind a build switch that is off by default. D: keep it as it is. Recommended: B. Approve and say A, B, C or D in the note, or comment. [[analysis/code-review-2026-11#4. Eight tenths of the board's dependencies serve a search mode nothing calls]]
  [2026-11-15 03:46] agent — Posted for the board task `docs/code-review-owner-choices`, from the codebase review.

    **Who uses it**, read today. `just tasks search` searches by words unless `--mode vector`, `hybrid` or `exact` is given (`crates/tasks/src/cli/search.rs`). The board page's search is by words (`apps/symphony-server/src/api.rs`, `store.search`). Of the shipped skills and the documents only the board's guide (`crates/tasks/AGENTS.md`) names those modes, and of the consuming project's tracked files only one analysis. Whether an agent has typed them is not known: the board records no search.

    **What it finds.** The consuming project's database card (2026-10-06) measured 0.82 of the top five results on target over ten real queries, against 0.76 for keywords. The later study with the model you chose, in that project's port plan, gave 0.82 for words and meaning together against 0.80 for words alone and 0.74 for meaning alone: one more useful result in fifty.

    **What it costs:**
    - *Packages:* `cargo tree -p symphony-board -e normal,build --prefix none --no-dedupe | sort -u` lists 189, and 36 with `--prune tract-onnx --prune tract-linalg --prune tokenizers` before the sort. Every build of the board, the server, `symphony` and `agent-send` compiles them.
    - *The model:* 133 MB in each project's `symphony/models/`, in its git common dir; two copies on this machine.
    - *The server's pass*, counted in its log (`grep -E 'task embedding catch-up( of [a-z-]+)? ended after'` on `server.log` in the machine folder) up to 07:30 UTC on 2026-11-15: 6,262 passes since 2026-11-11, 15.9 hours in all, 0.9 s at the median. 3,578 are from before the log named each pass's project (2.5 hours), 1,655 the consuming project's (0.4 hours) and 1,029 this repository's (13.0 hours). On 2026-11-14 this repository's passes ran 9.5 hours back to back: two stopped at the 4-hour limit ("failed: no answer within 14400 s", at 07:52 and 11:52 UTC) and the third embedded 1,792 chunks in 89 minutes. From 01:00 to 07:30 UTC on 2026-11-15, 172 of its 332 passes embedded something, each 2 s to 4.6 minutes, 43 s at the median: 2.9 of those 6.5 hours. Each pass loads the model again (7.8 s on average) and runs it on one thread at low priority (`nice -n 10 tasks embed --maintenance --threads 1`, `apps/symphony-server/src/front/embeddings.rs`); by its own count, 832 chunks took 8,756 s. Its CPU is not measured.
    - *Bytes:* the release run's build of `tasks` at hand is 20,337,280; without the model it is not measured today. On B's branch, at its thread's last measure (2026-11-12), `tasks` went from 24,293,376 to 8,484,176 bytes with the model's program at 16,641,552; its first measure took a cold build of `tasks` from 769 to 183 s of CPU.

    **A, delete it.** It removes what you asked for and chose a model for. The three modes, `tasks embed`, the server's pass and its hours, the download and the three packages go: about 1,150 lines of source (`crates/tasks/src/embedding.rs` and `cli/embedding.rs` less their tests, `store/embeddings.rs`, half of `cli/search.rs`, the server's `front/embeddings.rs`), about 800 of tests and two benchmarks (`examples/search_bench.rs`, `examples/embedding_cpu.rs`). The model's tables can stay in the board's schema, unused, so no board need move to a new schema for it. B's branch is dropped.

    **B, a program of its own.** `tasks-embed` holds the model, and `tasks embed` and a search by meaning start it. Where it is not built, those searches say what to build, and searching by words is unchanged. The branch (`perf/embedding-in-its-own-binary`, `c7142edd`, approved by its reviewer on 2026-11-12; no land of it was tried) adds about 3,500 lines. It is 704 commits behind main and conflicts with it in 14 files, so its cost is bringing it up to main and a second review. A consuming project then builds `tasks-embed` from its own board package, as the branch's guide says; until it does, its pass fails each minute there is a change to embed. B leaves the pass's hours as they are: the server already runs the pass as a process of its own, and under B that process starts the model's program, which loads the model each pass as today.

    **C, a build switch.** One package as now; the default build leaves the model out, and the modes say which build has it. The build with the model has its tests to run somewhere too, in the release run at least.

    **D, as today.** Nothing changes.

    **What would end the pass's hours**, short of A: a change to the pass itself, such as running it less often or only once a search by meaning is asked for, or keeping the model loaded between passes. That is a task of its own under B, C or D.

    **Why B.** It keeps what you asked for and the model you chose, and takes the 153 packages out of `tasks`, the server, `symphony` and `agent-send`, as your words below ask; it is built and approved, so its cost is a merge and a second review rather than new work. A is smaller and ends the pass too, but it removes a feature you asked for to save what B saves.

    **Earlier verdicts checked.** No card here asks about search by meaning. In the consuming project's register, from before this repository was split from it: its port plan records that you asked for tasks to be searchable by meaning, and your approval of `tasks-search-embedding-model` (A, 2026-10-29) chose bge-small and authorized "a local Rust inference runtime in a follow-up", which `crates/tasks/Cargo.toml` cites for the three packages. That card chose the model, not whether the feature stays, and B, C and D keep it. The board's database card (2026-10-06) offered search by meaning under both databases, and your approval chose the database. Your words of 2026-11-12, "we should really have separate crates that have lower binary sizes", are what B's task was filed on: A, B and C each meet them and D does not.
  [2026-11-15 06:48] owner · approved — A for now.
  [2026-11-15 11:13] agent — Recorded: A, for now. Search by meaning is deleted and the board searches by words alone: the board task `tools/board-search-by-words-alone` removes the model, its three modes, `tasks embed`, the server's pass and the packages they bring, and the branch built for B is dropped. The model's tables stay in the schema, unused, so no board moves for it. Bringing it back later would be a new card. The board's guide now tells agents to build nothing on it.
  [2026-11-15 11:14] agent · resolved

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

- [r] **server-socket-hand-over** — When the shared server takes a new build (the checkout it is built from changed, as a release's refresh changes it, or a newer build was started), the new server gets ready while the old one keeps answering, then takes over the old one's open port. Pages never meet a server that is down, and a new build that fails to start leaves the old one serving. Should it keep this hand-over, or simply stop and start again? A: keep it. B: a plain restart: the old server stops and the new one starts on the port; the watch for a new build and the build itself stay as they are. Recommended: A. Approve and say A or B in the note, or comment. [[analysis/code-review-2026-11#Choices that are the owner's]]
  [2026-11-15 03:46] agent — Posted for the board task `docs/code-review-owner-choices`, from the codebase review.

    **What you would notice under B**, at each switch:
    - The port is closed until the new server listens, then its pages wait while it warms up: 0.9 to 47 s over the 25 warm-ups in its log (one a project) across 13 server starts, 6.3 s at the median.
    - Open pages say "Reconnecting: the board may be out of date" and try again after 1 s, then 2, 4 and on up to 30 s. A page opened then fails to load or waits; a save fails and keeps its draft.
    - A new build that compiles but fails to start leaves no server until someone starts one, and the refresh's wait for it ends unanswered.

    Under A, open pages lose their live feed for about a second and take it again from the new server, warmed up already, and a failed start is said on the old server's notice while it keeps serving. Under both, the build itself (10 s to 21 minutes in the log) runs while the old server answers.

    **How often.** The server's log holds 10 hand-overs since 2026-11-11, none failed. Once, at 10:34:21 UTC on 2026-11-12, the new server (pid 42043) began to stop 22 ms after it took over, the log not saying why; it had stopped by 10:34:27, and the next server answered at 10:35:14. That was a stop, not a failed hand-over. A release's refresh makes a hand-over each time it moves the consuming project's pin.

    **What the refresh depends on.** The server's own reload: it sees the new `Cargo.lock` of the checkout it is built from, builds and switches. The refresh then waits up to 20 minutes for another process to answer on the port (`crates/symphony/src/release/refresh.rs`). Under B that still holds, as long as the new server runs as a new process rather than in the old one's place, which keeps its process number.

    **The lines.** The server's whole update is 2,046 lines of source and 2,920 of tests (`wc -l` of the server's `src/reload.rs`, `src/reload/stamp.rs`, `src/reload/source_event.rs` and `src/host.rs` with the library's `src/reload.rs`; and of the server's `src/reload/tests.rs`, `src/host_tests.rs` and `tests/after_reload.rs` with the library's `src/reload/build_tests.rs`). B keeps the watch, the stamp and the build. It removes the hand-over itself, about 530 lines of source (`sed -n '23,415p;619,651p' apps/symphony-server/src/reload.rs` gives 426 of them, `src/host.rs` 78, the hand-off signal and the old server's drain in `src/front.rs` the rest) and 549 of tests (lines 353 to 592 of `src/reload/tests.rs`, 1007 to 1230 of `tests/after_reload.rs`, and `src/host_tests.rs`), and needs a stop and a start of its own, with their tests, not written.

    **Why A.** It works, and B saves about a quarter of the update's lines for a gap of seconds at every switch and no server at all after a build that fails to start.

    **Earlier verdicts checked.** No card asks about the hand-over. **server-framework** (A, keep Axum) counted the server's own listener, which the hand-over relies on, in the cost of moving off Axum, and noted that Axum's next version will need the drain checked; it did not ask whether to keep the hand-over.
  [2026-11-15 06:48] owner · approved — A
  [2026-11-15 11:13] agent — Recorded: A, the hand-over stays. Nothing changes.
  [2026-11-15 11:14] agent · resolved