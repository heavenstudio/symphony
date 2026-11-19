# GitHub issue exchange and pull request delivery

`symphony-github` exchanges selected tasks with the issues of one GitHub.com repository, and
answers [a land delivered by a pull request](#delivering-a-land) there. It is optional and turned
off: a project that never commits its settings file runs none of it, and its
board, its pages and its landing are as they are without it.

It is a program of its own (`crates/symphony-github`). It reaches the board through
[`tasks api`](TASKS.md#the-tasks-api) and nothing else, GitHub through `curl`, and keeps what it
must remember in the folder the board names for it. No other package of the workspace builds with
it, which `crates/symphony-github/tests/boundary.rs` holds. Nothing starts it but an operator,
and a land where the project's `[land] delivery` names it: there is no schedule, no webhook and
no page that runs it. The design it follows, and what of
that design is not built, is [the proposal](analysis/github-integration.md).

## Turning it on

The settings are `symphony-github.toml` at the root of the tree the project's default branch
commits (`[project] default_branch` in the main checkout's `symphony.toml`, `main` unless it
says another). The adapter reads that file from git's objects, never from a checkout: a file a
branch commits, one in a linked worktree and one not committed turn nothing on. A file that is
absent is the adapter turned off. One that does not read (a key it does not know, a value of
another type, settings that do not hold together) is refused by name, and every command reads a
refusal as turned off.

| Key | Default | What it sets |
| --- | --- | --- |
| `enabled` | `false` | whether anything is read from or written to GitHub |
| `repository` | none | `"OWNER/NAME"` on GitHub.com, the one repository reached; never a URL or another host |
| `issues` | `"off"` | `"import"`, `"export"` or `"exchange"` (both) |
| `pull_requests` | `"off"` | `"publish"` answers [a land's delivery](#delivering-a-land); `"track"` is read and held to its word, and this build tracks none and says so |
| `token_file` | none | the absolute path of a file holding the token |
| `import_labels` | `[]` | the labels that select an open issue for import |
| `import_comments` | `false` | whether the comments of a mapped issue become notes on its task |
| `export_label` | none | a label put on each issue the adapter creates |
| `checks_token_file` | none | the absolute path of a file holding the token that writes a delivery's checks, never `token_file` |
| `checks_app_id` | none | the number of the GitHub App that token is of: a check by any other is not read as the adapter's |
| `landing_app_id` | none | the number of the GitHub App `token_file`'s token is of where a land is delivered: the one actor [the default branch's rules](#the-protection-a-delivery-needs) let update it, never `checks_app_id` |
| `close_issues` | `false` | whether a delivered task's mapped issue is closed once the board closed the task |

`enabled = true` needs `repository` and `token_file`, an `issues` that imports needs
`import_labels`, and `pull_requests = "publish"` needs `checks_token_file`, `checks_app_id` and
`landing_app_id`. The settings are read again before every operation that writes GitHub or the
board, so a landing that turns the adapter off stops a run at its next dispatch.

The issues are listed from where the last listing ended, and that place holds only under the
settings it was read under. After a landing that changes them, the next preview lists every
issue again, so an open issue that carried a label before the label selected it is imported
without anyone editing it.

## The token

The settings name where the token is, never the token. The file is outside the repository (its
checkouts and its git folder), a plain file readable by its owner alone (`chmod 600`), and holds
one token. Any of these not so is refused before a request is made, in a line that shows nothing
of the file.

`curl` reads the whole request from its standard input: the URL, the headers with the token
among them, and the body. So the token is in no argument, no URL and no environment variable,
and `-q` keeps a `~/.curlrc` from adding to the request. No redirect is followed. What `curl`
says when it fails is redacted before it is printed or journaled, and nothing the adapter keeps
holds the token. A machine without `curl` on `PATH` is told so, and nothing is sent.

A fine-grained token scoped to the one repository is enough: issues read for `import`, issues
read and write for `export` and `exchange`. A token that GitHub refuses (401) pauses the
exchange with everything kept; the next preview that succeeds lifts the pause.

A delivery uses two tokens, each held by the same rules. `token_file`'s opens and merges the pull
request and reads [the protection](#the-protection-a-delivery-needs): an installation token of
the GitHub App `landing_app_id` names, with pull requests read and write, contents read and
write, and issues write where `close_issues` is set, and no administration. `checks_token_file`'s writes the checks: an installation token of the one
GitHub App `checks_app_id` names, with checks read and write and nothing else. GitHub takes a
check from an App alone, so what merges cannot say the evidence stands. The adapter mints no
token: whoever installs the App keeps a current one in the file. The head itself is pushed by the
checkout's own git, with the credentials a person's `git push` there uses.

## The commands

Each is run in a checkout of the project. `--tasks PROGRAM` names the board's program, which is
otherwise the `tasks` beside `symphony-github` or the one on `PATH`.

- **`status`** says what the committed settings allow and what is kept. It reads no remote and
  writes nothing, not even its folder.
- **`publish SLUG --title TITLE --body-file FILE`** opts a task in: TITLE and FILE's text are
  kept as its curated public text and shown as they will be published. Nothing is sent.
- **`preview [--rebind]`** reads both sides and shows what a reconcile would do, with the exact text of
  every write, then keeps that preview under the digest it prints. It writes nothing to GitHub
  or the board. Turned off, it reads nothing from GitHub and shows only the curated texts that
  wait. `--rebind` binds to [another repository found at the name](#what-pauses-or-suspends).
- **`reconcile --preview DIGEST --as HANDLE`** applies the preview of that digest and no other:
  at most 50 operations a run, or `--limit N`. HANDLE is the board session the board's writes
  are made as.
- **`resolve SLUG local|remote`** keeps one side of a conflict, or takes up a suspended mapping
  again. It writes only the adapter's own folder; the next preview shows what follows.
- **`preflight [--branch BASE]`** reads [the protection](#the-protection-a-delivery-needs) of
  BASE, the default branch unless named, as a delivery reads it before it writes, and says that
  it holds or each thing missing on a line of its own. It writes nothing, here or on GitHub, and
  exits 0 only where a delivery would go on.
- **`deliver VERB --request FILE`** answers one verb of [a land's delivery](#delivering-a-land).
  A land runs it; an operator has no reason to.

## What is exchanged

**An imported issue** is an open issue carrying one of `import_labels`. It becomes a proposal
in the backlog whose slug is numbers alone (`github/issue-REPOSITORY-NUMBER`), its brief the
issue's text under a line saying where it is from and who opened it. It starts nobody, claims
nothing and addresses nobody. Remote text is cleaned to content: control characters dropped, a
title one line of 200 characters, a body of 60,000 with a line saying where it was cut.

**A published task** is one opted in with `publish`. Its issue carries the curated text and
nothing of the brief or the thread, and ends in a marker, an HTML comment holding a digest of
the repository's number and the task's slug, which shows the slug to nobody.

Anyone who can open an issue can write a marker, so a marker alone maps nothing. An issue
carrying a task's marker is mapped to it only where `issues` exports and this side kept a curated
text for the task or journaled its publication; otherwise the preview names the issue and maps
nothing, and no note of it reaches the task. Where the curated text kept and the issue's differ,
the mapping starts as a conflict, never as an edit pushed into the issue.

**Edits.** Each side of a mapping is compared with the digest it had when the two were last the
same; no time is compared. A task's brief is judged from the one read that gives its token, so
an edit made while a preview runs is seen by it or refused by the board, never written over. The local side of an import is the task's title and brief, and of a
publication its curated text.

| Local | Remote | What a reconcile does |
| --- | --- | --- |
| unchanged | changed | the local side takes the issue's text, where `issues` imports |
| changed | unchanged | the issue takes the curated text, where `issues` exports; an imported task's own brief is never published |
| changed | changed, the same text | both are recorded as synchronized |
| changed | changed, different | a conflict: neither is overwritten, and a note says so once |

A conflict stays until `resolve` keeps a side. Keeping the local side of an import records the
issue's edit as seen and leaves the brief changed, so a later edit there conflicts again rather
than replacing what was kept.

**Notes.** A mapped issue closed or reopened is a note to `@orchestrator`, a triage request
that moves nothing. With `import_comments`, each comment becomes a note quoting it, addressed to
nobody. Each note is written once for its key however often it is planned, and only while the
board still holds the mapped task under the slug: after a restore that put another there, the
note is skipped.

Not exchanged: thread messages, labels other than `export_label` on creation, assignees,
workflow, roles, dependencies and stamps.

## What is kept

The state folder is `symphony/integrations/github` in the git common dir
([what a program outside the board keeps](TASKS.md#what-a-program-outside-the-board-keeps)): in
no worktree and no commit, and backed up with the repository's common dir or not at all.

- `state.json`: the repository bound by its number, a mapping for each task exchanged (the
  task's number with the board's recovery generation, the issue's number and ids, each side's
  digest), the curated texts, and what pauses the exchange.
- `state.json` holds as well the pull request each delivered task is bound to, with the task's
  number and, once it landed, the commit.
- `preview.json`: the preview that waits.
- `journal.jsonl`: a line before each operation is dispatched (its id, target, the digest of
  what it writes, what it expects to find, the settings' revision, who ran it) and a line with
  its result before the state moves on by it. An edit of a brief journals the brief's earlier
  words, which the board does not keep.
- `lock`: held by whichever command is writing. A second one is refused, never made to wait.

## Running it twice, and a run cut short

A reconcile is safe to run again and to interrupt.

- A preview already applied dispatches nothing: the command says so and exits 0.
- A run cut short is run again with the same digest. An operation acknowledged in the journal
  is settled from it and not made again.
- An operation dispatched whose result never arrived is looked for before it is tried: a
  created issue by its marker among every issue of the repository, under whichever preview it
  was dispatched (one found is mapped, more than one stops with none mapped and none created), an issue's edit and a brief's edit by
  reading the side they wrote, a proposal by its slug and the line naming its issue.
- A brief is rewritten only by `edit_bound`, with the token the preview read. A token the board
  refuses ends the run and drops the preview: nothing is tried on what the board holds now, and
  the next preview reads the task again.
- An issue is read again before it is rewritten. One that changed since the preview is not
  overwritten: it is a conflict.

## What pauses or suspends

| What happened | What follows |
| --- | --- |
| The settings turn it off, or change, after a preview | the preview is dropped; turning it on again replays nothing, and a new preview shows what accumulated |
| GitHub refuses the token (401) | paused, everything kept, until a preview succeeds |
| The repository named is missing or renamed (404, 301) | paused until the committed settings name it; the same repository under a new name keeps its mappings |
| Another repository answers at the name | paused until `preview --rebind` is applied, which binds to it and suspends every mapping; none is deleted and nothing is published again |
| An issue answers 404 | its mapping is flagged; the task is not touched and the issue is not created again |
| The rate limit (429, or 403 with `Retry-After` or no quota left) | nothing is sent before the time GitHub gave; the same preview goes on after it |
| The board was restored or rebound, or the slug names another task | the mapping is suspended until `resolve`; no task is written by a mapping kept for another |
| The board has no recovery binding | no mapping is written or made: [bind the board](TASKS.md#a-board-that-was-copied-or-restored) first |
| The state folder was lost | the next preview maps again what both sides still show (a proposal by its slug and issue line) and imports none of it twice; a publication is named with its issue and mapped again once `publish` opts its task in, found by its marker and not published twice |
| Issues moved between pages while they were listed | what was read is used and the cursor stays, so the next preview lists from it again |

## Delivering a land

A project whose `symphony.toml` commits `[land] delivery = ["symphony-github", "deliver"]` lands
its tasks by pull request ([TASKS](TASKS.md#a-land-delivered-by-a-pull-request)): the land runs
`symphony-github deliver VERB --request FILE` for each of four verbs and reads one line of JSON.
The adapter answers only `enabled` settings whose `pull_requests` is `"publish"`; under any other
it answers `refused` and asks GitHub nothing. It decides nothing: the land hands it the task's
evidence, and it reads no board for it.

- **`publish`** reads [the protection](#the-protection-a-delivery-needs) of the default branch
  first and refuses where it does not hold, then pushes the head to the branch named as the
  task, never forced, and only when
  `origin` is the settings' repository on GitHub.com. It finds the open pull request of that
  branch into the default branch before it opens one, as a draft, which nobody merges, then writes the evidence as checks on the
  head: `symphony/assessment`, `symphony/NAME` for each stamp (success where it stands, neutral
  where the assessment waived it, failure where it is missing) and `symphony/eligible`. That last
  one is what a branch's protection requires, from the App `checks_app_id` names. It is written
  last when it passes and first when it fails, so it never passes over a stamp not yet written,
  and the checks are read back as that App's before `published` is answered.
- **`merge`** reads the protection again, refusing as `publish` does, then marks the draft ready and asks GitHub once for a squash of the bound pull request at
  the head it is given, which GitHub refuses for any other head. A mark that fails or whose answer
  is lost merges nothing. A pull request already merged is answered as merged,
  with no second request.
- **`result`** reads the bound pull request: open, merged with its commit, or closed.
- **`landed`** keeps the commit, and closes the task's mapped issue where `close_issues` says.

The pull request's title and text, and the squash commit's title, are the task's curated text
(`publish SLUG`) or, with none, its slug. The title and brief the land hands over, the thread,
the handles and the reasons in the evidence are sent nowhere: a check says a stamp's name and
where it stands, and no more.

A land that finds the evidence of a bound pull request withdrawn publishes that too: `publish`
with a snapshot that is not eligible pushes the head the branch is at and fails
`symphony/eligible` there before any other check moves. The pull request stays as it is, ready
if a merge was asked for before: what keeps it from merging then is the failed check the
branch's rules require. Where the protection no longer holds, a snapshot that blocks is still
said, by its checks alone, on the pull request already bound and at the head it is at: no head
is pushed and no pull request opened past a policy that does not hold. A waived stamp's check is
neutral and says only that the assessment waived it: the assessor's reason is board text and is
sent nowhere.

Every write is journaled before it is made and looked for before it is made again: a pull
request by its branch, a check by its name, its conclusion and the evidence's revision, a merge
by the pull request's own state. A lost answer is answered `unavailable` or `unknown`, and the
next run finds what was made. Evidence of another revision rewrites the checks of that head.

Every request is GitHub's REST API made by `curl`, as the exchange's are, and the one thing REST
has no call for, marking a draft ready, is a single GraphQL mutation by the same `curl`. The `gh`
CLI is not used or needed. The adapter is no GitHub App and holds no App's key: it cannot mint or
renew an installation token, so `checks_token_file` holds one made outside it, which GitHub
expires within the hour. A token that expired is answered 401, `unavailable`, and nothing merges
until the file holds a current one. Keeping it current is the operator's, by whatever they run
for it; nothing here does.

## The protection a delivery needs

A delivery is held to its evidence by GitHub only where the default branch's rules say so, and
the adapter reads them before each step that counts on them: before `publish` pushes a head
whose evidence stands, and again before `merge` marks the pull request ready and asks for the
squash (`crates/symphony-github/src/preflight.rs`). Where they do not hold it answers `refused`
with `kind` `policy` and each thing missing, and pushes, opens, marks and merges nothing; an
answer GitHub did not give is `unavailable`, never a pass. `preflight` says the same to an
operator. The adapter edits no rule, ruleset, protection or visibility: putting them right is a
repository administrator's act on GitHub, and nothing here turns delivery on.

It makes three kinds of read, with `token_file`'s token: the repository, the rules in effect for
the branch (`GET /repos/OWNER/NAME/rules/branches/BRANCH`, every page), which GitHub evaluates
from every active ruleset that applies, an organization's among them, and each ruleset those
rules come from (`GET /repos/OWNER/NAME/rulesets/ID`) for who may bypass it. A rule holds
everyone only when its ruleset's bypass list was read and is empty. What must hold:

| What | How it is read |
| --- | --- |
| The repository is the one bound, and the branch its default one | its number, which a rename keeps, and `default_branch`; not archived |
| A pull request is required | a `pull_request` rule nobody bypasses |
| `symphony/eligible` is required from the App that writes it | a `required_status_checks` rule nobody bypasses naming that check with `integration_id` = `checks_app_id`; one naming it from any source or another App does not count, and no rule may hold the branch up to date (`strict_required_status_checks_policy`), which a delivery's one merge of the default branch does not meet |
| No force push, no deletion | `non_fast_forward` and `deletion` rules nobody bypasses |
| Only the landing App updates the branch | an `update` rule whose ruleset's one bypass actor is the App `landing_app_id` (`Integration`) |
| Squash and no other method | the repository's merge settings narrowed by the pull request rules nobody bypasses; no `merge_queue` rule and no auto-merge |
| The merging token is not said to administer | the repository's `permissions.admin` for the token is `false`; absent or anything else is unknown and refused |

The two halves are two rulesets, since one ruleset has one bypass list: the requirements with
nobody on theirs, and the update restriction with the landing App alone on its own. Without the
second, anyone who may write merges a pull request whose check is green, and a check stays
green on GitHub after the board withdrew its evidence until a land says otherwise
([delivering a land](#delivering-a-land)); with it, only a land merges, and a land reads the
evidence again first. Classic branch protection is not read: GitHub layers it with rulesets and
applies the most restrictive of each rule, so it adds to what is required and takes nothing
away, and reading it asks for administration.

**The rights each read needs**, by GitHub's own reference
([rules](https://docs.github.com/en/rest/repos/rules),
[repositories](https://docs.github.com/en/rest/repos/repos#get-a-repository)): the rules in
effect and the rulesets are shown to anyone who may read the repository, the repository's merge
settings with contents read and write, and a ruleset's `bypass_actors` only to who has write
access to that ruleset. So a token that does not administer the repository, as the landing
App's must not, is not shown the bypass lists, and the adapter reads a list it is not shown as
unknown and refuses. Which credential may read them for a delivery is not decided, and until it
is no repository passes with a landing token of the rights above: delivery stays off.

**What these reads establish, and what they do not.**

- Established at each read: the repository's number, the rules GitHub says are in effect for
  the branch, each ruleset's bypass list where shown, and the merge settings.
- Configured, not verified: that `token_file`'s token is of the App `landing_app_id` names. No
  read names the App an installation token is of. A token of anyone else is refused the merge
  by GitHub itself under the update restriction, which this adapter relies on and does not
  prove.
- Configured, not verified: that what merges cannot change the rules it is held to. No read
  proves it. `permissions.admin` is what the repository says of the token and does not measure
  write access to a ruleset, which an App's administration permission or a custom role that
  edits repository rules gives. And since a bypass list is shown only to who may write its
  ruleset, a pass whose lists were read with `token_file` means that token may write those
  rulesets: `token_file` is never given more rights to reach a pass.
- Not established: how GitHub applies the two rulesets together to a merge by the landing App,
  which a disposable repository proves and no read does; and that the rules stay as read. A
  repository administrator can change a rule between the adapter's read and GitHub's own act on
  the request that follows, and nothing here closes that: the next step and the next land read
  again and refuse, and the journal keeps a digest of each policy that held (`policy`), so a
  change is seen after the fact. No check constrains an administrator who removes a protection.
- Not done by this build: telling GitHub when evidence is withdrawn at the moment the board
  records it. It is told at the next land, so until the update restriction above is in force a
  green check can outlive its evidence where a person can merge.

## Checking it

`just test-rs -p symphony-github` runs its tests: each module's own, the exchange on a board
and a repository held in memory with every failure above planted, and the exchange on a real
scratch board through `tasks api` with its requests made by a real `curl` to a loopback server.
The delivery's verbs are asked of the same repository in memory, which answers pull requests,
their merge and a commit's checks, and its rules and rulesets, which it holds a merge to. None needs a GitHub repository or a credential. A run against GitHub itself needs a disposable
repository and a token its owner supplies, and is nobody's to start without that word.
