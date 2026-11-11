# Task board

The standalone workspace provides the generic board and document commands, and
the default landing, `just land SLUG` ([the default landing](#the-default-landing)),
which `symphony init` installs in every project. Verification, certificates and the
landing/stamp recipes in the workflow examples are supplied by a consuming project,
whose own `land` recipe replaces the default everywhere for it; no project gate
executable or stamp backend is bundled here. See the workspace [command ownership](../AGENTS.md#command-ownership).

How several agent sessions share work, and how they, and the owner, talk
about it. Sessions may run in any worktree of this repository; the board is the
same for all of them.


Commands shown use the consuming project's recipes and board connection.

## Where it lives

A SurrealDB database (namespace `sf`, database `tasks`) whose server keeps its
data, its address and its log in the git common dir, `.git/sf-tasks/`, so every
worktree reaches the same board, a claim is one transaction, and none of it
shows up in `git status`. Whatever needs the board starts the server when it is
not running: the first `just tasks`, `just symphony` as it starts, a page of the
Symphony. It starts detached, so it outlives whoever started it, and that is
also the restart: a server that was killed is started again on the same data.
The heartbeat hook never starts it. `TASKS_SERVER` (an address) and
`TASKS_DATABASE` reach another board, as the tests do. It is working state, not
history: `just tasks export` writes a readable snapshot to
`docs/history/tasks.md` when one is worth keeping.

Each project has a board of its own. A run's project is named by the nearest
`symphony.toml` from its folder up, never past the top of the checkout it is in;
with none, by the machine registry's entry for its repository's main checkout,
else by that checkout's folder name. Its board is that repository's, in its
common dir, under the same namespace and database. The Symphony serves each
registered project's board, and reconnects to one whose server restarted. A
project's heartbeat keys, sweep state and staffing lock stay in its common dir.

Each build of the board writes one schema version, which the board records. A
build opens a board at its own version with one read and no write. A newer
build migrates an older board forward once, its additive steps and the new
version in one transaction before any other write. An older build is refused
before it writes, naming the project and `just symphony`. A Symphony one version
behind still reads the board, and every write it would send is refused the same
way. A store opened before a newer build migrated is refused at its next write.
The Symphony never moves a board forward or makes one, so a branch's command line
or preview server with a newer schema leaves the live board as it is: a board
behind its build, or not made yet, it refuses before it reads, naming `just tasks
list` in the project, the board command that moves or makes it. A branch tries a
newer schema on a scratch board (`TASKS_DATABASE=NAME`), which its own `just
tasks --database NAME` moves.

The store takes about what it holds. Every server the board starts keeps each
value beside its key, where compaction drops the versions a rewrite leaves, and
logs at most 256 MiB of writes before it moves them to an sstable. SurrealKV's
default had sent every value over 4 KiB to a value log that only grows: 102 GiB
held a board of 0.5 GiB. The `tasks-reclaim` binary, the owner's to run from the main checkout,
rebuilds a store that grew that way. It stops the server, exports every
database, imports them into a new store, and checks that store's exports byte
for byte against the old ones. Only then does it swap the stores and remove the
old one. The exports stay in `sf-tasks/reclaim/exports/`. `--dir FOLDER`
rehearses on a copy of `sf-tasks/` (`cp -c`, a clone).

The command is the `tasks` binary (`crates/tasks`). In a project `symphony init`
set up, `just tasks` is the recipe it installs in `symphony.just`: the project's
build linked at `.target/bin/tasks`, else, from a linked worktree, the main
checkout's link, else `tasks` on `PATH`; it builds nothing, and says which link
to make when it finds none. The Symphony's own board clients find it the same
way. This repository's `justfile` has a `tasks` recipe of its own, which wins:
`tools/rs-bin.sh` builds the binary before every run, so the first command after
a change to its sources takes a minute or two, later ones only Cargo's check
that nothing changed. On the live board it is the main checkout's build, from
whichever worktree it runs; a branch's own build runs only on a scratch board
(`--database NAME`). The heartbeat hook runs the build its tree last linked
(`.target/bin/tasks`) and builds nothing. There is no Python board adapter.
`launch`, `restore`, `cleanup`, `close` and `reap` are its own, hc's terminals and all
(`crates/tasks/src/launch/`); the board calls no `herdr`. The server is the pinned
SurrealDB at `sf-surreal/VERSION/surreal` in the git common dir, or the binary
`SF_SURREAL` names. In this repository `bash tools/surreal.sh` fetches it there,
checked against its pinned SHA-256 (the Rust tests fetch it the same way); a
project puts SurrealDB's release binary there, which `symphony init` lists among
the steps left. The board says so when it is missing.

| | |
|---|---|
| `just tasks …` | everything an agent does (`just tasks --help`) |
| `just tasks search QUERY [--status S] [--limit N] [--json]` | ranked full-text matches in task titles, briefs, slugs and threads; task rows with matching excerpts (default: 20); query capped at 1024 UTF-8 bytes and 32 terms (word/number runs and punctuation/symbols, after lowercasing) |
| `just tasks stats [--days N] [--json]` | where the pipeline's time goes: lead time, time in each status, bounces, tasks done per day and what sits longest ([Where the time goes](#where-the-time-goes)) |
| `just tasks status [--json]` | the board's health over the last hour, as its Symphony server recorded it: failed dispatches and sweeps, the slowest request, write latency ([Board health](#board-health)) |
| `just tasks callback [ADDRESS] --as HANDLE` | register a native channel; addressed events start a turn directly |
| `just tasks pull --as HANDLE` | the whole PULL digest as HANDLE hears it now; a callback carries only what changed since the last PULL it received |
| `just tasks wait --as HANDLE [--timeout S]` | for a session with no channel: exits printing what a callback would have pushed it since its last read, once there is any; run it as a background command and again when it exits |
| `just agent-send HANDLE "text" [--as SENDER]` | message an agent directly through its registered channel |
| `just symphony` → **▦ tasks**, **◉ agents** | the board, each task's page and thread, and who is connected |

## Sessions

Every agent that works from the board has a **handle** (`amber-heron`), a
kind (`orchestrator`, `worker`, `reviewer`) and, for a subagent, a parent. The
handle is printed by `just tasks register`; later commands take it from
`TASKS_SESSION` or `--as`. Then run `just tasks callback --as HANDLE`: it detects
the Claude inbox socket or Codex thread. An explicit `uds:/absolute/socket` or
`codex:THREAD-UUID` works too. A session with no channel is listed as such and
keeps the inbox fallback. hc terminals are for inspection, never delivery.

`just agent-send HANDLE "text"` addresses an agent without a task. The sender
comes from `TASKS_SESSION` or `--as`; delivery is one plain
`AGENT from SENDER to HANDLE — text` event line. Reply with
`just agent-send SENDER "text" --as HANDLE`. Whitespace is flattened as in board callbacks.
It uses the callback outbox, identity checks, retries and inbox fallback. Output
distinguishes sent from queued; sent confirms native transport only. An agent
without a channel is refused with the command to register one and the option
to send on a task with `just tasks say SLUG TEXT --to HANDLE` for its inbox.
The Agents page shows each native address and the command to reach it. Scratch
boards use `just agent-send --database NAME HANDLE "text" --as SENDER`.

`just tasks callback --chat` registers the app's chat recipient. Chat selects
the explicit seat, then an `--if-free` fallback, then a live orchestrator with
a native callback, choosing the lexicographically smallest handle among plain
orchestrators. A normal `just tasks callback --as HANDLE` is enough for that
last fallback; `--chat --if-free` opts into the higher-priority fallback without
displacing the seat. Sweep-confirmed native presence counts as live. A recipient
suspended after delivery failures waits for the sweep to restore it. While no
channel holds the seat, an orchestrator reading by `just tasks wait` holds it:
a session is waiting from its first `wait` until it registers a channel (or
turns one off) or leaves, and of the live waiting orchestrators the smallest
handle receives chat at its next read; one that stops waiting holds it while it
stays live, so what the owner says waits for that read. While nobody holds the
seat, every orchestrator's `just tasks pull` ends with what the owner said that
nobody received, and its `just tasks inbox` takes it, after which nobody reads
it again: an owner's word never goes to no one unseen.
The app names the recipient, and says when nobody receives chat. Saves queue
asks, follow-ups, card messages, verdict notes and callouts directly; an outbox
beside the asks preserves them across a board outage. No file watcher discovers
chat. `--off` or leaving the board unregisters delivery.

Board writes dispatch addressed messages and role/status/new-work changes. A
command line's write queues what each recipient hears, as the write left the
board, and answers without delivering it: a detached `tasks dispatch` delivers,
one a host at a time, which makes one more pass for each write that commits
while it runs. One that finds another running waits until that pass has ended,
and takes over if the other's lease lapses first.
The shared Symphony server schedules PULL digests and STALE/LOST transitions and
retries failed deliveries. Per-recipient leases prevent concurrent writers
from sending the same queued event. A successful native send acknowledges the
inbox; failure retries three times, is recorded on the task thread, and retains
the event in the inbox. As with any external queue, a process crash after the
native send but before acknowledgement can repeat delivery; messages carry
stable board ids. No normal agent event remains a stream; the browser's live
feed remains a stream because it updates the owner's page.

### Board health

A board too slow to answer delivers no callback, and says nothing on the board itself, so the
Symphony server counts what it sees in memory, for each project it serves, over the last hour
(`crates/tasks/src/board_health.rs`): its clock's callback dispatch passes, ok and failed, with
the newest failure's error, kept on one line and cut to its first and last characters when it
is long (a tick that cannot reach the board at all is a failed pass); the idle
sweeps, the same way; every request any client in the process sent the board, the slowest with
the line of code that sent it (its tracked caller), the slow ones (a second or more) and the
unanswered ones; and the p50 and p90 latency of the requests that may write. Counting adds no
request. `/api/health` (`/<id>/api/health` on the one server) answers it from that memory,
without a board read. The machine's one server also writes it to `sf-tasks/health.json` in the
git common dir (`health.<database>.json` for a scratch database; `TASKS_HEALTH` names another
file) every ten seconds, and at once when the dispatch or the sweep starts or stops failing; a
private server keeps it in memory alone. Its clock's thread and its sweep's write it one at a
time, each from its record to the rename of a file of that write's own, so a reader never meets
half a record and the last to land holds every pass. `just tasks status` prints that record
without asking the board, and `--json` prints it as `/api/health` answers it, beside the file
and the warnings. A warning stands from a failed dispatch pass, or a failed sweep, until its
passes have been good for a minute, so a board failing every other tick is warned of throughout,
not on and off; one good pass does not clear it, and the hour still counts the failure. A newest
pass that failed more than ten minutes ago, with none since, is warned of no more. While one stands,
every page of the project says so on the server's notice (beside a failed server update, when
there is one), told when it starts and when it ends, and the project picker says so under the
project's name; both read the server's memory, not the board (`apps/symphony-server/src/live/health.rs`).
`tasks status` also warns when the record is more than a minute old, naming the server's pid and
whether it still runs: no dispatch pass has been recorded since.

**A disk running full** is warned of in the same two places: `tasks status` when the disk holding
the record has less free than `SF_LOW_DISK_GB` gigabytes (20 when unset; `0` turns it off, and a
value that is no number is said and 20 taken), and every page of a project while the disk
holding its checkout has, read by the server each time it tells the pages of its health; the page
says under how many gigabytes, not how many, so it is told when the disk goes low and when it is
freed, not at every build's write. A full disk fails every build on the machine and can corrupt
the board's database; the warning names `just tasks sweep`, which
clears the build output of finished worktrees (below). `--json` adds `disk`: the folder read, its
free bytes and the threshold (`crates/tasks/src/board_health/disk.rs`).

The Claude wire envelope is newline-delimited
`{"type":"user","message":{"role":"user","content":"…"}}`, observed in the
installed Claude Code 2.1.308 socket example. Its body format is undocumented;
this is the compatibility assumption. On macOS the board sends it without
authentication and never reads the session's messaging token. Claude's inbound
controls can hold or refuse it; a successful socket write confirms transport,
not that Claude accepted the message. No project inbound setting is changed.
Codex delivery uses the installed `codex queue --thread UUID --message TEXT`.
For isolated runs use `tasks --database NAME ...` and
`symphony-server --scratch NAME --port N --root ROOT`; the latter keeps
its asks in `artifacts/callbacks-NAME/asks.db`.

**Agent sessions behind a task** (`task_sessions`): `just tasks launch SLUG` starts the
agent the project's `[agents] launch` in its main checkout's `symphony.toml` names (`default`,
`codex` or `claude`, with its `model` and `effort`), else a Codex agent (`--no-daemon`; model
`gpt-6.1-sol`, effort `xhigh`, workspace-write with network access), in an hc terminal of its
own, `hc run --bg --no-parent --outlive-parent --keep`: no child of the caller's terminal,
which the app would show under it rather than in a tab. `--kind` and `--model` win over the
project's; its model and effort go with its agent, so `--kind` naming the other agent takes the
board's model for that one (Codex `gpt-6.1-sol` at `xhigh`, Claude `sonnet` at its own effort).
A table that does not parse, or names another agent, refuses the launch. The `launched` line
names the agent, model and effort it started, and `launch --help` ends with what a bare launch
starts where it runs (`crates/tasks/src/launch/defaults.rs`). The
terminal shows in its stream tab (`code` for an executor, `review` for a reviewer, or
`--tab` such as `3d` or `infra`) of the project's space in the Herdr app, found by name and
made when missing (the labels `herdr.space` and `herdr.tab`); the owner's tabs and other
projects' terminals are never a destination. Once hc calls the agent ready (`idle` or
`done`), its first message goes in by `hc send`, never forced, and the launch is confirmed
only by that send's receipt (`started`): an agent waiting on a person (a folder-trust
dialog), never ready, or not starting on the message fails the launch naming the terminal,
which is killed and removed. It writes its handle, role, agent session
id, worktree, branch, terminal and start time; `just tasks show` lists them under
`Sessions:`. No terminal the owner may be looking at is closed: one in the tab the Herdr
app shows, the agent it shows, or held over them (`herdr-app status`, read last, right
before the kill, after a last `hc get` that must still find it finished, running the program
cleanup judged: the same pid and foreground); when the window cannot say or the terminal
works again, it stays and cleanup names why. A shell closes only with the shell itself in
its foreground: a job it runs in front, or one it became by `exec`, is work. When a terminal is closed (`just
tasks cleanup`, or the task becoming done or cancelled: `hc kill`, then `hc rm`) the
record stays and says why and how to bring it back,
`just tasks restore SLUG [HANDLE]`, which resumes the conversation in the same
worktree, in a new terminal. A record from before hc names a herdr pane (`w1:p…`): that
terminal is gone, so cleanup records it closed with its restore line, and restore brings
the agent back in an hc terminal. `just tasks close SLUG HANDLE` closes one agent's terminal
whatever role it holds (a reviewer waiting for a new tip), records it the same way once the
terminal is killed, bound to that terminal, and retires the callback registered from it; it is
refused for the caller's own session or terminal, one the owner may be looking at, and one whose
program is not at rest (work under it as the idle sweep reads it, any test run included, or
another program in its foreground). A close and a restore of one session each hold it while they
run, so neither starts while the other runs; one whose hold lapsed and was taken over writes
nothing more, and a restore that lost it kills the terminal it started. A Codex record
takes the thread its callback registered whichever is written first; one closed with no thread
says on its thread that restore cannot resume it. See the [tasks skill](../crates/symphony/shipped/skills/tasks/SKILL.md#launch-restore-clean-up-herdr).

A consuming project chooses its agent models, staffing policy and terminal lifecycle.
The board records those sessions and roles; it imposes no model preference.

**Optional automatic staffing:** `just tasks staff [--dry-run]` makes one pass, highest
priority first. Enable it in `tools/task_staff.json` in the consuming checkout: `enabled`
defaults to false, `max_sessions` to 8 and `min_free_memory_gib` to 4. `only` is an
optional list of task slugs whose subtrees may be staffed; an empty list allows all
orchestrated work. The config belongs to the checkout running the command (main for
the Symphony). `reap` runs staffing before its five-minute idle sweep. A shared
`sf-tasks/staff.lock` keeps concurrent staffing passes from dispatching twice.

`reap` also recovers missing agents with addressed work left unanswered for thirty
minutes, even when staffing is disabled. It resumes a recorded conversation once
per handle/post, after the hc terminal inventory confirms its absence, and posts the
restore on the task thread. With no recorded conversation it only flags the work;
it never starts a fresh conversation on its own. A failed automatic attempt stays
flagged for manual action. `reap --dry-run` starts no recovery.

Only ready tasks with dependencies done and no executor or reservation qualify;
review tasks with no reviewer get one breakage-only reviewer through an instruction
child. A land-request task's opening `Review` line records the branch under review;
an explicit `Branch:` or `Worktree ..., branch ...` line does too. Otherwise reviews
use the executor's recorded branch, falling back to the task slug.
Dry runs resolve that branch and show its tip too. A task whose target or dispatch
fails prints its failure and leaves later tasks eligible; staffing errors never
prevent the idle sweep. Work already staffed through a child is left alone. A live orchestrator on
the task or an ancestor is required. Owner-only work is excluded by `owner-only`,
`owner only`, `owner tier`, `sign-off`, `signoff` or `bless` in its slug/title, or a body
line beginning `Owner-only:`, `Owner only:`, `Tier: owner` or `Review tier: owner`.
Use these explicit markers for owner decisions.

Before launching, the pass prompts an idle worker/reviewer holding no unfinished
work in a suitable open terminal: an executor already on the task's own branch, or an
instruction worker/reviewer in a detached checkout. A checkout with a live or stale
executor is never reused. Otherwise it launches what a bare `just tasks launch` would (the
project's `[agents] launch`, else Codex) in a separate worktree: the
task's own branch, or detached main for an instruction or review. Live agent terminals
and live board agent sessions without a terminal count toward the cap;
reuse adds no session. Memory below the floor defers work. Every proposal, dispatch,
deferral and failure prints one line. Only a successful ping or launch is posted on
the task; deferrals and dry runs post nothing. A dry run previews even while
disabled and creates no task, session or terminal.

**Idle agents are closed by the board, not by a reminder** (owner, 2026-10-07). A task
that becomes done or cancelled closes its sessions at once; the rest is a sweep the
Symphony's process (`just symphony`) runs every five minutes. A lock in `sf-tasks/`,
beside the board's server, keeps two sweeps (a second app's, or `just tasks reap` by
hand) from running at once, and nothing goes into any monitor. It closes (`hc kill`,
then `hc rm`) one of this board's terminals (label `sf.board`) whose named agent was
idle (or stuck on the folder-trust prompt) at two sweeps at least five minutes apart,
however many apps sweep, when neither its handle nor a subagent session under it holds
an executor role on an in-progress, blocked or review task, a reviewer role on a task in
review, an unanswered addressed post, or a reviewer awaiting re-review after changes
returned its task to in-progress. That last keep includes landing changes messages,
on the branch task or any numbered review task. A pending post keeps the terminal
immediately, before the thirty-minute recovery threshold. A post, status move or verdict
by its addressed handle answers earlier work; reading its inbox does not.
Nothing runs under it but MCP helpers (Codex's configured `node_repl` and the ChatGPT
app's `cua-repl.mjs` under its own `node` among them, each by its exact path, and
Codex's `codex-code-mode-host` in a standalone release's `bin` folder), caffeinate and
board commands: anything else is work, a `codex exec` job included. The roles above
keep it for an hour at most (owner, 2026-11-08: "Only sessions doing active work or that
will soon (less than one hour) have active work should be kept open"): idle at its
prompt that long, from its last tool call or the last change of its role or task
status, it closes saying what brings it back; only an unanswered addressed post keeps
it longer. A terminal whose program ended is removed at the first sweep, the open
records naming it closed first with their restore line.
Status, roles and the window are read again right before the close, and the terminal and the window once
more after the close is recorded, right before the kill: work resumed by then (its agent
working, or work under it) or the window showing it reopens the record. Never an unnamed terminal or another board's (it may be the owner's), an
orchestrator or a chat, the caller's (`HC_TERMINAL_ID` or its session), or one on screen in the Herdr app, which
starts its idle wait again (as does a window that cannot say what it shows).
Each close is one line on the handle's task thread, with its restore line, written
before the terminal is closed. `just tasks reap --dry-run` names what the next sweep would close
and why each terminal stays; a sweep tells its log why it keeps an idle one, once per
reason. `just tasks clean` runs cleanup and one sweep at once, never closing the caller's.
Code: `crates/tasks/src/launch/reap.rs`.

**Finished worktrees go by themselves** (owner, 2026-10-21: "do any cleanups for worktrees
that can help decrease needless disk usage. No need to ask, this should happen automatically
when tasks are complete"). The worktree sweep (`just tasks sweep`, the rules in
the consuming project's worktree policy) starts, detached, whenever work
finishes: a task going to done or cancelled (its closed sessions' worktrees then go without
the 2 h wait), `just tasks leave`, `just tasks reap`, and at most once an hour `just tasks
inbox`, by a stamp. The command that finished the work never waits on it and says nothing of
it; each sweep runs from the main checkout, by the main checkout's build of `tasks` (never
the caller's, whose sweep may not have landed; with none built it says so and sweeps nothing),
and appends what it did to `sf-tasks/sweep.log` in the git common dir. Status writes through
`tasks api` (for example, a nightly or reap caller writing status) do not start this CLI trigger;
the next hourly sweep covers them. The Symphony's explicit finish action starts its own sweep.
A worktree with live work is never removed: a live Claude Code lock, a live or stale session bound to it (a
subagent's session is bound to its own `agent-ID` worktree as well), a live task tied to it
(its branch names the task's slug, or the slug with its last `/` as `-`), a process inside
it, an owner verdict or any change keep it. Before each removal the sweep refreshes sessions,
launches, locks and activity and searches that tree for process working directories. A detached
agent's merged `worktree-agent-ID` branch stays while its live or stale session still binds its
worktree. In-process agents work only in their own `agent-ID` worktree; a different directory
they create is not bound by their agent id. Full sessions register from the worktree they use.
For a backlog task, ignored test output under `artifacts/` keeps the tree too, even when explicit
landing cleanup would discard it; only done/cancelled tasks (or no task) release all output.
A board `TASKS_SERVER` names (a test's) starts no sweep unless
`TASKS_SWEEP=on`; `TASKS_SWEEP=off` starts none anywhere. Code: `crates/tasks/src/sweep.rs`.

**The build output of finished work goes too, the worktree staying.** A worktree the sweep keeps
(a Claude Code agent's, locked while its orchestrator runs, say) loses its build folders
(`.target/`, `symphony/.target/`), each gigabytes, once every task tied to it is done or
cancelled, one of them done; no agent bound to it beat or made a tool call within the hour, and
nothing touched its git files in that hour; its HEAD is merged work, an ancestor of main that came
in by a merge or was committed in that worktree; and no process works inside it. A branch with no
commits ahead of main is not merged work, so a fresh worktree keeps its build. A task in progress,
in review, blocked or ready keeps it. Right before it removes, the sweep reads the worktree
listing and the board again and decides again, and asks again whether a process works inside;
then it takes each Cargo build lock in the folder without waiting (a build holding one keeps the
folder), renames the folder aside under them and removes it. A folder reached through a link
(`symphony` or `.target` linking elsewhere) is never touched. `just tasks sweep --dry-run` says
`would clear build output PATH (why)`. Code: `crates/tasks/src/sweep/build_output.rs`.

**A linked worktree's first build starts from the main checkout's.** `tools/cargo.sh`, which
every build here goes through, makes a worktree's missing build folder as a copy-on-write clone
of the main checkout's `.target/debug` (`tools/seed-target.sh`: clonefile on APFS, a reflink on
Linux; nothing on a volume that cannot clone), taken under Cargo's own lock on that folder and
without the workspace's own packages, so the worktree compiles those from its own sources and
reuses every dependency. While the clone holds that lock, a build in the main checkout and every
board command (each builds the main checkout's launcher) waits: on macOS for one clonefile call
of the whole folder (`tools/seed-clone.rs`, compiled by rustc on the spot), else for `cp -c`
cloning each file. A clone takes disk only as a build replaces what it holds; `du` counts each
clone in full, `df` says what the disk holds. It is staged as `.target.seed.PID`, which a signal
removes with the clone and a later seed or sweep removes once its process is gone.
`SF_SEED_TARGET=off` turns it off. Worktrees never share one build folder: Cargo names the same
package of two worktrees the same, so each would run the other's build of it. What it costs and
saves, measured: [the analysis](analysis/worktree-build-folders.md).

**Health** is derived from when the session was last seen. Every command
counts as a sign of life, and so does every tool call of a subagent (below):

| health | meaning |
|---|---|
| live | seen in the last 90 s, or inside one long tool call that could still be running |
| stale | quiet, or its monitor has stopped: its work is still held, nobody may take it |
| lost | gone for 10 minutes holding claimable work: that work can be claimed, on the record |
| left | it said `just tasks leave` (its roles were released), or it has been gone 10 minutes holding nothing anyone could claim |

**Claimable work** is what a session executes in `concept`, `design`, `ready`
or `in_progress`, and what it reviews that is still in `review`. A session
whose part is finished (its task done, or waiting in review for someone else)
ends quietly as `left`; if its task comes back with `changes`, it is `lost`
again, and that task can be claimed. `just tasks who`, the Symphony's
**◉ agents** and the attach monitor read the same health.

Callback registration does not expire. The legacy attach stream remains only
for sessions with no native channel. Tool calls are still the heartbeat.

A **subagent** has no monitor; its own tool calls are its heartbeat. The
project's `PreToolUse` (Bash) and `PostToolUse` hooks
(`.target/bin/symphony hook tasks-beat`, `just tasks beat`) see the Claude Code agent id
on each of its tool calls. A `just tasks …` command the subagent runs binds it
to the handle that command acts as, and so does one naming a justfile or a
folder before `tasks` (`--justfile PATH`, `-f PATH`, `--working-directory DIR`
or `-d DIR`, as a worktree's `just --justfile MAIN/justfile tasks …`): its own
`--as <handle>` flag, else a leading `TASKS_SESSION=<handle>` on that same
command. A handle anywhere else
in a command (a grep pattern, text inside a quoted argument, another recipe
such as `just land` run under someone's `TASKS_SESSION`) binds nothing. From
then on each of its tool calls beats for that handle, until a later board
command names another. A full session with no agent id (one started in an hc terminal, or a
`claude -p` run) is bound and beats the same way, keyed by its session id, but keeps the first handle
it names: naming another (registering or relaying for a child) moves nothing. A handle
with a monitor of its own (an orchestrator) is never bound: its monitor beats for it,
and a session that names such a handle is barred from binding at all, so the calls of
an orchestrator never keep a child it registered alive. The hook looks at the board
only for a board command or a key already bound. A foreground Bash call
reads live for as long as its timeout allows (at most 10 minutes), so a long
`verify-full` does not look like silence; of calls running side by side, the
one that may run longest keeps the mark, and only its own end clears it. Each
tool call of a bound subagent also records, on its binding (never its session
row, which keeps where its orchestrator ran `register`), the checkout that call
ran in: the top folder above the hook's `cwd` and the branch checked out there,
read from the `.git` files, and, when that checkout changed, the time it came
there; a call with no `cwd`, or one outside any checkout, moves neither (the
last checkout stands: such a call says nothing of where it works). A call there
that reads a time before any other there did (the clock went back) keeps that
earliest time too, never moving when it came. On Linux, whose kernel dates a
process's start on the clock since boot, the hook also reads that clock (its
boot's id and `/proc/uptime`): the binding keeps the reading of the call that
came to the checkout and of its latest call anywhere, a later boot's replacing
an earlier one's, and none when a call read none. Setting the wall clock moves
neither those readings nor a start dated on that clock. A
subagent waiting on a background command of its own (a `cargo test`, a build)
makes no calls; while a live process it started works in that recorded linked
worktree it reads `running_test` in the agents API, and is neither stale, lost
nor orphaning its task ([The agents API](#the-agents-api)).
A subagent that stops makes no more calls, leaves nothing working there, and
ages to stale and lost as before.

Orchestrators can share the board, each holding its own slug prefixes. A decision
one side makes that the other must act on becomes a short board task for that side,
and a side that approves documents commits them regularly, because worktree agents
see only what is committed.

## The agents API

The app is the source of truth for who is doing what: `GET /api/agents` of the Symphony and `just tasks agents
[--json] [--all]` return the same document (`crates/tasks/src/cli/agents.rs`; these keys are a contract, pinned by
`crates/tasks/tests/cli/agents.rs`). The board (sessions, roles, `task_sessions`) is the record; hc's
live terminals (`hc ls`) and the test-slot locks are probes that say what each agent is doing now, and `probes` names
either as `unavailable` when it cannot be asked, the rest being built from the board alone. The hc probe keeps the
key `herdr` and an agent's terminal the key `pane`: names the wire keeps. The Symphony's `/api/agents` also names
that probe `panes`, with the same answer under both keys, and its client reads `panes`, else `herdr`
(`crates/symphony-api/src/agent_probes.rs`), so a client and a server one build apart still agree; the
rest of its document is the board's.

```
{"generated": 1790000000.0,
 "probes": {"herdr": "ok", "slots": "ok"},
 "agents": [{
   "handle": "amber-heron", "kind": "worker", "model": "sonnet", "parent": "cobalt-otter",
   "task": "sim/jump", "role": "executor",           # the task it works now (an open one first)
   "tasks": [{"slug": "sim/jump", "role": "executor", "status": "in_progress"}],
   "worktree": "/…/lamp-works-wt/sim-jump", "branch": "sim/jump",   # where it really works
   "agent": "claude", "session_id": "9aa0…", "pane": "t5",
   "callback": "uds:/tmp/cc-socks/1234.sock", "chat": false,
   "state": "running_test",
   "state_detail": {"recipe": "verify-full", "phase": "waiting", "since": 1789999000.0},
   "health": "live",
   "last_tool_call": 1789999900.0,
   "last_message": {"slug": "sim/jump", "ts": 1789999800.0, "text": "the ring is at 900"},
   "slots": null}]}                                    # an orchestrator's: {"open": 3, "cap": 8}
```

`state` is `working`, `running_test` (a run of its own holds a test slot, `phase: holding`, or is queued for one,
`phase: waiting`, `state_detail` naming the recipe and since when; or a quiet subagent's own worktree holds a live
process, `{"recipe": "process", "phase": "running", "since": null}`, or the machine's processes cannot be listed to
tell, `"phase": "unknown"`), `waiting_on_input` (hc sees an approval or a
question), `idle`, or `gone` (no terminal and no sign of life). An agent running or queued for its own verify reads
`running_test` and `health: live`, never `lost`; so does a subagent whose worktree holds a live process, and one whose
processes cannot be listed reads `health: unknown`. `last_tool_call` is the time of the last tool call of the agent
(`null` before the first): the monitor's 20 s beat and board commands do not move it.
`callback` is the native address (`uds:` or `codex:`), or `null` for an inbox-only
session; `chat` says whether this session is the current chat recipient, including
an orchestrator fallback. `slots` is, under a project cap ([Slots](#slots)), the open
tasks an orchestrator (or any session holding open work's slot) holds against the
cap, and `null` otherwise. No credentials appear in this API.

The rules a port must copy, in this order:

- **Who is listed.** A session that ran `leave` (it has `left_at`) is omitted unless `--all`. So is one the board calls
  `left` (quiet, holding nothing another could claim; every session that never ran anything ends so) that has neither a
  terminal nor a run of its own nor a native registration: it is gone. A native
  registration stays visible while idle; its activity health is not fabricated.
  A quiet one with a terminal or a run stays (an executor whose task waits in review,
  its terminal idle). `--all` lists everything and changes nothing else.
- **worktree and branch.** From the agent's task session (`just tasks launch` recorded it; the one for its primary task,
  else its latest), else from its terminal's working directory (and the branch checked out there), else, for a
  subagent, the checkout its binding recorded at its last tool call, else, only with none of these, the directory
  `register` ran in: an agent that was never launched and has no terminal.
- **Which terminal.** The task session's terminal if hc still lists it live, else the terminal whose program runs the
  task session's conversation or the one bound by the heartbeat hook (when hc gives its argv), else the terminal titled
  or named with the handle. A terminal without an address is ignored.
- **Which run is the agent's.** First a run tagged with its handle (a slot records the `TASKS_SESSION` it ran for), wherever
  it runs: a verify in a trial merge, or by an agent that was never launched. Else, for an agent that is alone in its
  worktree, the run whose worktree is exactly its own (realpath; a slot records its git top level) and that carries no
  handle of any session on the board (left ones included, listed or not). Never a run in a directory beneath a worktree (the main checkout holds `.worktrees/` and
  exports), never to agents that share a worktree (all those registered in the main checkout); the count of agents sharing a
  worktree takes every session, left ones included, so `--all` never changes who a run belongs to. Several runs: holding
  before waiting, then the oldest.
- **What a quiet subagent waits on.** A subagent (an in-process binding) that is stale, lost or unknown, has no matched
  run and no terminal, and whose recorded checkout is a linked worktree (its `.git` a file; never the main checkout,
  which every agent registered there shares) waits on a process when one the machine lists is its own: it works in
  that worktree, its working directory the worktree's top folder or below it, with no other `.git` between (a scratch
  repository or a nested worktree is not its), compared component by component, never by a string prefix; and it
  started while the agent was there, from a second before its binding recorded that checkout to a second after its
  last tool call, on the clock its start is dated by: the board's for macOS, whose stamp is fixed when it started;
  the clock since boot for Linux, against the readings of that clock its calls took, never a wall time worked out
  from today's boot time, which setting the wall clock moves (a start in another boot than its last call's began
  after that call). A server or shell started there before the agent came, or after it went quiet, is not its, so a
  stray process never hides abandoned work. With none of its own there, the board cannot tell, and so never calls it
  lost, while one working there may be its: its start cannot be read (no permission, an unexpected record; one that
  ended since the listing is left out), or it began after the earliest time a call of the agent's there read and
  before it came, which only a clock that went back while it was there leaves, or it is dated since boot and the
  call that came, or its last call, read no clock since boot (a start after a last reading is still not its). The processes
  are listed with their starts (`lsof`, as the worktree sweep lists them, and the kernel's process table) only when
  such an agent is in the view, once a view, and a long-lived reader (the server's clock and dispatcher, the
  monitor) keeps the listing for 20 s by its store's clock, from when the listing ended.
- **state.** A matched run is `running_test`. Else a quiet subagent waiting on a process is `running_test` (`phase:
  running`), or, when the processes cannot be listed or the board cannot tell whether one there is its,
  `running_test` with `phase: unknown`. Else the terminal's status
  (`working`, `blocked` is `waiting_on_input`, `idle` and `done` are `idle`). Else the board's health: live is `working`,
  stale `idle`, lost or left `gone`. A matched run, and a process its subagent waits on, also lift `stale` and `lost` to
  `live`; processes that cannot be listed, or one whose ownership cannot be told, make them `unknown`.
- **A probe that fails** is `unavailable` in `probes`, and the answer is built from the board; a failing branch lookup is an
  empty branch.

Slot holders and waiters come from `test_slots.runs` (a held slot file whose writer is alive and whose lock is held; a
place in the pool's FIFO line, `<prefix>-<12-digit sequence>.place`, in the pool's own prefix: `slot-` and `render-`, that its waiter still holds); a slot file whose writer is dead is free and is not locked to find out.

An orchestrator's callback uses it: it does not call an agent stale or lost while `running_test`, and it receives a PULL
digest (`crates/tasks/src/cli/pull.rs`) once at its first dispatch, even when empty, then only when an actionable list gains an entry absent from the last delivered PULL after forgetting entries absent from any computed digest: red tasks ([Slots](#slots)) with no live executor 10 minutes after they were filed, in any status short of review, listed first as `red_unstaffed`, counted in the header only when there is one, and in no other list, `dependency_blocked` included, a cancelled dependency of its named in its note; ready tasks with no live executor
(one on a subtask still to be executed counts), in-progress with none, review with no reviewer, tasks that landed
(the branch is on main, or `landed <slug> on main` is on the task's thread) and are still open (a status change on the task after that line voids it, whether a move, a verdict such as `changes`, or a re-claim: the task is new work, not a landing left open), and idle agents, each scoped to the orchestrator's own and unclaimed tasks (an idle agent another orchestrator launched is left out). An executor
or reviewer that is `running_test` counts as present however quiet the board says it is.
A task in review whose tip a reviewer asked changes of is rework, counted in progress
([The pipeline alarm](#the-pipeline-alarm)). While the pipeline alarm stands, the whole board's
follows the header, whoever the digest is for, and its raising is an entry.
Counts and the clock do not trigger delivery. A departure alone pushes nothing;
an entry that stays present is never re-sent, and an ordinary entry that leaves and returns pushes once.
The first PULL a recipient receives is the whole digest: `PULL HH:MM` with every count, then each list's
first six rows and how many more it holds, but `unanswered`, whose every post is listed with the command that
answers it, since a post a PULL tells is never told again. Each later one says only what changed since the last PULL it
acknowledged: the counts that moved (`PULL ready_unstaffed=34 (was 33)`, or `PULL counts unchanged`), the
liveness and low-volume lines it was not told, each row a list gained, and `resolved LIST a, b` for the rows
that left it. A backlog of 33 that gains one row is that header and one row. What the last acknowledged PULL said
is kept in the recipient's callback state (`pull_said`, which older dispatchers ignore); after an older
dispatcher's acknowledgement, which does not record it, the next PULL is the whole digest again. A dispatcher
built before PULLs said only what changed (the shared server's, until it is rebuilt) records the entries of the
PULL it acknowledges and not what it said: a row it told that the recipient still hears shows it, and the next PULL
is the whole digest. Only when every row it told has left the recipient's digest by then is the next PULL measured
against the baseline before it: its header can repeat a count that moved before, with the older `(was N)`, its
liveness line can repeat or miss a change, and those rows are not said `resolved`. Rows added are exact either way,
never lost or said twice, and the next PULL this generation records is exact again.
A channel registered anew starts again from the whole digest: `--off`, a registration where none was (after
`--off` or a `tasks wait`) or one another process listens on (`Store::callback_in`) forgets which PULL the
session acknowledged, and a PULL waiting for it gives way to the whole digest; the posts it was told stay told.
Only the same listener moving to a new socket keeps its account, so what waited out an outage is said there as it
stands. An orchestrator reads the whole digest at its decision points with `just tasks pull` whatever it heard.
A pass whose unanswered-post scan fails says why on stderr and delivers the rest, and its PULL says nothing of
those posts, neither their count nor any of them resolved; their count is then unknown to the recipient, so the
next PULL whose scan answers says it (`unanswered=1 (was unknown)`) and resolves the posts answered meanwhile.
Unanswered work retains its task/post identity across departures and returns.
`unanswered` lists an executor on an in-progress, blocked or review task, or a reviewer
on a task in review, when it is lost or left (or its recorded terminal is closed) and an
addressed post has waited thirty minutes without a later message, status move or
verdict from that handle. A deliberate role release also answers earlier requests;
an automatic departure does not. Posts belong to their recipients at post time,
so a replacement never inherits its predecessor's role-addressed work. Each line
names the post and the recorded conversation's `just tasks restore SLUG HANDLE`
command, or the explicit launch command when no conversation was recorded. The
orchestrator receives each task/post once, even if it leaves and returns; a new
unanswered post can push a new line for the same task. Its count appears only when
nonzero. `reap` makes the sole automatic recovery attempt described above.

What is pushed is what the recipient cannot predict and must act on, said once: posts and verdicts, NEW,
STATUS, ROLE, BLOCKED, the PULL above, and STALE, LOST or LIVENESS UNKNOWN once for each transition of a quiet
child, never once a pass (a child it already hears as stale is not also listed idle, in a pushed PULL, a wait
or `tasks pull` alike). A recipient is never told its own writes: not its own moves, not a role move it made (a hand-over, a take-over), and not what its own
children (the sessions it registered, `--parent`) take on or let go, their reservations, claims and releases
and the moves and readiness those make; a child's own move, to review say, is still said, and so is one it made
before a claim or release between two looks (a block, then a release, says the block). What is read is the
rest: `just tasks pull --as HANDLE` prints the whole digest as HANDLE hears it now, whatever was pushed, and
changes nothing; `just tasks show`, `list` and `agents` read the board as it is.

A push through a channel arrives wrapped in the receiving client's notice and costs a turn. A session that can
hold a background command reads the same lines instead: `just tasks wait --as HANDLE [--timeout S]` exits
printing what a channel would have carried since its last read, once there is any, and the session runs it
again. It is the dispatch's own pass for HANDLE (`crates/tasks/src/cli/commands/wait.rs`,
`Store::collect_callbacks`), its lines printed rather than delivered, each text flushed before it is recorded
as delivered (one that does not print is retried five seconds on, so a failure repeats a line and never loses
one), so it says the same lines by the same diff and account, from the same callback state: the first read is the whole digest
and any posts waiting, each later one only what changed. It looks when the board's marker moved, every two
seconds at most, and at least every fifteen, since a child going quiet or a PULL falling due changes with time
alone; with `--timeout` it gives up after S seconds with `WAIT nothing changed in S s` (`0` looks once). It
is refused while HANDLE has a channel, which takes those lines (`just tasks callback --off --as HANDLE` gives
them to `wait`), and nothing else dispatches to a session without one, so between reads nothing is said and
nothing lost. Waiting is the session's sign of life, as a monitor's is: it beats for HANDLE every twenty seconds
or so, at a look, so an orchestrator waiting on its board is not called stale for it. Each look also watches
the panes HANDLE oversees, one hc sample a look, as the server's clock does for a channel, so a pane waiting on
a prompt is said as `BLOCKED` by the second look that sees it; and while HANDLE holds the chat seat (see the
callback section above) the owner's chat events come with the rest.
What waiting costs, measured on 2026-11-11: once HANDLE has its callback state and is on the chat seat's waiting
list (its first look puts it there), a look at a board that stood still is nine reads and no write; a poll between
looks is one read of the board's marker; a beat is one write. While the board stands still that is about 30
marker reads, four looks and two beats a minute: some 66 reads and 2 writes. Every write to the board moves the
marker, a session's heartbeat too, so on a board where other sessions beat a look comes at nearly every poll, up to
30 a minute (about 300 reads). A look took 7 ms on a small board and 0.8 to 0.9 s as a command of its own on
the 2.2k-task board of the perf suite (its process's start and connection included, a debug build at load 35 to
80), and the first look there, which says the whole digest, 18 s.

A legacy native observer starts from the current board on upgrade rather than replaying its history.
Its upgrade marker and message cursor are separate callback-state fields that older writers leave intact.
The fields writers older than that scoping read (the observation, the open-status map and the delivered PULL entries)
keep their whole-board meaning; the scoped observation and the entries it withheld are kept beside them, in fields
those writers ignore, and read over them: a line either writer said is not said again by the other.
A line still waiting for delivery to an orchestrator (a NEW, STATUS, ROLE, BLOCKED, STALE, LIVENESS UNKNOWN or LOST)
is said only if a dispatch would tell it that line now, by the same test, so one a hand-over or a released claim took
from it meanwhile is dropped, or a LOST said again of the work it still hears; the hand-over's own ROLE line is kept,
and a quiet child holding no work is still its parent's to hear of. A PULL still waiting is never delivered as it was
written: the digest is built again for its recipient, counts included, and said in its place, by one rule over what
the recipient hears now, what the PULLs it acknowledged told it and what the waiting PULL tells. With nothing heard
beyond what it acknowledged, the waiting PULL is taken back; an entry beyond both, shown or only counted, makes a new
PULL with a retry budget of its own; otherwise the waiting PULL keeps its key and attempts and says the digest as it
is now. What it was told changes only when it acknowledges a PULL, whichever writer records that, so work it gains
while one waits is said and work that only departs renews no budget. Work it hears is told once a PULL naming it is
acknowledged, whatever older dispatchers write, queue or acknowledge after. Only what was withheld from it is
ambiguous, since an older dispatcher's own PULL told it the whole board and reads the same as the waiting one:
withheld work counts as untold, so when a hand-over makes it the recipient's, one PULL counts it, once more at most
for that hand-over and never left out, and nothing repeats on its own. Posts and verdicts are kept: they were said to
whoever held the role then.
Codex callbacks use the queue by default; `git config sf.codexSteer true` opts in to
active-turn steering. The owner decides its first live use.

## Tasks and roles

A task is a slug (`sim/jump-departure`, the same slug the branch and commits
use), a title, a markdown body, a priority from `0` (most urgent) to `4`, and
optional links: `--link docs/game/SPEC.md#Heading` to the spec it implements,
`--depends` for tasks that must be done first, `--parent` for a larger task
or an instruction to an agent ([Subtasks](#subtasks)).

A cancelled dependency does not satisfy that requirement. Cancellation moves its
unfinished dependents to `blocked` and records the cancelled prerequisite on their
threads, with the status each held (`(was ready)`). Creating or moving a task with
a cancelled dependency blocks it too.
`next` excludes that work; PULL lists it as `dependency_blocked` with the reason,
including older tasks still in their previous stage. Done and cancelled dependents
keep their status. An orchestrator must replan the dependent work before it resumes.

`just tasks depends SLUG --replace OLD NEW [--as HANDLE]` points a task at a
successor; `--add DEP` and `--remove DEP` add or drop one, and each flag repeats.
It refuses a task the board does not hold, a finished task, a cancelled
prerequisite, a dependency already there or not there, and a cycle, and records
the change on the task's thread. A task a cancellation blocked that then waits on
nothing cancelled goes back to the status its block line recorded, or to `ready`
from `in_progress` when nobody executes it; a block line written before the
status was recorded sends it to `ready` (`in_progress` with an executor). A task
blocked by hand stays blocked.

`just tasks priority SLUG N [--as HANDLE]` changes an existing task's priority,
from `0` to `4`, and records the actor and both values on its thread. `next`,
`list` and the PULL digest use the new priority immediately; the change sends
no callback.

The pipeline is `backlog → concept → design → ready → in_progress → review →
done`. The board shows the moving stages in one row, and where work ends
up (`done`, `blocked`, `cancelled`) in a row below it. A task
starts wherever it belongs (`--stage concept`, `--stage design`, or `ready` by
default). Work with no concept or design step goes straight to `ready`.
Claiming concept or design work keeps the task in its stage; the executor
moves it on when the concept or design is done. The card shows what waits in
each document the task links ("N waiting" undecided cards), for information
only: nothing waits on it.

**Review waiting on nobody is blocked.** The owner, 2026-11-03: "the task was
in review, but actually waiting for some approval. So it should had been
blocked instead in regards to the status, since no work was happening." A task
in review whose branch has every stamp it needs to land but ones nobody is
producing (the owner's approval, a dependency only that approval collects, a
hold its orchestrator names: the consuming project's stamp policy)
goes to `blocked`, with a line on its thread naming them. `just land`, `just
land-approve` and `just land-stamps` make that move when they find it, and move
the task back to `review` once one arrives: the owner's approval (the
Symphony's approve-to-land button, or relayed), a waiver, or a stamp someone
produces going missing. A role holder may move blocked work back to review, and
so may the owner; a command run by a session with no role on the task says the
move it could not make, and leaves it to the next run by a role holder. A
landing finishes work still held blocked as it does work in review. It
refuses work in progress, but finishes a task taken back to `in_progress`
after its last read of it, as main moved: that branch is on main, so the
landing tells its executor that further changes go in a new task. No other
command closes work in progress. `landed` takes the statuses this landing
closes (`from`; review and blocked when it names none) and checks them against
the status its write reads; its answer names the one it found, and a close from
`in_progress` addresses the verdict to `@executor` in that write.

A task that has posted a card for the owner is lifted out of its column into
a **Waiting for your sign-off** lane above the board. That needs all of:

- the task is not `done`, `blocked` or `cancelled`, and someone executes it;
- its body or thread names a card slug whole (a card named `x` is not
  named by a mention of `x-2`);
- that card is still undecided in a document the task links: the cards a
  document page counts as "N decisions waiting" (`docs_tree.pending`).

Deciding the card puts the task back in its column. Work nobody holds, or that
posted no card, stays in its own column however its documents stand.

**`done` is reached only by a reviewer's `approve`, a landing, or, for an
instruction subtask, its executor's `done`.** Several sessions
can hold one task at once, one role each:

| role | how | may |
|---|---|---|
| orchestrator | `just tasks role SLUG orchestrator` | coordinate: spawn the others, follow the thread; one a task |
| executor | `just tasks claim SLUG` | do the work, move it to `review` |
| reviewer | `just tasks role SLUG reviewer` | give the verdict: `approve` (done) or `changes` (back to the executor) |
| reserved | `just tasks reserve SLUG` | hold it for later, for an hour, so nobody else claims it |

A branch reaches `main` only through `just land`: the Symphony's default
([below](#the-default-landing)), or the consuming project's own, whose recipe replaces it.
Under a consuming project's landing policy its
reviewer gives one verdict, not two: `just land-approve BRANCH --tip SHA` with
the full commit it checked, from its own
session (`TASKS_SESSION`), holding the reviewer role on the branch's task,
recorded on the branch's patch (`--adversarial` for the second reviewer of a
tier 3 change). The landing then finishes the task in review for it: no board
`approve` is needed. A branch with no task gets `review/BRANCH` from
`just land-request BRANCH`, and the session that ran that (or the task's
executor) is the one that lands it.

### The default landing

`just land SLUG --as HANDLE` (`tasks land`, `crates/tasks/src/land.rs`) lands the
task's branch, named as its slug, on the default branch. The owner, on the card
reports-small-choices in [DECISIONS](DECISIONS.md): "convention over configuration",
and "lower level replaces higher level": a project's own `land` recipe replaces it.
One land of a repository runs at a time (`land.lock` in `sf-tasks/`); a second
waits its turn and says whose. The settings are read from the default branch as the
main checkout has it committed, never from the branch landing, so a branch cannot
change its own check.

- **Approved.** The task's last verdict is its reviewer's `approve`, which left it
  done, and the approval covers the branch's tip: the commit the verdict names, else,
  when it names none of the branch's, every commit of the branch made before the
  verdict. The board records no tip, so the landing's line on the thread then says it
  judged by time. A commit after the approved one refuses, naming it.
- **Merged and checked.** In a worktree of its own (`land` in the project's `[agents]
  worktrees`, else `sf-tasks/land/worktree`), never the main checkout: one `--no-ff`
  merge commit, "Merge branch 'SLUG'"; then the project's `[land] generators`, whose
  changes, inside their outputs alone, are one commit after it ("Regenerate NAMES after
  SLUG"); then `[verdicts] check`, its output in `sf-tasks/land/SLUG.log`. This
  repository's generator rebuilds the client distribution when it is stale
  ([CHECKS](CHECKS.md#checks-with-no-reviewer)).
- **Moved.** The main checkout, on the default branch with no staged or tracked edit,
  fast-forwards to what passed, so the default branch's new tree is the tree checked.
  When the default branch moved meanwhile, the merge and the check run again on its
  new tip; a red then is said on the thread to `@orchestrator`.
- **Pushed.** Unless `[land] push = false` or the repository has no `origin`, the
  default branch alone is pushed, no other branch and no tag, never forced: what
  `origin` gained since is fetched first, merged and checked the same way. A push that
  fails leaves the landing done here and says so in its line, with the command to
  retry; a merge with `origin` that conflicts is said to `@orchestrator` too.
- **Closed and swept.** `landed` writes the line `landed SLUG on main at SHA`, with what
  was regenerated and whether it pushed, and closes a task in review, blocked or in
  progress; an approved one stays done. The sweep then removes the task's finished
  worktrees.

Each refusal is one line, on the land's stderr and on the task's thread, and leaves
the default branch where it was: no approval standing, a commit after it, a main
checkout with edits or another branch checked out, a conflict (never resolved), a
generator that fails or writes outside its outputs, and a red check.

**A task has one orchestrator.** Taking the role on a task another session
holds while it is live, stale or of unknown liveness is refused, naming the
holder and the command that moves it: `just tasks handover SLUG HANDLE`, run by
the holder, drops its role and gives the role to HANDLE in one write. The
thread says `A handed orchestrator over to B; address B or @orchestrator`,
and B hears one `ROLE SLUG B orchestrator, STATUS (from A)`, whatever other
role B holds there; A made the move and is not told it. HANDLE must still be there (live, stale or
of unknown liveness); a hand-over to a session gone quiet, or leaving as it is
written, is refused, naming its health; under a cap, so is one that would raise
HANDLE's open tasks past it ([Slots](#slots)). Taking the role over a holder that is
lost or left drops that one in the same write, and the thread says `B took
orchestrator over from A (lost)`, which A hears and B, who made it, does not; so
does taking it again where a left holder still sits beside you. A task is its orchestrator's when that session holds the
role on it, or on the nearest task above it that has a holder still there; a
task with none is **unclaimed**, as are a lost or left orchestrator's tasks
until another takes them over. Each orchestrator hears its own tasks and the
unclaimed ones only (`crates/tasks/src/orchestration.rs`): every
orchestrator hears an unclaimed task's NEW lines, PULL digest rows and the
STATUS of its move to `blocked`, which no digest row shows, and only its owner
a claimed one's. A post to `@orchestrator`, and a
verdict, go to the nearest holder still there, passing one gone quiet. A
session gone stale or lost, or waiting in its terminal, is said to the
orchestrator of the work it holds, its LOST line naming only that
orchestrator's tasks and its BLOCKED line only a launch for one of them; a
session whose work is nobody's, or who holds none, is
said to its parent, as before. The digest's idle sessions leave out the
children of another orchestrator still there. Another orchestrator's task
reaches it only in a post to its own handle. `just tasks list --doubled` lists the tasks
that still hold two orchestrators from before the rule; one of them settles it
with `handover` or `release SLUG --role orchestrator`. No board data is
rewritten for them.

The executor can never review its own task, and the reviewer cannot claim it.
Of any number of simultaneous claims exactly one wins. `just tasks next` picks
work for you: your own reservation first, then priority, then the areas you
already work in (`--affinity`, and the slug prefix of tasks you hold).

## Subtasks

Every instruction an orchestrator gives an agent is a **subtask**: a task made
with `--parent`, whose description is the instruction, word for word. The agent's
whole prompt is then `/work SLUG HANDLE` (`.agents/skills/work/SKILL.md`), never a brief
in a file. A parent's subtasks nest to any depth; a task is never its own
ancestor (`just tasks parent SLUG PARENT` moves one, and refuses a cycle).

```
just tasks create perf/x/review "Review perf/x" --parent perf/x --for amber-heron --instruction --body-file - <<'EOF'
(the instruction, verbatim)
EOF
```

`--body-file -` reads the body from stdin, so a long instruction needs no file.
A consuming project's optional brief guard refuses writing a brief to a file: to
`artifacts/orchestrator/` (which must not exist), or to a brief in a session scratchpad (a file named
`brief*` or `*-brief.*`, or under `briefs/` or `followups/`), and says to
create a subtask instead.
A consuming project's optional wait guard refuses a loop waiting on machine-wide process state
(`pgrep`/`ps` for verify, test-sim, godot): wait on your own run's log or PID.

- `--for HANDLE` reserves it for that handle with no lapse of the hour (the
  handle is one the orchestrator registered for the agent). `next` offers it to no
  one else, and only that handle can claim it. The reservation still drops when
  that handle reads `lost` or `left` (ten minutes without a sign of life, holding
  nothing claimable yet), after which anyone may claim the task: start the agent
  promptly, or register the handle again and re-reserve.
- Every subtask closes with its executor's `just tasks done SLUG "integration commit and focused result"`, including ordinary implementation slices. Only the top-level parent enters review, gathers stamps and lands. `--instruction` still marks an agent instruction, but is not required for lightweight completion.
- Source slices start from their parent feature branch, commit there or return commits for integration, and run only their focused checks. A separate agent is justified only by useful parallelism. No independent slice review or landing. A parent's gate is evidence too, the tests closest to its change, which its reviewer judges; the full verification is no task's, and runs nightly ([CHECKS](CHECKS.md)).
- `just tasks show` lists a task's subtasks with each one's status and holder;
  `just tasks list` and the Symphony's **tasks** page, under its **Task
  groups** heading, show the same tree, at any depth.
- Events climb: a message to `@orchestrator`, any verdict and any
  status change on a subtask reaches the orchestrator of the nearest task, at
  or above it, that has one, so the orchestrator takes the role on the root
  only. A subtask handed to another orchestrator is that one's, with what is
  below it. A message to a handle or to another role stays with them.

## Slots

An orchestrator holds at most a set number of open tasks, its slots, so work is
pushed to done before new work starts (`crates/tasks/src/slots.rs`).
The number is the project's: `orchestrator_slots` in the `[board]` table of
`symphony.toml`, read from the repository's main checkout, as `on_review` is,
so a branch cannot raise its own cap. With no key, or no file, nothing is
capped. A value that does not parse, or one below 1, is said on stderr
(`⚠ …; no cap on an orchestrator's open tasks applies`) and caps nothing.

- **Open** is `in_progress` or `review`, or `blocked` under a hold that keeps
  the slot (below). Other `blocked` work, `backlog`, `concept`, `design` and
  `ready` hold no slot; a slot frees when its task is `done` or `cancelled`,
  or leaves the open statuses another way.
- **Each open task, one slot.** A subtask holds a slot of its own, so
  gathering open work under one task frees none. Two kinds of task share the
  slot of another: a review mirror (`review/BRANCH`, `review-N/BRANCH`,
  `SLUG/review`) counts with the task it reviews when the board holds that
  task, and an instruction subtask (`instruction: true`) with the nearest
  task above it that is not one.
- **Whose slot.** The orchestrator of the task, or of the nearest task above it
  that has one whose session still holds ([Subtasks](#subtasks)); a task two
  orchestrators hold counts against both, and a start is refused if either is
  full. A task no orchestrator holds counts against the parent session
  (`register --parent`) of the session starting it, or of its executors once
  open.
- **Where it bites.** A claim into `in_progress`; a move into `in_progress`
  or `review` from a status that holds no slot, blocked work going back to
  review included; a dependency change that sends blocked work back to
  `in_progress` or `review`; taking the `orchestrator` role on a task with
  open work at or below it, or being handed it (`handover`, refused as the
  receiver's take would be); and a change of parent that brings open work at
  or below the task under an orchestrator's tree. Each is refused when it would raise an
  orchestrator's open tasks past the cap, naming them and their status, a
  shared slot's tasks joined by `with`:
  `sim/x cannot start: cobalt-otter holds 8 open tasks, the cap of 8 an
  orchestrator holds: sim/a (review) with review/sim/a (review), …; a slot
  frees when one of them is done or cancelled`. The count is the number of
  entries listed. `tasks api` holds the same cap. A start inside a slot
  already open (a mirror of a task in review, an instruction for open work)
  is never refused.
- **A landing's hold.** A landing that holds a task in review blocked while
  every stamp its branch lacks is one nobody is producing does it with a hold
  (`Store::hold_blocked`, `tasks api`'s `hold`, api version 4): the task
  stays blocked, as any blocked task does to everyone on it, and keeps its
  slot, shown as `(blocked, held)`, so its move
  back to review starts nothing and is never refused. The hold is the record
  `counter:['hold', SLUG]`, stamped with the task's `updated` as the hold left
  it, and ends with the task's next move (a landing's close and the owner's
  moves included: every status write deletes it), change of parent or of
  dependencies.
  Work blocked by any other move holds no slot, and takes one to come back.
  The PULL header and the agents API do not read holds: they count a held task
  as the blocked task it is.
- **Red work takes no slot.** A task filed for a red that stops every
  landing is marked so where it is filed (`create`'s `red`, naming what is
  red, `tasks api` version 7; kept as the board field `fields._board.red`,
  `crates/tasks/src/red.rs`). Only a red's filer marks: a session of
  kind `filer`, which `tasks api`'s `register` takes and the command line's
  `register` does not, as the project's tools that file reds register. A
  create naming `red` from any other session is refused, and `set_field`
  refuses the board's own `_board` fields. The mark goes only on a task of a
  series reds are filed in (`red::SERIES`: `tests/main-red-*`, a red verify
  of main, and `tests/nightly-red-*`, a red night), whoever marks it, so no
  feature takes it; never on a task named under one (`red::filed`: no `/`
  past the series' prefix), which the nightly's reading of its reds and
  land's of the standing reds take for ordinary work too. `mark_red` marks an open red its
  filer filed before it marked reds, and makes it p0, once: what it judges is
  read in the transaction that writes, so two at once post one line. While the
  marked task is open, it and the work under it take no slot: subtasks,
  review mirrors and instructions are judged by the task whose work they are
  and the tasks above it. Red work starts at any count and frees none. A red
  done or cancelled exempts nothing: the work under it counts again, new
  work there included. Its close is never refused, though it raises its
  owners' counts: whether or not the closing store caps starts (the
  nightly's does not), it holds their slot counters and the red's own,
  `counter:['red', SLUG]`, which every start of work under the open red holds
  too, so a start decided on a board read before the close is decided again,
  counted. A close decides what it holds on the task as it read it, and its
  write goes on only from that version, so a red marked between the two
  (an earlier red the nightly or main's verify marks) is read and decided
  again, and a task it read as not on the board is refused there, never
  closed unfenced as a red filed since. `unmark_red`, from a filer too, lifts
  the mark of an open red that stays open as ordinary work (main's green
  reached it once work on it started): it and the work under it count from
  then, decided and held as a close is, never refused; a task unmarked or
  closed is left as it is. A refusal names the owner's open red work after its
  counted slots: `…; a slot frees when one of them is done or cancelled; not
  counted, as red work takes no slot: tests/main-red-abc (in_progress)`. The
  mark alone tells; a slug never does. The PULL header's `slots=` and the
  agents API count no red work. As with review, nothing authenticates a
  session: a filer is honour-based, guarding against a mistake, not malice.
- **What it leaves alone.** The owner's moves, changes of parent and
  hand-overs are not capped.
- **Not retroactive.** An orchestrator above the cap keeps every task and
  works them; it starts nothing new until it is below the cap.

Two starts at once cannot both take the last slot: every start that changes an
orchestrator's open tasks, one into a slot already open included, holds
`counter:['slots', HANDLE]`, read in the same transaction as the task it
starts, and the write that loses is decided again on the new board. The cap
reads the open tasks through the `task_status` index, the holds as a range of
record ids and their tasks by id, the tasks above them and the tasks they
review by id, a level a round trip, and their roles through `role_slug`; it
reads no table whole (`tests/store/slots/cost.rs`). Each
orchestrator's PULL header shows `slots=OPEN/CAP`, and the agents API its
`slots` ([The agents API](#the-agents-api)).

## The pipeline alarm

Work piling up in review is said, not left for someone to notice
(`crates/tasks/src/alarm.rs`). Review holding more than twice the work
in progress for 4 hours raises the alarm; it clears once review has held no
more than twice in progress for an hour, so a short dip does not lower it and
a short rise does not raise it. Exactly twice is not over; review with nothing
in progress is.

- **Thresholds.** The project's, beside `orchestrator_slots` in the `[board]`
  table of `symphony.toml`, read from the main checkout as the cap is:
  `review_alarm_ratio` (2), `review_alarm_hours` (4) and
  `review_alarm_clear_hours` (1), whole numbers of at least 1. A key not
  written takes its default; a value that does not parse, or one below 1, is
  said on stderr (`⚠ …; the pipeline alarm takes its default thresholds`) and
  the defaults apply.
- **What counts.** Work only: a review mirror counts with the task it reviews
  and an instruction subtask with the task it serves, as for
  [Slots](#slots), neither on its own, as the tree stood at each moment: a
  mirror counts on its own until the task it reviews is made, and an
  instruction subtask from when it leaves its parent (a move's line names the
  parent it left, `HANDLE put it under PARENT (was PARENT)`), so a task made or
  moved changes the counts from then on, never the hours before. A task in
  review whose branch's tip a reviewer asked changes of is rework (below),
  counted in progress.
- **From the record.** The board keeps no history of counts. Every change of a
  task's status writes a line on its thread (a claim, a move, a verdict, a
  landing, a block by a cancelled dependency and its end), so each reader walks
  the last day's lines (or twice the raise and clear spans, if longer) back
  from each task's status now and replays the alarm over the counts. What held
  when that window opened is read, never assumed: a task's status then, where
  its oldest line in the window is a claim or a landing, and the record of
  what its landing waits on then, for one in review some time in the window,
  come from its newest such lines before the window, read back past any field
  value whose text only reads like one; and when the window holds neither a
  raise span over nor a clear span under, so the alarm's state at its start
  decides, a window twice as long is replayed, until one does or it opens
  before the board's first task, when the alarm was lowered. Every reader at
  one moment finds the same alarm, and
  nothing writes it. The window's lines are read through the message time
  index and those before it from each task's own thread; no table is read
  whole.
- **What it says.** While it stands, every orchestrator's PULL header ends
  `review_alarm=REVIEW/IN_PROGRESS`, the whole board's whoever the digest is
  for, and the lines after the header say the counts and since when, then the
  5 tasks longest in review, longest first, each with when it went in and
  what its landing waits on:

  ```text
  PULL 14:05 executing=4 … idle=0 review_alarm=10/4
    review_alarm: review 10, in progress 4, more than 2× since 09:40
    review_alarm p1 tools/x — X   (in review since 11-07 21:12; missing certificate, review)
  ```

  In the hour that clears it, the line says `at most 2× since HH:MM; it clears
  after 1 h of that`. Its raising is an entry of its own, so the monitor
  delivers it once, as it delivers any new entry.
- **What a landing waits on.** The board holds no stamps; the tool that lands
  a task records what its landing lacks in the board field
  `fields._board.waits` (`Store::record_waits`; `set_field` is a plugin's): the tip it judged, `: `, then
  each name, `, ` between them (`0123abc…: certificate, review`), none after
  the colon when it lacks nothing. The alarm shows the names as written
  (`missing certificate, review`), `missing nothing` for none, and `its tip is
  newer than its landing's last look` when the branch has moved past the tip
  the record judged. A task with no record shows no note. Each record names
  the assessment it states, kept with when it was written
  (`fields._board.assessed`): the same assessment retried writes nothing,
  and a fresh one writes its record again however like the last it reads.
  Stamp and approval reassessment, refused landing attempts and completed tip
  verification submit these assessments through a detached, owned delivery.
  It checks the current branch and task state, evaluates stamps and merge
  conflicts, and records `changes` only for an effective reviewer veto. A
  superseded interrupted verification supplies no completed assessment.
  Local submissions and receipts live under `sf-land/assessments/` in the
  common directory; the delivery child is stopped and reaped after 30 seconds
  or checkout removal. A delayed board, an older API or an unconfirmed reply
  leaves pending evidence; `just land-stamps BRANCH` retries the latest
  submission, preserving a committed occurrence's identity, time and line.
  Pending evidence alone does not confirm a board write.
- **Rework.** The name `changes` in a record says a reviewer asked changes of
  that tip. While the branch's tip is still the one judged and the branch has
  not moved past it since the record, the task is rework: PULL counts it in progress (its live executor as executing, and one
  nobody executes as `in_progress_orphaned`, noted `in review; changes asked
  of its tip`), not as `review_unreviewed`, and the alarm counts it in progress
  from the record to the next; a change of status ends nothing, since the
  record judged a tip, which a move into progress and back keeps. The first
  commit the branch moved to from the tip judged, after the record, ends it,
  from the moment that commit was made or the branch moved to it, however late
  its record comes and whatever follows (more commits, the task taken back
  into progress): the task waits on review again. The moves come from the
  repository the reader runs in, as the branch's reference log tells them: the
  next commit made on the tip judged, or what replaced it, amended or rebased
  away. Without that log, the first commit after the tip judged on the
  branch's line of first parents ends it, never one that reaches it through a
  merge's other parent; a reader that cannot see a task's branch takes each
  record as judging the tip of its time, and one that cannot place the tip
  judged takes the tip now as the first after it. The board keeps the first
  end it learns of each record (`fields._board.fixed`), and nothing later
  moves it: not another commit, an amend or a rebase, nor what the repository
  tells after. A new record of changes on a tip the branch came back to, from
  a fresh assessment, is a request of its own, whatever its text: it waits on
  the branch's next move from that tip. The record answered, retried, is no
  new request: PULL and the alarm count the task in review alike.

## Where the time goes

The board keeps no table of status changes. Where a task's time went is read
back from what it already stores: the task's `created`, `updated` and `status`,
and the `system` and `verdict` lines its changes left on its thread
(`crates/tasks/src/stages.rs`). The store builds its move and claim lines
with that module's functions and the release, block and unblock lines from its
constants; the verdict words and the `(was STATUS)` ending are written out on
both sides.

| line | what it changes |
|---|---|
| `HANDLE moved it FROM → TO` | the move it names, the owner's too |
| `HANDLE claimed it[, handed over from …]` | ready work is in progress; a claim of work in another status changes none |
| `HANDLE released executor (released)`, `HANDLE released every role (left)` | work in progress whose last executor let go is ready again |
| `blocked: cancelled dependencies: … (was STATUS)` | blocked |
| `unblocked: no cancelled dependency left; back to STATUS` | the status it names |
| a `changes` verdict | review back to in progress: a bounce |
| any other verdict (`approve`, a subtask's `done`, a landing) | done |

The lines are replayed in the order of their message ids, the order they were
written in: a line's time is its writer's clock, so one dated before the line
written ahead of it is taken at that line's time. A line naming the status
the task already holds changes nothing, and nothing after the line that closed
the task is read. The status a task was made in is the one the first line that
names a status says it left; with no such line it is the task's own status
where a task can be made in it, else `ready`. Where the replay ends in another
status than the task holds (a change the thread does not record), that change
is dated at the task's `updated`. An open task is counted up to now; a done or
cancelled one stops at the line that closed it, and the closing status itself
counts no time. The statuses' times add up to the time from creation to close
(or to now).

`just tasks show SLUG` prints it in one line under the task's roles and links:

```
  Stages: ready 0.4 h · in_progress 1.3 h (2 visits) · review 0.8 h · 1 bounce; first claimed after 0.4 h
```

Each status is in the order the task first held it, with its visits when more
than one; a bounce is review sending the task back to in progress, by a
`changes` verdict or a move. After the `;` comes the wait from the task's
creation to its first claim (`never claimed in 20.0 h` when nobody claimed it:
to its close, else to now). That wait is a part of the statuses before it,
usually the time in `ready`, and is never added to them.
`just tasks show SLUG --json` carries the same as `stages`, every duration in
seconds and every time in epoch seconds:

```json
"stages": {
  "statuses": {
    "ready": {"first_entered": 1800000000.0, "seconds": 1440.0, "visits": 1},
    "in_progress": {"first_entered": 1800001440.0, "seconds": 4680.0, "visits": 2}
  },
  "unclaimed_seconds": 1440.0,
  "first_claimed": 1800001440.0,
  "bounces": 1,
  "current": {"status": "in_progress", "since": 1800006000.0},
  "closed": null
}
```

`just tasks stats [--days N] [--json]` (14 days unless `--days` says, from 1
to 3650; no session needed) is the same reading over every top-level task
made in the window, subtasks left out. Every figure is of those tasks but the
tasks done per day, which its header says:

- how many hold each status;
- for the done ones, the lead time (creation to close) and the time in each
  open status, each as median, p75 and p90 (a value between two tasks is
  taken proportionally between them); a status is spread over the done tasks
  that held it, and says how many did;
- on a line of its own, the done ones' wait before their first claim: a part
  of the statuses above it, not a row to add to them;
- how many tasks review sent back how often (`0 × 41 · 1 × 6 · 2 × 1`);
- top-level tasks done on each day of the window, whenever they were made (a
  task made a month ago and closed yesterday counts yesterday), by this
  machine's time zone, every date of the window named once;
- the ten open tasks that have held their status longest, with that status
  and how long;
- how many tasks are blocked and how many are ready with no executor, each
  with the median time they have been there.

`--json` carries the same figures as `by_status`, `lead_seconds`,
`stage_seconds` (by status; each spread is `tasks`, `median`, `p75`, `p90`),
`waiting_before_first_claim_seconds` (beside `stage_seconds`, never in it),
`bounces`, `done_per_day`, `slowest`, `blocked` and
`ready_unstaffed` (`tasks`, `median_seconds`), every duration in seconds.

`stats` reads the window in one round trip (`Store::stage_records`): the
top-level tasks made since the window's start and the done ones written since
(a task closed in the window is one of them), who executes them, and each one's thread
by its own range of keys with the posts left out, never a thread at a time.
With the read that opens the board that is two requests whatever the board
holds (`tests/cli/stats.rs` counts them over 300 tasks).

## Talking on a task

Every task has a thread, and the thread is the way to reach whoever is on the
task, whatever worktree or machine it runs in:

```
just tasks say sim/jump-departure "the dock ring is at 900" --to @reviewer
```

`--to` takes a handle, a role (`@executor`, `@reviewer`, `@orchestrator`,
`@reserved`), or `owner`. Addressed messages reach the matching native callback
as plain `MSG` event lines. Without an address, an agent's post stays on the
thread; the orchestrator reads it in the app or with `just tasks show SLUG`.
Verdicts reach the executor and orchestrator, and owner messages reach everyone
on the task. Post milestones only, address what someone must act on, never
acknowledge a message, and never repeat a post on a subtask and its parent.
A session without a channel keeps `just tasks inbox`.

The owner reads the board and every thread in the Symphony, sends messages
to anyone on a task, changes its priority from p0 to p4 and cancels open tasks.
Cancellation asks for confirmation; done and cancelled tasks cannot be cancelled
again. The board records these writes as `owner`. In the shared app, cancellation
also closes the task's sessions and starts the worktree sweep, as the CLI does.
An agent message addressed to `owner` rings the app's 🔔. Creating, claiming,
other moves and verdicts go through agents, and one task the app files itself: a verdict
that waits on an agent goes on a `verdicts/…` task, created as the app's
own `app` session, never as the owner ([document review](REVIEW.md)).

## Links with the documents

A task's `--link` makes its spec show a **Tasks** panel at the bottom of that
document's page in the Symphony. In the other direction, write
`[[task:sim/jump-departure]]` in any document: it renders as a link to the
task, with its live status beside it.

## Limits

- **One machine.** The board's server listens on this machine alone
  (`127.0.0.1`) and keeps its data in `.git/`; a session on another machine or
  in the cloud cannot reach it.
- **The owner's review of a task is the reviewer's verdict**, not a web button.
  The owner steers through the thread.
- **Nothing is reassigned automatically.** A lost session's work becomes
  claimable, and the claim that takes it over says so in the thread.
- **A subagent is seen through its tool calls and what it left running.** One
  waiting on a background job without calling a tool reads `running_test` while
  a process it started works in the linked worktree its last tool call ran in;
  one with nothing of its own working there (or in the main checkout, which says
  nothing of whose a process is) goes stale after 90 s, and lost after 10 minutes if it holds
  claimable work. The board's own health (`just tasks show`, a claim) still ages
  by its beats alone.
