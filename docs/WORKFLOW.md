# Workflow definitions

`symphony_api::workflow` supplies additive data types for delivery definitions, positions and
operation requests. `symphony_board::workflow` supplies the strict candidate reader,
normalization, revision identity, host prerequisite checks and grants, built-in definition v1 and
the reader of the project file the default branch commits; `symphony::project::workflow`
re-exports those items at their earlier paths. Existing public structs, Rust calls, task wire
fields, CLI operations and board writers keep their meaning. No workflow HTTP or board API method
is installed by these data types.

The [project file](PROJECT_FILE.md#workflow) accepts `[workflow]` only as a candidate format:
`candidate(text)` reads and validates that table without activating it. Ordinary project reads
refuse supplied custom definitions with `unsupported_activation`, including a supplied copy of
the default. Invalid definitions are refused with their validation error. `Project::of` reports
the refusal through its existing project problem mechanism. Omission runs the current board
implementation unchanged; `candidate` selects the immutable built-in v1 definition.

The selected activation contract is automatic activation of a valid committed definition when
the board reads it, with complete mappings and prerequisites required, as decided by
[the activation card](DECISIONS.md#configurable-workflow-activation). The board activates
([below](#activation-and-mappings)) and keeps its records behind fences on every writer
([records](#records-on-the-board)); authoritative admission remains implementation work. A candidate's validation or a host
prerequisite check is not activation. The [graph design](analysis/configurable-delivery-workflow.md)
describes the remaining system; artifact policy receipts and execution attempts retain their
separate ownership.

## Activation and mappings

A board activates only what its project's default branch commits: `committed(main, branch)` reads
the project file from the tip of that branch's tree, never the index or a working tree, and a
file that is a link, a folder or not UTF-8 refuses the read. A file with no `[workflow]` is the
board's existing writers. A custom definition carries its revision (the commit and the file's
blob) and its `[[workflow_mappings]]` ([PROJECT_FILE](PROJECT_FILE.md#workflow_mappings)): each
maps a pair the definition no longer declares to one it declares, once, and never remaps a pair
still declared, since an ID keeps its meaning. A task the existing writers kept starts from
built-in v1's pair of its status (`legacy_pair`).

A capability a definition declares is supported only when the host's `Grants` give it to a
principal, and a principal holds only the capabilities a grant names: the definition's
declaration mints nothing, and an unknown capability fails the prerequisites. A principal is
`symphony_plugin::execution::Principal`, built by the host from what it authenticated (an agent
session by its handle, the local owner, a board service), never read from a request. The local
host's policy is fixed in code (`Host::local`): the local owner holds `owner`, the board's
recovery service holds `recovery`, and no adapter is registered. A hosted board brings its own
`Host`, built from the principals its ingress authenticated; nothing in a project file or a
request extends it.

Every command that opens a project's live board to write follows the workflow its default
branch commits (`workflow::follow`, `Store::follow_committed`): a project that commits no
`[workflow]` on a board that never activated one reads the branch tip and writes nothing, and a
repository without that branch commits nothing. A committed definition whose digest is not the
active one is activated, in one transaction fenced on the active generation and on the version
of every task it reads (`Store::activate_workflow`). It records the definition under its digest,
places every task a workflow holds and every open task the board's writers keep, records each
move as a transition by the board's activation service and the activation with the mappings it
applied, and moves the active generation to it. A task whose pair, resume pair and status stay
keeps its revision; done and cancelled tasks the board's writers kept stay theirs. A new commit of
identical content is a new generation that moves nothing.

The whole activation is refused, nothing written and the previous workflow left active, and the
command says why on stderr and runs on: a capability, adapter or guard the host does not support
(every guard, until revision-bound guards); recovery on attempt failure or cancellation
acknowledgement that moves work, automatic edges and retry bounds, which need durable attempts;
`accepted_output` and `landed` dependencies, parent integration, and an adapter's prepare or
finalize, as no adapter runs here yet and a prepared intent never completes on a caller's word; a
pair or resume pair neither declared nor mapped; a mapping onto a position recording an outcome, satisfied dependants or a completion its
source lacks; and a task whose live executor or reviewer it would move or restaff. A live holder
stands in for a live attempt until durable attempts are read here. A file that drops `[workflow]`
after an activation is refused as well: returning to the board's own writers is not supported
yet, and rolling back is a forward activation of another committed definition with mappings.

## Records on the board

From board schema 23 a custom workflow's records are one `workflow` table in both stores, each a
`kind` and a `body` under a compound id: the active generation (`['active']`), each definition by
its digest, each activation by its generation, and each transition of a task by its slug and the
workflow revision it made (`['event', SLUG, REVISION]`). A task the workflow holds keeps its
position in `fields._board.workflow`: its pair, the pair a blocked task resumes to, the generation
and digest it was last moved under, and its workflow revision, which only its own transitions move.
The position, and the status it projects to, change only in the one write that records their
transition (`symphony_board::workflow::Transition`), from the revision the writer read; of two
writers from one revision one commits, and the other is refused as `stale_task`.

The database holds every writer to that, the board's own writers and its generic field, import
and copy paths included. A write that changes a task's position, or the status of a task that has
one, is refused unless the position is the next revision and the transition recorded for it names
the same pair, resume, generation, digest and status. A position is never removed, and a task is
never put back at a position its later transitions left. A recorded definition, activation,
transition or receipt is never changed, and the active generation moves forward only to an
activation recorded for it. `Store::raw` refuses the table. A board with no custom workflow holds
none of these records, its export is the text it was, and every other write runs as before. The
whole-board copy carries the records before the tasks, so a copy that lost them cannot restore a
task the workflow holds. A reader one schema behind keeps and ignores the position.

## Definition format 1

IDs begin with a lowercase ASCII letter and contain only lowercase ASCII letters, digits,
`_` and `-`. IDs are case-sensitive and unique within each namespace. Labels are display text
and confer no behavior or authority. The root fields are in the project-file reference.
Every record rejects unknown fields and every enum rejects unknown variants. Optional fields
are omitted in TOML or `null` in JSON; all other fields are required. Empty lists are explicit.

| Record | Fields |
| --- | --- |
| Pair | `phase`, `state`: IDs naming one explicitly declared position; not a Cartesian product. |
| Phase | `id`, nonempty `label`, unique unsigned `order` and `phone_order`, `column` boolean. The default's backlog/blocked/cancelled groups are outside its six delivery columns; Build and Test are separate process overlays. Phone order follows the current board, including backlog. |
| State | `id`, nonempty `label`. These are coordination states, never execution-attempt status. |
| Position | `at` pair; optional `staffing` (`executor`, `reviewer`, `orchestrator`, omission means none); `slot`; `dependencies`; `unsuccessful_dependency`; booleans `accepts_work`, `terminal`, `satisfies_dependencies`; conservative `legacy`; optional `outcome`; `completion`; optional `recovery`. |
| Recovery | Required actions `release`, `lost_holder`, `attempt_failure`, `cancellation_acknowledged`. Every staffed position declares all four. |
| Recovery action | Optional `target` pair; boolean `manual`; `slot`; booleans `release_role` and `release_reservation`. No target means stay and requires explicit manual recovery. A target must be ordinary nonterminal coordination work without outcome, dependency-satisfaction or completion facts; completing work requires a typed edge. |
| Guard | `id`, `adapter`: an opaque required policy receipt, with no executable expression or cached pass. |
| Edge | `id`, nonempty `label`, exact `from` and `to` pairs, `operation`, optional legacy `alias`, nonempty `authority`, `inputs` ID list, `dependencies`, `guards` ID list, ordered `effects`, boolean `automatic`, optional `retry`, boolean `human_wait`. |
| Retry | Positive unsigned `maximum`, `exhausted` pair with explicit manual recovery. |

`slot` is `retain`, `acquire` or `release`. Dependency predicates are `none`, `done`, `landed` or
`accepted_output`; unsuccessful predecessor handling is `block`, `cancel` or `manual`.
Outcomes are `success`, `cancellation` or `failure`. Completion requirements are `none`,
`landing`, `covered_close` or `parent_integration`. A terminal position declares an outcome and
allows no ordinary outgoing edge, staffing or accepted work. Dependency satisfaction is a
separate declaration: default approved-but-unlanded work retains the existing `done` predicate
without being declared a repository landing or terminal delivery.

Legacy projections are `backlog`, `concept`, `design`, `ready`, `in_progress`, `review`, `done`,
`blocked`, `cancelled`. They preserve compatibility views and do not authorize legacy custom
mutations. Each source pair has at most one route per alias and per operation/alias combination;
multiple explicit aliases may expose choices of the same operation without choosing for a client.

## Authority, operations and effects

Authority records are tagged by `kind`: `task_role` requires `role`; `project_capability` and
`system_capability` require `capability`. A declared name never grants membership or authority.
The host must supply the capability and adapter support independently (`prerequisites`) and
must bind a trusted principal and its task relationship outside caller-controlled JSON at
admission. Authority alternatives are an unordered OR. A human wait requires every alternative
to be a project capability and requires a deliberate action; adding a task-role or system
alternative cannot bypass the human requirement. Review operations
require an independent task reviewer; the future admission service must enforce the exclusions
and evidence revisions, regardless of any configured label or actor string.

Operations are `create`, `claim`, `release`, `advance`, `request_review`, `approve`, `changes`,
`block`, `resume`, `retry`, `cancel`, `complete_subtask`, `close_covered`, `prepare_land`,
`confirm_land`, `prepare_external`, `confirm_external`. Creation away from the initial position
requires an explicit route. External operations use declared adapters; no operation executes
project code while validating a definition.

Effects are tagged by `kind` with these exact parameters:

| Kind | Parameters |
| --- | --- |
| `acquire_role`, `release_role` | `role` |
| `slot` | `policy`: `retain`, `acquire`, `release` |
| `review` | boolean `approved` |
| `block` | boolean `save_resume` (a block operation requires true) |
| `clear_block` | none |
| `outcome` | `outcome`, boolean `satisfies_dependencies` |
| `parent_integration` | none |
| `prepare`, `finalize` | declared `adapter` |

Every edge explicitly declares a slot effect. Claim/release require their role effects,
approve/changes their matching review effect, block/resume their block effect, cancel its
cancellation outcome, subtask completion its parent integration and success outcome, covered
closure its success outcome, and external prepare/finalize their matching effect. Advance
cannot impersonate reviews, parent integration, outcomes or external effects. The validator
checks structure and referenced requirements; it never evaluates a guard or mints a receipt.
Terminal routes must record the destination's declared outcome/dependency satisfaction and
meet its landing, covered-close or parent-integration requirement when one is declared.

Deliberate rework cycles are legal. Every automatic edge participating in a cycle requires a
finite retry maximum and manual exhaustion destination. Each nonterminal position must reach
an outcome or explicit manual recovery; every configured terminal completion must be reachable
from the initial position. Host prerequisites, retained history, input revisions, capacity and
live migrations require the future admission/storage engine beyond these syntax checks.
Recovery targets, including retry exhaustion, cannot record completion through the limited
recovery-action format. They must use a typed completion edge for outcomes, satisfied
dependencies, terminal delivery or a landing/covered-close/parent-integration boundary.
Terminal positions may declare a manual stay, but no recovery route out to further work.

## Revisions and additive API records

`Validated::new` validates then normalizes namespace lists by stable ID; unordered input/guard
lists and authorities are normalized too. Phase display/phone order, labels and effect order
remain significant. The normalized definition, format, source commit and project-file blob
form a SHA-256 revision digest. Source IDs must be lowercase Git object IDs (40 or 64 hex
characters). The caller obtains committed source metadata from its Git reader; the hashing
function validates its shape and does not attest that caller's provenance. `committed(root,
reference)` reads a real commit and its project-file blob from a repository root, ignoring the
index and working tree; a missing, linked, non-blob or non-UTF-8 file refuses the read. A project
file with no workflow table still selects the default candidate.
`check_revision` rejects substituted content, format, digest or metadata. Revision identity
does not supply an activation generation or an execution-attempt identity.

`WorkflowRead` carries definition, revision and activation generation. `PositionSnapshot`
carries workflow revision, activation generation, monotonic task revision and exact pair.
`OperationRequest` carries project/task identity, edge ID, expected snapshot, input revision
map, actor-bound idempotency key and typed input map; it contains no trusted actor. Input values
are tagged by `kind` (`text`, `revision`, `pair`, `flag`) with their typed `value`.
`OperationResult` carries the committed transition ID, effects and current `NextActions`.
Each preview action names edge/operation, destination, label, responsible authority requirements,
required inputs and stable unavailability codes. A preview is never permission to write.

The data types provide stale task/workflow/activation/input, unknown/removed ID, unauthorized,
dependency, capacity, human wait, unavailable guard, unsupported activation/writer/adapter and
explicit-action-required refusal codes. There is no installed apply-operation service yet.
Old wire consumers receive no new fields from this delivery, and existing public construction
and calls remain available through the compatibility release.

## Built-in behavior

`builtin()` returns a fresh copy of version 1 in the same definition format. Its move routes
match every pair in the current board's transition matrix. Preparation claims, held preparation
and review, ready/work claims, release recovery, backward moves, blocks and held blocks,
verdicts, approved-but-unlanded work, reopen, subtask and covered closure, and landing boundaries
are declared separately. The opaque `legacy_board` requirement names the current board's
authority, dependency, stamp, parent, hold, completion and landing predicates. It supplies no
replacement stamp engine. Existing writers continue to enforce those predicates directly.
Covered-close routes exist for every open legacy position, including backlog, preparation,
blocks and held preparation/review, under that existing holder-and-covering-landing predicate.
