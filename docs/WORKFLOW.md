# Workflow definitions

`symphony_api::workflow` supplies additive data types for delivery definitions, positions and
operation requests. `symphony_board::workflow` supplies the strict candidate reader,
normalization, revision identity, host prerequisite checks and grants, built-in definition v1 and
the reader of the project file the default branch commits; `symphony::project::workflow`
re-exports those items at their earlier paths. Existing public structs, Rust calls, task wire
fields, CLI operations and board writers keep their meaning on a board with no custom workflow.
A custom workflow's tasks move through the board's one admission ([below](#admission)), which
`tasks workflow`, three `tasks api` methods and the owner's `/api/workflow-apply` reach.

The [project file](PROJECT_FILE.md#workflow) accepts `[workflow]`: `candidate(text)` reads and
validates that table without activating it, and ordinary project reads accept a definition the
local host supports and refuse any other with `unsupported_activation`, naming what it needs
(`activate::supported`, [below](#activation-and-mappings)). Invalid definitions are refused with
their validation error. `Project::of` reports the refusal through its existing project problem
mechanism. Omission runs the current board implementation unchanged; `candidate` selects the
immutable built-in v1 definition.

The selected activation contract is automatic activation of a valid committed definition when
the board reads it, with complete mappings and prerequisites required, as decided by
[the activation card](DECISIONS.md#configurable-workflow-activation). The board activates
([below](#activation-and-mappings)), admits each operation ([admission](#admission)) and keeps
its records behind fences on every writer ([records](#records-on-the-board)). A candidate's
validation or a host prerequisite check is not activation. The
[graph design](analysis/configurable-delivery-workflow.md) describes the remaining system:
configured staffing, PULL, recovery and attempts (`tools/workflow-operations`,
`tools/workflow-attempt-integration`), the app's reading of phases and actions
(`app/workflow-actions`), landing adapters and guards each have a task of their own; artifact
policy receipts and execution attempts retain their separate ownership. A dependency on an
accepted output is admitted here ([accepted outputs](#accepted-outputs)).

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
branch commits (`workflow::follow`, `Store::follow_committed`), but `agent-send`, whose message
moves no task and whose program links none of the workflow: a project that commits no
`[workflow]` on a board that never activated one reads the branch tip and writes nothing, and a
repository without that branch commits nothing. A committed definition, or mappings, other than
the active one's is activated, in one transaction fenced on the active generation and on the
version of every task it reads (`Store::activate_workflow`); a commit that changes neither, any
landing of other files, leaves the active generation, every task's version and every record as
they were, and the active revision names the commit that activated it. It records the definition under its digest,
places every task a workflow holds and every open task the board's writers keep, records each
move as a transition by the board's activation service and the activation with the mappings it
applied, and moves the active generation to it. A task whose pair, resume pair and status stay
keeps its revision; done and cancelled tasks the board's writers kept stay theirs until a write
of theirs takes one out of done or cancelled (the default landing sends an approved task back to
review, say): that write places it, as its first transition, where built-in v1's pair for the
status it moves to stands on the active workflow, declared or mapped by the activation's
mappings, and is refused, naming the pair, where nothing maps it, the mapping would give it an
outcome, satisfied dependants or a completion, or the pair stands for another status. The database
refuses every other write that takes a task with no position out of done or cancelled while a
workflow is active. A new commit of other mappings or another definition (a label) is a new
generation, which moves no task whose pair it still declares.

The whole activation is refused, nothing written and the previous workflow left active, and the
command says why on stderr and runs on: a capability, adapter or guard the host does not support
(every guard, until revision-bound guards); recovery on attempt failure or cancellation
acknowledgement that moves work, automatic edges and retry bounds, which need durable attempts;
`landed` dependencies, parent integration, and an adapter's prepare or
finalize, as no adapter runs here yet and a prepared intent never completes on a caller's word; a
pair or resume pair neither declared nor mapped; a mapping onto a position recording an outcome, satisfied dependants or a completion its
source lacks; and a task whose live executor or reviewer it would move or restaff. A live holder
stands in for a live attempt until durable attempts are read here. A file that drops `[workflow]`
after an activation is refused as well: returning to the board's own writers is not supported
yet, and rolling back is a forward activation of another committed definition with mappings.

## Admission

Every move of a task a custom workflow holds is one typed operation admitted by the board
(`Store::apply_operation`, `symphony_board::workflow::admit`), whatever surface sends it: `just
tasks workflow apply SLUG EDGE --revision N`, the `workflow_apply` api method, the owner's
`/api/workflow-apply`, or a linked Rust caller. The principal is the host's, never the request's:
the CLI and the api bind the agent session the call names, which must be on the board and not
have left, and refuse the owner, who acts from the Symphony as the local owner. An
`OperationRequest` fences the caller's intention, not its authority.

Admission reads the active workflow, the task, its role rows, the tasks it depends on and the
key's receipt in one read, then refuses, by its code, in this order: a request naming another
project than the board's, or a task no workflow holds (`unknown_id`); a request whose digest, generation, workflow revision or pair is not the task's now
(`stale_workflow`, `stale_activation`, `stale_task`); an edge the definition does not declare, or
one that leaves another pair (`unknown_id`); an automatic edge (`unsupported_writer`); a prepare,
finalize or parent integration, which no host-verified adapter runs yet (`unsupported_adapter`);
any guard (`guard_unavailable`); a human wait taken by anyone but a person (`human_wait`); a
principal meeting none of the edge's authorities (`unauthorized`); an input the edge does not
declare (`unknown_id`), one it declares and the request lacks, an `acquire_role` taken by anyone
but an agent, or a resume to a pair other than the one its block saved
(`explicit_action_required`); an input revision naming anything but the accepted output of a task
it depends on, for an edge depending on `accepted_output`, as the board records no other
(`stale_input`); a dependency predicate not met (`dependency`), and for `accepted_output` an
output the request does not name (`explicit_action_required`) or one other than the output
standing (`stale_input`); and a start past the project's cap (`capacity`).

A task role is held on this task: a holder of its row, or, by an edge that takes the role, a
session no other holder or reservation stands against, its own reservation consumed. A
reviewer's role, by holding or taking it, is a fresh session's (RULES.md
`workflow/reviewers-not-the-owner`): never one that executed the task or work under it, filed it,
or orchestrates or orchestrated it, whatever the graph says. A capability is held only by a
principal the host's grants name. A `done` dependency is met by a task the workflow holds at a
position that satisfies its dependants, or one the board's writers keep in done; an
`accepted_output` dependency by the output standing for it, named
([accepted outputs](#accepted-outputs)); a dependency not on the board meets nothing.

The write is one transaction that checks again everything the decision read: the active
generation and digest, the task's version, each dependency's version, each role relied on, no
other holder or reservation of a role taken, the key unused, the agent on the board, and the
cap's counters. Anything moved since reads and decides again, so a request the board moved past
is refused as stale, never replayed on where another left the task. The write records the
transition and its receipt, drops and adds the role rows its effects name, puts the task at its
pair and projected status, settles its hold when its status moves, and posts `took EDGE
(SLUG@REVISION): FROM → TO` on its thread. It answers the transition's id, its effects and the
principal's next actions. A key is its principal's: a request repeating it answers the first
result when it is the same request, and is refused when it is another; `tasks workflow apply`
sends the receipt's request again when its command is repeated.

`Store::workflow_actions` previews each edge leaving the task's pair with what stops the principal
taking it now, its authority named without saying who holds it: never a permit.
`Store::workflow_read` answers the active definition, revision and generation. A task filed while
a custom workflow is active starts at the definition's initial pair, its first transition
recorded in the write that files it, and a filing that asks for another status is refused. A
filing is written as the board's own first, so a board with no workflow files it in one write as
before; that write refuses itself where a workflow is active, and the task is filed again at the
initial pair.
The board's own writers neither move nor staff a task the workflow holds: a move, claim,
reservation, verdict, block, close or takeover of its executor, reviewer or reservation refuses
as `explicit_action_required`, naming `just tasks workflow actions`, and a status written past
them meets the database's fence with the same code; a release and a leave go by the position's
recovery ([below](#recovery)). The orchestrator's role stays the board's coordination, which its
writers keep. `just tasks next` and PULL offer no task a workflow holds to a claim, nor name one
unstaffed, orphaned or unreviewed (a working one counts as executing), and the board's block of
a dependant whose prerequisite was cancelled leaves one to its edges' dependency predicates.
Staffing and the launcher do not read the workflow yet (`tools/workflow-operations`).

## Accepted outputs

A task's accepted output is the identity of the transition that accepted it, `SLUG@REVISION`:
the one an edge records when it moves the task to a position that satisfies its dependants from
one that does not (built-in v1's approve, covered close, subtask completion and land). The board
mints it, so it is never a caller's claim, and the transition record keeps it, never changed, as
its `accepted`. It stands while the task stays at positions that satisfy dependants: every later
transition there carries it, an edge's, an activation's mapping or a recovery's stay. A move to a
position that does not satisfy them supersedes it (a reopen, a rework, an activation whose new
definition no longer has the pair satisfy them, which records that as a transition that moves
nothing), and none stands until an edge accepts the task's output again, under a new identity. A
mapping or a recovery never accepts one, and a task the board's own writers keep, done before the
activation or not, has none.

An edge depending on `accepted_output` is taken only on the output standing for each task its task
depends on, each named in the request's `input_revisions` under that task's slug: none standing is
`dependency`, one not named is `explicit_action_required`, and one other than the output standing,
superseded or never accepted, is `stale_input`, so a caller that read an output a supersession
replaced never takes the new one unread. The receipt keeps the request, and with it the outputs it
took. The write is fenced on each prerequisite's version (held `FOR SHARE` in PostgreSQL), so a
supersession and an admission racing from one read let one of them win: a supersession written
first makes the admission decide again and refuses it, and an admission written first took the
output standing then. What a supersession does to a dependant already admitted or delivered is
not this check's (`tools/workflow-revision-guards`). `just tasks workflow actions SLUG` prints the
task's own output standing and, where an edge leaving its pair depends on accepted outputs, the
one standing for each task it depends on, which `workflow apply … --accept SLUG=OUTPUT` names;
`Store::accepted_outputs` reads the same.

## Recovery

A holder's exit from a task a custom workflow holds is answered by the position's configured
action, never by a target the board invents (`symphony_board::workflow::recover`), and written as
an admitted operation is, by the board's recovery service, which the host must grant `recovery`
(`Host::local` does). Three exits come there: an agent's own `just tasks release` of its role
(the position's `release` action), a session's `just tasks leave` (its `lost_holder` action, for
each task it staffs), and `just tasks workflow recover SLUG HOLDER`, which any session on the
board may ask for a holder whose session is lost or left (`Store::recover_holder`, its
`lost_holder` action); a live, stale or unswept holder lets go of its own.

The action reads the role the position staffs. A role the position does not staff (an executor's
left after its work went to review, a reservation) goes alone, and the position stays as it
was. The staffed role goes when the action releases it; where it does not, an agent's own release
is refused as `explicit_action_required` (an edge takes it out), and a lost or leaving holder keeps
it. Every reservation on the task goes when the action releases them. The task then moves to the
action's target, which clears a block's resume pair, or stays, keeping it: either way a
transition by the recovery service records the move or the stay with what it recovered (the
trigger, the holder, its role) and the slot effect, and a manual action leaves the position marked
until its next transition (`TaskPosition::manual`), its thread line saying it waits on a person.
A move into work is counted under the project's cap as a start is.

The write is fenced on the active workflow, the task's version, each role it relies on or drops
and, for a lost holder, the holder's session as it was read: a holder that beats again, or takes
an edge first, wins, and the recovery decides again from where it left the task. The orchestrator
role is never a workflow's: a release or leave of it is the board's own. Nothing recovers a lost
holder by itself yet, and attempt failure and cancellation acknowledgement wait on durable
attempts (`tools/workflow-operations`, `tools/workflow-attempt-integration`).

## Records on the board

From board schema 24 a custom workflow's records are one `workflow` table in both stores, each a
`kind` and a `body` under a compound id: the active generation (`['active']`), each definition by
its digest, each activation by its generation, and each transition of a task by its slug and the
workflow revision it made (`['event', SLUG, REVISION]`), with the accepted output standing after it
([accepted outputs](#accepted-outputs)). A task the workflow holds keeps its
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
require an independent task reviewer; admission enforces the exclusions regardless of any
configured label or actor string ([admission](#admission)), and refuses evidence revisions until
the board records them.

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
live migrations are activation's and admission's, beyond these syntax checks.
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
explicit-action-required refusal codes; a refusal's text starts with its code. Old wire consumers
receive no new fields from this delivery, and existing public construction and calls remain
available through the compatibility release.

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
