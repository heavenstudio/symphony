---
name: create-task
description: File a task on the board so an executor can work from the brief alone, without duplicating existing work or missing a required field. Trigger on "file a task", "create a task", "add this to the board", or /create-task.
---
<!-- symphony-stamp: 91c7f87623eb44c0 files=SKILL.md -->

# File a task on the board

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

- `just tasks role SLUG orchestrator` if you intend to follow this task's
  events.
- Tell the owner the slug in one line.
- Starting an executor on it is a separate step — see the task-orchestrator
  skill.

## Worked example

```
TASKS_SESSION=<handle> just tasks create app/scheduled-start "Start on schedule" \
  --priority 2 --stage ready \
  --link "docs/SPEC.md#Scheduling" \
  --depends app/start-input \
  --body-file artifacts/scheduled-start-brief.md
```

(`--as <handle>` works in place of the `TASKS_SESSION` prefix.)
