# Checks

What a task in this repository proves before review, who judges it, and where the full
verification runs. A consuming project keeps its own gates
([command ownership](../AGENTS.md#command-ownership)); the rule is RULES.md
`workflow/evidence-is-the-gate`.

A task does not run the whole suite: it proves that the work it did works, with the tests closest
to it, and the evidence good enough for its risk is its stamp. The full verification is left to
the release run, on main.

## The gate is evidence

Every task, parent or slice, feature or fix, proves that what it changed works:

- it runs the tests closest to its change ([picking them](#picking-the-focused-set)) and
  `just lint-rs`;
- a client change also builds the client for its tests and pictures (`just client-update`, which
  a server of the branch's own then serves) and commits no distribution: the land keeps main's
  `apps/symphony-server/client/public`, and the regenerator a land starts rebuilds it from main
  ([checks with no reviewer](#checks-with-no-reviewer)), so `git checkout --
  apps/symphony-server/client/public` puts main's back before the commit;
- before it moves to review it posts on its thread the exact commands, each one's result line
  (`test result: ok. 12 passed; 0 failed; …`), and the other evidence its kind of change asks for
  ([below](#evidence-by-kind-of-change)).

No task runs `just verify`, the whole browser target or the whole workspace's tests as its gate,
whatever its brief asks: that is [the release run](#the-release-run)'s.

What a task needs to land is its assessment's ([needed stamps](TASKS.md#needed-stamps)): an
assessor that neither filed, orchestrates nor executed it reads its diff when it is filed and each
time it moves to review, before any reviewer, and writes the stamps it needs from the project's
catalogue (`just tasks stamps`): `tests` (its recorded test runs green at the tip), `review`,
`pictures`, `owner`; a task no assessment has read needs `review` alone. The land lands it once
every stamp it needs stands on its tip, and refuses naming who gives a missing one ([the default
landing](TASKS.md#the-default-landing)). A change to a test or a check (`[land] check_paths`)
keeps `review`, since a loosened test or check is how a red turns green unfixed. A change of
settings' values needs `tests` alone, the settings check ([by kind](#evidence-by-kind-of-change)).

A gate that must give way for one task gives way by its orchestrator's recorded decision, never
by a word in a terminal: a limit raised, or a waivable stamp waived at one tip
([gate decisions](TASKS.md#gate-decisions)). A red test, a missing review where the stamps name
one and the owner's approve are never waived.

Where review is needed, a reviewer reads the diff against the evidence and judges whether it is
good enough for the change's risk, and builds or runs nothing the task's recorded runs at the
tip already show green ([no second build in review](#no-second-build-in-review)), never the
suite. Evidence that falls short is a `changes` verdict naming what to add. The reviewer's approve is that stamp:
the board verdict (`just tasks verdict SLUG approve "FULL_SHA: why"`, naming the tip it read,
RULES.md `workflow/reviewers-not-the-owner`), given on the task it reviews, never on a review
subtask, and drawn as a round of its review gate ([Gates](TASKS.md#gates)). A slice's evidence is
judged in its parent's review.

**Higher risk asks for stronger evidence, never for the whole suite.** A change's risk is what a
mistake in it would break and how far that reaches: one view, every agent's board commands, the
wire every client reads. Stronger evidence is more of the tests the change could break (every
test target of the changed package, the browser target still by filter; the targets of the
packages that call a changed public item), a target only the release run runs that covers it, or
a measurement before and after.

## Consumer API changes

A public Rust item a known consumer calls is replaced across at least two releases
(RULES.md `code/consumer-api-two-releases`, [its card](analysis/release-candidate.md#consumer-api-break-release)):

1. Ship the replacement alongside the old item, which keeps its signature and behaviour. Do not
   mark the old item `#[deprecated]`: a consumer's lint denies warnings, so the warning itself
   would break its gate. Add the replacement's consumer calls to the compile guard as they begin.
2. Consumers move to the replacement through their own tasks and landing gates, against a
   release that carries both items. Their checkouts are read-only to work here.
3. Remove the old item only in a separate task for a release after the one that shipped the
   replacement. Its evidence names that release and the consumers' migration commits, with the
   call sites read on their committed default branches showing that none still calls the old
   item. Update the guard's list with that removal, never simply to make a break compile.

The lists are `tests/consumer_api.rs` in `crates/tasks`, `crates/symphony` (also holding the kit,
wire types, docs, schema and args), `apps/symphony-server`, `crates/process-cwd` and
`crates/repository-read` ([what they hold](RELEASES.md#the-release-run), step 4). Run the affected
compile guards and `just lint-rs`; a reviewer checks every public-item removal or signature
change in a crate a consumer links against these lists and the release and migration evidence.
The guards' headers say what they leave to a consumer's own gate. `just semver-check` holds
the kit/helpers' public names, constructors' arity, exhaustive variants and derived traits
against crates.io. Their guards keep arbitrary argument, return and field types, cross-crate
type identity and known-consumer migration requirements: the tool does not compare arbitrary
types, and a major bump alone does not satisfy the two-release rule. Known consumer names stay
for that rule; vocabulary variant inventories wider than those calls are left to the semver check.

This overlap does not replace the kit's and helpers' major-version requirements
([packages on crates.io](RELEASES.md#packages-on-cratesio)), or the separate versioned
[`tasks api` contract](BOARD_API.md#the-tasks-api). Symphony's release checks build no consumer:
consumer-owned landing gates meet the new pin, and releases run this repository's checks alone.

## What a landing tells a consumer

A release's notes say what a project does to take it ([RELEASES](RELEASES.md#its-notes)), and
what git cannot derive is said by the landing that causes it. A branch owes an entry, a trailer
of one of its commits (`Consumer-Note: SECTION: TEXT`), when a project that takes the release
must change or run something because of it:

- `manifest`: a project's `Cargo.toml` changes more than the line that names the release (a
  package renamed, a feature a project must name).
- `board`: it moves the board schema: what the move changes for a project beyond the migrate.
- `api`: it ships a replacement beside a public item a consumer calls (`OLD => NEW`), or removes
  or changes one (`removed OLD => NEW`), the two steps of the rule above.
- `settings`: a `symphony.toml` key is read anew, ignored or refused, a file beside it is read, or
  a command a project runs for its settings takes other arguments.
- `init`: a project must do more than run `symphony init` again for the installed recipes, skills,
  hooks and rules.
- `tools`: it needs a tool, or a version of one, that the derived pins do not name.
- `once`: something a project runs once.

A change a consumer cannot see owes none. `just lint-rs` is red at the branch's own tip for a
schema move, a call a consumer-API guard stopped holding, or a key the settings reference lists
no more, each without its entry, and for a trailer that reads as no entry; a reviewer reads the
rest, and that an entry says what a project does, not what the change was.

## The board is a contract

The board's schema version (`schema::VERSION`, `crates/tasks/src/schema.rs`) is a contract between
the builds that read and write a board, and it counts breaking steps alone. A step that only adds
is an additive step on the contract (`schema::ADDED`): the version stays, the step's definitions
run after the contract's, and the board counts the steps it has (`meta:additions`). Builds on
either side of an additive step read and write the board. Only a breaking step moves boards under
their writers, and only it is held by the writers it would leave behind
([BOARD_OPERATIONS](BOARD_OPERATIONS.md#who-writes-a-board)).

An additive step may define:

- tables of its own, with any definitions of theirs;
- definitions of the database's own (an analyzer, a function), none changed;
- an index that is not unique, on any table;
- on an open table (`schema::OPEN`: `task`, `link`, `session`, `role`, `message`, `delivery`,
  `delivery_key`, `barred_key`, `task_session`, `ask`, `usage`), a field at the top of its rows
  that may be none (on PostgreSQL a nullable column), with no default, assertion, value or other
  rule of its own.

Anything else is breaking: a field on any other table, a field a row must have, a field retyped or
removed, an assertion changed, a unique index, an event or a trigger on a table there before, a
database-wide definition changed, a step that writes rows. The catalogue test holds it (below):
each step has its own files (`ENGINE-VERSION.STEPS.txt`), each the one before with only what the
list allows; the tables a step declares (`Addition::tables`) are the ones its definitions add; and
a board with rows in it, moved by the step, keeps every row as it was. What no catalogue shows is
the reviewer's, as for a declared step below: a rule held in code that a build without the step
skips, a meaning changed, a table such a build must write. The step's `why` says how a build
without it reads and writes around it.

What each side does, on both engines:

- A build behind the board's steps reads and writes it as it is. Every read of an open table's row
  passes a field it does not know, as the store's reader hands the row only the fields its struct
  names (`rows::open_row`), and every write of one sets named fields, so the field stays: no
  statement replaces such a row whole (`store/surreal/fence_tests.rs`). The whole-board dump reads
  each row exactly, handed every field, which its struct refuses by name, as a copy would drop it. The code of a step asks
  `Store::additions` before it reads or writes what the step defines.
- A build ahead adds its steps where it may move the board ([BOARD_OPERATIONS](BOARD_OPERATIONS.md#which-build-moves-a-live-board)),
  holding no writer, and uses the board as it is where it may not. The guard refuses, in the
  write's own transaction, a write naming a table of a step the board lacks, whatever its quoting
  or nesting, in the statement as it is sent on either engine; and the database refuses a field or
  a table it does not define, a SurrealDB board being STRICT. Nothing of a refused write is
  written.
- A whole-board copy, its staging, a recovery's rebind, and the dump that a copy, an export or a
  move to another engine reads, are held to the build's own steps exactly; the dump also refuses a
  field it does not know, naming it.

A breaking step raises the version and empties `schema::ADDED`, its steps' definitions going into
the contract's own. Contract 28 is the first: builds before it read rows as no field more than
they knew and replaced some rows whole, so its move refuses every build before it. A breaking step
owes the `board` consumer note (above) and starts a release run at once
([RELEASES](RELEASES.md#the-cadence)); an additive step owes neither, as a project's builds of the
contract keep reading and writing its board.

## A schema step that keeps the Symphony writing

This is the rule from before the contract (above), kept for a breaking step whose change the
Symphony one schema behind never meets; nothing is declared under it. A schema step that only adds
tables of its own, with their fields, indexes and events, and definitions of the database's own
changes nothing the Symphony one schema behind writes. Such a step may be declared to keep that Symphony writing: its version is listed in `schema::COMPATIBLE`
(`crates/tasks/src/schema.rs`) with one line saying why, and nothing is listed by default. Its
migration then writes, beside the version and in the same transaction on both engines, the oldest
schema whose Symphony may write the board (`meta:schema`'s `writable_from`): the version before
for a listed step, its own version otherwise. The schema guard reads that record in each write, on
both engines. A store the Symphony's reader opened (`symphony::taskview::connect`: the server, its
page saves and the `symphony` commands that reach the board through it) writes a board one schema
ahead only while the record names its schema or an older one. Every other case is refused, as for
a step not listed ([BOARD_OPERATIONS](BOARD_OPERATIONS.md#who-writes-a-board)):

- a record that is missing or of any other shape;
- a board two schemas ahead;
- a writer's store (`tasks`, `agent-send`, `tasks-copy`, a project's own program);
- a whole-board copy's staging;
- a recovery's rebind.

The declaration is checked where the database shows it. Each schema's definitions are committed,
one file per engine (`crates/tasks/tests/store/catalogue/ENGINE-VERSION.txt`):

- on SurrealDB, every table, field, index and event, and the database's analyzers, functions and
  parameters;
- on PostgreSQL, every column, constraint, index and trigger, and the functions in the board's
  schema.

The store test (`just test-rs -p symphony-board --test store catalogue`) holds a fresh board of
this build to its version's files, and holds a declared version to the files of the version
before. A line of a table that was there before fails it, whether added, removed or changed, and
so does a database-wide line removed or changed; the failure names the table. A step that changes
the schema writes its own files by running that command with `TASKS_CATALOGUE=write`, and commits
them. The files of earlier versions are never rewritten.

What no catalogue shows is the reviewer's to read. A step that does any of the following is not
declared:

- it adds a rule held in code alone that forbids what the Symphony one behind would still write
  to a table that was there before (a status move, a field's value, a cancel): that Symphony's
  writes skip the rule until a release at the step's schema is current, and what they wrote
  meanwhile stays, to be put right by a fix;
- it rewrites rows, or gives a field that was there before a new meaning, which the Symphony one
  behind would read or write with the old one;
- it adds a table the Symphony one behind is expected to write, which that Symphony knows nothing
  of.

A declared step is a schema move all the same. The release run started at once after it
([RELEASES](RELEASES.md#the-cadence)) and the hold of a second schema step until a release serving
the first is current both stand, and its landing owes the `board` consumer note. The declaration
helps only a Symphony that reads the record, which means a release that carries this mechanism
and is `current`. An older release refuses every write one schema ahead, whatever the record
says.

## Evidence by kind of change

- **A pure function** (a parser, a formatter, a state transition): unit tests of its edge cases
  beside it, and their run (`just test-rs -p PACKAGE --lib MODULE`).
- **A CLI behaviour** (a `tasks` or `symphony` subcommand, its output, its refusals and exit
  code): a test of the command, on a scratch board or a temporary folder, for the success and
  each refusal, in the target that tests that command: the board's `cli` target
  (`just test-rs -p symphony-board --test cli COMMAND`), with the store tests under it when the
  store changed; for a `symphony` subcommand, the `symphony` target named for its area (`asks`,
  `edit`, `picks`, `register`, `render`, `tree`). A change that adds, renames or retires a
  `symphony` subcommand, or changes a test that lists them, also runs the two targets that list
  every command the binary has: `just test-rs -p symphony-app --test serve` and
  `just test-rs -p symphony-app --lib cli::commands`.
- **A server route** (a handler, its status, headers and body, or where its answer comes from,
  such as the hub's last board read): the route's tests in the server
  targets (`-p symphony-server --test after_http`, `-p symphony-app --test serve`), the wire target
  (`-p symphony-api --test wire`) when a payload's shape changed, and the route's answer, status
  and body, read with `curl` from a server of the task's own on a scratch board
  (`TASKS_DATABASE=NAME just symphony --port 0`,
  [the server guide](../apps/symphony-server/README.md)), so a branch's build never opens the
  live board.
- **A client page.** Before and after screenshots of each changed view, dark and
  light where colour changed, desktop and phone width where layout changed
  (`just symphony-cli shot URL --out FILE.png`, with `--phone` and `--scheme dark`;
  `just symphony-cli shot --help` says the rest); the new tests for the changed behaviour (its
  pure parts in the client's unit tests, `just client-lint`; the page in
  `-p symphony-web-tests --test browser`); the existing tests the
  change was likely to break, updated where they pinned what changed, and green
  ([by page](#the-browser-target-by-page)); and `just client-update` for the pictures, its
  distribution not committed.
- **A page made faster** (a change meant to shorten a page's open to synced, in the client or the
  server): `tools/rs-bin.sh symphony-build synced BASE PAGE…` against a server of the task's own
  on a scratch board, before the change and after it, on the same machine under its working load,
  and the comparison the second run prints beside the first, which it names with `--against RUN`
  (the name the first printed: each run is kept in the checkout's artifacts folder, `synced/`,
  with the load, CPU busy and memory it ran under, and a server rebuilt on `--port 0` answers at
  another address), posted on the thread. A test holds work that must not come back (a second
  full read, a render a check did not need) by its count, never a time
  ([time in tests](#time-in-tests)).
- **A schema or launcher change** (the board's schema and its migration, `tools/rs-bin.sh`,
  `tools/cargo.sh`, the justfile, `symphony init`, the hooks): a mistake here stops every agent,
  so it asks the most: the migration run from the previous schema on a scratch board (the board's
  `store` target and `--lib schema`), the `perf` target for a query or index change
  (`just test-rs -p symphony-board --test perf`), the launcher's and hooks' own targets
  (`-p symphony-board --test launcher`, `-p symphony-app --test hooks`, `-p symphony-app --lib init`),
  and the command run in a scratch worktree, never against the live board.
- **A package's publication** (a member's `publish`, version, crates.io metadata, `include` list
  or a published package's dependency on a sibling): `just test-rs -p symphony-app --test packages`
  and `cargo publish --dry-run` naming every published package together, from the committed tip
  (no `--allow-dirty`), its output on the thread ([RELEASES](RELEASES.md#packages-on-cratesio));
  never a publication. A change to a published package's sources, a doc comment's too, moves its
  version where crates.io holds that version with other sources: `just lint-rs` is red at the
  branch's own tip until it does, by the crates check of the published packages the branch
  changes ([a version moves with its sources](RELEASES.md#packages-on-cratesio)). Its green lint
  is that evidence where the step's lines name the package as read; where crates.io gave no
  answer the step says `not checked` on stderr, naming the packages, and is no red and no
  evidence: run `just crates-check`, which reads every published package and fails on a crates.io
  it cannot read, and post it. The
  packages test is also the closest one for a change to a file a package carries a copy of (the
  client's `board.css` and `frame.css`, `tools/fixture_process.sh`): it fails until the copy is
  made again, naming the `cp`.
- **Weight added to a shipped binary** (a dependency or a dependency's feature, a library the
  binary did not reach before): `just size-check` at the tip, its lines on the thread, every binary within its
  line ([a red at the size check](#a-red-at-the-size-check)).
- **The kit's or a helper's public items** (what a project links at the version it names,
  [RELEASES](RELEASES.md#packages-on-cratesio)): the package's own tests, and the packages test
  (`crates/symphony/tests/packages.rs`), which fails a kit package that builds with a member
  outside the kit and a helper that builds with one outside the helpers. The reviewer reads what no
  test holds: that no public item shows a type of a member outside the package's line (rustc's lint
  for a private dependency's type is unstable), and that a struct added to `symphony_plugin::protocol`
  comes with a constructor (its `#[non_exhaustive]` the protocol's tests hold, as they hold
  `Request`'s and name each enum there left exhaustive). `just semver-check` compares all eight
  kit/helper packages with crates.io, requiring the version move their public API needs, including
  a new argument to a request's `new`. The kit's tests hold vocabulary lists `#[non_exhaustive]`
  and name the two left exhaustive (`crates/symphony-plugin/tests/vocabulary.rs`), and
  `crates/symphony-api/tests/vocabulary_drawn.rs` holds every kind to an arm in each match of
  this workspace, in the compiler's place. Serialization compatibility and arbitrary type changes
  still need the package tests, typed consumer guards and review. Review also checks that a new
  vocabulary variant moves the kit's minor: the tool allows an addition to a non-exhaustive enum
  even in a patch, while this project's contract requires a minor.
- **A doc** (a document, guide, skill or rule): `just symphony-cli docs-audit --check` (every
  document renders, and every recipe a text of this repository names is one a project has,
  [below](#the-recipes-a-text-names)) and `just symphony-cli status --lint` (no card slug in prose,
  nothing the plugins' prose rules refuse) clean, both run at the branch's final tip, after its
  last edit: a run before an edit is evidence of the text before it. Typed so, each is a test
  run the recorder keeps with its commit
  ([MACHINE_WORK](MACHINE_WORK.md#recording-builds-and-tests)), so its reviewer reads it as
  recorded, like any test. A guide, or a document under
  `docs/` but `docs/history/`, a card's own lines and a register region aside, also runs the check for the owner's words in the
  shapes the guides gave them, dated or straight-quoted (`just test-rs -p symphony-app --test hooks
  owner_words`, RULES.md `docs/no-ledger-in-documents`), and its reviewer reads for any other
  shape. For a shipped skill,
  `just symphony-cli init --root .` run so the installed copies and their stamps are current, and
  the shipped skills' tests (`just test-rs -p symphony-app --lib init`). It runs no other test.
- **A change of settings** (values of `symphony.toml` for settings the Symphony already reads,
  with the guide's or the rules' words that repeat them): `just settings-check` at the tip, the
  project file's shape alone ([PROJECT_FILE](PROJECT_FILE.md#the-settings-check)), recorded as
  a test run, which is its `tests` stamp, and the doc checks above for the words. It runs no
  other test and takes no reviewer ([TASKS](TASKS.md#needed-stamps)).

### The recipes a text names

A `just NAME` in what this repository says is a recipe `symphony init` installs
(`crates/symphony/shipped/symphony.just`), the consumer's landing, named as the project's own, a
word of prose, in one of its guides a recipe of its justfile, or, in a text about this repository,
one of its own recipes or one filed to be made that the lists name for that file: a freshly
initialised project has no other. The lists are `tools/named-recipes.txt`, this repository's data,
a line to a list (`consumers NAME...`, `prose WORD...`, `own FILE NAME...`, `filed FILE NAME
TASK`), which one reader (`crates/symphony/src/docs_audit/recipes.rs`) reads from the checkout, so
no binary carries them. The word after `just` is read past the options that leave a recipe to run
(a justfile or folder named, the quiet, verbose, dry-run and yes switches), across one line break
but never a blank line. One rule, two places:

- the docs audit, in the Symphony's own checkout alone (its project file names it `symphony`, and
  it holds the lists), judges its documents, the Markdown under `docs/` but the decision register
  (`docs/DECISIONS.md`), `docs/analysis/` and `docs/history/`, which are records, as are a card's
  own lines wherever it sits in a document, which it passes over; and, of the files
  git's index holds (so a new one once added), every file of what it delivers, under
  `crates/symphony/shipped/` (the skills, the Codex rules and the recipes), and its guides, each
  `AGENTS.md`, `README.md` and `RULES.md` at the root or in a package under `crates/` or `apps/`.
  A consuming project's texts name its own recipes, and its audit judges none of them, the
  Symphony's crate vendored in it or not;
- the hooks test (`named_recipes`) judges what the Symphony delivers too, and the strings the
  binaries print, which no docs change touches: a change to such a string runs it
  (`just test-rs -p symphony-app --test hooks named_recipes`).

Each name either refuses is an error naming its file and line. Each entry of the lists is kept
current, still named in its file and a filed one beside its task (`[[task:SLUG]]` on the line that
names it), by the one place that judges its file: the audit for the texts above, the test for every
other. Neither judges the records, the tests, `tools/`, or the installed copies of the shipped
skills under `.agents/skills/`, which `symphony init` keeps the same as what it delivers.

So a text that names one is red at its own tip, and should it land, the regenerator's run of the
audit on main after the land's round files it within minutes
([checks with no reviewer](#checks-with-no-reviewer)), not the next release.

## Picking the focused set

1. List what the branch changed: `git diff --name-only main...HEAD`.
2. Name each file's package: `crates/tasks` is `symphony-board`, `crates/symphony` is
   `symphony`, `apps/symphony-server` is `symphony-server`, `apps/symphony-web-tests` is `symphony-web-tests`
   (the client's browser and build tests), `apps/symphony-desktop` is `symphony-desktop` (the Mac
   app, whose windows no test opens), and any other `crates/NAME` is `NAME`; `apps/symphony-web`
   is the client, no Cargo package, whose unit tests `just client-lint` runs with its other checks.
3. For each package:
   - the unit tests of the changed modules: `just test-rs -p PACKAGE --lib MODULE`, the module
     path as a filter (`store::work` for `crates/tasks/src/store/work.rs`);
   - the integration targets that exercise them. A target is a file or folder under the
     package's `tests/` (`--test store` is `crates/tasks/tests/store/`).
     `just test-rs -p PACKAGE --test TARGET -- --list` prints its tests' names; a filter is any
     part of a name or module path (`priority::`, `queue`), and several after `--` run every test
     any of them matches;
   - when a public item's contract changed, the tests of the packages that call it (`git grep`
     its name).
4. `just lint-rs` (which runs `just client-lint` and the board tests' raw-statement fence,
   `crates/tasks/tests/raw_fence.rs`), and, when `apps/symphony-web` changed, the client built (`just client-update`) for its tests and
   pictures, its distribution never committed.
5. Main is merged once, when the work is complete, and the targets steps 1 to 3 pick run again
   at the new tip, before review, as main may have changed what the branch's code meets
   ([after main moves](#after-main-moves)).

A package's own run (`-p PACKAGE`) and the workspace's can differ, as Cargo turns on a
dependency's features for every package one build takes: the workspace and `-p symphony-server`
link `serde_json` with `arbitrary_precision`, `float_roundtrip`, `preserve_order` and `raw_value`,
as the server does, and so do the own runs of `symphony-board` and `symphony-app`, as the `tasks` and
`symphony` binaries do (the board's dependency asks for them), of `symphony-api`,
`symphony-blocks` and `symphony-plugin` (a dev-dependency asks for them, which never reaches the
client or a plugin's build) and of `symphony-schema` (`raw_value` aside, which changes no read),
while the own runs of `symphony-web-tests` and `symphony-chrome` link it with none of them, so a
test of theirs that reads a JSON object's key order or a number's text can pass alone and fail in the
workspace's run.

Steps 1 to 3 without the filters are mechanical, and a recipe that runs them from the branch's
diff, `just check-fast`, is filed as the task [[task:tools/check-fast]]. Choosing the filters
stays the agent's, and the reviewer's to judge.

### The browser target by page

`just test-rs -p symphony-web-tests --test browser -- FILTER…` builds the release client once and
opens each test's page in headless Chrome. Most of its tests are named for the page or control
they open, so a page change runs the names that page's words match. Others draw several pages
each, under names no page's words match, and a change to a page they draw runs them too:
`polish::` (its popups, panels, headings and trouble blocks), `shot::` (the `shot` command's
own tests, which capture the tasks board), `projects::` (every link a page draws, under the
project's base) and `budget` (what an entry page loads). Each row below is the whole filter,
both kinds, to copy after `--`:

| What changed | Filters after `--` |
|---|---|
| the home queue (`/`) | `queue` `polish::` `projects::` `budget` `phone::` |
| the queue page (`/queue`) | `queue` `cache_pages::` `landmarks::` `skeleton::` `header::` `look_parity::` `postponed_labels` `later_advances` `restored_mockups` `reconnect_pills` `polish::` `projects::` `budget` |
| the tasks board (`/tasks`) and its cards | `board` `task_columns` `task_controls` `tasks_and` `a_revealed_column` `owner_` `machine_work` `run_records::` `cache_pages::` `kept_pages::` `landmarks::` `layout_shift::` `skeleton::` `feed_deltas::` `header::` `look_parity::` `restored::` `components::` `polish::` `shot::` `projects::` `budget` |
| a task's page, its gates and its timeline | `task_page` `task_panel` `task_priority` `task_gates::` `timeline::` `owner_controls::` `owner_save::` `cache_tasks::` `cache_ahead` `feed_deltas::` `notice_layout::` `landmarks::` `layout_shift::` `skeleton::` `header::` `look_parity::` `restored::` `components::` `persistent::` `polish::` `projects::` `budget` `preload::` |
| the agents page (`/agents`) | `machine_work` `the_agents_page` `header::` `look_parity::` `the_chat_opened_on` `polish::` `projects::` `budget` |
| the reports page (`/reports`) | `reports::` `header::` `polish::` `projects::` `budget` |
| the runs page (`/runs`) | `runs::` `header::` `polish::` `projects::` `preload::` `budget` |
| document and folder pages | `document` `docs_index` `owner_` `cache_ahead` `cache_pages::` `restored::` `landmarks::` `skeleton::` `layout_shift::` `look_parity::` `polish::` `projects::` `budget` |
| the header, on every page | `header::` `floating_controls::` `panel_open::` `persistent::` `polish::` `projects::` |
| the panel (the chat, add task, the activity list, Appearance, toasts, the pending bar) | `panel_open::` `persistent::` `header::` `floating_controls::` `components::` `look_parity::` `server_notices::` `notice_layout::` `polish::` `projects::` `budget` |
| shared controls (select, menu, button, tooltip, theme) | `components::` `select` `theme` `polish::` |
| the project switch and a project's look | `projects::` |
| the feed and its stream | `feed` `stream` |
| what an entry page loads (its bytes, its order) | `budget` `phone::` `preload::` |

## Worked examples

**A board command's refusal.** The branch rewords the refusal `just tasks priority` gives for a
value outside 0 to 4, in `crates/tasks/src/store/work.rs`, and updates the store test that pins
its text.

```
just test-rs -p symphony-board --lib store::work
just test-rs -p symphony-board --test store priority::
just test-rs -p symphony-board --test cli priority::
just lint-rs
```

The thread gets the four commands and their result lines. No `perf`: the statement the store
sends is unchanged, and a change to it would add `--test perf`. No client test: no client file
changed.

**A row on the queue page.** The branch shows each card's priority beside its title in the queue
page's rows, in `apps/symphony-web/src/queue.tsx` and `apps/symphony-web/src/queue.css`.

```
just symphony-cli shot URL --out queue-before.png   (desktop and phone, light and dark, before the change)
just symphony-cli shot URL --out queue-after.png    (the same four, after it)
just client-lint
just test-rs -p symphony-web-tests --test browser -- queue polish:: projects::
just client-update
just lint-rs
```

A new unit test in `queue.test.tsx` pins the priority in the row's markup, and `just client-lint`
runs it with the lint, the type check and the wire types' check; `queue` runs the browser tests of
the queue's counts, order, spacing and filter colours, which a taller row or new markup could
break, and the branch updates any that pinned the old row, naming it. `polish::` and `projects::`
draw the page's panels and its one heading, and the link each row draws under the project's base.
A change to home's rows would add `budget` and `phone::`, as home is an entry page. The thread
gets the eight pictures, the commands and their result lines and the updated tests named; the
commit holds the source alone, `just client-update`'s distribution put back as main has it.

## The release run

`just verify` is the full verification: lint, the real client check, the
[size budgets](#size-budgets), the docs audit, the crates check
([RELEASES](RELEASES.md#packages-on-cratesio)), and every test target of the workspace, the
headless browser acceptance and the board's `perf` and `child_guard` targets
([the board guide](../crates/tasks/AGENTS.md)) among them. It is the release run's, and no task's.
`just release` runs it on main's tip, as fetched from `origin`, in a clean checkout outside this
folder (`../symphony-wt/release`), and judges the commit by that alone, with a check of who
bypasses the default branch's rules after it where the project names one
([RELEASES](RELEASES.md#who-bypasses-the-rules-again)): it builds and tests nothing of the
projects this machine serves, whose own gates meet a change when they take a release.
Where the project names `[releases] hosted`, the same `just verify` of the same commit runs on
GitHub's runners instead, a job for each step, and the run reads its result, files its reds and
tags here; what GitHub does not answer is verified here
([RELEASES](RELEASES.md#on-githubs-runners)), but in a run started by
[the hosted clock](RELEASES.md#the-hosted-clock), which verifies nothing here. It is off unless the project says, and never a
task's gate: no push or pull request starts it. A
green run tags a release; a red one files each red check as a task in the
`tests/release-red-*` series at p0 ([TASKS](TASKS.md#slots)), staffed first and filed as a fast
fix whose orchestrator lands only a fix changing no test or check (RULES.md
`workflow/release-reds-first`), one task for each red step of verify, naming the commands that
rerun each failure alone (each failed test, else the step's own command, a test run's a target at a
time), the load when it ended and the commits since the last green run
(`git log --oneline LAST_GREEN..TIP`), and the next green run closes it; a run that
stops short of deciding is the red `tests/release-red-run`. Before it files a red, the run reruns
each failed test alone, one at a time, over the same build (below): one that fails again is red,
its task saying it failed twice, and one that passes alone is no red, and has one p1 task of its
run's ([RELEASES](RELEASES.md#the-release-run), step 5). A test red on load all the same is
answered by its discard ([time in tests](#time-in-tests)). A run red in a check holds its commit
as the release candidate while its reds' own tasks fix it on the branch `release-candidate`, then
reruns what was red and what the fixes changed, and releases that commit and its fixes alone
([RELEASES](RELEASES.md#the-release-candidate)). It is the one place every test runs
on main: `just land` runs none ([after main moves](#after-main-moves)). It may be scheduled more
often than daily, and its schedule is the owner's to install. It also runs every 20 landings:
a land starts `--due` on the main checkout's linked build of `symphony`, which counts with git
alone and builds nothing until that many tasks have landed since the last release or the last run
that checked something; then it builds in the release run's own checkout and runs on main's tip,
never beside a run in progress ([the cadence](RELEASES.md#the-cadence)). How it runs, what it
records and how it is scheduled: [RELEASES](RELEASES.md#the-release-run).

**Every step runs.** `just verify` runs its steps in order, lint, the client check, the size
budgets, the docs audit, the crates and semver checks, the hosted board's check
(`just hosted-check`, [POSTGRESQL](POSTGRESQL.md#a-hosted-board)) and the tests (`tools/verify.sh`), each whatever the
ones before it did, and is red when any is, so a red step never hides the ones after it. Each step
is recorded as a step
of the verify run ([RUNS](RUNS.md)) and writes its output to a log of its own,
`build/verify/STEP.log` (a folder of the run's when the release run runs it), which `steps.tsv`
beside it lists with each step's exit status, seconds and command and the load average it ended
under (over 1, 5 and 15 minutes, with the processor count). Its own output says each step as it
starts and ends, with the last lines of a red one, and at its end each step with its log, then the
red ones. `just verify STEP...` runs those steps alone, in that order; `tests/LANE` in place of
`tests` runs that lane of the tests alone, which is how a release candidate's rerun runs what a
fix changed and carries the green of the rest ([RELEASES](RELEASES.md#a-steps-inputs)). What each
step reads is said beside it in `tools/verify.sh` (`reads`), and a step that reads a path outside
the packages it names, or outside the root's other entries, says so there. The tests a red
`nextest-setup` keeps from starting are listed all the same, their status `-`: not run, no red of
their own, and run by the next rerun. An interrupt (Ctrl-C) stops
it once the step running ends: it runs none after it, says where it stopped and exits 130. The tests serve the client
the client check built in the same verify (`SYMPHONY_UI_DIST`), and the check's folder
(`build/verified-client`) is cleared before it runs, so a client an earlier check left there is
never served. When the client check is red on a client it built, the tests run against that one;
when it built none, or did not run, the tests runner publishes one client before scheduling,
red if that build fails too. The tests' log and the summary say which.

**The tests side by side.** The tests step (`tools/test-lanes.sh`) builds the workspace's test
binaries once with cargo-nextest 0.9.152, then reuses that binary metadata for one nextest scheduler.
The release owns an explicit `nextest-setup` step before tests, including a tests-only rerun. It
runs `just nextest-setup`: this target's `tools/nextest/` is checked first (`CARGO_TARGET_DIR`, else
`.target/`), then a linked worktree's main checkout `.target/tools/nextest/`. With neither copy
valid, setup provisions the main cache once for its worktrees, or this target in a main checkout.
It downloads the [upstream pinned release](https://github.com/nextest-rs/nextest/releases/tag/cargo-nextest-0.9.152),
verifies the archive and extracted executable against SHA-256 values in `tools/nextest.sh`, and
publishes it by one rename. Every use verifies the executable's hash and version before executing
it. No Cargo install, build folder or unverified PATH copy is used. Ordinary release cleans keep
the cache; if it was cleared, the next setup step provisions it again. A provisioning failure is
red at `nextest-setup`, and tests does not start. Tests never download or install a runner.

Focused `just test-rs` uses Cargo and needs no nextest. The live success/failure/stall fixture is
explicitly ignored in that default run; the tests step selects it once by its exact name through
workspace Cargo before concurrent batches. Its missing runner is a failure, never a skip. The
nextest filter excludes that case, so it is neither omitted nor run twice. Cargo keeps the same
workspace features, profile and environment as the one build for this invocation too. A failure's
narrow rerun retains `--ignored` for this case, so it executes the fixture again.

To run that live fixture after setup, select it explicitly. The recipe supplies `--locked`:

```sh
just test-rs --config 'profile.dev.build-override.debug="line-tables-only"' \
  --workspace --no-fail-fast --lib -- --ignored --exact \
  release::nextest::tests::the_pinned_runner_reports_a_fixture_failure_and_a_terminated_test --nocapture
```

Nextest runs six cases at once (the caller's `RUST_TEST_THREADS` wins), with groups capping Chrome
and database cases at two each (`.config/nextest.toml`). Retries are explicitly zero. Every nextest case
has its duration in the log, and a per-case timeout ends a stalled case and its children. A timeout
is a failure, never a pass, and replaces none of the tests' own assertions. The runner emits both
its status lines and libtest-json-plus 0.1 events: the release readers use the events to name each
failed or timed-out case and the exact Cargo command that reruns it alone. Captured output stays
inside an event, so a command a fixture prints is no failure of the release run.

The standalone target compiles a copied workspace: its cases run after the other nextest cases
and reserve the entire nextest pool. The shared Cargo batches below still run independently;
this reservation controls nextest's own child servers and work.

Every terminating timeout in the release profile is a stall backstop, not a cost budget. Its
deadline is three times the ceiling, in seconds, of the slowest completed loaded measurement
available for its scope. A complete Cargo binary's time is a conservative upper bound for any
one of its cases. These are the measurements that derive `.config/nextest.toml`:

| Scope | Loaded measurement | Associated load, 1/5/15 minutes | Source | Backstop |
| --- | --- | --- | --- | --- |
| Ordinary nextest cases | 131.95 s, whole `after_lifecycle` binary | 124.30 / 142.61 / 111.32 | Release `2026-11-18T0825-74490`, commit `7f1842c6051b` | 3 × ceil(131.95) = **396 s** |
| Standalone target | 869.16 s, whole binary, completed | 124.30 / 142.61 / 111.32 | Same release's tests log | 3 × ceil(869.16) = **2,610 s** |
| Ordinary-case cross-check | 118.725 s, semver's real checker case | Step sampled up to 151.29 / 108.97 / 97.70 | Task run `82f06742130c0cff7b6ff249c74444db`, commit `3f534895` | Covered by the 396 s default |
| Standalone cross-check | 304.09 s, case alone | Run sample 159.54 / 118.83 / 117.52 | Focused Cargo rerun of `3f534895` | Covered by the 2,610 s override |
| Superseded standalone limit | Killed at 600.025 s; completion time unknown | Loaded task run above | Task run `82f06742130c0cff7b6ff249c74444db` | Censored evidence, not a completed timing or a valid backstop |

Release load samples are those the tests step recorded when it ended; task samples are from
the running checks, not a claim of constant load. The loaded release's completed 869.16 s
measurement also exceeds the censored 600.025 s observation, so no completion time is inferred
from the killed run. The original task baseline was an earlier, dirty source over `284fafb7`:
1,615 s at sampled load roughly 45–84, 7,940 passed and 16 ignored including four doctests.
It remains historical evidence, not a same-source or quiet-machine performance comparison.

The periods in the config are those measured ceilings, with termination after three periods.
The ten-second termination grace begins after the case is already failed as a stall; it is
cleanup time, not a passing allowance or test budget. The repository-only `test-stalls` stream
annotator reads the actual release profile and adds the reached limit and the load at the event
to each nextest timeout. The red reader says `taken as a stall`, its limit and that load, and
keeps the same exact rerun command. All other output passes unchanged. A reporter failure makes
the step red, and no retry changes a timeout into a pass. Raw historical nextest logs remain
readable, with no invented limit or load where none was recorded.

The suites with shared fixtures retain one libtest process: the board's `perf`, `sweep`, `cli`,
`server`, `store` and `launch`; the app's `after_commit`, `after_publication`, `after_worktrees`,
`hooks`, `picks` and `project`; the server's `after_http`; the build package's `release`; and the
browser suite. They share the store fixture's database server (including its transitive imports),
perf's generated board, sweep's repository template or the browser's build gzip cache. Each
integration target is one Cargo batch. The `symphony-app`, `symphony-board` and `symphony-server`
library tests share that store fixture too: a Cargo library batch uses the build command's Rust
exec adapter to run exactly those three metadata-listed images, leaving the stateless libraries
to nextest. Cargo supplies the package working directory, environment and dynamic-library paths;
the adapter changes none and replaces itself with the selected image, preserving arguments,
process identity, signals and status. An artifact absent from the metadata is refused.

Cargo also runs the doc tests, which nextest does not run. These exceptions, rather than a table
partitioning every target, are the runner's list (`tools/test-lanes.sh --list`); other binaries,
new targets included, belong to nextest. The shared-process exceptions keep their libtest suite
measurements and test assertions; they acquire no nextest per-case timing or timeout. A path or
metadata cache and an executable's warmup do not require a shared process: each nextest case can
prepare its own from the supplied build outputs.

The client's `build` test runs before scheduling: it may relink `symphony-build`, which other
targets execute or copy (`standalone`). When the client check supplied no build, the runner
publishes one client before scheduling, so no browser case builds or relinks while other tests
run. Every test receives the build's binaries through `SYMPHONY_TEST_TASKS_BINARY` and
`SYMPHONY_TEST_BINARIES`; a caller's values win. Each Cargo call keeps the workspace build's
feature graph and its build-script/proc-macro debug info, so no concurrent batch relinks anything.
On macOS Cargo still makes each binary's copy in the target folder again at every call, removing
it first; elsewhere it hard-links the binary to its build, copying only where it cannot link, and
a later call leaves a link in place. So before scheduling the runner makes each copy the build
lists a hard link to the file it copies, which every later call leaves in place: a batch that
starts never leaves another's test without the binary it runs. Every server, store and browser
still takes its own folder and a port the system picks.

Nextest and the Cargo batches keep live logs in `tests-lanes/` beside the step's log
(`SYMPHONY_TEST_LANE_LOGS`; `build/test-lanes/` when run by hand), with the binary metadata beside
them, and `lanes.tsv` there lists each lane that ran with its exit status and seconds, which the
verify lists after the tests step as its parts (`tests/LANE`): the step's red stays one red and
one task. `tools/test-lanes.sh LANE...` runs those lanes alone, over the one build. What each lane
reads is said beside the lanes (`reads`: its test target's packages, derived, another package
whose binary its tests run, a path outside them, or the whole tree), and a new lane, or a test
that starts reading a file outside its packages, says so there
([RELEASES](RELEASES.md#a-steps-inputs)). The step names each log before scheduling. At most half the processors online run batches
at once (at least one); each batch's output follows whole in the step's log once they all end.
Signals reach the foreground batches, which are waited for before the runner exits, and a forced
stop leaves their live output at the named paths. Native failures retain Cargo's target lists;
nextest failures retain their own events. A batch is green only on an explicit exit status of zero.

**A failure rerun alone.** The runner names the folder of its lanes' logs first (`test-lanes:
logs in FOLDER`), keeps there the client its tests ran against beside the binary metadata, and
says last that it ran every lane it was asked for (`test-lanes: ended: every lane asked for ran`),
a line a run cut short never reaches. `tools/test-lanes.sh --alone FOLDER ARGS` reruns one
failure of that run alone over its build, ARGS as a red's rerun line names it (`-p PACKAGE --test
TARGET -- NAME --exact`, a target with no name for one rerun whole, `--workspace` and a kind for a
lane's own line). A test, or a target whole, runs by nextest from the folder's `binaries.json`, by
the filterset of its package, kind, binary and name, with no retry, `--no-tests fail` and the
release profile's timeouts; a doctest, or the ignored runner fixture, by Cargo as its lane runs
it; a lane's own line, or a shared-fixture target whole, as that lane alone, its logs in
`FOLDER/alone`. Nothing is built again, and it exits as the rerun did. The release run reruns each
failure of a red tests step this way, at most 12, before it files a red
([RELEASES](RELEASES.md#the-release-run), step 5).

**Until the owner installs its schedule, only the cadence runs `just verify` on main.** A break
that no task's focused tests reach stays unseen until 20 landings have gathered: the gap is the
price of focused gates without their full run, and narrows once it also runs each day. The
first landing with no release and no run on record counts the line's whole history, and starts
the first run at once.

## Time in tests

A test passes or fails on what the code did, never on how long a loaded machine took to let it
(RULES.md `tests/no-test-fails-on-load`). The machine the release run runs on also runs every
agent's builds, at load averages of 50 to 700, so a test reads time in one of three ways, through
the workspace's one helper, `crates/test-time` (a development dependency only):

- **A cost** is the CPU the work took, or a count of the work, never its wall.
  `test_time::cpu::spent(|| work)` bills a closure the CPU of its thread; `thread()` and `process()`
  read this thread's and this process's CPU; `of(pid)` reads a live process's
  (`proc_pid_rusage` on macOS, `/proc/PID/stat` on Linux) and `reap(pid)` what a child took in all
  at its reap. The board's `perf` target bills its server this way (`crates/tasks/tests/perf/cpu.rs`).
- **A wait** looks at its condition until it holds:
  `test_time::until(WHAT, test_time::backstop(ALLOWANCE), || condition)`, or `until_true` for a
  condition that holds or does not. Its only time is a wall-clock backstop, six times the wait's
  allowance and never under two minutes. The Chrome driver's own waits keep the same rule
  (`crates/chrome-devtools`). Past it, the wait fails with a `Stall`: `WHAT: stalled: still not so
  after N looks, and the wait's wall-clock backstop of B ran out after W; load average a b c over 1,
  5 and 15 minutes`. A loop of `sleep`s with a count, or an `Instant` deadline asserted, is a wait
  written by hand: rewrite it. A sleep followed by a check that something did not happen proves
  nothing on a loaded machine, where the sleep may end before the thing would have. It waits on a
  positive barrier instead.
- **Real time** stays only where the product's promise is elapsed time: a deadline, a backoff, a
  page's two seconds on a phone. Its clock is injected where it can be: an environment variable,
  a setting the test passes, or a page clock the test drives. A test that cannot inject it stays
  and runs in `just verify` until the owner says otherwise.

**Who takes a test out of the release run: the owner, and no one else.** An executor who finds a test
that none of the three ways can hold proposes it on its task's thread: the promise the test guards
and why no CPU bound, count, wait or injected clock stands in for it. The orchestrator puts it to
the owner, and the test leaves `just verify` only on the owner's word. A test the owner lets go on
such a proposal is not dropped but moved: to a recipe a quiet machine runs, which still runs the
test. No such recipe exists yet, so nothing is `#[ignore]`d for a quiet machine; a test the owner
lets go so waits in the release run until that recipe lands, and is then listed below with its
promise and the owner's word.

**A test that fails in the release run and passes alone is no red.** The run reruns each failed
test alone, one at a time, in the same checkout and over the same build, up to 12: one that passes
alone holds no release, and the run files one p1 task for all such tests of it, outside the red
series, which finds why, then fixes the code or the runner, rewrites the test on a condition, a
count, CPU time or an injected clock, or discards it with one line on its thread; one that fails
again is red, its task saying it failed twice ([RELEASES](RELEASES.md#the-release-run), step 5).
A retry of nextest's never counts as a pass, and no timeout or budget is widened for a rerun.

**A test red on load in the release run all the same is discarded.** A rerun neither fails nor
passes on the machine's load, so a red test that failed alone too, or was not rerun, because of
load (its failure is a wall-clock bound, a sleep-ordered step or a timeout under load), in a
release run or the release candidate's rerun, is answered by its red's own fix removing it from
the tree, with one line on the red's thread naming the test, the evidence it is load (its runs
alone and the failing line) and what it proved. This rule is the owner's approval: no card is put to them to discard it. If what it
proved matters, a follow-up task may bring it back written on a condition, a count, CPU time or an
injected clock; the release does not wait for that. No rerun is spent on it, no rewrite of it is waited for, and
nobody lowers the machine's load to help a run pass: an orchestrator holds no other work back to
keep the load down for a run. A test that fails alone on what its code did, or fails the same way
without load, is not this case and is fixed in the code.

No budget, allowance or tolerance is widened to make a loaded run pass. Evidence for a test
rewritten under this rule is the test run alone five times on the loaded machine, the load average
stated with each result, and once more under a load of the agent's own (a few busy loops in its own
process group, started and stopped by their PIDs).

**Out of the release run for a quiet machine:** none.

## Size budgets

What Symphony ships is checked for size, so it never grows unseen and can be brought down. Each
binary this workspace delivers (`symphony-server`, `symphony`, `tasks` and
`agent-send`) has a budget, and so do the client builds the server embeds (those
`symphony-build publish-client` keeps under `apps/symphony-server/client/public`): one line each in
`tools/size-budget.toml`, in bytes.
`just size-check` builds the four as `cargo install` builds them, with `Cargo.toml`'s
`[profile.release]`, and runs `symphony-build size-budget`, which prints each size beside its
budget (`symphony-build`, the commands only this repository's build and release run use, is no
binary it delivers, and has no line; nor has `tasks-move`, which a release installs beside
`tasks` for the one move of a board's store and no project links or runs by a recipe, its bytes
said in the release's manifest and the install's line).
Past a line it fails, naming the size, the budget and how to see what grew: `cargo bloat --release
--crates --bin NAME` (`cargo install cargo-bloat --root DIR`, never a workspace dependency) beside
the same at a commit within it (the last release, or before the first one, the commit that last
set the budget file), or for the client, the newest build's files beside the build before it. A
binary's line is its file; the server's is its file less the client it embeds, which has its line
of its own, so each line grows for one reason. The sizes are this machine's (aarch64-apple-darwin),
where the release run runs.

The client's line counts each build the distribution keeps whole (its `builds.json`, newest first):
a file two builds share is counted for each, and `index.html` and `builds.json` once. That is the
most the distribution weighs with those builds, which it does once they share no file: a change to
the chunk every page imports renames every chunk that imports it, as each carries the shared one's
name and export names in its own bytes, so the build after such a landing shares almost nothing
with the two before it. What the server embeds, each file once, rises and falls with how many
files the kept builds happen to share (from about one build's weight to three at three builds); the
line moves only with what a build weighs and how many are kept. The check prints both, with each
build's weight, so one build's growth shows though the line is the builds'. A file under `assets/`
that no kept build names, or a build naming a file that is not there, is an error, not a pass: it
would be embedded and not counted.

The shipped binaries are the ones `symphony init` links for a project (`BUILDS`,
`crates/symphony/src/init.rs`), and a test holds the recipe's build to them: a binary of them
with no line, or a line for any other, is an error, not a pass. So is a server built before the
client files beside it last changed (a hand run after `just client-update`), whose own bytes the
check cannot tell; it says to rebuild.

A binary links what it may run, and a test holds a library out of one that never runs it
(`tests/links.rs` of `symphony`): the JSON Schema validator, which would add a fifth to its size:
a schema check is a library call a consumer's code makes (`symphony::schema::validate`), and
`symphony serve` answers `validate` as a function it lacks. And it holds the board's migration out
of it, with both databases' schema texts: `symphony` opens a board only to read, by the reader's entry
(`Store::read_as`, `Store::reading`), which names no migration, where an opening by the rule
(`Store::opening`) links it for the writer it may be handed.

On macOS every binary of the three packages links with `-no_exported_symbols` (each package's
`build.rs`), and the same `tests/links.rs`, with the board's, holds each shipped binary to it. A
link writes by default an export trie, a name for each global symbol, for code that loads the
binary to find them by, and no code loads these: the trie the flag leaves out is about 360 KB of
the server and 330 KB of `symphony` and of `tasks`. Every crate builds in one codegen unit
(`codegen-units = 1` under `Cargo.toml`'s `[profile.release]`): in the compiler's default 16 a
crate compiles a generic or helper into each unit that reaches it, with its unwind tables, inlines
across none of them, and copies a large constant table (comrak's, regex-syntax's Unicode tables)
into each unit that reads it. One unit saves about 2.7 MB of the server, 1.8 MB of `symphony`,
2.4 MB of `tasks` and 0.5 MB of `agent-send`. It costs each crate's release compile, a cold
release build about 30 % longer, and a change's size no longer moves with code shifting between
units.

The release profile leaves out the symbol table (`strip = true`), and with no export trie either,
nothing in a shipped binary names a function. A panic still prints its thread, file:line:col and
message, but `RUST_BACKTRACE=1` prints no frames and `RUST_BACKTRACE=full` raw addresses, and
lldb and atos (so `sample`, a crash report and Instruments) name no frame. A build to debug or
profile sets `CARGO_PROFILE_RELEASE_STRIP=debuginfo`, which keeps every name.

The release profile and the link flag are this workspace's: a consuming project that builds its
own host or board binaries in a workspace of its own gets them only by setting them there. That is
`strip = true` and `codegen-units = 1` under `[profile.release]` in its workspace's `Cargo.toml`,
and, on macOS, `cargo:rustc-link-arg-bins=-Wl,-no_exported_symbols` printed by its host package's
own `build.rs`, as Cargo passes a link argument only to the bins of the package that prints it,
never to a dependent's.

It is a step of `just verify` (`size-check`), so [the release run](#the-release-run) runs it on
main's tip. No task's gate runs it, as it costs a release build of the whole workspace, except a
task whose change adds weight to a shipped binary ([a red at the size check](#a-red-at-the-size-check)).

What every agent reads at the start of a session has lines as well, in bytes, in
`crates/symphony/tests/hooks/size_lines.toml`: `RULES.md`, the root guide and the three package
guides (`apps/symphony-web`, `apps/symphony-desktop`, `crates/tasks`), each file under
`crates/symphony/shipped/skills/`, and each project page beside an installed skill
(`.agents/skills/NAME/project.md`). The hooks test beside that file
(`just test-rs -p symphony-app --test hooks size_lines`) is red on a file past its line, naming the
file, its bytes and its line, on one of those files with no line, and on a line for a file that is
none of them. A line is the file's size in the commit that sets it plus a tenth, rounded up to a
multiple of 100 bytes, so an added rule fits and a retold mechanism does not. A red asks for a cut
first, or for the mechanism the file retells to move to its document or module comment; a line is
raised only as a binary's is ([a red at the size check](#a-red-at-the-size-check)), by the
orchestrator's gate decision naming `crates/symphony/tests/hooks/size_lines.toml:KEY`, to the size
plus a tenth and never higher. KEY is the file's path with its `.md` dropped and each `/` and `.`
made `-` (`apps-symphony-web-AGENTS`), as a raise names no path, and the test reads each line as a
raise does. The file of lines is under `tests/`, a check path, so a change to it keeps `review`.

### Where a line stands

A binary's line is its size, as `just size-check` measures it in the commit that sets the line,
plus 4 %, rounded down to a multiple of 10,000 bytes. The commit of a reduction sets the lines: it
lowers the line of each binary it made smaller to that, and leaves a line where it stood when the
rule would put it higher. The 4 % is the room the landings after a reduction share until the next
one. Within its line, `symphony-build size-budget` prints where the rule brings down a binary's
line that stands higher (`its line comes down to …`), so the commit of a reduction takes that
number, not a choice of its own. A line is raised only as [a red at the size check](#a-red-at-the-size-check)
allows, by the same rule: to the binary's size plus 4 %, never higher. The client's line is set by
the same rule from what the check counts of the distribution main holds when it is set, and the
check prints no such number for it: a reduction of the newest build reaches that count in full only
once the builds before it are no longer kept.

### A red at the size check

A binary past its line is a red of the landing that took it there, not of the release that finds
it. A task whose change adds weight to a shipped binary (a dependency or a dependency's feature, a
library the binary did not reach before) runs `just size-check` at its tip and posts the sizes with
its evidence. A binary past its line there is the task's own to take back before review, in its
branch, by a reduction first: bytes of its own code, or of anything else that binary links. A
raise of its line is the exception RULES.md `workflow/release-reds-first` names for a budget, and
it is the task's orchestrator's decision, never its executor's: recorded with `just tasks gate
SLUG tools/size-budget.toml:NAME --raise NEW --why WHY --as HANDLE`
([gate decisions](TASKS.md#gate-decisions)), then made in a commit of its own that names the
growth, with the task's thread saying why no reduction will do, landed only on a fresh reviewer's
approve ([the default landing](TASKS.md#the-default-landing)) and only at the number decided, or
below the default branch's should a further cut lower it.
`NEW` is the caller's number, which the command does not check: for a size line the house form is
[the rule above](#where-a-line-stands), the binary's size as `just size-check` measures it at the
task's tip plus 4 %, rounded down to a multiple of 10,000 bytes, never higher. A first-visit cap
of the client's ([its guide](../apps/symphony-web/AGENTS.md)) is raised the same way, a decision
naming its constant (`apps/symphony-web-tests/tests/browser/main.rs:TASKS_FIRST_VISIT_BYTES`)
with the bytes measured on the fixture.

The landings that each stayed under a line can still cross it together, and the release run finds
that: a red of its size step, fixed the same way, a reduction first. To find what grew, the red's
task reads a link map of the binary at the last green tip and at the red one, by crate and by
module: every live symbol with its size and the object it came from. After `just size-check`,
touch the binary's own source file and run the recipe's build again, its packages and `--bin
NAME`, with a `RUSTC_WRAPPER` that adds `-C link-arg=-Wl,-map,FILE` to that binary's link alone:
Cargo relinks that binary and nothing else, its bytes as they were. Built with fewer packages
(`-p symphony-app` alone, or `cargo rustc` of one), the dependencies take other features and the
binary is another one. `cargo bloat` needs an unstripped rebuild of everything.

## Checks with no reviewer

`symphony.toml`'s `[verdicts] check` is what a generated branch (the owner's verdicts with the
outputs a generator makes from them) must pass before it lands with no reviewer
(`crates/symphony/src/project.rs`, `Verdicts`). It is `just symphony-cli docs-audit --check`:
such a branch changes documents and what is generated from them, and the docs audit is the check
closest to them. This project names no verdict generator, so no generated branch runs it yet; a
generator added later adds its outputs' own tests to it.

`just land` runs that check on the merge it lands unless `[land] check = false`, which this
project sets, as its delivery by pull request requires: a land here makes no merge of its own,
GitHub squashing the task's pull request once its checks pass
([a land delivered by a pull request](TASKS.md#a-land-delivered-by-a-pull-request)). The docs
audit is evidence at a document change's tip ([by kind](#evidence-by-kind-of-change)); once a
land moves main, the regenerator it starts runs it on what its round left main holding (its own
commit once landed, with whatever main gained during the build), and a red there is one task with
one owner,
`tests/main-red-check` at p0, closed by the round it passes on
([TASKS](TASKS.md#the-regenerator)). The owner's verdicts, whose commit runs the same check on
main, are refused from that land on; the regenerator shortens the time to finding out why, to
within a round of the land that brought it. The audit is part of the release run's `just verify`
too.

A land runs none of its `[land] generators` either: the regenerator it starts makes them of
main's newest tip after it ([TASKS](TASKS.md#the-regenerator)). This project's one is the
client distribution. Its key (`tools/client-key.sh`, the hash of the client's package) skips a tip
whose client inputs and committed distribution passed before; otherwise `just client-check`
builds the client fresh and compares it with the committed one, and only when they differ does
`just client-update` rebuild it and `just client-check` compare again, the change committed on a
branch of its own and delivered as the task `generated/outputs-at-TIP-COMMIT`
([TASKS](TASKS.md#what-the-generators-make-delivered-by-a-pull-request)). A branch commits no
distribution: a land refuses a head that changes one, before anything is published, and tells its
executor to put it back as main has it. Between a land and the delivery of the regenerator's task
main's committed client is behind its sources: `just tasks regenerate --status` says how far,
and the release run waits for it and releases nothing of a main still behind. The one exception
is a release candidate's fix that changes what the client is built from: no regenerator builds
the candidate, so the head its approve names commits the distribution its own sources make
(`just client-update`), which its rerun's client check proves, and the branch then merges main
once, keeping main's side of it, and delivers that second head
([RELEASES](RELEASES.md#the-release-candidate)).

## No second build in review

A task's builds and tests go through the recorder
([MACHINE_WORK](MACHINE_WORK.md#recording-builds-and-tests)), which keeps each run with the task
and the commit it ran at. The recorded runs at the task's tip are its evidence, and the thread's
post names the commands and the tip (RULES.md `workflow/no-second-build-in-review`). A stamp
stands on evidence of work already done, so nothing is built twice to land once.

**One command reads the record.** `just tasks runs SLUG` prints what the recorder kept of the
task at its branch's tip, `--tip SHA` at another commit ([TASKS](TASKS.md#needed-stamps)): each
build and test command the task ran there with its newest result, when and by whom, and the runs
of it before there; each command a runner refused there, with the refusal's line; the `tests`
stamp as its reader reads it; the project's `[verdicts] check`
(the docs audit here) with its run there, or `no run`; and how many of its runs are at other
commits, none of them the tip's. The executor reads it before it moves the task to review, and
the reviewer reads it beside the thread's evidence.

**The executor runs at the tip it asks review for.** A run made before the last commit is of
another commit: the executor runs it again at the tip, and no reviewer does it in its place. A
filter goes after `--` (`just test-rs -p PACKAGE --test TARGET -- FILTER FILTER`), as
[step 3](#picking-the-focused-set) writes it: typed without it, Cargo refuses the command line,
and the run is recorded as refused, no red and no green at the tip, so the command typed right
is still to run. A command that ran and cannot pass at the commit is a red no green run of
another command answers: only a new commit clears it
([TASKS](TASKS.md#what-clears-a-red-run)).

**The reviewer takes a green recorded run at the tip as run.** It reads the diff against the
record (`just tasks runs SLUG`) and judges whether those runs cover the change. It builds or
runs only what the record does not cover and reading cannot settle:

- a doubt it names, with the smallest target that answers it (one test by its filter, never the
  package);
- a mutation, to see that a test the task adds can fail;
- a run the executor left out, where the reviewer's own run is quicker than a round: otherwise a
  `changes` naming the command.

A run the evidence claims and no record at the tip shows is asked for in a `changes`, as is a
red one.

**The `tests` stamp reads the same record.** It is met by the newest recorded run of each build
and test command the task ran at the tip, every one green and one test run among them
([needed stamps](TASKS.md#needed-stamps)). So a reviewer's second run of a command proves
nothing the stamp did not read, and where it is recorded for the task it becomes that command's
newest run: red under load, it takes away a stamp that stood. The stamp says the runs are green,
never that they are the right runs; that is the assessor's reason where it names `tests`, and
the reviewer's judgment where the task needs `review`.

**What changes do to each other is the release run's.** A task's evidence is of its own change.
What it breaks beside another change that landed meanwhile is found on main, by
[the release run](#the-release-run) and the candidate it holds, and filed as a red with one
owner: no review builds for it.

## After main moves

A branch's evidence is of the commit it ran at, and main moves on meanwhile: what main gained can
break the branch's change, or be broken by it, in a test neither side's evidence ran. A branch
that adds a `symphony` subcommand and merges main's new test that lists the subcommands is red at
its own tip, in a 0.2 s test it never ran.

**A branch merges main once, at review, and never chases it after.** A task merges main into its
branch when its work is complete and it is about to ask for review, so the review is of the
change on main as it is (RULES.md `workflow/never-chase-main`). From then on it does not merge
main because main moved: main moves with every landing, so a branch that merges, builds and
measures finds main moved again when that ends, and would never reach its review. It lands
once its stamps stand on its tip and it merges without conflict. It merges main again in two
cases alone: its land reports a conflict, or a reviewer asks for the merge by naming what main
gained that breaks the change or that the change breaks. A reviewer does not send a branch back
only because main moved. A figure a guide quotes, such as a page's first-visit bytes or a
binary's size, is measured once, at the review tip, and stays as measured there: no check
compares a guide's figures with a fresh build, and the budgets are held by their tests.

**After merging main, before review: the executor's and the reviewer's rule.** After a branch
merges main, the closest targets of the packages the branch itself changed run again on the
branch, at the new tip, before review, as main may have changed what they meet. They are the
targets [steps 1 to 3](#picking-the-focused-set) pick at the new tip, where `main...HEAD` still
lists the branch's own files alone; a package only main's commits touched is main's evidence, not
the branch's. The evidence posted names the new tip. The reviewer approves only a tip the
evidence names, and a branch that merges main during its review, in one of those two cases, runs
them again first.

**A release candidate's fix merges `release-candidate`, never main, before its land.** A red's
own task while the release run holds a candidate ([RELEASES](RELEASES.md#the-release-candidate))
starts at the local branch `release-candidate` and merges it again before review, as that branch
moves when another red's fix lands; the same rule then holds with `release-candidate` in place of
main: its closest targets run again at the new tip, and the evidence names it. Its evidence is
the red check's failures run alone at its tip, the commands its task names. Main is merged only
when its land conflicts, after an approve, and then the approve, standing over a merge that
settles documents alone or given afresh for the merged tip
([TASKS](TASKS.md#the-default-landing)), leaves
the candidate on the commit approved before the merge; a fix that needed no review has none, so
its land then drops the candidate. One that merges main before any approve gives the candidate
nothing it can take, and drops it. Where a pull request delivers it, a fix that changes what the
client is built from merges main once before its land, from a second head after its first is
approved, keeping main's side of the generated paths, and that head is assessed, tested and
approved as any new head ([RELEASES](RELEASES.md#the-release-candidate)).

**At the land: nothing automatic, and nothing that blocks.** A land's only check on the merge is
`[verdicts] check`, which this project turns off (`[land] check = false`, `symphony.toml`), for
the regenerator to run on main's tip after it ([checks with no reviewer](#checks-with-no-reviewer)),
and it runs no test and builds nothing: a test or a build in the land would have to run again
whenever main moved before the land finished, so lands would wait on each other. An orchestrator may run by hand, before it lands a task, what it
judges the land needs, such as the closest targets of a package both the branch and main changed
since the branch last merged it, in a scratch worktree of its own
(`git worktree add --detach DIR main`, then `git -C DIR merge --no-edit TIP`); no other land
waits on it, and a red goes back to `@executor`
([the default landing](TASKS.md#the-default-landing)).

**What evidence missed, the full run finds.** [The release run](#the-release-run) runs every target
on main's tip every 20 landings, and each day or more often once the owner schedules it, and
publishes a release when every check is green; each red it finds is one task, with one owner.

- [-] **land-tests-changed-packages** — `just land` checks a merge with the docs audit alone and runs no test, so a red that a branch's evidence missed reaches main. Three did tonight, each already red at the approved tip: a route change whose `after_http` nobody ran; a branch that added a `symphony` command and merged main's new test that lists them, and never ran that 0.2 s test; and a board change whose `polish::` and `shot::` browser tests nobody ran. Should the land's check also run, on the merge, the fast test targets of the packages the branch changed? A: no; the executor and the reviewer run them at the tip, and the lander reruns by hand what main gained since, as CHECKS now says. B: yes, for each package that both the branch and main's newer commits changed. C: yes, for every package the branch changed, on every land. Recommended: C. Approve and say A, B or C in the note, or comment. [[CHECKS#After main moves]]
  [2026-11-12 06:06] agent — Posted for the board task `tools/checks-name-the-tests-landings-missed`.

    **What the land would have caught under each**, of tonight's three reds:

    | red at the tip | A | B | C |
    |---|---|---|---|
    | a route change: `-p symphony-server --test after_http` | no | yes, as main had changed `symphony-server` too | yes |
    | a new command: `-p symphony --test serve` | no | no: main had changed none of the branch's packages since its tip | yes |
    | a board change: its `polish::` and `shot::` browser tests | no | no | no |

    None of the three was made by the land's merge, so B's own case, two lines of work green alone and red together, did not occur. The tests CHECKS now names for each kind of change (a route's targets, the rule for a new command, the tasks board's row), run at the tip, catch all three: A rests on that alone, and B and C are the land's net under it.

    **A land today** rebuilds the client in its own worktree to compare it with the committed one, 7 to 20 minutes (median 11, over the 17 client checks lands ran tonight), builds `symphony` for the docs audit, and runs no test.

    **What a package's fast targets take**, cargo's own run times from tonight's three nightly runs, build not included, a fast target being one that took under 2.5 minutes in every run:

    | package | its fast targets | run |
    |---|---|---|
    | `symphony` | all but `nightly` | 3 to 5 min |
    | `symphony-board` | the library, `child_guard`, `cli`, `launch`, `store` | 4 to 6 min |
    | `symphony-server` | the library and the `after_*` targets | 3 to 3.5 min |
    | `symphony-ui` | all but `browser` and `wasm_build` | 10 to 30 s |
    | each other crate | all | seconds |

    First the land's worktree builds those test binaries: the whole workspace's took 4 and 5 minutes on two nights and 18 on a loaded one, and a land's packages are part of that.

    **Not fast, so in neither:** the browser target first (171 s one night, 61 minutes on another when its tests timed out), the board's `perf`, `sweep` and `server` (2.5 to 15 minutes each; `perf` took 883 s in one run) and `launcher` (seconds, and 10 minutes once), the server's `standalone` (3 to 5), the client's `wasm_build` (1 to 3) and `symphony`'s `nightly` (nearly 3). They stay the nightly's: under load their timeouts would turn lands red that changed nothing they test. Where the line falls is the recipe's to set, not this card's.

    **What B and C cost over tonight's lands**, each land's packages weighed by those run times (the middle of each range), the client distribution counted as `symphony-server` as CHECKS does:

    | lands since | lands | B runs tests on | for, in all | C runs tests on | for, in all |
    |---|---|---|---|---|---|
    | 18:00 on 11-11 | 47 | 31 | 2.8 hours | 41 | 4.4 hours |
    | 00:00 on 11-11 | 64 | 39 | 3.6 hours | 55 | 6 hours |

    Main's newer commits mostly touch the same few packages a branch does, so B runs on two lands in three and costs about two thirds of what C does. A land that runs either adds 5 to 6.5 minutes on average and up to 13 (a branch through `symphony`, the board and the server), beside the client check's 11.

    **A, by hand.** Nothing added to a land. Cost: a target someone leaves out reaches main red and stays there until a full run finds it.

    **B, shared packages.** Catches one of the three, because main happened to have changed the same package. Cost: about two thirds of C's.

    **C, every changed package.** Catches two of the three whatever main did meanwhile, and B's case as well, since every package both changed is one the branch changed. Cost: the minutes above, on 41 of the 47 lands.

    **Why C.** The reds that reached main tonight were evidence that left a target out, which C catches on every land and B only where main moved in the same package; B costs two thirds of C and caught half as much. A adds nothing to a land, but rests on every executor and reviewer running what CHECKS names, which three branches tonight did not. Built as one recipe the check names, which picks the packages from the branch's diff: the mechanical half of `just check-fast`, filed as `tools/check-fast`.
  [2026-11-12 06:42] owner · rejected — Adding 5 minutes may seem low, but the issue is that it becomes a “blocking 5 mins”. Every new PR that gets merged could invalidate the 5 min run, making it have to run it again and again and again, spiraling the 5 minute forever. A solution like a lock or queue would directly bottleneck the pipeline which isn’t acceptable either. So a nightly or more often run that then runs all tests and publishes when green is the only acceptable choice I can think of. Or the orchestrators could do a manual run of what they think is needed to ensure quality. But this cannot be an automated approach or that would wait and spiral OR add a number of tickets bottleneck to be merged and both are not acceptable.
  [2026-11-12 06:44] agent — Done as you say, with no replacement card: the land's check stays the docs audit and runs no test, and nothing automatic waits on a land. Quality rests on three things: the evidence at the tip that CHECKS names for each kind of change, run again at the new tip after a branch merges main, for the packages the branch itself changed; an orchestrator's run by hand of what it judges a land needs, which nothing waits on; and the full run (`just nightly`), nightly or more often, which runs every test and publishes a release when green, each red it finds a task with one owner. CHECKS and RELEASES now say so. [[CHECKS#After main moves]]