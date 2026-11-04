# crates/tasks/

The workspace, the cargo wrapper and the toolchain pin: [the Rust section of
the root guide](../../AGENTS.md#rust).

The `tasks` crate is the task board ([TASKS](../../docs/TASKS.md)): its
store over SurrealDB, the server it starts in `sf-tasks/` in the git common
dir, and the `tasks` binary, which is `just tasks` and `just tasks-attach`.
These recipes and `just worktree-sweep` go through `tools/tasks.sh`: live board
commands always run committed main's build, preserving the calling worktree's
working directory. `sf-cargo/tasks-main/TREE` in the common dir records the
binary for main's tree id. A warm call checks that executable and runs it with
no export, build or source hashing. On a miss, an ancestor main build serves
immediately when its committed schema and supported RPC server versions match.
One background builder exports committed Rust sources and atomically publishes
the new pointer after success. It owns a separate process group, so maintenance
callers ending their groups leave the refresh running. A changed or unreadable
contract, or no earlier executable, waits for a build. Failures keep the earlier
build and are reported on later commands; background output is in
`sf-cargo/tasks-main/build.log`, foreground output goes to caller stderr.
Uncommitted main changes stay off the live board. The launcher reports the main
commit and build
on stderr. Agents keep using `just tasks … --as HANDLE`: that outer command is
what the beat hook recognises and binds, before the recipe selects the binary
internally. A `--justfile PATH`, `-f PATH`, `--working-directory DIR` or `-d
DIR` between `just` and `tasks` binds the same way, so a worktree's `just
--justfile MAIN/justfile tasks … --as HANDLE` does.
To run branch code, explicitly select a non-live database with `TASKS_DATABASE`
or a leading `--database NAME`; a leading option overrides the environment.
With the live `tasks` database, `TASKS_SERVER` selects branch code only when its
validated TCP port differs from the canonical local listener in
`sf-tasks/server.json`. The exact live address runs main. Different spellings
on that port, invalid ports and missing or malformed records refuse the
override; explicitly name a scratch database to use branch code there.
`TASKS_DB` is the old SQLite cutover input, not a scratch selector. Never run
`tools/rs-bin.sh tasks` directly against the live board from a branch.
The crate's tests keep running their own binary; `SF_TASKS_CLI` continues to
inject that binary into the symphony crate's board clients.
`tasks callback` registers the session's native channel (`uds:` or `codex:`).
Chat selects the explicit seat, then an `--if-free` fallback, then a live
orchestrator with a native callback; among plain orchestrators the
lexicographically smallest handle wins. No chat flag is needed for that last
fallback. Sweep-confirmed presence counts as live, and a recipient suspended
after delivery failures waits for the sweep to restore it.
`just agent-send HANDLE TEXT` runs the standalone `agent-send` binary, using
the same callback outbox without a task. It names its sender (`TASKS_SESSION`
or `--as`) in one plain AGENT line; the [tasks skill](../../../.agents/skills/tasks/SKILL.md#events)
gives the reply command. No channel refuses with registration and
task-inbox instructions. `--database NAME` isolates scratch runs.
Channel metadata and delivery state live in separate schemaless tables, so
older binaries can still read and update session records. Keep callback address
and chat ownership off `session` entirely: even an optional field breaks older
readers once a row carries it. Leaving deletes the handle's callback state.
UDS registrations pin the listener's kernel peer pid and process start time.
Sweep and delivery connections check that identity; a dead or replaced listener
retires the registration, never transfers it to a new process at the same path.
An address held by another valid registration is refused. In-process bindings
are recorded outside session by the native hook, and cannot register callbacks
or take over a handle that holds a native registration;
unclassified bound keys also fail closed until a full-session hook binds them.
An unclassified binding asks for a retry after the new tasks binary is linked;
a known in-process binding uses its board inbox. Reads use cached evidence,
never connect or retire registrations. A refused or full socket is retryable
while the stored process identity is alive; it does not lose its registration.
The sweep alone confirms presence (never a delivery
or an accepted Codex queue). Its STALE/LOST suppression lasts REAP_SECONDS plus
LIVE_SECONDS, across the next sweep. Sockets are checked even without Herdr;
Codex presence uses thread identities across all workspaces, while pane cleanup
remains scoped to its caller. Dry runs change neither board nor sweep files. A
Codex registration whose presence confirmation expired reads `unknown` and retains its work:
the presence sweep failed or is overdue, rather than having established the agent's absence.
That grace lasts at most ten minutes after confirmation expires; then ordinary heartbeat health
applies again, making unfinished work claimable without a beat. Each confirmed pane resets it.
The next successful sweep confirms the pane or retires the registration.
An incomplete pane inventory still confirms its identified threads; it withholds retirement of
unmatched registrations until an inventory establishes absence. PULL lists unknown handles only
while they hold claimable work, just as the per-child warning does.
The shared server sweeps once on startup, then every five minutes. Each command has a four-minute
deadline and its owned process group is ended on timeout or shutdown, so the next sweep can run.
An unchanged callback snapshot drains its durable outbox without rescanning the inbox; its message
cursor advances only with the persisted observation and queued events.
Unknown observations keep the heartbeat health older binaries derive in `seen`, with separate
metadata restoring Unknown for newer readers. Both old and new writers must share that baseline
past the ten-minute lost threshold, or alternating dispatchers repeat liveness notices.
A finished idle pane still closes under the original cleanup rules, retiring its
registration. Restore prompts re-register the resumed process's callback first.
Neither channel checks nor delivery fabricate tool calls.
Outbox rows are indexed by handle
and delivery state, with a numeric sequence for ordering; chat retries belong
only to the current recipient. Transactional mutation events advance `counter:board` for board
inputs, callback registration/observation changes and owner chat. A Store caches the raw board
snapshot together with that marker, read in one transaction, and rebuilds its derived health,
presence grace and reservation state against the current clock. The Symphony server uses those
cached inputs even without a callback recipient; machine/test probes and the agents view still
run each tick. Unchanged live observations skip planning until the periodic PULL check; health
changes invalidate that shortcut, and due outbox retries are checked every tick.
Dispatch plans before leasing and refreshes its inputs and observation again after acquiring
the small `callback_lease` record. Its revision identifies the holder, including on release;
taking or releasing it never rewrites `seen` or `statuses` or allocates a counter. Lease rows
survive retirement and pruning so a revision is never reused. A completed periodic PULL check
records its time in that small row even when the digest adds no entry, rearming the deadline
across fresh Stores without moving the board marker. A tick with no changed observation, due
delivery or due periodic check writes nothing. The clock's tick, never a write's dispatch, also
watches panes (`attach::pane_watch`): while a recipient oversees a session with an open launch
record, it lists herdr's panes once a tick and queues BLOCKED once an episode, or WATCH once while
herdr cannot be asked; what it last said is `callback_state.pane_watch`, off the board marker.
It looks before the tick's dispatch, and the tick's PULL view and the clock's agents view read
its sample (`Store::clock_panes`), so a tick asks herdr once. It plans from the stored
`pane_watch` and looks again under the lease, so two clocks on one board say an episode once.
The legacy monitor runs the same watch on its own looks. Observation state is checkpointed
with newly queued events, before transport. Finished tasks leave its status, role and verdict
maps after their final diff; messages and pending events retain their durable cursors and outbox.
Finished tasks cannot reopen through the board's transitions. A hand-edited reopen has no
previous status to diff: an orchestrator can hear NEW for ready work or a new PULL entry, but
no recipient hears STATUS for that hand edit, and a worker receives no reopen notification.
Delivered events and unused binding provenance
are pruned; pending events remain in the inbox. A reachable suspended chat
recipient recovers its saved registration flags on the sweep: a plain orchestrator
returns plain and a non-seat fallback returns as a fallback. A saved seat is
restored only when another recipient has not taken it. Repeated failures from a
stale dispatcher preserve the saved flags while recovery is pending.
Owner saves journal locally for the server clock
to deliver, outside the save lock. Socket writes use one buffer and a 250 ms
timeout. UDS delivery reserves and reads back capacity for the complete envelope before writing.
A batch that exceeds capacity splits between events; only an individually oversized event stays
in the inbox after its attempt budget. Liveness sweeps re-arm transport failures, never events
already found individually too large. A new callback registration can reset that size finding.
Every generated event line, including NEW and PULL titles, is flattened before
it enters the callback envelope. Owner chat payloads retain their reply lines.
The UDS transport sends only the
newline-delimited SDK user envelope; it never reads or sends an authentication
token and never claims to be the receiving session's child. Native inbound
controls still apply. `tasks --database NAME ...` is the explicit
`TASKS_DATABASE` override for scratch runs; it disables repository sweeps.
`tools/rs-bin.sh` builds each binary once per hash of its sources into
`sf-cargo/bin/` in the git common dir, for every worktree whose sources are the
same, and links `.target/bin/NAME` to the build the tree last ran. After a live
`just tasks` command, the launcher points the calling checkout's tasks link at
main's build too; the heartbeat hook runs that link, building nothing. Scratch
commands first look up `rs-bin.sh --where tasks`, then use `--build` only if
the executable is missing; neither changes that live link. The native `tasks api`
JSON interface (`src/cli/api.rs`) takes a call on stdin; `tasks api --lines` takes
one JSON call per line on stdin and answers each on its own line (a line that
is a list of calls gets a list of answers), the methods being the store's, plus
`threads` and `snapshot`, which read several things in one round trip.
`launch`, `restore`, `cleanup`, `finish` and `reap` (the Symphony's idle
sweep, every five minutes) drive herdr's panes and git's worktrees from
`src/launch/`; after a launch's prompt it looks at the pane again and sends
enter while the prompt still sits in the input line. Its tests' handles:
`TASKS_SERVER` and `TASKS_DATABASE` reach a board of the test's own
(`tasks.Server`, one in-memory server per test process, a database per test);
`TASKS_NOW` pins the clock, and a monitor's waits step it (each says `STEP
<now>` and reads a line); `TASKS_PULL_SECONDS` sets how often an
orchestrator's monitor computes its PULL digest (only a new actionable entry
prints one); `TASKS_REAP_STATE` is where the
idle sweep keeps what it saw; `HERDR_BIN` names a fake herdr (the crate's
`fake-herdr` binary, its panes a JSON file `FAKE_HERDR_STATE` names).

A child's input and output go through `sys::output` (or `sys::spawn_output`,
`sys::spawn_piped` for one that reads stdin, and `sys::wait_output`), never
`Command::output` or `Stdio::piped`, here and in the sources of `symphony`
and `symphony-server`; but in `store/codex_delivery.rs`, whose proxy is
answered line by line and killed, never read to its end. `port::listens` is
handed what runs its `lsof`: `sys::output` from those sources, `Command::output`
from `apps/symphony-ui`'s browser test, which compiles `port.rs` alone. On macOS std
makes a pipe or socket pair and sets close-on-exec in a second call, so a
spawn on another thread between the two keeps the writer and holds the
reader's EOF off for that child's whole life: a herdr answer then reads as
unanswered. `sys::pipe` opens a FIFO close-on-exec in the open itself;
macOS's `poll` never sees a FIFO's end, so its readers are read, never polled.
Those helpers take a `sys::Command`, std's builder by its method names plus
`new_session` and `pass_fd`, and so does every spawn in a session of its own
(`sys::new_session` is gone). std takes its fork path for a `pre_exec`, a
session or a changed environment with a bare name, and that path's exec-status
pipe has the same gap, so a racing spawn held `spawn()` itself; on macOS a
`sys::Command` spawns with `posix_spawn` and `POSIX_SPAWN_CLOEXEC_DEFAULT`: no
status pipe, and the child inherits only its streams and the descriptors it is
passed. A std `Command` cannot say it cleared its environment, its group or its
session, so it is never translated. The helpers live in the leaf
`process_cwd::spawn`, which `sys` re-exports, so the cwd scans' `lsof` shares them.

`staff` makes one staffing pass (`src/launch/staff.rs`), also called before `reap`.
Its sole config is `tools/task_staff.json` in the calling checkout, disabled by
default; its pass lock is in the common `sf-tasks/`. Launch defaults to Codex;
`--kind claude` selects Claude. The legacy `claude_session` storage column keeps
Claude ids as before and Codex ids prefixed with `codex:`; restore dispatches on
that prefix, and the agents API strips it from the conversation id.

`tasks sweep` is the worktree sweep (`src/sweep.rs`, its rules in its module
doc and [tools/AGENTS.md](../../../tools/AGENTS.md#landing-on-main)): `git.rs` its
git reads, `process.rs` the locks' pids (`kill(pid, 0)`, `ps`); the leaf
`crates/process-cwd/` owns the bounded `lsof` scan shared with the cargo sweep.
`trigger.rs` owns
the detached sweeps work finishing starts (`done`, `move`,
`verdict`, `leave`, `reap`, and `inbox` once an hour). Those run the main
checkout's build of `tasks`, the one its `tools/rs-bin.sh --where tasks` names,
never the caller's: a worktree with sweep code that has not landed must not run
it on the repository. With no such build the log says so and nothing sweeps. A
board `TASKS_SERVER` names starts none unless `TASKS_SWEEP=on`, so the CLI's
tests never sweep this repository; the sweep's own tests (`tests/sweep/`) build
real repositories in a temporary folder, and spawn the binary `TASKS_BIN` names
for the triggers, which stands in for the main checkout's build only on a
board `TASKS_SERVER` names.

Its store tests start one in-memory server per test binary and give each test a
database of its own. The server is the pinned SurrealDB `tools/surreal.sh`
fetches once per machine into `sf-surreal/` in the git common dir, refused
unless its SHA-256 is the one pinned there; `SF_SURREAL` names another binary.
The client speaks the RPC in CBOR, never JSON, whose variables lose any string
that starts like a record id, a datetime or a UUID. `tasks-copy` copies a whole
board into a server, replacing what it held, in one transaction: `tasks-copy ADDR
[DATABASE] < dump.json`, or `tasks-copy --board` for the board this run reaches;
`tasks-copy --board --out` writes that board's dump, in the shape the copy reads,
and changes nothing. `just tasks-cutover` (the `tasks-cutover` binary) moves the
old SQLite board into it once, one way, reading SQLite through `rusqlite`
([TASKS](../../../docs/workflow/TASKS.md#the-move-to-surrealdb)). On the in-memory
engine a transaction that big costs more than linearly (the board at the move:
39 server CPU-s, against 1.5 on `surrealkv://`).

Every server `server::Home` starts gets `server::Storage` as its environment
(`SURREAL_SURREALKV_VLOG_THRESHOLD`, `SURREAL_SURREALKV_MAX_MEMTABLE_SIZE`).
Each value stays beside its key in the sstables, where compaction drops the
versions a rewrite leaves, and the write-ahead log holds at most 256 MiB.
SurrealKV's own threshold, 4 KiB, sent large values to a value log. It deletes a
value-log file only once no sstable points into it, so a row rewritten in place
kept every version: 102 GiB for a 0.5 GiB board (tools/tasks-db-space). Never
set `SURREAL_SURREALKV_ENABLE_VLOG=false`: a store with values in its log then
cannot read them. `tasks-reclaim` (`just tasks-reclaim`, the owner's) is
`Home::reclaim`. Under the starters' lock it ends the board's server and starts
an unrecorded one on the store. It exports every namespace's every database with
`surreal export` and imports them into `reclaim/data` under `Storage`. Then it
re-exports, compares each file byte for byte along with the root's and
namespaces' users and accesses, swaps the stores, starts the board's server and
removes the old store; the exports stay. A failure before the swap restarts the
old store untouched. A start refuses while the store sits in `reclaim/old`
without a successor, and the next reclaim puts it back before starting over.
Without arguments it reclaims the board of the repository it runs in; `--dir
FOLDER` reclaims a board folder elsewhere, as its tests and a rehearsal on a
clone do (an APFS clone of `sf-tasks/data`: 102.8 GiB to 1.5 GiB, the server
down 8 min).

`tasks search` keeps keyword ranking by default, including exact slugs. `--mode
vector` uses BGE chunk neighbours; `--mode hybrid` offers reciprocal rank fusion;
`--mode exact` checks HNSW recall with exact cosine. Output and JSON keep the same
task fields, score and excerpt fields. Status filtering precedes the task limit;
filtered vector searches use exact cosine to avoid HNSW post-filter recall loss.
Chunk neighbours fold to one task by best similarity, with adaptive overfetch
for repeated or stale chunks. Search rejects a chunk whose source text changed.

`tasks embed [--task SLUG] [--batch-size N] [--threads N]` is the explicit catch-up. It embeds
title/brief sources and each non-system thread message, including review
verdicts. Normal board writes and keyword search do no inference. Source
SHA-256 and model/version ids identify stale work; a source manifest and all
its 384-F32 chunks publish atomically. Missing parts are repaired, removed
sources are pruned, and a model id change re-embeds every source. The schema's
`task_embedding` HNSW index is cosine/F32 over `task_chunk.embedding`.

The shared Symphony server runs `tools/tasks.sh embed --maintenance --threads 1`
at startup and every minute in an owned child with `nice -n 10`. This is the
committed-main launcher, without the starter's agent, database or server
environment. Private previews never run it. Failures retry; server shutdown
cancels the child, and completed sources survive interruption. If the shared
server is down, catch-up resumes at startup. New semantic matches therefore
become visible after a completed pass, rather than synchronously with writes.
`--maintenance` writes only derived caches and needs no agent identity; an
explicitly supplied identity is still validated. A nonblocking per-database
file lock admits one embed writer. Unchanged maintenance passes read only the message counter,
task update stamps and model id against the last successful whole-board checkpoint, never source
text, chunk manifests, model loading or downloads. The checkpoint is the server's SHA-256 of those
stamps, never the stamps: kept whole, they were a 165 KB row rewritten by each changed pass. A
changed board scans hashes and part counts and embeds only changed sources. Explicit catch-up also repairs derived cache damage without a
board write. Manual bulk catch-up uses
one thread too; `--threads 1..64` explicitly overrides it for bulk work.
Short search queries always use one inference thread.

The approved BAAI/bge-small-en-v1.5 model runs locally through tract-onnx 0.22.5
on CPU. `embedding.rs` pins the publisher revision and SHA-256 of the weights
and tokenizer, fetched on first use into `sf-task-models/` in the common dir,
under a download lock and atomic rename. Cargo needs no installed inference
library or model download. Queries use the publisher's retrieval instruction;
CLS pooling is L2 normalized. WordPiece chunks reserve two of 512 tokens for
CLS/SEP and retain their original token ids, including at subword boundaries.
Board text never enters network requests. Tests use the `Embedder` seam with
deterministic vectors and a tiny local tokenizer, requiring no model/network.

`cargo run --release -p tasks --example search_bench -- --database NAME --out
artifacts/HANDLE/embedding-bench` (through the root cargo wrapper) reads a board
snapshot without upgrading the live schema, copies it only to the explicitly
named non-live `TASKS_DATABASE`, and measures catch-up and all ten documented
queries. `--resume` keeps that scratch copy; `--queries-only` additionally skips
all catch-up. `--maintenance-cost` with both flags measures unchanged passes and
rebuilds one task's derived cache only in that scratch database. `--model-only`
stops after model loading and warm query timings. Its JSON includes the board snapshot, load
averages, timings, top five for each ranking, and exact-vs-HNSW recall.

`examples/embedding_cpu.rs --snapshot FILE --out DIR --threads N` reads the frozen
snapshot without connecting to a database. It measures process CPU separately
from wall time, samples length-sorted batches across the full distribution, and
labels the weighted full-corpus CPU estimate. `--reference FILE` compares the
sample vectors across thread counts. Neither benchmark estimates database CPU
from the client's CPU time.
