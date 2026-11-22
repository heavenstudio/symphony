# Execution providers and durable attempts

A consuming project needs to run a task on infrastructure of its own, without a developer's
terminal or CLI. Symphony should own the durable record of each execution attempt and the rules
for accepting its results. The project should own the provider adapter, credentials, runtime
identity and artifact storage. The local hc launcher remains a valid execution provider.

This is a proposal, awaiting [its decision card](#execution-provider-attempts).
The operations, records and capabilities below are proposed, not existing APIs or settings.
Approval of this document's landing does not approve implementation, a paid session, deployment,
shared-service restart, consumer edit or live-board migration.

## Source boundary

Inspection base: **`3b460adb62827f674669e8e99f3090561107264f`**. The following links name
inspected source in this repository; line numbers and symbols refer to that revision. No vendor
API behavior is assumed or verified. A project's adapter must establish the guarantees its
provider can actually make.

| Concern | Inspected source | Supported at the inspection base |
| --- | --- | --- |
| Local execution | [launch/mod.rs](../../crates/tasks/src/launch/mod.rs), `launch` at line 266; [startup.rs](../../crates/tasks/src/launch/startup.rs), `first_message`; [staff.rs](../../crates/tasks/src/launch/staff.rs), `lock` | Registers an agent, takes its role, creates or admits a local worktree, starts an hc terminal and confirms the initial message before recording a successful launch. Staffing uses a local file lock. |
| Local recovery and controls | [restore.rs](../../crates/tasks/src/launch/restore.rs), `restore` at line 16; [go.rs](../../crates/tasks/src/launch/go.rs), `focus` at line 53 and `screen` at line 70 | Restores a recorded conversation into a local terminal. Focus and screen reads explicitly address hc. These are not remote execution controls. |
| Launch history | [store/launches.rs](../../crates/tasks/src/store/launches.rs), `NewLaunch` at line 17 and `record_launch` at line 124; [rows.rs](../../crates/tasks/src/rows.rs), `TaskSessionRow` at line 275; [PostgreSQL statements](../../crates/tasks/src/store/postgres/launches.rs), `KEY` and `put` | One mutable `task_session` record per task and handle. Relaunch replaces it; restore changes its pane. `claude_session` carries a Claude conversation or a `codex:` thread identifier, separately from the pane. No immutable execution-attempt history. |
| Existing fencing | [store/launches.rs](../../crates/tasks/src/store/launches.rs), `take_session` at line 334 and `renew_session` at line 357; [SurrealDB statements](../../crates/tasks/src/store/surreal/launches.rs), `HOLDS`; [PostgreSQL statements](../../crates/tasks/src/store/postgres/launches.rs), `HOLDS` | Close and restore hold a revisioned session lease and fence record updates against takeover. This supplies a pattern, not an execution lease. |
| Delivery | [store/callbacks.rs](../../crates/tasks/src/store/callbacks.rs), module contract and `Event`; [store/dispatcher.rs](../../crates/tasks/src/store/dispatcher.rs), `dispatch_detached` at line 87 | Durable notification outbox, recipient and host leases, acknowledgement and retry. A dispatch pass delivers board messages to registered recipients; it does not provision or reconcile task resources. |
| Lease clock and scope | [store/mod.rs](../../crates/tasks/src/store/mod.rs), `Store::now` at line 303 and `now` at line 1268; [dispatcher statements](../../crates/tasks/src/store/surreal/dispatcher.rs), `TAKE`; [PostgreSQL twin](../../crates/tasks/src/store/postgres/dispatcher.rs), `take` | Dispatcher lease is keyed by host and compares the caller-supplied store clock, normally process `SystemTime`. It does not supply a database-clock lease or a cross-replica execution-capacity reservation. |
| Database boundary | [store/database.rs](../../crates/tasks/src/store/database.rs), `Database` at line 13; [store/transaction.rs](../../crates/tasks/src/store/transaction.rs), `run_transaction`; [schema.rs](../../crates/tasks/src/schema.rs), `task_session` definitions | SurrealDB and PostgreSQL statement paths, guarded transactions and additive migrations already exist. PostgreSQL refuses statements without a twin; this is not proof that every board operation is already ported. |
| Plugin transport | [plugin traits](../../crates/symphony-plugin/src/lib.rs), `Transport` at line 530; [protocol calls](../../crates/symphony-plugin/src/protocol.rs), `RegisterAgent`, `Deliver`, `Alive` | Register, deliver and probe an agent address. No provisioning, attempt lifecycle or result collection contract. |
| Process plugins | [plugins/process.rs](../../crates/symphony/src/plugins/process.rs), `WAIT` at line 73 and `unanswered` at line 342; [process seams](../../crates/symphony/src/plugins/process_seams.rs) | Ten-second RPC deadline, cancellation and process restart. Routes and board actions have process proxies; a declaration containing transports is refused, even though transport call types exist in the protocol. |
| Human context | [plugin traits](../../crates/symphony-plugin/src/lib.rs), `Route` at line 333, `Submission` at line 359 and `BoardAction` at line 373; [protocol calls](../../crates/symphony-plugin/src/protocol.rs), `Serve` at line 542, `Offer` at line 548 and `Submit` at line 554 | Route gets root and rest; action gets root, task and submission payload/note. Neither receives an authenticated human principal. An agent handle or a payload saying “owner” cannot establish one. |
| Session links | [session_url.rs](../../crates/tasks/src/session_url.rs); [store/session_urls.rs](../../crates/tasks/src/store/session_urls.rs), setters/readers at lines 17/29; [wire agents](../../crates/symphony-api/src/agents.rs), `resolve_sessions` at line 231 | Validated HTTP(S) address on a live handle, stored beside the session in `callback_state` and removed on leave. The agents view resolves URL/pane and parent fallback. This is display support, not historical links or access authorization. |

The operating references are [the board](../TASKS.md), [plugin RPC](../PLUGIN_PROTOCOL.md),
[consumer API compatibility](../CHECKS.md#consumer-api-changes) and
[the board's invariants](../../crates/tasks/AGENTS.md#invariants-a-change-keeps).

## The smallest boundary

Use one execution coordinator in the board library, called by an explicitly configured host or
project worker. It admits work, commits intent, leases a pending operation, calls an adapter
outside the transaction, and conditionally records the answer. A restart reads pending records
and reconciles them. It needs neither a new workflow engine nor a separate generic job service.
The worker's event source and deployment belong to its host; there must be a configured recovery
pass for committed pending work even if a notification is lost.

Keep these responsibilities separate:

| Symphony | Consuming project |
| --- | --- |
| Attempt identity, task/role admission, input binding, operation intent, retry keys, leases, revisions, normalized observations and result eligibility | Provider/account selection, vendor REST clients, credentials and quotas, runtime image, workspace materialization and network policy |
| Durable facts about collection and cleanup; authorized task/attempt reads and controls | Artifact storage, retention policy, upload/download and content verification, provider-specific stop/delete behavior |
| A bounded, versioned adapter contract and the hc adapter | Any hosted provider implementation and the service that runs its worker |

Execution state is separate from configured task/delivery phases. The approved
[workflow direction](project-workflow.md#project-workflow-shape) permits a project-defined lifecycle;
execution must not freeze it to today's phases. A workflow can authorize an execution intent or
consume its evidence, while its own revision-bound transition and landing rules decide progress.
An environment becoming ready or a turn ending does not mean delivery is complete.

The contract uses typed values and opaque references, never provider commands in a task's text.
Credentials stay behind the adapter. An adapter's configuration reference identifies its stable
provider/account/environment scope, without exposing secrets. A runtime's board credential is
scoped to its project, attempt and existing role; it is not the adapter's provider credential.
The initial role set remains executor/reviewer, with existing independent-review and admission
rules. This design grants no new role, landing authority or automatic staffing policy.

- [r] **execution-provider-attempts** — A project could run an executor or reviewer on its own cloud service while Symphony keeps the history, recovery and results of each attempt. Local terminals would keep working. Should Symphony own this common execution boundary and attempt record, with provider clients, credentials and file storage left to each project? Recommended: yes. This adds durable start/control intent, recovery after uncertain responses and authorized historical session links; it does not choose or operate a cloud provider. The cost is a versioned adapter contract, additive board records and recovery logic, then task API/client support. Approve this direction, or comment. [[analysis/remote-execution-attempts#The smallest boundary]]
  [2026-11-17 19:25] agent — Proposed for `tools/remote-execution-attempts`, from source revision `3b460adb62827f674669e8e99f3090561107264f`. This is design only. The analysis separates existing launch/delivery/link support from proposed attempts, failure recovery and actor checks, and lists the tests future implementation must prove. No provider session, deployment, consumer edit, live migration or implementation is authorized by landing this proposal. Follow-up implementation briefs are proposed only after this direction is approved.

    **Earlier verdicts checked.** **consumer-contract-versioning** (B) places the reusable contract in the kit and leaves project code outside the library; **consumer-api-break-release** keeps old public APIs working for a release. Both are preserved. **project-workflow-shape** (C) permits a configurable task lifecycle; execution observations here supply evidence rather than deciding workflow progress. **desktop-client** reserves hosted login and mentions future people; it does not supply authenticated human context to plugin calls. None decides whether Symphony owns the execution boundary and durable attempts, so that direction remains this card's question.
  [2026-11-17 20:35] owner · approved — Yes
  [2026-11-17 20:59] agent — Your “Yes” approves the common execution boundary and durable attempt record. I will schedule its implementation while preserving local terminals and leaving provider clients, credentials and file storage with each project. This does not authorize live provider sessions or deployment.
  [2026-11-17 20:59] agent — Recovered the exact callback reply supplied by its writer after a concurrent save omitted it; original owner verdict retained.
  [2026-11-17 20:59] agent · resolved

## Adapter operations

These names describe semantics, not a Rust trait to copy into a release. Every call has a bounded
deadline and returns a receipt, observation, explicit unsupported response or ambiguous outcome.
No call waits for the whole execution. All mutating calls carry the persisted operation key,
attempt ID and immutable input digest. Capabilities are checked before admission and their
version/configuration is pinned on the attempt.

When a provider provisions an environment and submits work through separate calls, `start`
tracks those as separate keyed steps in the same durable operation history. Commit each intent
before its I/O and each receipt before advancing; the adapter cannot keep the only progress
record in memory. Reconcile work submission independently of resource creation, with a stable
submission key and payload digest. Resource readiness or queue disappearance alone never proves
the initial task was accepted or completed.

| Operation | Boundary and answer |
| --- | --- |
| `start` | Accept the recorded specification and create or locate its execution resource. Answer with an opaque resource ID and acceptance evidence, or definite rejection, or unknown. Acceptance is not proof the runtime received its task or completed it. |
| `reconcile` | Resolve a start/resume/control operation by key and any recorded resource ID after timeout, restart or takeover. Return found with matching input/key, authoritative absence, unknown, or conflicting matches. A transient “not found” is unknown unless the adapter guarantees absence and no later acceptance. |
| `observe` | Read one known resource: provisioning, running, paused, terminal outcome or unknown, with resource generation and an ordered observation cursor where supported. Include optional runtime conversation, checkpoint, completion and artifact-manifest references. Lost contact is unknown, not failure. |
| `cancel` | Request that the attempt cease work. Persist cancellation before calling it. Acknowledgement of the request is not proof the resource is quiescent; observe/reconcile until confirmed. Symphony immediately bars result promotion. |
| `stop` | Stop the owned execution resource, including a hard stop where declared. It does not delete artifacts or history. Distinguish resumable pause from terminated runtime and from resource destruction. Unsupported pause/stop is explicit, never treated as cancellation success. |
| `resume` | Start a new attempt linked to a stopped/paused predecessor and an explicit checkpoint or conversation reference. The adapter declares whether it creates a new resource or reuses a quiescent resource with a new generation. It cannot resume work whose cancellation or predecessor stop is still uncertain. |
| `external_link` | Resolve an authorized, bounded-lived view reference for a resource or conversation. Advertise view, local focus and local screen separately. A remote provider need not have either local capability. Viewing is never a control operation. |

Capabilities also state create idempotency scope and retention, reconciliation guarantees,
observation ordering, cancellation/stop support, resume support, and artifact enumeration/read
support. Start plus recovery that cannot create duplicates is required for unattended dispatch.
When a provider lacks idempotency or authoritative reconciliation, an ambiguous start is held
for explicit recovery; Symphony must not silently provision another resource. An operator may
accept a new-attempt risk only as a recorded authorized decision after identifying or isolating
the possible predecessor, never as an unlabelled retry.

Provider retry keys are scoped by project/board identity, adapter configuration, attempt UUID,
operation kind and operation sequence. Repeating the same operation reuses its key and input;
changed input or an intentional execution retry creates a new attempt UUID and key. A resume
also gets a new attempt. This separates an HTTP retry from another execution, even when they
share a runtime conversation. Expiration of a provider's key guarantee requires reconciliation
or intervention, not a fresh `start` with the old key.

Waking a stopped environment solely to inspect or collect its files is a recorded maintenance
operation on the existing attempt, where supported. It does not resubmit work, clear cancellation
or create a continuation. Continuing execution uses the new linked attempt described above.

## Durable records and identities

An attempt is a permanent identity with immutable inputs and append-only facts; its current
state is a mutable projection guarded by revision. “Immutable attempt” does not mean execution
can never change state. Retries and resumes do not overwrite the prior attempt.

| Proposed record | Minimum content |
| --- | --- |
| Attempt | UUID; project/board and task identity; role and agent-handle reference; predecessor and relation (`retry` or `resume`); immutable task-input revision/digest, repository revision and supplied manifest reference; adapter/configuration/contract version and capability snapshot; initiating principal and authorization-policy revision; creation time |
| Current attempt state | Expected state revision; desired operation; observed runtime state/outcome; current resource binding/generation; lease owner, monotonically increasing fencing epoch and database-clock expiry; runtime execution epoch; capacity reservation; next recovery work; cancellation, collection and cleanup projections |
| Operation/fact history | Attempt UUID and monotonic sequence; operation kind/key and immutable request digest; actor, accepted time, response/ambiguity; observed provider cursor; prior/resulting state revision; errors and retry classification. No credentials or signed artifact URLs. |
| Result manifest | Attempt and exact input revision; producer/resource generation; artifact reference, digest, size and completeness/provenance; collection verification and eligibility. Include partial output and collection failures. Storage remains the project's. |

Keep three identities distinct. **Attempt UUID** identifies one execution history. **Provider
resource ID** is an opaque value scoped to the adapter configuration; it is not a terminal ID
or an authorization credential. **Runtime conversation ID** identifies resumable agent context
and is optional; it may outlive a resource and be shared across linked attempts. A local pane
is a fourth, optional adapter-specific address. Binding a resource or conversation is a recorded
fact; replacing one requires a new generation/fact rather than rewriting its history.

The input revision binds both the task brief supplied to the runtime and the exact repository
commit or supplied input manifest. A task's edit or branch movement does not mutate that input.
An authorized control may continue the older attempt, but accepting its result against a newer
task input requires a new explicit decision; the default is stale and ineligible.

Use additive tables/read APIs, not new fields on `session`, `TaskSessionRow` or `NewLaunch`.
Keep their strict readers and existing semantics intact. Do not fabricate attempts for historical
launches: those rows remain legacy launch evidence. The first implementation follows the
board's schema guards and both database statement paths, with migration tested on scratch data.
This proposal neither selects a new database nor moves the live schema.

Every task, artifact, predecessor and resource reference is resolved within the owning project/
board namespace; composite lookups enforce that scope even when an ID is globally unique.
The runtime execution epoch is distinct from the worker's lease epoch: worker takeover can
observe the same runtime, while restore/resume or replacement execution gets a new runtime
epoch and credential. Stale runtime callbacks, writes and results cannot act for its successor.

## Commit intent, then perform I/O

1. Authenticate the requester and authorize task, role, provider and operation. In one transaction
   check the expected task/input revision, existing role/admission limits and any current
   attempt for that task/role lane. Commit the new attempt, its `start` intent/key and the lane's
   active-attempt pointer together. Reserve execution capacity in this transaction too: workers
   contend on shared reservation rows for the configured project/provider/role scopes, never
   process-local counts or a host-only lock. A repeated client request key returns that same attempt;
   a different concurrent request conflicts. A deliberately additional reviewer needs the
   existing explicit admission, not a new way around it.
2. Claim the pending operation with a database-clock lease and increasing epoch. Acquire, renew,
   cancel and result writes compare expected state/epoch and conflict on the lease/state rows;
   renew and result acceptance require an unexpired lease by database time. Commit that claim
   before any adapter I/O; never keep a database transaction open across the call. A takeover
   reconciles an in-flight operation before repeating it. Lease expiry alone proves nothing
   about the provider resource.
3. Conditionally append its result and advance the projection only if attempt, operation key,
   input digest, resource generation, epoch and expected state revision still match. The write
   must conflict with a simultaneous takeover/cancel. A stale response may be retained as
   diagnostic evidence by the current owner, but cannot bind a replacement resource, clear
   cancellation, advance the task or publish a result.
4. Schedule the next bounded observation/control/collection from durable state. Notifications
   can wake the worker and tell agents what happened, but the committed operation is the work
   source. Retry transient errors with bounded backoff; unknown side effects go to reconcile.

An authorized cancellation request does not need to own a live worker lease. Its transaction
checks the current attempt/runtime epoch and state revision, records the intent and invalidates
in-flight result acceptance. The worker then performs the provider control under its own lease.

Database fencing protects accepted writes, not a vendor API. A delayed old worker can still
reach the provider after its lease expires. Provider idempotency and reconciliation handle
that gap; operation generation also fences runtime writes and artifact publication. A stale
worker never blindly stops a possibly adopted resource: it requests cleanup through the current
owner using the exact attempt/resource generation. A resource reused for resume must reject
older-generation controls or wait until all earlier uncertain controls are resolved; otherwise
that adapter cannot advertise resource reuse. Unknown duplicate resources are quarantined for
reconciliation, not guessed away.

Capacity reservations outlive worker leases: a timed-out or cancelled attempt may still consume
provider resources. Release capacity only on authoritative rejection/absence or confirmed
quiescence under its policy, in a fenced transaction; never on worker death or cancel receipt.
An uncertain resource stays reserved and visible for reconciliation. Reuse the store's existing
transaction/lease patterns, but add this execution scope and authoritative clock explicitly;
do not silently change callback-delivery semantics. The durable operation record is the
execution outbox, separate from the notification outbox. Database connection/version support
and remote artifact-store implementation remain separate work.

## Lifecycle, collection and cleanup

Track independent dimensions, rather than making a single “done” flag mean all of these:

| Dimension | Proposed states and rule |
| --- | --- |
| Dispatch/runtime | `pending`, `starting`, `unknown`, `running`, `paused`, `terminal`. Terminal carries `succeeded`, `failed`, `cancelled` or `stopped` plus evidence. Observation cannot move a terminal generation back to running. |
| Cancellation | `none`, `requested`, `confirmed`, `unconfirmed`. Once requested, this attempt cannot publish success, even if the provider reports success afterward. A queued start cancelled before any I/O is confirmed without provisioning; an in-flight start must be reconciled and its resource stopped. |
| Collection | `not_started`, `collecting`, `complete`, `partial`, `failed`, or `none` with explicit evidence that there are no artifacts. Retrying collection keeps its key and verified content; provider failure still triggers enumeration. |
| Cleanup | `not_due`, `pending`, `running`, `complete`, `failed` or `unknown`. Delete only owned, quiescent resources of the recorded generation after collection or an explicit retention/discard decision. Failure here remains visible and retryable; it cannot erase attempt history. |

Completion of execution does not complete a task or grant any stamp. Only a result from the
current eligible attempt/input/generation, with no cancellation and with the required verified
artifacts, may be proposed as task evidence through the existing role and landing rules.
Once that attempt is superseded, late outputs remain historical and cannot replace current
results. A provider success with missing or unverified files is incomplete evidence. A provider
failure with files already written preserves those files as partial evidence; it is not an empty
result and does not become success because collection succeeded.

Collection uses bounded manifest and content reads through the project's adapter/storage
service, outside plugin render/submit calls. References resolve only for authorized callers;
untrusted paths cannot escape the intended destination. Verify content digests, sizes,
producer/input identity and completeness before exposure as task evidence. Persist collection
intent before copies or uploads, deduplicate by attempt and artifact identity, and resume after
restart. Clean up compute separately from retained artifacts. Removing an artifact under its
retention policy leaves a tombstone/reference status in the attempt, not a broken claim that it
can still be downloaded.

The project's cleanup hook supplies bounded, keyed stop/delete receipts to this same history;
it is not a new storage implementation. Destructive cleanup requires its own recorded intent
and the ownership, generation, collection and retention checks above.

## Plugins and trusted actors

The execution coordinator is callable without a UI plugin. A linked project host can supply
its adapter directly using kit-owned types that do not expose board-store internals. A process
plugin can present controls and read status, and submit a short durable intent to its project's
worker through an authenticated boundary. Its RPC reply is an attempt/operation receipt, not the
execution result. No child job is kept alive by plugin stdin or an outstanding RPC. If a future
plugin seam forwards adapter operations, it forwards bounded receipts/observations only and
needs explicit protocol negotiation; existing process transport support must not be assumed.
The smallest implementation needs no generalized plugin job protocol.

Existing `Route` and `BoardAction` cannot authenticate a human from their arguments. Before a
hosted UI can start or control execution, add a separate versioned context-aware entrypoint
alongside the old traits/calls, or use the consuming host's authenticated control API. The host
constructs trusted context after authentication and project/task authorization: principal kind
(human/service/agent), stable issuer/subject, project, allowed operation and relevant policy
revision. It is never taken from `Submission.payload`, a note, a query string or an agent's
chosen handle. Serialize the same trusted context to a process adapter over its trusted host
channel; do not leak the user's login token or provider credentials.

Persist the initiating principal on intent and the acting service on later effects. Check
current authorization again for cancel/stop/resume, artifact reads, link resolution and result
acceptance; an old receipt is not a permanent grant. Agent authority remains its bound role,
not human authority. A host that has no authenticated-human context can supply only an explicitly
configured service identity with limited operations; a request needing a human is refused.
Linked/process plugins remain trusted project code, not a new security sandbox.

Preserve today's live `session_url` and Go to session behavior. Add authorized attempt-history
reads and keep historical link references on attempts, so leaving a handle no longer discards
the only reference to its execution. Resolve view links on demand with task/project and
provider access checks; do not persist bearer/signed URLs or put them in shared thread text,
logs or caches. Scheme validation alone is not authorization. Live display may project an
eligible current attempt's link into the existing field, while legacy addresses keep their
meaning. A remote attempt has no pane; focus/screen remain local capabilities. A view link
does not authorize cancel, shell access or downloads.

## Checks an implementation must prove

These are proposed acceptance cases, **not tests run for this design**. Use fake adapters, a
controlled clock and scratch databases/workspaces; no paid sessions or live migrations.

| Case | Required observable result |
| --- | --- |
| Timeout after provider accepted start | Restart reconciles the same key to one resource; no second paid job and no premature failure. |
| Rejected start, authoritative absence, eventual visibility and expired key retention | Distinguish rejection/absence/unknown; repeat only when safe, hold ambiguous work, refuse automatic duplicate creation. |
| Crash before intent commit, after commit, after I/O and before result commit | No unrecorded start before intent; committed work is recovered; accepted resource is found without overwriting another attempt. |
| Two dispatchers, skewed worker clocks and expired-lease takeover | Database time and transactional reservations enforce capacity across replicas; one admitted attempt per lane/request; idempotent provider calls; old epoch cannot renew, update state or publish artifacts. Different reviewer lanes still require explicit admission. |
| Cancel before dispatch, during start, during collection and concurrent with success | Cancellation bars promotion at its commit; late resource is discovered and stopped; retained files remain historical. A success already accepted before cancel stays in history, with later eligibility withdrawn. |
| Stop timeout or delayed old control followed by resume | No assumption of quiescence; no reused resource while an old stop may still affect it; new attempt/generation preserves the predecessor and conversation. Unsupported resume is explicit. |
| Duplicate/out-of-order observations and webhook replay | Resource/cursor/generation/revision checks reject stale transitions and duplicate facts. Webhooks authenticate and trigger reconciliation; they do not bypass the coordinator. |
| New task input or replacement attempt while old work completes | Late result cannot change the task, stamps, current links or current result. Historical facts stay queryable. |
| Files written before runtime failure, crash during upload and empty manifest | Enumerate after failure, retain verified partial files, retry collection without duplicate publication, distinguish no files from unknown/unreadable files. |
| Cleanup failure, collection failure and retention expiry | Each remains independently visible/recoverable; no deletion before the collection/retention decision, no destructive control of another attempt's resource. |
| Forged owner in action payload, cross-project ID and unauthorized link/artifact read | No human authority from payload; composite project references prevent cross-project data/control or capability URL disclosure. Same behavior through linked and process entrypoints. |
| Restore with the old runtime still reporting | A new runtime epoch/credential rejects predecessor writes; worker takeover alone does not create another runtime. |
| Local hc launch, restore, close, first-message refusal and Go to session | Existing local behavior and rollback remain covered, with real pane IDs; remote execution never passes a fake pane to hc. |
| Mixed old/new clients and board schemas | Old launch/session APIs and readers work through the compatibility release; new tables and reads are additive on both database paths. |

## Delivery after approval

First resolve [the boundary decision](#execution-provider-attempts). Only then
propose narrowly scoped tasks in this order; these are work boundaries, not implementation
authorization or filed dependencies:

1. **Kit contract:** bounded adapter requests, outcomes, capabilities, opaque identity and trusted
   actor types; fake adapter contract tests, constructors and protocol/version compatibility.
   Add new context-aware calls beside old ones only where a real consumer requires them.
2. **Board attempts:** additive records, atomic admission/intent, fenced recovery and collection/
   cleanup projections in `crates/tasks` launch/store, using the approved contract. Prove the
   ambiguity/race cases on scratch data and preserve local launcher regression tests. Wrap hc
   incrementally; do not require remote configuration for any existing local operation.
3. **Task delivery:** authorized attempt-history reads, controls, artifact/link references and
   additive wire/client presentation; preserve live session links and test refusals and stale
   results. Read the client guide before selecting implementation paths. Consumer-owned provider
   and storage adapters have their own project tasks and gates.

Land each shared contract before its users. New public APIs coexist with the working old items
for a release without deprecation warnings; removal is a separate later-release task after
proven consumer migration. Kit major-version and protocol negotiation requirements still apply.
Future code needs focused regression tests, coverage analysis, repository lint and the checks
its kind requires, an independent assessment/review and the normal landing contract. This
document supplies no executable settings, provider SDK, hosted service or live schema change.
