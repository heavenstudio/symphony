# Artifact approvals at workflow transitions

A project can require a person's approval before a particular delivery handoff. That approval
must identify what the person saw: the requirements, candidate, evidence and destination. A
later candidate cannot inherit it merely because the task is still in the same column.

This is the design contract for the artifact-guard extension to the
[configurable workflow contract](configurable-delivery-workflow.md#Admission and authority),
using the project-owned approval authority selected on
[its enforcement card](#revision-bound-project-guards). Symphony supplies atomic
admission and refusal; the project supplies authoritative human approval records and exact,
versioned attestations. This is not a check runner, risk engine or replacement stamp catalogue.
No API, ledger adapter, human identity integration or enforcement is implemented by this document.

The [activation choice](configurable-delivery-workflow.md#configurable-workflow-activation) selects automatic
activation of a valid committed workflow, with complete mappings and prerequisite checks.
The [ingress choice](../../operations/analysis/hosted-ingress.md#hosted-ingress-boundary) selects the server adapter for
trusted visitor/project context. Those directions are decided; their implementations and the
project's approval-authority integration must exist before protected operations can activate.

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
| `crates/tasks/src/store/transaction.rs`, `schema.rs`, `store/postgres/schema.rs` | Guarded transactions and paired stores are the persistence seam. Atomic input/evidence/authority-generation comparisons and admission receipts still need implementation; human approval records remain project-owned. |
| [Workflow admission](configurable-delivery-workflow.md#Admission and authority) | Reserves project-policy receipts, expected revisions and typed effects. This proposal supplies artifact and approval semantics for that boundary. |
| [Durable attempts](../../engineering/analysis/remote-execution-attempts.md#Durable records and identities) | Proposed immutable attempt/input and result-manifest references join evidence to its producer. Attempt success grants no approval or transition. |

The project owns its authoritative approval ledger, decision methods, criteria, applicability,
approver membership and any quorum or separation requirements beyond existing reviewer
independence. It also owns artifact storage, checks, test locks, environment descriptions and
landing/deployment implementation. Symphony owns the board's input selections, accepted authority
generation bindings, admission receipts, transition/effect audit and atomic refusal. Project
code evaluates criteria and attests ledger facts against a supplied snapshot; it cannot turn an
agent into an authenticated person, bypass core scoping or mutate workflow state through its answer.

## Where approval authority lives

The project chooses or builds the authoritative human approval ledger. Its adapter provides
authenticated request/decision operations, retained decision history and exact versioned,
revocation-aware attestations. Symphony checks those attestations through one mandatory
admission path with project/task/gate/input/target bindings. It does not own a common approval
record lifecycle or reconstruct the project's ledger from its audit receipts.

| Project authority | Symphony admission |
| --- | --- |
| Request/round creation, human decisions and withdrawal, authenticated membership and quorum, immutable facts and their retention | Trusted adapter/issuer configuration, expected input/workflow bindings, monotonic authority observations, committing transition predicates and admission/refusal audit |
| Current ledger generation and durable publication/reconciliation of decision or revocation observations | Atomic comparison of the accepted authority generation and board read set in both stores; refusing missing, stale, rejected, revoked or unavailable authority |
| Authenticated evidence of who approved which exact request, with its result, time, policy and evidence references | Minimal attestation snapshot/digest, fact references, verified scope and initiating/applying principal needed to audit the admitted operation |

The configured authority identity, adapter version and trust material come from the active
committed project policy, never a task payload or caller-supplied issuer. A signature establishes
provenance, not current authorization: an older signed pass cannot override a newer withdrawal,
authority epoch or policy. The project bears the ledger's audit, availability and compatibility
obligations. Missing capability keeps protected operations unavailable; it does not make them
advisory or fall back to a cached pass.

The project authority publishes versioned decision/withdrawal observations into the board's
guarded generation discipline before an attestation at that generation can authorize admission.
Immediate revocation across systems requires a cooperating admission barrier. A fetched
signature or timestamp alone cannot make a later database commit atomic with an external ledger.
The [external-effect boundary](#The external effect boundary) therefore retains its truthful
last-validated-dispatch limit. These are adapter eligibility conditions, not permission to
ignore known stale authority.

The [workflow direction](project-workflow.md#project-workflow-shape) still chooses the graph and the
[stamp decision](needed-stamps.md#needed-stamps) still preserves generic readers and project
landing ownership. No stamp kind or assessment waiver is added. Projects without guards retain
their existing behavior. Historical alternatives and the approval-record ownership decision
remain on [the enforcement card](#revision-bound-project-guards).

- [r] **revision-bound-project-guards** — A project could require a person's approval of exact requirements, a candidate or a package and destination before a delivery handoff. Where should the authoritative approval record live? A: Symphony stores the common human request/decision records; the project supplies criteria and gate membership. B: each project owns its approval ledger and supplies exact, versioned attestations to Symphony's transition service. Both check current inputs and authorized people at admission and before an adapter sends a merge or deployment. Recommended: A. Approve and say A or B, or comment. [[analysis/revision-bound-transition-guards#Where approval authority lives]]
  [2026-11-17 20:14] agent — Proposed for `design/revision-bound-transition-guards`. This is design only: no runtime enforcement, human identity service, consumer edit or live migration is claimed or authorized here.

    **A:** Symphony implements the record lifecycle, authenticated decision methods and audit once, beside its paired-store admission checks. Projects adopt those common facts and fresh-approval rules. **B:** projects choose or build their own ledger and preserve its decision semantics, but must supply bounded availability, history and versioned revocation/freshness integration. Today's cached Gates answer and custom landing command are not that contract. Both preserve existing stamps, project landing ownership and unchanged projects with no guards.

    **Shared limit.** A changed bound input, policy or workflow activation requires a fresh approval; the recommended contract has no carry-forward. External effects are authorized at their last validated dispatch snapshot: a later withdrawal cannot recall an already sent request. A project requiring stronger remote revocation needs a cooperating adapter before that edge can activate. A supplies no human login or membership service by itself; B cannot authorize advancement from an unavailable or outdated ledger.

    **Earlier verdicts checked.** **project-workflow-shape** approved C and its unchanged defaults; it does not decide the artifact/approval authority or external-effect guarantee. **needed-stamps** keeps its four generic readers and project replacement policy: this extension adds no stamp kind, check runner or assessor waiver. **configurable-workflow-activation** remains a separate pending choice about when a graph becomes active. **hosted-ingress-boundary** also remains pending; its visitor context alone would not establish project gate membership. None approves this concrete guard/principal/record contract. Gate placements in the linked delivery requirements do not assign project approvers or deployment targets.
  [2026-11-17 20:28] agent — Clarified the source-backed alternatives: the delivery requirements prescribe human decisions on exact artifacts, while the existing gate/landing seams retain project evidence policy; neither chooses approval-record custody. The analysis details A and B's required authority adapter. This asks only the new guard policy, separately from workflow activation and durable byte storage.
  [2026-11-17 20:44] owner · approved — B
  [2026-11-17 20:54] agent — Your “B” selects project-owned approval ledgers supplying exact, versioned attestations to Symphony. Admission must refuse unavailable, stale or revoked authority and recheck current inputs and authorized people before dispatch. Symphony will not own the human approval ledger; the analysis and implementation handoff will be aligned to B while preserving the existing stamps and project landing policy.
  [2026-11-17 20:54] agent · resolved

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
| Approval request | Project-ledger-owned unique instance/round ID; project/task/delivery scope; gate and policy version; workflow revision and activation generation; exact scoped manifest and evidence references; target identity where applicable; request revision and pending/superseded projection. |
| Decision fact | Project-ledger-owned request ID, approve/reject/withdraw result, stable authenticated person ID and identity issuer, authorization-policy/membership revision, authority accepted time, optional policy expiry, evidence references and rationale. Append-only sequence and actor-bound operation key identify the fact. |
| Authority attestation | Configured authority/adapter identity and contract version; project/task/gate/request scope; workflow revision/activation; exact manifest/evidence/dependency/target bindings; ledger epoch and monotonic decision/revocation generation; human fact references/digests, authorized person/capability and current aggregate result; issued time, validity bound and authenticated integrity proof. |
| Admission receipt | Board-owned project/task/edge and source revision; workflow revision/activation; policy/guard implementation version; exact input/evidence/dependency/accepted-authority generations; attestation snapshot/digest and project-ledger fact references; result/reasons and board accepted time. It is an internal admission audit record, not a bearer permit or human approval ledger. |
| Transition/effect history | Initiating principal and applying service, before/after revisions, receipt/fact references, refusal or committed result, rework relationship, durable external-effect intent and observed outcome. Retained IDs resolve after renames and policy changes. |

Approval requests and decision facts stay in the project ledger; their fields above define the
proof the adapter must be able to supply, not new Symphony-owned approval tables. Board audit
retains the minimal attestation material and references needed to explain an operation. Complete
human decision history, authoritative current result and its restore remain the project's duty.

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

The trusted project identity adapter supplies versioned membership and authorization facts to
the project's approval authority. That authority validates permission for the human decision;
Symphony validates its current attestation and accepted generation for a protected transition.
Unknown, expired or unavailable membership refuses a new decision and protected admission.
Record the person who decided separately from the service that saved it and the service that
applied the transition. Existing independent review remains independently required where the
workflow/landing policy requires it. The server ingress adapter supplies trusted visitor context;
it does not itself assign gate membership or manufacture a project approval.

A person reads the immutable request, inputs, evidence and target, then explicitly submits a
decision with the expected request, selection, workflow and authorization revisions. The
project authority checks permission and expected ledger revisions in its own committing
transaction. The decision remains bound to that exact board/input snapshot. A stale form cannot
approve a replacement candidate: known stale inputs refuse at decision time, and a race with a
board change leaves at most a historical decision whose attestation fails current admission.
Duplicate identical submissions return the original result; replaying an actor-bound operation
as another person, or with another payload, refuses. Separate
people deciding concurrently produce separate facts only while the same ledger request is
current. The project's versioned rule decides how their decisions combine. Its authenticated
attestation identifies the current decision set and satisfied required capabilities; Symphony
checks that proof's scope, provenance, revisions and required bindings. A rejection, missing
quorum or unverified approving fact cannot become pass through an adapter's status label.

Approval does not itself move the task or perform delivery. The next operation obtains a fresh
attestation of the current decision set. Rejection and withdrawal advance the project's ledger
generation and invalidate its positive attestation; their accepted board observation fences
admission, as below. A later approve requires an explicit new request/round. Older approval
cannot reappear when a newer rejection, expiry or supersession is removed from a display. No first-slice
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

Board-owned selection, dependency-link, evidence, accepted-output and policy changes pass
through the versioned board service. In the transaction that changes one, advance a project
guard generation and its affected record revisions. Project-ledger changes instead use the
authority protocol below; they are not writes to a common Symphony approval table. Admission
compares the board guard generation, accepted authority generation and exact read set,
preventing a dependency added during evaluation from escaping the comparison. Unrelated changes
can force a fresh evaluation without invalidating an unaffected human approval.

The project authority atomically saves a decision/withdrawal, a new ledger generation and
durable observation intent in its own ledger. The authenticated observation names project,
authority epoch, expected previous generation and current decision-state digest. Symphony
accepts it with compare-and-set, advances its guard fence and records an idempotent acknowledgement.
Out-of-order observations cannot lower a generation; conflicting digests at one generation or
an unknown epoch make authority unknown until reconciled. A ledger restore needs a new authority
epoch and explicit reconciliation, not reuse of an old pass.

A positive attestation must name the acknowledged generation and match a fresh evaluation of
the authority's current state. An unacknowledged newer generation, lost acknowledgement or
unavailable authority requires reconciliation and re-evaluation before new admission. A crash
retries the same observation key; it does not regenerate the human decision. Revocation and
expiry known to either side cannot be hidden by cached attestation data.

These stores do not share a transaction. An observation that has not reached Symphony cannot
atomically revoke a transition it already admitted on an earlier validated snapshot. A project
requiring immediate cross-system revocation must supply a cooperative admission barrier that
fences outstanding observations/receipts before the change becomes effective for admission;
otherwise that stronger policy cannot activate. Receipt validity bounds are required but are
not a substitute for this barrier. Final effect dispatch always revalidates current authority.

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

The new guard seam is an additive, versioned project-authority call beside `Gates`, with explicit
capability negotiation. It accepts the immutable operation snapshot and scoped principal
context; it returns an authenticated typed attestation with pass/refusal/unknown, required
ledger decision references and reasons, bound to all revisions it read. Request/decision writes
use the project's authenticated authority methods; evidence and ledger custody stay with the
project. This is bounded evaluation of existing evidence, with no arbitrary command launcher
and no irreversible side effect.

1. Resolve the active workflow/guard bindings from committed authoritative policy. Check the
   trusted actor's task relationship, operation capability and expected source/workflow/input
   revisions. Read candidate inputs, dependencies, evidence and accepted authority generations;
   the adapter resolves current project-ledger rounds and authorization facts. Required guards
   are conjunctive with existing board and stamp predicates.
2. Evaluate project criteria outside the database transaction, with a deadline. A missing
   required adapter, absent/pending/rejected/outdated/unknown evidence, error, panic, timeout or
   unsupported contract refuses the operation. Do not commit position, role, slot, outcome or
   effect changes; retain an auditable refusal with reason and relevant revisions.
3. In one guarded transaction compare the project guard generation, activation, task source,
   selected manifest, all evidence/dependency revisions and accepted authority epoch/generation.
   Verify attestation integrity from the configured issuer, exact scope, acknowledged current
   ledger/round and authorization bindings, required approving fact references, eligible human
   capabilities and validity. Do not query or mutate the external ledger in this transaction.
   Reevaluate board predicates and the existing generic stamp requirements where applicable.
   Then commit the transition, audit, idempotency result and
   durable effect intent together. A read-only snapshot alone is insufficient: concurrent
   invalidation and admission must conflict or serialize on the authoritative records.
4. On a race return stale with the current accessible blockers; do not silently reevaluate and
   advance a new candidate. An operation key is scoped to actor/project/task/operation and its
   immutable payload. A successful retry returns the original transition, not another move.
   Recovery retries use its saved fencing identity and expected revisions.

For the external approval ledger and authorization facts, commit checks the board's accepted
authority generation, the attestation's explicit freshness/expiry contract and server clock.
This establishes admission against the attested snapshot, not atomic visibility of an unreported
ledger or directory revocation. Strict live authorization requires a cooperating authority or
refuses activation. A plugin cache, signature alone or self-reported timestamp supplies neither
guarantee. Symphony audits the evidence it admitted without taking over ledger custody.

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
Human actions are delegated to the configured project authority with trusted actor context and
expected request/input revisions; Symphony does not save them as its own approval records.
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
unsupported versions refuse protected actions. Persist additive board-owned authority
observations, admission receipts and effect/audit records in both stores, preserving strict
old-row decoding and copy/export history. Project approval-ledger backup/restore remains its
authority's duty; a board restore cannot reinstate old approvals or grants. Reconcile the
authority epoch/generation and fence incompatible writers before guarded operation resumes.
No live migration, consuming-profile edit or cloud integration is authorized by landing this
analysis.

## Implementation and evidence

The enforcement, automatic workflow activation and server-ingress adapter directions are
decided. Land the additive attestation/admission contract before its consumers. One
implementation parent can then carry board authority-fence/receipt storage, admission/routes
and delegated client actions, coordinating with workflow and attempt implementations. The
project supplies its approval-ledger adapter and authenticated decision methods. Project
criteria, versioned revocation/freshness support and trusted host membership integration are
prerequisites to a real project activation, not guessed defaults in Symphony. Automatic
activation still refuses an incompatible definition or missing adapter/mapping prerequisite;
it does not manufacture approval or carry an old request into a new activation generation.

| Slice | Required focused proof |
| --- | --- |
| Contract/identity | Canonical manifest and digest tests, explicit inapplicable/missing slots, project/task/gate/round/target isolation, locked-test bindings, attestation issuer/integrity and epoch/generation checks, protocol capability/error cases and consumer compile guards. |
| Project authority/board storage | Fake project ledgers and authenticated people, agents and services; forged identities/issuers refused; current/removed/unknown membership, quorum/rejection/withdrawal/expiry, new round and duplicate/conflicting keys. Both SurrealDB and PostgreSQL preserve accepted authority fences, attestation receipts and audit without creating a common approval ledger; project adapter contract tests prove its fact/history custody. |
| Invalidation/admission | Concurrent baseline versus decision and transition, dependency update/addition, parallel child completion, policy activation, input ABA, double transition and approval revocation; no partial writes. Verify unaffected scope survives while affected downstream approvals stale immediately for admission. |
| Every route/effect | UI/API/CLI/Rust, scheduler/reaper, result callbacks, subtask/covered/parent closes, import/restore and old-writer bypass attempts. Missing/error/panic/timeout/cached pass refuses. Test out-of-order/replayed attestations, lost observation acknowledgements, ledger/board restore epochs and withdrawal during evaluation/dispatch. Default/project landing parity, head/package/target movement, crash before send/after effect, duplicate reconciliation and failed deploy/retry fixtures. |
| Client/project integration | Phone/wide pending, rejected, outdated, unknown and changed-target gate views; a person approves the exact request, stale form refuses, agent cannot approve. Client build/lint, screenshots and isolated four-handoff fixture with fake membership and guards, plus unchanged default-project behavior. |

Run each slice's closest unit/store/contract/API/CLI tests and repository lint. Review coverage
for refusal, race, recovery and authorization branches, including tests that would catch a
missing commit-time comparison. End-to-end evidence covers the ordinary handoff/rework path;
cheap edge cases belong in units and paired-store fixtures. The full suite is the release run's.
This document's evidence is the current docs audit/status lint and owner-words check at its
final tip. It claims no implementation, passing runtime tests, coverage or phone UI result.
