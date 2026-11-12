# Artifacts attached, not committed

Where a task's pictures and other evidence live once they leave the worktree that made them, how a
card, a thread post or a document names one so the link works wherever the board is served, and
how long each is kept. This is a design: nothing here is built. The choice of where the bytes live
is the owner's, on one card in [DECISIONS](../DECISIONS.md).

- **Recommended: E, a store addressed by content.** An agent attaches a file with one command,
  which copies it into its project's main checkout at `artifacts/uploads/<sha256>.<ext>`, the store
  the owner's own uploads already use, and prints the line to post. That name never moves: every
  page, every branch's view and the phone read it from the server, whatever happens to the
  worktree. Nothing reaches git, so a clone of main and a consumer's Cargo fetch carry none of it.
- **Kept while anything references it.** A reference index, rebuilt from what Symphony already
  reads (the documents on the default branch, the cards on open branches, the board's briefs and
  threads), says who names each artifact. When the space passes the cap, the check lists only what
  nothing references, largest first, for an agent to look at and remove; nothing is removed by a
  timer.
- **Not recommended.** Git LFS commits a pointer into main for every file, copies every file into
  each worktree, and on GitHub frees storage only when the repository is deleted. Git notes keep
  every artifact in one history. A ref of their own outside every branch works and costs a clone
  nothing, but puts every artifact into the GitHub repository's size; it stays the way to give a
  second machine its own copy, should one need it.
- **Not verified:** whether GitHub keeps a pushed ref outside `refs/heads/` and `refs/tags/`, and
  what deleting one frees there; nothing was pushed to find out.

The owner, 2026-11-12, on where a swept worktree's artifacts go: "images should be kept, build
should not. Images and other artifacts can be pushed into a main tree. However there should be a cap
to the size of the folder, after which it should ask/warn agents to manually check and remove stale
images and artifacts not in use. Maybe 5 GB limit?" and then: "Instead of directly commiting,
artifacts could be attached some with some different process. Either nodes, or git files? Forgot the
name but I think there was something for this type of challenge in particular. But if there's
something that still actively references the artifact, then it should be kept until space used
becomes a problems requiring it to be remove."

How facts are marked: a fact about the code names the file it was read in; a measurement says how
it was taken; a fact from outside names its source by URL beside it, with the words quoted where
they could be disputed; **Not verified** marks what could not be checked. "Read 2026-11-12" is this
machine at about 15:00 that day: Symphony's main at `6029a28f`, every worktree `git worktree list`
names read in place, Star Foundry's checkout and worktrees read in place, and both changed in
nothing. Sizes are in GiB and MiB (powers of two); GitHub's quotas are in GiB too.

![Today: an artifact is a path in one checkout](attached-artifacts/today.svg)

## Today

### Where they are

Measured with `du`, `find` and `stat` over every checkout's artifacts folder, read 2026-11-12:

| | Checkouts with an artifacts folder | Files | Size | Pictures |
|---|---|---|---|---|
| This repository, 41 checkouts | 17 (one empty) | 5,788 | 4.62 GiB | 1,453 files, 248.9 MiB |
| of which its main checkout | 1 | 1,782 | 234 MiB | 1,366 files, 233.5 MiB |
| Star Foundry's main checkout | 1 | 295,963 | 17.2 GiB | 18,863 files, 5.46 GiB |
| Star Foundry's 388 worktrees | 388 | not counted | 219 GiB (`du`) | 9,138 files, 1.88 GiB |

- **This repository's pictures are its tasks' evidence**: 1,427 of the 1,453 are in folders named
  for a task (`app/header-names/`, `tools/task-timeline/`), as AGENTS.md asks ("Pictures kept as a
  task's evidence go under the project's artifacts folder in the task's slug"). Half are under
  144 KiB, nine in ten under 335 KiB, the largest 859 KiB. 1,384 of them are distinct by content
  (237.8 MiB): little is stored twice.
- **The rest is scratch data, not evidence**: 3.35 GiB is one review's scratch repository and board
  (`hg-review`), 1.0 GiB two worktrees' test proofs (`standalone-proof`), and Star Foundry's 219 GiB
  in worktrees is mostly per-agent folders of builds and boards (one holds 46 GiB). The owner's
  "images should be kept, build should not" already sorts these: none of it is to be attached.
- **Star Foundry's pictures** total 28,001 files and 7.35 GiB; half are under 30 KiB, nine in ten
  under 730 KiB, and one of them is over 10 MiB (12.5 MiB).

### How they are named

- **Evidence by path.** A thread post names a picture by its path in a checkout: `artifacts/SLUG/…`
  in the main checkout, or `.claude/worktrees/agent-ID/artifacts/…` in a worktree. Of the board's
  2,041 posts (307 tasks, every thread read with `just tasks show --json`), 123 name the artifacts
  folder: 23 distinct files and 48 task folders. 14 posts name a worktree's path, in 8 worktrees;
  6 of those 8 are gone, so those posts' paths lead nowhere. For two of the six a folder of the
  same name is in the main checkout; the other four's are nowhere on this machine.
- **The owner's uploads by content.** A file the owner attaches in the app is stored once under its
  SHA-256 in the main checkout's `artifacts/uploads/`, named `/artifacts/uploads/<sha256>.<ext>`
  in the post, and "a file posted from a branch page shows on every branch"
  ([REVIEW](../REVIEW.md#attaching-files); `crates/symphony/src/uploads.rs`). The route is
  answered before any branch's tree (`apps/symphony-server/src/api.rs`). Three are stored today,
  1.1 MiB.
- **Card pictures committed.** This register's cards show 81 pictures, every one a file committed
  under `docs/` (50 pictures, 4.27 MiB, on main); 61 picture blobs, 5.2 MiB, were ever committed
  there, 39 of them on 2026-11-11 and 11 on 2026-11-12 (`git log` and `git cat-file` over main).
  The docs audit refuses an open card whose picture is not on disk, and skips uploads
  (`crates/symphony/src/docs_audit.rs`, `card_pictures`); a link into the artifacts folder that is
  not on disk is a warning, "not on this machine" (`artifacts`, same file).
- **Star Foundry's documents by path into its ignored folder.** Its 143 Markdown documents show
  1,533 pictures; 1,491 point into `artifacts/`, 1,340 distinct paths. 1,310 of those are in its
  main checkout (2.47 GiB) and 30 are not: they are broken already, on this machine. None shows on
  any other machine or in any clone.

### What breaks

- **A worktree goes, its paths go.** The sweep in progress on the board task
  `tools/sweep-removes-nothing` moves a finished worktree's artifacts into the main checkout's
  `artifacts/swept/NAME/` before removing it, with a 5 GB cap and a warning: nothing is lost, but
  every path a thread named in that worktree now points at nothing. Before it, a removed worktree's
  ignored files went with it (`git worktree remove` deletes them), as four of the six gone
  worktrees' folders above did.
- **Another machine sees nothing.** The artifacts folder is ignored (`/artifacts/` in
  `.gitignore`), so a clone, a consumer's Cargo checkout and any server on another machine lack
  every file; REVIEW says so plainly: "The app shows them on this machine and nowhere else"
  ([Limits, honestly](../REVIEW.md#limits-honestly)).

### How fast they grow

By each file's modification time, of what is still on disk (files in removed worktrees are not
counted, so these are lower bounds):

| Day | This repository's pictures | Star Foundry's pictures |
|---|---|---|
| 2026-11-06 to 2026-11-10 | none (this repository's folder starts on 11-11) | 49 MiB in all: 16 on 11-07, 7 on 11-08, 25 on 11-09, 1 on 11-10 |
| 2026-11-11 | 600 files, 94 MiB | none |
| 2026-11-12, to 15:00 | 853 files, 155 MiB | none |

This repository adds about 125 MiB of pictures a day while its board is this busy, so a 5 GiB cap
holds about 40 days of it if nothing is removed. Star Foundry's pictures came mostly in October
(7,756 files, 1.7 GiB, in its worktrees alone); its main checkout's artifacts are past 5 GiB of
pictures already.

### What a clone and a consumer's fetch cost today

- **A clone of main** is a 99 MiB pack (`git count-objects`), 1,353 commits since 2026-11-02. Of
  it, 65.6 MiB is the client distribution (2,922 blobs, 474 MiB before compression) and 4.9 MiB
  the committed pictures; everything else, code and documents, 4.5 MiB.
- **A consumer's Cargo fetch.** Star Foundry's Cargo keeps a bare copy of Symphony's history per
  URL, 78 and 87 MiB, and a checkout per pinned commit: 20 of them, 1.7 GiB, each about 379 MiB,
  of which 272 MiB is the client distribution and 4.9 MiB `docs/` with 47 pictures (`du` of
  `~/.cargo/git`, read only). Cargo fetches whole history: shallow fetches are still the unstable
  `-Zgit=shallow-deps` ([unstable](https://doc.rust-lang.org/cargo/reference/unstable.html)).
- **Artifacts add nothing to either today**, being ignored; committed pictures add their bytes to
  every clone and every pinned checkout for good.

## The mechanisms

### Git LFS

- **What is committed.** A pointer of about 130 bytes names the file's SHA-256 and size; the bytes
  go to the LFS server on push (tried below). Every worktree checks out the full file from the
  repository's LFS cache; `git lfs dedup` exists to turn those copies into copy-on-write clones
  afterwards ("re-creating working tree files as clones of the files in the Git LFS storage
  directory", `git lfs help dedup`, git-lfs 3.8.0), so a plain checkout copies.
- **GitHub's quota.** GitHub Free and Pro include "10 GiB bandwidth and 10 GiB storage", Team 250
  GiB of each ([Git LFS billing](https://docs.github.com/en/billing/concepts/product-billing/git-lfs));
  beyond it "storage for Git LFS files will cost $0.07 per GiB per month (USD)" and "download
  bandwidth will cost $0.0875 per GiB"
  ([changelog, 2024-06-03](https://github.blog/changelog/2024-06-03-new-enterprise-accounts-have-metered-billing-for-git-lfs/)).
  With a $0 budget, over quota "You will not be able to push new files back up" and over bandwidth
  "Git LFS support is disabled on your account until the next month" (the billing page).
- **Storage is never given back.** "Git LFS storage is calculated based on all Git LFS objects
  associated with a repository, regardless of when they were uploaded"
  ([storage and bandwidth](https://docs.github.com/en/repositories/working-with-files/managing-large-files/about-storage-and-bandwidth-usage)),
  and "To remove Git LFS objects from a repository, delete and recreate the repository", or ask
  support ([removing files](https://docs.github.com/en/repositories/working-with-files/managing-large-files/removing-files-from-git-large-file-storage)).
- **Bandwidth is each download.** "When you download a Git LFS file, the bandwidth you use is
  included in the repository owner's bandwidth usage", GitHub Actions included (storage and
  bandwidth page).
- **Cargo does not fetch LFS objects.** An open issue since 2021: "Right now, the lfs objects are
  not retrieved"; a Cargo maintainer's comment: "Cargo internally uses libgit2 which is not aware of
  git-lfs" ([cargo#9692](https://github.com/rust-lang/cargo/issues/9692)). Tried below with Cargo
  1.99: the checkout holds the pointer, with or without `net.git-fetch-with-cli`.

### A ref of their own

- **Git allows it, a clone skips it.** A clone "creates remote-tracking branches for each branch in
  the cloned repository" ([git-clone](https://git-scm.com/docs/git-clone)); only `--mirror` "maps all
  refs (including remote-tracking branches, notes etc.)". So a ref such as `refs/artifacts/SLUG`,
  holding a commit whose tree is that task's files, reaches only who fetches it by name (tried
  below).
- **Cargo never fetches it.** Cargo's fetch names one ref: `+refs/heads/{b}` for a branch, the
  tag for a tag, `+{oid}:refs/commit/{oid}` for a locked commit, and only for a revision it cannot
  resolve "we fetch all branches and tags, and then we pray" (`src/sources/git/utils.rs`, `fetch`,
  in [rust-lang/cargo](https://github.com/rust-lang/cargo), read at master 2026-11-12). The Cargo
  copy of Symphony fetched from GitHub on this machine holds 15 `refs/commit/…` refs and
  `refs/remotes/origin/main`; another, fetched from this checkout over `file://`, holds 135
  remote-tracking branches, so the fetch of every branch does happen (each copy's `packed-refs`,
  read).
- **GitHub's limits.** "GitHub blocks files larger than 100 MiB", and "We recommend repositories
  remain small, ideally less than 1 GB, and less than 5 GB is strongly recommended"
  ([large files](https://docs.github.com/en/repositories/working-with-files/managing-large-files/about-large-files-on-github)).
  **Not verified:** that GitHub keeps a pushed ref outside `refs/heads/` and `refs/tags/`, whether
  its objects count in the repository's size, and what deleting the ref frees and when; GitHub's
  documentation says nothing of it, and nothing was pushed to find out.

### Git notes

- "Commit notes are blobs containing extra information about an object", kept under a notes ref,
  `refs/notes/commits` by default ([git-notes](https://git-scm.com/docs/git-notes)).
- **Text, by design.** A binary note must be made with `git hash-object` and `notes add -C`, as
  `notes add -F` "is not binary-safe", and "it doesn't make much sense to display non-text-format
  notes with `git log`, so if you use such notes, you'll probably need to write some special-purpose
  tools" (same page).
- **Keyed on a commit, one history for all.** A note hangs on one object, and every note in a ref
  lives in that ref's one history: fetching it fetches every note ever kept there, and dropping one
  note's bytes for good means rewriting that history and force-pushing it. A rebased or squashed
  commit loses its note unless `notes.rewriteRef` is set (same page).
- **GitHub shows none**: "Displaying Git notes on GitHub is no longer supported" (an update of
  2014-08-14 on [Git Notes Display](https://github.blog/news-insights/git-notes-display)).

### Outside git

- **On the machine.** The owner's uploads are already a store addressed by content in the main
  checkout, served at a name that is the same on every branch (above).
- **Off the machine, when it is wanted.** An object store, such as Cloudflare R2 at "$0.015 /
  GB-month" with "10 GB-month / month" free and egress "Free"
  ([R2 pricing](https://developers.cloudflare.com/r2/pricing/)), or GitHub's release assets: "Each
  file included in a release must be under 2 GiB. There is no limit on the total size of a release,
  nor bandwidth usage", at most 1,000 assets a release
  ([releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases)),
  but "Releases are based on Git tags", and a clone fetches tags.
- **Several machines, one board.** The runs design already says how a second machine reads what
  stays on the first: "Logs stay on their machine", and a log "opens on demand when its machine
  serves" ([reports](reports-and-monitoring.md#several-machines-one-board)).

### Tried on a scratch repository

A script in a scratch folder made a bare repository, pushed to it over `file://`, cloned it and
fetched it with Cargo, nothing on this repository or its remote: one crate with a random 200 KiB
`docs/pics/evidence.png` tracked by LFS, the same bytes as a task's commit under
`refs/artifacts/tools/example` and as a binary note under `refs/notes/artifacts`. git 2.54.0,
git-lfs 3.8.0, Cargo 1.99.0. The B and Cargo lines came out the same in three runs; C and D in the
two runs after the first, which had stored the LFS pointer in their place (`git hash-object` ran
the LFS filter until given `--no-filters`):

- **B.** The commit holds `version https://git-lfs.github.com/spec/v1`, the `oid sha256:` and
  `size 204800`; the remote holds one LFS object. A plain clone gets the 204,800 bytes; one with
  `GIT_LFS_SKIP_SMUDGE=1` gets the 131-byte pointer.
- **Cargo and B.** A consumer naming the crate as a git dependency: `cargo fetch` checks out the
  131-byte pointer, the same with `CARGO_NET_GIT_FETCH_WITH_CLI=true`, and the consumer builds,
  since nothing compiles the picture.
- **C and D.** The remote lists `refs/artifacts/tools/example` and `refs/notes/artifacts`. A plain
  clone holds only `refs/heads/main` and its remote-tracking refs, and Cargo's copy only
  `refs/remotes/origin/main`. In the last run a bare clone took 12 KiB of objects and a mirror
  clone 212 KiB, the picture once (the ref and the note share one blob). A fetch of the one ref by name brings that
  task's tree; a fetch of the notes ref brings the note, 204,800 bytes.

## The options

The table first, then each. Numbers are this repository's unless a row names Star Foundry.

| | A. The folder, with the cap | B. Git LFS | C. A ref of their own | D. Git notes | E. A store by content |
|---|---|---|---|---|---|
| Committed to main | nothing | a pointer per file, for good | nothing | nothing | nothing |
| A clone of main adds | 0 | the pointers; a clone that checks out downloads every file in the tree: 249 MiB here, up to 7.35 GiB for Star Foundry | 0 (tried); a mirror clone adds every ref | 0; a fetch of the notes ref adds every note at once | 0 |
| A consumer's Cargo fetch adds | 0 | the pointers, never the files (tried) | 0 (Cargo's source; tried) | 0 | 0 |
| The host's quota and cost | none | 10 GiB of storage and 10 GiB a month of downloads free, then $0.07 a GiB a month and $0.0875 a GiB | the repository's size: under 5 GB strongly recommended | as C | none |
| Removing gives the space back | yes | here; never on GitHub, short of deleting the repository | here; on GitHub not documented | only by rewriting the notes history | yes |
| Each worktree's disk | its own files | a copy of every file in its tree: 41 × 249 MiB here | none | none | none |
| A link on another machine | never | once that machine fetched the file | once it fetched the task's ref | once it fetched every note | once its server asks this one, later |
| The server, when the bytes are not here | a broken picture | the pointer, a broken picture, unless it fetches first | fetches the task's ref, or a broken picture offline | fetches the notes ref | a broken picture: here only once an agent removed it, elsewhere until its server asks this one |
| Needs on every machine | nothing | git-lfs | nothing new | nothing new | nothing new |

### A. The folder, with the cap

The ignored folder as it is, with the sweep's move of a finished worktree's artifacts into the main
checkout and its 5 GB cap and warning.

- **A link** names a path in a checkout. It breaks when the sweep moves a worktree's folder aside,
  and never works on another machine.
- **Retention**: the cap warns, and an agent decides from paths alone what is still in use; nothing
  says which post or card names a file.
- **Costs** nothing new, and fixes nothing of the broken links.

### B. Git LFS

The artifacts folder leaves `.gitignore`, `git lfs track` takes its pictures, and a task's evidence
is committed on its branch.

- **It is committed to main**: a pointer per file reaches main with every landing, which is the
  committing the owner asked to avoid, only smaller. Kept out of main, on a branch of their own,
  it is C with LFS's costs added.
- **A link** names the committed path, `artifacts/SLUG/x.png`, which shows wherever the file was
  fetched: on another machine once its clone pulled the LFS object, on a branch's view only while
  that branch holds the file.
- **Removing frees nothing on GitHub**: every object ever pushed counts until the repository is
  deleted, so the owner's check could remove a file here and never get the host's space back. At
  this repository's 125 MiB a day the free 10 GiB is full in about 80 days, and it only grows:
  about $0.70 a month for each further 10 GiB. Star Foundry's 7.35 GiB of pictures fit, but one clone
  that checks out downloads up to all of them, most of a month's free 10 GiB.
- **Every worktree copies every file**: 41 worktrees here hold 41 copies of 249 MiB, about 10 GiB,
  and Star Foundry's 388 would hold 388 copies of 7.35 GiB, more than this disk, unless each runs
  `git lfs dedup` or skips the files, and then holds pointers.
- **A consumer's Cargo fetch** gets pointers (tried), harmless while no build reads them; a file a
  build embeds can never be under LFS.
- **The server** must tell a pointer from a picture and fetch the object before serving it, or it
  serves 131 bytes of text as a PNG.

### C. A ref of their own

Each task's artifacts are a commit with no parent under `refs/artifacts/SLUG`, pushed to the remote
and fetched by name when wanted; one ref a task, so deleting it drops only that task's files.

- **A clone of main and a consumer's fetch carry none of it** (tried, and Cargo's fetch names one
  ref). A mirror clone, or a fetch of `+refs/*:refs/*`, takes everything.
- **Not an orphan branch.** A branch with no common history is still a branch: every clone takes
  every branch, and Cargo's fetch of a revision it cannot resolve takes them all, so an orphan
  branch of artifacts would reach every clone. The ref has to sit outside `refs/heads/`.
- **The host's size**: every artifact pushed counts in the repository, where GitHub strongly
  recommends under 5 GB; this repository's pictures would reach that in about 40 days with nothing
  removed, and Star Foundry's 7.35 GiB are past it now. Whether GitHub keeps such a ref, counts it,
  and frees it when deleted is not documented: **not verified**.
- **A link** names the file by content (below); the server finds it in the ref's tree, fetching the
  ref when this machine lacks it.
- **Costs**: a push per attachment or per task, a fetch before a picture shows on a new machine,
  and git commands in the server's path.

### D. Git notes

A note on the task's commit holds the file, or a list of the files' hashes with the bytes kept as
in C or E.

- **Holding the files**, D is C with every task in one history: fetching any picture fetches all of
  them, and removing one rewrites the notes history and force-pushes it. A note follows a commit,
  but evidence belongs to a task whose commits are rebased, squashed or never landed.
- **Holding a list**, the bytes still live in C or E, and the list is what the reference index
  below keeps without git. A link names the file by content, as under C and E.
- **GitHub shows no notes**, and every reader needs a tool of its own.

### E. A store addressed by content (recommended)

![Proposed: an artifact is attached by its content and kept while anything names it](attached-artifacts/proposed.svg)

The owner's uploads, opened to agents. One command copies a file into the main checkout's
`artifacts/uploads/<sha256>.<ext>` (the project's artifacts folder, through `Project::artifact`)
and prints the line to post. The main checkout rather than the machine folder, because "A project's
state stays in its checkout: paths under its artifacts folder go through `Project::artifact`"
(AGENTS.md), and the uploads are there already.

- **A link never moves.** `/artifacts/uploads/<sha256>.png` is answered from the main checkout on
  every page, every branch's view (`/@tree/…`) and the phone, before any tree is read; a worktree
  swept away takes nothing with it.
- **Nothing reaches git**: no clone, mirror or Cargo fetch grows; no host quota is spent.
- **Removing gives the space back** at once, and only an agent's check removes.
- **Dedup for free**: the same picture taken twice is stored once (1,453 pictures here are 1,384
  contents).
- **Costs**: one machine holds the only copy, so a lost disk loses the evidence (as today); the
  store takes what uploads take, PNG, JPEG, GIF and WebP pictures and the listed documents up to
  10 MiB each, and refuses SVG and HTML, which stay committed where a document needs them. A second
  machine serving the board shows a picture only once its server asks this machine's for it, as run
  logs are read on demand: that send is built when there is a second machine, not now.

## Naming an artifact

Under C, D and E alike an artifact is named by the SHA-256 of its bytes, so the name never depends
on where the bytes are kept and a later move from E to C or to an object store changes no link:

- **On a thread or a card**, the line the attach command prints:
  `![board on a phone](/artifacts/uploads/<sha256>.png)`, or `[report](/artifacts/uploads/<sha256>.pdf)`
  for any other kind. It is the line the owner's uploads already post, so the app already draws it
  on a card and in an agent's thread post, which is drawn from its Markdown
  (`crates/symphony/src/uploads.rs`, `reference`; `apps/symphony-ui/src/task_detail.rs`,
  `TaskMessage`).
- **In a document**, the same name. A picture a document argues with, such as this document's two
  diagrams, can stay committed beside it, as today; a screenshot kept as evidence is attached.
- **Old paths.** A path a post already names (`artifacts/SLUG/x.png`, a worktree's) keeps working
  through a table the sweep writes when it attaches a finished worktree's pictures: old path to
  hash, read by the server when a path is not on disk.

## Retention

The owner's rule: an artifact that anything still references is kept; an
unreferenced one is kept too until space becomes a problem, and only then is it a candidate,
removed by an agent's check, never by a timer.

- **Space becomes a problem** when the project's artifacts folder (the store, `swept/` and the task
  folders) passes the cap, 5 GB as the owner suggested, which the sweep's warning already measures,
  or when free disk drops under the board's low-disk line (20 GB, `SF_LOW_DISK_GB`).
- **Then the check** lists the unreferenced candidates, largest first, each with its size, when it
  was attached and the task that attached it, and the total that removing them would free. An agent
  looks and removes with one command, which refuses any file the index finds referenced at that
  moment. No timer, no automatic removal.
- **If what is referenced alone passes the cap**, the check says so and names the largest
  referenced files and who names them; what to drop is then the owner's question, on a card.
- **Scratch data is not an artifact**: the sweep's own rule for leftovers (the owner: "build should
  not") applies to it, and nothing here attaches it.

## The reference index

### What counts as a reference

| Referrer | Counts while | What names an artifact |
|---|---|---|
| a card | it is not resolved, in a register on the default branch or on an open branch | its lines |
| a task | it is not closed | its brief and its thread, and its own folder `artifacts/SLUG/` |
| a document | it is on the default branch (every file `Project::walk` lists, the registers included) | its text |
| a thread post | always, on any task, open or closed | its text |
| an open ask | until it is deleted, a day after its last message | its messages |

So a resolved card on main still keeps its pictures, being in a document on the default branch, and
evidence named on any thread is never a candidate: what becomes one is what nothing names, a
screenshot never posted, a test's leftover, a scratch folder.

**A name** is a stored name, `/artifacts/uploads/` and 64 hex digits (exact, so cheap to find), or
an older path into the artifacts folder of the main checkout, a worktree or a `/@tree/` view; a
folder named by a post (`artifacts/app/header-names/`) references every file under it.

### How it is found cheaply

- **One file per project**, in its artifacts folder (`uploads/references.json`), mapping each name
  to its referrers: a document's path, a card's slug and branch, a task's slug and message id. It is
  rebuilt from the sources whenever it is lost, so it is not state to keep safe. The references
  themselves are what the board already keeps, its briefs and posts; a table of them on the board
  would be a schema raise, which refuses every older writer of the board until it is rebuilt
  ([plugin delivery](plugin-delivery.md#the-board)), for data that can be derived.
- **Updated by difference.** Documents: `git diff --name-only` of the default branch between the
  commit the index last read and its tip, reading only the Markdown files that changed; branches:
  the cards the server's branch scan already reads (`crates/symphony/src/branches.rs`); the board:
  posts whose id is above the last one read (ids only grow; this thread's first post is 2039) and
  briefs whose task changed since; a task's close moves its folder out of the referenced set.
- **Measured, a full rebuild is cheap**: a regular expression over Star Foundry's 143 documents
  (22 MiB) took 0.07 to 0.12 s, three runs at load 49 on 16 cores, and over this board's 2,041
  posts once read (2.4 MiB) 0.03 s. Reading the posts from the board is the cost; reading only new
  ones keeps it small.
- **Read when needed**: by the cap's check and the removal command, and by a listing of the store
  (`symphony-cli artifacts`: each file's size, attached when and by which task, and what names it),
  never on a page view.

## Why E

E is the only option in which a link never breaks on this machine and removing a file gives its
space back, with nothing added to git or to the host's quota. It is the owner's uploads, already
built and served on every branch, opened to agents. B commits pointers into main and makes the
owner's check powerless on GitHub; D keeps everything in one history; C is the sound way to give a
second machine its own copy and costs a clone nothing, but every artifact counts against a GitHub
repository's size, which Star Foundry's pictures already pass. Since every option but A names an
artifact by its content, choosing E now leaves C, or an object store, open later as a copy behind
the same names.

## The implementation tasks

In order, each landing before the next reads it:

1. **Attach.** `just symphony-cli attach FILE… [--task SLUG]` copies each file into the store under
   the uploads' rules, prints its Markdown line, and records the attaching task; `symphony-cli
   shot --attach` does the same for a page shot. AGENTS.md, the shipped skills and CHECKS say to
   attach evidence and post the line, and the docs audit checks an attached card picture is in the
   store.
2. **The reference index**: the file, its updates by difference, its rebuild, and `symphony-cli
   artifacts` listing the store with what names each file.
3. **The cap's check**: after `tools/sweep-removes-nothing` lands its cap and warning, the warning
   counts the store and lists the unreferenced candidates, and a removal command refuses a
   referenced file; never a timer.
4. **The sweep attaches named evidence**: before it removes a finished worktree, the pictures its
   tasks' threads name are attached, and the table of old path to hash is written, so a post that
   named the worktree's path still shows its picture.
5. **A note for Star Foundry's board**: its documents' 1,340 pictures under its ignored folder can be
   attached by one rewrite of their links, which is Star Foundry's call.

Not filed: a second machine's server asking this one for a picture, and a copy off this machine
(C or an object store); each when it is wanted.

## Open questions

1. **Size.** Attachments take the uploads' 10 MiB a file; one of Star Foundry's 28,001 pictures is
   larger (12.5 MiB). Raise it for agents, or keep it?
2. **A copy off the machine.** E keeps one copy, as today. Is a lost disk's evidence acceptable
   until a second machine or an object store is wanted?
3. **Posts on closed tasks** count as references for good, so evidence named on any thread is never
   a candidate. If that alone passes the cap, the check asks; should closed tasks older than some
   age stop counting instead?

## Verified and inferred

- **Read in code:** each fact that names a file, in Symphony at `6029a28f`; Cargo's `fetch` at its
  master, 2026-11-12.
- **Measured:** sizes, counts and file times of every artifacts folder of this repository's 41
  checkouts and of Star Foundry's main checkout and 388 worktrees (`du`, `find`, `stat`, `shasum`);
  the 2,041 posts of this board's 307 tasks (`just tasks list --all --json` and `show --json`);
  Star Foundry's document pictures (`grep` of its Markdown, then `stat`); this repository's history
  (`git count-objects`, `git rev-list --objects`, `git cat-file --batch-check`); Star Foundry's
  Cargo copies of Symphony (`du`, its `packed-refs`); the scratch trials; the full scan's time.
- **Inferred:** growth per day from file times, which miss what removed worktrees held; days to fill
  a cap from two days of this board's work; LFS's cost from GitHub's published prices.
- **From outside:** each claim beside its URL, read 2026-11-12.
- **Not verified:** whether GitHub keeps a ref outside heads and tags, counts its objects in the
  repository's size and frees them when it is deleted; whether git-lfs copies or clones files at
  checkout on APFS beyond what `git lfs dedup`'s help implies.
- **Not tried:** any option beyond the scratch trials; nothing here was built.
