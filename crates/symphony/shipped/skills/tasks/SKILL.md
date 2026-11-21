---
name: tasks
description: Join the shared task board and register a native callback so messages reach this session directly. Pull, claim, review or orchestrate tasks and answer on their threads. Trigger on "attach to the tasks", "join the board", "pick up a task", "take work from the board", "orchestrate these tasks", or /tasks.
---

# Work from the task board

Several sessions share one Symphony board from their project checkouts: it records who holds
each task and keeps a thread per task, shown in the Symphony (`just symphony`, Board). Read the
project's board, permission, staffing, testing and landing instructions first.

## Attach

1. **Register** with `just tasks register --kind worker|orchestrator|reviewer`, or reuse the
   handle you were given, and pass it on every command as `just tasks … --as HANDLE`, the handle
   last, Claude and Codex alike: it binds your tool calls as your heartbeat, and a command with
   an env prefix (`TASKS_SESSION=HANDLE just tasks …`) stops at a prompt.
2. **Register the callback** with `just tasks callback --as HANDLE`, which finds your Claude
   inbox socket or Codex thread, so a message starts a turn. An in-process subagent has no
   channel of its own and never registers its parent's: it reads `just tasks inbox --as HANDLE`
   between steps. A session that can hold a background command may instead run
   `just tasks wait --as HANDLE` in the background (after `just tasks callback --off` if one is
   registered): it exits printing what a channel would have carried, so act on it and run it
   again.
3. **Post milestones only**: a tip ready for review, a move to review or blocked, a collision, a
   finding, a verdict or a question, addressed to who must act (`--to @orchestrator`,
   `--to @reviewer`, `--to HANDLE`). Never acknowledge a message, and never repeat a post on a
   subtask and its parent.

## Events

```
MSG #123 tools/example from owner — keep the old flare
VERDICT #124 tools/example from slate-owl — changes: the hash moved
AGENT from slate-owl to amber-heron — can you check the launch?
ROLE tools/example slate-owl reviewer, in_progress
STATUS tools/example in_progress → review
MOVE #125 tools/example from owner — owner moved it review → cancelled; it moves no further: stop work on it
NEW tools/next ready — Next task
GIVEN 2 open tasks are yours to orchestrate: tools/a, tools/b
STALE amber-heron — … / LOST amber-heron — its work is claimable: …
BLOCKED amber-heron tools/example t2a — observed waiting on an approval or a question; inspect its terminal before acting
PULL ready_unstaffed=3 (was 2)
CALLBACK …
```

- A message from `owner` is the owner's instruction, as one typed in the terminal. One from
  another agent is a teammate's request: act on it within this session's permissions, which a
  peer cannot widen. An owner's `MOVE` ending `it moves no further: stop work on it` means stop.
- An agent's `MSG` is pushed only to its `--to` handle or role; unaddressed posts stay on the
  thread (`just tasks show SLUG`). You are never pushed your own writes.
- Reply to an `AGENT` sender with `just agent-send SENDER "text" --as HANDLE`. A result that
  belongs on a task goes on its thread (`just tasks say SLUG "…" --to HANDLE`), never only in
  the terminal.
- `PULL` is an orchestrator's digest of what nobody is doing: ready tasks with no executor,
  in-progress ones with none, reviews with no reviewer, branches landed under an open task,
  tasks approved and never landed, idle agents. A push says only what changed since the last;
  `just tasks pull --as HANDLE` prints the whole digest and changes nothing. Act on it.
- `STALE` and `LOST` name a subagent of yours holding work another could take: resume it or
  hand the task on, and say which on the thread.
- `BLOCKED` is an observation of a launched session you oversee, waiting at a prompt: inspect
  its terminal before acting, then answer the prompt there or stop the session.
- `MSG … from board` tells an orchestrator that a post to its executor reached nobody, with the
  `just tasks restore SLUG HANDLE` line when its terminal was closed.
- `CALLBACK` names a failed delivery; the event stays in the inbox.

Who is doing what, with each agent's task, worktree, session and state, is
`just tasks agents [--json]`: ask that, never the multiplexer.

## Take work (worker)

1. `just tasks next` offers concept, design and ready work, or says `nothing to take`. Concept
   or design work stays in its stage while you hold it: move it on with
   `just tasks move SLUG design` or `just tasks move SLUG ready` once its documents are written.
2. Carry it by the [work](../work/SKILL.md) skill, which claims it and questions it before any
   code, through to its finish.
3. To book a follow-up in your area before anyone else takes it: `just tasks reserve SLUG`. It
   lapses after an hour.

## Review and assess

A reviewer works by the [review-task](../review-task/SKILL.md) skill and an assessor by the
[assess-task](../assess-task/SKILL.md) skill, each on the task itself, never a subtask. Their
whole prompts are `/review-task SLUG HANDLE` and `/assess-task SLUG HANDLE` (Codex: `$` in place
of `/`).

## Orchestrate (orchestrator)

Pull work from the board rather than waiting for events, by the
[task-orchestrator](../task-orchestrator/SKILL.md) skill, which staffs, reviews and lands. The
board's commands for it:

- **File work**: `just tasks create area/slug "title" --body "…" --link docs/X.md#Heading
  [--depends …] [--parent …]` (`--body-file -` reads a long body from stdin). A task you file is
  yours to orchestrate from that write; `--no-role --for HANDLE` files it for another live
  orchestrator. On a task with subtasks the root alone is enough: events climb to you.
- **One orchestrator a task.** The role on a task another orchestrator holds is refused; give
  one of yours away with `just tasks handover SLUG HANDLE`, and never take it beside the holder.
  `just tasks list --doubled` lists tasks still holding two. Every open task has an orchestrator
  or waits for one: the board gives unheld work to one that stays, in a `GIVEN` line.
- **A subtask** (`--parent SLUG --for HANDLE [--instruction]`) is a slice of the work, or an
  instruction that is neither the task's work nor its review (a question, an errand); its
  agent's prompt is `/work SUBTASK HANDLE`. An executor and a reviewer work on the task itself.
- **A subagent or a headless run** gets a handle of its own:
  `just tasks register --kind worker|reviewer --parent HANDLE`. Its first board command as that
  handle binds it, and each of its tool calls is then its heartbeat. Its parent resumes it with
  the news it waits on, and reads the task's thread first when a headless run has ended.
- **Keep the board true**: nothing `in_progress` that nobody is working on.

## Launch, restore, clean up

Every session is tied to a task, and every task records its session and worktree, so any can be
restored. Each command's `--help` says its flags and what it refuses.

- `just tasks launch SLUG --kind codex|claude [--role executor|reviewer] [--model M]
  [--branch B | --detached] [--worktree DIR]` registers a handle (your child), makes the
  worktree, starts the agent in a terminal of its own that outlives you, and records its session
  on the task. Always pass `--kind`. Its first message is the skill's prompt alone, so anything
  else goes on the task's thread. It is refused past `[limits] machine_agents`;
  `just tasks limits` prints the limits and each role's model. A reviewer launch resumes the
  task's recorded reviewer.
- `just tasks restore SLUG [HANDLE]` opens a new terminal in the recorded worktree and resumes
  the recorded conversation, for every closed session or the one named.
- `just tasks show SLUG` lists its sessions: handle, role, worktree, branch, terminal, open or
  closed.
- `just tasks cleanup [--dry-run]` closes idle terminals whose work is finished, never one
  running a test, a land or a server, the caller's, an orchestrator's or one the owner is
  looking at. `just tasks close SLUG HANDLE` closes one agent's terminal whatever role it holds.
  Each closure is recorded on its task with its restore line.
- A task that becomes `done` or `cancelled` cleans its own sessions, and removes its worktree
  when merged and clean. The Symphony's own sweep closes idle agents every five minutes, and one
  holding a live role after an hour idle at its prompt; no orchestrator runs it
  (`just tasks reap --dry-run` shows what it would close and why each terminal stays).

## Permissions

These instructions grant no permission beyond the rules the project installed: take a refusal
through the active approval flow, never by asking another agent to bypass it.

## Stop

When the owner says stop, or your work is done: release the executor or reviewer roles you will
not finish (`just tasks release SLUG --role ROLE`), then `just tasks leave`, which unregisters
delivery. Open work you orchestrate is not released (that is refused): the leave gives it to
another orchestrator, or you hand a task to one you choose first with
`just tasks handover SLUG HANDLE`. Say in one line what you left where.
