# GitHub issue exchange and pull request delivery

`symphony-github` exchanges selected tasks with the issues of one GitHub.com repository,
answers [a land delivered by a pull request](#delivering-a-land) there, and answers a release run
with [a GitHub Release](#publishing-a-release) for a tag it pushed. It is optional and turned
off: a project that never commits its settings file runs none of it, and its
board, its pages and its landing are as they are without it.

To set it up, follow [Setting up GitHub](GITHUB_SETUP.md): building the program, the token, the
GitHub Apps and rulesets a delivery needs, each step in order, and how to check it works. This
page is the reference those steps link to.

It is a program of its own (`crates/symphony-github`). It reaches the board through
[`tasks api`](BOARD_API.md#the-tasks-api) and nothing else, GitHub through `curl`, and keeps what it
must remember in the folder the board names for it. No other package of the workspace builds with
it, which `crates/symphony-github/tests/boundary.rs` holds. Nothing starts it but an operator,
a land where the project's `[land] delivery` names it, and a release run where its `[releases]
github` or `[releases] rulesets` does: there is no schedule, no webhook and no page that runs it. The design it follows, and what of
that design is not built, is [the proposal](analysis/github-integration.md).

## Turning it on

The settings are `symphony-github.toml` at the root of the tree the project's default branch
commits (`[project] default_branch` in the `symphony.toml` the main checkout's HEAD commits, as
the land reads it, `main` unless it says another). The adapter reads that file from git's
objects, never from a checkout: a file a branch commits, one in a linked worktree and one not
committed turn nothing on, and an edit of the main checkout's `symphony.toml` names no branch
until it is committed. A file that is
absent is the adapter turned off. One that does not read (a key it does not know, a value of
another type, settings that do not hold together) is refused by name, and every command reads a
refusal as turned off.

| Key | Default | What it sets |
| --- | --- | --- |
| `enabled` | `false` | whether anything is read from or written to GitHub |
| `repository` | none | `"OWNER/NAME"` on GitHub.com, the one repository reached; never a URL or another host |
| `issues` | `"off"` | `"import"`, `"export"` or `"exchange"` (both) |
| `pull_requests` | `"off"` | `"publish"` answers [a land's delivery](#delivering-a-land); `"track"` is read and held to its word, and this build tracks none and says so |
| `releases` | `"off"` | `"publish"` answers [a release run](#publishing-a-release) with a GitHub Release for a tag it pushed |
| `verification` | `"off"` | `"hosted"` answers [a release run](#verifying-a-release) with its commit's verification on GitHub's runners |
| `token_file` | none | the absolute path of a file holding the token |
| `import_labels` | `[]` | the labels that select an open issue for import |
| `import_comments` | `false` | whether the comments of a mapped issue become notes on its task |
| `export_label` | none | a label put on each issue the adapter creates |
| `app_id` | none | the number of the one GitHub App `token_file`'s token is of: for a delivery, the App whose checks [the default branch's rules](#the-protection-a-delivery-needs) require, which pushes, opens the pull request and merges; a check by any other is not read as the adapter's |
| `close_issues` | `false` | whether a delivered task's mapped issue is closed once the board closed the task |
| `upstream_id` | none | the number at GitHub of the one repository [an import](#delivering-an-import) is of; with none, no import is delivered and the default branch permits squash alone |
| `installation_id` | none | the number of the App's installation on the repository, whose token [the program makes](#tokens-it-makes) in `token_file` |
| `key_file` | none | the absolute path of that App's private key |

`enabled = true` needs `repository` and `token_file`, an `issues` that imports needs
`import_labels`, `pull_requests = "publish"` needs `app_id`, and `upstream_id` needs
`pull_requests = "publish"`. The App's installation and key file come together, with its number
and the token file they make the token of; the key file is never the token file. The keys of the
three Apps a delivery once took (`checks_*`, `landing_*` and `policy_*`) are each refused by
name, naming these. The settings are read again before every operation that writes GitHub or the
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
read and write for `export` and `exchange`, and contents read and write for `releases =
"publish"`, which GitHub takes a release's creation as, and actions read and write for
`verification = "hosted"`, which starts the workflow and reads its runs. A token that GitHub refuses (401) pauses
the exchange with everything kept; the next preview that succeeds lifts the pause.

A delivery uses the same one token: an installation token of the GitHub App `app_id` names, with
no administration. It pushes nothing (the checkout's own git does), opens, merges and closes the
pull request, turns GitHub's auto-merge of it on and off, writes the checks, reads the other
checks at its head, and reads [the protection](#the-protection-a-delivery-needs). GitHub
takes a check from an App alone, and the default branch requires `symphony/eligible` from this one,
so a person who may write cannot say the evidence stands. Who may bypass the rules is the one
thing the App is not shown, by design: it is [verified](#who-bypasses-the-rules) with the gh CLI
of someone who administers the repository. The token needs the least each call it is sent with
needs, and no more, beside the metadata (read) GitHub adds to every one; a token [the program
makes](#tokens-it-makes) is asked for exactly these:

| Its calls | Permission |
| --- | --- |
| the issues, their comments and their edits (`/issues`) | issues read for `import`; issues write for `export` and `exchange`, and where `close_issues` closes a delivered task's issue |
| the repository and its merge settings, the pull requests, their merge and their closing (`/pulls`, `/pulls/N/merge`), marking one ready and turning its auto-merge on and off (GraphQL) | pull requests write and contents write, under `pull_requests = "publish"` |
| a head's checks, read and written (`/commits/SHA/check-runs`, `/check-runs`) | checks write, under `pull_requests = "publish"` |
| a release by its tag, the tag and the release's creation (`/releases/tags/TAG`, `/git/ref/tags/TAG`, `POST /releases`) | contents write, under `releases = "publish"` |
| the workflow's runs, its start for one commit, one run, its jobs, their logs and its artifacts (`/actions/workflows/release-verify.yml/runs` and `/dispatches`, `/actions/runs/N`, `/actions/jobs/N/logs`, `/actions/artifacts/N/zip`) and the commit (`/commits/SHA`) | actions write, under `verification = "hosted"` |
| the rules in effect for the branch (`/rules/branches/BRANCH`), each ruleset they come from (`/rulesets/ID`, never shown its bypass list) and a public upstream's repository | metadata, which every token has; a private upstream is read only by a token its installation reaches |

Where the settings name no installation and key, whoever installs the App keeps a current token
in the file. The head itself is pushed by the
checkout's own git, with the credentials a person's `git push` there uses.

## Tokens it makes

Where the settings name the App's installation and private key beside its token file
(`installation_id` and `key_file`), that file is the program's own: it makes the token there itself, at the first use
that finds it absent, expired, within five minutes of expiring, holding a token it did not
write, or made for permissions other than the settings ask for now, and uses the one there
otherwise. Setting them hands the file to the program, which
replaces a token written there by hand at once (`crates/symphony-github/src/mint.rs`).

- **The key** is held as the token files are: outside the repository, a plain file readable by
  its owner alone, and refused otherwise before anything is sent. `openssl` signs the App's JSON
  Web Token with it (RS256, made a minute back and held nine minutes, as GitHub takes none of
  more than ten), reading the key from its file, so the key's bytes never enter the program and
  nothing `openssl` says is shown. A machine without `openssl` on `PATH` is told to install it.
- **The request** is `POST /app/installations/ID/access_tokens`, made by `curl` with the JWT on its
  standard input as every token is, asking for the one repository `repository` names and the
  permissions [the table above](#the-token) gives that token, no more: an App given more holds
  them, and its token does not. So a made `token_file` token reads no repository but that one:
  with a private upstream (`upstream_id`), an import is refused as not readable with the
  App's token (404), and `token_file` stays kept by hand, holding a token that reads both.
- **The file** holds the token on its first line, `expires_at` with the time GitHub gave on its
  second, and on its third the permissions it was asked for
  (`permissions checks=write,contents=write,pull_requests=write`): a token made before the
  settings asked for more, or by a build that wrote no third line, is made again at its next use.
  It is written to a file of its own beside it, readable by its owner alone, then renamed
  over it: a reader finds the old token or the new, never part of either, and two lands at once
  leave one of two tokens that both hold.
- **A failure writes nothing** and leaves the file as it was. A key that is not there, is
  readable by others or is not a key `openssl` signs with, GitHub refusing the key (401: not that
  App's key, or the App's number wrong), finding no installation (404), or refusing what is asked
  (422, in GitHub's words: a permission the App does not hold, or a repository it is not
  installed on) is refused, naming the setting to put right. GitHub's 401 is said in its own
  words, as it is answered to a JWT made by a clock more than a minute ahead of GitHub's or nine
  minutes behind as well as to a key that is not the App's. GitHub not answering is
  `unavailable`, and the next use asks again.

No key, JWT or token is said, journaled or kept anywhere but the token file. Where the settings
name no App for a file, it is read as it is: its token is sent, and one GitHub expired is
answered 401, `unavailable`, until the file holds a current one. The program runs no schedule
and nothing between uses: a token is made only by a command that sends it.

**`tokens`** says, for each token file the settings name, whether its token is current and until
when, or that it is absent, expired, made for other permissions or kept by hand, and never the
token. It reads no remote and
writes nothing, and exits 0 unless a file is refused.

## The commands

Each is run in a checkout of the project. `--tasks PROGRAM` names the board's program, which is
otherwise the `tasks` beside `symphony-github` or the one on `PATH`.

- **`status`** says what the committed settings allow and what is kept. It reads no remote and
  writes nothing, not even its folder.
- **`publish SLUG --title TITLE --body-file FILE`** opts a task in: TITLE and FILE's text are
  kept as its curated public text and shown as they will be published. Nothing is sent. A text
  an issue would not hold whole with its marker is refused, and so is a task imported from an
  issue, whose text is its brief.
- **`preview [--rebind]`** reads both sides and shows what a reconcile would do, with the exact text of
  every write, then keeps that preview under the digest it prints. It writes nothing to GitHub
  or the board, and nothing anywhere but a [token it makes](#tokens-it-makes). Turned off, it
  reads nothing from GitHub and shows only the curated texts that wait, and neither it nor
  `reconcile` makes the state folder where nothing is kept. `--rebind` binds to
  [another repository found at the name](#what-pauses-or-suspends).
- **`reconcile --preview DIGEST --as HANDLE`** applies the preview of that digest and no other:
  at most 50 operations a run, or `--limit N`. HANDLE is the board session the board's writes
  are made as.
- **`resolve SLUG local|remote`** keeps one side of a conflict, or takes up a suspended mapping
  again. It writes only the adapter's own folder; the next preview shows what follows.
- **`drop SLUG`** drops a mapping [found wrong](#dropping-a-mapping). It writes only the
  adapter's own folder.
- **`preflight [--branch BASE]`** reads [the protection](#the-protection-a-delivery-needs) of
  BASE, the default branch unless named, as a delivery reads it before it writes, and says that
  it holds or each thing missing on a line of its own. It writes nothing, here or on GitHub, but
  a [token it makes](#tokens-it-makes), and exits 0 only where a delivery would go on.
- **`rulesets [--verify] [--branch BASE]`** reads the rulesets in effect on BASE in full, their
  bypass lists among them, with the gh CLI of someone who administers the repository, says each
  and judges [the protection](#the-protection-a-delivery-needs) on them; `--verify` keeps [who
  bypasses each](#who-bypasses-the-rules) where it holds. It writes nothing to GitHub, and exits 0
  only where the protection holds. A release run runs `rulesets --verify` where the project's
  `[releases] rulesets` names it ([again at each release](#who-bypasses-the-rules)).
- **`deliver VERB --request FILE`** answers one verb of [a land's delivery](#delivering-a-land).
  A land runs it; an operator has no reason to.
- **`release --request FILE`** answers a release run with
  [a GitHub Release](#publishing-a-release) for the tag FILE names. A release run runs it; an
  operator asks through that run's own `--github`.
- **`verify --request FILE`** answers a release run with
  [its commit's verification](#verifying-a-release) on GitHub's runners. A release run runs it;
  an operator has no reason to.
- **`tokens`** says whether the token is current and when it expires, never the token
  ([tokens it makes](#tokens-it-makes)).

## What is exchanged

**An imported issue** is an open issue carrying one of `import_labels`. It becomes a proposal
in the backlog whose slug is numbers alone (`github/issue-REPOSITORY-NUMBER`), its brief the
issue's text under a line saying where it is from and who opened it. It starts nobody, claims
nothing and addresses nobody. Remote text is cleaned to content: control characters dropped, a
title one line of 200 characters, a body of 60,000 with a line saying where it was cut.

**A published task** is one opted in with `publish`. Its issue carries the curated text and
nothing of the brief or the thread, and ends in a marker, an HTML comment holding a digest of
the repository's number and the task's slug, which shows the slug to nobody. A curated text is
read back with its marker within the 60,000 characters a read keeps, so `publish` refuses one
longer than that less the marker's line.

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

## Dropping a mapping

A mapping found wrong is answered by neither side of `resolve`: a planted issue that carries a
waiting curated text's marker, adopted as a conflict, holds a stranger's words that are not the
task's to take, and pushing the curated text into it would make the stranger's issue the
publication. `drop SLUG` drops the mapping and keeps the issue as not the task's: no preview maps
the two again, counts it as a duplicate of the task's publication, or takes it for a publication
whose answer was lost. A curated text kept stays, so the next preview shows it published as a
new issue. Nothing is written to the issue or the board, and the journal holds the drop as an act
of its own, its intent carrying the mapping it drops. The preview waiting is dropped before
anything else is written, so a drop or a `resolve` cut short never leaves one read before it.

No command undoes a drop. The issue's id stays under the task's slug in `dropped` in
`state.json`; removing it there by hand, while no command runs, lets the next preview map the
two again.

## What is kept

The state folder is `symphony/integrations/github` in the git common dir
([what a program outside the board keeps](BOARD_API.md#what-a-program-outside-the-board-keeps)): in
no worktree and no commit, and backed up with the repository's common dir or not at all.

- `state.json`: the repository bound by its number, a mapping for each task exchanged (the
  task's number with the board's recovery generation, the issue's number and ids, each side's
  digest), the curated texts, and what pauses the exchange.
- `state.json` holds as well the pull request each delivered task is bound to, with the task's
  number and, once it landed, the commit, the issues dropped from each task's mapping, and the
  GitHub Release each release's tag is published as, with its number, its page and the commit.
- `preview.json`: the preview that waits.
- `journal.jsonl`: a line before each operation is dispatched (its id, target, the digest of
  what it writes, what it expects to find, the settings' revision, who ran it) and a line with
  its result before the state moves on by it. An edit of a brief journals the brief's earlier
  words with its intent, which the board does not keep, so an edit cut short can be put back. A
  last line a crash cut short is left out, and cut away before the next is written; any other
  line that does not read stops every command that reads the journal, naming it, since a line
  lost may be an intent.
- `lock`: held by whichever command is writing. A second one is refused, never made to wait.

## Running it twice, and a run cut short

A reconcile is safe to run again and to interrupt.

- A preview already applied dispatches nothing: the command says so and exits 0.
- A run cut short is run again with the same digest. An operation acknowledged in the journal
  is settled from it and not made again.
- An operation dispatched whose result never arrived is looked for before it is tried: a
  created issue by its marker among every issue of the repository, under whichever preview it
  was dispatched (one found is mapped, more than one stops with none mapped and none created), an issue's edit and a brief's edit by
  reading the side they wrote, a proposal by its slug and the line naming its issue. Such a
  publication, or one skipped, is looked for at every later apply of it until an acknowledgment
  maps an issue: each costs one request for each hundred issues of the repository, up to 1,000
  pages (100,000 issues). A repository with more ends the search there: the run stops with the
  publication's result still unknown and creates nothing, since a public issue made twice cannot
  be recalled.
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
| The board has no recovery binding | no mapping is written or made: [bind the board](BOARD_OPERATIONS.md#a-board-that-was-copied-or-restored) first |
| The state folder was lost | the next preview maps again what both sides still show (a proposal by its slug and issue line) and imports none of it twice; a publication is named with its issue and mapped again once `publish` opts its task in, found by its marker and not published twice |
| Issues moved between pages while they were listed | what was read is used and the cursor stays, so the next preview lists from it again |

## Delivering a land

A project whose `symphony.toml` commits `[land] delivery = ["symphony-github", "deliver"]` lands
its tasks by pull request ([TASKS](TASKS.md#a-land-delivered-by-a-pull-request)): the land runs
`symphony-github deliver VERB --request FILE` for each verb below and reads one line of JSON.
The adapter answers only `enabled` settings whose `pull_requests` is `"publish"`; under any other
it answers `refused` and asks GitHub nothing. It decides nothing: the land hands it the task's
evidence, and it reads no board for it.

- **`publish`** reads [the protection](#the-protection-a-delivery-needs) of the default branch
  first and refuses where it does not hold, then pushes the head to the branch named as the
  task, never forced, and only when `origin` is the settings' repository on GitHub.com. It finds
  the open pull request of that branch into the default branch before it opens one, ready for
  review, as GitHub turns on no auto-merge of a draft; one an earlier build opened as a draft is
  marked ready. It then writes the evidence as checks on the head: `symphony/assessment`,
  `symphony/NAME` for each stamp and `symphony/eligible`, which is what the branch's protection
  requires, from the App `app_id` names. Each check is in one of four states (below).
  `symphony/eligible` is written last when it passes and first otherwise, so it never passes over
  a stamp not yet written, and the checks are read back as that App's before `published` is
  answered. GitHub lists a check a few seconds after it takes it, so one not listed yet is read
  again after rests of 1, 2, 4 and 8 s, well inside the time the land gives the program, and
  nothing is written again; one still not listed then is answered `unavailable`, and the next run
  finds what was written. GitHub also moves a pull request that is open already to a head pushed
  a few seconds after the push, so one still at the head it was read at before the push is read
  again after the same rests, once the checks stand, and `published` names the head pushed; one
  that has not moved then is answered `unavailable`, and the next run reads it first. A head
  that is neither, as someone else pushed, is answered as it is, and the land refuses it.
- **`arm`** reads the protection again, refusing as `publish` does, then turns on GitHub's
  auto-merge of the bound pull request at the head it is given (`expectedHeadOid`), by the method
  and commit text `merge` would ask for. GitHub then merges it itself once every check the branch
  requires passes, `symphony/eligible` among them. A pull request that is a draft is marked ready
  first. It answers:
  - `{"state": "armed", "pull": N, "head": SHA}`, also when auto-merge was on already by that
    method and that text (GitHub's `auto_merge.commit_title` and `commit_message`), which it
    turns on no second time. GitHub merges by the text it was armed with, so one on by another
    text, armed at an earlier head or before the task's curated text was written, is turned off
    and on again at this head with this text. One turned off whose arming GitHub then refuses,
    or whose answer is lost, is answered so and left as GitHub has it: the next run reads it
    first and arms it;
  - `{"state": "mergeable", "pull": N, "head": SHA}` where GitHub can merge it already (it takes
    no auto-merge of a pull request in `clean` or `unstable` status): the land asks `merge`;
  - `{"state": "merged", "sha": SHA}` and `{"state": "closed"}` where it is so already;
  - `{"state": "refused", "kind": KIND, "why": TEXT}`: `head` where the pull request is at
    another head, `conflict` where it is in conflict with its base, `policy` (with `missing`)
    where the protection does not hold, `permission` for anything else GitHub refused, such as a
    repository that does not allow auto-merge;
  - `{"state": "unavailable", "why": TEXT}` where the answer was lost: the next run reads first.
- **`merge`** reads the protection again, refusing as `publish` does, then marks a draft ready
  and asks GitHub once for a squash of the bound pull request at the head it is given, which
  GitHub refuses for any other head. A mark that fails or whose answer is lost merges nothing. A
  pull request already merged is answered as merged, with no second request.
- **`disarm`** turns auto-merge of the bound pull request off, so no check that passes later
  merges it. It answers `{"state": "disarmed"}` (also where it was off), `{"state": "merged",
  "sha": SHA}` where GitHub merged it first, `{"state": "closed"}`, `refused` with `kind`
  `permission`, or `unavailable`.
- **`close`** turns auto-merge off as `disarm` does, then closes the pull request unmerged, its
  branch left as it is. It answers `{"state": "closed"}` (also where it was closed),
  `{"state": "merged", "sha": SHA}` where GitHub merged it first, `refused` with `kind`
  `permission`, or `unavailable`.
- **`result`** reads the bound pull request and writes nothing: `{"state", "head", "sha",
  "base", "repository", "auto_merge", "mergeable_state", "foreign"}`. `state` is `open`,
  `merged` (with `sha`, its commit) or `closed`; `auto_merge` is whether auto-merge is on;
  `mergeable_state` is GitHub's word for whether it merges (`clean`, `blocked`, `dirty`,
  `unstable`, `unknown` and the rest); and `foreign`, read only while it is open, is each check
  run at its head another App wrote whose newest run did not pass (`failure`, `cancelled`,
  `timed_out`, `action_required`, `startup_failure`), as `{"name", "conclusion"}`. The App's own
  checks are never foreign. A CI that writes commit statuses rather than check runs is not read,
  so a required one that fails holds the merge, unnamed, until `[land] merge_within_minutes`
  passes and the watcher says so.
- **`landed`** keeps the commit, and closes the task's mapped issue where `close_issues` says.

A check says one of four things of the head:

| Check | Passes | Neutral | Fails | Pending |
| --- | --- | --- | --- | --- |
| `symphony/assessment` | an assessor read it | | | not read yet |
| `symphony/NAME` | the stamp stands | the assessment waived it | missing, and red at the head (`red`) | missing |
| `symphony/eligible` | every stamp the task needs stands | | a stamp it needs is red, or evidence that stood at this head was withdrawn | anything else |

A pending check is a check run still in progress, with no conclusion: GitHub holds a required
check, and so a merge and an armed auto-merge, until it completes. A check that completed and is
pending again is written as a new run beside the old one, as GitHub goes by the newest run of a
name; one pending that completes is that run, completed. Once `symphony/eligible` has passed at a
head, evidence there that is short of standing again fails it, titled as withdrawn, rather than
leave it pending, and it stays failing at that head until the evidence stands again.

The pull request's title and text, and the squash commit's title, are the task's curated text
(`publish SLUG`) or, with none, its slug; an import's merge commit is titled as below. The title and brief the land hands over, the thread,
the handles and the reasons in the evidence are sent nowhere: a check says a stamp's name and
where it stands, and no more.

A land that finds the evidence of a bound pull request withdrawn publishes that too: `publish`
with a snapshot that is not eligible pushes the head the branch is at and fails
`symphony/eligible` there before any other check moves. The pull request stays as it is, armed
if it was: what keeps it from merging then is the failed check the branch's rules require, and a
land that gives the task up asks `disarm` or `close`. Where the protection no longer holds, a snapshot that blocks is still
said, by its checks alone, on the pull request already bound and at the head it is at: no head
is pushed and no pull request opened past a policy that does not hold. A waived stamp's check is
neutral and says only that the assessment waived it: the assessor's reason is board text and is
sent nowhere.

Every write is journaled before it is made and looked for before it is made again: a pull
request by its branch, a check by its name, its conclusion and the evidence's revision, a merge,
an auto-merge and a close by the pull request's own state. A lost answer is answered `unavailable` or `unknown`, and the
next run finds what was made. Evidence of another revision rewrites the checks of that head.

Every request is GitHub's REST API made by `curl`, as the exchange's are, and the three things
REST has no call for, marking a draft ready and turning auto-merge on
(`enablePullRequestAutoMerge`) and off (`disablePullRequestAutoMerge`), are each a single GraphQL
mutation by the same `curl`. A delivery
never runs the `gh` CLI: only [verifying who bypasses the rules](#who-bypasses-the-rules) does, as
someone who administers the repository. The installation token, which GitHub expires within the
hour, is [made by the program](#tokens-it-makes) where the settings name the App's installation
and key, so a land goes on with no file refreshed by hand. Where they do not, the
file holds one made outside it: a token that expired is answered 401, `unavailable`, and nothing
merges until the file holds a current one.

## Delivering an import

A downstream project imports its upstream's history by a task whose head merges an upstream
commit, which the land delivers as an import, merged by a merge commit and never squashed, only
where the project file, the head's assessment and the history fetched from the upstream agree
([TASKS](TASKS.md#an-import-delivered-by-a-pull-request)). Each `publish` and `merge` of one
says so: `operation` `import`, and `upstream` with its address, its branch and the commit
imported. The adapter delivers it only where the settings pin the upstream by its number,
`upstream_id`, and answers anything else as a task: squashed, and refused where it asks for a
merge commit.

Before `publish` pushes an import's head, and again before `merge` asks for it, the adapter reads
the upstream at the address the land fetched, with `token_file`'s token, and refuses, pushing and
merging nothing, where:

- the address is not a repository on GitHub.com, or that token cannot read it, as a token
  [the program makes](#tokens-it-makes) cannot read a private upstream;
- the repository there is not the one `upstream_id` pins (a rename keeps its number), or it is
  the repository delivered to;
- the upstream is private and the repository delivered to is not, which would publish its
  history; or either does not say whether it is private;
- the evidence names another import, or none, than the operation asked for.

Its `published` answer names the upstream's number and name, which the land requires of an
import: an older adapter answers none, and nothing merges. The merge is GitHub's merge commit of
the pull request at the head given (`merge_method` `merge`), titled `Merge branch 'SLUG'` as the
land's own merge is, which the release cadence counts; its message names the head, the pull
request and the commit imported.

The land fetches the upstream itself for every head it delivers, ordinary tasks as well as
imports, to refuse a task head that holds its commits
([TASKS](TASKS.md#an-import-delivered-by-a-pull-request)). It fetches with git and the
machine's own credentials; the adapter's token reads the upstream through GitHub's API only.
While `[land.upstream]` is set, an upstream that machine cannot fetch stops every delivery of the
repository before `publish` is asked. The line names the upstream and git's reason, and the
lander lands again once it answers. A renamed repository keeps answering through GitHub's
redirect until another takes its old name. An address that no longer answers, such as a
repository deleted or made private, is corrected on the default branch, but no delivery makes
that correction, since its land is refused too. [The protection below](#the-protection-a-delivery-needs)
merges nothing into the branch without the App's green check, so the repository's administrator
lands the correction at GitHub, setting the ruleset aside for it and verifying it again after
(`rulesets --verify`), as the adapter edits no rule.

What this does not establish: that a history kept private is never pushed elsewhere. An import
is the one operation that brings another repository's commits in, and a task head holding them
is refused, but a branch that copies a private history's files as commits of its own is a task,
and only its review sees what it publishes.

## Publishing a release

A project whose `symphony.toml` commits `[releases] github = ["symphony-github", "release"]` has
its release run ask for a GitHub Release of each tag it pushed
([RELEASES](RELEASES.md#a-github-release)): the run starts `symphony-github release --request
FILE` in the main checkout and reads one line of JSON. The adapter answers only `enabled`
settings whose `releases` is `"publish"`; under any other it answers `refused` and asks GitHub
nothing, keeping nothing. The request names the release's `tag`, the `commit` it tags, its
`body` and whether it is the `latest`.

In order, with `token_file`'s token (`crates/symphony-github/src/release.rs`):

1. **The repository** is read and held to the one the state is bound to by its number, as a
   delivery holds it; with none bound yet, it is bound. Another repository at the name is
   refused: nothing is published to it.
2. **The release is looked for by its tag** (`GET /repos/OWNER/NAME/releases/tags/TAG`). One
   found is kept as the tag's and answered `found`: none is made, and it is left as it is,
   whatever its text. One that was published here and GitHub no longer shows, because a person
   removed it or made it a draft, is refused and not made again.
3. **The tag is read at GitHub** (`GET …/git/ref/tags/TAG`, through the tag's own object where it
   is annotated) and must name the request's commit. GitHub makes a tag of its own for a release
   whose tag it does not hold, so with no tag there, or one naming another commit, nothing is
   asked for.
4. **The release is made** (`POST …/releases`): its name the tag's, its body the text handed
   over, cleaned to content and cut at 100,000 characters with a line saying so, neither a draft
   nor a prerelease, and marked the latest only where the request says. Nothing is uploaded.

The answer's `state` is `published` (made now), `found`, `refused` (running again as things are
makes none: the settings, the repository, the tag, a token that may not write contents),
`unavailable` (GitHub gave no answer to go on by, and nothing was made) or `unknown` (the release
was asked for and its answer was lost).

It is safe to run twice and to interrupt. The one write is journaled before it is made, and
every run looks for the release by its tag first, so a second run, a run after a lost answer and
a run after one killed between the request and its answer make no second release. A creation
GitHub refuses as there already, made by another run since the look, is found and kept. This
command sends one kind of write, a release's creation: it edits and deletes no release and
makes, moves and deletes no tag.

## Verifying a release

A project whose `symphony.toml` commits `[releases] hosted = ["symphony-github", "verify"]` has
its release run verify its commit on GitHub's runners
([RELEASES](RELEASES.md#on-githubs-runners)): the run starts `symphony-github verify --request
FILE` in the main checkout and reads one line of JSON. The adapter answers only `enabled`
settings whose `verification` is `"hosted"`; under any other it answers `refused` and asks GitHub
nothing, keeping nothing. The request names the `commit`, its 40 hexadecimal digits, the
`folder` the run's logs are downloaded into, which must be there, and the `seconds` the run may
be waited for (six hours with none).

In order, with `token_file`'s token (`crates/symphony-github/src/verify.rs`):

1. **The repository** is read and held to the one the state is bound to by its number, as a
   delivery holds it; with none bound yet, it is bound. Another repository at the name is
   refused: nothing is started in it.
2. **The commit must be there** (`GET …/commits/SHA`): one GitHub does not hold is `unavailable`,
   and nothing is started.
3. **The workflow is started** on the default branch for that commit (`POST
   …/actions/workflows/release-verify.yml/dispatches`, its one input the commit), the one write
   this command sends, journaled before it is made. The state's lock is held until GitHub
   answers it and never while the run is waited for, so a land's delivery is not kept waiting.
4. **Its run is found**: the newest dispatched run of the workflow titled for the commit that
   was not listed before the dispatch, looked for over two minutes, as GitHub lists a run a
   little after it takes the dispatch. A dispatch whose answer was lost is looked for the same
   way; none showing is `unavailable`.
5. **The run is waited for**, read every 30 seconds until it has ended. Twenty reads in a row
   left unanswered give the wait up as `unavailable`; the time given ending first is `late`, the
   run left to end.
6. **It is believed** only if GitHub says it is of the bound repository, of
   `.github/workflows/release-verify.yml`, started by a `workflow_dispatch` on the default
   branch, titled for exactly the commit, and completed with a conclusion; anything else is
   `refused` and nothing is downloaded. Whether the workflow file it ran is the one the default
   branch commits is the release run's to check with its own git, by the commit the answer
   names.
7. **Its logs are downloaded** into the folder: each job's log (`job-N.log`) and each artifact's
   archive (`artifact-N.zip`). GitHub answers each with a redirect to another host, which a
   second `curl` fetches with no token, its URL read from standard input and said nowhere, as it
   carries the download's own signature. A job with no log and an artifact that cannot be
   fetched are said in the answer (`no_log`, `missing`) and stop nothing.

The answer's `state` is `ended` (with `run`, its page, attempt, conclusion and the commit whose
workflow file it ran; `jobs`, each with its conclusion as GitHub gave it, its seconds and its
log; and `artifacts`), `late`, `refused` (asking again as things are changes nothing: the
settings, the repository, the workflow file, a token that may not start it, a run not believed)
or `unavailable` (GitHub gave no answer to go on by). A red run is `ended` like a green one: what
its jobs concluded is the release run's to judge.

It is safe to run twice: each request starts a run of its own and reads that one, and the
workflow is given no secret and writes nothing, so a run started and never read costs runner
minutes and nothing else. The command cancels no run.

## The protection a delivery needs

A delivery is held to its evidence by GitHub only where the default branch's rules say so, and
the adapter reads them before each step that counts on them: before `publish` pushes a head
whose evidence stands, and again before `arm` turns on auto-merge and before `merge` marks the
pull request ready and asks for the squash (`crates/symphony-github/src/preflight.rs`). Where they do not hold it answers `refused`
with `kind` `policy` and each thing missing, and pushes, opens, marks, arms and merges nothing; an
answer GitHub did not give is `unavailable`, never a pass. `preflight` says the same to an
operator. The adapter edits no rule, ruleset, protection or visibility: putting them right is a
repository administrator's act on GitHub ([the steps](GITHUB_SETUP.md#delivery-by-pull-request)),
and nothing here turns delivery on.

It makes three kinds of read, each with the one App's token: the repository, the rules in effect
for the branch (`GET /repos/OWNER/NAME/rules/branches/BRANCH`, every page), which GitHub
evaluates from every active ruleset that applies, an organization's among them, and each ruleset
those rules come from (`GET /repos/OWNER/NAME/rulesets/ID`): whether it is active, when it last
changed (`updated_at`) and whether the App itself may bypass it (`current_user_can_bypass`). Who
else may bypass it the App is not shown, and is taken from [its
verification](#who-bypasses-the-rules). A ruleset GitHub does not answer for (expired, refused,
limited) is `unavailable`, so a land comes back to it, and evidence that blocks is still said, by
its checks, as it is whenever the policy does not hold. A rule holds everyone only when its
ruleset was verified with an empty bypass list, has not changed since, and the App does not
bypass it. What must hold:

| What | How it is read |
| --- | --- |
| The repository is the one bound, and the branch its default one | its number, which a rename keeps, and `default_branch`; not archived |
| A pull request is required | a `pull_request` rule nobody bypasses |
| `symphony/eligible` is required from the App | a `required_status_checks` rule nobody bypasses naming that check with `integration_id` = `app_id`; one naming it from any source or another App does not count, and no rule may hold the branch up to date (`strict_required_status_checks_policy`), which a delivery's one merge of the default branch does not meet |
| No force push, no deletion | `non_fast_forward` and `deletion` rules nobody bypasses |
| No rule restricts updates of the branch | no `update` rule in effect, whoever it lets through: the required check is what holds a merge to its evidence, and GitHub's auto-merge does not merge past a rule that asks for a bypass |
| Squash and no other method, or with `upstream_id` squash and merge commits | the repository's merge settings narrowed by the pull request rules nobody bypasses; no `merge_queue` rule; with `upstream_id`, no `required_linear_history` rule, which refuses a merge commit |
| The App's token is not said to administer | the repository's `permissions.admin` for the token is `false`; absent or anything else is unknown and refused |

So one ruleset, which nobody bypasses, is the whole protection. Anyone who may write can merge a
pull request whose `symphony/eligible` is green, as GitHub's auto-merge does by itself; nobody can
merge one whose check is not green from the App, or push the branch, whatever their role. A check
stays green on GitHub after the board withdrew its evidence until a land says otherwise
([delivering a land](#delivering-a-land)): that is the window a person who may write could merge
in. Classic branch protection is not read: GitHub layers it with rulesets and applies the most
restrictive of each rule, so it adds to what is required and takes nothing away, and reading it
asks for administration.

### Who bypasses the rules

GitHub shows a ruleset's `bypass_actors` only to who has write access to that ruleset
([rules](https://docs.github.com/en/rest/repos/rules)), which the App must not have. So who
bypasses each ruleset is read by someone who administers the repository, with their own gh CLI:
`symphony-github rulesets --verify` runs `gh api` with a path alone (a GET) for the repository,
the rules on the default branch and each ruleset they come from, says each ruleset and who
bypasses it, judges the policy above on those rules, and where it holds keeps each ruleset in the
state: its number, the repository's, its bypass list and the `updated_at` GitHub gave
(`crates/symphony-github/src/rulesets.rs`). Without `--verify` it keeps nothing. The token of who
is signed in to gh stays gh's: the program never reads it.

Each land takes a ruleset's verified list only while the ruleset's `updated_at`, as the App reads
it, is still the instant verified (compared as instants: gh and the App are told it in different
zones). Adding a bypass actor, taking one out and changing a ruleset's enforcement each moved its
`updated_at` on a disposable repository, read by an App with no administration as by gh. So a
ruleset changed in any of those ways since its verification holds nobody, and the delivery is
refused, naming the command, until it is verified again; one never verified holds nobody the same
way. Setup verifies once ([the steps](GITHUB_SETUP.md#delivery-by-pull-request)), and
whoever changes a ruleset verifies it again. Where the project's `symphony.toml` names
`[releases] rulesets = ["symphony-github", "rulesets", "--verify"]`, each release run verifies
again too, with the gh of whoever runs the release, as a check of the run
([RELEASES](RELEASES.md#who-bypasses-the-rules-again)): a ruleset changed in a way that keeps the
policy is taken again from that run on, and one that does not is a red of the run, filed with
each thing missing. Nothing else verifies, and no land does.

**What these reads establish, and what they do not.**

- Established at each read: the repository's number, the rules GitHub says are in effect for
  the branch, whether each ruleset is active, when it last changed and whether the App bypasses
  it, and the merge settings.
- Established at each verification, and taken while the ruleset's `updated_at` stands: who else
  bypasses it.
- Configured, not verified: that `token_file`'s token is of the App `app_id` names. No read names
  the App an installation token is of. A token of anyone else writes checks GitHub does not take
  as the App's, so the required check is never green from it.
- Configured, not verified: that the App cannot change the rules it is held to. No read proves
  it. `permissions.admin` is what the repository says of the token and does not measure write
  access to a ruleset, which an App's administration permission or a custom role that edits
  repository rules gives. The App is given no administration, and its token is asked for none.
- Not established: that every change to a ruleset moves its `updated_at`. Enforcement and the
  bypass list were seen to; a change GitHub made without moving it would be taken as the
  verified list until the next verification. Nor that the rules stay as read: an administrator
  can change a rule between the adapter's read and GitHub's own act on the request that follows,
  and the next step and the next land read again and refuse; the journal keeps a digest of each
  policy that held (`policy`), so a change is seen after the fact. No check constrains an
  administrator who removes a protection.
- Not done by this build: telling GitHub when evidence is withdrawn at the moment the board
  records it. It is told at the next land, so a green check can outlive its evidence where a
  person who may write merges.

## Checking it

A setup of a repository of your own is checked by `status`, `preview`, `rulesets` and `preflight`
([Setting up GitHub](GITHUB_SETUP.md#checking-it)). The program itself is checked by its tests:
`just test-rs -p symphony-github` runs them: each module's own, the exchange on a board
and a repository held in memory with every failure above planted, and the exchange on a real
scratch board through `tasks api` with its requests made by a real `curl` to a loopback server.
The delivery's verbs are asked of the same repository in memory, which answers pull requests,
their merge, their auto-merge and their closing, and a commit's checks, and its rules
and rulesets, which it holds a merge to; it merges a pull request whose auto-merge is on after
each check written, once nothing the rules require is missing at its head, as GitHub does. A
release is asked of it too, which answers a tag, the release of a tag and its creation, and
makes a tag of its own for a release whose tag it lacks, as GitHub does. The rulesets are verified through a gh that answers what that repository answers a person.
The tokens it makes are
asked of GitHub's side of the App held in memory too, which verifies each JWT's signature with
the App's public key by `openssl`, as the program signs it, so those tests need `openssl` on
`PATH`. None needs a GitHub repository or a credential. A run against GitHub itself needs a disposable
repository and a token its owner supplies, and is nobody's to start without that word.
