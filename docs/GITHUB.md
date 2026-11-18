# GitHub issue exchange

`symphony-github` exchanges selected tasks with the issues of one GitHub.com repository. It is
optional and turned off: a project that never commits its settings file runs none of it, and its
board, its pages and its landing are as they are without it.

It is a program of its own (`crates/symphony-github`). It reaches the board through
[`tasks api`](TASKS.md#the-tasks-api) and nothing else, GitHub through `curl`, and keeps what it
must remember in the folder the board names for it. No other package of the workspace builds with
it, which `crates/symphony-github/tests/boundary.rs` holds. Nothing starts it but an operator:
there is no schedule, no webhook and no page that runs it. The design it follows, and what of
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
| `pull_requests` | `"off"` | `"track"` or `"publish"` are read and held to their words; this build serves neither and says so |
| `token_file` | none | the absolute path of a file holding the token |
| `import_labels` | `[]` | the labels that select an open issue for import |
| `import_comments` | `false` | whether the comments of a mapped issue become notes on its task |
| `export_label` | none | a label put on each issue the adapter creates |

`enabled = true` needs `repository` and `token_file`, and an `issues` that imports needs
`import_labels`. The settings are read again before every request that changes GitHub, so a
landing that turns the adapter off stops a run at its next dispatch.

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

## The commands

Each is run in a checkout of the project. `--tasks PROGRAM` names the board's program, which is
otherwise the `tasks` beside `symphony-github` or the one on `PATH`.

- **`status`** says what the committed settings allow and what is kept. It reads no remote and
  writes nothing, not even its folder.
- **`publish SLUG --title TITLE --body-file FILE`** opts a task in: TITLE and FILE's text are
  kept as its curated public text and shown as they will be published. Nothing is sent.
- **`preview`** reads both sides and shows what a reconcile would do, with the exact text of
  every write, then keeps that preview under the digest it prints. It writes nothing to GitHub
  or the board. Turned off, it reads nothing from GitHub and shows only the curated texts that
  wait.
- **`reconcile --preview DIGEST --as HANDLE`** applies the preview of that digest and no other:
  at most 50 operations a run, or `--limit N`. HANDLE is the board session the board's writes
  are made as.
- **`resolve SLUG local|remote`** keeps one side of a conflict, or takes up a suspended mapping
  again. It writes only the adapter's own folder; the next preview shows what follows.

## What is exchanged

**An imported issue** is an open issue carrying one of `import_labels`. It becomes a proposal
in the backlog whose slug is numbers alone (`github/issue-REPOSITORY-NUMBER`), its brief the
issue's text under a line saying where it is from and who opened it. It starts nobody, claims
nothing and addresses nobody. Remote text is cleaned to content: control characters dropped, a
title one line of 200 characters, a body of 60,000 with a line saying where it was cut.

**A published task** is one opted in with `publish`. Its issue carries the curated text and
nothing of the brief or the thread, and ends in a marker, an HTML comment holding a digest of
the repository's number and the task's slug, which shows the slug to nobody.

**Edits.** Each side of a mapping is compared with the digest it had when the two were last the
same; no time is compared. The local side of an import is the task's title and brief, and of a
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
nobody. Each note is written once for its key however often it is planned.

Not exchanged: thread messages, labels other than `export_label` on creation, assignees,
workflow, roles, dependencies and stamps.

## What is kept

The state folder is `symphony/integrations/github` in the git common dir
([what a program outside the board keeps](TASKS.md#what-a-program-outside-the-board-keeps)): in
no worktree and no commit, and backed up with the repository's common dir or not at all.

- `state.json`: the repository bound by its number, a mapping for each task exchanged (the
  task's number with the board's recovery generation, the issue's number and ids, each side's
  digest), the curated texts, and what pauses the exchange.
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
  created issue by its marker among every issue of the repository (one found is mapped, more
  than one stops with none mapped and none created), an issue's edit and a brief's edit by
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
| The state folder was lost | the next preview maps again what both sides still show (a proposal by its slug and issue line, a publication by its marker) and imports or publishes none of it twice |
| Issues moved between pages while they were listed | what was read is used and the cursor stays, so the next preview lists from it again |

## Checking it

`just test-rs -p symphony-github` runs its tests: each module's own, the exchange on a board
and a repository held in memory with every failure above planted, and the exchange on a real
scratch board through `tasks api` with its requests made by a real `curl` to a loopback server.
None needs a GitHub repository or a credential. A run against GitHub itself needs a disposable
repository and a token its owner supplies, and is nobody's to start without that word.
