---
name: create-task
description: File a task on the board so an executor can work from the brief alone, without duplicating existing work or missing a required field. Trigger on "file a task", "create a task", "add this to the board", or /create-task.
---

# File a task on the board

Read the destination project's task conventions and permissions first. Use the
board CLI help for supported arguments; this page describes how to prepare a brief. Load `/tasks` first if you
are not yet attached to the board; a subagent has no monitor, so it gets its
handle from `just tasks register` instead.

## 1. Look before filing

Run `just tasks list --all`, which keeps the done and cancelled tasks plain
`list` leaves out (add `--status design` etc. to narrow it), and
`just tasks search "WORDS"` with the work's own words, and scan both for the same
work, including a concepts-session slug already in flight. If an open task
already covers it, add to its thread with `just tasks say` instead of filing a
second one; if a finished one did, say so in the new brief.

## 2. Pick the slug

`area/name` — `app`, `tests`, `tools`, `data`, `media`, `docs`,
`skills`, `perf`, … It becomes the branch name too, so keep it short and
distinct from anything in the list you just ran.

## 3. Priority and stage

- `--priority` is 0 (most urgent) to 4; `p0` to `p4` read the same.
- `--stage` is one of `backlog`, `concept`, `design`, `ready` (omit it for the
  default, `ready`):
  - `ready` — work with no design step.
  - `design` or `concept` — an owner-signed design doc is still needed. Agents
    never sign that off themselves.
  - `backlog` — a parked follow-up.

## 4. Links between tasks and docs

- `--depends SLUG` (repeatable) names tasks that must already exist — create
  tasks in dependency order, or create the depended-on one first.
- `--link docs/X.md#Heading` (repeatable) points at the spec section it
  implements.
- `--parent SLUG` attaches it under a larger task; like `--depends`, the
  store checks that it already exists.

## 5. Write the brief

Pass it with `--body-file`, written outside the tree (scratchpad or
`artifacts/` — never committed). Give the executor everything it needs
without the thread:

```
what and why — the measurement or evidence that motivated it
spec: docs/X.md#Heading — approved cards it builds ([x], [r], [a] only;
  [ ], [?], [-], [~] are not approved)
pending: what must not be built yet
touches: files/areas it changes, and tasks it must not collide with
acceptance: observable behavior and error cases
evidence: the tests that cover the change and their green run, posted on the
  thread after implementing under the project's test policy, and what the kind
  of change calls for (before and after screenshots of a changed page)
gates: the focused checks the change needs (the tests closest to it and the
  project's lint, never the full suite), review evidence and landing
  prerequisites, using its configured entrypoints; do not invent recipes
rulings: owner answers that apply here, quoted as already approved principles,
  never proposed again on a card; unseen designs applying them still need review
communication: post milestones only (tip ready for review, move to review or
  blocked, collision, finding, verdict, question). Address action with
  --to @orchestrator, --to @reviewer or --to HANDLE. Never acknowledge a
  message or repeat a post on both a subtask and its parent.
```

## 6. File it

`just tasks create SLUG "title" --priority N --stage STAGE --link … --depends
… --parent … --body-file PATH`. This is the step that actually puts it on the
board — the earlier sections only get the arguments right.

A documentation brief must state the destination project's navigation, link and
validation requirements. Review-card citations and document status conventions
also follow that project; this skill imposes no separate document execution lane.

## 8. Afterwards

- Every open task has an orchestrator, and the create's line names it
  (`; HANDLE orchestrates it`). Filed by an orchestrator session, the task is
  yours from the create, unless you passed `--no-role --for HANDLE` (a live
  orchestrator, who takes it) or a parent another holds already makes it
  theirs. Filed by any other session, the board gives it to the orchestrator
  of the task you work on, else of your parent session, else to one that
  stays (live past its first live line, and waiting, watching or holding a
  task); with none that stays it says `it waits for an orchestrator that
  stays`, and the first to come to stay is given it.
- Tell the owner the slug in one line.
- Starting an executor on it is a separate step — see the task-orchestrator
  skill.

## Worked example

```
just tasks create app/scheduled-start "Start on schedule" \
  --priority 2 --stage ready \
  --link "docs/SPEC.md#Scheduling" \
  --depends app/start-input \
  --body-file artifacts/scheduled-start-brief.md --as <handle>
```

(The handle goes in `--as`: no allow rule passes a `TASKS_SESSION=` prefix.)
