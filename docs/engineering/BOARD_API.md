# The board's JSON APIs

The two contracts a program, not a person, reads the board by: `tasks api`, the board's methods
as JSON, and the agents API, who is doing what. Using the board is [TASKS](../workflow/TASKS.md); running
it is [BOARD_OPERATIONS](../operations/BOARD_OPERATIONS.md).

## The tasks API

`tasks api` is the board's methods as JSON, for a program rather than a person: one call
`{"method": NAME, "args": {…}}`, or a list of them, on stdin, each answered `{"ok": RESULT}`,
`{"refused": WHY}` (the board's refusal, which wrote nothing) or `{"error": WHY}`; `tasks api
--lines` answers a call per line over one connection (`crates/tasks/src/cli/api.rs`). It is how a
project's own program reaches its board without linking the board's library.

It is a contract, versioned `MAJOR.MINOR`. `{"method": "version"}` answers
`{"api": {"major": M, "minor": N}, "schema": S}`, S the board schema the build writes, without
reaching the board: a program that must know what a build serves, such as a project's launcher
choosing between builds, asks it, never reading Symphony's source. Within a major a method, an
argument or a field of an answer is added, never removed, renamed or changed in meaning, and an
argument added has a default, its absence read as before; each addition raises the minor, and the
api version this guide gives a method (`hold`, api version 4) is its minor of major 1. A new major
is a task of its own. The calls a registered project's programs make today are held by
`crates/tasks/tests/cli/api/contract.rs`: each request, and the fields of its answer the caller
reads with their JSON types, so a change that would break one fails its own task's gate. What the
api promises is its calls, not the board's rules behind them, such as who may move or judge a
task. Major 2 began at 2.0: `create` refuses `fast_fix`, including a null value. Tasks needing no
review name their stamps through an assessment. The fields and calls held by the consumer contract
keep their shapes. The current API is 2.6: minor 5 added `attempts`, `{"slug": S}`, a task's
execution attempts as `symphony_api::TaskAttempts`, read as the board's own service under the
project the call runs in, refused from outside a project's checkout
([EXECUTION_ATTEMPTS](EXECUTION_ATTEMPTS.md#on-a-tasks-page)); minor 6 added what a guarded edge is
taken with ([below](#a-task-a-custom-workflow-holds)).

### What a program outside the board keeps

Minor 1 of major 2 adds what a program needs to exchange tasks with another system, an issue
tracker for one, without reading the database. It names no such system: the program is the
project's own, and nothing here makes a request, keeps a credential or a mapping, or turns one on.

- **`id`**, a field of every task the api answers (`get`, `tasks`, `create` and the rest): the
  task's number, the order it was made in (`crates/tasks/src/rows.rs`). A retitle keeps it, and a
  task has no other name to change: no call renames a slug. It names the task only along one
  history of the board, never by itself across a copy or a restore, which give numbers again
  ([a board that was copied or restored](../operations/BOARD_OPERATIONS.md#a-board-that-was-copied-or-restored)): a program keeps it with the generation it was
  read under, which is the token. `just tasks show`, `list` and `search` print that same record
  under `--json` (`crates/tasks/src/cli/shape.rs`), so each task there carries its `id` too.
- **`bound`**, `{"slug": S}`: the task's title and brief and its token, `Store::bound`'s answer,
  `{"bound": {"compat": 1, "generation": "…", "id": N, "version": N}, "title": "…", "body": "…"}`.
- **`edit_bound`**, `{"slug": S, "bound": TOKEN, "by": HANDLE}` with any of `"title"`, `"body"`
  (the brief, whole) and `"links"`, what is left out staying as it is (`Store::edit_bound`): the
  one call that rewrites a brief. It is `Store::amend_bound`'s write, the fence first in the
  write's own transaction: refused unless the board holds the token's generation and the task is
  the one of that number at that version, and a refusal writes nothing, no title, brief, link,
  line on the thread or act of its session. A changed title and a changed brief are each a line
  on the thread naming who wrote it; the brief's earlier words are not kept, so a program that
  must keep them reads them first, with the token. Its answer is `bound`'s, the task as the write
  left it with the token to write it next. A call that gives none of the three is refused.
- **`integration_state`**, `{"name": NAME}`: `{"path": "…"}`, the folder the integration NAME
  keeps its state in, `symphony/integrations/NAME` in the git common dir
  (`crates/tasks/src/folders.rs`). It is the same from the main checkout and every linked
  worktree, is in no worktree and so outlives each, and is in no commit. The call reaches no
  board and makes nothing: the program makes the folder, keeps what it needs there and takes its
  own lock, and the board never reads, sweeps or copies it, so it is in no dump or snapshot of
  the board and is backed up with the repository's common dir or not at all. NAME is one to
  sixty-four lowercase letters, digits and hyphens, a letter or a digit first, never a path.

`bound` and `edit_bound` answer only where the board has a recovery binding: `tasks api` hands
its store the board home's ([a board that was copied or restored](../operations/BOARD_OPERATIONS.md#a-board-that-was-copied-or-restored)), so on a board
`TASKS_SERVER` names, and on one never bound or revoked, both are refused in words that say which,
and every other call is as it was. What the token does not tell apart is said there too, and the
api claims no more: a program that finds its token refused reads the task again and reconciles
what it kept, never writing on the number alone.

### The evidence of a task at its head

Minor 2 adds one read, for the program that makes a task's evidence public
([a land delivered by a pull request](../workflow/TASKS.md#a-land-delivered-by-a-pull-request)).

- **`evidence`**, `{"slug": S}` with an optional `"head"`: the task's evidence at the tip of the
  branch named as it in the main checkout, refused when `head` is given and is not that tip. It
  writes nothing and names no host. Its answer is one snapshot
  (`crates/tasks/src/land/evidence.rs`): `version` (1), `slug`, `task` (the task's `id`),
  `status`, `head`, `branch` (the default branch), `policy` and `catalogue` (digests of the
  committed project file and of the stamps' catalogue), `assessment` (who assessed, the commit
  read, why, the stamps named, and from minor 3 `import`, the whole upstream commit it names the
  head an import of, only where it names one) and `assessed` (whether it read the head), `stamps` (every stamp of
  the catalogue, each `met`, `waived` or `missing` with its reader's line and the runs or thread
  lines it stands on), `blocking` (each reason it is not eligible), `eligible`, and `revision`, a
  digest of all of it: two snapshots of one revision say the same.

It is the reading the land itself goes by, one function, and stricter than the default land in
what ties evidence to the head: the assessment is of the head, so one of another commit or of the
brief waives nothing and none is no waiver; the approve names the head, never covering it by
time; and every stamp is said, a waived one with who waived it and why, the assessment or a
[gate decision](../workflow/TASKS.md#gate-decisions) for the head. A raise a gate decision gave that the head does
not carry is among `blocking`, as it refuses the default land.

### A task a custom workflow holds

Minor 4 adds the three calls by which an agent reads and moves a task a custom workflow holds,
through the board's one admission ([WORKFLOW](../workflow/WORKFLOW.md#admission)). On a board with no custom
workflow active, `workflow_read` answers null and the other two are refused, saying none is
active. Each answer is a type of `symphony_api::workflow`.

- **`workflow_read`**, `{}`: the active definition, its revision and its activation generation
  (`WorkflowRead`).
- **`workflow_actions`**, `{"slug": S, "handle": H}`: each edge leaving the task's pair, with what
  stops session `H` taking it now and its authority named without who holds it (`NextActions`):
  a preview, never a permit.
- **`workflow_apply`**, `{"handle": H, "request": OperationRequest}`: one typed operation admitted
  for session `H`, which must be on the board and not have left, never for a principal the request
  names; its answer is the `OperationResult`, and its refusal names its code.

Minor 6 adds what a guarded edge is taken with ([WORKFLOW](../workflow/WORKFLOW.md#guards)):

- **`workflow_apply`** takes `"attestations": [Signed]` beside its request, the project
  authority's signed word for each guard of the edge; without it a guarded edge is refused
  `guard_unavailable`.
- **`workflow_observe`**, `{"signed": Signed}`: an authority's signed observation of its state,
  which the board accepts only from the state it accepted before; its answer is `{"observed":
  "accepted" | "unchanged" | "conflicted", "authority": …}`.

## The agents API

The app is the source of truth for who is doing what: `GET /api/agents` of the Symphony and `just tasks agents
[--json] [--all]` return the same document (`crates/tasks/src/cli/agents.rs`; these keys are a contract, pinned by
`crates/tasks/tests/cli/agents.rs`). The board (sessions, roles, `task_sessions`) is the record; hc's
live terminals (`hc ls`) and the test-slot locks in the folder the project's `[board] slot_locks` names
(none when unset) are probes that say what each agent is doing now, and `probes` names
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
   "following": 0,                                   # the open tasks it orchestrates
   "worktree": "/…/lamp-works-wt/sim-jump", "branch": "sim/jump",   # where it really works
   "agent": "claude", "session_id": "9aa0…", "pane": "t5",
   "callback": "uds:/tmp/cc-socks/1234.sock", "chat": false,
   "state": "running_test",
   "state_detail": {"recipe": "verify-full", "phase": "waiting", "since": 1789999000.0},
   "health": "live",
   "last_tool_call": 1789999900.0,
   "last_message": {"slug": "sim/jump", "ts": 1789999800.0, "text": "the ring is at 900"},
   "slots": null,                                     # an orchestrator's: {"open": 3, "cap": 8}
   "session_url": "https://claude.ai/code/session_01…",  # the web address its session gave, else null
   "in_process": false,                               # a subagent running inside its parent's session
   "tokens": {"input": 12, "cache_write_5m": 3400, "cache_write_1h": 560,
              "cache_read": 78900, "output": 1200, "calls": 7},  # what its calls took, else null
   "cost": {"dollars": 0.061308, "input": 4.8e-05, "cache_write_5m": 0.017,
            "cache_write_1h": 0.00448, "cache_read": 0.01578, "output": 0.024,
            "unpriced": [], "prices": "2026-11-16"}}]}  # their estimated cost, else null
```

`state` is `working`, `running_test` (a run of its own holds a test slot, `phase: holding`, or is queued for one,
`phase: waiting`, `state_detail` naming the recipe and since when; or a quiet subagent's own worktree holds a live
process, `{"recipe": "process", "phase": "running", "since": null}`, or the machine's processes cannot be listed to
tell, `"phase": "unknown"`), `waiting_on_input` (hc sees an approval or a
question), `idle`, or `gone` (no terminal and no sign of life). An agent running or queued for its own verify reads
`running_test` and `health: live`, never `lost`; so does a subagent whose worktree holds a live process, and one whose
processes cannot be listed reads `health: unknown`. `last_tool_call` is the time of the last tool call of the agent
(`null` before the first): the monitor's 20 s beat and board commands do not move it.
`tasks` lists every task the agent holds a role on but those it orchestrates, and `following` counts
the open ones it orchestrates (neither done nor cancelled), the count of its Following list on the
agents page, which reads those tasks a page at a time.
`callback` is the native address (`uds:` or `codex:`), or `null` for an inbox-only
session; `chat` says whether this session is the current chat recipient, including
an orchestrator fallback. `slots` is, under a project cap ([Slots](../workflow/TASKS.md#slots)), the
tasks an orchestrator (or any session holding open work's slot) holds against the
cap, and `null` otherwise. `session_url` is the web address the session gave
([Going to a session](../workflow/TASKS.md#going-to-a-session)), else `null`; `in_process` says the agent runs inside
another session, a subagent bound to its parent's, with no pane of its own. `tokens` is what the
handle's model calls took, all told, by kind, as its beats recorded them, else `null` (none recorded;
[Where the tokens go](../workflow/TASKS.md#where-the-tokens-go)), and `cost` what they cost, estimated at API list
prices, else `null` ([What the tokens cost](../workflow/TASKS.md#what-the-tokens-cost)). No credentials appear in this
API.

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
  (`working`, `blocked` is `waiting_on_input`, `idle` and `done` are `idle`). Else, for a session that is neither an
  orchestrator nor the chat and whose listener the idle sweep's last round confirmed, `idle` once it is at rest:
  outside any tool call (its `busy_until` passed) and its last tool call more than 90 s ago (`LIVE_SECONDS`, the
  window its health reads live for), when it connected standing for that call before its first. The confirmation
  keeps such a session seen, so its health reads live after each round and stale between two, and says nothing of
  whether it works. Else the board's health: live is `working`,
  stale `idle`, lost or left `gone`. A matched run, and a process its subagent waits on, also lift `stale` and `lost` to
  `live`; processes that cannot be listed, or one whose ownership cannot be told, make them `unknown`.
- **A probe that fails** is `unavailable` in `probes`, and the answer is built from the board; a failing branch lookup is an
  empty branch.

Slot holders and waiters come from `test_slots.runs` (a held slot file whose writer is alive and whose lock is held; a
place in the pool's FIFO line, `<prefix>-<12-digit sequence>.place`, in the pool's own prefix: `slot-` and `render-`, that its waiter still holds); a slot file whose writer is dead is free and is not locked to find out.

### What a callback pushes

An orchestrator's callback uses it: it does not call an agent stale or lost while `running_test`, and it receives a PULL
digest (`crates/tasks/src/cli/pull.rs`) once at its first dispatch, even when empty, then only when an actionable list gains an entry absent from the last delivered PULL after forgetting entries absent from any computed digest: red tasks ([Slots](../workflow/TASKS.md#slots)) with no live executor 10 minutes after they were filed, in any status short of review, listed first as `red_unstaffed`, counted in the header only when there is one, and in no other list, `dependency_blocked` included, a cancelled dependency of its named in its note; ready tasks with no live executor
(one on a subtask still to be executed counts), in-progress with none, review with no reviewer, tasks that landed
(the branch is on main, or `landed <slug> on main` is on the task's thread) and are still open (a status change on the task after that line voids it, whether a move, a verdict such as `changes`, or a re-claim: the task is new work, not a landing left open), tasks a reviewer approved that no landing has closed 10 minutes after the approve, done on the board and still to land, listed as `approved_unlanded`, counted in the header only when there is one, each with when it was approved and what its landing's record says it waits on (`approved 11-17 09:12, not landed; its landing waits on tests`), and idle agents, each scoped to the orchestrator's own tasks and those with no orchestrator yet ([Tasks and roles](../workflow/TASKS.md#tasks-and-roles)) (an idle agent another orchestrator launched is left out). An executor
or reviewer that is `running_test` counts as present however quiet the board says it is.
A task in review whose tip a reviewer asked changes of is rework, counted in progress
([The pipeline alarm](../workflow/TASKS.md#the-pipeline-alarm)). While the pipeline alarm stands, the whole board's
follows the header, whoever the digest is for, and its raising is an entry.
Counts and the clock do not trigger delivery. A departure alone pushes nothing;
an entry that stays present is never re-sent, and an ordinary entry that leaves and returns pushes once.
The first PULL a recipient receives is the whole digest: `PULL HH:MM` with every count, then each list's
first six rows and how many more it holds, but `unanswered`, whose every post is listed with its
actionable advice, delivered once per target and action. Each later one says only what changed since the last PULL it
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
The scan reads only the open tasks' threads, and each of its reads waits at most ten seconds and is sent once, so a
board command never waits minutes on it. A pass whose unanswered-post scan fails says why on stderr and delivers the rest, and its PULL says nothing of
those posts, neither their count nor any of them resolved; their count is then unknown to the recipient, so the
next PULL whose scan answers says it (`unanswered=1 (was unknown)`) and resolves the posts answered meanwhile.
Unanswered work retains its task/post identity across departures and returns; executor handoffs
also distinguish the current target and whether it needs follow-up or recovery.
`unanswered` lists an executor on an in-progress, blocked or review task, or a reviewer
on a task in review, when it is lost or left (or its recorded terminal is closed) and an
addressed post has waited thirty minutes without a later message, status move or
verdict from that handle. A deliberate role release also answers earlier requests;
an automatic departure does not. Posts belong to their recipients at post time,
so a replacement never inherits its predecessor's role-addressed work. Each line
names the post and the recorded conversation's `just tasks restore SLUG HANDLE`
command, or the explicit launch command when no conversation was recorded. When a different
executor now holds the task, the original request stays visible: its advice names both handles
and follows up with the current executor using `just tasks say`. A live, stale or liveness-unknown
replacement, or one running a test, prevents duplicate launch advice. If that replacement is lost or leaves, the advice
offers an executor launch again, without reviving the predecessor's conversation. Each target
and action is delivered once, so replacement loss can push recovery after an earlier follow-up;
an unchanged request and action are never repeated. Its count appears only when
nonzero. `reap` uses the same successor eligibility before automatic recovery, checking current
task, session and native-presence inputs again in its reservation transaction, with fresh
running-test evidence. A changed observation or an active successor consumes no recovery attempt;
once that successor no longer qualifies, the unanswered conversation can recover. `reap` makes
the sole automatic recovery attempt ([TASKS](../workflow/TASKS.md#sessions)).

What is pushed is what the recipient cannot predict and must act on, said once: posts and verdicts, the
owner's moves (a `MOVE` line, [Talking on a task](../workflow/TASKS.md#talking-on-a-task)), NEW, STATUS, ROLE, GIVEN (one line for every task the board gave the recipient since its last read, [Tasks and roles](../workflow/TASKS.md#tasks-and-roles)), BLOCKED, WATCH, the PULL above, and STALE, LOST or LIVENESS UNKNOWN once for each transition of a quiet
child, never once a pass (a child it already hears as stale is not also listed idle, in a pushed PULL, a wait
or `tasks pull` alike). No such line is said of a child whose listener answers the idle sweep, so one at rest is
listed idle whatever its health, and its entry stays through each round: said once. A session with no terminal
is listed by its last tool call ([the agents API](#the-agents-api)), and the recipient's callback state keeps the
beat each one was told idle at (`pull_idle`, from the PULL it acknowledged, which older dispatchers leave as it
is): a dispatcher that does not list such a session forgets its entry, and this one still reads it as told while
the session rests on that beat, so two builds on one board say it once. A new tool call is a new beat, and the
rest after it is said again. WATCH says the pane watch could not ask hc, while it looks for no BLOCKED: once hc has
failed at every sample for 30 s, never for one slow answer that the next sample answers, and again only after
hc has answered. A gap of more than 30 s between two samples is time nobody sampled, the machine asleep or no
server, and the stretch starts again at the sample after it
(`OUTAGE_SECONDS`, `crates/tasks/src/cli/attach/panes.rs`). A recipient is never told its own writes: not its own moves, not a role move it made (a hand-over, a take-over), and not what its own
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
a prompt is said as `BLOCKED` by the second look that sees it, and hc failing at every look for 30 s as one
`WATCH`; and while HANDLE holds the chat seat (the
callback section of [TASKS](../workflow/TASKS.md#sessions)) the owner's chat events come with the rest.
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
for that hand-over and never left out, and nothing repeats on its own. Posts, verdicts and the owner's `MOVE`
lines are kept: they were said to whoever held the role then.
Codex callbacks use the queue by default; `git config symphony.codexSteer true` opts in to
active-turn steering. The owner decides its first live use.
