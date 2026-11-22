# crates/tasks/

The `symphony-board` crate (this folder) is the task board: its store over SurrealDB, the database
server it starts in `symphony/tasks/` in the git common dir, and the binaries `tasks`
(`just tasks`), `agent-send` (`just agent-send`), `tasks-copy`, `tasks-reclaim` and `tasks-move`. The workspace,
the Cargo wrapper and the toolchain pin are [the root guide's](../../AGENTS.md#rust).

This guide is what to know before changing the crate: where each job lives, the invariants a change
keeps, and how to test against a scratch board. What the board does is [TASKS](../../docs/TASKS.md);
how a module does it is that module's own doc. A behaviour a change makes is said in the TASKS
section that owns it or in its module's doc, not here: this guide changes when a job moves or an
invariant does.

## Where each job lives

Paths are under `src/` unless they say otherwise. The test targets are the folders of `tests/` with
a `main.rs` that says what each holds (`store`, `cli`, `server`, `sweep`, `launch`, `perf` and
`child_guard`) and the files `consumer_api.rs`, `launcher.rs` (its modules in `launcher/`) and
`links.rs`.

### The command line

`bin/tasks.rs` is the command line `just tasks` runs (`cli/mod.rs`, each command in `cli/commands/`
or a file of its own in `cli/`, its help in `cli/usage.rs`; `cli/args.rs` reads the arguments as the
board's first, Python, command line did). Every build of `tasks`, this crate's and a consuming
project's board-only one, is `cli::process_main` and nothing else: a `main` of its own that calls
`cli::main` delivers in every write and starts no dispatcher (`tests/server/dispatch.rs`).
`bin/agent-send.rs` is `cli/agent_send.rs`; `bin/tasks-copy.rs` and `bin/tasks-reclaim.rs` say their
own, and `bin/tasks-move.rs` is `move_cli.rs`: a program apart, so `tasks` links no move. `cli/api.rs` is `tasks api` ([TASKS](../../docs/TASKS.md#the-tasks-api)), `cli/agents.rs` the
agents API ([TASKS](../../docs/TASKS.md#the-agents-api)), `cli/beat.rs` the heartbeat hook,
`usage.rs` and `usage/transcript.rs` what its beats read of the session's own transcript, kept
beside the bound keys and never in the database
([TASKS](../../docs/TASKS.md#where-the-tokens-go)), `usage/price.rs` what those tokens cost at the
price table `usage/prices.toml` every build embeds, whose edit
`just test-rs -p symphony-board --lib usage::price` reads as the builds do
([TASKS](../../docs/TASKS.md#what-the-tokens-cost)),
`cli/attach/` the monitor, `cli/pull.rs` the PULL digest, `cli/commands/wait.rs` `tasks wait`,
`cli/detached.rs` the detached dispatcher a write starts, and `cli/database.rs`, `cli/snapshot.rs`,
`cli/schema.rs` and `cli/migrate.rs` the commands of the same names. `--json` prints `serde_json::Value`, its records' fields in `cli/shape.rs`, from `rows.rs`.

### The recorder and the runs

`cli/recorder.rs` is the recorder, `tasks run`, which `tools/record.sh` runs
([MACHINE_WORK](../../docs/MACHINE_WORK.md#recording-builds-and-tests)); `runs.rs`, `runs/` and
`run_logs.rs` the runs' day files, roll-up and logs ([RUNS](../../docs/RUNS.md)); `recording.rs`
when a server generation's recorders record; `cli/machine_work.rs` and `cli/machine_work/` the build
and test work the agents view reports ([MACHINE_WORK](../../docs/MACHINE_WORK.md)).

### The store, its schema and its client

`store/` is `Store`: `work/` the task writes (making, moving, roles, needs, red marks, the
orchestrator the board gives, covered closes), `messages.rs` threads, inboxes and keyed posts,
`asks.rs` the owner's asks ([TASKS](../../docs/TASKS.md#asks)), `sessions.rs` sessions and beats,
`spending.rs` the tokens a beat adds to the `usage` table in its own transaction
(`Store::agent_beat_spending`; `agent_beat_in` and `Beat` stay as consumers call them),
`launches.rs` launch records and the session hold, `cap.rs` the slot cap's reads, `history.rs` the
status lines the alarm replays, `stamp.rs` what tells a task and its thread from the ones a reader
read before, in one small read, `copy.rs` the whole-board copy, `opening.rs` how a store opens by
the schema rule, `transaction.rs` the schema guard every write starts with and the one decoder
of its answers, which reads each struct from an object alone (the crate's `objects.rs`, the one
reader of the JSON the board holds as a value and of the text it drops when it cannot read it; a
read whose refusal a caller is shown stays `serde_json`'s), `writers.rs` the line each program that opens a board to write
keeps, which `tasks migrate` reads before it moves one
([TASKS](../../docs/TASKS.md#who-writes-a-board)), `recovery.rs` the recovery generation, the
token and the writes bound to it and the authority a store is handed, with `server/recovery.rs`
the board home's file of it and `cli/recovery.rs` `tasks recovery`
([TASKS](../../docs/TASKS.md#a-board-that-was-copied-or-restored)). `lib.rs`'s doc states
the rule of every write. `store/surreal/` is every statement the store sends, a module beside each
store module, `store/postgres/` its PostgreSQL twin of each one written so far and the board's
tables there, `store/statement.rs` the pair a call names, `store/database.rs` the database a board
is in, and `store/raw.rs` (`Store::raw`) a board's rows as a test sets and reads them
([the seam](#the-stores-statements-live-in-one-place)). `pg.rs` is the PostgreSQL client. `schema.rs` is the tables, the indexes, the version and its migration
steps; `live.rs` which builds may move a project's live board, and `live/seen.rs` the highest schema
it was seen at. `rpc.rs` (with `rpc/read.rs`) and `cbor.rs` are the client: SurrealDB's RPC in CBOR
over plain HTTP.

### Reaching a board

`project.rs` names a run's project, and so its board. `git.rs` finds the common dir, `folders.rs`
names the state folders in it, once, `builds.rs` finds a project's build of a binary (the recipes',
the hooks' and the triggers' rule). `confine.rs` is where a path an unasked recipe is handed may
lead ([the root guide](../../AGENTS.md#agent-integration)), and `launch/bounds.rs` where an agent
the board starts for one may run. `tools/rs-bin.sh` runs a board binary from a linked worktree on
the live board as the main checkout's build, of what that checkout holds, committed or not, in its
target folder; its comments and `tests/launcher.rs` say which runs count as the live board's.
A reach whose project's folder or repository is gone never walks up to the board above it, and no
start makes a board's folders at or above its common dir again (`project::held`, `Home::make`;
[TASKS](../../docs/TASKS.md#board-health)).

### Callbacks, delivery and PULL

`store/callbacks.rs` (and `callbacks/visit.rs`) is the native channels and the durable outbox: every
write observes the board for each recipient and queues what it hears. `store/dispatcher.rs` is the
one dispatcher a host runs at a time, `store/pool.rs` the messages a pass reads,
`store/callback_state.rs` a recipient's observation, `store/pull_account.rs` what a PULL stands for,
`store/queued_lines.rs` queued lines judged again at delivery, `store/callback_peer.rs` a socket's
listener identity, `store/codex_delivery.rs` and `codex_queue.rs` Codex's queue. `store/snapshot.rs`
is the board snapshot a store caches with the board's marker. `cli/attach/events.rs` makes the event
lines, `cli/attach/panes.rs` the pane watch. `store/unanswered.rs` is the one definition of an
unanswered post (PULL, recovery and the reap read it). Who hears what:
[TASKS](../../docs/TASKS.md#sessions).

### Terminals and staffing

`launch/` drives hc's terminals and the Herdr app through `launch::hc::Hc` alone (the crate calls no
`herdr`): `startup.rs` a launch's start, `restore.rs`, `close.rs` and `cleanup.rs`, `screen.rs` what
the owner may be looking at, `procs.rs` what under a terminal is work, `place.rs` its tab, `reap.rs`
and `reap/` the idle sweep the Symphony runs every five minutes, `recover.rs` the resume of a
conversation an unanswered post waits on, `staff.rs` a staffing pass, `defaults.rs` the agent and
model a launch starts, `running.rs` the machine's agents. `bin/fake-hc.rs` stands in for hc and the
app in tests.

### Worktrees, landing and what follows a move

`sweep.rs` and `sweep/` are the worktree sweep: what a finished worktree sets aside (`aside.rs`),
the build output it clears (`build_output.rs`), what the main checkout keeps (`kept.rs`), a lock's
holder (`holder.rs`), the proof of a head a pull request squashed (`delivered.rs`), the detached
sweeps work finishing starts (`trigger.rs`). The lsof scan is the leaf `crates/process-cwd/`.
`on_review.rs` and `on_land.rs` start `[board] on_review` and `[board] on_land`;
`cli/commands/finish.rs` is what follows a move. `land.rs` and `land/` are
`tasks land`, the default landing ([TASKS](../../docs/TASKS.md#the-default-landing)): `needs.rs`
what it requires, `check_paths.rs` which tests and checks a branch changes, `moving.rs` how it moves
the main checkout, `regenerate.rs` the regenerator ([TASKS](../../docs/TASKS.md#the-regenerator)).
`adapter.rs` is the land of a task a custom workflow holds, the adapter `default_land` on the
workflow's external-effect boundary (`workflow/effect.rs` its vocabulary and decisions,
`store/admission/effects.rs` its three writes, [WORKFLOW](../../docs/WORKFLOW.md#external-effects)).
`delivery.rs` is a land delivered by a hosted pull request and `evidence.rs` the one reading of a
task's evidence it and `tasks api` go by
([TASKS](../../docs/TASKS.md#a-land-delivered-by-a-pull-request)).
`covering.rs` with `store/work/covered.rs` is a close covered by another landing
([TASKS](../../docs/TASKS.md#a-task-another-landing-covers)). `stamps.rs` and `stamps/read.rs` are
the stamps' catalogue and readers, `stamps/posted.rs` the cards a branch posted that the `owner`
stamp's reader reads ([TASKS](../../docs/TASKS.md#needed-stamps)), `gates.rs` with
`store/work/gates.rs` and `cli/commands/gate.rs` a task's gate decisions, which `land/needs.rs`
reads ([TASKS](../../docs/TASKS.md#gate-decisions)), `red.rs` the red mark.

### Execution attempts

`execution.rs` and `execution/` are the attempts a project's execution provider runs
([EXECUTION_ATTEMPTS](../../docs/EXECUTION_ATTEMPTS.md)): `coordinator.rs` admission, controls and
the worker's step, `rules.rs` the transitions it decides on what a write read, `evidence.rs`
collection, cleanup and eligibility, `record.rs` the row and its facts, `keys.rs` the ids and
operation keys, `reader.rs` the reads that need no provider, `view.rs` the board scope
(`board_scope`) and an attempt on the wire; `store/execution.rs` with its statements is their
store.

### The rules of work

`slots.rs` with `store/cap.rs` is the cap on an orchestrator's open work
([TASKS](../../docs/TASKS.md#slots)); `orchestration.rs` with `store/work/given.rs` one orchestrator
per task, and the limit on the open tasks one holds
([TASKS](../../docs/TASKS.md#the-limit-on-the-tasks-an-orchestrator-holds)); `limits.rs` the project's `[limits]` and the alarm's thresholds, read again when the file
changes ([TASKS](../../docs/TASKS.md#limits-and-models)); `local.rs` the one reader of the local settings
file over them and over a launch's agent and models
([PROJECT_FILE](../../docs/PROJECT_FILE.md#the-local-settings-file)), and the one judge of what
that file and the board's store may hold; `settings.rs` with `store/stored_settings.rs` and
`cli/commands/settings.rs` the board's store of preferences, read between the project file and
the local file ([PROJECT_FILE](../../docs/PROJECT_FILE.md#the-boards-store)); `alarm.rs` and `alarm/` the pipeline
alarm, `cli/pull/pipeline.rs` as PULL says it ([TASKS](../../docs/TASKS.md#the-pipeline-alarm));
`store/work/moves.rs` a move that names where it goes from; `health.rs` a session's health, derived
and never stored; `stages.rs` where a task's time went
([TASKS](../../docs/TASKS.md#where-the-time-goes)), `arm.rs` the arm a task is filed on, which
`stats` groups by; `reports.rs` the board's part of the day files.

### The database server

`server.rs` is `server::Home`, which alone starts, stops and keeps the board's SurrealDB server;
`server/` holds its parts, each a module doc: `starters.rs` the lock every start, stop, hold,
release and reclaim takes, `ending.rs` the asks to end, `apart.rs` a start off a reader's thread,
`left.rs` the private replay's record, `snapshot.rs` the snapshots, `hold.rs`, `reclaim.rs`,
`storage.rs` the SurrealKV settings, `backend.rs` the board home's durable engine identity and
server selection, `export.rs` what a refresh reads of a running board, and
`scratch.rs` a fixture's supervised server. `postgres.rs` is the board's PostgreSQL beside it,
the machine's own from `[board] postgresql` or discovered on `PATH`, which `Home` starts and stops
for the board whose identity names it, on a socket under `/tmp` and no TCP;
`postgres/lifecycle.rs` its hold, stopped-cluster snapshot, recovery into a scratch home or over
its own board, and vacuum ([POSTGRESQL](../../docs/POSTGRESQL.md)). `moving.rs` moves a board's
store between the engines and holds the journalled switch a restore over a board shares; a
table added to the board is placed as copied, carried or left in `store/copy.rs`, whose test
fails on one that is none, since a move would drop it. `port.rs` is a child
server's loopback port.

`pg.rs` is the socket client of that PostgreSQL. `pg/external.rs` is the hosted board's settings,
`[board.external_postgresql]`, which every build reads and judges, and `pg/hosted.rs` the hosted
client, compiled only with the `hosted-postgresql` feature
([a hosted board](../../docs/POSTGRESQL.md#a-hosted-board)): `hosted/secret.rs` reads the
references and judges the URL, `hosted/session.rs` is one verified connection and the one place
a driver's or a server's error becomes a refusal, `hosted/pool.rs` the process's bounded pool.
A change there keeps four things: no refusal carries a driver's or a server's own words (they
can hold the URL, a name or a statement), only a kind, a fixed reason, a SQLSTATE and where the
server's log has the rest; nothing is sent a second time, whatever was lost; a connection goes
back to the pool only when its request left it clean; and the store reaches a hosted board by
its private dispatch (`store/database.rs`'s `Board`), the public `Database` and `pg::Client`
as they are. PostgreSQL 16 has an analyzer of its own (`store/postgres/schema/sixteen.rs`, from
`unicode16.tsv`, PostgreSQL 18's answers as data): a change to the analyzer's definition is
proved on both by `tests/hosted/analyzer.rs`. The feature's tests are `just hosted-check`, which
no `just test-rs` of the default build runs.
`board_health.rs` is the query log and the hour the Symphony serves
([TASKS](../../docs/TASKS.md#board-health)). The owner's side is TASKS's [stuck
servers](../../docs/TASKS.md#stuck-servers) and [a board that went
back](../../docs/TASKS.md#a-board-that-went-back).

### Search

`store/search.rs` and `cli/search.rs` are `tasks search`: by words alone, ranked by SurrealDB's
full-text indexes over titles, briefs, slugs and threads, with no inference and no network request.
On PostgreSQL the same analyzer's terms are counted by triggers into `words`
(`store/postgres/schema.rs`) and scored by SurrealDB's own BM25 (`store/postgres/search.rs`), so a
search finds and ranks alike on either, each score to the bit (`tests/store/search.rs`). Latin
letters use deunicode's exceptions before decomposition, including letters outside Extended-A.
Words in another script (including Arabic) differ: SurrealDB spells them in Latin letters and
PostgreSQL keeps them. Letters newer than PostgreSQL's Unicode version are not letters there.
The board had a search by meaning, deleted
([its card](../../docs/analysis/code-review-2026-11.md#board-search-by-meaning)): the tables it wrote (`task_chunk`,
`task_embedding_source`, `task_embedding_checkpoint`) keep their definitions in `schema.rs`, since
removing one is a schema move, and nothing here writes or reads them but the whole-board copy, which
empties them. A board that holds their rows opens and searches as any other
(`tests/cli/search.rs`).

### Children

`sys.rs` and `sys/` are what the board asks of the machine and how it starts a child: `sys::command`
and `sys::command_with` build every one, `sys::output`, `sys::spawn_output`, `sys::spawn_piped` and
`sys::wait_output` read it, from the leaf `process_cwd::spawn` and `process_cwd::session`, which the
Symphony's `children` and a plugin's process share.

## Invariants a change keeps

Each is stated in full where it points.

### No branch's build moves the live board

The live board is one database many builds open at once: main's, the shared server's, a consuming
project's pinned one, every worktree's. Only a build the default branch says is its own moves it
forward, a process Cargo runs is refused it before anything is read, and a board binary run from a
linked worktree runs main's build there (`live.rs`; [TASKS](../../docs/TASKS.md#where-it-lives);
`tests/cli/main_only.rs` holds every entry on a scratch repository). A new way into the board (a
binary, a command, an API) goes through `cli::writable_board` and `Reached::opening`, so the rule
and the rollback record (`live/seen.rs`) hold for it; `cli::board_client` is kept, with its
signature, for consumers' code alone. A branch's build run by its path (`.target/debug/tasks`,
`tools/rs-bin.sh --build`) skips the launcher's rule, though not the board's: run it only on a
scratch board.

### The schema moves forward, and older builds keep reading

The board is a contract ([CHECKS](../../docs/CHECKS.md#the-board-is-a-contract)): `schema::VERSION`
counts breaking steps alone, and a step that only adds is an additive step on it
(`schema::ADDED`, each with the tables it defines and why it is additive), which leaves the version
as it is. A breaking step is a new version, migrated once, in one transaction, by main's build at
its first command after it lands; an additive step is migrated the same way where a build may move
the board, and holds no writer. Every write transaction starts with the schema guard, which also
refuses a write naming a table of a step the board lacks (`schema::needs`), and a reader one
version behind still reads (`schema.rs`, `store/opening.rs`, `store/transaction.rs`;
`tests/store/schema.rs`, `tests/store/open_read.rs`, `tests/store/contract.rs`,
`store/transaction_tests.rs`). So an older build keeps running against a newer board for a while,
and a change keeps what it reads:

- the open tables (`schema::OPEN`) are read past a field a build does not know and written by
  named fields, never replaced whole (`store/surreal/fence_tests.rs`), so a field of an additive
  step survives every build of the contract. Each row struct refuses a field it does not know
  (`deny_unknown_fields`), and the store's reader hands an open table's row only the fields it
  names (`rows::open_row`, `objects::read`), so the whole-board dump alone, read exactly
  (`objects::read_exact`), refuses one by name, as a copy would drop it. A new table, or a new field on one, is a step's;
  a closed table changes only by a breaking step;
- before contract 28 nothing new went on `session`, as builds before it refused an optional field
  once a row carried it: callback addresses, chat ownership, delivery state and bindings live in
  schemaless tables (`callback_state`, `callback_event`, `callback_lease`, `chat_event`); and an
  old column keeps its old reading: a launch record's `claude_session` holds Claude ids as before
  and Codex ids prefixed `codex:`, which restore dispatches on and the agents API strips;
- an in-process binding never registers a callback or takes over a handle with a native
  registration, and a bound key of no known class fails closed until a full-session hook binds it
  (its refusal asks for a retry once the new `tasks` is linked);
- no task field an older reader would refuse: the board's own fields go under `fields._board` and
  only their own writers write them (`set_field` refuses them), and a hold is a record
  (`counter:['hold', SLUG]`), never a field;
- a new meaning goes in a field older binaries ignore and keep, beside the old one, which keeps its
  meaning, with no schema change: an orchestrator's `seen`, `statuses` and PULL entries keep the
  whole-board meaning older dispatchers read, and what it hears and was told lives in `narrowing`,
  `pull_withheld`, `pull_awaiting`, `pull_said` and `pull_idle`, written at both acknowledgement sites
  (`store/callback_state.rs`, `store/pull_account.rs`); `revision`, which every older write moves,
  is never read, and taking or releasing a `callback_lease` rewrites no `seen` or `statuses`;
- with an older dispatcher interleaving, no entry an orchestrator hears is lost and no line is said
  twice (`store/pull_account.rs`; `tests/store/mixed_dispatchers.rs`,
  `tests/store/pull_acknowledgements.rs`, `tests/store/pending_pull.rs`); a new kind of line that
  names a task takes its test of whom to tell from `store/queued_lines.rs`;
- a counter an older reader caches is retired where it refuses it, never removed (`counter:board` at
  the smallest signed integer, `schema::BOARD_MARKER`).
- the `writer` table stays schemaless with no event, its line written without the schema guard, as
  builds of other schemas write it and a line must move no marker; an opening reads its line in its
  one read and never reads the store's `Clock` (a line's time is the machine's, so a test ages one
  by moving its `seen` back by hand); and the Symphony, which opens its board to read, stays
  uncounted: it takes a new schema only by its reload after the move, so counting it would refuse
  every refresh's migrate (`store/writers.rs`; `tests/store/writers.rs`).
- each schema's definitions are committed, one file per engine
  (`tests/store/catalogue/ENGINE-VERSION.txt`, and `ENGINE-VERSION.STEPS.txt` for its additive
  steps, written with `TASKS_CATALOGUE=write`); each additive step is the file before with only
  what an additive step may add, its declared tables and no row written, and a version in
  `schema::COMPATIBLE` keeps every table of the one before as that version's file has it
  (`tests/store/catalogue.rs`). Only a store opened to read through the rule has the reader's
  role (`Store::reader`, set in `store/opening.rs`), and only its ordinary write may take a board
  one ahead whose `meta:schema` record lets it: a writer's, an unchecked store's, a copy's staging
  and a recovery's rebind are held to their own schema, and the last two, with the dump, to their
  own additive steps exactly (`store/transaction.rs`'s `Run`;
  `tests/store/compatible_ahead.rs`, [CHECKS](../../docs/CHECKS.md#a-schema-step-that-keeps-the-symphony-writing)).

### Its contracts

`tasks api` is a contract ([TASKS](../../docs/TASKS.md#the-tasks-api)): within a major a method, an
argument or an answer's field is added, never removed, renamed or changed in meaning, an argument
added has a default, and each addition raises `MINOR`. `tests/cli/api/contract.rs` holds the calls a
registered project's programs make; a call such a program starts making is added there. The names
consumers call are held by `tests/consumer_api.rs` ([RELEASES](../../docs/RELEASES.md)), and one
changes only with a consumer release plan (the crate's `clippy.toml` says why
`avoid-breaking-exported-api` is off).

For those Rust calls, the [two-release rule](../../docs/CHECKS.md#consumer-api-changes) keeps the
working old item without `#[deprecated]` beside its replacement for a release. Removal is a
separate later task with read-only consumer migration evidence and the compile list updated.
That Rust compatibility rule is separate from the `tasks api` version contract above.

### A write decides on what it read

A rule reads what it decides on, decides in the process, and writes in one transaction fenced on
what it read; SurrealDB's transactions are optimistic and conflict only on rows both write, so two
writes that must not pass each other write one row (`lib.rs`). The cap is that rule at scale: a
start bumps `counter:['slots', HANDLE]` of every orchestrator whose open work it changes, a red's
close and every start under it hold `counter:['red', SLUG]`, and the owner's cancel fences on the
task's version (`slots.rs`, `store/cap.rs`; `tests/store/slots/`). A pass of the board's gives is
fenced on the task versions and the lost holders' last beats it read (`given::FENCES`), and
`leave`'s gives on the orchestrator roles it read (`$held`). A close and a restore hold the session
(`Store::take_session`) and fence every write on it (`store/launches.rs`). A write that keeps only
what a reader derived (`Store::record_fixed`) moves no task version, so it never fences a concurrent
move as stale. A plain `Store::create` gives no orchestrator: a new way to file goes through
`Store::give_filed`, as the command line's and the API's `create` do.

### A copy replaces the recovery generation

Whatever replaces a board's rows whole replaces `recovery:board` in the same transaction, and
whatever puts a database's files in place revokes the home's bindings first
(`Home::revoke_recoveries`): a copy that kept the generation would hand the numbers and versions
it gives again to tokens read before it. A write bound to a token starts with the fence
(`surreal/recovery.rs`'s `FENCED` and its twin, their variables from `Store::bound_vars`), before
anything it writes, writes the task's own row, and refuses a mismatch rather than read again. A
store finds no authority by itself: it is handed one (`Store::recovering`), and a rebind never
adopts the generation it found (`store/recovery.rs`; `tests/store/recovery.rs`, and the restore
in `tests/server/postgres.rs`).

### The store's statements live in one place

The store's modules hold the rules: what is read, what is decided, the variables a statement is
handed and the shape of its answer. The statement's text is `store/surreal/`'s, under a name the
store's module asks for whole (a constant, or a function of the few values its text turns on), so
no SurrealQL is written anywhere else but in the files `store/surreal/fence_tests.rs` lists with
their reasons (the schema, the client, the server's reclaim). Its PostgreSQL twin is
`store/postgres/`'s, by the same name in a module of the same name, and a call names both
(`store/statement.rs`'s `both`); a call that names its SurrealQL alone has no twin yet, and on a
PostgreSQL board it is refused, naming its line, before anything is sent. Nothing reaches a client
but through `store/transaction.rs`, the store's one sender, `store/raw.rs`, which sends the rows'
own statements, and `store/database.rs`, which makes a board's database. A new statement goes
there; a test that needs a row no command writes, or reads one, asks `Store::raw` by table, id and
equal fields and writes no statement of its own, and one that must (the engine itself is its
subject) is in the list `tests/raw_fence.rs` holds, a target of its own that `just lint-rs` runs, so
a branch that touches any board target's tests meets it. A store test written once for both
databases is `on_both!`'s (`tests/store/main.rs`), each run on the binary's own scratch server of
each (`tests/store/common.rs`'s `Backend`).

### Delivery never loses a line

A write is never sent to the database again; only a request whose text proves it a read goes once
more on a new connection (`rpc/read.rs`). A dispatcher whose lease was taken while it built writes
no account and sends nothing, so two never send one line (`store/dispatcher.rs`;
`tests/store/slow_builds.rs`); a recipient's observation is checkpointed with the events it queued,
before anything is sent; a print or a send that fails is a retry, so a line may repeat but is never
lost; a post under a key is made once (`Store::say_keyed`). Every event line is flattened before it
enters the envelope, but owner chat's payload, which keeps its reply lines. The UDS transport sends
only the newline-delimited SDK user envelope: it never reads or sends an authentication token and
never claims to be the receiving session's child.

### Children start through `sys`

Every child is built by `sys::command` or `sys::command_with`, without the starter's session
(`sys::SESSION_VARS`) or the marks of how it was asked for (`sys::MARKS`), and its output read by
the `sys` helpers, never `Command::output` or `Stdio::piped`, here and in the sources of `symphony`
and `symphony-server`, but in `store/codex_delivery.rs`, whose proxy is answered line by line and
killed, never read to its end (`sys.rs` and `process_cwd::spawn` say why: macOS's pipe gap, which
holds a reader's end of file off for a racing child's life, and hc's scope). A call that must keep a
name sets it on the command after, saying why: `tasks run` gives its command the caller's session
and marks back, and a land's children name the land (`TASKS_SESSION`, `TASKS_TASK`) so what its
check builds is recorded for the task. Clippy holds it: the crate's `clippy.toml` refuses std's and
`process_cwd`'s builders however they are spelled, and the release run's `tests/child_guard` tries
the rule. A git is `sys::git()` (the Symphony's `children::git`), less `sys::GIT_VARS`, which a git
hook hands what it runs, so it acts on its own folder's repository; only `commit.rs` keeps them.

### The database server is never killed

Only `server::Home` starts the board's server, so every one runs under `server::Storage`: one
started by hand keeps SurrealKV's own memtable, gigabytes of log before an sstable, and its own
value-log threshold, under which a row rewritten in place keeps every version (`server/storage.rs`).
A stop sends one SIGTERM, once the server answered, and waits; it never sends a second, which
SurrealDB takes as an immediate shutdown, and never signals a server that has not answered, which is
replaying: at `--log warn` a replay that fits its memtable logs nothing, so nothing tells it from a
hang, and that call is the owner's. Every start, stop, hold, release and reclaim takes the starters'
lock; a start outside a scratch place snapshots the store first, and one that replays keeps it on
every board; a replay's server is never the board's. Never set
`SURREAL_SURREALKV_ENABLE_VLOG=false`: a store with values in its log then cannot read them.
(`server.rs` and `server/`; the owner's side in [TASKS](../../docs/TASKS.md#stuck-servers).)

### A terminal closes only on information

A close never takes the caller's terminal (`HC_TERMINAL_ID`), an orchestrator's or a chat's, nor one
the owner may be looking at, and reads the window last, right before the kill
(`screen::close_unseen`). A terminal whose `hc get`, process table or window cannot be read stays,
said as `kept pane … (never closed on no information: WHY)`. The idle sweep's last looks read the
board again, and a session doing work, or that will within the hour, keeps its terminal
(`reap::IDLE_HOUR`). (`launch/close.rs`, `launch/cleanup.rs`, `launch/reap.rs`; `tests/launch/`.)
Presence is judged the same way: only the sweep confirms it, never a delivery or an accepted Codex
queue, and an incomplete terminal inventory confirms the threads it identifies and retires no
registration it could not match (`store/callbacks.rs`).

### Work finishing runs main's build

The sweeps work finishing starts run the main checkout's build of `tasks` (`builds::of_main`), never
the caller's, a linked worktree's link included: a worktree with sweep code that has not landed must
not run it on the repository (`sweep/trigger.rs`); the Symphony's reap runs that build too
(`board_build` in `apps/symphony-server/src/front.rs`). `on_review` and `on_land` run the project's
command detached from the main checkout. A board `TASKS_SERVER` names starts no sweep, `on_review`,
`on_land` or regenerator unless its variable says `on` (below).

### The land and the verdict share one rule

`tasks land` runs only under the `land` recipe's `SYMPHONY_LAND` (`land::admit`), a guard against
accident, its checks being the gate. The approve that counts is one rule, `Held::unfresh`, which
`Store::verdict`, the land, the assessor's `needs` and staffing's reviewer pick share, and the land
goes by the stamps through `land/needs.rs` over the readers of `stamps/read.rs`, which the task page
draws too, never a copy. Its steps and their order are `land.rs`'s doc and
[TASKS](../../docs/TASKS.md#the-default-landing).

### Queries keep their indexes

What a write, each recipient of its dispatch and each tick read goes through an index, never a scan
of message, delivery, role, session or callback_state (`schema.rs`'s doc lists them), and a lookup
reads in proportion to what it is asked (`store/unanswered.rs`). The `perf` target plans every such
statement and every event's; a new statement or a changed condition on those paths runs it (below).
A transaction over many rows avoids what costs more than linearly on the engine: an index a copy
inserts into, an event that UPSERTs (`schema.rs`). On PostgreSQL the whole-board read and the
reads that are one plain list of rows (`listed`, `list_of`) answer an item a row (`Pg::Listed`,
`store/postgres/mod.rs`), not one `jsonb_agg` value, which the backend builds whole, in several
times its size, and keeps until its command ends. A thread, several threads, the task tree, an
inbox's lines since its last look, the open asks and the list of every session
(`sessions::sessions`) are still one value; a new read of a list that grows with the board is
written as `Pg::Listed` ([the measurement](../../docs/analysis/board-store-baseline.md#postgresqls-memory-peak-measured-2026-11-18)).

### Time in tests

A product deadline a test would trip on a loaded machine is a setting read only when set, never
raised for anyone else: the recorder's waits scale by `TASKS_RUN_WAIT_SCALE` on a scratch board
alone, and these are seconds (`Env::seconds`): `TASKS_RECORDER_PROBE_WAIT`, `TASKS_ON_LAND_SAYS`,
`TASKS_HC_ENDING`, `TASKS_CODEX_WAIT`, `TASKS_UNANSWERED_WAIT`. A test of the deadline sets it
short; every other test gives it a backstop. Where a test must act only after the product did
something on a thread of its own, the product leaves a mark it reads only when set
(`TASKS_RUN_END_MARK`). The rule is RULES.md `tests/no-test-fails-on-load`; the helpers
`crates/test-time` ([CHECKS](../../docs/CHECKS.md#time-in-tests)).

## Testing against a scratch board

No test, example or `cargo run` reaches the live board (above). A test's board is a database of its
own on a server of the test binary's, or a fixture repository's in a temporary folder; by hand, a
branch's code runs on a scratch database of the live server, `just tasks --database NAME …` (the
last leading `--database`, else `TASKS_DATABASE`, not `tasks`) or
`just agent-send --database NAME HANDLE TEXT --as SENDER`, which runs the branch's build and moves
that database forward, and never sweeps a repository.

### The servers

The store's tests start one in-memory server per test binary and give each test a database of its
own (`tests/store/common.rs`: `Fixture`, the settable clock and process table, the `Seed` a board is
copied from, `gate.rs` for a statement held mid-write). The server is the pinned SurrealDB
`tools/surreal.sh` fetches once per repository into `symphony/surreal/` in the git common dir,
refused unless its SHA-256 is the pinned one; `SYMPHONY_SURREAL` names another binary, and
`server/fetch_tests.rs` holds the project's fetch line to the script's pin. A fixture board under
the temporary folder or a checkout's `artifacts/` is a scratch place: its server is supervised and
ends with the process that started it (`server/scratch.rs`), and a fixture repository there is its
own project, so no test reaches the board around it. The PostgreSQL tests (`tests/server/postgres.rs`)
run the machine's PostgreSQL 18 or later, from this repository's `symphony.toml` (`[board]
postgresql`) or the first executable `postgres` on `PATH`, with `initdb`, `pg_ctl` and `psql` in
that same folder ([setup](../../docs/PROJECT_FILE.md#board)). An invalid override or incompatible
installation fails; each server runs on a scratch board of its own, started and stopped by the test.
The hosted board's tests (`tests/hosted/`, `just hosted-check`) start PostgreSQL 16 and 18 servers
of their own with TLS on a loopback port (`tests/hosted/cluster.rs`): 18 found the same way, 16 in
Homebrew's keg or the folder `SYMPHONY_POSTGRESQL_16` names, and `openssl` for their certificates.

### The command line's tests

The crate's tests run their own `tasks` binary; `SYMPHONY_TASKS_CLI` injects it into the `symphony`
crate's board clients. They reach the test's board with `TASKS_SERVER` and `TASKS_DATABASE`, and
steer the rest with:

- `TASKS_NOW`, the clock, which a monitor's waits step (each says `STEP <now>` and reads a line);
  `TASKS_PULL_SECONDS`, how often an orchestrator's monitor computes its PULL digest;
- `TASKS_DISPATCH=inline`, the write's delivery made in the write, for a test that reads what a
  command delivered once it ended;
- `TASKS_REAP_STATE`, where the idle sweep keeps what it saw;
- `HC_BIN` and `HERDR_APP_BIN`, a fake hc and window: the crate's `fake-hc`, its terminals, the
  window's layout and what it shows in the JSON file `FAKE_HC_STATE` names, each call a line of
  `FAKE_HC_LOG`, the calls `FAKE_HC_FAIL` names refused; a test that names none is given
  `/usr/bin/false` by the test runs (`just test-rs`, `tools/test-lanes.sh`), so no case reads this
  machine's terminals, where one named as a fixture's session would be taken for its pane;
- `TASKS_PS_TABLE`, a file of `ps -axo pid=,ppid=,command=` lines read in place of `ps`, so no case
  reads this machine's process table; read only with a `TASKS_DATABASE` other than the live one;
- `TASKS_SWEEP=on`, `TASKS_ON_REVIEW=on`, `TASKS_ON_LAND=on` and `TASKS_REGENERATE=on`, without
  which a board `TASKS_SERVER` names starts none of them (`off` starts none anywhere), and
  `TASKS_BIN`, the build the triggers spawn in the main checkout's place on such a board.

A test of a command that reads a mark (`SYMPHONY_UNASKED`, `SYMPHONY_LAND`) runs it in an
environment of its own (`symphony::cli::main_in`, the board's `Env`), never this process's, so a
test run under a recipe or a land is not red or green by what it inherits. The library's own tests
make `ps` or a clone fail on their own thread alone (`server::PS_FAILS`, `snapshot::CLONE_FAILS`),
take the snapshot a scratch start skips (`Home::snapshotting`), and give a store its limits and
thresholds without a file (`Store::limiting`, `Store::alarming`).

### Repositories and terminals

The sweep's and the land's tests (`tests/sweep/`) build real repositories in a temporary folder,
land on a board of their own and push to a scratch bare remote; `land_signals.rs` runs the binary
and signals it. The terminals' tests (`tests/launch/`) drive `fake-hc`. `tests/server/` starts real
servers on scratch stores, killing a starter mid-replay (`left_replay.rs`) and holding a store that
went back (`hold.rs`).

### At scale, and the release run's targets

The `perf` target is the board at the live one's scale on `surrealkv://`: the plan check of every
statement a per-write, per-recipient or per-tick path sends, and each command's CPU within a bound
`tests/perf/timings.rs` states (`tests/perf/main.rs` says the rest). Only the release run runs it
here, so a change to the board's queries, its indexes or the api runs it by hand:
`just test-rs -p symphony-board --test perf`; `SYMPHONY_PERF_TASKS=PATH` times another `tasks` build
on the same board. `tests/child_guard` is a nested build, the release run's alone.
`examples/dispatch_load.rs` measures concurrent writes against a scratch database seeded from a
dump. `examples/board_load.rs` is the store's meter: a fixed amount of the agents' work (beats and
the board commands they run, as the `tasks` binary) on a scratch board whose server `server::Home`
starts, reporting each command's cost and the server's CPU, memory and bytes; a change of the
store, its settings or its engine runs it on both sides the same day, from one export and one
build of `tasks`, and what the store cost before any such change is
[the store's baseline](../../docs/analysis/board-store-baseline.md). An example's tests run only
when named: `just test-rs -p symphony-board --example board_load`. Which tests a change runs, and
the evidence its kind asks for:
[CHECKS](../../docs/CHECKS.md#picking-the-focused-set).
