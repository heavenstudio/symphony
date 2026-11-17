# Optional GitHub integration

This proposal exchanges selected tasks with GitHub issues and publishes work as pull requests.
None of it is implemented or enabled. The companion [public-readiness audit](public-readiness.md)
concerns repository exposure independently of integration adoption.

Recommend one optional GitHub adapter with independently enabled issue and PR capabilities. It
runs outside the board through the versioned JSON API. A process plugin can present status and
explicit actions. Disabling it leaves local tasks, reviews and landing available.

## Existing contracts and gaps

| Boundary | What exists | Relevant limit |
| --- | --- | --- |
| Board access | [Tasks API](../TASKS.md#the-tasks-api) 2.0: `version`, `tasks`, `get`, `create`, `amend`, `thread`, keyed `say`, guarded lifecycle operations | Creation already enforces unique slugs; internal task ID is not exposed here. No conditional brief update; `amend` updates title/links, not the brief. |
| Presentation | [Process plugins](../PLUGIN_PROTOCOL.md): commands, components, routes, board actions, watches | A watch warms/forgets state; it is not a durable task-event feed. Process plugins cannot supply board fields or gates. |
| Delivery here | [Default land](../TASKS.md#the-default-landing): required stamps at the reviewed tip, local merge, then main push | [Standing rules](../../RULES.md) authorize no task-branch pushes. Outbound PR publication needs an explicit policy change before activation here. |
| GitHub | Issue and PR REST endpoints | Issue listings include PRs. Separate them by `pull_request`; use PR endpoints for PR identity/head information. [GitHub documents the distinction](https://docs.github.com/en/rest/issues/issues). |

The adapter never writes database tables directly or derives API behavior from Symphony source.
The local JSON API trusts programs running as this OS user. Adapter validation is the boundary
for remote input; a GitHub username does not become a local owner or independent reviewer.

## Package and process boundary

| Choice | Benefit | Cost |
| --- | --- | --- |
| **One adapter, independent issues/PRs — recommended** | Shares credentials, repository identity, reconciliation and diagnostics; either capability works alone. | Separate permission checks, journals and tests per capability. |
| Separate issue and PR plugins | Independent deployment. | Duplicates provider mechanics before a consumer needs separate deployment. |
| Integration inside the board/server | No extra program. | Couples local operation and server lifetime to a remote provider. |

The board-facing interface lists/reads selected work, imports a proposal, applies a conditional
content edit, appends a keyed note, and reads delivery evidence. Its Symphony implementation uses
`tasks api`; another board can implement that interface without the plugin kit. Extract a general
multi-provider framework only when a second board uses it. The optional presentation plugin
uses kit blocks and reads local status; rendering performs no remote mutation.

Initially an explicit bounded reconcile command does the work. A scheduler may later run that
same operation. Neither a page render nor each agent/worktree starts a worker. One lock protects
the project's shared integration state, not the machine. GitHub failures leave pending work and
visible diagnostics; they do not stop the board or implicitly become landing gates.

## Enable, disable and credentials

These are proposed adapter settings, not keys accepted by today's `symphony.toml`. The adapter
would validate its own `symphony-github.toml`. Existing `[plugins.process]` can register its
presentation binary. Configuration contains credential references, never their values.

| Setting | Proposed behavior |
| --- | --- |
| `enabled` | False by default; permits local preview/status only, with no GitHub reads, writes or replay. |
| `repository` | Explicit allowed GitHub.com owner/repository; first connection pins numeric repository ID. Never inferred from task links. Other hosts wait for a tested host/credential policy. |
| `issues` | `off`, `import`, `export`, `exchange`; default off. Only individually linked tasks and explicitly selected issue labels participate. |
| `pull_requests` | `off`, `track`, `publish`; default off. Track needs no branch push; publish also needs project authorization. |
| Publication selection | Local per-task opt-in and preview of exact public content. It does not expose the internal thread/brief wholesale. |

Only the main checkout's committed configuration enables/widens access; branch files cannot.
Missing/invalid configuration fails closed. Check enablement again before every dispatch. An
already accepted remote write cannot be recalled; reconcile an in-flight unknown result before
retrying. Pause retains mappings/journal, removal deletes no remote issue or PR, and reenable
first makes a preview requiring explicit application instead of replaying accumulated work.

Mappings must survive worktree removal, artifact sweeps and restart. A small board API capability
would resolve their home in the canonical main board's Symphony state, with state-folder names
centralized in `crates/tasks/src/folders.rs`. Keep it out of Git and document backup/restore.
That capability is a prerequisite, not an API that exists today. Another checkout must not become
a second publisher unnoticed.

Prefer a repository-scoped GitHub App installation for unattended use. Issue exchange needs
issues read/write; PR publication needs pull requests read/write, and branch publication also
needs contents write. Import/track-only operation can stay read-only. No administration or
workflows permission is requested. GitHub supports restricting installation tokens to selected
repositories and permissions; see [App security practices](https://docs.github.com/en/apps/creating-github-apps/about-creating-github-apps/best-practices-for-creating-a-github-app).
An explicit fine-grained token provider can serve a local trial; never silently reuse all `gh`
or Git credentials. Key material belongs in an OS credential store or restricted operator secret
file outside the repository. Keep it out of child argv, URLs, plugin environments and logs.
Revocation/401 pauses writes and preserves recovery state.

## Authority and field ownership

An imported issue creates a backlog proposal, with attribution and canonical URL. It cannot
start an agent, execute text, claim work, approve a card, stamp a task or land a branch. Treat
remote Markdown as content, never a tool instruction, shell argument or local path. Fetch no
attachments or linked private URLs automatically.

| Data | Exchange rule |
| --- | --- |
| Title/brief | Import once, then compare each side with the last synchronized version. One changed side can update an unchanged side; divergent edits become conflicts. Conditional board updates must check the expected version atomically. Never timestamp last-writer-wins. |
| Public local description | Curated export may differ from the execution brief. Preview before first publication; later edits use the same conflict rule. |
| Labels/assignees | Write only configured adapter-owned labels; preserve all others. Priority mapping is explicit. GitHub assignees are not agent handles. |
| Workflow, roles, dependencies, stamps | Symphony owns these. Remote close/reopen becomes a triage request, never automatic local completion, cancellation or revival. |
| Delivery | A proved local land can close its linked issue when that task's policy asks. Cancellation has a separate explanation and is never recorded as delivery. |
| Comments | Selected remote discussion becomes attributed keyed notes. Export only messages explicitly marked public, excluding private owner notes, logs, screenshots, callback addresses and attachments. |
| Contributor PR | Link and show head SHA/state. New commits invalidate evidence for the previous SHA. GitHub review does not automatically become a local verdict. |

Open/closed is less expressive than the local lifecycle. Project labels may display that lifecycle,
but editing them is not a move command. Publication preview matters even with a clean secret
scan: private planning text need not contain a token to be unsuitable for a public issue.

## Identity and recovery

Map local project/task immutable identity to numeric repository ID, issue/PR global ID, and current
URL/number. Titles, slugs, repository names and usernames are display names. Renames preserve
identity; transfer to an unapproved repository pauses until explicit rebind. A 404 means missing
or inaccessible, never delete local work. Until immutable local identity is exposed, slug changes
require explicit rebinding.

Before writing, persist operation ID, target, digest, expected base and configuration revision.
Persist acknowledgment before advancing a cursor. Imported tasks use a deterministic slug from
repository/issue IDs: existing unique creation prevents duplicate imports after a crash. On a
collision, verify the recorded identity instead of adopting an unrelated task. Keyed `say`
deduplicates notes. Expose the existing internal task ID through an additive API field and prove
its backup/restore lifetime; introduce no second UUID scheme. Conditional content updates still
need additive API work before reliable exchange.

Do not assume GitHub creation has a client idempotency key. Include an opaque operation marker
in the created body. After a timeout, list the complete selected scope for that marker before a
retry; do not rely on immediate search indexing. Adopt one match; stop on multiple matches.
State loss requires a recovery/rebind preview, not republishing the backlog. Suppress echoes by
operation ID and content digest, not by ignoring everything the bot account writes.

Re-fetch before updates. An unexpected version becomes a conflict. Do not assume GitHub offers
transactional compare-and-set across both systems: preserve prior/intended content, verify each
write and surface concurrent edits. The proposal does not promise lossless simultaneous editing
without a proved endpoint contract.

Manual reconciliation uses pagination, authenticated conditional reads, overlapping update
windows and full selected-scope checks. Check closed issues and removed selections; advance
checkpoints only after complete pages succeed. Serialize mutations and obey `Retry-After` and
rate-limit resets with bounded backoff. GitHub [recommends webhooks and documents conditional
polling/retries](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api).
Polling is the initial tradeoff for a local app without a public receiver.

A later optional webhook receiver stays separate from owner-write routes: HTTPS, signature
validation, bounded bodies, event/action allowlist, durable delivery-ID deduplication and queued
processing. Re-read canonical objects for out-of-order/replayed events; the reconciler repairs
missed events. Follow [GitHub's webhook guidance](https://docs.github.com/en/webhooks/using-webhooks/best-practices-for-using-webhooks).
No tunnel or shared-server reconfiguration is implicit.

## Pull requests and local land

A PR needs a remote head and base. Main against that same main after delivery cannot represent
the delivered diff; [GitHub's PR endpoint](https://docs.github.com/en/rest/pulls/pulls#create-a-pull-request)
defines those inputs. This is the material delivery decision:

| Option | Visible result | Effect on policy |
| --- | --- | --- |
| **A. Track PRs and publish delivery links first** | Incoming PRs linked to tasks; local deliveries update issues with merge links. | Only land pushes main. Creates no outbound task PR. Smallest initial release. |
| **B. Publish PR before local land — recommended for outbound PRs** | One PR per parent task and a stable remote head showing its reviewed SHA. | Explicitly permit selected task-branch pushes; local land remains sole merger/pusher of main. |
| C. GitHub merges, local board reconciles | GitHub becomes merge authority. | Replaces present local stamp/merge/closure sequence; outside this proposal. |

Under B, publish only after the executor's one merge of main and named review tip. Push the
configured publication branch to a checked remote using argument arrays; open a draft PR with
curated public content and SHA. This grants no stamp. Review fixes append commits. Unexpected
remote edits or non-fast-forward updates stop for reconciliation; no force push is included.

Before land, local tip, mapped PR head and required evidence must name the same SHA. Publication
stays advisory by default; projects wanting it mandatory must configure that in their landing
policy. After local merge and successful main push, reconcile observed GitHub state and post the
merge link once. Prove in a scratch repository whether the local merge shape makes GitHub mark
the PR merged; a successful close API call is not proof of merge. No GitHub merge endpoint,
auto-merge, independent main push, tag publication, protection change or visibility change is
part of the adapter. Importing an external PR never checks out or executes its code.

## Implementation and decision handoff

1. Complete [readiness remediation](public-readiness.md#remediation-order) independently.
2. Land only missing board contracts first: conditional content update, exposure of existing
   task identity and durable state discovery. Reuse unique creation. Preserve old calls under the [consumer API
   rule](../CHECKS.md#consumer-api-changes). No provider-specific database access.
3. Ship disabled configuration, preview and manual issue import/export. Use a fixture HTTP
   server and scratch board; real API proof needs an explicitly authorized scratch GitHub repo.
4. Add exchange/recovery tests: lost responses, duplicate creation, state loss, concurrent edits,
   rename/transfer, 404/401, changing pagination, rate limits, disable during dispatch, reenable
   previews, malicious content, and credentials/paths in diagnostics.
5. Add status/actions plugin and PR tracking. Add publication only after choosing B; test stale
   heads, remote divergence, review fixes, failed main push, merge detection and crash recovery
   after push but before acknowledgment.

The [delivery card](../DECISIONS.md#github-outbound-pr-delivery) asks A or B with the policy change
and extra public branch exposure visible. A does not fulfill outbound PR creation; B does.
Optionality and MIT are already settled by the task, so need no new question. Separate packages
can wait for a real deployment need. The additive cards preserve existing register content and
owner history; no broader migration is included.
