# Durable project artifacts

Proposed: let a consuming project keep working evidence in private durable storage and rebuild
its document workspace from retained Git revisions. A local project keeps its current files and
attachment links. This is a design boundary, not implemented persistence or permission to move
data. Its [decision card](../../DECISIONS.md#durable-project-artifacts) asks about that optional
boundary; provider implementation and any migration need separately approved work.

## Source and scope

The Symphony source inspected is commit `3b460adb62827f674669e8e99f3090561107264f`.
The links below name the source files in this repository; these observations are bounded to that
revision, not claims about a deployed service.

The consumer scope was checked against the [hosted persistence request](https://toptal-core.atlassian.net/browse/UNS-1643)
and [durable evidence request](https://toptal-core.atlassian.net/browse/UNS-1650): the former needs
state and files to survive an ephemeral runtime; the latter binds evidence to its producer and
revision and requires access after session shutdown. Those requests establish intended behavior,
not implemented guarantees or approval of this proposal. Gallery presentation is outside this
storage task.

| Concern | Current source and behavior |
|---|---|
| Attachments | [uploads.rs](../../../crates/symphony/src/uploads.rs), `sniff`, `store`, `path`, `reference`: content-addressed files in the server checkout's configured artifacts folder, named by SHA-256 plus extension. Writes use a temporary file, file sync and rename. Reuse of an existing file checks its size, not its digest. There is no remote durability receipt or metadata publication transaction. |
| Upload and byte routes | [write.rs](../../../apps/symphony-server/src/write.rs), `upload`, and [media_files.rs](../../../apps/symphony-server/src/media_files.rs), `upload`: a bounded upload, kind-specific MIME, sandbox and `nosniff`; file reads stay under the upload folder. The byte route uses public immutable caching. These are local-serving behaviors, not a hosted tenant authorization contract. |
| Admission | [gate.rs](../../../apps/symphony-server/src/gate.rs) judges loopback/tailnet peers and Host; write origin checks are in `write.rs`. A hosted authenticated actor and project membership must come from a consuming project's trusted admission boundary. |
| Runs and logs | [runs.rs](../../../crates/tasks/src/runs.rs), `folder`, `line`, and [run_logs.rs](../../../crates/tasks/src/run_logs.rs), `create`, `read`: JSONL day records and append-only log files under the Git common directory, with bounded log reads. Their existing pruning constants are 90 days for run records and 14 for logs. A growing log is not an immutable artifact. |
| Documents and configuration | [project.rs](../../../crates/symphony/src/project.rs), `file`, `walk`, `artifact`, and [entrypoint.rs](../../../crates/symphony/src/entrypoint.rs) map routes and restrict discovery. [save.rs](../../../crates/symphony/src/save.rs), `document`, reads and edits checkout text under write locks; [commit.rs](../../../crates/symphony/src/commit.rs) queues immutable edits and publishes verdict commits. Local locks and the posted-card guard do not supply a cross-replica expected-revision write. |

The existing [artifact split](../../workflow/analysis/attached-artifacts.md) and
[clone-picture decision](../../DECISIONS.md#pictures-in-every-clone) remain in force: document and
card pictures, including those of past decisions, are committed beside their documents. Working
evidence may stay outside Git. The cancelled reference-index task is not an implemented durable
reference ledger. This proposal changes neither current retention nor existing documents.

Downstream source was also read through the authenticated GitHub contents API, pinned to commit
`61f5ab6c50504a0d97552d19e4796d3ea20c7cc3`: the
[session artifact routes](https://github.com/toptal/maestro/blob/61f5ab6c50504a0d97552d19e4796d3ea20c7cc3/maestro-api/src/routes/artifacts.rs)
authorize ownership and execute stat/read operations in the session pod; the
[shared-copy store](https://github.com/toptal/maestro/blob/61f5ab6c50504a0d97552d19e4796d3ea20c7cc3/maestro-api-artifacts/src/shared_artifact_copy_store.rs)
keys put/get/delete by link ID, without an immutable-generation argument. These are different
read paths: a saved copy need not require a live pod. Neither interface alone establishes the
immutable project evidence contract below. The decoded files independently match the reporter's
cached source copies; their SHA-256 values are respectively
`9167eee274e8e0ec14c4a50c73acc02c8df2aeb63cc44a4b00609068310bc3d6` and
`6d99585a3b82d474ed7dc347e15a0ed1abdfdcd67f8c98b58019eabbfe0a8e20`.
Share-lifetime purge behavior is reported by the brief, not proved by these two files alone.
No platform bucket, paid resource or hosted team URL is selected here, and source inspection
does not verify a deployed service.

## Smallest boundary

Use two existing kinds of authority: versioned repositories for committed documents/configuration,
and the board's durable metadata store for artifact identities, references and publication intent.
A consuming project supplies a byte-store adapter. Do not create a second task scheduler, document
database, vendor client or credential format in Symphony.

The byte adapter needs only conditional immutable put, inspect/verify, generation-pinned read
(including bounded ranges) and conditional delete. Operations return bounded outcomes including
unknown outcome, unavailable, missing, integrity failure and forbidden; provider errors must not
expose credentials or internal locators. Every request carries a server-resolved project scope
and retry identity. Reconciliation uses the durable intent and these operations, not worker RAM.
Deletion is exposed as a capability to the existing authorized cleanup flow, never as a new timer.
Repository fetch and commit publication use the project's repository integration, independently
of the byte adapter. Exact Rust traits, schema and wire versions are future implementation work.

A hosted adapter is eligible only if an acknowledged put survives deletion of the producing
worker and application container, and can subsequently read the exact acknowledged generation.
The provider documents that durability boundary. An ephemeral pod filesystem, a mutable shared
link, or an upload acknowledged only in an application buffer cannot satisfy it. A local adapter
may retain current disk semantics; selecting it makes no cloud-survival promise.

## Identity and provenance

All lookup and uniqueness keys include a stable project identity within its owning tenant. A
display name, checkout path, task slug or repository URL is not that identity. Restore preserves
the identity; an intentional clone into another project allocates a new scope and an explicit
import map. Identical task slugs, artifact IDs, Git object IDs or digests in two projects confer
no shared authority. Cross-project import requires authorization on both sides.

| Record | Required meaning |
|---|---|
| Artifact reference | `(project_id, artifact_id)`, with an opaque server-issued ID. It names immutable bytes, not a path, bucket key, digest, provider session or bearer token. |
| Sealed byte descriptor | Digest algorithm and full digest, exact byte count, validated media type, safe display name, provider binding, private storage locator and immutable object generation. The descriptor is sealed before availability; replacing bytes creates a new artifact. |
| Provenance | Producer kind (human upload, run or attempt); immutable input repository identity and full Git commit, plus input tree/dirty status where relevant. An attempt producer also records its attempt UUID, task/role identity, execution epoch and accepted result fence. Manual uploads need no fabricated attempt. |
| Delivery binding | Exact delivered repository identity and full commit/tree, artifact references and approval/evidence record identities. Input and delivered revisions are distinct; a branch name, current HEAD or runtime conversation ID cannot stand in for either. A pending delivery may not yet have a delivered commit. Binding it appends an immutable publication record rather than changing the artifact's bytes or its original provenance. |
| Control metadata | Pending/available/failed state, operation/retry ID, state revision and durable failure/reconciliation details. These may advance with compare-and-set; sealed bytes and historical publication records do not. |

Dirty or uncommitted work is not recoverable merely because its parent commit exists. Before it
can be delivered, retain its patch or complete workspace manifest as verified artifacts, with a
base commit and digest for every additional file, or commit it through the authorized repository
workflow. Never claim an uncommitted workspace is the contents of HEAD. Secrets and credentials
are outside this manifest; restoration resolves their authorized secret bindings separately and
fails explicitly when required bindings are unavailable.

The public wire carries project-scoped artifact links and safe metadata, never absolute checkout
paths, private locators, provider credentials or public object URLs. A repository-relative document
route is resolved against its pinned project configuration. Illustrative route shape:
`/<project>/api/artifacts/<opaque-id>`; this is not an installed route or frozen wire API.

## Bytes before publication

The metadata transaction and object storage do not share a transaction. Keep the following steps
recoverable and idempotent:

1. Authorize the upload and producer, apply limits, and commit a **pending** metadata intent with
   project scope, operation/retry ID, expected producer fence and proposed descriptor. That durable
   intent is the reconciliation work item; it can use the board's existing outbox primitives.
2. Validate bytes server-side and compute their digest and size. Upload privately with
   create-if-absent semantics, scoped by project and operation. Repeating the same operation with
   different bytes is a conflict; retrying it with the same bytes must not create two publications.
3. Obtain the immutable generation and verify exact bytes, size and checksum. A generic ETag or
   an unverified client-supplied checksum is insufficient. If the provider cannot attest a full
   digest for that generation, read it back and hash it. A timeout after put is an unknown outcome:
   inspect/reconcile the recorded key, never assume failure and overwrite it.
4. Compare-and-set pending to **available** with the sealed descriptor after durable verification.
   Availability means the bytes can be read independently of the worker; it grants no right to
   advance a task. A mismatch becomes **failed**, with no publishable reference.
5. In the authoritative result/publication transaction, recheck actor, project, task/attempt
   epoch, lease fence and expected state revision; require every referenced artifact available,
   and create the reference edges and delivery/approval binding atomically with the accepted
   result. Only then may a delivered task or decision expose that evidence as complete.

For a Git-backed decision, write a durable publication intent naming the exact source blob and
attachments, publish the resulting commit to the authorized durable repository, then record the
verified commit and references. Keep the source and prospective output pinned throughout. A
crash between Git publication and metadata commit is reconciled by that immutable intent; it
does not authorize publishing a different current branch. A locally queued commit alone is not
a completed hosted publication.

| Interruption | Recovery and visible result |
|---|---|
| Intent saved, no object yet | Pending; bounded retries with backoff or an explicit failed outcome. The task is not deliverable. |
| Object stored, acknowledgment or metadata update lost | Reconcile the operation's exact generation and verify it. Reuse matching bytes, fail mismatches; do not publish on a timeout. |
| Available object, result transaction lost | Retry the same publication ID. Reference insertion/result acceptance is idempotent. The object is protected by its pending publication intent. |
| Producer stopped/deleted, or result arrives after cancellation | Collection may preserve already durable bytes as failed-attempt evidence. It cannot invent success or bypass cancellation. Undurable pod-only files are explicitly missing. |
| Stale worker, old lease or pre-restore result arrives | Reject task publication. Bytes already uploaded may remain available but unreferenced until authorized cleanup; they do not become delivery evidence. |
| Byte read later finds missing or corrupt generation | Fail explicitly and mark availability failed through a state revision. Keep reference/approval history and raise repair work; never substitute a newer object or an unchecked local copy. |

Failure records distinguish transient unavailability from missing/corrupt bytes and terminal policy
refusal. Retries reconcile an existing operation and use its deadline/backoff; they do not mint
new task attempts or delete objects on ambiguity. Duplicate content may share one verified object
within a project, while retaining separate provenance and reference edges. Cross-tenant
deduplication is unnecessary for this boundary and must not leak existence or skip authorization.

## Execution boundary

Coordinate with board task `tools/remote-execution-attempts`: an execution attempt owns dispatch,
lease renewal, cancellation, result fencing and restore execution epoch. Storage owns byte
durability and its publication prerequisites. The authoritative accepted-result transaction joins
them using project/board/task identity, immutable attempt UUID, role/handle and exact task-input
revision/digest and repository/commit (or retained manifest). It checks the current active-attempt
pointer, provider configuration/resource generation, runtime execution epoch, keyed operation,
expected attempt-state revision and the accepting coordinator's unexpired database-clock worker
lease epoch. Worker takeover changes the worker lease, not the runtime identity. Restore, resume
or replacement fences the runtime identity and credentials independently. Task/workflow admission
also checks its own current state revision and policy; successful storage grants no stamp. This
join was confirmed on the execution task's thread against design commit
`fa785271f662d85b68345aba74d3f8981ad7afde`; it is not a released API. A storage callback cannot
mark the task done, land a branch or advance its phase. Runtime conversation IDs and provider
resource IDs remain separate from artifact IDs.

Files produced before failure are collected only through an authorized, fenced collection path.
They may document an unsuccessful attempt, without satisfying successful-delivery stamps. Stop
or deletion must report whether collection completed and which evidence is missing. An adapter
that can read only a live pod must finish collection before deleting it; if it cannot, no durable
artifact claim is made. Stopping a worker never removes artifacts already referenced elsewhere.

## Authorization and reads

The consuming project's trusted request adapter supplies actor identity and current tenant/project
membership; Symphony must not accept those claims from a body, query parameter or provider
callback. Check permission on metadata lookup/listing, upload/finalize, reference creation,
document revision fetch, export, restore and deletion, and on **every byte read**, including
HEAD, range, conditional and retry requests. An opaque ID or guessed digest grants no access.
Workers receive scope to their current attempt, not unrestricted project storage credentials.

For the hosted contract, proxy private object reads through that authorization boundary. Do not
redirect to publicly readable objects or reusable bearer download URLs. Revocation denies later
requests, including cache hits and conditional reads; recheck authorization before returning 304.
Use private/no-store responses for sensitive hosted evidence, with no shared cache that can bypass
the check. Already downloaded bytes cannot be recalled. Bound and cancel active reads when their
authorization is revoked; a provider must not continue serving an unbounded stream under an old
grant. Audit access without putting secrets or locators in logs.

Retain the upload validator's 10 MiB bound, content sniffing, safe names, accepted kinds and
active-content refusals, plus kind-specific MIME, `nosniff` and sandbox/disposition behavior.
An object provider is not a way around them. Run logs retain their own bounded, plain-text read
contract: freeze a completed log or publish explicit immutable prefix snapshots with byte counts
and completion status. A live tail remains provisional and is not sealed final evidence. The
attachment cap is not silently applied to whole run logs, nor are their existing read limits removed.

## Documents, approvals and rehydration

On a fresh application disk, resolve an authorized project and retained repository identity, fetch
the **exact** configuration and document commits, verify their Git object identities, then map
routes using that configuration's entrypoints and include/exclude rules. Rehydrate committed
document/card images from their Git blobs. Hydrate external working evidence from the manifest's
artifact IDs and immutable generations, verifying digest and size before use. A local checkout is
a disposable cache. Missing commits, blobs, configuration, artifacts or required secret bindings
produce a specific incomplete-restore failure; never fall back to the latest branch or another
project's copy. Retaining a commit string alone does not retain its reachable objects: the
repository integration must pin the required commits and their trees/blobs in its backup horizon.

An approval records authenticated approver, decision identity, exact source repository/commit and
document blob, the exact card content digest, attachment descriptors and delivered revision it
judges. Record the new verdict commit separately from the content that was reviewed; adding a
verdict cannot retroactively approve text the reader never saw. A later content or evidence change
requires a new binding. Display or branch movement alone cannot transfer approval.

An edit submits its expected project/configuration revision, document commit/blob and stable
decision identity. Compare them against the authoritative current head in one admitted write;
use a conditional repository ref update to prevent two replicas overwriting each other. On
mismatch return a conflict with the authorized current revision, preserving the user's pending
edit. Rebase only with an explicit, reviewable merge; do not silently carry an approval across
changed content. The current local save/commit path remains available; these hosted revision
checks are proposed additions, not assertions about what its locks already guarantee.

## References, cleanup and compatibility

The durable reference set covers active tasks, decision/document revisions, attempts and their
collection/publication intents, and every retained backup/export or other restorable snapshot.
An artifact cannot be garbage-collected while any of these can restore a reference to it. Closing
or deleting a worker does not remove a task's or backup's edges. A reference count alone is
insufficient if legacy text, Git history or an export can name objects outside that count.

Cleanup remains an authorized project operation under the existing retention policy. Enumerate
candidates only from a complete reference set; unknown/incomplete indexing means retain. Reserve
a candidate with a metadata state revision, recheck all roots and protect against concurrent new
references; publication must reject or retry an object reserved for deletion. Only then delete
that exact unreferenced generation and record the outcome. An ambiguous delete is reconciled,
never followed by deleting whatever generation is now latest. This also handles abandoned pending
uploads and multipart remnants once no intent, collection or restore can use them. No automatic
age-based deletion or new retention period is approved here. A new remote retention policy would
be a separate choice; object lifecycle rules cannot run underneath protected references.

Migration is opt-in, resumable and additive:

1. Inventory legacy `/artifacts/uploads/<sha>.<ext>` references, task/card/document text and
   committed history, run day files and log records within the project's retained restore scope.
   Resolve paths only through `Project::artifact` and approved run folders, without following
   arbitrary links. Report missing files before promising a complete migration.
2. Import each local attachment using the publication flow, recomputing digest and size rather
   than trusting the filename or existing-file size shortcut. Record a project-scoped legacy
   alias to its artifact ID. Keep original text, URLs and local files; no document rewrite or
   automatic local deletion. Old aliases in a hosted project resolve through authorization.
3. Snapshot day files and finished logs at a consistent boundary, preserving run IDs, timing,
   commit/tree/dirty and partial/incomplete flags. A live run requires a recorder checkpoint or
   an explicitly partial snapshot. Map old run/log references to sealed generations and preserve
   bounded range reads; do not turn a partial log into a completed run.
4. Reconcile every reference and generation before enabling hosted reads. Mark incomplete imports
   as such. Rollback chooses local reads where the original bytes exist; it cannot claim durable
   success for a remote object that was never verified. Existing pruning and local operation stay
   as they are until separately authorized integration supplies the reference protection required
   for the hosted guarantee.

Old public Rust calls and attachment references keep working. New provider-aware interfaces ship
alongside them under the [two-release rule](../../workflow/CHECKS.md#consumer-api-changes); removal waits for
a later release and proven consumer migration. No unconditional switch of local users is needed.

## Backup and restore contract

A versioned export manifest binds a project/tenant identity and export ID to a consistent metadata
snapshot/checkpoint, reference roots, pending operations, approvals/delivery bindings, attempts,
and every required repository identity/commit/tree/blob. It lists each artifact ID, digest
algorithm/value, size, media type and immutable generation, including legacy aliases and partial
log status. Keep private provider locators and adapter configuration in a protected mapping;
exclude credentials and expiring download links. Include required secret-binding names, not values.
The manifest itself has an integrity digest and is retained outside the ephemeral application disk.

Creating an export pins its reference closure before objects can be collected, captures the
database checkpoint and required Git objects, verifies copied generations, then marks the export
complete. A pending/failed export is not a restorable backup. Database backup alone is incomplete;
copying the latest objects after an unrelated SQL snapshot is not a consistent backup. The
consumer supplies backup scheduling and durable repositories/stores; no schedule is installed here.

Restore validates manifest integrity/version/scope, metadata/reference closure, Git objects and
every artifact's exact generation, checksum and length in an isolated destination before serving
it. A provider relocation uses an explicit verified generation map; it never silently replaces a
sealed descriptor. Reapply current authorization and revocations from the trusted identity service
before exposing bytes. Advance the execution epoch and invalidate pre-restore dispatch/upload
capabilities before reconciling pending work, so delayed callbacks cannot become current results.
Only then admit fresh execution. A backup's old grants do not reinstate access. Missing/corrupt
objects or revisions leave restore explicitly incomplete, with a repair list and no deliverable
claim for affected evidence.

## Validation for implementation

These are future provider/metadata contract tests, not tests run by this design task. Use local and
fake remote adapters with injected crash boundaries and deterministic faults, then a consumer-owned
private-store integration fixture. No live project data or shared-service restart is needed.

| Case | Required observation |
|---|---|
| Worker stopped/deleted after durable upload | Exact bytes remain readable; incomplete collection is reported, with no fabricated success. |
| Empty app disk, restart, scale-zero, second replica | Exact Git revisions and artifacts rehydrate without the old pod, checkout or in-memory registry. |
| Repeated/concurrent upload and publication | Same retry key/bytes converges; differing bytes conflict; one accepted result and correct reference edges. |
| Crash at each upload/SQL/Git boundary | Pending work reconciles idempotently; no deliverable result precedes verified durable bytes; no referenced object is deleted. |
| Missing, truncated, corrupt or replaced generation | Read/restore fails for the exact object, including equal-size corruption; no fallback to latest. |
| Same IDs, slugs and digest across tenants/projects | Every read, alias, import and restore stays isolated; guessed digest and public URL confer no authority. |
| Revocation after metadata read or during stream | Subsequent GET/HEAD/range/304 and cached requests are denied; active delivery is bounded/cancelled. |
| Delayed completion after cancel, fence change or restore | Bytes cannot advance task/phase/approval; old epoch and fence are rejected. |
| Cleanup races publication or retained export | Atomic metadata arbitration protects active/restorable roots; unknown roots retain bytes; delete pins generation. |
| Stale document/card/configuration edit or missing commit | Conflict or explicit missing-revision failure; changed content never inherits an old approval. |
| Legacy references and live logs | Links work without text rewriting; incomplete snapshots remain partial; bounded reads and upload guards hold. |

Future code needs focused unit and provider contract tests for each state transition and failure
case, coverage analysis of those units, lint and independent review under the normal landing
contract. This design's evidence is source inspection and documentation/navigation/card checks
only; it supplies no provider deployment, security accreditation or consumer readiness claim.
