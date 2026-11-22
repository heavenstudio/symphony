# crates/tasks/

The `symphony-board` crate (this folder) is the task board: its store over SurrealDB, the database
server it starts in `symphony/tasks/` in the git common dir, and the binaries `tasks`
(`just tasks`), `agent-send` (`just agent-send`), `tasks-copy`, `tasks-reclaim` and `tasks-move`.
The workspace, the Cargo wrapper and the toolchain pin are
[the root guide's](../../AGENTS.md#rust).

What the board does is [TASKS](../../docs/TASKS.md), and how a module does it is that module's
own doc: a behaviour a change makes is said there, and this guide changes only when a job moves
or an invariant does.

## Where each job lives

Paths are under `src/` unless they say otherwise, and each folder's `mod.rs` or same-named file
names its parts. The test
targets are the folders of `tests/` with a `main.rs` that says what each holds (`store`, `cli`,
`server`, `sweep`, `launch`, `perf`, `hosted`, `child_guard`) and the files `consumer_api.rs`,
`launcher.rs`, `links.rs` and `raw_fence.rs`.

- **The command line**: `bin/tasks.rs` over `cli/` (each command in `cli/commands/` or a file of
  its own, its help in `cli/usage.rs`). `--json` prints `serde_json::Value`, its records' fields
  in `cli/shape.rs`. `cli/api.rs` is `tasks api`, `cli/agents.rs` the agents API, `cli/beat.rs`
  the heartbeat hook, `cli/attach/` the monitor and its event lines, `cli/pull.rs` the PULL
  digest. `bin/tasks-move.rs` is a program apart, so `tasks` links no move.
- **The recorder and the runs**: `cli/recorder.rs` (`tasks run`), `runs.rs`, `runs/`,
  `run_logs.rs`, `recording.rs`, `cli/machine_work/`
  ([MACHINE_WORK](../../docs/MACHINE_WORK.md), [RUNS](../../docs/RUNS.md)).
- **Tokens and their cost**: `usage.rs`, `usage/`, `store/spending.rs` (`agent_beat_in` and
  `Beat` stay as consumers call them). An edit of the price table
  `usage/prices.toml` is read as the builds read it by
  `just test-rs -p symphony-board --lib usage::price`.
- **The store**: `store/` is `Store` (`work/` the task writes, `messages.rs`, `asks.rs`,
  `sessions.rs`, `launches.rs`, `cap.rs`, `writers.rs`, `recovery.rs`, `copy.rs`, `opening.rs`,
  `transaction.rs`); `lib.rs`'s doc states the rule of every write. Its statements are
  `store/surreal/` and `store/postgres/` ([below](#the-stores-statements-live-in-one-place)).
  `schema.rs` is the tables, the indexes, the version and its steps; `live.rs` which builds may
  move a live board; `rpc.rs`, `cbor.rs` and `pg.rs` the clients.
- **Reaching a board**: `project.rs`, `git.rs`, `folders.rs` (the state folders, named once),
  `builds.rs`, `confine.rs` (where an unasked recipe's path may lead), `launch/bounds.rs`.
- **Callbacks, delivery and PULL**: `store/callbacks.rs`, `store/dispatcher.rs`,
  `store/callback_state.rs`, `store/pull_account.rs`, `store/queued_lines.rs`,
  `store/codex_delivery.rs`; `store/unanswered.rs` is the one definition of an unanswered post.
  Who hears what: [TASKS](../../docs/TASKS.md#sessions).
- **Terminals and staffing**: `launch/` drives hc's terminals and the Herdr app through
  `launch::hc::Hc` alone (the crate calls no `herdr`); `bin/fake-hc.rs` stands in for both in
  tests.
- **Worktrees, landing and what follows a move**: `sweep.rs` and `sweep/`, `on_review.rs`,
  `on_land.rs`, `cli/commands/finish.rs`; `land.rs` and `land/`
  ([the default landing](../../docs/TASKS.md#the-default-landing)), `land/adapter.rs` and
  `workflow/effect.rs` ([WORKFLOW](../../docs/WORKFLOW.md#external-effects)), `land/delivery.rs`
  and `land/evidence.rs` ([a land delivered by a pull
  request](../../docs/TASKS.md#a-land-delivered-by-a-pull-request)), `covering.rs`, `stamps.rs`
  and `stamps/` ([needed stamps](../../docs/TASKS.md#needed-stamps)), `gates.rs`
  ([gate decisions](../../docs/TASKS.md#gate-decisions)), `red.rs`.
- **Execution attempts**: `execution.rs` and `execution/`, `store/execution.rs`
  ([EXECUTION_ATTEMPTS](../../docs/EXECUTION_ATTEMPTS.md)).
- **The rules of work**: `slots.rs` ([slots](../../docs/TASKS.md#slots)), `orchestration.rs`
  with `store/work/given.rs`
  ([the limit](../../docs/TASKS.md#the-limit-on-the-tasks-an-orchestrator-holds)), `limits.rs`,
  `local.rs` (the one reader of the local settings file, and the one judge of what that file and
  the board's store may hold), `settings.rs` with `store/stored_settings.rs` and
  `cli/commands/settings.rs` (the board's store of preferences, read between the project file and
  the local file: [PROJECT_FILE](../../docs/PROJECT_FILE.md#the-boards-store)), `alarm.rs`
  ([the pipeline alarm](../../docs/TASKS.md#the-pipeline-alarm)), `health.rs`, `stages.rs`,
  `arm.rs`, `reports.rs`.
- **The database server**: `server.rs` is `server::Home`, with its parts in `server/`:
  `server/postgres.rs` and `server/postgres/lifecycle.rs` the board's PostgreSQL beside it
  ([POSTGRESQL](../../docs/POSTGRESQL.md)), `server/moving.rs` a board's move between the engines;
  `board_health.rs` the query log. The owner's side is TASKS's
  [stuck servers](../../docs/TASKS.md#stuck-servers) and
  [a board that went back](../../docs/TASKS.md#a-board-that-went-back).
- **A hosted board**: `pg/external.rs` its settings, which every build reads, and `pg/hosted.rs`
  with `pg/hosted/` its client, compiled only with the `hosted-postgresql` feature
  ([a hosted board](../../docs/POSTGRESQL.md#a-hosted-board)).
- **Search**: `store/search.rs`, `cli/search.rs`, `store/postgres/search.rs`: by words alone, the
  same on either engine (`tests/store/search.rs`).
- **Children**: `sys.rs` and `sys/`, over the leaf `crates/process-cwd`.

## Invariants a change keeps

Each is stated in full where it points.

### No branch's build moves the live board

The live board is one database many builds open at once. Only a build the default branch says is
its own moves it forward, a process Cargo runs is refused it, and a board binary run from a linked
worktree runs main's build there (`live.rs`; [TASKS](../../docs/TASKS.md#where-it-lives);
`tests/cli/main_only.rs`). A new way into the board (a binary, a command, an API) goes through
`cli::writable_board` and `Reached::opening`; `cli::board_client` is kept, with its signature, for
consumers' code alone. A branch's build run by its path (`.target/debug/tasks`,
`tools/rs-bin.sh --build`) skips the launcher's rule: run it only on a scratch board.

Every build of `tasks`, this crate's and a consuming project's board-only one, is
`cli::process_main` and nothing else: a `main` that calls `cli::main` starts no dispatcher
(`tests/server/dispatch.rs`).

### The schema moves forward, and older builds keep reading

The board is a contract ([CHECKS](../../docs/CHECKS.md#the-board-is-a-contract)):
`schema::VERSION` counts breaking steps alone, and a step that only adds is an additive step
(`schema::ADDED`), which leaves the version as it is. Either is migrated once, in one transaction,
by a build that may move the board. Every write transaction starts with the schema guard, and a
reader one version behind still reads (`schema.rs`, `store/opening.rs`, `store/transaction.rs`;
`tests/store/schema.rs`, `open_read.rs`, `contract.rs`). So an older build keeps running against
a newer board, and a change keeps what it reads:

- A new table, or a new field on one, is a step's; a closed table changes only by a breaking
  step. The open tables (`schema::OPEN`) are written by named fields, never replaced whole
  (`store/surreal/fence_tests.rs`). Each row struct refuses a field it does not know, and the
  store's reader hands an open table's row only the fields it names (`rows::open_row`,
  `objects::read`), so an older build reads past a field an additive step adds; the whole-board
  dump alone reads exactly.
- No task field an older reader would refuse: the board's own fields go under `fields._board`,
  written only by their own writers, and a hold is a record, never a field.
- A new meaning goes in a field older binaries ignore and keep, beside the old one, which keeps
  its meaning (an orchestrator's `seen`, `statuses` and PULL entries), and an old column keeps
  its old reading (a launch record's `claude_session` holds Claude ids as before, and Codex ids
  prefixed `codex:`). Callback addresses, chat ownership,
  delivery state and bindings live in the schemaless callback tables, not on `session`.
- With an older dispatcher interleaving, no entry an orchestrator hears is lost and no line is
  said twice (`store/pull_account.rs`; `tests/store/mixed_dispatchers.rs`,
  `pull_acknowledgements.rs`, `pending_pull.rs`); a new kind of line that names a task takes its
  test of whom to tell from `store/queued_lines.rs`.
- An in-process binding never registers a callback or takes over a handle with a native
  registration.
- A counter an older reader caches is retired where it refuses it, never removed
  (`schema::BOARD_MARKER`). A table nothing writes any more keeps its definition, as removing one
  is a schema move (the search-by-meaning tables,
  [their card](../../docs/analysis/code-review-2026-11.md#board-search-by-meaning)).
- The `writer` table stays schemaless with no event, its line written without the schema guard,
  and the Symphony, which opens its board to read, stays uncounted (`store/writers.rs`;
  `tests/store/writers.rs`).
- Each schema's definitions are committed, one file per engine
  (`tests/store/catalogue/ENGINE-VERSION.txt` and `.STEPS.txt`, written with
  `TASKS_CATALOGUE=write`), and `tests/store/catalogue.rs` holds a step to what it may add. Which
  store may take a board one ahead: `store/transaction.rs`'s `Run`,
  [CHECKS](../../docs/CHECKS.md#a-schema-step-that-keeps-the-symphony-writing).
- A table added to the board is placed as copied, carried or left in `store/copy.rs`, whose test
  fails on one that is none, since a move would drop it.

### Its contracts

`tasks api` is a contract ([TASKS](../../docs/TASKS.md#the-tasks-api)): within a major a method,
an argument or an answer's field is added, never removed, renamed or changed in meaning, an
argument added has a default, and each addition raises `MINOR`. `tests/cli/api/contract.rs` holds
the calls a registered project's programs make; a call such a program starts making is added
there. The Rust names consumers call are held by `tests/consumer_api.rs` and change only by the
[two-release rule](../../docs/CHECKS.md#consumer-api-changes), a rule apart from the `tasks api`
version.

### A write decides on what it read

A rule reads what it decides on, decides in the process, and writes in one transaction fenced on
what it read; the transactions are optimistic and conflict only on rows both write, so two writes
that must not pass each other write one row (`lib.rs`; `slots.rs`, `store/cap.rs`,
`store/launches.rs`; `tests/store/slots/`). A write that keeps only what a reader derived
(`Store::record_fixed`) moves no task version. A plain `Store::create` gives no orchestrator: a
new way to file goes through `Store::give_filed`.

### A copy replaces the recovery generation

Whatever replaces a board's rows whole replaces `recovery:board` in the same transaction, and
whatever puts a database's files in place revokes the home's bindings first
(`Home::revoke_recoveries`). A write bound to a token starts with the fence, before anything it
writes, and refuses a mismatch rather than read again; a store finds no authority by itself
(`store/recovery.rs`; `tests/store/recovery.rs`).

### The store's statements live in one place

The store's modules hold the rules; a statement's text is `store/surreal/`'s, under a name the
store's module asks for whole, and its PostgreSQL twin `store/postgres/`'s by the same name: a
call names both (`store/statement.rs`'s `both`), and one with no twin is refused on a PostgreSQL
board before anything is sent. No SurrealQL is written anywhere else but in the files
`store/surreal/fence_tests.rs` lists with their reasons. Nothing reaches a client but through
`store/transaction.rs`, `store/raw.rs` and `store/database.rs`. A test that needs a row no command
writes asks `Store::raw` by table, id and equal fields and writes no statement of its own; one
that must is in the list `tests/raw_fence.rs` holds, which `just lint-rs` runs. A store test
written once for both databases is `on_both!`'s (`tests/store/main.rs`).

### Delivery never loses a line

A write is never sent to the database again; only a request whose text proves it a read goes once
more (`rpc/read.rs`). A dispatcher whose lease was taken while it built sends nothing
(`store/dispatcher.rs`; `tests/store/slow_builds.rs`); a recipient's observation is checkpointed
with the events it queued, before anything is sent; a failed send is a retry, so a line may repeat
but is never lost; a post under a key is made once (`Store::say_keyed`). Every event line is
flattened before it enters the envelope, but owner chat's payload, which keeps its reply lines
(`store/callbacks.rs`). The UDS transport sends
only the SDK user envelope: no authentication token, and no claim to be the receiver's child.

### A hosted board's refusals carry no secret

A change under `pg/hosted/` keeps four things: no refusal carries a driver's or a server's own
words (they can hold the URL, a name or a statement), only a kind, a fixed reason, a SQLSTATE and
where the server's log has the rest; nothing is sent a second time; a connection goes back to the
pool only when its request left it clean; and the store reaches a hosted board by its private
dispatch (`store/database.rs`'s `Board`), the public `Database` and `pg::Client` as they are. A
change to the analyzer's definition is proved on PostgreSQL 16 and 18 by
`tests/hosted/analyzer.rs`. The feature's tests are `just hosted-check`, which no `just test-rs`
of the default build runs.

### Children start through `sys`

Every child is built by `sys::command` or `sys::command_with`, without the starter's session or
marks, and its output read by the `sys` helpers, never `Command::output` or `Stdio::piped`, here
and in the sources of `symphony` and `symphony-server` (`sys.rs` says why). A call that must keep
a name sets it on the command after, saying why. A git is `sys::git()`. Clippy holds it (the
crate's `clippy.toml`), and the release run's `tests/child_guard` tries the rule.

### The database server is never killed

Only `server::Home` starts the board's server, so every one runs under `server::Storage`. A stop
sends one SIGTERM, once the server answered, and waits: never a second, which SurrealDB takes as
an immediate shutdown, and never a signal to a server that has not answered, which is replaying;
that call is the owner's. Every start, stop, hold, release and reclaim takes the starters' lock.
Never set `SURREAL_SURREALKV_ENABLE_VLOG=false`: a store with values in its log then cannot read
them (`server.rs` and `server/`; [TASKS](../../docs/TASKS.md#stuck-servers)).

### A terminal closes only on information

A close never takes the caller's terminal, an orchestrator's or a chat's, nor one the owner may be
looking at, and reads the window last, right before the kill. A terminal whose `hc get`, process
table or window cannot be read stays. Presence is judged the same way: only the sweep confirms it,
never a delivery (`launch/close.rs`, `launch/cleanup.rs`, `launch/reap.rs`, `store/callbacks.rs`;
`tests/launch/`).

### Work finishing runs main's build

The sweeps work finishing starts run the main checkout's build of `tasks` (`builds::of_main`),
never the caller's (`sweep/trigger.rs`); `on_review` and `on_land` run the project's command
detached from the main checkout. A board `TASKS_SERVER` names starts no sweep, `on_review`,
`on_land` or regenerator unless its variable says `on` ([below](#the-command-lines-tests)).

### The land and the verdict share one rule

`tasks land` runs only under the `land` recipe's `SYMPHONY_LAND` (`land::admit`), a guard against
accident. The approve that counts is one rule, `Held::unfresh`, which `Store::verdict`, the land,
the assessor's `needs` and staffing's reviewer pick share, and the land goes by the stamps through
`land/needs.rs` over the readers of `stamps/read.rs`, which the task page draws too, never a copy
(`land.rs`'s doc).

### Queries keep their indexes

What a write, each recipient of its dispatch and each tick read goes through an index, never a
scan of message, delivery, role, session or callback_state (`schema.rs`'s doc lists them). The
`perf` target plans every such statement: a new statement or a changed condition on those paths
runs it ([below](#at-scale)). A transaction over many rows avoids what costs more than linearly on
the engine: an index a copy inserts into, an event that UPSERTs. On PostgreSQL a new read of a
list that grows with the board is written as `Pg::Listed` (`store/postgres/mod.rs`), a row an
item, not one `jsonb_agg` value.

### Time in tests

A product deadline a test would trip on a loaded machine is a setting read only when set, never
raised for anyone else (`Env::seconds`: `TASKS_RECORDER_PROBE_WAIT`, `TASKS_ON_LAND_SAYS`,
`TASKS_HC_ENDING`, `TASKS_CODEX_WAIT`, `TASKS_UNANSWERED_WAIT`; `TASKS_RUN_WAIT_SCALE` on a
scratch board alone). A test of the deadline sets it short; every other test gives it a backstop.
The rule is RULES.md `tests/no-test-fails-on-load`; the helpers `crates/test-time`
([CHECKS](../../docs/CHECKS.md#time-in-tests)).

## Testing against a scratch board

No test, example or `cargo run` reaches the live board. A test's board is a database of its own on
a server of the test binary's, or a fixture repository's in a temporary folder. By hand, a
branch's code runs on a scratch database of the live server, `just tasks --database NAME …` or
`just agent-send --database NAME HANDLE TEXT --as SENDER`, which runs the branch's build, moves
that database forward and never sweeps a repository.

### The servers

The store's tests start one in-memory server per test binary and give each test a database of its
own (`tests/store/common.rs`). The server is the pinned SurrealDB `tools/surreal.sh` fetches once
per repository; `SYMPHONY_SURREAL` names another binary. A fixture board under the temporary
folder or a checkout's `artifacts/` is a scratch place: its server ends with the process that
started it, and a fixture repository there is its own project. The PostgreSQL tests
(`tests/server/postgres.rs`) and the hosted board's (`tests/hosted/`, `just hosted-check`) need
what [the root guide](../../AGENTS.md#what-it-needs-installed) lists.

### The command line's tests

The crate's tests run their own `tasks` binary; `SYMPHONY_TASKS_CLI` injects it into the
`symphony` crate's board clients. They reach the test's board with `TASKS_SERVER` and
`TASKS_DATABASE`, and steer the rest with:

- `TASKS_NOW`, the clock, and `TASKS_PULL_SECONDS`, how often a monitor computes its PULL;
- `TASKS_DISPATCH=inline`, the write's delivery made in the write;
- `TASKS_REAP_STATE`, where the idle sweep keeps what it saw;
- `HC_BIN` and `HERDR_APP_BIN`, a fake hc and window (`fake-hc`, steered by `FAKE_HC_STATE`,
  `FAKE_HC_LOG` and `FAKE_HC_FAIL`); `just test-rs` gives a test that names none `/usr/bin/false`,
  so no case reads this machine's terminals;
- `TASKS_PS_TABLE`, a file read in place of `ps`;
- `TASKS_SWEEP=on`, `TASKS_ON_REVIEW=on`, `TASKS_ON_LAND=on` and `TASKS_REGENERATE=on`, without
  which a board `TASKS_SERVER` names starts none of them, and `TASKS_BIN`, the build the triggers
  spawn there.

A test of a command that reads a mark (`SYMPHONY_UNASKED`, `SYMPHONY_LAND`) runs it in an
environment of its own (`symphony::cli::main_in`, the board's `Env`), never this process's, so a
test run under a recipe or a land is not red or green by what it inherits.

### At scale

The `perf` target is the board at the live one's scale: the plan check of every statement a
per-write, per-recipient or per-tick path sends, and each command's CPU within a bound
(`tests/perf/main.rs`). Only the release run runs it, so a change to the board's queries, its
indexes or the api runs it by hand: `just test-rs -p symphony-board --test perf`
(`SYMPHONY_PERF_TASKS=PATH` times another `tasks` build on the same board).
`tests/child_guard` is the release run's alone. `examples/board_load.rs` is the store's meter: a
change of the store, its settings or its engine runs it on both sides the same day, against
[the store's baseline](../../docs/analysis/board-store-baseline.md). An example's tests run only
when named: `just test-rs -p symphony-board --example board_load`. Which tests a change runs:
[CHECKS](../../docs/CHECKS.md#picking-the-focused-set).
