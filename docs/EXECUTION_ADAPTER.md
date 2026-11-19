# Execution adapter contract

`symphony_plugin::execution` is a provider-neutral kit contract for short execution operations.
A project supplies an `Adapter`; a host supplies its trusted pinned `Profile`, current
`Authorization` and persisted `Call`. `invoke` checks those bindings before calling the adapter
and checks the answer before returning it. The kit links no board or server code.

The [execution design](analysis/remote-execution-attempts.md) describes the coordinator this
contract supports. The kit supplies no attempt database, atomic admission, capacity reservation,
lease clock, epoch persistence, runtime credential fencing, result promotion or dispatcher.
Supplying a fencing number does not implement any of those guarantees. Existing local launch,
session controls and notification `Transport` retain their own calls and semantics.

## Identity and authority

`Scope` holds stable project and board identities. `AttemptRef` adds the task and attempt UUID.
`OperationIdentity` adds configuration, immutable input digest, persisted key, operation kind,
sequence, worker fencing epoch and runtime execution epoch. A retry of I/O retains its original
key and input. An intentional retry or resume creates a new attempt. A coordinator persists
intent before I/O and conditionally accepts the answer against its current authoritative state.

`ResourceId` is opaque and meaningful only in its pinned configuration. `Resource` binds it to
an attempt and exact generation. `ConversationId` is separate and optional; it is never a pane
or proof that a resource is running. `Input` pins task revision, repository revision, digest and
an immutable manifest reference. `ContentRef` describes a reference and digest without providing
storage, verification, credentials or a download policy.

The host authenticates and authorizes the principal before constructing
`Authorization::from_host`. Its grant names one attempt, one operation and the policy revision.
The current acting principal is distinct from `Call::initiator`, which comes from the persisted
intent. Human, service and agent identities carry stable issuer/subject and no credentials.
Payload claims, including an owner name, never construct a trusted grant or upgrade an agent.
Legacy `Route` and `BoardAction` arguments cannot establish an authenticated human.

An authorization deserialized from arbitrary input has no trust. Only an authenticated host
channel may carry it to an adapter process. The host must reauthorize every control and link
read and must use the project's authenticated admission boundary. This contract implements no
identity provider, tenant membership system or credential verifier.

## Calls and outcomes

An adapter must bound all its I/O by the supplied monotonic deadline and return without waiting
for execution completion. `CallLimit` permits 1 through 30,000 milliseconds per wire call;
the host separately enforces the original end-to-end transport deadline. `invoke` refuses an
expired or excessive deadline before I/O and turns late answers into `Unknown::Deadline`.
It cannot preempt arbitrary linked project code: adapter compliance and transport enforcement
are required. A job's lifetime must never depend on an outstanding RPC or plugin stdin.

| Request | Meaning |
| --- | --- |
| `Start` | Create or locate the execution resource for the immutable input. Acceptance does not prove task submission or completion. |
| `Reconcile` | Resolve a non-resume operation after timeout or takeover. Retain its original kind/key/input and use fresh authorization for reconciliation. |
| `ReconcileResume` | Resolve a resume while carrying its persisted predecessor, continuation reference and mode. A found resource must satisfy the same ID/generation constraints as direct resume; a plain `Reconcile` for a resume is refused. |
| `Observe` | Read one resource generation and optional cursor. Return runtime state, conversation and optional checkpoint/completion/artifact-manifest references. |
| `Cancel` | Request cessation. A receipt confirms acceptance of the request, never quiescence. The coordinator bars promotion when it records cancellation. |
| `Stop` | Request pause, termination or hard stop as separately advertised. It deletes neither artifacts nor history. |
| `Resume` | Start a new attempt linked to a same-task predecessor with recorded quiescence evidence and an explicit checkpoint or conversation. Every supplied conversation ID, checkpoint reference and checkpoint digest must be nonempty, even when another supplied reference is valid. New-resource and reuse-with-new-generation modes are separate capabilities. |
| `ExternalLink` | Resolve a current-authorized view for the exact resource, with a positive lifetime of at most 30 seconds. Viewing grants no execution control. |

Capabilities advertise operations, stop/resume modes, composite-key idempotency retention,
authoritative absence, observation ordering, artifact enumeration/read, external view, local
focus and local screen separately. Defaults advertise nothing. A pinned capability snapshot is
not an admission policy: the coordinator must establish that its provider's key retention and
recovery guarantees safely cover an unattended dispatch. An expired key never licenses a blind
start. Multi-step provisioning/submission needs independently persisted keyed steps, including
submission evidence; resource readiness alone is insufficient.

`Accepted` is an operation receipt; `Found` is a reconciled matching receipt. `Absent` is accepted
only for reconciliation with an advertised authoritative guarantee and evidence that no later
acceptance can occur. Eventual visibility, lost contact, expired keys and deadlines stay
`Unknown`. `Conflicting` preserves ambiguous matches for quarantine/intervention. Rejection and
unsupported capability remain explicit. Wrong reply kinds, identities, resource generations,
input or fence values become `Unknown::InvalidReply`. No answer completes a task or grants a
delivery stamp. Artifact-manifest availability grants no result eligibility.

## Process compatibility

`execution::wire::VERSION` is 1.1, independently negotiated from the
[UI plugin protocol](PLUGIN_PROTOCOL.md). `negotiate` chooses a common minor in the same major;
an `Envelope` must carry that exact negotiated version, also pinned in `Profile`. It carries
the trusted authorization, call and bounded limit. Minor 0 carries the original requests;
`ReconcileResume` requires minor 1. The host checks `Request::since` before forwarding, and
dispatch refuses unsupported minors without adapter I/O. No fallback may drop resume constraints.
The coordinator persists the original predecessor/mode before resume I/O and reuses them for
reconciliation. `Envelope::dispatch` is a data boundary for
an authenticated host channel; it launches no process and installs no plugin forwarding seam.
Every `Answer`, including absence and ambiguity, carries the agreed version, original operation identity and
epochs; `Envelope::accept` checks them and the outcome before returning it to the sender.
Existing plugin declarations advertise no execution support.

Wire enums are externally tagged. Unknown fields are ignored; missing required fields, invalid
limits and unknown variants fail closed. A new required field or incompatible meaning needs a
new major. Public structs and enums are non-exhaustive and have constructors/defaults so later
Rust additions preserve source compatibility. The existing plugin protocol, constructors and
linked/process notification calls are unchanged. Transport failures are ambiguous to the sender,
which reconciles the same operation rather than submitting a second execution.

The board's coordinator ([execution attempts](EXECUTION_ATTEMPTS.md)) supplies the durable
intent, recovery, fencing and capacity this kit leaves out; integration still requires the host's
current authorization and provider-specific idempotency/generation proof. Real providers,
credentials, paid sessions, storage and live activation are outside this kit contract.
