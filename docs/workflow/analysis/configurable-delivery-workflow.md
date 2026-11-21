# Configurable delivery workflows

A project defines its delivery phases and the states work can occupy within them. Every way of
advancing work uses the same server admission rules. A task can therefore be in Verify while
awaiting review, or in Build while blocked, without renaming a column or losing its phase.

This is the implementation contract proposed for the C direction on
[the workflow card](../../DECISIONS.md#project-workflow-shape). It is not implemented. The earlier
[options analysis](project-workflow.md) explains the alternatives; its B recommendation does not
govern this contract. The one additional product choice is
[how a changed graph becomes active](../../DECISIONS.md#configurable-workflow-activation).
Project artifact approval semantics remain the separate design task
`design/revision-bound-transition-guards`; execution providers and immutable attempts remain
`tools/remote-execution-attempts`. Neither dependency is claimed available here.

## Evidence and implementation seams

Source inspected at `3b460adb62827f674669e8e99f3090561107264f`:

| Concern | Source and present constraint |
| --- | --- |
| Project settings | `crates/symphony/src/project/file.rs`, `File` and `unknown`: no workflow table; strict loading reports it as unknown. |
| Task operations | `crates/tasks/src/store/work.rs`, `STATUSES`, `TRANSITIONS`, `moving_from` and `landed`: fixed lifecycle, with special block, reopen and completion paths. `store/mod.rs`, `claim` and `ruling`, already arbitrate claims and versioned writes. |
| Persistence | `crates/tasks/src/schema.rs`, `store/postgres/schema.rs`, `store/transaction.rs` and `store/opening.rs`: paired stores, schema-guarded transactions and restricted live migration. `rows.rs` denies unknown task fields. |
| Compatibility | `store/writers.rs` records programs and schema versions. A schema guard fences every ordinary write, including a writer already open when migration happens. `cli/api.rs` promises additive changes within its API major. |
| Operations | `launch/staff.rs`, `launch/reap.rs`, `slots.rs`, `cli/pull/` and `store/cap.rs` select behavior by fixed statuses. Task dependency satisfaction is distinct from a successful process exit. |
| Presentation | `apps/symphony-web/src/boardwait.tsx` fixes columns; `crates/symphony-api/src/lib.rs` carries status and blocked source; `crates/tasks/src/stages.rs` replays known thread lines. Build and Test are process overlays. |
| Evidence | `stamps.rs`, `stamps/read.rs` and `land/needs.rs` own existing stamps. `crates/symphony-plugin/src/lib.rs`, `Gates`, supplies displayable project evidence; `BoardAction::submit` has no authenticated-person argument. A cached gate answer is not a transition authorization. |

[The source goal](https://toptal-core.atlassian.net/browse/UNS-1196) asks for project columns and
gates. [Blueprint to Deploy](https://toptal-core.atlassian.net/wiki/spaces/UNS/pages/6597967921),
version 7, describes intended requirements, not working code: separate phase and state, human
handoffs on exact artifacts, rework, durable attempts and evidence. Its prose was read; its
attachment images were not inspected. This contract imports no client data, provider, gate
assignment, deployment target or consuming-project configuration.

## Three separate identities

| Object | Identity and meaning |
| --- | --- |
| Delivery phase | A project-defined stable ID, such as `blueprint`, `build`, `verify` or `deploy`. Labels and board order are presentation. No built-in enum decides what the word Build means. |
| Task coordination state | A project-defined stable ID, such as `available`, `working`, `awaiting_review`, `blocked`, `failed` or `accepted`. It describes what can happen to the task. It is independent of phase. |
| Execution attempt | An immutable attempt ID with provider/run lifecycle, input revision and fencing identity. Running, cancellation requested, collection and provider failure belong to that attempt. Several historical attempts can belong to one task. |

The task's authoritative position is `(workflow_revision, phase_id, state_id)`, with a monotonic
task revision. A graph enumerates valid phase/state pairs and edges between pairs; it need not
permit the Cartesian product. A state may be reused in several phases. An edge changes either
coordinate or both. ID syntax is lowercase ASCII letters, digits, `_` and `-`, starting with a
letter; IDs are unique within their phase/state/edge namespace and never silently case-folded.
Labels may change; an ID cannot be reused to give retained history another meaning.

The workflow revision identifies an immutable normalized definition, its format version, the
committed project-file blob and source commit. Its digest includes behavior, labels and order;
a label edit can stale a UI request but cannot reinterpret an earlier event. An activation
generation increases even when reactivating identical content, fencing requests from before a
rollback or restore. Definitions remain readable for as long as retained records reference them.

Task outcome, dependency satisfaction, worktree cleanup eligibility and repository landing are
separate recorded facts. In particular, an approved task may still await landing or deployment.
Provider success and an agent's final message imply none of these facts.

The attempt-design join uses project/board/task identity plus attempt UUID and immutable input
revision/digest. Input revision is distinct from task coordination revision. Result promotion
also checks the current-attempt pointer, operation key, attempt-state revision, worker lease
epoch and provider resource/runtime generation, including the restore execution epoch. Persist
the initiating trusted principal separately from the service applying the result. These are
requirements agreed on the execution task thread, not a released attempt API.

## Definition and validation

The proposed `[workflow]` in committed `symphony.toml` replaces the whole default definition.
Omission selects a versioned built-in definition reproducing current behavior. A supplied but
invalid table refuses activation; it never falls back to defaults. All names in this section
are proposed data fields, not settings accepted by the current reader.

| Definition part | Required content |
| --- | --- |
| Identity and entry | Format version; initial phase/state pair; supported task kinds. Creation at another pair requires an explicit creation route and authority. |
| Phases | Stable ID, label, display order. Retained inactive phases stay resolvable in history. |
| States | Stable ID and label. State names carry no hidden claim, terminal or review semantics. |
| Positions | Valid phase/state pair; staffing role or explicit `none`; task slot policy (`retain`, `acquire` or `release` through its incoming edge); dependency policy; whether the position accepts work or is terminal; conservative legacy status projection for old read-only views. |
| Edges | Stable ID, exact source and destination, typed operation, authorized task roles/capabilities, required inputs, dependency predicate and explicit effects. No executable expressions or shell commands. |
| Recovery | For each position with a holder: action for role release, lost holder, attempt failure and cancellation acknowledgement; target pair or explicit stay/manual recovery; reservation and slot effects. |
| Completion | Explicit success, cancellation or failure outcome; whether dependencies are satisfied; whether landing, covered-close or parent integration is required. Terminal means no further ordinary work, not merely a label. |
| Presentation | Column/group and phone order by phase; supported state filters and labels. Process overlays are outside this list. |

Supported effect primitives are board operations: acquire/release a role, acquire/release/retain
a task slot, record a review verdict, record/clear a block with its resume pair, record an
outcome, accept a parent integration, and prepare/finalize an external effect. Projects compose
them into their graph; they do not add executable effects. `claim`, `release`, `advance`,
`request_review`, `approve`, `changes`, `block`, `resume`, `retry`, `cancel`, `complete_subtask`,
`close_covered`, `prepare_land` and `confirm_land` name operation classes, not fixed destinations.
An additional deployment adapter uses the same prepare/finalize boundary. No edge can use
`advance` to impersonate approval, parent integration, a merge or deployment.

Validation rejects duplicate/unknown IDs, unknown keys or operations, invalid pairs, ambiguous
legacy aliases, missing effect parameters, nonexistent capability/adapter references and
unreachable configured completion. Every nonterminal position must have a route to an outcome
or explicit manual recovery, and every staffed position must define lost-holder handling.
Cycles are allowed for rework; automatic cycles require a finite retry bound and a manual
destination when exhausted. Human waits require an identified capability, never an agent retry
loop. A graph without reviews is legal when project evidence policy requires none. A graph
with reviews cannot weaken reviewer independence. Activation also validates all retained live
positions and references, not merely graph syntax.

The configuration exposes the built-in definition in exactly the same format. It supplies
concrete effects and routes for the current preparation claims, ready claim, backwards moves,
blocks, held blocks, verdicts, approved-but-unlanded work, subtask closes and covered closes.
The implementation derives those routes from current behavior tests rather than inventing a
stricter default from the custom example below.

## Admission and authority

One board service admits typed operations; UI, CLI/API, skills, staffing, reaper, attempt result
handlers, completion and landing adapters call it. Store methods remain the enforcement
boundary even for linked Rust callers. A metadata field setter, raw status write, callback or
plugin button cannot update the authoritative position. Import/restore is a separately
authorized maintenance path that validates the same invariants and writes an audit record.

An agent principal is bound to its authenticated session, project and held task role. An owner
or project-person capability comes from a trusted host authentication/authorization context,
not a request's `handle`, role string or plugin submission. A system principal has only the
particular recovery/dispatch capability assigned to it. Project capability names refer to
membership supplied by the host/project identity adapter; unknown membership refuses the action.
Local owner transport retains its existing authority. Remote human routes remain unavailable
until trusted actor propagation exists. This contract defines that requirement without adding
an identity service.

Admission checks capability and task relationship together. A configured executor edge does
not grant every registered executor access to every task. Independent review preserves the
current exclusion of those who filed, requested, orchestrated or executed the work, including
its work subtasks. Multiple review stages create distinct review rounds on the input revision;
an earlier approve does not answer a later round unless the existing evidence policy expressly
says so. Graph configuration cannot waive independence, task confinement or parent closure.

An operation request carries project/task identity, edge ID, expected task revision, expected
workflow revision and activation generation, expected source pair, input revision references,
an actor-bound idempotency key and typed operation inputs. The trusted principal travels
outside caller-controlled JSON. A successful answer returns the committed transition ID, new
task revision, position, role/slot effects and current next actions.

1. Resolve the active immutable definition and read the task, roles, dependencies, capacity and
   relevant evidence generations. Derive the concrete action and authority from that snapshot.
2. Obtain any project policy decision outside a database transaction. Its receipt is scoped to
   that exact project, task, edge, workflow generation and input/evidence revisions. Missing,
   pending, rejected, outdated, unknown, error or timeout refuses admission. No arbitrary command
   runs in a transaction.
3. In one transaction, compare every revision used by the decision, the active generation and
   actor authority; reevaluate board predicates; then write position, roles, slots, block/outcome,
   review facts, the structured transition event, idempotency result and durable effect intent.
   Dependency or capacity races must conflict or lock their shared records in both stores.
4. Deliver notifications and external work after commit. A crash cannot erase the intent or
   create an unrecorded board transition. Reconciliation repeats the same typed operation with
   its key and fencing identity, never an unconditional status patch.

Two requests from one source revision yield one transition. The loser receives a stale
response, not an automatic replay on the winner's destination. A repeated key with the same
actor and payload returns the original result; the same key with different payload is refused.
Internal database retries may reread to arbitrate storage contention but cannot replace the
client's expected source/revisions. Unrelated thread posts need not invalidate position
requests; any fact used by admission has its own checked revision.

## Legal next actions and errors

Additive workflow-read, next-actions and apply-operation methods expose this contract. Their
exact API method names are implementation work; none exists today. The read returns the active
definition/revision/generation. Next actions is scoped to a task and principal and returns the
snapshot revision, edge/operation IDs, destination pair, display label, responsible role,
required inputs and eligibility reasons. It can show a human wait or an unavailable action
without granting the caller that capability. Sensitive evidence and membership are filtered by
the same access policy as the task. A legal-next-actions answer is a preview, never a permit.

| Refusal | Required response and effect |
| --- | --- |
| Stale task, workflow, activation or input/evidence revision | Stable error code, current accessible snapshot and next actions; no partial write. Caller deliberately refreshes. |
| Unknown phase/state/edge | Name the ID and active workflow revision; never infer from a display label or accept a free-form target. |
| Removed ID | Identify the retained revision and applicable migration; no implicit reset to ready. |
| Unauthorized operation or missing trusted actor | Refuse without leaking private membership or artifact details. |
| Open/failed dependency, capacity or human wait | Identify the actionable blocker and responsible role where visible; retain phase/state and evidence. |
| Unavailable guard or unsupported writer/adapter | Refuse the guarded operation, with the missing capability/version. Display caches cannot turn unknown into pass. |

Legacy commands and API methods keep their current meaning for the default workflow. A custom
workflow-aware CLI can resolve an unambiguous legacy alias after reading next actions and then
submit the expected revisions, or return `explicit_action_required`. A legacy API request
without the required source/revision tokens refuses custom-workflow mutation; the server cannot
invent a fresh client intention by reading the current task. Existing signatures remain working
on default projects beside the new API for at least the consumer two-release window. Existing
API fields are not renamed, removed or changed in meaning within their major.

## Worked graph and recovery

This table is an illustrative profile fragment, not the complete default or a loadable TOML
file. All referenced positions and recovery routes must be supplied in a real definition.

| From | Operation and actor | To and effect |
| --- | --- | --- |
| Build / available | Claim by eligible executor, dependencies and capacity satisfied | Build / working; acquire executor and one task slot. |
| Build / working | Block by holder | Build / blocked; save exact resume pair and reason; explicit hold or release of slot. |
| Build / blocked | Resume by authorized holder/coordinator | Build / working only after current dependencies and capacity pass; no phase loss. |
| Build / working | Submit by executor | Verify / awaiting_review; record candidate inputs, release executor's active capacity, request reviewer. |
| Verify / awaiting_review | Claim review by fresh reviewer | Verify / reviewing; acquire reviewer, retain delivery phase. |
| Verify / reviewing | Changes by that reviewer | Build / available with rework link and failed evidence retained; prior attempt remains historical. |
| Verify / reviewing | Approve by that reviewer, project policy satisfied | Deploy / awaiting_acceptance; review recorded, task still undelivered. |
| Deploy / awaiting_acceptance | Authorized human handoff through project guard | Deploy / available; approval references exact package/target, without performing deployment. |
| Deploy / working | Attempt fails | Deploy / failed, with attempt link and finite retry/manual recovery; no completion. |
| Deploy / failed | Deliberate retry by permitted actor | Deploy / available; new attempt identity, retained failed outputs and evidence. |
| Any configured open pair | Cancel by authorized actor | Same phase / cancelled; fence dispatch/results and record intent to cancel an active attempt. |

An invalidated requirement baseline can follow an explicit rework edge back to Blueprint.
That transition records the previous candidate and the new input generation; it does not
delete tests, verdicts, artifacts or attempts, or make old approval current again.

Dependency eligibility is explicit per consuming edge: require predecessor completion, landing,
or an accepted output identity. The built-in definition preserves its current `done` predicate,
including cancellation propagation. Custom profiles must name whether cancelled/terminal-failed
dependencies block, cancel dependents or require manual resolution; success is never inferred
from a phase label. Milestone/output dependencies use immutable accepted revision identities.
When that output is superseded, new admissions fail until current policy accepts the replacement;
invalidation of already-running or already-delivered dependents belongs to the project guard
design and creates rework records rather than erasing completed history.

Staffing and PULL consume the same eligible actions and responsible roles. A phase called Verify
does not inherently select a reviewer; the configured position does. Human waits are routed to
their authorized people. Reservations, assignees, project/orchestrator limits and independent
review remain checks of the claim operation. Task WIP and attempt/provider capacity are distinct
ledgers: transferring executor to reviewer must explicitly transfer/release task capacity, and
attempt reservation remains the attempt service's responsibility. Both are transactionally
reserved when an action starts both kinds of work. No scheduler starts an attempt before its
claim/dispatch intent is committed.

The reaper uses configured lost-holder operations. It releases a lost role, then stays or moves
to the specified pair with recorded reason and slot effects; it cannot invent a ready target.
Recovering a conversation does not itself resume a blocked task. Attempt failure or success is
a fenced observation, then a proposed graph operation, checked against current task/workflow
and input revisions. A late result can be retained on its historical attempt but cannot advance
cancelled, superseded or reassigned work. Cancellation and retry races are arbitrated by that
same compare-and-swap path. Exactly-once external work is not promised by a board transaction.

## Evidence, completion and external effects

The generic engine owns graph order, task authority, transactions and audit. Existing stamps
own generic evidence. Project policy owns artifact criteria, approval membership, applicability,
check runners, risk tiers, certificates and release/deployment rules. Graph edges may reference
a named policy guard, but do not define a second stamp catalogue, arbitrary evaluator or runner.
An assessment selecting no generic stamps cannot waive a project's required handoff.

The dependent guard design must supply a versioned receipt and atomic generation comparison
for the inputs admission uses. An arbitrary plugin display answer, filesystem mtime or cached
green badge cannot satisfy this requirement. If the board cannot atomically verify freshness,
the guarded edge is unavailable until a suitable adapter exists. Guard evaluation must not
perform its irreversible effect before admission. The separate task decides artifact identity,
invalidation/carry-forward and authenticated approval storage; this contract reserves their
revision references and refusal behavior without claiming those mechanisms implemented.

The default land keeps existing stamp readers and project policy. A custom land integrates
through prepare/finalize: prepare admits an edge and reserves the delivery operation, binding
project/task, repository identity, PR identity where applicable, exact reviewed head, authoritative
landing-policy revision, workflow generation, input/evidence snapshot revisions and destination.
The adapter revalidates those bindings before recording its durable keyed merge/deploy intent,
then checks the actual remote/head/target preconditions before sending the external request.
The intent is committed before external I/O. A stale or cancelled reservation found before
dispatch refuses the effect. Finalization requires the authoritative remote squash/deployment
receipt and the adapter's required reconciliation; provider or graph success alone cannot
finalize delivery. A managed approve writes no verdict message: the head a landing binds as
reviewed is the `head` input of the approve transition's request, read back from its receipt.
[WORKFLOW](../WORKFLOW.md#external-effects) is the contract as built.

A request already in flight cannot be recalled by a later evidence withdrawal or cancellation.
Reconciliation records the actual outcome against the policy/evidence snapshot that authorized
its intent, and records a recovery requirement when current policy no longer accepts it. Graph
state alone is never proof of Git merge or deployment. The GitHub landing design owns head-bound
evidence, merge-time revalidation, merge authority, squash provenance and reconciliation. Any
stronger cross-system sealing mechanism is that design's proposal to assess, not a requirement
silently added by this graph contract.

If a crash occurs after an external merge but before board finalization, reconciliation reads
the actual effect by its idempotency/provenance identity, records its receipt and finishes the
same operation. If policy changed in the gap, record the real external outcome and a recovery
requirement; never pretend the merge did not happen or mark fresh policy satisfied. Adapters
may activate only edges whose promised head/policy/evidence semantics they can enforce at the
side-effect boundary; merge-time snapshot validation does not promise atomic remote revocation.

A subtask still completes by executor integration into its parent, not an independent release.
That completion names the integration commit and accepted outputs through admission. Only the
parent enters its independent review/delivery cycle. Covered close names the other task's actual
landing. Neither path bypasses required project guards. Sweep eligibility follows authoritative
completion/landing and retained owner lines, never a custom terminal-looking state name.

## Version changes, migration and rollback

The proposed activation policy is pending on
[its own card](../../DECISIONS.md#configurable-workflow-activation). Reading a committed definition
and making it authoritative are distinct operations. A task branch cannot grant itself authority.
The candidate must come from the project's committed default branch; an adapter records its
capabilities/version before any custom graph activates.

1. Ship additive reads and the built-in definition first. Keep existing task status as its
   compatibility projection, with the same meaning; store workflow metadata in a reserved
   board-owned record/namespace that older task decoders ignore and preserve. Do not add fields
   to strict session rows. Structured transition, immutable definition and activation records
   are new paired-store data retained by copy/export/restore. Each custom position explicitly
   projects to one of the existing statuses for a coarse old read-only view: blocked stays
   blocked, review stays review, and `done` is never used for unaccepted work. The projection
   grants no authority and no workflow-aware scheduler or dependency reader uses it. If a
   position cannot preserve an old field's meaning, that old read surface must refuse it as
   unsupported; its meaning is not repurposed to fit the graph.
2. Introduce a schema version fencing **all** old writers before workflow-aware writes start,
   including running processes. Writer inventory provides a preview; the transaction guard is
   the enforcement. A writer merely reporting a capability cannot bypass it. Workflow-aware
   readers get phase/state; an old reader sees its preserved legacy status and no new meanings
   in that field. Old-client actions on custom tasks refuse safely rather than collapsing them.
3. Backfill the default graph on a scratch copy of each supported store. Map backlog, concept,
   design, ready, in_progress, review and done to their same-named phase and state. Map blocked
   to state blocked and its recorded source phase/resume pair; cancelled retains its last
   recorded phase. When source history is insufficient, use a retained `unassigned` phase and
   manual-resolution marker, never guess Build. Existing approved-but-unlanded done, landed,
   subtask and covered outcomes are distinguished from their recorded facts. Missing provenance
   remains unknown. Legacy history is kept and replayed by its old parser.
4. Preview a custom activation against a board revision: all affected task pairs, resume pairs,
   pending actions, roles, dependency outputs and active attempts. Require explicit pair/edge
   mappings for removed or semantically changed IDs, including hidden blocked resume targets.
   Labels/order alone need no pair mapping. Mapping does not fabricate approvals, successful
   outcomes or new accepted outputs. Historical definitions and IDs remain untouched.
5. Activate with expected old generation, candidate digest and preview/board revision. In one
   transaction fence competing writes, apply complete live mappings, retain source/destination
   positions in migration events and publish the new generation. A stale preview, unknown ID,
   unsupported adapter/writer or missing mapping refuses the entire activation. In-flight
   attempts either drain before activation or are explicitly invalidated with cancellation
   intents; they are never rebound to new inputs. No partial batch is visible as an active graph.
6. Roll back by a new forward activation with explicit mappings and a new generation, never by
   downgrading schema or deleting records. A Git revert alone leaves the active graph unchanged
   until such activation. Old binaries can resume writing only after a separately supported
   downgrade/export path proves full representability; this proposal provides no destructive
   downgrade. Restoring a snapshot establishes a new execution epoch so prior requests/results
   cannot become current. Copy/export preserve definitions, events, attempts and evidence links.

The existing live-board/main-only migration and writer rules still apply. No live migration,
shared-service restart, consumer checkout edit or deployment is authorized by this design.

## Client and agent contract

Board columns group by configured delivery phase; state chips and filters show waiting,
blocked, failed, review and cancellation without moving the task to an unrelated phase.
The built-in graph retains today's seven-column and phone behavior. Build and Test continue as
recorded-process overlays even when a project has a delivery phase labelled Build.

The task page offers server next actions and shows the actor required for an unavailable one.
Drag, buttons and CLI input submit edge IDs with expected revisions; stale results prompt a
refresh. Agents, Agents-page summaries, staffing explanations and shipped role guidance read
the same contract. They cannot assume literal `ready`, `review` or `done` destinations. Saved
selections are keyed by project and stable IDs, with removed selections returning to a valid
view rather than writing a task. Keyboard and phone actions expose the same operations.

Timeline and reports use structured events containing both coordinates, workflow revision,
recorded labels, actor, operation and linked attempt/evidence/effect identities. Phase duration
and coordination-state duration are separate totals; run time is separate again. Legacy events
retain their original interpretation. Renaming a label or changing an edge never rewrites
earlier review rounds, failures or phase time.

## Implementation briefs

These are proposed briefs for the orchestrator, not newly filed tasks or authorization to
activate a graph. Land the additive shared contract before consumers use it, following the
shared-data rule. Then one implementation parent owns the remaining slices and its delivery
cycle. Project guards and execution attempts stay their separately owned prerequisites.

| Proposed brief | Scope and acceptance | Focused evidence |
| --- | --- | --- |
| `tools/workflow-contract` | Add the shared reader/types, immutable default definition and validation in project/board API modules. Expose additive workflow/next-action shapes beside existing APIs, preserve public Rust calls for two releases, and document every accepted setting. No custom activation yet. | Reader unit tests for invalid IDs, ambiguous routes, missing recovery, rework cycles and default semantics; API contract and consumer compile guards. |
| `tools/workflow-storage` | Add task position/revisions, definitions, events, activation and operation receipts to both stores. Copy/export/import preserve them; old writers fail at every transaction after the schema fence. Default backfill retains unknown provenance and legacy status meaning. | Paired store tests for defaults, blocked/cancelled provenance, approved versus landed, failed migration rollback, old running writer refusal, history-preserving copy/restore and strict old-row decoding. |
| `tools/workflow-admission` | Implement the authoritative service and route every mutator, including owner/API/Rust/CLI, claim, verdict, block/resume, subtask, covered close and completion through it. Add trusted principal input and opaque policy receipt checks without an artifact engine. | Unit/store/API/CLI tests of every operation class; unauthorized role/person, fresh-review exclusion, stale task/workflow/guard, duplicate keys, conflicting payloads, concurrent transitions/dependency/capacity races and guard unavailability, in both stores. |
| `tools/workflow-operations` | Replace status switches in PULL, staffing, slots, alarm, recovery/reaper and sweep with configured behavior. Consume fenced attempt observations and persist dispatch intents. Preserve local launch behavior and distinct task/attempt capacity. | Default parity fixtures plus claim/review handoff, lost holder in every staffed position, held block, retry exhaustion, cancel/result races, restart/duplicate dispatch and phase retention. Attempt integration waits for its durable contract. |
| `app/workflow-actions` | Consume phase/state/next actions in board, task, Agents, phone, filters, timeline and reports; update shipped work/review/orchestrator guidance and command help. Preserve legacy/default views and process overlays. | Client unit tests for invalidated IDs, stale actions and saved selections; focused browser flow for two review stages/rework and Build-blocked/Verify-review; phone/wide pictures, client build and lint. |
| `tools/workflow-activation` | Implement preview/explicit activation after its card is decided, paired-store mappings and schema/adapter fences. Integrate default/custom land prepare/finalize and capability checks; do not supply project criteria. | Scratch-store migration and rollback matrix, stale preview, in-flight attempt drain/invalidate, missing/removed IDs, crash after external effect, default/custom landing-adapter parity; reject custom activation without guards required by the profile. |

Every code slice records its closest tests and repository lint; a client slice builds the
client and leaves distribution regeneration to the project's existing process. Coverage review
must inspect missed effect/operation branches, especially refusal and race cases, rather than
count only the happy path. The final implementation parent demonstrates three isolated fixtures:
no workflow setting with full default command parity; a custom two-review/post-approval-delivery
graph; and the consuming project's profile supplied by that project with unchanged gate/landing
decisions. The Blueprint-to-Deploy fixture requires the guard design before claiming human gate
enforcement. The full release suite remains the release run's work.

This design step is checked with the documented document audit, status lint and owner-words
check at its final tip. No behavior, schema migration, client result, coverage result or code
test is claimed by the proposal itself.
