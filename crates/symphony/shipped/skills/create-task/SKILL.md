---
name: create-task
description: File a task on the board so an executor can work from the brief alone, without duplicating existing work or missing a required field. Trigger on "file a task", "create a task", "add this to the board", or /create-task.
---

# File a task on the board

Mechanics live in [docs/workflow/TASKS.md](../../../docs/workflow/TASKS.md); this
page is what to get right before and while filing. Load `/tasks` first if you
are not yet attached to the board; a subagent has no monitor, so it gets its
handle from `just tasks register` instead.

## 1. Look before filing

Run `just tasks list` (add `--status design` etc. to narrow it) and scan for
the same work, including a concepts-session slug already in flight. If a task
already covers it, add to its thread with `just tasks say` instead of filing a
second one.

## 2. Pick the slug

`area/name` — `sim`, `view`, `tests`, `tools`, `content`, `art`, `docs`,
`skills`, `perf`, … It becomes the branch name too, so keep it short and
distinct from anything in the list you just ran.

## 3. Priority and stage

- `--priority` is an integer 0 (most urgent) to 4. `--priority p2` is refused
  — the flag takes a number, not a p-label.
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
acceptance: a consequence over a run, never a constant
  (AGENTS.md "Gameplay is not correctness")
evidence: the tests that cover the change and their green run, posted on the
  thread after implementing (RULES.md `workflow/tests-after`)
gates: just check, the targeted tests of what changed (its suites, the
  crate's own targets, lint-rs for Rust, docs-index for docs) and just
  precheck; no branch just verify: the full fast plan runs on the tip at the
  move to review, and on main after the landing
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

A brief that adds, moves or deletes a doc must say to link it from the folder's `README.md`
(and docs/README.md's Folders list for a folder); `just docs-index`, part of both verify
scopes, fails on a page its README does not link, or a list README without a row for it.

## 7. Never

Never name a review card in a design doc's prose — both verify scopes refuse
it.

## 8. Afterwards

- `just tasks role SLUG orchestrator` if you intend to follow this task's
  events.
- Tell the owner the slug in one line.
- Starting an executor on it is a separate step — see the task-orchestrator
  skill.

## Worked example

```
TASKS_SESSION=<handle> just tasks create sim/jump-departure "Depart the jump point on schedule" \
  --priority 2 --stage ready \
  --link "docs/game/SPEC.md#4. Weapons & damage" \
  --depends sim/jump-arrival \
  --body-file artifacts/jump-departure-brief.md
```

(`--as <handle>` works in place of the `TASKS_SESSION` prefix.)
