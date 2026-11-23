# Optional GitHub integration

This proposal exchanges selected tasks with GitHub issues and publishes work as pull requests.
Its manual issue exchange is built, turned off unless a project commits its settings
([GITHUB](../GITHUB.md)); the pull requests, the presentation plugin and the rest are not, and
nothing is enabled. The companion [public-readiness audit](public-readiness.md)
concerns repository exposure independently of integration adoption.

Recommend one optional GitHub adapter with independently enabled issue and PR capabilities. It
runs outside the board through the versioned JSON API. A process plugin can present status and
explicit actions. Disabling it leaves local tasks, reviews and landing available.

The [delivery decision](#github-outbound-pr-delivery) directs enabled PR mode to
the requirements in [[task:inbox/change-just-land-and-review-commands-to]]: stamped/code work
lands through `gh` PR squash merge, with the required local evidence represented and enforced
on GitHub. GitHub performs the merge and local state reconciles afterward, with no local merge
pushed in its place; an import from a configured upstream is the one delivery merged with its
ancestry kept instead of squashed
([delivery operations](../../workflow/analysis/github-pr-landing.md#delivery-operations),
[the workflow](../UPSTREAM_DOWNSTREAM.md)). Checks remain local; there is no per-PR hosted CI. Simple document-only local delivery
is an optional constrained exception for that concept to specify. These are design requirements,
not changes to today's production landing command.

## Existing contracts and gaps

| Boundary | What exists | Relevant limit |
| --- | --- | --- |
| Board access | [Tasks API](../../engineering/BOARD_API.md#the-tasks-api) 2.1: `version`, `tasks`, `get`, `create`, `amend`, `thread`, keyed `say`, guarded lifecycle operations, `bound`, `edit_bound`, `integration_state` | Creation enforces unique slugs. A brief is rewritten conditionally by `edit_bound`, with the token `bound` gives under the board's recovery generation; `amend` updates title/links, not the brief. |
| Presentation | [Process plugins](../../engineering/PLUGIN_PROTOCOL.md): commands, components, routes, board actions, gates, watches | A watch warms/forgets state; it is not a durable task-event feed. Process plugins cannot supply board fields. |
| Delivery here | [Default land](../../workflow/TASKS.md#the-default-landing): required stamps at the reviewed tip, local merge, then main push | This remains the implemented path. Enabled PR delivery needs the separately scoped squash-merge/evidence enforcement implementation and selected branch publication before activation. |
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
In enabled PR delivery, the configured GitHub evidence gate is explicit: a failed remote check
or unavailable merge blocks that delivery, without silently falling back to local merge.

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
needs contents write. Import/track-only operation can stay read-only. These are transport
permissions; the separate landing concept specifies trusted stamp reporting and repository
enforcement permissions. No administration or workflow mutation is implicit in the adapter.
GitHub supports restricting installation tokens to selected
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
| Delivery | A proved configured land can close its linked issue when that task's policy asks. In enabled PR mode that includes the canonical remote merge result and local reconciliation. Cancellation has a separate explanation and is never recorded as delivery. |
| Comments | Selected remote discussion becomes attributed keyed notes. Export only messages explicitly marked public, excluding private owner notes, logs, screenshots, callback addresses and attachments. |
| Contributor PR | Link and show head SHA/state. New commits invalidate evidence for the previous SHA. GitHub review does not automatically become a local verdict. |

Open/closed is less expressive than the local lifecycle. Project labels may display that lifecycle,
but editing them is not a move command. Publication preview matters even with a clean secret
scan: private planning text need not contain a token to be unsuitable for a public issue.

## Identity and recovery

Bind the local project/task within its proved board lifetime to numeric repository ID, issue/PR
global ID and current URL/number. Supported title edits retain the local row ID; the current API
has no task-slug rename operation, and adding one is outside the initial adapter scope. Repository
names and usernames are display names; transfer to an unapproved repository pauses until explicit
rebind. A 404 means missing or inaccessible, never delete local work. A changed local binding
requires reconciliation rather than adopting another task with the same slug or numeric ID.

Before writing, persist operation ID, target, digest, expected base, board and restore authority
scope, and configuration revision.
Persist acknowledgment before advancing a cursor. Imported tasks use a deterministic slug from
repository/issue IDs: existing unique creation prevents duplicate imports after a crash. On a
collision, verify the recorded identity instead of adopting an unrelated task. Keyed `say`
deduplicates notes. Expose the existing task ID only with its proved lifetime within the board,
without promising that it stays immutable through restore. The investigation at `c3f11c0f6`, on
[[task:tools/github-board-contracts]], reproduced an old-dump restore resetting the task counter
and row versions: an ID allocated after the snapshot can later identify another task, while an
expected version can repeat. Source inspection finds the reset in both copy backends; the
prerequisite still needs its planned proof on each backend. Retaining one restored row's ID does
not prove safety for all mappings or stale writes.

Reliable exchange therefore still needs additive identity/content contracts and a proved restore
fence. Check an authoritative restore/rebind scope atomically with conditional writes, and reject
stale mappings/tokens after restore or state loss. A version-only comparison or a documented manual
rebind instruction alone does not supply that guarantee. The prerequisite must identify and test
an existing authority signal and its lifetime; if none exists, report bounded alternatives and
migration/compatibility consequences before implementing new authority. No second task identity
scheme or broad copy/schema redesign is assumed by this proposal.

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
defines those inputs. The enabled-mode requirements are settled; their detailed landing design
is owned by [[task:inbox/change-just-land-and-review-commands-to]].

Publish the parent task's draft PR after the executor's one merge of main and named review tip.
Push the
configured publication branch to a checked remote using argument arrays; open a draft PR with
curated public content and SHA. This grants no stamp. Review fixes append commits. Unexpected
remote edits or non-fast-forward updates stop for reconciliation; no force push is included.

Before land, local tip, canonical PR head and required evidence must name the same SHA. Publish
the assessment's required or explicitly waived local evidence to GitHub so missing/stale stamps
block merge. A remote label, comment or unauthenticated success claim is not a local stamp.
The landing concept defines the trusted reporter, waiver authority and enforcement boundary;
this adapter does not invent another gate engine or run hosted checks.

The landing command performs the enabled `gh` merge (squash for a task, a merge commit for an
upstream import, each verified by its receipt's topology), records its resulting remote commit
and reconciles local state. Journal the attempt before dispatch and recover its canonical result
after a lost response; a successful PR-close call is not proof of merge. Squash provenance must
retain the reviewed PR head and its evidence even though the resulting main commit differs.
Post delivery links/close linked issues only after that configured delivery is proved.

Simple document-only local delivery may remain where a constrained policy can enforce its
scope; a `.md` filename alone must not grant a general protected-branch bypass. The separate
concept decides that boundary, retries, branch protection and reconciliation. Disabled PR mode
retains local landing. The adapter never merges autonomously, enables auto-merge, publishes tags
or changes visibility/protections. Importing an external PR never checks out or executes its code.

- [r] **github-outbound-pr-delivery** — Symphony can show local work on GitHub either as delivery updates linking the landed commit, or as pull requests opened before the local merge. Which should the first optional integration deliver? A: issue exchange and incoming PR tracking first; local deliveries post merge links, with outbound PR creation deferred. B: also publish a draft PR from a selected task branch before local land, naming its reviewed commit. This explicitly permits those branch pushes when enabled; local stamps still decide readiness and the local land alone merges/pushes main. Recommended: B, because it delivers outbound PRs as requested. Approve and say A or B in the note, or comment. [[analysis/github-integration#Pull requests and local land]]
  [2026-11-17 18:59] agent — The current policy pushes only main through land. A keeps that policy but does not create outbound PRs. B exposes selected task branches and needs a publisher, scoped credentials and recovery; a PR or GitHub review grants no local approval. Neither option enables integration now or lets GitHub merge main.

    **Earlier verdicts checked.** **needed-stamps** governs local evidence and stays in force under both choices. **release-delivery** governs installed releases, not task branches. Your task asks for optional GitHub integration and PR delivery; it does not choose the publication step or authorize implementation during this design phase. Optionality and MIT are already settled and are not asked again.
  [2026-11-17 19:13] owner · approved — answered in another ticket just written.
  [2026-11-17 19:18] agent — Recorded against your new task, inbox/change-just-land-and-review-commands-to: when PR mode is enabled, stamped changes use GitHub squash merges with assessed evidence enforced on GitHub; checks still run locally, and simple document-only changes may retain local delivery. That task is staffed for the integration design. This supersedes the local-merge-only assumption above; we will align the GitHub proposal without asking this choice again.
  [2026-11-17 19:18] agent · resolved
  [2026-11-17 19:21] agent — Located the owner-created ticket inbox/change-just-land-and-review-commands-to. Its direction is settled: once the GitHub plugin lands and PR mode is enabled, stamped/code delivery uses gh PR squash merge, required or explicitly waived local stamp evidence is represented and enforced on GitHub, and checks remain local without per-PR hosted CI. Simple document-only local delivery is an optional constrained boundary for that concept to specify. This supersedes my proposed local-only merge assumption; I will update the integration analysis and coordinate with that ticket without asking another A/B question or enabling anything here.

## Implementation and decision handoff

1. Complete [readiness remediation](public-readiness.md#remediation-order) independently.
2. Land only missing board contracts first: conditional content updates checked against restore
   authority, truthfully scoped task identity and durable state discovery. Reuse unique creation.
   Preserve old calls under the [consumer API
   rule](../../workflow/CHECKS.md#consumer-api-changes). No provider-specific database access.
3. Ship disabled configuration, preview and manual issue import/export. Use a fixture HTTP
   server and scratch board; real API proof needs an explicitly authorized scratch GitHub repo.
4. Add exchange/recovery tests: lost responses, duplicate creation, state loss, concurrent edits,
   rename/transfer, 404/401, changing pagination, rate limits, disable during dispatch, reenable
   previews, malicious content, and credentials/paths in diagnostics.
5. Add status/actions plugin and PR tracking, then implement the settled enabled PR direction
   with its landing/evidence prerequisite. Test stale heads, remote divergence, review fixes,
   missing/waived stamps, squash provenance, import topology and wrong-method merges, constrained document delivery and crash recovery
   after remote success but before acknowledgment.

The [delivery card](#github-outbound-pr-delivery) records the referral to the
owner-created PR landing task; its requirements replace an assumption that every mode merges
locally. No second A/B answer is needed. Optionality and MIT are settled too. Separate packages
can wait for a real deployment need. Existing register content and owner history remain intact;
no production integration or broad document migration is implemented here.
