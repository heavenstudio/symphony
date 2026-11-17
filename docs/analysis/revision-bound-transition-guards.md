# Artifact approvals at workflow transitions

A project can require a person's approval before a particular delivery handoff. That approval
must identify what the person saw: the requirements, candidate, evidence and destination. A
later candidate cannot inherit it merely because the task is still in the same column.

This is the proposed artifact-guard extension to the
[configurable workflow contract](configurable-delivery-workflow.md#Admission and authority),
awaiting [its enforcement decision](../DECISIONS.md#revision-bound-project-guards). It defines
one authoritative admission boundary, not a check runner, risk engine or replacement stamp
catalogue. No API, approval store, human identity integration or enforcement is implemented by
this document. Implementation also waits for the workflow contract's activation choice.

## Source and ownership

The [delivery requirements](https://toptal-core.atlassian.net/wiki/spaces/UNS/pages/6597967921),
version 7, and [the profile task](https://toptal-core.atlassian.net/browse/UNS-1647) require four
separate human handoffs and invalidation after relevant input changes. The page's prose was
read; its embedded attachment images were not inspected. They establish the following gate
placements, not Symphony APIs, complete criteria, assigned people or deployment environments.

| Illustrative handoff | Approval scope supplied by the project |
| --- | --- |
| Blueprint to Architecture | Exact requirements and acceptance baseline, with its source references. |
| Build tests to staging candidate | Candidate code, locked test definition and matching build/review evidence. |
| Verification/demo to packaging | Exact staging candidate, environment and verification/release acceptance evidence. |
| Published package to provisioning | Immutable package identity and explicit deployment target. |

Other projects choose their own gates. Each gate is a distinct request; passing an agent turn,
answering a document card, sending a notification, or recording a generic reviewer verdict does
not answer it. One action cannot implicitly approve all four handoffs.

Inspection base: `c25e1448e3d8327a49e107834b3192d5a0cd9cf3`, including the landed workflow
proposal. These are existing seams, not evidence that this extension is already enforced:

| Seam | Present boundary and proposed use |
| --- | --- |
| `crates/tasks/src/stamps.rs`, `stamps/read.rs`, `land/needs.rs` | Four generic readers and per-tip assessments decide default landing evidence. Keep their meanings and readers. An assessment cannot waive a configured project guard. |
| `crates/symphony-plugin/src/lib.rs`, `Gates` | Typed project gate display, cached by status, branch tip and `kept_while`. Reuse presentation vocabulary; the answer carries no admission authority. |
| The same module, `BoardAction::submit` | Receives root, task and payload/note, without authenticated-person context. It cannot safely record these human approvals as it stands. |
| `crates/tasks/src/store/transaction.rs`, `schema.rs`, `store/postgres/schema.rs` | Guarded transactions and paired stores are the persistence seam. Atomic input/evidence comparisons and approval facts still need implementation. |
| [Workflow admission](configurable-delivery-workflow.md#Admission and authority) | Reserves project-policy receipts, expected revisions and typed effects. This proposal supplies artifact and approval semantics for that boundary. |
| [Durable attempts](remote-execution-attempts.md#Durable records and identities) | Proposed immutable attempt/input and result-manifest references join evidence to its producer. Attempt success grants no approval or transition. |

Under the recommended shared-record option below, the project owns criteria, applicability,
approver membership and any quorum or separation
requirements beyond existing reviewer independence. It also owns its artifact storage, checks,
test locks, environment descriptions and landing/deployment implementation. Symphony owns the
scoped immutable facts, trusted-principal admission, atomic comparisons, audit and refusal.
Project code evaluates its criteria against a supplied snapshot; it cannot manufacture a
human principal, bypass core scoping or mutate workflow state through its answer.

## Where approval authority lives

The unresolved choice is who owns the authoritative human decision record at a protected
handoff. The delivery requirements establish exact-artifact human decisions and refusal of
unauthorized advancement; they do not choose where those decisions are stored. The approved
[workflow direction](../DECISIONS.md#project-workflow-shape) chooses the configurable graph.
The [stamp decision](../DECISIONS.md#needed-stamps) preserves generic readers and project
landing ownership. Neither supplies or chooses this new approval authority.

| Option | Source basis and proposed boundary | Cost and tradeoff |
| --- | --- | --- |
| A: shared decision records | Extend the board's guarded, paired-store transactions with common immutable request/decision facts. Project code supplies criteria and membership, as the existing typed gate seam supplies project evidence; the board validates current human facts at admission. | Symphony implements and versions the record lifecycle, authenticated decision methods, audit and UI once. Projects adopt that model and its conservative fresh-approval rules; host authentication and membership integration are still needed. |
| B: project approval authority | Keep human request/decision facts in an approval ledger the project chooses or builds. Extend the project-policy seam beyond today's display-only `Gates` answer with authenticated decision operations and exact versioned attestations; the board stores admission receipts and authoritative generation bindings. | Projects keep their own approval semantics and storage, but implement the ledger, audit/history and a bounded freshness/revocation protocol. Protected admission depends on that service being available and compatible. The existing cache or custom landing command alone cannot provide it. |

Both options use the same mandatory admission path and exact project/task/gate/input/target
bindings. Neither adds a generic stamp kind or lets an assessment waive a gate. Both require
authenticated people, retained rejection/rework history, commit-time generation checks and
adapter checks before external effects. Missing, stale or unavailable authority refuses the
protected operation. A project with no guards retains its existing behavior.

For B, the project authority must publish a versioned decision/withdrawal observation into the
board's guarded generation discipline before that observation can authorize admission. If it
requires revocation effective across systems immediately, it must cooperate in an admission
barrier; a fetched signature or timestamp alone cannot make a later database commit atomic with
the external ledger. The last-validated-snapshot limit also applies to remote effects under A.
Neither option promises unreported remote revocation. These limits are eligibility conditions
for the project's adapter, not permission to fall back to an old pass.

Recommend A: it puts the shared approval lifecycle beside the transaction that must arbitrate
it and gives every protected route the same human-decision semantics. B remains a real
alternative for a project that needs its own approval authority and accepts its extra adapter
and availability obligations; the sources do not show an existing service already meeting them.
The remaining sections specify A in detail. Choosing B would retain these identity/admission
invariants while placing request/decision custody and its decision methods behind the project
authority adapter. The separate workflow activation and durable-byte-storage choices decide
neither option. This is a design choice, with no runtime comparison or implementation claimed.

## Identities and records

Use project/board identity plus immutable IDs throughout. A task slug, branch name, URL, package
tag or environment label alone is not an artifact identity. All names below describe proposed
data, not accepted settings or released Rust fields.

| Record | Minimum authoritative content |
| --- | --- |
| Artifact reference | Owning project/board, artifact ID, kind, immutable content digest with algorithm/format, producer/repository identity and immutable revision, provenance and evidence-store reference. A URL locates bytes; the digest identifies them. |
| Input manifest | Immutable ID/digest and schema version; delivery/task scope; named artifact slots; explicit upstream accepted-output references and dependency links. Each applicable slot is present; a project policy explicitly declares an inapplicable slot. Omission is unknown. |
| Current selection | Project/delivery/task scope, selected manifest and per-input revisions; monotonic selection revision. Historical manifests never change. The repository head and external target preconditions are independently checked where they matter. |
| Gate definition/binding | Project gate ID, immutable policy/criteria version, scope, protected workflow edge/effect classes, required input/evidence slots and approver capabilities. It belongs to the active committed project policy, never a branch-local field or task assessment. |
| Approval request | Unique instance/round ID; project/task/delivery scope; gate and policy version; workflow revision and activation generation; exact scoped manifest and evidence references; target identity where applicable; request revision and pending/superseded projection. |
| Decision fact | Request ID, approve/reject/withdraw result, stable authenticated person ID and identity issuer, authorization-policy/membership revision, server accepted time, optional policy expiry, evidence references and rationale. Append-only sequence and actor-bound operation key identify the fact. |
| Admission receipt | Project/task/edge and source revision; workflow revision/activation; policy/guard implementation version; exact input/evidence/dependency/authority generations; required approval fact IDs; result/reasons; evaluation time and bounded validity. It is an internal decision record, not a bearer permit. |
| Transition/effect history | Initiating principal and applying service, before/after revisions, receipt/fact references, refusal or committed result, rework relationship, durable external-effect intent and observed outcome. Retained IDs resolve after renames and policy changes. |

The project's input manifest identifies baseline/specification, repository/code commit, locked
test definition and lock revision, test/build results, environment/configuration revision,
package digest and target as applicable. Test evidence binds the code, test lock and environment
it actually exercised; changing the lock is a new input, not a way to make an old run green.
An environment reference includes its account/tenant, environment ID and relevant configuration
revision. A release target includes project, account/tenant, environment and destination/resource
identity. Credentials and expiring signed URLs are excluded from durable facts.

Registration of a selection or evidence fact requires its own scoped producer/curator capability
and expected revisions. The trusted artifact adapter verifies digest, provenance and collection
completeness before evidence is eligible; an agent posting a hash or mutable URL is only a claim.
Lost required bytes, integrity failure or unverifiable provenance makes the evidence unknown.
Preserve its reference and failure history rather than substituting other bytes under its ID.

A gate's manifest is its declared subset plus dependency closure, not every future artifact of
the delivery. Building code need not stale a requirements-only approval; changing requirements
does stale every dependent approval. Unknown relevance is conservative: require re-evaluation
and a new approval where an unchanged scope cannot be established. Equal bytes in another
project or target do not make decisions transferable. Shared source artifacts may be referenced
as inputs, but each project records its own acceptance and approvals.

## A person's decision

The host supplies an authenticated principal outside caller-controlled JSON. A human principal
has a stable person/issuer identity and current project capabilities. Agent and system/service
principals are separate types with narrowly assigned capabilities; neither can become a person
by submitting an agent handle, `owner`, email, role name or claimed identity in a payload.
An agent may prepare evidence and request a decision, subject to its task authority.

The trusted project identity adapter supplies versioned membership and authorization facts.
The board maintains the authoritative accepted authorization generation used for decisions;
unknown, expired or unavailable membership refuses a new decision and protected admission.
Record the person who decided separately from the server that saved it. Existing independent
review remains independently required where the workflow/landing policy requires it.

A person reads the immutable request, inputs, evidence and target, then explicitly submits a
decision with the expected request, selection, workflow and authorization revisions. The
service checks those bindings and permission in the committing transaction. A stale form cannot
approve the replacement candidate. Duplicate identical submissions return the original result;
replaying an actor-bound operation as another person, or with another payload, refuses. Separate
people deciding concurrently produce separate facts only while the same request is current. The project's versioned rule
decides how their decisions combine; Symphony verifies every referenced approving fact and
required capability, and never treats a rejection or missing quorum as pass.

Approval does not itself move the task or perform delivery. The next operation reads the
current decision set. Rejection and withdrawal invalidate pass immediately through a generation
change; a later approve requires an explicit new request/round. Older approval cannot reappear
when a newer rejection, expiry or supersession is removed from a display. No first-slice
carry-forward is supported: a changed bound input, policy or workflow activation requires a
fresh decision. Even reactivating identical workflow content uses a new generation.
Withdrawal requires the original person or an explicitly authorized project revoker and records
that actor; automatic invalidation is a separate system fact, never a fabricated human decision.

The existing local owner transport retains its present authority for existing operations. To
answer a new person gate it must also be bound by the trusted host to a real stable person;
loopback access or an owner-labelled message is insufficient. Remote human approval routes
stay unavailable until authenticated actor propagation and project membership exist. This
contract supplies no login system, directory integration or direct approval from Slack.

## Invalidation and dependencies

All selection, decision, evidence, accepted-output and authority changes pass through the same
versioned service. In the transaction that changes one, advance a project guard generation and
its affected record revisions. Admission compares that generation as well as the exact read
set, preventing a dependency added during evaluation from escaping the comparison. Unrelated
changes can force a fresh evaluation without invalidating an unaffected human approval.

| Change | Required consequence |
| --- | --- |
| Baseline/specification or acceptance/test-lock change | Supersede affected gate requests and matching downstream evidence; require new runs/decisions on the replacement inputs according to project criteria. |
| Code/candidate change | Stale code-dependent test/review, verification, package and target approvals; a baseline-only decision remains historical and may still apply to its unchanged baseline. |
| Environment, evidence pack, package or target change | Invalidate all requests whose declared scope or dependency closure includes that identity. A package tag moving without a new registered digest is unknown and refuses use. |
| Rejection, withdrawal, expired validity or lost approver authority | No current pass. Preserve the decision and authorization-at-decision history; require a current authorized decision round. |
| Gate criteria, required membership rule, workflow revision or activation change | New policy/request identities; no implicit approval migration. Existing workflow mappings do not create evidence. |
| Upstream accepted output superseded or invalidated | Dependent admissions and result promotion refuse until they accept a current replacement. Record affected work and its required rework route. |

Dependency links reference exact accepted output IDs, not merely `done` or a predecessor's phase.
For tasks on the same board, admission resolves the current dependency closure and input
selections under the committing transaction's conflict/locking discipline. Both stores must
arbitrate changes to shared predecessor records. Fan-out notifications and materialized stale
badges may lag; permission must not depend on their arrival. Missing dependency provenance,
cycles without a resolvable accepted-input closure or unverified output identity refuses pass.

Already running attempts retain their original inputs and outputs, but invalidation bars their
promotion as current evidence. Follow the configured cancel/stay/rework operation; a reaper
cannot accept old results to escape the block. Already delivered work keeps its actual outcome
and gets a visible affected-delivery/recovery record. Reopening creates a new request/round and
links the failure; it never rewrites the delivered history or silently rolls back a deployment.

For an external source, the project must register an immutable, verified snapshot and explicitly
define whether it consumes that pinned snapshot or requires a current upstream selection.
The latter requires an authoritative version/freshness observation admitted into the board;
lost contact is unknown. Board transactions cannot promise instantaneous revocation in another
system. A project requiring that stronger guarantee cannot activate the relevant edge without
an adapter that can enforce it at the external boundary.

## One admission path

The new guard seam is an additive, versioned project-policy call beside `Gates`, with explicit
capability negotiation. It accepts the immutable operation snapshot and scoped principal
context; it returns typed pass/refusal/unknown, required decision references and reasons, bound
to all revisions it read. Artifact storage stays with the project. This is bounded evaluation
of existing evidence, with no arbitrary command launcher and no irreversible side effect.

1. Resolve the active workflow/guard bindings from committed authoritative policy. Check the
   trusted actor's task relationship, operation capability and expected source/workflow/input
   revisions. Read candidate inputs, dependencies, evidence, approval rounds and authorization
   generations. Required guards are conjunctive with existing board and stamp predicates.
2. Evaluate project criteria outside the database transaction, with a deadline. A missing
   required adapter, absent/pending/rejected/outdated/unknown evidence, error, panic, timeout or
   unsupported contract refuses the operation. Do not commit position, role, slot, outcome or
   effect changes; retain an auditable refusal with reason and relevant revisions.
3. In one guarded transaction compare the project guard generation, activation, task source,
   selected manifest, all evidence/dependency/current-round revisions and authorization facts.
   Verify the receipt's exact scope and every required approving human fact, its current
   eligibility and validity. Reevaluate board predicates and the existing generic stamp
   requirements where applicable. Then commit the transition, audit, idempotency result and
   durable effect intent together. A read-only snapshot alone is insufficient: concurrent
   invalidation and admission must conflict or serialize on the authoritative records.
4. On a race return stale with the current accessible blockers; do not silently reevaluate and
   advance a new candidate. An operation key is scoped to actor/project/task/operation and its
   immutable payload. A successful retry returns the original transition, not another move.
   Recovery retries use its saved fencing identity and expected revisions.

For authorization outside the database, the trusted adapter must publish a versioned fact with
an explicit freshness/expiry contract before evaluation. Commit checks the board's accepted
generation and server clock. This establishes admission against that fact, not atomic visibility
of an unreported directory revocation. Strict live authorization requires a cooperating authority
or refuses activation. A plugin cache or self-reported timestamp supplies neither guarantee.

The service is also the store/library enforcement boundary. Caller-supplied raw status, task
fields, a successful plugin button, an assessor's empty stamp list or an unguarded old Rust
method cannot write a guarded position. Schema/capability fencing rejects every old writer,
including one already open. Import, restore and maintenance must preserve invariants and audit;
restoring a snapshot establishes a new activation/execution epoch and makes outstanding
requests/receipts stale. Operator repair is a recorded privileged recovery, never an approval.

| Route | Required enforcement |
| --- | --- |
| UI drag/button, keyboard/phone, CLI/API and linked Rust caller | Submit the same typed operation with expected revisions. Read/next-action previews confer no permission. |
| Agent/skill, scheduler, staffing and reaper | Use constrained agent/system principals and the same admission methods. Lost-holder or timeout recovery cannot invent human approval. A cancellation/rework edge may remain available because it grants no protected handoff. |
| Attempt callback/result promotion | Check attempt/input/current-attempt and lease/runtime fencing as well as applicable project guards. Process success cannot complete a delivery. |
| Subtask integration and parent completion | Name exact integration commit/accepted outputs; check bindings for the protected parent handoff. No child `done` or parallel child result silently accepts a changed parent baseline. |
| Covered close, dependencies and sweep | Require the actual accepted/landed output and applicable guards. A terminal-looking label or cancelled dependency cannot fabricate acceptance. Preserve evidence needed by retained history. |
| Default land, custom land and deployment adapter | Prepare/finalize through admission, while retaining their own existing evidence and execution authority. An adapter unable to enforce a protected effect cannot activate that edge. |

Bindings cover protected effect classes as well as named edges: a graph cannot add a second
unguarded `advance`, recovery, creation or completion route to obtain the same acceptance/effect.
Activation validates this coverage, referenced gates and required adapter capabilities.
An intentional project-policy change is a new authoritative policy activation with audit and
new requests, never a per-task generic assessment waiver.

## The external effect boundary

Board admission is atomic only with board facts. For landing/deployment, prepare records a
reservation binding project/task, operation key, source repository/head, policy/workflow,
input/evidence/approval generations, immutable package and exact destination. It is not a
standing delivery permit. Revalidate immediately before dispatching the durable keyed effect
intent, and verify the actual head/package/target preconditions on the external request itself.
Record intent before I/O. A moved head, replaced package, changed target or known invalidation
before dispatch refuses the effect; local adapters use their repository mutation lock and remote
adapters need equivalent conditional head/target support for the guarantee they advertise.

The smallest contract promises authorization at the last validated dispatch snapshot, not atomic
revocation across the board and another system. Withdrawal after sending a request cannot
recall it. If a project requires approval to remain revocable until the remote effect commits,
its adapter needs a cooperating conditional authorization/sealing protocol; unsupported means
the edge cannot activate. A lease, cache TTL or check followed by unconditional I/O does not
provide that stronger guarantee. External merge authority and provenance remain the landing
adapter's responsibility, not this project's new guard reader.

After a crash before dispatch, revalidate the saved intent before sending. After an uncertain
send or a crash after effect but before finalization, reconcile by operation/provenance identity.
Do not send a fresh merge or deployment merely because the board has no receipt. Unknown outcome
stays pending reconciliation. A definitive receipt records the actual effect against the
snapshot that authorized it. If inputs/policy changed in the gap, retain the effect and record
recovery required; never mark the new approvals satisfied. Failed deployment is a retained
failed attempt/effect with deliberate retry or rollback, each newly admitted where required.

## Display and compatibility

The gate panel reuses typed gate status and evidence links, with additive request/fact/input
identities and accessible actions. Phone and wide views show which person capability is needed,
the exact candidate/target, pending/rejected/outdated/unknown reason and reapproval/rework link.
The person sees what changed before deciding. Unauthorized viewers receive only permitted
artifact and membership detail. CLI/API report the same stable refusal codes and current
accessible blockers. A notification links to this request; its delivery is not a decision fact.

`Gates::answer` and `kept_while` can continue to cache display answers. They cannot issue a human
decision, supply a trusted actor, or reuse a `met` result as admission. A receipt from an earlier
read is also insufficient: only the admission service can validate its complete bindings in the
committing transaction. Error/timeout stays unknown regardless of the last displayed pass.

Projects with no configured artifact guards keep existing commands, stamp selection and landing
behavior. The four generic stamp readers are unchanged; no new stamp kind or check runner is
introduced. The proposal changes authority only for explicitly configured protected operations.
A project's custom land keeps deciding its own evidence and performing its own effect; it must
adopt the prepare/finalize and effect checks before those operations activate. An unrelated
external command run outside Symphony's authority cannot be prevented by a board check; the
project must constrain its merge/deploy credentials and routes to its enforcing adapter.

Ship new public calls beside working old APIs for the consumer two-release window. Do not add
authenticated-person claims to caller-controlled `Submission` or repurpose legacy owner/review
stamps. A negotiated new kit/protocol seam carries trusted context and guard references;
unsupported versions refuse protected actions. Persist additive board-owned records in both
stores, preserve strict old-row decoding and copy/export history, and fence incompatible writers
before guard activation. No live migration, consuming-profile edit or cloud integration is
authorized by landing this analysis.

## Implementation and evidence

After the enforcement and workflow activation decisions, land the additive shared contract
before its consumers. One implementation parent can then carry the store, admission/routes and
client slices, coordinating with workflow and attempt implementations. Project criteria and
authenticated host integration are prerequisites to a real project activation, not guessed
defaults in Symphony.

| Slice | Required focused proof |
| --- | --- |
| Contract/identity | Canonical manifest and digest tests, explicit inapplicable/missing slots, project/task/gate/round/target isolation, locked-test bindings, protocol capability/error cases and consumer compile guards. |
| Approval/store | Fake authenticated people, agents and services; forged identities refused; current/removed/unknown membership, quorum/rejection/withdrawal/expiry, new round and duplicate/conflicting key tests. Both SurrealDB and PostgreSQL preserve facts and refuse unauthorized writes. |
| Invalidation/admission | Concurrent baseline versus decision and transition, dependency update/addition, parallel child completion, policy activation, input ABA, double transition and approval revocation; no partial writes. Verify unaffected scope survives while affected downstream approvals stale immediately for admission. |
| Every route/effect | UI/API/CLI/Rust, scheduler/reaper, result callbacks, subtask/covered/parent closes, import/restore and old-writer bypass attempts. Missing/error/panic/timeout/cached pass refuses. Default/project landing parity, head/package/target movement, crash before send/after effect, duplicate reconciliation and failed deploy/retry fixtures. |
| Client/project integration | Phone/wide pending, rejected, outdated, unknown and changed-target gate views; a person approves the exact request, stale form refuses, agent cannot approve. Client build/lint, screenshots and isolated four-handoff fixture with fake membership and guards, plus unchanged default-project behavior. |

Run each slice's closest unit/store/contract/API/CLI tests and repository lint. Review coverage
for refusal, race, recovery and authorization branches, including tests that would catch a
missing commit-time comparison. End-to-end evidence covers the ordinary handoff/rework path;
cheap edge cases belong in units and paired-store fixtures. The full suite is the release run's.
This document's evidence is the current docs audit/status lint and owner-words check at its
final tip. It claims no implementation, passing runtime tests, coverage or phone UI result.
