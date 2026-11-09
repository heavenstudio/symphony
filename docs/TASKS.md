# Task board

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

The store takes about what it holds. Every server the board starts keeps each
value beside its key, where compaction drops the versions a rewrite leaves, and
logs at most 256 MiB of writes before it moves them to an sstable. SurrealKV's
default had sent every value over 4 KiB to a value log that only grows: 102 GiB
held a board of 0.5 GiB. `just tasks-reclaim`, the owner's to run from the main checkout,
rebuilds a store that grew that way. It stops the server, exports every
database, imports them into a new store, and checks that store's exports byte
for byte against the old ones. Only then does it swap the stores and remove the
old one. The exports stay in `sf-tasks/reclaim/exports/`. `--dir FOLDER`
rehearses on a copy of `sf-tasks/` (`cp -c`, a clone).

The command is the `tasks` binary (`symphony/crates/tasks`), built from this tree's
sources once for every worktree that shares them (`tools/rs-bin.sh`): the
first command after a change to them builds it, which takes a minute or two.
The heartbeat hook runs the build its tree last ran (`.target/bin/tasks`) and
builds nothing. There is no Python board adapter.
`launch`, `restore`, `cleanup`, `close` and `reap` are its own, hc's terminals and all
(`symphony/crates/tasks/src/launch/`); the board calls no `herdr`. The server is the pinned SurrealDB, fetched once
per machine into `sf-surreal/` in the git common dir by `bash tools/surreal.sh`
(the Rust tests fetch it the same way); the board says so when it is missing.

| | |
|---|---|
| `just tasks …` | everything an agent does (`just tasks --help`) |
| `just tasks search QUERY [--status S] [--limit N] [--json]` | ranked full-text matches in task titles, briefs, slugs and threads; task rows with matching excerpts (default: 20); query capped at 1024 UTF-8 bytes and 32 terms (word/number runs and punctuation/symbols, after lowercasing) |
| `just tasks callback [ADDRESS] --as HANDLE` | register a native channel; addressed events start a turn directly |
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
suspended after delivery failures waits for the sweep to restore it.
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

**Agent sessions behind a task** (`task_sessions`): `just tasks launch SLUG` starts a
Codex agent (`--no-daemon`; default model `gpt-6.1-sol`, effort `xhigh`, workspace-write
with network access) in an hc terminal of its own, `hc run --bg --no-parent --outlive-parent
--keep`: no child of the caller's terminal, which the app would show under it rather than in
a tab. `--kind claude --model sonnet` selects Claude. The
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
says on its thread that restore cannot resume it. See the [tasks skill](../../.agents/skills/tasks/SKILL.md#launch-restore-clean-up-herdr).

**Which agent starts where** is a [standing rule](../../RULES.md#workflowclaude-first-agents):
executors and reviewers are Claude, started as in-process subagents or
`claude -p`, with a handle registered for them and a subtask made `--for` it. A
terminal, `just tasks launch SLUG --kind K` (`--role reviewer` for a reviewer),
holds work that needs one: Codex's adversarial review of a branch a Claude
executor wrote (`gpt-6.1-sol` or better, `xhigh`), its image generation and 3D
modelling, or an agent that must be watched or outlive its orchestrator
([task-orchestrator](../../.agents/skills/task-orchestrator/SKILL.md#which-way-an-agent-starts)).

**Optional automatic staffing:** `just tasks staff [--dry-run]` makes one pass, highest
priority first. Enable it in [task_staff.json](../../tools/task_staff.json): `enabled`
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
executor is never reused. Otherwise it launches Codex in a separate worktree: the
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
app's `cua-repl.mjs` under its own `node` among them, each by its exact path),
caffeinate and board commands: anything else is work. Status, roles and the window are read again right before the close, and the terminal and the window once
more after the close is recorded, right before the kill: work resumed by then (its agent
working, or work under it) or the window showing it reopens the record. Never an unnamed terminal or another board's (it may be the owner's), an
orchestrator, the caller's (`HC_TERMINAL_ID`), or one on screen in the Herdr app, which
starts its idle wait again (as does a window that cannot say what it shows).
Each close is one line on the handle's task thread, with its restore line, written
before the terminal is closed. `just tasks reap --dry-run` names what the next sweep would close.
Code: `symphony/crates/tasks/src/launch/reap.rs`.

**Finished worktrees go by themselves** (owner, 2026-10-21: "do any cleanups for worktrees
that can help decrease needless disk usage. No need to ask, this should happen automatically
when tasks are complete"). The worktree sweep (`just tasks sweep`, the rules in
[tools/AGENTS.md](../../tools/AGENTS.md#landing-on-main)) starts, detached, whenever work
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
`TASKS_SWEEP=on`; `TASKS_SWEEP=off` starts none anywhere. Code: `symphony/crates/tasks/src/sweep.rs`.

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
(`sf-hook tasks-beat`, `just tasks beat`) see the Claude Code agent id
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
one that may run longest keeps the mark, and only its own end clears it. A
subagent that stops makes no more calls, and ages to stale and lost as before.

Orchestrators can share the board, each holding its own slug prefixes. A decision
one side makes that the other must act on becomes a short board task for that side,
and a side that approves documents commits them regularly, because worktree agents
see only what is committed.

## The agents API

The app is the source of truth for who is doing what: `GET /api/agents` of the Symphony and `just tasks agents
[--json] [--all]` return the same document (`symphony/crates/tasks/src/cli/agents.rs`; these keys are a contract, pinned by
`symphony/crates/tasks/tests/cli/agents.rs`). The board (sessions, roles, `task_sessions`) is the record; hc's
live terminals (`hc ls`) and the test-slot locks are probes that say what each agent is doing now, and `probes` names
either as `unavailable` when it cannot be asked, the rest being built from the board alone. The hc probe keeps the
key `herdr` and an agent's terminal the key `pane`: names the wire keeps. The Symphony's `/api/agents` also names
that probe `panes`, with the same answer under both keys, and its client reads `panes`, else `herdr`
(`symphony/crates/symphony-api/src/agent_probes.rs`), so a client and a server one build apart still agree; the
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
`phase: waiting`, `state_detail` naming the recipe and since when), `waiting_on_input` (hc sees an approval or a
question), `idle`, or `gone` (no terminal and no sign of life). An agent running or queued for its own verify reads
`running_test` and `health: live`, never `lost`. `last_tool_call` is the time of the last tool call of the agent
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
  else its latest), else from its terminal's working directory (and the branch checked out there), else, only with
  neither, the directory `register` ran in: an agent that was never launched and has no terminal.
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
- **state.** A matched run is `running_test`. Else the terminal's status (`working`, `blocked` is `waiting_on_input`, `idle`
  and `done` are `idle`). Else the board's health: live is `working`, stale `idle`, lost or left `gone`. A matched run also lifts
  `stale` and `lost` to `live`.
- **A probe that fails** is `unavailable` in `probes`, and the answer is built from the board; a failing branch lookup is an
  empty branch.

Slot holders and waiters come from `test_slots.runs` (a held slot file whose writer is alive and whose lock is held; a
place in the pool's FIFO line, `<prefix>-<12-digit sequence>.place`, in the pool's own prefix: `slot-` and `render-`, that its waiter still holds); a slot file whose writer is dead is free and is not locked to find out.

An orchestrator's callback uses it: it does not call an agent stale or lost while `running_test`, and it receives a PULL
digest (`symphony/crates/tasks/src/cli/pull.rs`) once at its first dispatch, even when empty, then only when an actionable list gains an entry absent from the last delivered PULL after forgetting entries absent from any computed digest: red tasks ([Slots](#slots)) with no live executor 10 minutes after they were filed, in any status short of review, listed first as `red_unstaffed`, counted in the header only when there is one, and in no other list, `dependency_blocked` included, a cancelled dependency of its named in its note; ready tasks with no live executor
(one on a subtask still to be executed counts), in-progress with none, review with no reviewer, tasks that landed
(the branch is on main, or `landed <slug> on main` is on the task's thread) and are still open (a status change on the task after that line voids it, whether a move, a verdict such as `changes`, or a re-claim: the task is new work, not a landing left open), and idle agents, each scoped to the orchestrator's own and unclaimed tasks (an idle agent another orchestrator launched is left out). An executor
or reviewer that is `running_test` counts as present however quiet the board says it is.
Counts and the clock do not trigger delivery. A departure alone pushes nothing;
an entry that stays present is never re-sent, and an ordinary entry that leaves and returns pushes once.
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
hold its orchestrator names: [MERGING](../../docs/workflow/MERGING.md#stamps))
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

A branch reaches `main` only through `just land` ([MERGING](../../docs/workflow/MERGING.md)). Its
reviewer gives one verdict, not two: `just land-approve BRANCH --tip SHA` with
the full commit it checked, from its own
session (`TASKS_SESSION`), holding the reviewer role on the branch's task,
recorded on the branch's patch (`--adversarial` for the second reviewer of a
tier 3 change). The landing then finishes the task in review for it: no board
`approve` is needed. A branch with no task gets `review/BRANCH` from
`just land-request BRANCH`, and the session that ran that (or the task's
executor) is the one that lands it.

**A task has one orchestrator.** Taking the role on a task another session
holds while it is live, stale or of unknown liveness is refused, naming the
holder and the command that moves it: `just tasks handover SLUG HANDLE`, run by
the holder, drops its role and gives the role to HANDLE in one write. The
thread says `A handed orchestrator over to B; address B or @orchestrator`,
and each of the two hears one `ROLE SLUG B orchestrator, STATUS (from A)`,
whatever other role B holds there. HANDLE must still be there (live, stale or
of unknown liveness); a hand-over to a session gone quiet, or leaving as it is
written, is refused, naming its health; under a cap, so is one that would raise
HANDLE's open tasks past it ([Slots](#slots)). Taking the role over a holder that is
lost or left drops that one in the same write, and the thread says `B took
orchestrator over from A (lost)`; so does taking it again where a left holder
still sits beside you. A task is its orchestrator's when that session holds the
role on it, or on the nearest task above it that has a holder still there; a
task with none is **unclaimed**, as are a lost or left orchestrator's tasks
until another takes them over. Each orchestrator hears its own tasks and the
unclaimed ones only (`symphony/crates/tasks/src/orchestration.rs`): every
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
A hook (`sf-hook brief-guard`) refuses writing a brief to a file: to
`artifacts/orchestrator/` (which must not exist), or to a brief in a session scratchpad (a file named
`brief*` or `*-brief.*`, or under `briefs/` or `followups/`), and says to
create a subtask instead.
A second hook (`sf-hook wait-guard`) refuses a loop waiting on machine-wide process state
(`pgrep`/`ps` for verify, test-sim, godot): wait on your own run's log or PID.

- `--for HANDLE` reserves it for that handle with no lapse of the hour (the
  handle is one the orchestrator registered for the agent). `next` offers it to no
  one else, and only that handle can claim it. The reservation still drops when
  that handle reads `lost` or `left` (ten minutes without a sign of life, holding
  nothing claimable yet), after which anyone may claim the task: start the agent
  promptly, or register the handle again and re-reserve.
- `--instruction` marks a subtask that closes itself, which needs a parent. Its
  executor ends it with `just tasks done SLUG "what came of it"`, the note
  being its result, and it goes `in_progress → done`. An instruction task never goes to `review` (`move` refuses it). Nothing else reaches
  `done` without a verdict or a landing: `done` is refused on any task not
  made with `--instruction`. The flag is how the board tells a task nobody lands
  from one that lands; the board does not look at branches. An instruction task
  is never the task a branch lands under (`just land` ignores it), so a branch
  of the same name gets its own `review/BRANCH`.
- `just tasks show` lists a task's subtasks with each one's status and holder;
  `just tasks list` and the Symphony's **tasks** page show the same tree, at
  any depth.
- Events climb: a message to `@orchestrator`, any verdict and any
  status change on a subtask reaches the orchestrator of the nearest task, at
  or above it, that has one, so the orchestrator takes the role on the root
  only. A subtask handed to another orchestrator is that one's, with what is
  below it. A message to a handle or to another role stays with them.

## Slots

An orchestrator holds at most a set number of open tasks, its slots, so work is
pushed to done before new work starts (`symphony/crates/tasks/src/slots.rs`).
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
  `symphony/crates/tasks/src/red.rs`). Only a red's filer marks: a session of
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
own `app` session, never as the owner ([REVIEW](../../docs/workflow/REVIEW.md)).

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
- **A subagent is seen only through its tool calls.** One waiting on a
  background job without calling a tool goes stale after 90 s, and lost after
  10 minutes if it holds claimable work.
