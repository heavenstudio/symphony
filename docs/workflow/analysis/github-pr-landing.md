# Landing through GitHub pull requests

Proposed contract for the optional GitHub integration: when PR delivery is enabled, `just land`
asks GitHub to merge the task's PR, by squash for a task and by a merge commit for an
[upstream import](#delivery-operations), and the local checkout reconciles to GitHub's result.
GitHub requires checks representing the task's assessment
and stamps; builds and tests still run locally. This is a concept deliverable for
[[task:inbox/change-just-land-and-review-commands-to]], dependent on
[[task:inbox/github-integration]]. No setting, permission, command behavior or repository rule
described here is implemented by this document. What is built of it, the task operation, is
[TASKS](../TASKS.md#a-land-delivered-by-a-pull-request) and
[GITHUB](../../operations/GITHUB.md#delivering-a-land), with the reading of the repository's protection
before each step ([GITHUB](../../operations/GITHUB.md#the-protection-a-delivery-needs)), the
generated-output delivery, a task the regenerator files
([TASKS](../TASKS.md#what-the-generators-make-delivered-by-a-pull-request)), the release
cadence and candidate reading its receipts ([RELEASES](../RELEASES.md#the-cadence)), and the
upstream import operation ([TASKS](../TASKS.md#an-import-delivered-by-a-pull-request),
[GITHUB](../../operations/GITHUB.md#delivering-an-import)); the protecting itself is not.

## Scope and ownership

Optional integration, squash delivery of tasks in enabled PR mode, ancestry-preserving delivery
of upstream imports, remote enforcement of assessed
stamps, and local execution without per-PR hosted CI are requirements. In enabled mode GitHub
performs every merge of the default branch: no local merge pushed in its place remains. An assessment may waive
stamps with a recorded reason. These requirements need no new product-choice card. The earlier
integration task owns issue exchange, installation, repository identity, credential handling and
explicit activation of outbound publication. This proposal supplies its PR landing contract;
issue synchronization alone grants no branch publication or merge permission.

The reference contracts remain [default landing](../TASKS.md#the-default-landing),
[needed stamps](../TASKS.md#needed-stamps), [gates](../TASKS.md#gates) and
[evidence](../CHECKS.md#the-gate-is-evidence). Today's default land integrates locally in
`crates/tasks/src/land.rs`, whose `Landing::integrate` is the proposed transport seam. Its
independent assessor, stamp readers and reviewer freshness rules remain authoritative. A
project's own landing recipe remains its own integration, and must explicitly support PR mode
before activation.

The board owns task identity, assessments, recorded runs and verdicts. The GitHub adapter owns
issue/PR publication and reconciliation through the [versioned tasks API](../../engineering/BOARD_API.md#the-tasks-api).
The landing policy/tool reads the authoritative assessment and stamp readers, controls GitHub
evidence enforcement and check publication, and owns merge execution and its durable receipt.
A transport helper may deliver an authorized snapshot's check writes with the publisher App
credential; it has no independent authority to assess, waive, approve or dispatch a merge.
The adapter never executes a command supplied by a PR.

Generic blocks and the existing gates vocabulary display the result. The prerequisite must
provide an authenticated evidence-export and landing interface through separately specified
versioned API additions; this document claims no such methods exist yet. The generic rendering
protocol alone is not a privileged execution interface. Provider success or workflow traversal
cannot substitute for the head-bound evidence decision or the remote merge receipt.

## Delivery operations

Enabled mode has two operations. Both are a PR that GitHub merges under the same required
checks; they differ in the merge method requested and the result the receipt verifies.

| Operation | Head H | Requested method | Verified result S |
| --- | --- | --- | --- |
| **Task** | A task's branch. | Squash. | One parent, the actual base; its tree proved against that parent and H. |
| **Upstream import** | A branch whose first-parent history from the base is a merge of one commit **U** of the configured upstream, then any review fixes. | Merge commit. | Two parents, the actual base then H; its tree equal to the merge of those two; U an ancestor of S. |

Squashing an import would deliver its files and discard U from the default branch's ancestry, so
the next import would merge the same upstream commits again. A repository that imports from an
upstream therefore never squashes an import, and one that does not import has the task operation
alone. An export toward the upstream is not a third operation: it reaches the upstream as an
ordinary task there ([the workflow](../../operations/UPSTREAM_DOWNSTREAM.md#a-selected-export)).

**What authorizes an import.** The operation is derived, never taken from a PR's title, body,
label or branch name, nor from a flag on the land command alone. All three of these must agree,
and the prepared binding and the evidence snapshot record the operation and U with H:

1. *Committed policy.* The default branch's policy names one upstream: host, immutable
   repository ID and default branch. With none named, the import operation does not exist in
   that repository.
2. *The assessment.* The independent assessment at H names the operation and U in full. An
   executor, an orchestrator or the PR's author cannot write it, as for a waiver.
3. *The graph.* From refs fetched afresh by the pinned identities, the landing tool checks that
   U is reachable from the upstream's default branch, that the last import (the merge base of
   the base and the upstream) is an ancestor of U, and that every commit of `B..H` off H's
   first-parent chain is an ancestor of U. A commit that is neither the branch's own nor the
   upstream's refuses.

**What refuses.** A head whose `B..H` holds upstream commits is never delivered as a task: with
an upstream named, the task operation checks for them and refuses, naming the assessor. A head
assessed as an import that fails the graph check refuses too. An import in a repository whose
policy names no upstream, or whose rules permit no merge commit, refuses and names the
administrative repair. Neither operation falls back to the other, nor to a local merge.

**Evidence is unchanged.** An import needs an assessment at H and every stamp it names, met or
explicitly waived, exactly as a task does; the operation waives nothing. Its reviewer reads the
merge against the automatic one and the commits after it, and its tests ran at H. A fix on the
branch is a new H with a new assessment. `symphony/eligible` succeeds only for a snapshot whose
operation, U and graph check are part of what was validated.

**What GitHub cannot bind.** GitHub's rules permit merge methods for a branch, not for one PR
according to a check. Where both methods are permitted, the method is the landing identity's
choice, bounded by the rule that lets only that identity update the default branch, and the
receipt's topology check finds a wrong one after the fact. This is a detection, not a
prevention, and its proof on a disposable repository is an activation prerequisite.

## Mode and publication

Three capabilities are independent: issue exchange, PR publication, and enforced PR landing.
All default off. Proposed configuration records the GitHub host, immutable repository ID,
default branch, publisher App ID and landing identity, alongside the selected capabilities.
These are conceptual fields, not loadable TOML keys. Read policy from the committed default
branch, never from the task branch. Enabling enforced landing requires PR publication and a
verified remote rule configuration. Disabling issue exchange does not disable enforcement.

Activation is an explicit administrative operation after implementation and scratch validation.
It verifies repository/remote identity, permissions, required check sources, merge method and
all default-branch writers. An absent plugin, expired authentication, renamed repository,
unreadable policy or changed protection makes enabled mode refuse and name the repair. It never
falls back to a local merge. Disable only after reconciling pending landings and deliberately
changing remote rules; a worktree setting cannot switch around those rules.

Enabling publication authorizes the integration's explicitly named task branches, whereas the
current local contract pushes only the default branch. Publish only the task's committed head,
after the owner-answer preservation checks. Bind one PR to one task/delivery attempt using
repository ID, PR number, head repository/ref and base ref. Persist that mapping before further
work. A user-editable PR body, title, label or issue link is not the binding. An existing PR must
match it exactly; ambiguity refuses instead of choosing by title. Initial enforced scope uses
branches in the configured repository; accepting fork PRs needs a separately tested import
path and never runs contributor code with publisher credentials.

## Evidence at the PR head

Call the complete published head SHA **H**, the current base SHA **B**, and the eventual squash
commit **S**. All assessment and stamp checks apply to H. Preserve the exact commit, not a
branch name, a timestamp, a PR synthetic merge ref or a shortened SHA. Before publication and
again before landing, require local task tip = remote branch tip = PR head H, the configured
base ref, and the bound repository IDs.

Fetch the authoritative remote default before reading its committed policy. A stale local main
cannot certify which policy is current. One durable repository delivery coordinator owns merge
requests and policy transitions across machines; separate worktree locks cannot provide this
serialization.

A versioned snapshot records task/delivery ID, H, assessment record ID and author, catalogue and
policy digests, selected stamps, every omission's reason, and each stamp's evidence IDs and
content digests. Runs include their exact commands, commit, result and recorder identity;
reviews include verdict ID, full approved SHA and freshness result; pictures include retained
attachment hashes; owner approval identifies the authenticated owner action and SHA. A stable
snapshot digest links the board record, GitHub check summaries and landing receipt. Publish
redacted result summaries and durable evidence references, never credentials or raw private
logs. A local-only evidence URL is not useful to a remote reviewer: the integration must publish
an authorized, retained summary/attachment or clearly mark the evidence as locally accessible.

PR titles and comments do not grant stamps. The existing review command remains the canonical
head-specific verdict; a plain GitHub approval is informational until a future authenticated
identity mapping can prove the board's independent-review rule. Mapping every agent session to
one human GitHub account would not prove independence. No second build is introduced by review.
The tests stamp exports the existing reader's recorded result; its assessment and reviewer judge
whether the named commands are sufficient. GitHub does not verify the truth of local execution.

Any new head requires a new snapshot and assessment at that exact head, followed by its needed
stamps. A force push, rebase, conflict resolution, generated-output correction or empty commit
changes H just as a code edit does. Keep old evidence attached to its old SHA for history, never
copy a green result to the new one. This PR transport deliberately accepts neither time-based
review coverage nor the local land's document-conflict carry exception: the remote gate requires
an explicit head binding.

Base movement alone does not stale H's task evidence or trigger another merge/build. Configure
checks without the up-to-date requirement, preserving the project's one-merge rule and leaving
combined-change verification to its release run. A changed catalogue or landing policy does
require a new snapshot against the new policy, even at unchanged H. Conflicts or incompatible
mandatory repository settings refuse. This distinction follows GitHub's documented
[strict and loose checks](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches).

## Checks GitHub requires

Use a dedicated GitHub App publisher, with check names namespaced by stable project ID where a
repository contains several projects. Configure a fixed required check for assessment, one for
each configured stamp, and one final eligibility check. The names below describe the default
catalogue; a changed catalogue requires an administrative rule migration before publishing
success. Keeping every stamp required remotely lets the assessor change needs without editing
branch protection for every task.

| Required check | Success means | Otherwise |
| --- | --- | --- |
| `symphony/assessment` | An independent assessment names H, the current policy/catalogue and an explicit needs set, including an explicit empty set. | Missing/stale/invalid assessment blocks even if defaults would permit a local land. |
| `symphony/tests`, `symphony/review`, `symphony/pictures`, `symphony/owner` | The stamp is met at H, or explicitly waived by that assessment with assessor, reason and record link. | Needed but missing, held, red or unknown blocks. |
| `symphony/eligible` | The complete consistent snapshot passed all stamp readers, identity checks and publication checks for this delivery. | Partial publication, pending updates, policy mismatch, draft/closed PR or an unresolved landing blocks. |

A waiver is a successful check titled **Waived by assessment**, with its reason and immutable
record reference; it never silently disappears. A present stamp says **Met** and names its
evidence. Never turn a needed red stamp into a waiver in the publisher, and never infer waiver
from an absent assessment. The assessor has the board's existing independence checks; an
executor, orchestrator, PR author or GitHub label cannot write an authorized waiver. Retaining
all assessment revisions makes a changed waiver attributable. No blanket emergency bypass is
introduced.

Use `queued`/`in_progress` while gathering a snapshot, `failure` or `action_required` for a
blocking outcome, and `success` only for met/authorized-waived checks. Do not use `neutral` or
`skipped` for an unknown or missing stamp: GitHub accepts them for required checks. Pin the
required contexts to the expected publisher App ID; an arbitrary status with the same name
must not satisfy them. These behaviors are documented in
[required status checks](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches).
Check-run writes require GitHub App credentials with Checks write permission; the
[Checks API](https://docs.github.com/en/rest/checks/runs) supports head SHA, external ID and
summary fields. Use a durable per-head check ID and generation record to reconcile retries;
create is not assumed to be idempotent. Do not create both a commit status and check run with
the same required name. No Actions workflow, hosted runner or GitHub-triggered test job is
needed.

### Publication freshness and the smaller first implementation

GitHub checks identify a commit and retain their published result; the merge request can match
the PR head. Those are documented platform features. Neither API offers a transaction with the
board's evidence records. The resulting limitation is an inference: a completed green check
can survive a publisher outage, and a same-head local evidence change can precede its remote
invalidation. A background mirror cannot promise instantaneous revocation.

**Recommended first implementation: trusted merge-time revalidation.** Immediately before
dispatch, the landing service reads the authoritative current head, policy, assessment and
stamp readers again, publishes their consistent snapshot, and reads back the source-pinned
checks. It compares the board's evidence revision with that snapshot when recording its merge
intent; a mismatch retries validation, not the build. A changed head gets no carried checks.
Only the trusted landing identity dispatches the head-matched squash, and every invalidating
event also queues a blocking remote update. An unavailable board, publisher or GitHub prevents
a new dispatch. A durable intent handles an already-dispatched request with an uncertain answer.

This smaller path meets the stated requirement that GitHub require an assessment and its
met/explicitly-waived stamps on the PR head, with a fresh local validation before dispatch. It
does not guarantee that a local withdrawal or new red arriving after the final validation can
cancel a request GitHub is already processing. GitHub checks the published state, not a live
board read. The receipt records the validated revision and request boundary so that this limit
is visible; a later change invalidates the checks or, after delivery, needs a correction/revert.
Repeated polling cannot remove that window. This is a proposed delivery policy, not an owner
approval of stronger consistency or an already-proved provider behavior.

**Stronger alternative, only if that additional guarantee is required:** seal an evidence
generation before making eligibility green. A new run, assessment/verdict change, owner-approval
withdrawal or policy activation must first publish and read back blocking eligibility, then
commit its invalidating record. Serialize those transitions and merge requests in one durable
coordinator, including every recorder and evidence writer. This closes the accepted-local-change
before-remote-invalidation race by delaying acceptance of the change; it cannot recall an
already-dispatched remote merge.

The stronger design makes GitHub availability a prerequisite for accepting those local writes
and starting another run at H. An outage leaves them pending while the sealed snapshot remains
authoritative; a timeout keeps it reserved until reconciliation proves the request's outcome.
An owner withdrawal must be recorded immediately as pending, with the in-flight limit shown.
It requires board-wide mutation changes, recovery of abandoned reservations and proof that no
writer bypasses the barrier. That cost is not justified by the stated remote-check requirement
alone. Keep it as an unapproved alternative, rather than an implementation prerequisite or a new
standing rule. Assess the smaller path first; a stricter consistency promise would be a separate
coordinated decision.

Both proposals need trusted check authorship and control of the merge writer. A proposed
ruleset arrangement separates (1) PR/check/squash requirements with no bypass
actors and (2) restricted default-branch updates, with only the landing App allowed through the
second ruleset. The App receives no bypass of the first. GitHub documents
[update restrictions and independent rules](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets);
the exact combined behavior must be proved on a disposable repository with the target account
and plan before activation. If it cannot be enforced there, enforced mode is unavailable;
merely documenting that users should avoid the merge button is insufficient.

## Permissions and protection

The publisher needs repository metadata/content/PR read and Checks write. A separate landing
credential needs only the contents and PR permissions required to publish its selected branches,
open/update PRs and merge them; GitHub documents Contents write for the
[merge endpoint](https://docs.github.com/en/rest/pulls/pulls#merge-a-pull-request).
Use installation-scoped credentials supplied by the prerequisite's credential provider. Neither
role receives repository administration, Actions/workflow write, organization administration or
stamp-author authority. Keep check-publishing credentials out of task builds and arbitrary
command environments. Splitting identities limits the ability of a compromised merger to forge
evidence; a compromised publisher/board remains part of the trust boundary.

Administrative setup requires PRs, the source-pinned checks above, squash as the permitted merge
method (and the merge commit as well only in a repository whose policy names an upstream; never
rebase, and no rule requiring linear history there), no routine direct default-branch pushes,
and protection against force push/deletion.
Require these for ordinary maintainers and administrators, and inventory every bypass rule,
token and overlapping ruleset. Repository administrators can change repository policy itself;
no check can constrain an administrator who removes its protection. Record and surface policy
drift, refuse subsequent lands, and never claim that boundary is tamper-proof.

Native required approval counts are not a substitute for the review stamp: a stamp may be
waived and board sessions have stricter freshness rules. Additional organization rules remain
additional requirements; do not silently weaken them. The initial transport supports ordinary,
unqueued PRs. Refuse a mandatory merge queue, auto-merge or stacked-PR path until their separate
head/group checks and receipts are implemented; queued acceptance is not delivery.

- [r] **github-delivery-policy-reader-standing** — The trial you approved was run and settled it: with Administration (write) the third App is shown both lists, and they are what they should be, nobody let past the evidence rule and the landing App alone let past the update rule. The App is back to Administration (read). GitHub shows those lists only to a reader with write access to the rule set ([its documentation](https://docs.github.com/en/rest/repos/rules#get-a-repository-ruleset)), and of the App permissions tried, the one that showed them, Administration (write), can also change or remove the rules. Should such an identity stand on the real repository, `heavenstudio/symphony`, so every land can read them? A: yes, the third App is installed on `heavenstudio/symphony` with Administration (write) and used at each land for that reading alone. B: no; a person confirms the two lists by hand when delivery is turned on and whenever the rules are changed, and every land checks everything else. C: neither, and pull-request landing stays off. Recommended: B, because the key that A keeps on the landing machine could remove the protection outright, which is more than the change it would be there to notice. B is not the same protection: a person's look takes the place of a reading at every land. Approve and say A, B or C in the note, or comment. [[analysis/github-pr-landing#Permissions and protection]]
  [2026-11-19 08:15] agent — Posted for the board task `tools/github-pr-verified-activation`, as **github-delivery-policy-write-trial** said it would be if write showed the lists.

    **What the trial showed.** One reading of each rule set on the throwaway repository, with a token limited to it: both answered with their list present, the evidence rule's empty and the update rule's naming the landing App alone. The token was revoked, the App was put back to Administration (read) and that was confirmed on its installation page. No rule was edited and the two other Apps were not touched.

    **Under A,** the machine that lands would hold a third key, and a token from it can edit or delete the rules of `heavenstudio/symphony`, which neither of the two approved Apps can. Symphony would use it for two reads at each land and nothing else, but of the GitHub App repository permissions tried, Administration (read) did not show the lists and only Administration (write) did, so the limit to reading would be Symphony's and not GitHub's. It needs a small change so the reading uses a token of its own, and its own task.

    **Under B,** every land still reads the rules in effect, their settings, the pinned check source and each App's own standing, as the two Apps can today. What goes unread is whether someone was added to a list after a person last looked. Adding someone takes write access to the rule sets, which a repository administrator has and another identity may be given, and which neither of the two Apps has. B replaces a reading at every land with a person's look when the rules change: a process to keep, not the same check, and a list changed without anyone saying so goes unseen until the next look. The check today refuses when it cannot read the lists, so B needs a change that lets it accept a recorded confirmation by a person in their place, and its own task.

    **Under every option** nothing is installed on `heavenstudio/symphony` and no permission changes with this card. Neither A nor B is built: each needs code of its own, in a task of its own, and proof with Symphony's own adapter against GitHub, and delivery stays off until then. The card is for `heavenstudio/symphony` alone: a consuming project's own repository keeps its own approval and administration.

    **Earlier verdicts checked.** **github-delivery-policy-write-trial**, approved as A, covers the one reading on the throwaway repository and says real use is asked separately, with B still open. **github-delivery-policy-reader**, approved as A, covers the read-only identity, which stays as it is. **github-delivery-proof-setup** gives the two Apps no Administration, which stays so under every option here.
  [2026-11-19 08:22] agent — Corrected before this card reached main, for the board task `docs/github-delivery-policy-reader-standing`: it names the real repository, `heavenstudio/symphony`; it says that no narrower grant showed the lists among the GitHub App permissions tried, not among every identity; it no longer says only an administrator can add someone to a list, since write access to a rule set is enough; and it says that B puts a person's look in place of a reading at every land, which is not the same check. That a rule set's list of who is let past is shown only to a reader with write access to it is [GitHub's documentation of reading a rule set](https://docs.github.com/en/rest/repos/rules#get-a-repository-ruleset).
  [2026-11-19 10:35] owner · approved — A
  [2026-11-19 11:05] agent — Taken as A for `heavenstudio/symphony` alone: a third App with Administration (write), used by Symphony for the two readings at each land and nothing else. It is built and proved in the board task `tools/github-pr-verified-activation`, by its executor, in commits of its own, not in a separate task as the card said: the proof of the delivery cannot be made without it. Installing that App on the repository is yours to do on GitHub when the executor asks; delivery stays off until the proof is posted and reviewed.
  [2026-11-21 18:13] agent · resolved

- [r] **github-delivery-policy-write-trial** — The read-only App you approved was made and tried, and GitHub does not show it the lists either: reading a repository's administration is not enough. The one level left to try is writing it, and an identity that can write a repository's administration can also change or remove the rules it is there to read. May that be tried, on the throwaway repository only? A: yes, the third App is given Administration (write) there, for one reading, to learn whether GitHub then shows the lists, and is put back to Administration (read) right after it. B: no; a person confirms the lists by hand when delivery is turned on, and lands check everything else. C: neither, and pull-request landing stays off. Recommended: A, because it risks only the throwaway repository and settles what GitHub does; it decides nothing for the real one. Approve and say A, B or C in the note, or comment. [[analysis/github-pr-landing#Permissions and protection]]
  [2026-11-19 02:45] agent — Posted for the board task `tools/github-pr-verified-activation`, as **github-delivery-policy-reader** said it would be if reading was not enough.

    **What was tried.** The third App, with Administration (read) and Metadata (read), installed on the throwaway repository alone, read both rule sets and the rules in effect. Neither list of who is let past was in its answers. Its own standing was: let past neither.

    **Under A,** the change is to that one App on that one repository. It is not installed anywhere else, the check publisher and the landing App keep exactly the rights they have, and nothing is granted until you answer. The trial is one reading with a token that is revoked after it, and the App is put back to Administration (read) as soon as that reading is done, so write lasts only that reading; it edits no rule. Whether write shows the lists is not known.

    **What A does not decide.** If write does show the lists, using it for real would mean an identity on the real repository that could remove its protection, present at every land. That would be a card of its own, with the trial's result on it, and B stays open then.

    **Under B,** every land still reads the rules, their settings and each App's own standing, which is what the two Apps can see today. What goes unread is whether someone was added to a list after the day a person looked.

    **Earlier verdicts checked.** **github-delivery-policy-reader**, approved as A, covers the read-only identity and says a write level is asked separately. **github-delivery-proof-setup** gives the two Apps no Administration, which stays so under every option here.
  [2026-11-19 07:42] owner · approved — A
  [2026-11-19 07:44] agent — A is approved for the temporary throwaway-repository trial, including immediate return to Administration read and token revocation. I attempted to reach the signed-in browser, but Chrome and Arc expose no accessible window and the browser connector is unavailable. No App permission or repository rule was changed and the trial has not run. It can continue under this same approval when the browser is accessible; no further permission decision is needed for the approved trial.
  [2026-11-19 07:44] agent · resolved
  [2026-11-19 08:14] agent — Your A trial is complete. On the throwaway repository only, Administration write showed both bypass lists: nobody bypasses the evidence rules, and only the landing App bypasses the update restriction. The token was revoked (GitHub returned 204), and I immediately restored Administration read; the installation page confirms read-only administration and metadata, with only the throwaway repository selected. No rule was edited and neither existing App changed. This proves write suffices here; it does not authorize keeping that power on a real repository. Production use remains a separate choice, with manual confirmation or leaving delivery off still available.

- [r] **github-delivery-policy-reader** — The two Apps you approved work on the throwaway repository: GitHub refused every merge it should and let the landing App's squash through. One thing they cannot do is read who is allowed past the rules. Before each land, Symphony is to check that nobody is let past the evidence rule and that the landing App alone is let past the update rule; GitHub shows that list to neither App, so the check answers "unknown" and a land refuses. May a third, read-only identity be added to read it? A: yes, a third App used for that reading alone, tried on the throwaway repository alone, with read-only permission that may or may not show the list. B: no third identity: a person confirms the lists by hand when delivery is turned on, and lands do not check them after. C: neither, and pull-request landing stays off. Recommended: A, because only a reading at each land notices a list changed later. Approve and say A, B or C in the note, or comment. [[analysis/github-pr-landing#Permissions and protection]]
  [2026-11-19 01:35] agent — Posted for the board task `tools/github-pr-verified-activation`, with the preflight's needs from `tools/github-delivery-preflight`.

    **What was measured** on the throwaway repository, with each App's own token and reads only:

    - Both Apps read every rule and its settings: the pull request, the check and the App it must come from, squash alone, no force push, no deletion, restricted updates.
    - Each reads its own standing: the check publisher is let past neither rule set, the landing App past the update rule only. That is as designed.
    - Neither is shown either list of who else is let past. An administrator's login sees them: empty on the evidence rule, the landing App alone on the update rule.

    **Under A,** the third App is registered as the two were, from your signed-in browser, installed on the throwaway repository alone, with no webhook. It never merges and never writes a check, and its key is kept as theirs are, outside every checkout and shown nowhere. Its permission is tried there before anything else: Administration (read) first. GitHub's documentation says the list is shown only to those who can edit the rule set, so read may not be enough. If only Administration (write) shows it, nothing is granted: that comes back to you as a card of its own, since it could change the rules it reads.

    **Under B,** the rules are still read at every land, and each App's own standing with them. What goes unread is whether someone else was added to a list after the day a person looked.

    **Earlier verdicts checked.** **github-delivery-proof-setup**, approved, covers two Apps and says neither gets Administration; a third identity that reads the repository's administration is not what it described, so it is asked here and changes nothing of the two.
  [2026-11-19 02:39] owner · approved — A
  [2026-11-19 02:45] agent — A carried out on the throwaway repository only. Created Symphony Policy Reader with Administration read and Metadata read, no webhook, installed only on symphony-delivery-proof. Its token read both rulesets successfully, but GitHub omitted both bypass lists; read-only permission is insufficient for the required check. The temporary token was revoked and the key is secured outside every checkout. No write permission was granted, neither existing App changed, and delivery remains off. Any trial of Administration write will be a separate decision card, as this option required.
  [2026-11-19 02:45] agent · resolved

- [r] **github-delivery-proof-setup** — Landing by pull request, which you approved, can be turned on only once GitHub is shown to refuse a pull request whose evidence is missing and to let nothing but the landing itself update the default branch. Showing that needs three things that do not exist yet: two small GitHub Apps in your organization, one that writes a task's evidence as checks and one that merges, and a public throwaway repository to try the rules on, so nothing is tried on the real one. Who may set them up? A: you register and install the two Apps, and an agent creates the throwaway repository with this machine's GitHub login, sets its rules and runs the proof. B: an agent does all of it on your word, the Apps included, from a browser session of yours signed in to the organization. C: you create the Apps and the repository and name them on the task. D: none for now, and pull-request landing stays off. Recommended: A, because the Apps' keys are lasting credentials of your organization, best held first by you alone, while the repository is disposable. Approve and say A, B, C or D in the note, or comment. [[analysis/github-pr-landing#Permissions and protection]]
  [2026-11-18 20:06] agent — Posted for the board task `tools/github-pr-verified-activation`, whose read of both repositories found no rule on either default branch and no App that could write the checks.

    **The two Apps,** both registered in the public repository's organization, with no webhook, installable on that account only and installed on the throwaway repository alone:

    - **The check publisher** writes the checks. Its permissions: Checks (read and write) and Metadata (read), and no other.
    - **The landing App** publishes a task's branch, opens its pull request and merges it. Its permissions: Contents (read and write), Pull requests (read and write) and Metadata (read), and no other.

    Neither gets Administration, Actions, Workflows or any organization permission. They stay two Apps so that what can merge cannot also write the evidence. One checks App with a person's token doing the merge is not the proof: a rule set can let an App, a team or a role through, never one person's token, so the second rule set below could not be tried. Each App's private key stays in a file outside every checkout, readable by its holder alone. The task needs each App's number and nothing else: no key, token or path to one goes on this card or on the board.

    **The repository:** public and in the same organization, so the rules are tried on the plan and visibility the real repository has, and holding a few scratch commits and nothing of Symphony's or anyone's private work. The login here cannot delete a repository, so removing it after the proof is yours under every option.

    **The rules proposed for its default branch,** two sets, made by whoever creates the repository:

    - **What every change needs,** with nobody on its bypass list: a pull request, the evidence checks accepted from the check publisher alone, squash as the only way to merge, no force push and no deletion.
    - **Who may update the branch:** nobody, but for the landing App, the one name on this set's bypass list.

    The landing App is let through the second set only, and neither App through the first.

    **What is tried there,** as none of this is proved until it is: that GitHub refuses the merge while a check is missing, failed or written by anything but the check publisher, the landing App's own merge included; that it refuses a push straight to the default branch and a merge pressed by a person, an administrator among them; and that the landing App's squash goes through once the checks pass. If the organization's plan does not enforce all of it, pull-request landing stays off.

    **Not asked here:** installing the Apps on the real repository, its rules and turning delivery on, which follow the proof as steps of their own. The private copy a consuming project keeps is its own organization's to set up, by its own decision.

    **Earlier verdicts checked.** **github-outbound-pr-delivery**, approved, and your word on `tools/github-pr-delivery` settle that tasks land by pull request with their evidence enforced on GitHub, and the landing design already has the two identities and the two rule sets, so none of that is asked. Neither says who may register an App or create a public repository in your organization, and no agent has been allowed to.
  [2026-11-19 00:58] agent — Corrected before this card reached main, for the board task `docs/github-delivery-proof-setup`: it first named one checks App and left the merge to a person's token, which could not prove the rules. Each permission is the one GitHub's documentation gives for the call it is for ([what each endpoint needs](https://docs.github.com/en/rest/authentication/permissions-required-for-github-apps), [only an App writes a check](https://docs.github.com/en/rest/checks/runs)), and the rules and their bypass lists are [GitHub's rule sets](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets).
  [2026-11-19 00:58] owner · approved — control my browser and do that step for me. Also, feel free to setup the throwaway repo.
  [2026-11-19 00:59] agent — Recorded as B: on your word an agent registers and installs the two Apps from your signed-in browser and creates the throwaway repository. Nothing is set up by this card's own task, which only publishes it: the setup and the proof are the board task tools/github-pr-verified-activation's, told of your answer on its thread. Each App's key is saved outside every checkout, readable by you alone, and shown nowhere. The proof also tries what a person can still merge by hand after a task's evidence is withdrawn, which should be nothing once updates are restricted. Removing the repository afterwards stays yours, as the login here cannot delete one.
  [2026-11-19 01:15] agent — Created the public throwaway repository [heavenstudio/symphony-delivery-proof](https://github.com/heavenstudio/symphony-delivery-proof), with only a scratch README and squash as its only merge method. Your approval covers the two Apps and their proof-repository installations; no further decision is needed. The signed-in browser reached the App registration form, but concurrent browser interaction keeps switching it away and interrupts control. App creation, protected-branch rules and the proof are still pending; no App or key has been created, and the real repositories are unchanged.
  [2026-11-19 01:15] agent · resolved

## Landing and recovery

Preparation and checks remain outside the short repository landing lock. All operations use
explicit repository and PR IDs. The durable state progression is:

| State | Action and recorded boundary |
| --- | --- |
| Prepared | Bind H, task/PR, operation (and U for an import), policy, snapshot and intended commit title/body; keep task ready for delivery, not landed. |
| Eligible | Revalidate the authoritative snapshot and its evidence revision, read back every check from the expected App at H, and record the matching intent. Recheck remote rules, owner-answer preservation and the local/remote head binding. |
| Merge requested | Persist the request intent before invoking `gh pr merge NUMBER --repo HOST/OWNER/REPO --squash --match-head-commit H --subject TITLE --body-file FILE`, with `--merge` in place of `--squash` for an import. Do not use admin, auto-merge or branch deletion. |
| Remote merged | Read the PR's merged state and returned commit S, fetch it, verify repository/base/head provenance, the resulting tree and the operation's topology, then persist a receipt. A command exit alone is not proof. |
| Reconciled | Fast-forward a clean local default checkout to the fetched remote default, close the task with H → S and the snapshot reference, then dispatch post-land work exactly once. |

The [CLI's head match and squash options](https://cli.github.com/manual/gh_pr_merge) pin the
request to H. GitHub's merge API also rejects a mismatching head. They do not pin B. Record B
observed before the request and the actual parent of S separately; prove the squash tree against
that actual parent and H. Do not claim local tests at H ran at S or on its combined base.

If GitHub refuses a conflict, changed head, missing check or permission, keep the task unlanded
and name its responsible role. A new head restarts assessment/evidence. A rate limit or transient
failure uses bounded backoff respecting the response and retains the same intent. Lost responses
and crashes first query the bound PR: merged at the expected H means adopt S; open means retry
only after revalidation; closed-unmerged, merged with an unexpected head, or merged in another
shape than its operation's means hold for
reconciliation. Never create another PR or retry by merging locally to resolve uncertainty.

A merge in the wrong shape is repaired forward, never by rewriting the default branch. An
import that was squashed has delivered its files without U: its task stays unlanded, the
receipt records the wrong-method commit, and a new import of the same U, whose merge changes no
file, restores the ancestry under its own assessment. A task merged as a merge commit has put
its branch's commits on the default branch; its receipt records that, and the hold names the
landing identity's fault for a person to judge.

The receipt includes repository/PR IDs, task, H, actual base parent, S and tree ID, assessment and
snapshot IDs, check IDs/generation, actor and reconciliation state, with the operation and, for
an import, the upstream's repository ID, U, every parent of S and the merge base with the
upstream before and after. Squash removes H from S's
ancestry, so the board's gates and sweep must read this receipt rather than expect a merge's
second parent; an import keeps H as that parent, and the receipt is still what they read. Retain the source commit and evidence before deleting a branch, and never infer
landing from subject text or patch similarity. A manually merged PR lacking the expected receipt
requires provenance reconciliation; it must not fabricate stamps after delivery.

A dirty/diverged local default checkout after remote success is **landed remotely, reconciliation
pending**. Do not reset, overwrite owner answers, merge again or push local main over GitHub.
Retry fetch/fast-forward and board closure separately. Post-land dispatch uses a durable receipt
key so a retry cannot count a landing twice or start duplicate release/regeneration work. The
remote default branch is authoritative in PR mode; the local land no longer pushes it.

Use an explicit squash title/body and the repository owner's authorship policy. No agent credit
or generated co-author trailer is added; the task, head and evidence belong in the receipt.

Existing `[verdicts] check` on a locally constructed merge and git commit hooks do not run inside
a GitHub squash operation. Activation must inventory their promises: checks needed by policy
become recorded local evidence before eligibility, and required commit metadata/signing must be
enforced and verified through the remote operation. Never silently lose a consumer's merge
check. This repository's disabled land check introduces no new full-suite task gate.

## Simple documents and other writers

Recommended initial boundary: direct local delivery remains available only with enforced PR
mode off. With it on, even a simple document uses a PR, with an explicit empty-needs assessment
where appropriate. The requirement allows retaining a simple-document exception; it does not
require one. No new owner choice is necessary to omit that optional bypass from the first
implementation.

If retained later, its only candidates are regular Markdown documents under the configured docs
entrypoint, whose parsed change merely files a task or records an owner answer. Exclude agent
guides, rules, skills, scripts, settings, executable modes, symlinks, submodules, policy/check
changes, generated outputs, and mixed commits. File extensions alone cannot prove this semantic
boundary, and an empty stamp set does not turn code into a simple document. A constrained writer
would have to authenticate the owner action, validate the full diff against an explicit policy,
preserve history and use compare-and-swap on the remote base. Refusal routes the change through
a PR. Native bypass permissions do not inspect that diff, so granting ordinary agents or the
landing identity a general direct-push exemption is not an implementation of this exception.

Inventory every writer before activation: task lands, upstream imports, document saves/commits,
task filing that writes Git, the regenerator, release fixes and manual tools
([each, in either mode](../../operations/UPSTREAM_DOWNSTREAM.md#every-writer-of-a-default-branch)). A local edit may remain local until its PR is
delivered; the app must show pending delivery and preserve it through retries, never report a
local save as remotely landed. Generated-client changes use their own assessed PR and do not
recursively trigger another generation when inputs are unchanged. Since GitHub cannot apply
the local land's keep-main-generated-output rule, correct unwanted generated changes on the task
branch before publishing its final H, then gather evidence there.

A release-candidate fix keeps its candidate-based source H and local evidence. It can squash
into main through its PR while the candidate advances to the retained H, rather than taking S
and unrelated newer main changes. Record both destinations in one recovery receipt; verify the
candidate has not changed incompatibly, advance it without force, and dispatch the release
rerun only after both required destinations are reconciled. Candidate state remains local,
never published as a branch by implication. A failure after the main merge is an outstanding
candidate reconciliation, not another main merge. Release tags and publication retain the
release run's authority. This dual-destination path needs focused proof before activation in a
project using release candidates.

## Implementation boundary and proof

The concept changes no production code. After the GitHub prerequisite lands, implement a small
shared evidence snapshot/receipt contract first, then the authenticated publisher and trusted
merge-time revalidation, then the hosted landing seam and all default-branch writers. Preserve
public consumer APIs under the two-release rule. Use independent assessment/review of the real
diff; this proposal grants no activation, remote merge or settings change.

The focused acceptance matrix for that implementation is:

| Area | Cases that must be proved |
| --- | --- |
| Mode/policy | Disabled keeps current behavior; enabled with missing plugin/auth/source protection refuses; task-branch policy cannot weaken the gate; custom lands and all default writers are accounted for. |
| Assessment/stamps | No assessment, stale assessment, forged author, missing/default stamp, needed red, owner hold, explicit empty needs and individually reasoned waivers. Same-named foreign-App checks cannot satisfy protection. |
| Identity/freshness | Wrong repository/base/PR, duplicate binding, local/remote disagreement, head race at merge, conflict resolution, rebase/force push and policy change. Base movement alone keeps valid H evidence. |
| Publication | Partial writes never publish eligibility success; duplicate/out-of-order events, reconnect, App outage, changed evidence before dispatch and a withdrawal during an uncertain request. Demonstrate the stated same-head propagation window; do not claim instantaneous revocation. No arbitrary commands from remote content. |
| Operation | Import with no upstream in policy, no assessment naming it, a foreign commit off the first-parent chain, or a U the upstream does not hold; a head holding upstream commits asked as a task; an import squashed and a task merged by commit, each held and repaired forward; repeated imports advance the merge base; an export arrives as a task with no private ancestry. |
| Delivery | Squash provenance and actual tree, an import's two parents and merged tree with the base moved, successful merge with lost response, closed-unmerged PR, dirty/diverged local main, board write failure, repeat retry, manual merge and retained H after branch deletion. Exactly one closure and post-land dispatch. |
| Other writers | Pending owner answer preserved, mixed/generated diff refused or corrected before evidence, regenerator PR without a loop, candidate/main partial delivery and no force move. Optional direct-doc writer stays disabled. |
| Protection | Disposable-repository API and UI attempts with missing/waived/present checks, untrusted status source, ordinary direct push, bypass-capable maintainer and landing App. Prove the App bypasses only update restriction, never PR/stamp requirements. |

Unit tests cover snapshot validation, waiver/freshness decisions and recovery transitions with
mocked transport; CLI tests cover refusals on scratch boards. A disposable GitHub repository
proves provider enforcement that mocks cannot. Keep builds local, collect coverage of the new
units, and run the changed packages' focused checks plus the project's lint. Do not substitute
hosted CI or the full release suite. Until those proofs and prerequisite contracts exist, PR
landing remains an unactivated design.
