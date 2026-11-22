# Optional working artifact contracts

The opt-in [`symphony::artifacts`](../../crates/symphony/src/artifacts.rs) core records immutable
working evidence through a private byte provider and a durable metadata CAS. Constructing it
enables no route, settings switch, import, cleanup schedule or task transition. Document/card
pictures remain Git-backed; existing attachment calls, paths and local defaults still apply.
The broader [durable artifact design](analysis/remote-artifact-store.md) describes the intended
integrations, beyond this first core, under [its card](analysis/remote-artifact-store.md#durable-project-artifacts).

## Publication and identity

The trusted host supplies stable tenant/project identities and current authorization on every
operation. A scope, artifact ID, checksum, provider generation or legacy URL grants no access.
The core has no alias resolver or public wire representation. Its serialized ledger and sealed
descriptors are private metadata, including provider bindings and exact generations.

[`Core::begin`](../../crates/symphony/src/artifacts/core.rs) applies the existing upload validator,
computes SHA-256 and length, and saves pending intent before calling a provider. A retry key is
unique within its scope; identical bytes, safe metadata, source and operation converge on one
opaque server-issued identity. Different bytes or provenance conflict. Input repository and full
commit are recorded independently of an optional delivered repository/commit. Dirty-workspace
manifests require their own integration; a parent commit alone cannot represent dirty input.

Upload/reconciliation reads back the acknowledged immutable generation and hashes the full bytes
before recording availability. Unknown put outcomes remain pending until exact-key inspection
and verification resolve them. Incomplete/forbidden/unavailable results stay explicit. Missing,
replaced and corrupt bytes fail closed, including same-size corruption; no read substitutes the
latest object. A range is taken only after full verification of the bounded upload.

Publication appends an immutable record and its owned reference edges atomically in the ledger.
Its key makes a lost metadata acknowledgement retryable without duplicate publication. This
records storage prerequisites only; it accepts no task result, awards no stamp and advances no
workflow. A separate authoritative result transaction must join the storage prerequisites.
Repeating an existing publication reports its original acceptance; it never revives released
reference edges or accepts a new task result.

## Fences and reference ownership

`Operation`/`Permit` supply a host-driven monotonic operation fence for this bounded core and its
deterministic fake-provider tests. Begin, finalization and new publication check the current
operation. Advancing or cancelling it rejects stale completion, including a cancellation between
byte verification and the metadata CAS. These types are not an execution lease, provider runtime
identity, independent recovery authority or persisted board-attempt contract. A database rollback
can roll back this ledger; it cannot establish a fresh post-restore authority.

Publication owners, open upload/publication intents and retained export roots protect bytes.
Closing a worker releases none of them. Release is explicit. Reference inventory starts unknown,
so cleanup retains every generation. A trusted complete inventory must cover retained Git/text
history, legacy references and every restore root before marking completeness. Reserving deletion
and checking roots share the same metadata CAS as publication/export insertion; new references
refuse a reserved generation. Conditional deletion retries the sealed generation only. Unknown
delete outcomes remain reserved. No automatic age deletion, provider lifecycle rule or remote
retention duration is selected here.

## Provider and metadata implementations

[`Provider`](../../crates/symphony/src/artifacts/model.rs) requires create-if-absent put, inspect,
exact-generation read and conditional delete. Its bounded failures exclude secrets and private
locators. It must return at most the existing upload size bound. A hosted provider must document
the worker/container loss its acknowledgement survives; an ETag alone is not integrity evidence.

[`Local`](../../crates/symphony/src/artifacts/local.rs) resolves the configured `Project::artifact`
folder and stores separate scoped objects under `durable/`. It uses OS object locks, synced files,
create-if-absent publication and a fresh generation on recreation. This is local disk persistence,
with no claim of surviving disk/container loss. It never changes the legacy uploads directory.

[`Metadata`](../../crates/symphony/src/artifacts/model.rs) requires durable per-scope atomic CAS,
including concurrent processes and unknown commit outcomes. One aggregate serializes changes
within a project; it is a bounded first implementation, with no per-object scalability claim.
Malformed/unknown metadata formats fail explicitly. No caller may reset operation tombstones.

[`BoardMetadata`](../../crates/symphony/src/artifacts/board.rs) uses the board's scoped
[`artifact_ledger` CAS](../../crates/tasks/src/store/artifacts.rs) on either database. Schema 21
adds this private table without changing existing task/session rows. An opaque JSON envelope
preserves nested nulls through both backends. Opening or constructing the adapter does not enable
hosted delivery. Copy/export/restore integration of its roots remains a separate prerequisite.

## Remaining integration checklist

- Join current operation/source/attempt identities with the real board accepted-result transaction,
  only after `tools/execution-adapter-contract` and authoritative attempt eligibility land. Retain
  fake identities as tests, with no inferred attempt success.
- Use `tools/board-recovery-generation` for logical-copy and independently supported physical
  restore/rebind authority after that contract lands. Ledger revisions alone do not fence restore.
- Implement a consumer-owned private provider, durability fixture and deployment/credential setup.
  This core reads no credentials and provisions nothing.
- Add authorized HTTP delivery only after resolving the consumer's reported signed evidence reads
  with residual expiry against the design's proxied current-auth reads and bounded stream
  cancellation. Preserve that report as provenance, not a verified deployment claim. Record the
  authorization/cache/revocation disposition before integration: GET/HEAD/range/304/cache and
  active streams must have explicit bounds. This core issues no URL and chooses no policy.
- Inventory/import legacy aliases additively, preserving old paths/text. No live import runs here.
- Retain and verify repository commits, workspace manifests, export reference closure and exact
  artifact generations before declaring an export complete or admitting restored evidence.
- Join project-owned authoritative approval attestations, under the revision-bound guard decision,
  without moving approval custody into this storage ledger.

These remain deployment prerequisites, not guarantees supplied by local/fake-provider tests.
