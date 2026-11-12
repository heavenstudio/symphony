# crates/tasks/

The workspace, the cargo wrapper and the toolchain pin: [the Rust section of
the root guide](../../AGENTS.md#rust).

The `symphony-board` crate (this folder) is the task board ([TASKS](../../docs/TASKS.md)): its
store over SurrealDB, the server it starts in `sf-tasks/` in the git common
dir, and the `tasks` binary, which is `just tasks`. Here `just tasks` and `just
agent-send` go through `tools/rs-bin.sh`, which builds with Cargo before every
run; in a project they are the recipes `symphony init` installs, which run its
linked build. The board clients of `just symphony-cli` (`taskcli.rs`) run the
`tasks` build the heartbeat finds: the checkout's `.target/bin/tasks` link, the
main checkout's, the live build `tools/tasks.sh` recorded, else `tasks` on PATH;
`SF_TASKS_CLI` names another command. A board
binary (`tasks`, `agent-send`, `tasks-copy`, `tasks-reclaim`, `tasks-cutover`)
run from a linked worktree against this repository's live board runs the main
checkout's launcher instead: main's build, in main's own target folder (the
caller's `CARGO_TARGET_DIR` is dropped), in the caller's folder and environment,
so no branch's build of a board binary runs on the live board. A branch's
`symphony` (`just symphony-cli`, a preview server) runs its own build: it opens
the board itself, only through `taskview::connect_in` and only to read (its
`connect_tests.rs` holds the symphony and server sources to that), and a reader
never moves a board forward or makes one (`schema::decide`), so a branch with
a newer schema cannot move the live board from there. The main checkout is the
common dir less `/.git`, as git finds it; the build is of what it holds,
committed or not. A run
is on the live board unless it names another database (the last leading
`--database NAME`, else `TASKS_DATABASE`, not `tasks`), or its working directory
is in another repository or none, or `TASKS_SERVER` names a port other than the
one `sf-tasks/server.json` in the common dir records, compared as the number the
binary reads (`04321` and `+4321` are 4321). With no record there is no live
server to name; a record naming no address, or either port spelled as no number,
counts as that port. A `--database` later in the arguments is read as the live
board's, so `just agent-send` takes it first to run branch code. A main checkout
with no launcher refuses the run, naming the scratch option. To run branch code,
name a scratch board. Agents keep using `just tasks … --as HANDLE`: that outer
command is what the beat hook recognises and binds.
A `--justfile PATH`, `-f PATH`, `--working-directory DIR` or `-d DIR` between
`just` and `tasks` binds the same way.

A run's board is its project's (`src/project.rs`): the nearest `symphony.toml`
from its working directory up, never past the top of the checkout it is in (the
first folder holding `.git`), names the project by its `[project] id`; with
none, or none naming an id, the machine registry (`projects.json` in
`SYMPHONY_MACHINE_DIR`, else the platform's Symphony folder) names it by the
entry for the repository's main checkout, else that checkout's folder name
does. The board is the repository's own, its server in that common dir and its
namespace and database unchanged. The heartbeat keys, the idle sweep's state
and the staffing lock stay there too. A fixture repository under a checkout's
`artifacts/` is its own project, so no test reaches the board around it.
The server `Home` starts for a board in a scratch place (under the temporary
folder or a checkout's `artifacts/`, `server::scratch`) is not detached as the
live board's is: a supervisor shell ends it once the process that started it
ends, however it ends, when the watchdog reads the end of that process's
lifeline pipe (`src/server/scratch.rs`). A test that starts a fixture board
leaves no database running under init. The supervisor's command line holds the
server's words (`start`, `--bind`, the data) among its own arguments, so a
server the record does not name, which `ensure` adopts and a reclaim ends, is
read from the process table only as the board's `surreal` itself, binding an
address, on the data as its last argument (`server_on`); a recorded pid counts
only with the data as its last argument too.

The schema rule (`src/schema.rs`, `Store::opening`): a store opens with one
read of `meta:schema`. A writer opens only a board at its own version. A newer
one migrates an older board, or one not made yet, forward once: its additive
steps and the new version in one transaction, before any other write, which
one of several openers commits while the others find it done. An older writer
is refused before any write, naming the project and `just symphony`. A reader
(the Symphony's `taskview::connect_in`) one version behind reads; more than
one behind, it is refused before it reads. A reader never moves a board
forward or makes one: a board behind it, or not made yet, it refuses after
that one read, defining and writing nothing, naming `just tasks migrate` in the
project's main checkout and a scratch board for a newer schema. A scratch board
is moved by its own `just tasks --database NAME`, which runs the branch's build.
Every write transaction
starts with the schema guard, which refuses a write to a board at another version in the
same transaction, so a reader's writes and those of a store opened before a
newer build migrated are refused there, the same way.

Which writer moves a project's live board forward (`src/live.rs`): the live
board is the `tasks` database on a repository's board server, the one on the
data in its common dir (`sf-tasks/data`), however a run reaches it: through its
project, or by an address, a `TASKS_SERVER` or a client made for it, which is
known by the server listening there wherever the run is (`live::served`: `lsof`
names the listener, `ps` its command line, whose data names the common dir,
`server::board_served`), so a run outside every repository with an empty
registry, or in a copy of the repository that kept the original's
`sf-tasks/server.json`, reaches the original's board, judged by the original's
default branch. Only where the machine cannot say who listens there (nothing
does, or no `lsof` or `ps` answers) is it known by the record of the working
directory's repository, then of a registered project, that names its port. Only a build
the repository's default branch says is its own moves one behind it, judged by
what that branch commits (`live::committed`, which `git` reads from the common
dir), never by what a checkout holds, the branch the main checkout has out or the
build's path. The default branch is `[project] default_branch` in the main
checkout's `symphony.toml`, else `main` (`live::default_branch`). Where it
commits `crates/tasks/src/schema.rs` (this repository), a build whose own schema
source (`include_str!`) is that file, byte for byte, under any command, and only
such a build makes the board when it is not made yet: a build of a branch the
main checkout has out is a branch's, whatever its working copy holds
(`tests/cli/main_only/default_branch.rs`). Where it commits none (a consuming
project, whose checkout holds a pin), only `just tasks migrate` run in a folder
of the main checkout, while any writer makes a board not made yet; in a
repository with no main checkout (a bare repository's worktrees, a submodule's
working tree, whose common dir is no `.git` folder), only `just tasks migrate`
run in any checkout of it. A project whose default branch is not `main` names
it in `default_branch`; one that does not is judged by its `main`, and with no
branch of that name commits no source, so only `just tasks migrate` moves its
board, as a consuming project's. Where `git` cannot read the branch's source
(a branch `default_branch` names that does not exist, a ref that names no
commit, a `git` that does not answer), nothing moves the board, `tasks migrate`
included, and the refusal says why (`tests/cli/main_only/default_branch.rs`).
Every other writer is refused before it writes,
naming `just tasks migrate` where it moves the board and a scratch board as its
own program takes one (`Opening::scratch`: `live::SCRATCH_FLAG` for the command
line and `agent-send`, `tasks-copy`'s own spelling, `live::SCRATCH_VARIABLE`
otherwise), each flag and variable of which that program's `--help` names
(`tests/cli/main_only/flags.rs`). So no
branch's build moves the live board, by its path, through the launcher (a
shared target folder included), `tasks api`, the sweep, `agent-send`,
`tasks-copy` or a test, while main's build moves it at its first command once
the schema lands. `just tasks migrate` says what it did (made, moved from one
version to the next, or nothing to move); on a live board outside its main
checkout (outside every checkout of a repository with none) it is refused
before it reads. A scratch board (`--database NAME`,
`TASKS_DATABASE`, a server the record does not name) any writer moves forward.
A process Cargo runs (`CARGO_MANIFEST_DIR` set: a test, an example, `cargo
run`) is refused the live board of the repository it runs from before anything
is read, and before the server is started when the command line, the
Symphony, `tasks-copy --board` or `tasks-cutover` reaches it
(`live::cargo_refusal`, `cli::board_client`): a test's board is a scratch one or
a fixture repository's in a temporary folder. So is every process that runs
under one Cargo or nextest started, whatever environment it was handed, by the
folder that one works in, which for a test is its package's (`live::cargo_here`:
each caller whose parent the kernel names `cargo` or `cargo-nextest`, read by
`sys::callers` and `sys::folder_of`, libproc on macOS and `/proc` on Linux,
never an environment, which macOS keeps from other processes): a child a test
starts with a cleared environment holds no `CARGO_MANIFEST_DIR` and is refused
as the test is, under `cargo test` and `cargo nextest run` alike. The ancestry
reaches no further. Cargo execs `cargo run`, an example and its subcommands in
its own place, so their parent is the shell: each is refused by its own
`CARGO_MANIFEST_DIR`, but a child of theirs with a cleared environment is not;
nor is one of a test binary run by its path, or a process reparented to launchd
or init (the recorder's watcher, double-forked, is one). Only a raw read
that opens no store (`cli::raw_client`, `search_bench`'s) is not refused there.
`tests/cli/main_only.rs` holds every entry to this on a scratch
repository. The rule holds from builds that carry it: a build of a commit before
it moves a live board as it always did.
`TASKS_DB` is the old SQLite cutover input, not a scratch selector. A branch's
build run by its path (`.target/debug/tasks`, `rs-bin.sh --build`) skips the
launcher's rule, though not the board's: run it only on a scratch board.
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
are recorded outside session by the native hook, with the checkout the agent's
last tool call ran in (`worktree`, `branch` and `placed_at` on
`callback_state:['binding', KEY]`, written by the beat only when they change,
and `stay_from`, lowered by a call there whose clock went back; on Linux
`placed_boot` and `last_boot` too, the clock since boot as the hook read it,
`sys/start.rs`), which the agents view reads as where it works and where it
looks, once a view and only for a quiet one, for a live process it waits on:
one working there that started between `placed_at` and its last tool call (a
start Linux dates since boot: between `placed_boot` and `last_boot` in the same
boot, never by a wall time), or `unknown` for one there whose start cannot be
read, falls between `stay_from` and `placed_at`, or is dated since boot with a
bounding reading missing (`cli/agents/place.rs`; a
long-lived reader keeps that listing on its store, dated when it ended,
`store/kept.rs`). They cannot
register callbacks or take over a handle that holds a native registration;
unclassified bound keys also fail closed until a full-session hook binds them.
An unclassified binding asks for a retry after the new tasks binary is linked;
a known in-process binding uses its board inbox. Reads use cached evidence,
never connect or retire registrations. A refused or full socket is retryable
while the stored process identity is alive; it does not lose its registration.
The sweep alone confirms presence (never a delivery
or an accepted Codex queue). Its STALE/LOST suppression lasts REAP_SECONDS plus
LIVE_SECONDS, across the next sweep. Sockets are checked even without hc;
Codex presence uses thread identities across every live hc terminal: a thread is
present while an open launch record puts it in a live codex terminal or its
registered terminal still runs codex, while terminal cleanup remains scoped to
this board's terminals (`sf.board`). Dry runs change neither board nor sweep files. A
Codex registration whose presence confirmation expired reads `unknown` and retains its work:
the presence sweep failed or is overdue, rather than having established the agent's absence.
That grace lasts at most ten minutes after confirmation expires; then ordinary heartbeat health
applies again, making unfinished work claimable without a beat. Each confirmed terminal resets it.
The next successful sweep confirms the terminal or retires the registration.
An incomplete terminal inventory still confirms its identified threads; it withholds retirement of
unmatched registrations until an inventory establishes absence. PULL lists unknown handles only
while they hold claimable work, just as the per-child warning does.
The shared server sweeps once on startup, then every five minutes. Each command has a four-minute
deadline and its owned process group is ended on timeout or shutdown, so the next sweep can run.
An unchanged callback snapshot drains its durable outbox without rescanning the inbox; its message
cursor advances only with the persisted observation and queued events.
Unknown observations keep the heartbeat health older binaries derive in `seen`, with separate
metadata restoring Unknown for newer readers. Both old and new writers must share that baseline
past the ten-minute lost threshold, or alternating dispatchers repeat liveness notices.
In the same way an orchestrator's `seen`, `statuses` and PULL entries keep the whole-board meaning
binaries older than one orchestrator per task read (`attach::legacy_seen`, every open status, every
digest entry); what it hears and was told lives in `narrowing`, `pull_withheld` and
`pull_awaiting`, which they ignore and keep, read over that record, so neither generation repeats
a NEW, LOST or PULL the other said.
An orchestrator's queued lines are judged at delivery by the test a dispatch makes of whom to tell
each one (`queued_lines`, a table of each kind and the `Seen`, `Owners`, `Overseen` or
`orchestration` call it shares with `attach::diff` and the pane watch), on its look now, so a
hand-over or a released claim meanwhile takes back what it would no longer be told: such a line
is dropped, and a LOST names only the work still told it. A queued PULL, whichever dispatcher
wrote it, is never judged by its text but by one rule (`pull_account`) over what the recipient
hears now, what the PULLs it acknowledged told it and what the PULL waiting tells: with nothing
heard beyond what was acknowledged it is taken back; an entry beyond both queues a new PULL, with
a key and retry budget of its own; otherwise it keeps its key and attempts and says the digest
as it is now. Whichever, the queued and event entries become the whole digest now, and what was
told changes only when a PULL is acknowledged: this generation's acknowledgement records what the
PULL left untold, and `pull_awaiting` reads an older one's, which records only its entries
(`pending_pull`, a table of each change while a PULL waits; `pull_acknowledgements`, each
generation's acknowledgement). A new kind of task-naming line takes its test from there too.
The contract while an older dispatcher interleaves: no entry the orchestrator hears is ever
lost, for one no PULL it acknowledged told it is counted by the next; no line is said twice,
but that work withheld from it, once a hand-over makes it its own, may be counted in one PULL
more, at most once for that hand-over; and nothing repeats on its own.
What was told is `pull_entries` but `pull_withheld` (`State::acknowledged`). This generation
records in `pull_entries` what was told and every entry the orchestrator does not hear, and
leaves out each entry it hears and was not told while the PULL telling it waits (`pull_queued`,
the whole digest, keeps older dispatchers from sending it). `pull_withheld` holds each entry it
does not hear and was not told from its first PULL on, since an older acknowledgement records
every entry of a PULL, the first one's too. An older write only ever takes entries out of
`pull_entries`, and every generation's acknowledgement puts in the entries of the PULL it
acknowledged, which told them. So an entry the orchestrator hears is told once it is there,
whoever acknowledged and whatever older dispatchers queue and acknowledge after, and untold
while it is not, even when an older one took this generation's PULL back unsaid for its own.
`revision`, which every older write moves, is not read. An inbox acknowledgement takes no lease,
nor does a presence sweep pruning what was delivered, so a dispatch writes its account only if
every PULL it read as waiting is still there undelivered (no key is used twice), and otherwise
reads and decides again.
What was withheld is the one ambiguity: an older dispatcher's own PULL tells the whole board,
and its acknowledgement of that PULL and of this generation's with the same entries leave the
same record; telling them apart needs the older binary changed. So withheld work stays untold
until a hand-over makes it the orchestrator's: then one PULL counts it, the one repeat when an
older PULL had told it, and once a PULL naming it is acknowledged it is in `pull_entries`, told
for good. A record naming an entry the PULL this generation left awaiting (`pull_awaiting`) did
not is an older one's own PULL acknowledged, which told every entry, withheld ones too.
Posts, verdicts and AGENT lines stay: they were said to whoever held the role then, and a new
holder hears no backlog.
What a PULL says is a separate question from whether one is sent. The first is the whole digest;
each later one this generation queues says what changed since the last one acknowledged
(`Digest::changes_since`): a baseline `pull_said` (`cli::pull::Said`: the counts, every entry
named, the sessions of unknown liveness), carried on the queued event and recorded by both
acknowledgement sites, in a field older binaries ignore, so no schema change. Rows added are the
entries heard and not told, the trigger's own test (`pull_account::told`), not those absent from
`pull_said`, so a row that left and came back is said once more, as it is pushed once more. An
older dispatcher's acknowledgement ends `pull_queued` but leaves `pull_awaiting`, and
`State::said` reads that as no baseline: the next PULL is the whole digest. One that keeps
`pull_awaiting` but not `pull_said` (built before PULLs said only what changed, as the shared
server may still be) ends both and records the PULL's entries; `pull_account::baseline` reads an
entry the recipient hears, recorded as told and not withheld, that `pull_said` does not name as
such a PULL, and the next PULL is the whole digest. What that cannot see, exactly: once every row
such a PULL told has left the recipient's digest before this generation's next PULL, that PULL is
measured against the baseline before it, so its header can say a count that moved before it
again, with the older `(was N)`, its liveness line can repeat or miss a change that PULL said, and
those rows are not said `resolved`. Rows added are exact, from `pull_entries`, which both record:
none is lost or said twice. The next PULL this generation's acknowledgement records is exact
again, and `tasks pull` says the whole digest meanwhile. `tasks pull` prints
the whole digest the recipient hears now (`pull::heard_now`) and writes nothing.
A channel registered anew starts from the whole digest: `callback_in` clears the PULL account
(`pull_entries`, `pull_said`, `pull_withheld`, `pull_awaiting`, `pull_queued`) and takes back a
waiting PULL when the address goes to or comes from none, or the listener's `owner` differs from
the one recorded; `pull_unanswered` stays, so a told post stays told. The same listener at a new
socket keeps the account, which is how a channel that waited out an outage hears what waited
(`hand_over_outage`, `pending_pull`).
A recipient is not told its own writes nor its children's staffing (`attach::diff_on`): its own
moves, a role move it wrote, the roles a session it registered (`parent`) takes or lets go, and
the move and readiness that child's claim or release makes. A quiet child is said once a health
transition, from the observation's stored health; one it hears as stale, lost or unknown is not
also listed idle (`pull::idle_and_busy`, which the dispatch, the legacy monitor and `tasks pull`
share, each passing the children its look follows).
`tasks wait` is the dispatch's own pass for one session with no channel (`Pass::Collect`,
`Store::collect_callbacks`): it plans as a delivering pass, takes the recipient's lease though no
address is registered, and its `Visit::send` prints what `deliver` would have sent, flushed, before
it is recorded as delivered by the same `record_callback_delivery` (a print that fails is a retry, as
a socket that refuses is, so a line is repeated and never lost), which records the PULL's
acknowledgement and the messages' deliveries. No other dispatcher visits a session without an
address, so its callback state is that reader's alone between reads; one with an address is
refused, since its channel takes the lines. A look also runs that session's pane watch
(`Store::watch_pane`, hc from the command's environment) and holds the chat seat when
`Store::chat_recipient` names it: with no channel able to, the live orchestrator first by handle
on `callback_state:chat`'s `waiting` list, which a session joins at its first look and leaves at
any `callback` registration. While nobody holds the seat, `Store::unheard_owner_events` gives
every orchestrator's `pull` and `inbox` what the owner said, and an inbox acknowledgement takes
it. The command beats for its session before a look, at
most every 20 s, as a monitor does, so the look reads the board its beat moved and the next poll
does not take its own beat for news. A look writes only to set its session up (its state row, the
waiting list) or to record what it printed: at a board that stood still it is nine reads and no
write (`pushes::a_waiting_session_looking_at_a_board_that_stood_still_writes_nothing`), and the
perf suite times one at scale (`timings`, `wait`); docs/TASKS.md says the cost per idle minute.
A finished idle terminal still closes under the original cleanup rules, retiring its
registration; an executor of a task in review is not finished, and keeps its terminal until it
has sat idle at its prompt an hour (`reap::IDLE_HOUR`), as every role on open work does.
An orchestrator below four canonical open slots receives a recipient-scoped `low_volume`
PULL condition, advising disjoint ready work or resolution of owned blockers. The existing
slot cap still governs starts. Each attach or callback pass shares one successful
`slot_holdings` observation across scheduling, recipient scoping and idle checks. A changed
canonical count invalidates an otherwise unchanged observation; a failed read reports the
error and preserves pending delivery. The condition uses the ordinary durable acknowledgement
account, so a count change within the low band does not repeat an acknowledged warning.

PULL's `unanswered` category lists a missing executor on `in_progress`, `blocked` or `review`,
or a missing reviewer on `review`, after an addressed post has waited thirty minutes. Each line
names the post and its restore command (or an explicit launch when no conversation was recorded).
The monitor and native stream emit that task/post once. Native acknowledgements retain those
identities in `callback_state.pull_unanswered`, separate from main's whole-board PULL account;
withheld or merely queued entries never enter that history. `store/unanswered.rs::unanswered_post`
defines pending work for the digest, restore selection and sweep: a post since that handle took
its role, without a later message, status move or verdict by it on that task or a descendant.
The descendant lookup reads only that recipient's eligible records after the post through
`message_sender`, then checks actual task parents. Direct `land-approve` approvals journal a
message through the existing board API. The final recovery reservation rechecks the recipient's
records and the parent edges used in its answer decision before reserving the attempt. Acquiring the
role also puts the predecessor's posts before the new holder's work, even at the same clock time.
Role aliases bind to the holders in the thread prefix at each post; a later departure or
replacement cannot transfer that request to another recorded conversation.
Handle-addressed requests retain the recipient's role from that history even without a launch
record. A deliberate role release answers older requests; an automatic departure does not.
Reading an inbox does not answer it. The periodic reap command reserves one durable recovery
attempt per handle/post and resumes the recorded conversation only after the hc terminal inventory
confirms its absence; it never starts a fresh conversation. An unsuccessful attempt remains
flagged for manual action.
A reviewer launched on an instruction child resumes that child's recorded conversation when the
unanswered post is on its parent; a departed reviewer explicitly named by handle still receives it.
An unanswered post keeps a terminal immediately, including through descendant sessions, for as
long as it waits. A role on open work keeps it until its agent has sat idle at its prompt
`reap::IDLE_HOUR` (the owner, 2026-11-08: "Only sessions doing active work or that will soon
(less than one hour) have active work should be kept open"): an executor `in_progress`,
`blocked` or in `review`, a reviewer of work in review, and a reviewer whose changes verdict
returned the task to `in_progress`, awaiting its re-review. That includes landing changes posted
as a message to the executor before it moves the task back. The hour starts again when the role
or its task's status changes, and runs from the agent's last tool call (`last_tool`) when that is
later. The close's line names what brings it back: a reviewer launch for the next tip resumes a
reviewer, a changes verdict or a post restores an executor (`restore_unanswered`).
A post or changes verdict to `@executor`, or a post to the executor by handle, that no
executor can read (no callback, and its launch record closed or no live beat) adds a
`board` post to `@orchestrator` on the
thread after the write commits (`store/messages.rs`, `tell_if_unread`); its failure is
said on stderr, never turned into a failed write.
A post under a key (`Store::say_keyed`; `tasks api` `say` with `key`) is made once per key on
a thread: its transaction makes the first post's line and `delivery_key:[slug, key]`, and
answers that line to every later post under the key, writing nothing, so a caller that may have
posted without reading its answer repeats the call rather than reading the thread, and a request
of its that outlived it cannot post twice. Asked to adopt (`Store::adopt_or_say`; `say` with
`adopt`), a key naming no line yet takes the thread's earliest line reading exactly the text to
the recipient, one made under no key, and writes nothing. The board's notice that no executor
reads a line is itself under the line's key (`unread:<id>`, taking a notice of the line made under
no key), so a post repeated under a key, or one that took a line, says it when nothing said so
yet, and never twice. The copy carries the keys with the board.
Restore prompts re-register the resumed process's callback first.
Neither channel checks nor delivery fabricate tool calls.
Outbox rows are indexed by handle
and delivery state, with a numeric sequence for ordering; chat retries belong
only to the current recipient. Transactional mutation events advance `counter:board_structure` for structural
inputs, callback registration/observation changes and owner chat. Session mutations instead advance
their own `session_revision` rows; authorization and liveness guards write those rows too, without
advancing the mutation count or heartbeat. Readers use the combined `schema::BOARD_MARKER`.
The schema16 migration preserves the structural count and retires the legacy scalar
`counter:board` at the minimum signed integer, which old unsigned cached readers refuse.
The version floor also refuses newly-opening schema14 readers. Beats advance only their
per-session revisions. A Store caches the raw board
snapshot together with that marker, read in one transaction, and rebuilds its derived health,
presence grace and reservation state against the current clock. The Symphony server uses those
cached inputs even without a callback recipient; machine/test probes and the agents view still
run each tick. Unchanged live observations skip planning until the periodic PULL check; health
changes invalidate that shortcut, and due outbox retries are checked every tick.
A command line's write observes before it answers and leaves the delivery to a detached
dispatcher: the `tasks` binary (`cli::main_detaching`) gives its stores a hand-off
(`Store::dispatching_by`). Every build of `tasks` is `cli::process_main` and nothing else, this
crate's and the containing project's board-only one alike (the one the live board runs): a
`main` of its own that calls `cli::main` delivers in every write and starts no dispatcher
(`tests/server/dispatch.rs` runs the binary in the live board's shape). The write still makes
the observation every write made (`Pass::Observe`), queuing in the outbox what each recipient
on its host hears, read against the board as the write left it: a hand-over or a move before
the delivery neither reroutes nor folds
it, and a callback registration takes its first look before it answers. It then starts
`tasks dispatch` of the same executable detached, in a session of its own (`cli/detached.rs`;
its lines go to `sf-tasks/dispatch.log` beside the repository's own board, nowhere for a
`TASKS_SERVER` board), which delivers. One dispatcher runs a host at a time
(`store/dispatcher.rs`, `Store::dispatch_detached`), under the row
`callback_lease:['dispatch', HOST]` (60 s, renewed on each pass): one that finds it held counts an
ask (`asked`) and waits until a pass begun after it (`served`) has ended (`completed`), or until
the holder's lease lapses, when it takes the host over, so the asks of a holder lost before or
during that pass are not lost; a new holder alone is not that pass. The holder passes
until no ask stands and every recipient was reached (one whose lease another dispatcher held is
passed again, for up to a recipient's lease), so 40 writes at once make a few passes, not 40.
Each host's dispatchers keep a row of their own, so none takes an ask for recipients it cannot
reach. A holder whose lease lapsed and was taken ends after its pass and leaves the row to the new
holder. A hand-off that cannot start the process delivers in the write, as every library store
does (the Symphony server's, the tests', `cli::main` in-process), and so does the binary under
`TASKS_DISPATCH=inline`, for a test that reads what a command delivered once it has ended. A pass
reads the delivered messages after the lowest recipient cursor once (`store/pool.rs`) and each
recipient's inbox filters that pool, looking its deliveries up by key, where each recipient
scanned the message and delivery tables itself; the plan before a recipient's lease no longer
reads its inbox, since one it would read always goes on to the lease, which reads it. A
dispatcher's own writes (an observation, a delivery) read the board marker before and after:
when nothing else moved it between, its cached snapshot still holds (`adopt`), since a dispatcher
writes nothing a snapshot reads, and the next recipient's leased read is the marker alone, not
the whole board again. Under each recipient's lease a delivering pass otherwise reads the board
and its account again, and its final transaction still re-decides when a PULL it read as waiting
was acknowledged or pruned. What the pass built under the lease is delivered however long the
build took. The delivery renews the lease in the read of what is due (`Store::deliver_pending`)
and begins no batch more than 30 s after that, by the board's clock, the one the lease's `until`
is read by. A dispatcher whose lease lapsed and was taken while it built (the lease's revision
moved) writes no account and sends nothing, so two never send one line, and the recipient
counts as not reached (`tests/store/slow_builds.rs`). A write's observation reads the account
again but keeps the board it read once it committed, and leaves a recipient whose recorded
observation is at or past that board's marker: that one read a board holding the write, and reading
the whole board again under every lease is what made 40 writers at once take minutes. It also
leaves, without its lease, a recipient whose look moved by the newest message alone and whose inbox
no message since its cursor could reach (`Store::could_hear` tests the pass's pool on the client:
to it, to a role, from the owner, a verdict): the write gave it nothing to hear, its recorded look
and cursors stay as when another dispatcher held its lease, and the delivering pass moves them. A
read of one statement (a plan's stored state, the marker, the pending check, the registrations, a
waiting dispatcher's look at the lease) is answered outside an explicit transaction
(`Store::read`): one statement reads one snapshot on its own, and every commit syncs the store,
about 5 ms on the board's server even when nothing was written, paid for each recipient of each
write. The inbox fetches the orchestrators' sessions by id from their roles: `session WHERE handle
IN (SELECT ... FROM role)` runs the role scan again for each session, seconds on a board of
thousands of each, for every inbox a message could reach, which is what made one write on a board
that size take minutes. A diff reads the confirmed registrations only for a child that went quiet
since the last look (`attach::events::quiet_children`). `examples/dispatch_load.rs` measures 40
concurrent `tasks say` against a scratch database seeded from a dump of the live board.
Dispatch plans before leasing and refreshes its inputs and observation again after acquiring
the small `callback_lease` record. Its revision identifies the holder, including on release;
taking or releasing it never rewrites `seen` or `statuses` or allocates a counter. Lease rows
survive retirement and pruning so a revision is never reused. A completed periodic PULL check
records its time in that small row even when the digest adds no entry, rearming the deadline
across fresh Stores without moving the board marker. A tick with no changed observation, due
delivery or due periodic check writes nothing. The clock's tick, never a write's dispatch, also
watches terminals (`attach::pane_watch`): while a recipient oversees a session with an open launch
record, it lists hc's live terminals (`hc ls`) once a tick and queues BLOCKED once an episode
(status `blocked`), or WATCH once while hc cannot be asked; what it last said is
`callback_state.pane_watch`, off the board marker.
It looks before the tick's dispatch, and the tick's PULL view and the clock's agents view read
its sample (`Store::clock_panes`), so a tick asks hc once. It plans from the stored
`pane_watch` and looks again under the lease, so two clocks on one board say an episode once.
The legacy monitor runs the same watch on its own looks. Observation state is checkpointed
with newly queued events, before transport. Finished tasks leave its status, role and verdict
maps after their final diff; messages and pending events retain their durable cursors and outbox.
One board transition reopens a finished task: `done` → `review` (`Store::move_to`), for an
approved task no landing closed, moved by its executor, reviewer or orchestrator, whose tip the
default branch does not hold (`land::reopenable`, which the command line and the API run against
the repository, refusing from outside one). Its status left the maps, so no STATUS diff says
it: the move itself posts a board line to `@executor` and one to `@reviewer`, which reach their
inbox and PULL digest though their panes are closed, the one to `@executor` passing to
`@orchestrator` while no executor is live, and the orchestrator restaffs. The command line's
move starts `[board] on_review` (`cli/commands/finish.rs`), as does the land's own reopen
(`land::Landing::refuse`). A hand-edited reopen has no previous status to diff: an
orchestrator can hear NEW for ready work or a new PULL entry, but no recipient hears STATUS for
that hand edit, and a worker receives no reopen notification.
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
Cargo replaces `debug/NAME` with a new copy on every build, even one that
relinked nothing, and macOS assesses each new executable file at its first run,
for seconds. So `tools/rs-bin.sh` runs `rs-bin/NAME` in the target folder, a
hard link to that copy renewed only when the copy's modification time or size
moved (the copy keeps the time of the build it copies): a no-op build runs the
file the last run ran. `--build` prints that path, `--link` points
`.target/bin/NAME` at it by a rename (the link the heartbeat hook runs, building
nothing), and `--where` names Cargo's own copy without building. A
`.target/bin/NAME` link to Cargo's copy is pointed at `rs-bin/NAME` by the next
run of that binary; nothing else changes `.target/bin/`. The native `tasks api`
JSON interface (`src/cli/api.rs`) takes a call on stdin; `tasks api --lines` takes
one JSON call per line on stdin and answers each on its own line (a line that
is a list of calls gets a list of answers), the methods being the store's, plus
`threads` and `snapshot`, which read several things in one round trip.
`notify` authorizes a tool's actor and posts an addressed `board` message to a
literal session handle on its task (neither `owner` nor a role). It reaches that
session's inbox even when the actor is the recipient; ordinary `say` posts retain
their agent author and self-post filtering.
`launch`, `restore`, `cleanup`, `clean`, `close`, `finish` and `reap` (the Symphony's idle
sweep, every five minutes) drive hc's terminals and git's worktrees from
`src/launch/`, through `launch::hc::Hc` alone; the crate calls no `herdr`.
A launch is one `hc run --bg --no-parent --outlive-parent --keep` of the
agent's own command line, with no message (no child of the caller's terminal,
which the app would show under it rather than in a tab), labelled `sf.board` (the main checkout's folder and the board's
database), `sf.handle`, and `herdr.space`/`herdr.tab`: the stream tab (`code`,
`review`, `3d`, `infra`) of the project's space in the Herdr app, found by name
(`herdr-app ls`, else the layout `hc connect` keeps) and made when missing,
never an owner's tab or another project's terminal. Once `hc get` calls the
agent `idle` or `done`, its first message goes in by `hc send`, never
`--force`, and only that send's `started: true` confirms the launch
(`launch/startup.rs`): hc records `first_message` for template runs alone. An
agent blocked on a person (Claude's folder-trust dialog), never ready, ended,
gone, or whose send hc refuses, does not start, or cannot tell of, fails the
launch naming the terminal, which is killed and removed; only Codex's hooks
chooser is answered on the way. Codex runs with `--no-daemon`; its configured
REPL helper (`procs::CODEX_REPL`) and the ChatGPT app's computer-use REPL
(`procs::CODEX_CUA_NODE` running `procs::CODEX_CUA_REPL`), each by that exact
path, and the code-mode host Codex 0.167.0 on runs under each session
(`procs::CODEX_CODE_MODE_HOST` in `FOLDER/.codex/packages/standalone/releases/VERSION/bin/`,
for any absolute `FOLDER`, not only `$HOME`, and any one `VERSION` folder, no part of
the path empty, `.` or `..`; never the bare name or another layout), are not work
under it; a `codex exec` job is.
Cleanup closes a live terminal only while its foreground is the program it
started, and for no agent a shell (`hc get`'s `foreground`: a job a shell runs
in front, or one it became by `exec`, is work). A
close is `hc kill` then `hc rm`, the remove waiting, by `hc get`, up to ten seconds for a killed
program to end (hc refuses to remove a live terminal) and never tried while the last look found it
live or could not say: one still live then is left killed, for the next sweep to remove once it
finds the program ended. A close is never of the caller's terminal
(`HC_TERMINAL_ID`) or session, an orchestrator's or a chat's, nor of one the owner may be looking at (`launch/screen.rs`,
from `herdr-app status`: its `herdr.tab` label the tab the window shows, the
agent it shows, or held by an overlay or float); cleanup, finish and the idle
sweep read the window last, after the close's own `hc get` and after the idle
sweep records the closure, right before the kill (`screen::close_unseen`). That
`hc get` must still find the terminal finished (an agent idle or done, the same
program: its pid, and for cleanup its foreground, which `exec` changes under one
pid, and nothing under it its busy classifier counts as work), and a window
that cannot say (the call fails, it reports errors, or an overlay or float names
no terminal) keeps the terminal; either keeps it, the sweep's record reopened. The idle sweep's
last looks read the board again too (`reap/closing.rs`): its agent's last tool call moved since
the sweep read it (`reap::TOOL_CALL`), or what holds it changed, a post addressed to it say, keeps
it, its record reopened. A record
naming a herdr pane (`w1:p…`) is a gone terminal: cleanup records it closed
with its restore line, and restore brings it back in an hc terminal. `close SLUG HANDLE`
(`launch/close.rs`) closes one agent's terminal whatever role it holds, judged as cleanup judges
a program (its foreground, `Running` at the last look) and as the idle sweep judges what runs
under an agent (`reap_busy_reason`), out of the owner's sight. A close and a restore of one
session each hold it for their whole run (`Store::take_session`, the row
`callback_lease:['session', HANDLE]`, 300 s, a close renewing it at its last look, a restore
before it starts the terminal), so neither starts while the other runs, nor a second close, and
one stopped part way holds it until it lapses. The hold's revision fences every write and
retirement either makes (`HOLDS` in `store/launches.rs`, checked in the write's own
transaction, which also writes the hold's row: the board's transactions are optimistic and
conflict only on rows both write, so a takeover committing first fails the write, and tried again
it is refused): once another took the session over, a close writes nothing more (the next finds the
terminal gone), and a restore kills the terminal it started and records nothing.
The close writes the closure only once the terminal is killed, bound to the record read
(`close_task_session_as`, `record_found_closed`), so a close kept or refused writes nothing and a
callback registered before the kill completes the record's thread. It then retires the
registration from that terminal, or the very one it read before (from no terminal):
`clear_callback` retires a registration only as it was read (address, listener, terminal or none,
`registration_revision`, and `registration_nonce`, a random id each registration writes), never
one registered since, though it took the same number after a retirement deleted the row. Its one
limit: a row an older build (which writes no nonce) makes again just as the one read was (the
same address, listener and terminal, numbered the same) cannot be told from it, and a close that
read that one may retire it. A Codex record written `codex:`
takes the thread the handle's callback
registered, at its launch or its close (`store/launches.rs`), so the order of the callback and
the record never loses it. Its tests' handles:
`TASKS_SERVER` and `TASKS_DATABASE` reach a board of the test's own
(`tasks.Server`, one in-memory server per test process, a database per test);
`TASKS_NOW` pins the clock, and a monitor's waits step it (each says `STEP
<now>` and reads a line); `TASKS_PULL_SECONDS` sets how often an
orchestrator's monitor computes its PULL digest (only a new actionable entry
prints one); `TASKS_REAP_STATE` is where the
idle sweep keeps what it saw; `HC_BIN` and `HERDR_APP_BIN` name a fake hc
and window (the crate's `fake-hc` binary, its terminals, the window's layout
and what the window shows (`screen`) a JSON file `FAKE_HC_STATE` names, each
call a line of `FAKE_HC_LOG`, and the calls `FAKE_HC_FAIL` names refused);
`TASKS_PS_TABLE` names a file of `ps -axo pid=,ppid=,command=` lines the commands
read in place of `ps` (`procs::table`), so no command-line case reads this
machine's process table; it is read only with a `TASKS_DATABASE` other than the
live one, never on the live board. A table, the file's or `ps`'s, that lists no
row for the terminal's process says nothing of it. A terminal the board would
close whose `hc get`, table or window cannot be read stays, and cleanup and
finish print `kept pane T [of H] (never closed on no information: WHY)`; one on
screen prints `kept pane T [of H] (on screen in the Herdr app)`.
The idle sweep (`launch/reap.rs`) removes each terminal of the board whose program ended at the
first sweep that finds it, out of the owner's sight, its open records closed first with their
restore line (`its program ended`); one whose records are closed already goes without a line.
It holds each record's session while it writes them, as a close or a restore does, and writes each
only as it read it, the same launch in that terminal (`reap/ended.rs`): a record a restore moved to
another terminal since is never closed for this one's end, and a restore under way keeps the
terminal until the next sweep. It returns every terminal of the board it leaves open with its reason (`Sweep::kept`), and
`reap.json` keeps the reason last said of each idle one, so `reap` writes
`· idle sweep keeps pane T of H (WHY)` to its stderr, the Symphony's log, once per change of
reason; `reap --dry-run` prints `keeps pane …` for every one. `clean` (`just clean`) runs
cleanup for its caller (`--as`, `cleanup_as`) and then one sweep, under their own rules, and
prints each terminal it closed (`closed pane T of H (WHY); restore: …`) and why each one it left
open stays (`kept pane T of H (WHY)`); with `--dry-run`, what it would close. Only a terminal
cleanup removed (or would) goes unsaid by the sweep: a record cleanup closes removes no terminal,
so an ended one on screen is still said kept, and why.

A child's input and output go through `sys::output` (or `sys::spawn_output`,
`sys::spawn_piped` for one that reads stdin, and `sys::wait_output`), never
`Command::output` or `Stdio::piped`, here and in the sources of `symphony`
and `symphony-server`; but in `store/codex_delivery.rs`, whose proxy is
answered line by line and killed, never read to its end. `port::listens` is
handed what runs its `lsof`: `sys::output` from those sources, `Command::output`
from `apps/symphony-ui`'s browser test, which compiles `port.rs` alone. On macOS std
makes a pipe or socket pair and sets close-on-exec in a second call, so a
spawn on another thread between the two keeps the writer and holds the
reader's EOF off for that child's whole life: an hc answer then reads as
unanswered. `sys::pipe` is a real pipe, its ends reopened close-on-exec and
close-on-fork under `sys::spawning()`, so no spawn under that lock copies them;
an end reaches a child only through a `sys::Command` (a std spawn handed one
cannot start). Not a FIFO, which macOS opens close-on-exec in one call: a read
already blocked on one can miss its end there, and `poll` never sees it.
Those helpers take a `sys::Command`, std's builder by its method names plus
`new_session`, `pass_fd`, `default_signal` (a signal at its default in the
child, though this process ignores it, which a test that ends its child by a
signal sets, since a background job inherits SIGINT and SIGQUIT ignored) and
`default_signals` (`default_signal` for every signal a process may catch, and none
blocked: the nightly's checks), and so does every spawn in a session of its own
(`sys::new_session` is gone). std takes its fork path for a `pre_exec`, a
session or a changed environment with a bare name, and that path's exec-status
pipe has the same gap, so a racing spawn held `spawn()` itself; on macOS a
`sys::Command` spawns with `posix_spawn` and `POSIX_SPAWN_CLOEXEC_DEFAULT`: no
status pipe, and the child inherits only its streams and the descriptors it is
passed. A std `Command` cannot say it cleared its environment, its group or its
session, so it is never translated. The helpers live in the leaf
`process_cwd::spawn`, which `sys` re-exports, so the cwd scans' `lsof` shares them.
Every child the board starts is built by `sys::command` (this process's
environment) or `sys::command_with` (a caller's `Env` as its whole environment),
both without the starter's session, `sys::SESSION_VARS`: `TASKS_SESSION`,
`HC_TERMINAL_ID`, `HC_TERMINAL_SOCKET`, `HC_SESSION` and `HC_SCOPE`. A child that
kept hc's would start its terminals under the starter's, which end with it, and
an hc call would answer for the starter's core and scope alone. That covers the
database server `Home` starts, its `ps` and `lsof`, the `surreal export` and
`import` a reclaim runs, every hc call (each names the
terminal it acts on; the caller's own is read from the `Env`, never handed to
hc), git, the detached dispatcher, sweep and review command, the sweep's cwd
scans (`process_cwd::process_cwds_with` and `occupied_with`, handed
`sys::command("lsof")`), and Codex delivery.
A call that must keep a name sets it on the command after, saying why: `tasks
run` gives the command it wraps its caller's session back, since that command
is the caller's own and its board commands and hc calls need it. A launched
agent's `TASKS_SESSION` is a word of the command line `hc run` starts, not
hc's. Clippy holds it: the crate's own `clippy.toml`, the
workspace's settings whole and then `disallowed-methods`, refuses std's and
`process_cwd`'s `Command::new` and `env_clear`, and `process_cwd::process_cwds`
and `occupied`, by the path they resolve to, however they are spelled, imported
or gated; the two helpers carry `#[expect(clippy::disallowed_methods)]`. The
library's test build and each integration test target allow it at their root.
`sys`'s tests hold the file to the workspace's settings, and the nightly-only
target `tests/child_guard` (a nested build, so never in the fast plan) runs the
toolchain's clippy over a probe crate that reads the same file. `port.rs` starts nothing: std's `lsof` for its `Lsof` is
`apps/symphony-ui`'s browser test's own. The Symphony's
`children` re-exports the list, `command` and `sessionless`.

`staff` makes one staffing pass (`src/launch/staff.rs`), also called before `reap`.
Its sole config is `tools/task_staff.json` in the calling checkout, disabled by
default; its pass lock is in the common `sf-tasks/`. A launch, the CLI's and staffing's, starts
the agent, model and effort `[agents] launch` in the main checkout's `symphony.toml` names
(`src/launch/defaults.rs`), else Codex on `gpt-6.1-sol` at `xhigh`; `--kind` and `--model` win,
and the project's model and effort go with its agent only. The legacy `claude_session` storage
column keeps Claude ids as before and Codex ids prefixed with `codex:`; restore dispatches on
that prefix, and the agents API strips it from the conversation id.

`tasks sweep` is the worktree sweep (`src/sweep.rs`, its rules in its module
doc and [tools/AGENTS.md](../../../tools/AGENTS.md#landing-on-main)): `git.rs` its
git reads, `process.rs` the locks' pids (`kill(pid, 0)`, `ps`); the leaf
`crates/process-cwd/` owns the bounded `lsof` scan shared with the cargo sweep.
`coalesce.rs` appends requests before trying the sweep lock: a busy caller exits
without opening the board, and the holder serves requests received during its
pass in another pass. `verdicts.rs` reuses positive retention evidence by HEAD,
index, folder and witness metadata (mode and change time included), plus ancestor
ignore policies and shared/global ignore inputs. Supported empty verdicts reuse a
bounded complete metadata inventory, including tracked exceptions under output cuts.
Warm reuse stats saved witnesses or inventory entries and their policy files without
walking the checkout. Unknown, oversized or unstable inputs conservatively rescan;
conditional/per-worktree configuration, attributes and unreadable policy decline reuse.
Fresh unavailable HEAD/removal guards retain a tree before lookup; eligible removal
holds its removal lease through both fresh final status guards and deletion. Regenerable `.target/`,
`symphony/.target/` (and a seed's or a clear's staging beside each, `.target.seed.PID`,
`.target.clear.PID`), `symphony/build/` and `.codex/hooks*` go with a merged tree;
`artifacts/<handle>/` keeps it. Foreground `--task` visits only that task's
finished worktrees and asks for a detached full pass. `sweep/build_output.rs` clears
those build folders, the worktree staying, from a worktree the sweep keeps once its
work is finished (its rules in its module doc): every tied task done or cancelled, one
done; no beat or tool call by an agent bound there, and no touch of its git files,
within the hour; its HEAD merged work, not a commit of main's first-parent line unless
its reflog says it was committed there, so a branch with no commits ahead of main
keeps its build; no process inside. Right before each removal it decides again on a new
listing and board read and asks `occupied` again; then it takes every Cargo build lock in
the folder without waiting (one held keeps it), renames the folder aside under them and
removes that. It never follows a link out of the worktree, and a stale staging whose
process is gone goes with the folder (`tests/sweep/build_output.rs`).
`trigger.rs` owns
the detached sweeps work finishing starts (`done`, `move`,
`verdict`, `leave`, `reap`, and `inbox` once an hour). Those run the main
checkout's build of `tasks` as the recipes there find it (`builds::of_main`: its
`.target/bin/tasks` link, else `tasks` on PATH), never the caller's, a linked
worktree's link included: a worktree with sweep code that has not landed must not
run it on the repository. With no such build the log says so and nothing sweeps.
The Symphony's reap and its finish of a cancelled task run that build too
(`front::board_build`). A
board `TASKS_SERVER` names starts none unless `TASKS_SWEEP=on`, so the CLI's
tests never sweep this repository; the sweep's own tests (`tests/sweep/`) build
real repositories in a temporary folder, and spawn the binary `TASKS_BIN` names
for the triggers, which stands in for the main checkout's build only on a
board `TASKS_SERVER` names.
`on_review.rs` owns the command a task moving to review starts: `[board]
on_review` in the main checkout's `symphony.toml`, read by the board itself
there (the default is none), run detached from the main checkout with the
task's slug, its branch (the slug, or what follows a `review/` or `review-N/`
prefix) and that branch's tip appended, its output appended to
`sf-tasks/on-review.log`; a slug naming no branch starts nothing and the log
says so. A board `TASKS_SERVER` names starts none unless `TASKS_ON_REVIEW=on`,
and `off` starts none anywhere (`tests/sweep/on_review.rs`).
`land.rs` is `tasks land`, the Symphony's default `just land SLUG` (its steps in its
module doc and [TASKS](../../docs/TASKS.md#the-default-landing)): it reads its
settings (`[project] default_branch`, `[agents] worktrees`, `[verdicts] check`,
`[land] generators`, `push` and `timeout_seconds`, any other `[land]` key refused)
from the default branch as the main checkout has it committed. `land::admit` lets it
run only under the `land` recipe's `SYMPHONY_LAND` and never where the project's
justfile runs a `land` of its own (`just --show land`): a guard against accident, the
approval check being the gate. It goes by the last verdict's `approve`
(`store::approved`) only when a session that never executed the task or a work subtask
of it at any depth gave it (an instruction subtask's executor, the reviewer that claimed
`SLUG/review`, executed none: the `instruction` flag says so), nor one that orchestrates
or ever orchestrated it (`Store::ever_held`, a `Held`), and not its filer; on a fast fix (`fast_fix.rs`, the `_board.fast_fix` mark
`create` writes) the orchestrator's verdict counts unless it executed the task, when a
fresh reviewer's approve lands it. The approve covers the tip by name, else by time
within the review; one that does not do sends the task back to review (`Store::move_to`
takes an approved, unlanded task from done to review), a conflict or a red is said to
`@executor`. It merges and checks in a worktree of its own locked with the reason
`symphony land: SLUG (pid N)` (so the sweep keeps it, and a later land replaces it
only once N is gone and `lsof` finds no process in it), moves the main checkout only by
a fast-forward to what passed, after writing a `land::Record` of the merge
(`sf-tasks/land/SLUG.merged`) and saying it on the thread, pushes the checked commit by
name, closes the task through `landed` (from `done`: a task sent back to review since
stays there), drops the record and runs the sweep for it. A tip on the default branch
is finished without merging again only by that record (its tip, its merge's second
parent, its checked commit on the default branch), pushing the recorded checked
commit; with none it is merged outside land, and refused. Its children are
`sys::command_with` of the caller's `Env`, with none of git's repository variables
nor the marker, and with `TASKS_SESSION` and `TASKS_TASK` naming the land, so what the
check builds is recorded for the task; each generator, check, fetch and push runs in a
process group of its own, named in `sf-tasks/land/running` while it runs (a later land
refuses while that group lives), which the timeout ends whole (`sys::end_group`), a
fetch or push within five minutes. SIGINT, SIGTERM and SIGHUP reach the land alone (a
`sys::Interrupts` scope records them): its watchdog ends the running group, the land
stops at its next step with one line, its worktree removed, and the command line ends
by the same signal (`land::interrupted`, `sys::end_by`). Its tests
(`tests/sweep/land.rs`, `land_approval.rs`, `land_limits.rs`, `land_signals.rs`, the
last running the `tasks` binary and signalling it) land in scratch repositories on a
board of their own and push to a scratch bare remote.
`slots.rs` owns the cap on an orchestrator's open tasks: `[board]
orchestrator_slots`, read from the main checkout's `symphony.toml` from the
`.git` files alone (no `git` process: every command reads it) when the CLI or
the Symphony opens its store (`cli::capped`, `Store::capping`); a store opened
without it caps nothing. A rule that may start work reads, in its view's
transaction (`Which::Starting`, `store/cap.rs`), the slot counters, the open
tasks through `task_status`, the holds (`counter:['hold', SLUG]`) and their
tasks, and their roles through `role_slug`; then the tasks above them and the
tasks they review by id, a level a round trip. It decides on the board before
and after the change (`slots::started`) and, for every orchestrator whose open
tasks it changes, checks and bumps `counter:['slots', HANDLE]` in the write, so
two starts at once are one retry, not two slots (`tests/store/slots.rs` and its
`moves` and `cost` modules, `tests/cli/slots.rs`). Red work takes no slot: an open task
`create`'s `red` marks (`fields._board.red`, `red.rs`, api 7, which the cap's reads
project beside each task) and the work under it, judged by the task whose work it is;
a refusal names it apart, and PULL lists a red nobody executes 10 minutes after filing
first, as `red_unstaffed`, before any cancelled dependency of its. Only a session of
kind `filer` marks, on `create` or later with `mark_red` (`store/work/red.rs`, whose
checks are read in its write's transaction), only a series' own task
(`red::filed`, never one named under it: the nightly's board and land's standing reds
judge by the same `red::filed_in`), and `set_field` refuses the board's own `_board`
fields. A red's close is never refused: in its write, on a store capping starts or
not, it holds the slot counters of the owners whose counts it raises
(`slots::changed`) and `counter:['red', SLUG]`, which every start of work under the
open red holds too (`slots::reds`, both in `slots/change.rs`), so a start decided
before the close is decided again (`tests/store/slots/moves.rs`). The owner's cancel,
which reads the cap's board before its own write, fences that write on the task's
version as read (`cap::FENCE`), so a red marked between them is read and decided again, and
refuses a task its read did not find. `unmark_red` (`store/work/red.rs`, a filer's) lifts an
open red's mark, read, fenced and held as a close (`cap::unmarking_slots`,
`tests/store/slots/red_marks.rs`). A change of parent of open
work and a hand-over of its orchestrator role are decided the same way
(`set_parent`, `hand_over`), but for the owner's. A hold (`Store::hold_blocked`,
`tasks api`'s `hold` from api 4, land's while stamps wait) keeps blocked work's slot: it is a record, not a task
field, since a reader built before it refuses a task row with a field it does
not know, and every write that moves the task, changes its parent or its
dependencies deletes it. Each statement the cap sends is in `SLOT_READS`, which
`tests/store/slots/cost.rs` has the server plan.
`alarm.rs` owns the pipeline alarm PULL says: `[board] review_alarm_ratio`,
`review_alarm_hours` and `review_alarm_clear_hours`, read as the cap is
(`cli::capped`, `Store::alarming`). It writes only where rework ended (below):
each digest reads the
status lines of the lookback window through `message_ts` (`store/history.rs`),
walks each task's back from its status now and replays the counts
(`cli/pull/pipeline.rs`). What held when the window opened is read, never
assumed: a task's newest lines before it, from its own thread's key range,
read back a page at a time past any field value that only reads like one
(`Store::lines_before`), where its status or record then rests on them
(`alarm::rests_on_before`), and the alarm's state then by replaying a window
twice as long while the replays from raised and from lowered disagree, back to
the board's first task. Which work a mirror or an instruction subtask counted
with is the tree as it stood then (`alarm/tree.rs`): each task from when it was
made, each instruction under the parent its moves' lines left it (`set_parent`
writes the parent left, `(was PARENT)`). What a
task's landing waits on is the landing tool's record in `fields._board.waits`
(`TIP: name, …`, `Store::record_waits`, given the assessment it states, kept
with its time and unique thread sequence in `fields._board.assessed`: the
mutation transaction retains committed assessment/text pairs in
`fields._board.assessments`, so retries, including older receipts after a newer
assessment, write nothing. A fresh assessment writes its own line whatever its
text), judged against the branch's tip (`cli::probes::tip`); a
record naming `changes` of the tip now, that the branch has not answered since
(the same `Fixes` as the alarm's), makes review rework, counted in
progress, and in the replay each record stands to the next, or to when the
branch first moved past the tip it judged after the record was written
(`alarm::reworked`, whatever the task's status now, `cli/pull/pipeline.rs`
`Fixes`; `tests/cli/pull/alarm/rework.rs`). The reader asks the branch's
reference log for the commit it first moved to from that tip after then (the
next made on it, what amended or rebased it away, or, for a tip it came back
to, the move after that), else the branch's line of first parents, where only
a commit whose first parent is that tip follows it (`cli::probes::tip`,
`tests/cli/probes/tips.rs`). The first answer of each record, by its tip and
unique thread sequence (`line:SID`), is kept in `fields._board.fixed`
(`Store::record_fixed`,
first write wins, with no line, and no new version to fence a concurrent move
as stale) and read back before asking again (`alarm::kept_fix`), so no later
commit, amend, rebase or reference log moves it, and a new record of changes
on a tip the branch came back to waits on a move of its own, in PULL and the
alarm alike, while the record answered, retried, asks no rework of either.
Historical timestamp-keyed ends are adopted only when the thread proves that
the timestamp names one occurrence of that tip; ambiguous or malformed evidence
supplies no saved end. The digest cache and replay boundaries retain the same
thread identity, even when independent assessments share a timestamp.

The production publisher uses API9's typed `assessment_state` and
`record_waits` operations (`assessment/1`). The former reads a task's status
and version; the checked mutation requires that pair atomically for a fresh
write. A committed assessment/text retry returns its original positive thread
sequence and time, even after task state moved, with `current: false` when a
newer assessment stands. That historical receipt confirms the occurrence,
never a current assessment. The guarded `assessment/2` route also validates
the immutable publication, original task/event binding, fence and verification
against the actual caller's repository and explicitly selected board context.
Its echoed publication witness and canonical receipt must match exactly;
the numeric API version alone proves no endpoint capability. Unknown methods,
incompatible payloads and unconfirmed replies remain pending in the landing
publisher's local journal.

A move that names where it goes from (`Store::move_from`, `tasks api`'s `move`
with `from` from api 6, `store/work/moves.rs`) is refused unless the task's
status, as the move's own write reads it, is one of those: a caller that decided
the move on an earlier read (land's changes verdict sending work in review back)
leaves a status another move set since, blocked say, as it is.

Its store tests start one in-memory server per test binary and give each test a
database of its own. The server is the pinned SurrealDB `tools/surreal.sh`
fetches once per machine into `sf-surreal/` in the git common dir, refused
unless its SHA-256 is the one pinned there; `SF_SURREAL` names another binary.
A project, which has no such script, fetches the same tarball by the same pin
with the line the board's refusal and `symphony init`'s Next give
(`server::surreal_fetch`; `server/fetch_tests.rs` holds its pins to the script's).
The client speaks the RPC in CBOR, never JSON, whose variables lose any string
that starts like a record id, a datetime or a UUID. A request written on a
connection that then gave no answer may have run, so a write is never sent
again; a request whose text proves it a read (`rpc::Kind::of`: only `SELECT`,
`RETURN`, `LET`, `INFO` and transaction statements, no write word anywhere,
and every `::` a call in a namespace that only computes, its word right against
the `::`: `fn ::bump()` is a write) goes once more on a new connection, never a
third time. A store opens a board at its build's schema version on one such
read of the version, defining and writing nothing, so a page's open over a slow
board goes once more as its read does. Only a writer (`Store::open`,
`Store::with`) defines a board, and only one not made yet or behind its build:
its namespace and database first when the server lacks either, then its tables
and version in one transaction (`tests/store/open_read.rs`); a board newer than
its build it refuses before any write, and a project's live board behind it it
moves only as `live::may_move` lets its build. A reader (the Symphony's
`taskview::connect_in`) refuses a board not made yet or behind its build after
that one read, defining and writing nothing (`tests/store/schema.rs`). Each sending waits the client's wait for its whole answer, however
it trickles in, so a read waits at most twice that. A late answer on a working
connection says the server is slow (`rpc::slow`), which the Symphony's read routes answer with 504, not the 503 of
a board that cannot be reached. Each request a `Client` sends is counted, with no request of its
own, in the `board_health::QueryLog` every client of that board in the process shares (by
address, namespace and database): its time, its tracked caller's line, whether it may write and
whether it went unanswered. The Symphony's hub adds its dispatch passes and idle sweeps
(`board_health::Recorder`) and serves the hour at `/api/health`; the one server writes it to
`sf-tasks/health.json`, which `tasks status` reads without the board
([TASKS](../../docs/TASKS.md#board-health)). `tasks-copy` copies a whole
board into a server, replacing what it held, in one transaction: `tasks-copy ADDR
[DATABASE] < dump.json`, or `tasks-copy --board` for the board this run reaches;
`tasks-copy --board --out` writes that board's dump, in the shape the copy reads,
and changes nothing. On `surrealkv://` every row of that transaction costs the
same, as long as no event UPSERTs a record (`src/schema.rs`): the board of
2026-11-04, 28 MB of dump, copies in 20 s (`tests/server/copy.rs`, a quarter of
it). The `tasks-cutover` binary moves the
old SQLite board into it once, one way, reading SQLite through `rusqlite`.
On the in-memory
engine a transaction that big costs more than linearly (the board at the move:
39 server CPU-s, against 1.5 on `surrealkv://`).

The `perf` test target (`tests/perf/`) is the board at scale: a generated board
at or above the live one of 2026-11-07 (50k messages, 2.6k sessions, about 3k
roles, 100 recipients on Unix sockets this process serves) on `surrealkv://`.
Its plan check runs say, claim, inbox, a dispatch and the `tasks api` calls
`land` and the Symphony make through a recording relay and has the server plan
(`EXPLAIN`) every statement they sent, read out of each request as the
server's lexer reads a keyword: in any case, past comments, ending at any byte
no name goes on with (`SELECT*`, `SELECT$projection`, a space outside ASCII)
and starting after none that a name, variable, path or record id holds
(`tests/perf/sql.rs`); and every statement the schema's events run, read out
of `schema::DEFINE`, which the relay never sees. A statement of a request
that the server plans as a read of rows and the check read nothing out of
fails it, so a spelling it does not know is reported; a `TableScan` with a
predicate over message, delivery, role, session or callback_state, a whole
index of one of them under a filter, or a subquery in a predicate, fails it
(`tests/perf/findings.rs`), so a query that loses its index (a dropped
`DEFINE INDEX`, a condition rewritten past it) fails there, naming the
statement and its plan. Its timings run say, claim, inbox and those api calls
three times each, each median of the CPU a run cost (the `tasks` process's own,
from its reap, and the board's server's while it ran, `tests/perf/cpu.rs`, each
run started once the server is quiet, `Board::quiet`, so what it still did for
the writes before is not counted) within a bound stated once in
`tests/perf/timings.rs`, so a loaded machine, which stretches a wall, does not
fail them; the walls are printed beside. Its looks of `tasks wait` are each
bounded by what the look before them left to do: the whole digest, or what
changed since a look that printed its PULL; one of the whole digest that
printed none fails, as a look delivers what it built however long that took.
`tests/perf/words.rs` deletes a rare sender's newest
word (its other one the board's first) and every message at once with the last
words named, as a schema 12 copy does, each within a bound of the server's CPU
stated there.
Its tier is nightly, so the fast plan leaves it out:
run it by hand (`tools/cargo.sh test -p symphony-board --test perf`) for a change to the board's
queries or the api. `SF_PERF_TASKS=PATH` times another `tasks` build on the
same board. A run leaves its folder in the temporary directory; the next run
removes each such folder whose process has ended.

Every server `server::Home` starts gets `server::Storage` as its environment
(`SURREAL_SURREALKV_VLOG_THRESHOLD`, `SURREAL_SURREALKV_MAX_MEMTABLE_SIZE`).
Each value stays beside its key in the sstables, where compaction drops the
versions a rewrite leaves, and the write-ahead log holds at most 256 MiB.
SurrealKV's own threshold, 4 KiB, sent large values to a value log. It deletes a
value-log file only once no sstable points into it, so a row rewritten in place
kept every version: 102 GiB for a 0.5 GiB board (tools/tasks-db-space). Never
set `SURREAL_SURREALKV_ENABLE_VLOG=false`: a store with values in its log then
cannot read them. `tasks-reclaim` (the owner's) is
`Home::reclaim`. Under the starters' lock it ends the board's server and starts
an unrecorded one on the store. It exports every namespace's every database with
`surreal export` and imports them into `reclaim/data` under `Storage`, each
database's file named by its place among them (`0002.sf.tasks.surql`), so two
names differing only in case never share one on macOS's case-folding
filesystem. Then it re-exports, compares each file byte for byte along with the root's and
namespaces' users and accesses, swaps the stores, starts the board's server and
removes the old store; the exports stay. It refuses a store without `sf/tasks`
and an `INFO` answer without its namespaces or databases object, which would
otherwise carry nothing and remove the store. Its private servers never take a
port the halted server had, which a monitor may still hold as the board's. A
failure before the swap restarts the old store untouched, and says the restart's
own failure beside the reclaim's. A failure after it ends every server on the
new store before the stores move back: a start whose address record fails ends
the server it started, since the next `ensure` would adopt one left
unrecorded. A start refuses while the store sits in `reclaim/old`
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
changed board scans hashes and part counts and embeds only changed sources. Explicit catch-up
also repairs derived cache damage without a board write. Manual bulk catch-up uses one thread too; `--threads 1..64` explicitly overrides it for bulk work.
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

`cargo run --release -p symphony-board --example search_bench -- --database NAME --out
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
