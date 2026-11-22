# A project's task workflow

A project should be able to use its own steps without giving every project a different board
implementation. The remaining choice is how much of the workflow it can replace: names alone,
steps within the existing lifecycle, or the lifecycle itself. This is a design proposal; none
of the workflow settings below is implemented. The choice is on
[its card](#project-workflow-shape).

The recommended scope is **B: project steps over the existing lifecycle**. A project could use
`Specification → Ready → Implement → Validate → Review → Done`; Implement and Validate would both
be work in progress. Projects needing different evidence already have configurable stamps and
their own landing policy. A second gate engine would duplicate that work.

## What exists

| Concern | Current source and behavior |
| --- | --- |
| Task state | `crates/tasks/src/store/work.rs`: nine statuses, the seven-step pipeline, claimable stages and allowed moves are fixed lists. Concept and design can be claimed without changing status; claiming ready starts work. |
| Completion and authority | The store's task writes enforce role ownership, independent verdicts, special subtask completion and landing. Moving a task to done is not an ordinary move. Done may mean approved but not yet landed. |
| Persistence | `crates/tasks/src/schema.rs` restricts status names; `store/postgres/schema.rs` is the other database's schema. Thread replay in `stages.rs` derives stage durations from the known names. |
| Agent operation | PULL, staffing, reaping, slot accounting and worktree sweeping use those lifecycle meanings. They cannot infer whether an arbitrary word means ready, active or finished. |
| Board view | `apps/symphony-web/src/boardwait.tsx` fixes the wide columns and phone tabs. Blocked work appears in its previous column, with its reason. Build and Test are temporary views of running work, not stored statuses. |
| Landing evidence | The project's committed `[stamps]` replaces the generic catalogue. A task's independent assessment selects its needs; the default land reads evidence at its tip. A project can replace the land recipe. |
| Project gates | The plugin's typed gates answer supplies project facts to the task page and board consumers; a same-named project gate replaces the generic one. Displaying a gate does not by itself make the default land enforce it. |

The operating references are [tasks and roles](../TASKS.md#tasks-and-roles),
[needed stamps](../TASKS.md#needed-stamps), [gates](../TASKS.md#gates) and
[project settings](../../operations/PROJECT_FILE.md). The source paths above identify where an implementation
would have to change, rather than another specification of those rules.

## The workflow choice

| Option | What a project can change | Board and settings cost | Client and land cost |
| --- | --- | --- | --- |
| **A. Names and order** | Rename, reorder or hide existing columns; no new steps or permission rules. | A presentation table in `symphony.toml`; no stored-state migration. Existing transitions, PULL and recovery stay fixed. | Columns, phone tabs and labels read that table. Land is unchanged. A hidden column must remain reachable through filters. |
| **B. Steps over the lifecycle** | Add preparation and active-work steps, name every column, and restrict who moves between them. Ready, review, blocked, done and cancelled retain their meanings. | One shared workflow reader, a step on each task, an atomic move policy and migration of both database backends. PULL and recovery use each step's lifecycle meaning. | Board, filters, timeline and skills read the same description. The default land checks that a task reached the review step as well as its existing evidence. Project lands receive the step too. |
| **C. A replaceable lifecycle** | Define any state graph, including several independent review stages or a delivery stage after approval. | A workflow engine: configurable claim, release, verdict, dependency, terminal and recovery effects, with their permission rules. Schema, APIs, staffing and recovery all depend on that engine. | Every page and skill consumes its operations. Each landing implementation must define its graph operation; every graph needs tests of completion and recovery. Highest cost and ongoing compatibility burden. |

**Why B.** It permits actual extra steps while retaining the meanings the board needs to staff,
recover and close work. A only changes presentation. C is justified when a project has a real
lifecycle B cannot express; no such lifecycle is specified by this task. These are relative work
estimates, not measured implementation times.

### The proposed settings under B

The project writes one complete `[workflow]` beside `[[entrypoints]]`. With no table, it gets
the whole current workflow. Writing a table replaces that default, rather than extending an
invisible list. All workflow names here are proposed; today's settings reader refuses them.

Each step has a stable `id`, a display `label` and a `phase` drawn from the existing nine
statuses. IDs identify moves and records; changing a label changes no task. Preparation
(`backlog`, `concept`, `design`) and `in_progress` can have several steps. There is exactly one
each of ready, review, blocked, done and cancelled. This keeps one destination for the existing
claim, verdict, block and landing operations. A step cannot make an active task terminal merely
by changing its label.

For example, these would be entries in the steps and moves of a complete definition, not a
loadable project file:

```toml
[[workflow.steps]]
id = "validate"
label = "Validate"
phase = "in_progress"
abandon_to = "ready"

[[workflow.moves]]
from = "implement"
to = "validate"
operation = "move"
roles = ["executor"]
```

The full definition also supplies column order, its create default, and the existing operation
routes: claim, review, approve, changes, block, resume, cancel and land. The reader validates
references, unique IDs, supported phases and roles, reachable completion, and unambiguous targets
for each operation. Every active step names its ready step for abandoned work. Missing required
operations, an unknown phase or role, and a removed ID still held by a task refuse activation;
they do not send the board back to permissive defaults.

The board reads policy from the main checkout's **committed** file, as it reads stamps. A task
branch cannot grant itself another move. A policy revision is recorded with every move, so a
later rename or reordering cannot rewrite the task's history. The API exposes that revision
with steps, labels and allowed operations. A request based on an older revision is refused with
the current next actions; the write checks policy, current step and actor together.

### Moves and people

An edge names its operation and allowed existing roles: executor, orchestrator, reviewer or
owner. An arbitrary agent with no task role gets no authority. An owner edge uses the owner's
authenticated page action; writing `owner` in a request or holding an agent role cannot satisfy
it. Projects may tighten existing permissions, not remove independent review or disguise a
terminal write as a move.

| Example | Who and effect |
| --- | --- |
| Specification to ready | Its executor or orchestrator, if allowed by that edge. A project can instead require the owner's action here. |
| Ready to implement | Claim assigns the executor and takes the existing work slot. It selects the configured initial work step. |
| Implement to validate | Its executor, without another claim or slot; both steps are active work. |
| Validate to review | Its executor or orchestrator after the configured route. A direct implement-to-review move is refused when no edge allows it. |
| Review to active work | A fresh reviewer's changes verdict selects the configured return step. Ordinary moves keep their existing, separate permissions. |
| Review to done | A valid approve or land operation, with today's independence and evidence rules; never an ordinary move edge. A subtask's completion and a covered close retain their separate rules. |
| Block and resume | Keep the exact previous step plus the named reason; resume to it only while it still exists and its dependencies permit it. |

A workflow route is an ordering constraint, not evidence that tests ran or a design was approved.
An owner action on a preparation step is likewise not a reusable approval of later code. Evidence
and its lifetime remain the stamp or project landing policy's responsibility. B does not add
multiple independent review rounds as stages; that belongs to the project's landing policy, or
would motivate C.

### What each reader changes

- **Schema and API:** add a step ID beside the existing status, which remains the semantic phase.
  Existing records with no step resolve to the matching default ID. All writers update the pair
  and append a structured step transition in the same transaction. Both database backends, copy,
  export and import retain it. Existing API status fields keep their meaning; the versioned
  contract gains the workflow and step fields. Builds unable to preserve them cannot write a
  board that has activated custom steps.
- **PULL and staffing:** return the actual step, next legal actions and responsible role.
  Claimability and slot accounting follow the phase. Ready work alone starts active work;
  preparation work stays at its step when claimed. A required owner move appears as waiting on
  that action, rather than ready work offered repeatedly to agents.
- **Reap and sweep:** losing an executor releases its role, never completes a step. Preparation
  remains in place; active work is requeued to its explicit `abandon_to`, recording its previous
  step for the next executor. Blocked tasks retain their resume step. Worktree removal still
  requires the existing landing/completion and owner-line checks; a custom label grants none.
- **Client:** `/tasks` reads columns, labels, counts and phone order from the workflow. Filters,
  page links and saved selections use IDs and the project identity; a removed selection falls
  back to a valid step. Blocked work stays at its recorded step. Build and Test remain run
  overlays. Task actions show the server's legal operations; stage durations retain stable IDs
  and the label recorded at each transition. Reports can group by phase across projects without
  merging their different steps.
- **Skills and CLI:** the work, review and orchestrator skills ask the board for next actions
  instead of assuming that every task next moves to a literal review status. Help, completion,
  CLI moves and API writes use the same policy. Current verbs remain aliases for the typed
  operations, so the default project's commands behave exactly as before.
- **Land:** the default land requires the configured review route to be complete and then reads
  the existing stamps at the tip. It preserves approved-but-unlanded done tasks and blocked review
  tasks, and the existing handling of a task that changed state during its merge. Subtasks still
  close within their parent. A project's own land remains its own enforcement and must consume
  the workflow contract before that project activates custom steps.

- [r] **project-workflow-shape** — A project could give its board extra steps, such as Implement then Validate, while a project with no settings keeps today's flow. How much can it replace? A: names and column order only; the steps and permissions stay fixed. B: extra preparation and work steps, each tied to today's lifecycle, with rules for who moves between them; one ready stage, one review and the existing completion rules remain. C: any state graph, including several review or delivery stages, with new rules for staffing, recovery and completion. Recommended: B. Approve and say A, B or C in the note, or comment. [[analysis/project-workflow#The workflow choice]]
  [2026-11-17 10:34] agent — Posted for `tools/studio/project-workflow`. This is a design only; no project changes its flow before implementation.

    **A costs least:** a presentation setting and client labels, with no task migration or change to landing. It cannot add Validate as a real step.

    **B:** one workflow definition in `symphony.toml`, a step stored beside each task's current status, and one permission check used by every move. The board's staffing and recovery keep their current meanings. Columns, phone tabs, filters, timeline and agent instructions read that definition. The default land checks that the task reached review, then reads the same evidence as today; a project's own land adopts the step contract before custom steps activate. Both database backends need migration and tests.

    **C costs most:** a workflow engine replaces the fixed lifecycle in the board, settings, client, skills and every landing adapter. Each project must specify what claiming, losing an agent, a verdict and completion do. It is warranted by a project whose real process B cannot express; this task names none.

    **Gates and migration under every option:** existing configurable stamps and the project's own landing policy keep deciding the evidence. Its checks, tiers and nightly remain its own. A project with no workflow setting keeps today's operations; the consuming project's flow and gates must give the same decisions before and after. Neither a live migration nor a consumer edit is authorized by this card.

    **Earlier verdicts checked:** **needed-stamps**, approved as "it's exactly what I had asked for", already decides the generic catalogue and project replacement, so no new gate engine is offered. **board-at-a-glance** (B) keeps blocked work in the step it stopped in; all options preserve that. Neither decides whether projects can add steps or replace the lifecycle, which is this card's choice.
  [2026-11-17 12:16] owner · approved — C
  [2026-11-17 12:28] agent — C is the chosen direction: projects can define their own states and transitions, including multiple review or delivery stages, with explicit rules for staffing, recovery and completion. The board, Agents page, timeline and agent guidance must read that configured workflow. Projects without a workflow setting retain today’s behavior, and existing stamps and project landing policies continue to decide the required evidence. This supersedes the recommendation for B in the design analysis; the workflow implementation remains to be built.
  [2026-11-17 12:28] agent · resolved

## Gates: use the existing boundary

The [approved catalogue design](needed-stamps.md#needed-stamps) already settles the generic
default and replacement rule. The default catalogue has tests, independent review, pictures and
owner approval; only review is required for a task with no assessment. An assessment may select
none. Configuring the catalogue changes guidelines and defaults, not how a stamp is read. The
current reader recognizes only those four kinds. Custom project stamp kinds were explicitly
left outside that design.

| Project requirement | Where it belongs |
| --- | --- |
| Which generic evidence a task needs | `[stamps]` plus its independent per-tip assessment; the task page draws the same catalogue. |
| Check IDs, commands, input sets and risk tiers | The project's own check catalogue and runner. Symphony records their runs; it supplies no selector, certificate format or risk-tier engine. |
| Extra stamps or more than one reviewer | The project's own landing policy and typed gates adapter. Adding a visible plugin gate does not add a reader to the default land. |
| A check before review or another custom move | The project's own command produces evidence, then asks the board for the permitted move. B's role and route policy does not execute commands or enforce an arbitrary plugin gate. Such enforcement would need a separate concrete proposal. |
| Nightly, another schedule or none | The project's own scheduler and checks; no schedule is enabled by omitting a workflow table. This repository's release settings apply to this repository alone. |
| Owner sign-off | The generic owner stamp where its assessment needs it, or the project's existing sign-off policy. A design card verdict and approval of a task's tip are different evidence. |

There is consequently no new gate setting or command runner in this proposal. Gates remain visible
through the existing typed answer, with evidence links and commit identities. Project policy
still decides whether missing evidence prevents review or landing. This uses the existing
[plugin protocol](../../engineering/PLUGIN_PROTOCOL.md) rather than importing a consuming project's tools into
Symphony. Its cost here is documentation; implementing B changes workflow routing, not the
project's catalogue, runners or release schedule.

## Migration and defaults

The default definition must reproduce **all current operations**, not just draw the same seven
words: skipped preparation, backwards moves, blocked resumption, dependency cancellation,
review changes, approved-but-unlanded done, subtask completion, covered closes and concurrent
landing all keep their behavior. The default wide board and phone order also stay as they are;
Build and Test stay overlays. No project needs to add a file to retain this.

1. Add the shared reader and default definition, and test them against the current transition
   and permission cases. Publish the additive API before any client or skill relies on it.
2. Migrate a scratch copy of each database backend: existing statuses map to the same-named
   default step, blocked records retain their recorded source, and old thread lines keep their
   existing replay. Historical totals and verdicts are not reinterpreted through new edges.
3. Move board writers, client, PULL, recovery and skills onto the definition. Only then allow a
   committed custom definition to activate. Apply the existing schema/writer compatibility rule;
   an old writer must never silently collapse Validate back into Implement.
4. Prove two independent scratch projects: one with the consuming project's unchanged flow and
   landing adapter; one with **no `symphony.toml`**, taking defaults. Compare their commands,
   transitions and gate answers before and after. Add a third fixture with the custom validation
   step to prove that project policy, evidence and saved client state stay separate.
5. A later workflow edit that removes or changes the phase of a used step needs an explicit ID
   mapping and a preview of affected tasks, including blocked resume targets. Activate the new
   definition and mappings atomically, retaining the previous revision for history. Reverting
   settings alone cannot roll a board containing custom steps back to an older writer.

The consuming project's existing catalogue, tiers, certificates, scheduled checks, plugin gates
and land recipe remain in that project and are not converted into Symphony defaults. The
migration is accepted only when the same inputs produce the same decisions there. This analysis
changes no consumer checkout or live board schema.

## Evidence an implementation would need

This task delivers the analysis and decision only. A later implementation would need policy
unit tests for every phase, role and illegal shortcut; omitted-file parity; duplicate and unknown
IDs; stale policy revisions; unreachable required operations; and removed blocked targets.
Store tests would cover both backends' migration, copy and concurrent moves. CLI/API tests
would cover legal next actions and refusal of old writers; recovery tests would cover abandoned
custom work without advancing it. Browser acceptance would compare default and custom workflows
on a phone and wide, including blocked tasks, live counts, overlays and stale saved selections.
Landing tests would preserve the existing stamp rules and the consumer adapter's decisions.

No workflow code, coverage figure, migration trial or client performance measurement is claimed
by this design task. The document checks are the evidence for the text delivered here.
