# Upstream and downstream repositories

How changes move between the public Symphony repository and an independent private copy of it
that a consuming project's organization keeps: a regular **import** of upstream into the
downstream, and an **export** of selected reusable changes back. Both are manual tasks a
maintainer files and works; detecting that an import is due, or that a change is worth
exporting, is not automated.

This page creates nothing: no repository, pull request, grant, schedule or setting. The Git
steps work with today's landing. What it says of pull requests holds only where
[PR delivery](../workflow/analysis/github-pr-landing.md) is implemented, proved and enabled, which it is not
yet ([before PR delivery is enabled](#before-pr-delivery-is-enabled)).

## The two repositories

| | Upstream | Downstream |
| --- | --- | --- |
| What it is | The public repository, `heavenstudio/symphony`. | A private repository of the consuming project's organization, with history of its own on top of upstream's. |
| Who writes | Its own maintainers, by its own landing. | The consuming project's maintainers, by its own landing. |
| What it may hold | Only what is fit to be public. | Upstream's history and private work. |
| Its board | Its own, with public task text. | Its own; private tasks stay there. |

The downstream is not a GitHub fork and not a snapshot: its default branch contains an upstream
commit as an ancestor, so the two share history and Git can tell what the downstream already
has. `git merge-base origin/main upstream/main` in the downstream clone names that commit, the
**last import**. A downstream with no common ancestor is refused
([below](#refusals-and-recovery)).

A maintainer keeps **two separate clones**, one per repository, each with its own repository as
`origin`:

- **The downstream clone.** `origin` is the downstream. Upstream is a second remote that can
  only be read, its tags kept apart from the downstream's own release tags, which have the same
  names:

  ```sh
  git remote add upstream https://github.com/heavenstudio/symphony
  git remote set-url --push upstream DISABLED-upstream-is-read-only-here
  git fetch --no-tags upstream main '+refs/tags/*:refs/upstream-tags/*'
  ```

- **The upstream clone.** `origin` is upstream, and it has no other remote. It never fetches
  from the downstream, so it holds no private commit and cannot push one.

Nothing private goes into the upstream clone but a patch file a person has read
([a selected export](#a-selected-export)). No worktree of one clone stands in for the other.

## What moves, and how

| Movement | From → to | Carries | Lands as |
| --- | --- | --- | --- |
| Import | Upstream → downstream | Every upstream commit since the last import, with its ancestry. | A merge commit: upstream's commit stays an ancestor of the downstream's default branch. |
| Export | Downstream → upstream | One reviewed, reusable change, rewritten as new commits on upstream's tip. No private commit. | An ordinary task of the upstream, by the upstream's landing. |

The two are not symmetric on purpose. An import must keep ancestry, or every later import
merges the same upstream commits again and meets the same conflicts again. An export must drop
ancestry, or the private history behind the change is published with it. Private `main` is
never merged into upstream, and no upstream branch is ever cut from it.

## The import

Once a week, and at once when the downstream needs an upstream fix, a maintainer files one task
on the downstream's board, its branch named as its slug (`sync/upstream-2026-11-18`), and works
it in the downstream clone.

1. **Name what is imported.** Fetch both remotes as above. **U** is one full upstream commit
   SHA: the newest upstream release tag by default (`refs/upstream-tags/release-…`), a later
   commit of upstream's default branch when the fix needed is not released yet. The task names U
   and the last import, and `git rev-list --count origin/main..U` is how many commits arrive.
   Zero means there is nothing to import, and the task is cancelled.
2. **Check it can only go forward.** `git merge-base --is-ancestor LAST_IMPORT U` must exit 0:
   U continues the history the downstream already has.
3. **Merge, never squash or rebase.** On the task's branch, cut from `origin/main`:
   `git merge --no-ff U`. The branch tip **H** is a merge whose first parent is the downstream's
   commit and whose second is U.
4. **Settle conflicts on that branch** ([below](#conflicts-and-review)), then run the checks
   closest to what the import changed and the lint, as for any task
   ([CHECKS](../workflow/CHECKS.md#the-gate-is-evidence)).
5. **Post the evidence and move to review** ([below](#import-evidence)). Where the downstream
   lands by pull request, its assessor names the import at the head it read, beside the stamps:
   `just tasks needs SLUG STAMP… --tip H --import U`, U in full. Without it, or named at another
   head, the land judges H as a task and refuses it
   ([TASKS](../workflow/TASKS.md#an-import-delivered-by-a-pull-request)).
6. **Land it** by the downstream's landing ([how each lands](#how-each-lands)), then check the
   result: `git merge-base origin/main upstream/main` now answers U.

Upstream moving while the import is in review changes nothing: U stays the commit the task
named, and what upstream gained waits for the next import.

### Conflicts and review

- **The merge is the change under review.** Upstream's commits were reviewed upstream; what the
  downstream's reviewer reads is how they met the private work: `git show --remerge-diff H`
  shows the merge against what Git would have merged automatically, and any commit after it on
  the branch is a fix. The tests at H are the evidence that the two work together.
- **Private behaviour is kept deliberately, never by accident.** Where upstream changed
  something the downstream changed too, the resolution says on the thread which side it took
  and why. A private variant worth keeping upstream is a candidate export, not a permanent
  conflict.
- **Generated outputs take the downstream's side.** No branch commits a client distribution
  ([the distribution](../../AGENTS.md#the-clients-distribution)): a conflict under
  `apps/symphony-server/client/public/` resolves to the downstream's side, and the
  [regenerator](../workflow/TASKS.md#the-regenerator) rebuilds it from the merged sources after the land.
- **Records with an owner's answers keep them.** A conflict in the decision register or another
  document that holds dated lines keeps every line of the downstream's; upstream's cards arrive
  beside them.
- **What decides landing is read closely.** An import can change the project file, the rules,
  the guides, the skills, the hooks and the recipes, which decide what the downstream runs and
  what lands there. Its assessor reads those paths' diff whatever else the import holds, and a
  setting the downstream chose stays as it chose it unless the task says otherwise.
- **A release candidate is left alone.** An import lands on the default branch only. It is
  never merged into the downstream's [release candidate](../workflow/RELEASES.md#the-release-candidate), and
  a red's fix that happens to exist upstream is still that red's own task. Upstream's candidate
  is a local branch that is never pushed, so no import carries it.

### Import evidence

The move to review posts, on the task's thread:

- the upstream repository, U in full and the release tag it is, where it is one;
- the last import, and the count of commits between them;
- H, and its parents (`git rev-list --parents -n1 H`);
- each conflicted path with the side taken, or "no conflicts";
- the commands run at H and their results, recorded as any task's are.

After the land: the default branch's commit **S**, its parents, and the merge base that now
answers U.

## A selected export

A change is exported when it is useful to every project and says nothing private. The
downstream's maintainer chooses it; nothing is exported because it merely exists. It takes two
tasks, one on each board, and both clones.

1. **Record the choice privately.** A task on the downstream's board names the private commits
   the change comes from and why it is reusable. This is where the private context stays.
2. **Make a patch, outside both clones.** In the downstream clone,
   `git diff A B -- PATHS > FILE`, written to a folder that belongs to neither. A diff, not
   `git format-patch` and never `git fetch` or `git cherry-pick` across the clones: those carry
   private commit messages, authors or objects.
3. **Read the patch for disclosure** ([below](#disclosure-checks)). The maintainer's go-ahead
   to publish it is recorded on the private task before anything is pushed: a pushed branch
   cannot be recalled.
4. **File the public task** on the upstream's board, in words written for the public: what the
   change does and why upstream wants it, with no private task, thread or name.
5. **Apply it on upstream's tip.** In the upstream clone, on the task's branch cut from a
   freshly fetched `origin/main`: `git apply --index FILE`, then a commit whose message is
   written there, anew.
6. **Prove the branch is clean** ([below](#export-evidence)), and run the upstream's own checks
   for the change: its closest tests, its lint, and the hook test that refuses a consuming
   project's name in any tracked file (`own_names`).
7. **Review and land it as any upstream task**, under the upstream's rules, by a reviewer who
   did not write it.
8. **Close the loop privately.** The private task records the upstream commit the export landed
   as. The next import brings it home: where the downstream's copy and upstream's are the same
   change, the merge changes nothing; where review changed it upstream, that is an ordinary
   import conflict, resolved to upstream's version unless the task says why not.

### Disclosure checks

Before the go-ahead, the patch and the words that go with it are read for:

- the consuming project's name, its organization, people, customers, hosts and paths, in code,
  comments, tests, fixtures and file names;
- credentials and anything shaped like one, with the scanner the
  [public-readiness audit](analysis/public-readiness.md) used, on the patch and on the commit
  message; a clean scan is not clearance by itself;
- private planning: task slugs, thread text, decisions and review notes of the private board;
- code that only makes sense with private code the patch does not carry, which is not reusable
  yet;
- the commit's author and committer, which are the public identity the upstream's
  [rules](../../RULES.md) ask for (`commits/no-ai-authorship`).

A doubt is a refusal: the change stays private until it is rewritten.

### Export evidence

In the upstream clone, with **X** the export branch:

| Check | Command | Must answer |
| --- | --- | --- |
| One remote | `git remote` | `origin` alone |
| Cut from upstream's tip | `git merge-base origin/main X` | The freshly fetched tip of `origin/main` |
| Only the export's commits | `git rev-list --count origin/main..X` | The number of commits written in step 5 |
| No merge | `git rev-list --count --merges origin/main..X` | `0` |

And across the clones, by lists of SHAs, never by a fetch: `git rev-list origin/main..X` in the
upstream clone, and `git rev-list origin/main --not upstream/main` in the downstream clone (its
private commits), each sorted to a file outside both. `comm -12` of the two must print nothing.

The private task's thread keeps those answers and the go-ahead. The public task's thread keeps
the usual evidence of a task, and nothing that names the downstream.

## How each lands

**With PR delivery off**, which is how every project lands today, nothing here needs GitHub
beyond a place to push: the [default landing](../workflow/TASKS.md#the-default-landing) merges the task's
branch into the default branch with a merge commit and pushes it. An import's branch lands that
way with U still an ancestor, and an export lands in the upstream as any of its tasks does.
This path stays available wherever PR delivery is not enabled.

**With PR delivery on**, GitHub performs every merge and the local checkout only follows:
there is no local merge of the default branch and no push of it, for an import as for anything
else. The two movements use the two
[delivery operations](../workflow/analysis/github-pr-landing.md#delivery-operations):

| | Where | Operation | GitHub merges by | The result must be |
| --- | --- | --- | --- | --- |
| Ordinary task | Either repository | Task | Squash | One commit on the actual base. |
| Export | Upstream | Task | Squash | One commit on the actual base, like any upstream task. |
| Import | Downstream | Upstream import | Merge commit | A commit with two parents, the actual base and H, with U its ancestor. |

The import is the only delivery that is not squashed. It passes the same gate as a task: an
independent assessment at H, every stamp it needs met or explicitly waived there, the checks
published by the trusted source, and a fresh validation before the request. What differs is the
merge method asked of GitHub and the shape the receipt verifies. The landing tells the two apart
by three things that must agree: the downstream's committed `[land.upstream]`, its assessment at
H naming U (`--import`), and the history fetched from the upstream then; GitHub's side pins the
upstream by its number (`upstream_id`). A head that holds upstream commits and is not delivered
as an import is refused, and an import the host squashed is held
([TASKS](../workflow/TASKS.md#an-import-delivered-by-a-pull-request),
[GITHUB](GITHUB.md#delivering-an-import)).

## Refusals and recovery

Each of these stops the work where it stands. None is settled by a force push, a rewrite of a
default branch, a local merge pushed in GitHub's place or `--allow-unrelated-histories`.

| What is found | Where | What happens | How it is put right |
| --- | --- | --- | --- |
| Evidence missing, red, or of another commit than H | Either | The landing refuses, naming who gives what is missing. A fix on the branch is a new H. | Assessment and stamps again at the new H; nothing is carried from the old one. |
| `origin` is not the expected repository, or (PR delivery) the pinned repository identity does not match | Either clone | Nothing is fetched into the work, pushed or merged. | Work in the right clone. A wrong remote is corrected by hand, by the maintainer. |
| A push to upstream from the downstream clone | Downstream clone | Git refuses: the push address is disabled. | Nothing to recover. The export procedure is the way. |
| An import head asked to land as a task, or squashed | Downstream | PR delivery refuses a task whose head holds upstream commits its base lacks, naming the newest one. | Its assessor names it an import at H: `just tasks needs SLUG STAMP… --tip H --import U`. |
| An import was squashed anyway, by any route | Downstream | Found after the fact: S has one parent, and the merge base still answers the earlier import. With PR delivery it is held for reconciliation, not closed as landed. | A new import task of the same U. Its merge changes no file and restores the ancestry; it is reviewed and landed as an import. History is not rewritten. |
| Private ancestry in an export: a merge in `origin/main..X`, a base that is not upstream's tip, or a SHA shared with the private list | Upstream clone, before any push | The export is not pushed. | Delete the branch and apply the patch again on a fresh branch in the upstream clone. |
| A private commit or branch reached upstream | Upstream | It is disclosed: deleting the branch does not withdraw what was fetched or what a pull request still names. | The maintainer is told at once and decides; anything secret in it is revoked first. No agent rewrites upstream's history. |
| Upstream was rewritten: the last import is no longer an ancestor of upstream's default branch | Downstream clone, step 2 | The import stops before any merge. | A person finds out why. The downstream keeps the history it has; following a rewritten upstream is a decision, not an import. |
| No common ancestor | Downstream clone | Git refuses the merge. | The downstream is seeded again from upstream's history, by its owner, before any import. |
| The downstream's default branch moved during review | Downstream | Nothing by itself: the import does not chase it. A real conflict at landing is refused. | One more merge of the default branch into the task's branch, a new H, and its evidence again. |
| A local merge interrupted | Either clone | The branch is unfinished and nothing is published. | `git merge --abort` and start the step again, or finish the resolution. |
| A landing interrupted, PR delivery off | Either | The land says where it stopped. | Run the land again, which finishes it. |
| A merge requested and the answer lost, PR delivery on | Either | The task stays unlanded here until the bound pull request is read. | Merged at H in the expected shape: adopt S. Still open: validate again, then ask again. Anything else: hold for reconciliation. Never a second pull request and never a local merge. |
| Merged on GitHub, the local default branch dirty or diverged | Either clone | Landed remotely, reconciliation pending. | Fetch and fast-forward only; the local side is repaired, never pushed over GitHub's. |

## Every writer of a default branch

PR enforcement holds only if nothing else writes the default branch. With PR delivery on, each
of these goes through a pull request and its checks, or is refused; with it off, each works as
it does today.

| Writer | PR delivery off | PR delivery on |
| --- | --- | --- |
| A task's land | Local merge commit, then a push. | Task operation: squash by GitHub, then local reconciliation. |
| An import | The same local merge, U kept as an ancestor. | Upstream import operation: merge commit by GitHub, then local reconciliation. |
| An export | A task's land in the upstream. | Task operation in the upstream. |
| The regenerator's client distribution | A commit of its own after a land. | A pull request of its own, assessed, with no second generation while its inputs are unchanged. |
| A release candidate's fix | Lands on the default branch and advances the local candidate. | Squashed into the default branch by its pull request while the candidate advances to the retained head, under one receipt. |
| A release | Pushes a tag alone, never a branch. | The same: a tag is not a write of the default branch, and stays the release run's. |
| A document save, an owner's answer, a task filed into Git | Commits as today. | A pull request, with an assessment that may need no stamp. No direct write remains: a document save is kept pending and delivered by a task of its own ([TASKS](../workflow/TASKS.md#a-document-save-under-delivery)). |
| A person or a tool pushing by hand | Possible, and outside every rule here. | Refused by the repository's rules for everyone but the landing identity. |

The detail of each row with PR delivery on, and the proof it needs, is
[the landing design's](../workflow/analysis/github-pr-landing.md#simple-documents-and-other-writers).

## Before PR delivery is enabled

Enabling it is a separate, explicit act in each repository, after its implementation exists.
For these two movements it also needs:

- **The downstream seeded from upstream's history**, so a common ancestor exists, and both
  repositories' identities pinned in the downstream's committed policy: its own (`repository`),
  and the upstream it may import from, by its address in `symphony.toml`
  (`[land.upstream]`) and by its number in `symphony-github.toml` (`upstream_id`).
- **The downstream's rules permitting merge commits as well as squash** for the landing
  identity alone, and no rule requiring linear history. The upstream needs squash alone.
- **Proof on disposable repositories** that the rules hold as the design says: an import merged
  as a merge commit with every check required, an attempt to squash it and an attempt to merge a
  task refused or held, and no route for anyone else to write the default branch.
- **The receipt's shape checks** run against real merges of both kinds, a lost answer and a
  wrong-method merge among them.
- **Every writer above** shown to go through a pull request or be refused.

What stands behind this page today is narrower, and is all it claims. The Git steps and each
topology check were run on scratch repositories on one machine: repeated imports move the merge
base forward and bring only new commits, a squashed import is detected and repaired by a second
import that changes no file, an export cut in the upstream clone shares no commit with the
private list and comes home without conflict, tags stay apart, and a rewritten or unrelated
upstream is refused. The import's delivery by pull request is built and tested against a host
that merges scratch repositories as asked, and the adapter's checks against an in-memory GitHub
(`crates/tasks/tests/sweep/land_delivery/import.rs`,
`crates/symphony-github/tests/exchange/import.rs`). No GitHub repository, rule, check or pull
request was exercised, so nothing here shows that GitHub enforces any of it.
