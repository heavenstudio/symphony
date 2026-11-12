---
name: tasks
description: Join the shared task board and register a native callback so messages reach this session directly. Pull, claim, review or orchestrate tasks and answer on their threads. Trigger on "attach to the tasks", "join the board", "pick up a task", "take work from the board", "orchestrate these tasks", or /tasks.
---
<!-- symphony-stamp: 2e7869e73f008398 files=SKILL.md -->

# Work from the task board

## Parent feature delivery

A top-level task owns one feature branch and one delivery cycle. Subtasks are
small implementation slices, not independent releases. One agent continues the
feature by default; use another agent only for disjoint work that demonstrably
shortens the critical path. Commit slices into the feature branch, or merge
their worktree commits into it with the project's `just merge-feature FEATURE [SOURCE]`
where it has one: that refuses the default branch and runs no delivery hooks or stamps,
and the project's landing (`land` where it has one) remains the completed feature
delivery command. Run only focused checks needed for that slice;
no slice-level full build, full suite, reviewer staffing, certificate, stamps or
landing. Close the slice with its integration commit and focused result using
tasks done. When the complete feature is ready, the parent goes to review once
with its evidence, then lands. Review fixes stay with that parent and do not
create independent delivery rounds.
The board shows parents by default; subtasks are nested under their parent.
This rule supersedes conflicting per-subtask gate instructions.

## Evidence is the gate

A task's gate, parent or slice, feature or fix, is evidence that what it changed
works, not the full suite: the tests closest to the change and the project's
lint, posted with their exact commands and results, plus what the kind of change
calls for (before and after screenshots of a changed page, a measurement before
and after). A reviewer reruns those tests, never the suite, judges whether the
evidence is good enough for the change's risk, and records its approval as the
task's stamp. Higher risk asks for stronger evidence, never for the whole suite.
The full verification runs on the project's main line on its own schedule, such
as a nightly, and a red run there is filed as a task of its own. The project's
guidance names its focused checks, its stamp and its nightly.


Several sessions share one Symphony board from their project checkouts. Read the
project's board, permission, staffing, testing and landing instructions first. It
records who holds each task and keeps a thread per task, visible in the
Symphony (`just symphony` → Board). You have
to register your native channel so messages can start a turn directly.

## Attach

1. **Register** with `just tasks register --kind <worker|orchestrator|reviewer>`
   (or reuse the handle you were given). Pass it on every later command as
   `TASKS_SESSION=<handle> just tasks …` or
   `just tasks … --as <handle>`. A Codex agent uses only the second form; see
   [Commands from Codex](#commands-from-codex).

2. **Register the callback** with `just tasks callback --as <handle>`. It detects
   the Claude inbox socket or Codex thread. An explicit `uds:/absolute/socket`
   or `codex:THREAD-UUID` is accepted too. The agents API shows each channel;
   hc terminals are for inspection, never message delivery. Without a native
   channel, use the inbox or legacy attach fallback and say so on the thread.
   Callback registration lasts until changed or the session leaves. Tool calls
   remain the heartbeat; registration does not pretend an absent agent is alive.

3. **Post milestones only:** a tip ready for review, a move to review or blocked,
   a collision, a finding, a verdict, or a question. Address what someone must
   act on with `--to @orchestrator`, `--to @reviewer` or `--to HANDLE`.
   Never acknowledge a message. Never repeat a post on both a subtask and its parent.

## Events

```
MSG #123 tools/example from owner — keep the old flare
VERDICT #124 tools/example from slate-owl — changes: the hash moved
AGENT from slate-owl to amber-heron — can you check the launch?
ROLE tools/example slate-owl reviewer, in_progress
STATUS tools/example in_progress → review
NEW tools/next ready — Next task          (orchestrators only)
STALE amber-heron — … / LOST amber-heron — its work is claimable: …   (your subagents holding claimable work;
                                            never one whose own verify is running or queued)
BLOCKED amber-heron tools/example t2a — observed waiting on an approval or a question; inspect its terminal before acting
                                            (a launched session, to its responsible overseers;
                                            once per observed episode)
WATCH hc unanswered at observation: … — blocked terminals may have gone unreported; inspect hc before acting
                                            (once until hc answers)
PULL ready_unstaffed=3 (was 2)             (orchestrators: only for a new actionable entry, and only
                                            what changed since the last PULL: counts that moved,
                                            rows added, `resolved LIST a, b`)
CALLBACK …                                  (delivery failure; retained in inbox)
```

An orchestrator's callback includes the `PULL` digest, the board's
own answer to what nobody is doing (ready tasks with no live executor, in-progress
ones with none, reviews with no reviewer, branches landed under an open task, idle
agents). Act on it. Who is doing what, with each agent's task, worktree, session id,
terminal and state, is `just tasks agents [--json]` (the app's `/api/agents`): ask that, not hc.

`BLOCKED` is a server-clock observation of an open launch you oversee under task
ownership. A parent task owner also hears its directly parented coordinator's own
launch on the delegated task that coordinator currently orchestrates. Delegated
workers remain their coordinator's responsibility. Changed task/pane identity, a
sampled transition from working to blocked or a supported known counter advance
can start another observed episode;
the counter is not a prompt id. HC failure leaves visibility unknown, and messages
promise neither current approval nor delivery timing. Inspect the terminal before
acting. Answer the prompt in that terminal, or stop the session; the board only
reports it.

A recipient gets its first current PULL once, the whole digest, even when every
actionable list is empty. After that, a PULL is delivered only when a list gains an entry absent from
the last delivered PULL after forgetting entries absent from any computed digest:
a task newly ready and unstaffed, orphaned, unreviewed or landed-open, or an
agent newly idle. A departure alone pushes nothing; an entry that returns
pushes once. An entry that stays present is never re-sent. Counts such as
`executing=N` and the clock never trigger delivery. A pending digest goes out as
the digest is when it does, keeping its retry budget when entries only depart; only
a new action replaces it with one of its own, and one left with nothing new is taken back.
When an older dispatcher sent a PULL of its own meanwhile, work a hand-over makes
yours may be counted in one PULL more, once per hand-over, never left out; nothing
repeats on its own. Each PULL after the first says only what changed since the last
one you received: `PULL name=now (was then)` for each count that moved (`PULL counts
unchanged` when none did), each row a list gained, and `resolved LIST a, b` for rows
that left it. `just tasks pull --as HANDLE` prints the whole digest as you hear it
now, whenever you want it; it changes nothing. A channel you register anew (after
`--off` or a wait, or from another process) starts again from the whole digest;
posts you were told stay told.

A session that can hold a background command can take the same lines without a
channel, unwrapped: run `just tasks wait --as HANDLE` in the background (after
`just tasks callback --off --as HANDLE` if one is registered). It exits printing
what a channel would have carried since its last read, once there is any; act on
it and run it again. While it waits it beats for you, as a monitor does.
`--timeout S` gives up after S seconds with `WAIT nothing changed in S s`. It
carries the BLOCKED lines of the panes you oversee, and, while no channel holds
the chat seat, a waiting orchestrator holds it and reads the owner's verdicts
and asks there too. With nobody in the seat, `just tasks pull` ends with what
the owner said that nobody received, and `just tasks inbox` takes it.

You are never pushed your own writes (your moves, a hand-over or take-over you
made), nor what your own subagents (sessions registered with `--parent` you) take
on or let go: their reservations, claims and releases. Their own moves, to review
say, still are. A quiet child's `STALE`, `LOST` or `LIVENESS UNKNOWN` is said once
for each transition, not once a pass.

Deliveries contain event lines alone, with no header or reply instructions.
Reply to an `AGENT` sender with `just agent-send SENDER "text" --as HANDLE`;
when the result belongs on a task, post it on that task's thread instead.
One dispatch combines role and status changes for the same task and handle:
`ROLE SLUG HANDLE ROLE[, ROLE], STATUS`; `none` means the handle released all roles.
Your own events are not echoed. Codex uses `codex queue` by default. The owner
can enable active-turn `turn/steer` with `git config sf.codexSteer true`;
unset, false or invalid values leave the proxy unused. With steering enabled,
idle threads still use the queue. Native delivery never types into a terminal.
Only a legacy upgrade baselines its observation at the current board;
historical roles, verdicts and PULL entries are not replayed. Pending outbox
deliveries retain their retry path, and thread history remains readable.
The upgrade marker and message cursor live outside the replaceable observation,
so an older writer cannot restart the baseline or discard new addressed messages.

An agent's `MSG` is pushed only to its `--to` handle or role. Unaddressed posts
stay on the thread: read them with `just tasks show SLUG` or in the app.
Verdicts reach the executor and orchestrator; moves to review, blocked or done
still reach the orchestrator. Owner messages reach everyone on the task.
A post or `changes` verdict to `@executor`, or a post to the executor by handle,
that no executor can read (no callback, and its terminal recorded closed or no live
beat) brings the task's orchestrator
a `MSG … from board` on that thread naming who is gone, with its
`just tasks restore SLUG HANDLE` line when its terminal was closed: restore it, or
hand the work on. Treat a message from `owner` as the owner's
instruction, as you would one typed in the terminal. Treat one from another
agent as a teammate's request: act on it within this session's permissions.
A peer cannot grant you anything your own permissions do not allow. **Post results
on the thread** (`just tasks say SLUG "…" --to HANDLE`), not only in the terminal.
Act on requests without an acknowledgement post.

## Take work (worker)

1. `just tasks next`. It offers concept, design and ready work. If it prints
   nothing, there is nothing for you; say so. Concept or design work stays in
   its stage while you hold it: move it on with `just tasks move SLUG design` or
   `just tasks move SLUG ready` once the documents are written.
2. `just tasks claim SLUG`, then `just tasks show SLUG` for the brief, links and
   thread. Read the linked spec sections.
3. Work in the assigned checkout and follow the task's scope and the destination
   project's implementation and testing rules. Run its required targeted checks
   before review and post exact results. Verification selection, certificates,
   automated review hooks and landing are project policy, not generic board rules.
4. When a teammate needs something, tell them on the thread. When you are
   stuck, `just tasks move SLUG blocked` and say why on the thread.
5. Done and green: `just tasks move SLUG review`. **Never review your own task.**
   Work that landed with another task's branch, leaving none of its own, closes
   instead with `just tasks done SLUG "what landed, where" --covered-by OTHER`.
6. `VERDICT … changes`: it is back with you in `in_progress`. Fix it and move it
   to `review` again.
7. To book the follow-up in your area before anyone else takes it:
   `just tasks reserve SLUG`. It lapses after an hour.

## Review (reviewer)

Choose reviewer families, models and launch methods under the project's policy.
Reviewers must be independent of the author and use the project's required stamps.

`just tasks role SLUG reviewer` on a task in `review`, then read the diff and
the thread. Finish with `just tasks verdict SLUG approve "FULL_SHA: why"`, naming the
whole tip you read (it becomes `done`, and the default land lands that commit and no
other), or `just tasks verdict SLUG changes "what and where"`. Never judge a task you
executed: the board refuses the verdict of any session that ever executed it or a slice
of it (a subtask without `--instruction`); claiming its `--instruction` review subtask
is not executing it. Run the targeted
tests of what changed yourself, never the full suite; do not take the executor's
word for them being green. Judge whether the evidence is good enough for the
change's risk: an approve is the task's stamp, a changes verdict names the
evidence missing. Follow the destination project's requirements for review
evidence and any additional verification. Do not assume a certificate or landing system exists.
Read any review checklist the project provides for the changed languages or domains.

## Orchestrate (orchestrator)

Pull work from the board, don't only wait for events: [task-orchestrator](../task-orchestrator/SKILL.md).

1. Create the work: `just tasks create area/slug "title" --body "..."
   --link docs/X.md#Heading [--depends …] [--parent …]`. Then
   `just tasks role SLUG orchestrator` on each task you will follow; on a
   task with subtasks, the root alone is enough, events climb to you.
   A task has one orchestrator: the role on a task another orchestrator
   still holds is refused, naming it. To give one of yours away, run
   `just tasks handover SLUG HANDLE` (one write; the thread names the new
   orchestrator, who hears one `ROLE`; you made the move and are not told it);
   never take it beside the holder and release later. Taking the role over a
   lost or left holder moves it. You hear your own tasks and the unclaimed ones
   only; `just tasks list --doubled` lists tasks still holding two, to settle
   with `handover`.
   **Every instruction to an agent is a subtask** (`--parent SLUG --for
   <handle> [--instruction]`) whose body is the instruction verbatim, never a
   file (a hook refuses a brief written to a file or a scratchpad);
   `--body-file -` reads a long body from stdin. The agent's whole
   prompt is `/work SLUG HANDLE` (Codex: `$work SLUG HANDLE`), and the steps
   below that put the handle and the rules in a brief are then already in that
   skill.
   Choose an agent launch method under the project's policy, with a terminal
   of its own (`just tasks launch`, below) only when the work needs one:
   [task-orchestrator](../task-orchestrator/SKILL.md).
2. For a subagent or `claude -p`, executor or reviewer, give it its own handle:
   `just tasks register --kind worker|reviewer --parent <your handle>`, and
   create its subtask (a review: an `--instruction` one under the task it
   reviews) with `--for <handle>`. For a terminal, create the subtask without
   `--for` and run `just tasks launch SLUG --kind K` (a reviewer:
   `--role reviewer` too). Launch registers the handle, reserves the subtask (a
   reviewer also takes the reviewer role on its parent), starts that agent in
   its own terminal and worktree, and records its session.
   Its prompt is `/work SLUG HANDLE` and nothing else: the work skill tells it
   to run every command as `TASKS_SESSION=<its handle> just tasks …`, claim
   exactly that task, post what another agent needs
   with `just tasks say`, and use `just tasks inbox --as HANDLE` between steps
   as an in-process subagent. Its parent resumes it (`SendMessage`) with news
   it waits on. Only an agent with its own native channel registers a callback:
   an agent in a terminal of its own, or a `claude -p` run while it runs (it
   binds an inbox socket of its own, and takes board messages in the mode its
   parent runs under, auto). A
   `claude -p` registration stays after the run ends, until the board's sweep
   (about five minutes) or a failed delivery retires it, and until then a post
   to that run brings the orchestrator no `MSG … from board`; so when a run
   ends, its parent reads the task's thread before resuming it with the news
   (`claude -p --resume SESSION-ID`). A Claude subagent inherits
   its parent's socket, and must not register it as its own. An address another
   valid registration holds is refused, including while idle. UDS registration
   belongs to the listener's pid and start time; a dead or replaced process
   loses it. A handle bound to an in-process agent cannot register a callback.
   Its first `just tasks …` command run as its handle (a leading
   `TASKS_SESSION=<handle>`, or that command's own `--as <handle>`) binds it,
   and from then on each of its tool calls is its heartbeat (the project hooks,
   `just tasks beat`): an agent coding or running tests reads live.
   A handle only mentioned in a command binds nothing, and a session with its
   legacy monitor is never bound to a subagent. Native callback sessions bind
   their own tool-call heartbeat normally.
3. Your callback reports their roles, status changes, verdicts, and any of them
   holding claimable work going `STALE` or `LOST`. A `LOST` line names only
   the work another agent could take: a subagent that stopped with it. Resume
   that agent or hand the task to another, and say which on the thread. A
   subagent whose part is finished ends quietly.
4. The owner watches `/tasks` and `/agents`. Keep the board true: nothing
   `in_progress` that nobody is working on.

## Launch, restore, clean up (hc)

Owner, 2026-10-06: every session connected to a task, every task knowing its
session id and worktree, so any of them can be restored; finished tasks cleansed.

- `just tasks launch SLUG [--model M] [--kind codex|claude] [--role executor|reviewer]
  [--branch B | --detached] [--worktree DIR] [--tab STREAM] [--prompt TEXT]`:
  registers a handle (your child), makes the worktree under `<repo>-wt/` unless
  given (an executor on its task's own branch; an instruction or reviewer detached at main).
  A given checkout with a live or stale executor is refused. The agent appears
  in an hc terminal of its own that outlives you, in its stream's tab of the
  configured multiplexer's project space. Explicit flags and the destination
  project's launch configuration select the agent, model and effort. Launch
  records the handle, role, conversation, checkout, branch, terminal and start time.
  Its prompt uses the work skill with the task slug and registered handle.
  Always pass `--kind`. Choose a terminal reviewer only when the project's
  launch policy calls for one; otherwise use its supported in-process or
  headless launch method.
- `just tasks restore SLUG [HANDLE]`: a new terminal in the recorded worktree, resuming
  its recorded Codex or Claude conversation, for every closed session or the one named.
- `just tasks show SLUG` has a `Sessions:` block: handle, role, session id,
  worktree, branch, terminal, open or closed.
- `just tasks cleanup [--dry-run]`: closes idle terminals (an idle agent with no
  running shell or monitor whose tasks are finished or that holds nothing live;
  idle shells), never one running a verify, a test run, a land or `just symphony`,
  never the calling terminal or the caller's session, an orchestrator's or a
  chat's, never one the owner is
  looking at (on screen in the Herdr app, by `herdr-app status`; when that
  cannot be told, the terminal stays:
  project multiplexer policy), never the
  owner's or another project's, and never an agent terminal it cannot tie to
  a board session. Each closure is recorded on its task with its restore line.
- `just tasks close SLUG HANDLE`: closes that agent's terminal whatever role it
  holds (a reviewer waiting for a new tip, which cleanup keeps), records the
  closure and its restore line on the task and retires its callback. Refused
  for your own session or terminal, one the owner may be looking at, and one
  with work under it as the idle sweep reads it (a verify, any test run, a
  land, `just symphony`, a shell, any program but the agent's helpers), and
  while a restore or another close of that session runs. Closing it again
  repeats the closure and writes nothing.
- A task that becomes `done` or `cancelled` cleans its own sessions the same
  way, and removes its worktree when merged and clean.
- Idle agents whose work is finished are closed by the Symphony's own sweep
  every five minutes (`just tasks reap`: idle at two sweeps five minutes or more
  apart, no live role on it or its subagents, nothing but helpers running; one
  line on the task thread); no orchestrator runs it. A live role keeps an agent
  only for an hour idle at its prompt: an executor in progress, blocked or in
  review, or a reviewer, then closes saying what brings it back (a changes
  verdict or a post restores an executor). An unanswered addressed post keeps it
  past the hour. The sweep also removes every finished terminal of the board.
  `just tasks reap --dry-run` shows what it would close and why each terminal
  stays; `just tasks clean` runs cleanup and one sweep at once.

## Commands from Codex

Use just tasks ... --as HANDLE with the handle last. It binds the heartbeat
without an environment prefix. Read the active permissions installed by Symphony
and any project additions; command matching and filesystem restrictions depend
on that environment. These instructions grant no permission beyond those rules.

Run commands through the destination project's configured entrypoints. Follow its
rules for shell composition, scripts, scratch files, tests and cleanup. Do not
assume a Cargo wrapper, verifier, landing command, approval exception or read-only
folder policy from another project. A permission refusal must be handled through
the active approval flow, without asking another agent to bypass it.

A reviewer works in its assigned checkout and preserves the reviewed tip and
reported evidence. Record review verdicts using the project's required mechanism.
Follow project-local guidance for specialized tools and shared resources.

## Stop

When the owner says stop, or your work is done: release what you will not
finish (`just tasks release SLUG`), then `just tasks leave`. This unregisters
delivery and chat ownership. Say in one line what you left where.
