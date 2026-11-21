# Artifacts attached, not committed

Where a task's pictures and other evidence live once they leave the worktree that made them, how a
card, a thread post or a document names one so the link works where it is read, and how long each
is kept. This is a design: of its tasks, the second is built (the committed half), and the rest are
not yet. F, the split, is chosen on its card in [DECISIONS](../../DECISIONS.md). The tasks that
build it are on the
board, in the order of [The implementation tasks](#the-implementation-tasks).

- **The requirement: a clone shows what the documents show.** A git clone shows the images,
  above all those the documents reference, those of past decisions included.
- **Which options a plain `git clone` passes.** A plain `git clone` shows a picture only when the
  picture is committed:
  - **Committing it** beside the document, as card pictures are today, passes with no extra command.
  - **B, Git LFS,** passes where git-lfs is installed and the account's LFS quota is intact or
    paid. Without git-lfs, the clone holds pointers until `git lfs pull`. Past the quota with no
    payment method, a clone gets only pointers until the next month.
  - **A and E** never put a picture in a clone, so they are excluded.
  - **C and D** are excluded too. Their files sit outside every branch: a clone skips them, and
    even a fetch by name never puts them where a document's link points.
- **Chosen: F, a split.** Recommended here, and chosen.
  - **What the documents show is committed.** A picture that a document or a decision card shows is
    committed beside it, as card pictures are now.
  - **Working evidence stays local.** Evidence that no document shows stays out of git, in the
    main checkout's store by content (E's store), under the cap of the sweep that lands first.
- **What each half costs, at today's growth.**
  - **The committed half, here:** 4.3 MiB on main, growing by about 2.5 MiB a day while the
    register is this busy. Every clone and every consumer's Cargo copy carries it, for good, and
    every checkout holds all of it: each worktree and each consumer's pinned Cargo checkout, 4.3 MiB
    each today, about 0.9 GiB each after a year at that pace.
  - **The committed half, for the consuming project:** its documents show 2.47 GiB of pictures that
    are still in its ignored folder. Committed plainly, that is 2.47 GiB in each of its worktrees,
    about 0.94 TiB across today's 388, and that decides plain commits against LFS there.
  - **The local half:** about 125 MiB a day of pictures here, held under 5 GB on this one disk,
    with no copy anywhere else.
- **Evidence posted only on a thread is in no clone under any option but committing it.** The
  board's threads are not in a clone either.
- **Nothing that reaches GitHub through main comes back off.**
  - An LFS object is removed only by deleting the repository or through support.
  - A commit is removed only by rewriting main and force-pushing it, which this repository never
    does.

  So only what a document shows goes there.
- **One rule, one cap, one kept folder, shared with the sweep.** The sweep's rule is the owner's. A
  file is kept while something still actively references it:
  - a task not done or cancelled;
  - a post on an open task's thread;
  - a card not resolved;
  - a document on the default branch or on an open task's branch;
  - an open ask.

  Past the cap (`[board] swept_cap_gb`, 5 GB unless the project sets its own), an agent removes
  what nothing references, closed tasks' files first. Nothing is removed on a timer.

The ask, on where a swept worktree's artifacts go: images are kept, build output is not. Images
and other artifacts can go into a main tree, with a cap on the folder's size (5 GB, say), past which
agents are asked to check by hand and remove stale images and artifacts no longer in use. Rather
than committed directly, artifacts could be attached by some other process made for this kind of
problem. What something still actively references is kept until the space used becomes a problem
that requires removing it. Then, on the card: a git clone shows the images, above all those the
documents reference, those of past decisions included, which excludes E and A; which option is
recommended; and, if B, whether pushed objects can be overwritten to delete them.

How facts are marked:
- A fact about the code names the file it was read in.
- A measurement says how it was taken.
- A fact from outside names its source by URL beside it, with the words quoted where they could be
  disputed.
- **Unknown** marks what no documentation says and no trial here could show. **Not verified**
  marks what could have been checked and was not.

"Read 2026-11-12" is this machine at about 15:00 that day, with Symphony's main at `6029a28f`.
Every worktree that `git worktree list` names, and the consuming project's checkout and worktrees,
were read in place and changed in nothing. GitHub's sizes, the backup, the consuming project's build
folders and its documents' pictures by month, and the open tasks' posts were read at about 16:00.
Sizes are in GiB and MiB (powers of two), as GitHub's quotas are; the sweep's cap is in GB (powers
of ten), as the sweep counts it.

![Today: an artifact is a path in one checkout](attached-artifacts/today.svg)

## Today

### Where they are

Measured with `du`, `find` and `stat` over every checkout's artifacts folder, read 2026-11-12:

| | Checkouts with an artifacts folder | Files | Size | Pictures |
|---|---|---|---|---|
| This repository, 41 checkouts | 17 (one empty) | 5,788 | 4.62 GiB | 1,453 files, 248.9 MiB |
| of which its main checkout | 1 | 1,782 | 234 MiB | 1,366 files, 233.5 MiB |
| The consuming project's main checkout | 1 | 295,963 | 17.2 GiB | 18,863 files, 5.46 GiB |
| The consuming project's 388 worktrees | 388 | not counted | 219 GiB (`du`) | 9,138 files, 1.88 GiB |

- **This repository's pictures are its tasks' evidence.** 1,427 of the 1,453 are in folders named
  for a task (`app/header-names/`, `tools/task-timeline/`), as AGENTS.md asks: "Pictures kept as a
  task's evidence go under the project's artifacts folder in the task's slug". Half are under
  144 KiB, nine in ten under 335 KiB, and the largest is 859 KiB. 1,384 of them are distinct by
  content (237.8 MiB), so little is stored twice.
- **The rest is scratch data, not evidence.**
  - 3.35 GiB is one review's scratch repository and board (`hg-review`).
  - 1.0 GiB is two worktrees' test proofs (`standalone-proof`).
  - The consuming project's 219 GiB in worktrees is mostly per-agent folders of coverage profiles,
    builds and boards. Only 20.6 GiB of it is in folders tagged as caches (`CACHEDIR.TAG`, as Cargo
    tags its build folders), and one worktree holds 40 GiB of coverage profiles.
- **The consuming project's pictures** total 28,001 files and 7.35 GiB. Half are under 30 KiB, nine
  in ten are under 730 KiB, and one is over 10 MiB (12.5 MiB).

### How they are named

- **Evidence by path.** A thread post names a picture by its path in a checkout:
  `artifacts/SLUG/…` in the main checkout, or `.claude/worktrees/agent-ID/artifacts/…` in a
  worktree. Every thread was read with `just tasks show --json`: 2,041 posts on 307 tasks.
  - 123 of those posts name the artifacts folder: 23 distinct files and 27 distinct folders, from
    45 tasks' threads. The folders they name hold 1,422 of the 1,453 pictures.
  - 14 posts, all on tasks now done, name a path in one of 8 worktrees.
  - 6 of those 8 worktrees are gone, so those paths lead nowhere. The other 2 go when the sweep
    takes them.
  - For two of the six, a folder of the same name is in the main checkout. The other four's folders
    are nowhere on this machine.
- **The owner's uploads by content.** A file the owner attaches in the app is stored once, under its
  SHA-256, in the main checkout's `artifacts/uploads/`, and the post names it
  `/artifacts/uploads/<sha256>.<ext>`. REVIEW says "a file posted from a branch page shows on every
  branch" ([REVIEW](../REVIEW.md#attaching-files); `crates/symphony/src/uploads.rs`). The server
  answers that route before reading any branch's tree (`apps/symphony-server/src/api.rs`). Three
  are stored today, 1.1 MiB, and no register names any of them.
- **Card pictures committed.**
  - This register's cards show 81 pictures. Every one is a file committed under `docs/`: 50
    pictures, 4.27 MiB, on main (41 raster pictures and 9 SVGs).
  - 61 picture blobs, 5.2 MiB, were ever committed there. Of the raster ones, 41 (4.0 MiB) came on
    2026-11-11 and 8 (1.1 MiB) on 2026-11-12 (`git log` and `git cat-file` over main).
  - The docs audit refuses an open card whose picture is not on disk, and skips uploads
    (`crates/symphony/src/docs_audit.rs`, `card_pictures`).
  - A link into the artifacts folder that is not on disk gets a warning, "not on this machine",
    looked for under the audit's own checkout (`artifacts`, same file).
  - No document here links a picture in the artifacts folder.
- **The consuming project's documents by path into its ignored folder.** Its 143 Markdown documents
  show 1,533 pictures.
  - 1,491 of them point into `artifacts/`, at 1,340 distinct paths. Most are in its design documents
    (one of them alone shows 290), and 23 are in its register.
  - 1,310 of those paths are in its main checkout: 2.47 GiB, all distinct, 1,276 of them PNG.
  - The other 30 are already broken, on this machine.
  - None of these pictures shows on any other machine or in any clone.

### What breaks

- **When a worktree goes, its paths go with it.** This design builds on a sweep that is not yet on
  main. It was read on its branch at `2a934225`, in `crates/tasks/src/sweep/aside.rs` and
  `kept.rs`:
  - Before removing a finished worktree, the sweep moves that worktree's artifacts, less its build
    output, into the main checkout's `artifacts/swept/NAME/`.
  - It removes nothing from there.
  - Past its cap it asks for a check by hand. It marks each of its five largest folders that an open
    task, an open task's thread or a document of `docs/` on main still names.

  Nothing is lost, but every path a thread named in that worktree now points at nothing. Before this
  sweep, a removed worktree's ignored files went with it (`git worktree remove` deletes them). That
  is how four of the six gone worktrees' folders above were lost.
- **A clone and other machines see nothing.** The artifacts folder is ignored (`/artifacts/` in
  `.gitignore`), so a clone, a consumer's Cargo checkout and any server on another machine lack
  every file. REVIEW says so plainly: "The app shows them on this machine and nowhere else"
  ([Limits, honestly](../REVIEW.md#limits-honestly)).
- **This disk holds the only copy.** Time Machine has no destination on this machine (`tmutil
  destinationinfo`, read 2026-11-12). **Not verified:** whether some other tool copies the folder.

### How fast they grow

Growth is read from each file's modification time, of what is still on disk. Files in removed
worktrees are not counted, so these are lower bounds:

| Day | This repository's pictures | The consuming project's pictures |
|---|---|---|
| 2026-11-06 to 2026-11-10 | none (this repository's folder starts on 11-11) | 49 MiB in all: 16 on 11-07, 7 on 11-08, 25 on 11-09, 1 on 11-10 |
| 2026-11-11 | 600 files, 94 MiB | none |
| 2026-11-12, to 15:00 | 853 files, 155 MiB | none |

- **This repository** adds about 125 MiB of evidence pictures a day while its board is this busy.
  At that rate a 5 GiB cap holds about 40 days, if nothing is removed.
- **This repository's documents** gained 5.2 MiB of committed pictures in the two days they have
  had any. That is about 2.5 MiB a day, or about 0.9 GiB a year at that pace.
- **The consuming project's pictures** came mostly in October: 7,756 files, 1.7 GiB, in its
  worktrees alone. The 1,310 pictures its documents show date, by file time, as follows:

  | Month | Files | Size |
  |---|---|---|
  | September | 943 | 2,126 MiB |
  | October | 362 | 401 MiB |
  | November, to the 12th | 5 | 1 MiB |

### What a clone, a consumer's fetch and GitHub hold today

- **A clone of main** is a 99 MiB pack (`git count-objects`), 1,353 commits since 2026-11-02:
  - 65.6 MiB is the client distribution (2,922 blobs, 474 MiB before compression);
  - 4.9 MiB is the committed pictures;
  - 4.5 MiB is everything else, code and documents.
- **A consumer's Cargo fetch.** The consuming project's Cargo keeps two kinds of copy of Symphony:
  - **A bare copy of its history** for each URL: 78 MiB and 87 MiB.
  - **A checkout for each pinned commit:** 20 of them, 1.7 GiB in all, each about 379 MiB. Of each
    checkout, 272 MiB is the client distribution and 4.9 MiB is `docs/`, with 47 pictures (`du` of
    `~/.cargo/git`, read only).

  Two later facts change the checkout's size:
  - Main at `8ce1661d`, later the same day, keeps 19 MiB of client distribution in 104 files. A
    checkout of a newer pin is therefore about 126 MiB, an estimate by subtraction, and its 4.9 MiB
    of pictures stay.
  - Cargo fetches the whole history: shallow fetches are still the unstable `-Zgit=shallow-deps`
    ([unstable](https://doc.rust-lang.org/cargo/reference/unstable.html)).
- **GitHub** reports Symphony's repository at 132,161 KB and the consuming project's at 124,909 KB
  (`gh repo view --json diskUsage`, read only). Both repositories are private.
- **Artifacts add nothing to any of these today**, being ignored. Committed pictures add their bytes
  to every clone and every pinned checkout, for good.

## The mechanisms

### A commit

- **A clone has it.** A committed file is in every clone and every checkout, with no extra
  command.
- **GitHub shows it in a document.** "Links starting with `/` will be relative to the repository
  root", and "GitHub will automatically transform your relative link or image path based on
  whatever branch you're currently on"
  ([relative links](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-readmes)).
- **Every working tree holds its own copy.** That includes each worktree, whatever the size:
  worktrees share git's objects but not their checked-out files.

### Git LFS

- **What is committed.** A pointer of about 130 bytes names the file's SHA-256 and size. The bytes
  go to the LFS server on push (tried below).
- **Where git-lfs is installed, a clone has the file.** Without git-lfs, the clone holds the
  pointer until `git lfs install` and `git lfs pull` (tried below).
- **A worktree can skip the files.** With `GIT_LFS_SKIP_SMUDGE=1` it holds pointers. Otherwise each
  worktree checks out the full file from the repository's LFS cache. `git lfs dedup` turns those
  copies into copy-on-write clones afterwards: "re-creating working tree files as clones of the
  files in the Git LFS storage directory" (`git lfs help dedup`, git-lfs 3.8.0).
- **GitHub's quota.** GitHub Free and Pro include "10 GiB bandwidth and 10 GiB storage", and Team
  250 GiB of each ([Git LFS billing](https://docs.github.com/en/billing/concepts/product-billing/git-lfs)).
  - **Beyond the quota:** GitHub's pricing calculator lists "Additional storage $0.07 per GiB" and
    "additional data transfer out $0.0875 per GiB"
    ([pricing calculator](https://github.com/pricing/calculator)). Storage "is billed by calculating
    an hourly usage rate" (the billing page).
  - **With no payment method, the quota is a wall.** "If your account does not have a valid
    payment method on file, usage is blocked once you use up your quota." With a payment method
    and a budget set to $0: "You are not charged for overages, but Git LFS usage is blocked for the
    rest of the calendar month" (the billing page, read 2026-11-12).
  - **What a clone gets then.** Past the storage quota: "You can still clone repositories with large
    assets", but "You will only retrieve the pointer files", and "You will not be able to push new
    files back up". Past the bandwidth quota: "Git LFS support is disabled on your account until the
    next month" (the billing page). So LFS shows a picture in a clone only while the account is
    inside its quota or pays.
  - **How fast a fresh clone uses it.** Each fresh clone downloads every LFS file in its tree. The
    consuming project's documents show 2.47 GiB, so about four fresh clones a month use the free 10
    GiB of bandwidth, and the fifth gets pointers until the next month, unless the account pays.
- **Bandwidth is each download.** "When you download a Git LFS file, the bandwidth you use is
  included in the repository owner's bandwidth usage", GitHub Actions included
  ([storage and bandwidth](https://docs.github.com/en/repositories/working-with-files/managing-large-files/about-storage-and-bandwidth-usage)).
- **Cargo does not fetch LFS objects.**
  - An issue open since 2021 says "Right now, the lfs objects are not retrieved". A Cargo
    maintainer's comment there says "Cargo internally uses libgit2 which is not aware of git-lfs"
    ([cargo#9692](https://github.com/rust-lang/cargo/issues/9692)).
  - Tried below with Cargo 1.99: the checkout holds the pointer, with or without
    `net.git-fetch-with-cli`, and so a consumer never downloads the pictures.

### A ref of their own

- **Git allows it, and a clone skips it.** A clone "creates remote-tracking branches for each branch
  in the cloned repository" ([git-clone](https://git-scm.com/docs/git-clone)). Only `--mirror`
  "maps all refs (including remote-tracking branches, notes etc.)". So a ref such as
  `refs/artifacts/SLUG`, holding a commit whose tree is that task's files, reaches only those who
  fetch it by name (tried below). Even then, its files are not in the checkout where a document's
  link points.
- **Cargo never fetches it.** Cargo's fetch names one ref (`src/sources/git/utils.rs`, `fetch`, in
  [rust-lang/cargo](https://github.com/rust-lang/cargo), read at master 2026-11-12):
  - `+refs/heads/{b}` for a branch;
  - the tag, for a tag;
  - `+{oid}:refs/commit/{oid}` for a locked commit;
  - and only for a revision it cannot resolve: "we fetch all branches and tags, and then we pray".

  Two Cargo copies of Symphony on this machine show both cases (each copy's `packed-refs`, read):
  - The copy fetched from GitHub holds 15 `refs/commit/…` refs and `refs/remotes/origin/main`.
  - The copy fetched from this checkout over `file://` holds 135 remote-tracking branches, so the
    fetch of every branch does happen.
- **GitHub's documented limits are on size.** "GitHub blocks files larger than 100 MiB", and "We
  recommend repositories remain small, ideally less than 1 GB, and less than 5 GB is strongly
  recommended"
  ([large files](https://docs.github.com/en/repositories/working-with-files/managing-large-files/about-large-files-on-github)).
- **Unknown:** three things about such a ref on GitHub, none of which its documentation mentions and
  none of which was pushed to find out:
  - whether GitHub accepts and keeps a pushed ref outside `refs/heads/` and `refs/tags/`;
  - whether its objects count in the repository's size;
  - what deleting the ref frees, and when.

  An option that depends on these checks them first, on a scratch repository on GitHub. Creating
  one on the owner's account is the owner's to ask for.

### Git notes

- "Commit notes are blobs containing extra information about an object". They are kept under a
  notes ref, `refs/notes/commits` by default ([git-notes](https://git-scm.com/docs/git-notes)).
- **Text, by design.** A binary note must be made with `git hash-object` and `notes add -C`, since
  `notes add -F` "is not binary-safe". The same page warns that "it doesn't make much sense to
  display non-text-format notes with `git log`, so if you use such notes, you'll probably need to
  write some special-purpose tools".
- **Keyed on a commit, with one history for all notes.** A note hangs on one object, and every note
  in a ref lives in that ref's one history.
  - **Fetching:** a plain fetch of the ref brings every note ever kept there. A partial clone can
    take one note's bytes at a time: GitHub serves blobless clones (`--filter=blob:none`), which
    fetch "blobs on-demand"
    ([partial clone](https://github.blog/open-source/git/get-up-to-speed-with-partial-clone-and-shallow-clone/)).
  - **Removing:** dropping one note's bytes for good means rewriting that history and force-pushing
    it.
  - **Rebases:** a rebased or squashed commit loses its note unless `notes.rewriteRef` is set.
- **A clone skips notes**, like any ref outside branches and tags.
- **GitHub shows none:** "Displaying Git notes on GitHub is no longer supported" (an update of
  2014-08-14 on [Git Notes Display](https://github.blog/news-insights/git-notes-display)).

### Outside git

- **On the machine.** The owner's uploads are already a store addressed by content in the main
  checkout, served under a name that is the same on every branch (above).
- **Off the machine, under the same names.** Either of two copies could hold the store's files
  under their hashes, with no git involved. Neither puts anything in a clone.
  - **GitHub's release assets.** "Each file included in a release must be under 2 GiB. There is no
    limit on the total size of a release, nor bandwidth usage", and "Up to 1000 release assets may
    be associated with a single release"
    ([releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases)).
    - **Not git objects:** assets are files GitHub keeps beside a release, so a clone or a Cargo
      fetch never carries them, whatever tag the release sits on.
    - **A tag:** a release needs one ("Releases are based on Git tags"), and here only the nightly
      pushes tags.
    - **Uploads and downloads:** each file is an upload through GitHub's API, and a private
      repository's assets need a token to download.
    - **Quota:** the page does not say whether assets count in any quota.
  - **An object store.** Cloudflare R2 charges "$0.015 / GB-month", with "10 GB-month / month" free
    and egress "Free" ([R2 pricing](https://developers.cloudflare.com/r2/pricing/)). It needs a new
    account and its credentials, outside GitHub.
- **Several machines, one board.** The runs design says how a second machine would read what stays
  on the first: "Logs stay on their machine", and a log "opens on demand when its machine serves"
  ([reports](../../product/analysis/reports-and-monitoring.md#several-machines-one-board)). That board of several machines
  is itself not built.

### Tried on a scratch repository

A script in a scratch folder ran the trial with git 2.54.0, git-lfs 3.8.0 and Cargo 1.99.0. Nothing
touched this repository or its remote. The script:
- made a bare repository, pushed to it over `file://`, cloned it and fetched it with Cargo;
- gave it one crate with a random 200 KiB `docs/pics/evidence.png`, stored three ways:
  - tracked by LFS;
  - as a task's commit under `refs/artifacts/tools/example`;
  - and as a binary note under `refs/notes/artifacts`.

The B and Cargo results came out the same in all three runs. C and D came out the same in the two
runs after the first. The first run had stored the LFS pointer in their place, because
`git hash-object` ran the LFS filter until given `--no-filters`.

- **B.** The commit holds `version https://git-lfs.github.com/spec/v1`, the `oid sha256:` and
  `size 204800`, and the remote holds one LFS object. A plain clone, with git-lfs installed, gets
  the 204,800 bytes. A clone with `GIT_LFS_SKIP_SMUDGE=1` gets the 131-byte pointer.
- **Cargo and B.** A consumer named the crate as a git dependency, and `cargo fetch` checked out the
  131-byte pointer, the same with `CARGO_NET_GIT_FETCH_WITH_CLI=true`. The consumer builds, since
  nothing compiles the picture.
- **C and D.** The remote lists `refs/artifacts/tools/example` and `refs/notes/artifacts`.
  - A plain clone holds only `refs/heads/main` and its remote-tracking refs. Cargo's copy holds only
    `refs/remotes/origin/main`.
  - In the last run a bare clone took 12 KiB of objects. A mirror clone took 212 KiB: the picture
    once, as the ref and the note share one blob.
  - A fetch of the one ref by name brings that task's tree. A fetch of the notes ref brings the note,
    204,800 bytes.

## The options

### What a plain clone shows

The requirement is checked against each option. Does a plain `git clone` show the pictures the
documents reference, past decisions' included?

| Option | A plain `git clone` shows them | With one more command | So |
|---|---|---|---|
| A. The sweep's folder | no: git ignores it | no | excluded |
| B. Git LFS | yes, where git-lfs is installed (tried) and the account's LFS quota is intact or paid; past it with no payment method, pointers only | without git-lfs, `git lfs install` and `git lfs pull`; past the quota, nothing until the next month or a payment | passes, while the quota holds or is paid |
| C. A ref of their own | no: a clone skips the ref (tried) | `git fetch origin 'refs/artifacts/*:refs/artifacts/*'`, and then a tool to put the files where the links point | excluded |
| D. Git notes | no: a clone skips the notes ref (tried) | `git fetch origin 'refs/notes/*:refs/notes/*'`; notes are never files in the checkout | excluded |
| E. The store alone | no: outside git | no | excluded |
| F. The split | yes, committed beside the document | none | passes |

The owner's guess that the requirement excludes A and E is right, and it excludes C and D too. A ref
kept as a copy of E's store, which was considered, fails in the same way C does.

### The options side by side

Numbers are this repository's unless a row names the consuming project. Under every option, what
stays local is kept by one rule and one cap ([Retention](#retention)).

| | A. The sweep's folder | B. Git LFS | C. A ref of their own | D. Git notes | E. The store alone | F. The split |
|---|---|---|---|---|---|---|
| A plain clone shows the documents' pictures | no | yes, with git-lfs and the quota intact or paid | no | no | no | yes |
| Committed to main | nothing | a pointer per file, for good | nothing | nothing | nothing | the pictures documents show, for good |
| A clone of main adds | 0 | the pointers, and then every file in its tree: 249 MiB here, up to 7.35 GiB for the consuming project, if all evidence went in | 0 (tried) | 0 | 0 | the documents' pictures: 5.2 MiB in the history here now, about 2.5 MiB a day; 2.47 GiB for the consuming project |
| A consumer's Cargo fetch adds | 0 | the pointers, never the files (tried) | 0 (tried) | 0 | 0 | the documents' pictures, in its copy of the history and in each pinned checkout |
| The host's quota and cost | none | 10 GiB stored and 10 GiB a month downloaded free, then $0.07 a GiB a month and $0.0875 a GiB | unknown: GitHub documents only the repository's size (under 5 GB strongly recommended), and whether these refs count in it is unknown | as C | none | the repository's size: 129 MiB on GitHub now |
| Removing what reached GitHub | never, short of deleting the repository or asking support | as B | deleting the ref, then unknown | rewriting the notes history | nothing reaches it | never: it would rewrite main |
| Each worktree's disk | its own files | a copy of every file in its tree, unless it skips them | none | none | none | a copy of the documents' pictures: 4.3 MiB here today, about 0.9 GiB after a year at that pace; 2.47 GiB for the consuming project |
| A dead disk here loses | all of it | what no push sent | what no push sent | what no push sent | all of it: about 250 MiB here, growing by up to 125 MiB a day, and the consuming project's 7.35 GiB | the working evidence only |
| A link names | a path in a checkout | the committed path | the file's SHA-256 | the file's SHA-256, if the note holds a list | the file's SHA-256 | in a document, the committed path; on a thread, the file's SHA-256 |
| Needs on every machine | nothing | git-lfs | nothing new | nothing new | nothing new | nothing new |

### A. The sweep's folder, alone

This is the sweep as it lands. A finished worktree's artifacts go to the main checkout's
`artifacts/swept/NAME/`, under the cap, with a check by hand.

- **Nothing reaches a clone.** That fails the requirement.
- **A link** names a path in a checkout. It breaks when the sweep moves a worktree's folder aside,
  and it never works on another machine.
- **Costs:** nothing new.

### B. Git LFS

The pictures leave `.gitignore`, `git lfs track` takes them, and they are committed on their
task's branch.

- **A clone shows them where git-lfs is installed and the quota holds.** Without git-lfs, a clone
  has pointers until `git lfs pull`. Past the account's LFS quota with no payment method, a clone
  gets "only ... the pointer files" until the next month (the billing page,
  [Git LFS](#git-lfs)). The consuming project's 2.47 GiB of documents' pictures use the free 10 GiB
  a month in about four fresh clones. A consumer's Cargo fetch always has pointers (tried).
- **A pointer per file reaches main for good.** The owner asked not to commit artifacts directly,
  and this commits pointers to them.
- **Nothing pushed comes back off GitHub**
  ([Removing what reaches GitHub](#removing-what-reaches-github)).
  - **For all evidence:** at this repository's 125 MiB a day, the free 10 GiB is full in about 80
    days, and from then on it costs about $0.70 a month for each further 10 GiB.
  - **For the documents' pictures alone:** at about 2.5 MiB a day, it fills in about ten years.
- **Each worktree** checks out every file in its tree, unless it skips them with
  `GIT_LFS_SKIP_SMUDGE=1`. If it skips them, it holds pointers, which the server would have to
  read through the LFS cache.
- **The server and the docs audit** must tell a pointer from a picture. Otherwise the server serves
  131 bytes of text as a PNG, and the audit counts a pointer as a picture on disk.

### C. A ref of their own

Each task's artifacts are a commit with no parent under `refs/artifacts/SLUG`. The commit is pushed
to the remote and fetched by name when wanted. There is one ref per task, so deleting a ref drops
only that task's files.

- **A clone never shows them.** A clone skips the ref (tried), which fails the requirement. Even
  after a fetch by name, the files are not where a document's link points.
- **It cannot be an orphan branch.** A branch with no common history is still a branch: every clone
  takes every branch, and Cargo's fetch of a revision it cannot resolve takes them all.
- **GitHub's handling is unknown.** It is unknown whether GitHub keeps such a ref, counts it in the
  repository's size and frees it when the ref is deleted. Under C, the first task would check all
  three on a scratch repository on GitHub.
- **Costs:** a push before a picture shows anywhere but here, and git in the server's path.

### D. Git notes

A note on the task's commit holds the file, or a list of the files' hashes with the bytes kept as in
C or E.

- **A clone never shows them.** It skips the notes ref, and notes are never files in a checkout.
- **Holding the files, D is C with every task in one history.** A plain fetch takes every picture,
  and removing one means rewriting that history and force-pushing it.
- **A note follows a commit,** but evidence belongs to a task, whose commits are rebased, squashed
  or never landed.
- **GitHub shows no notes.**

### E. A store addressed by content, alone

This is the owner's uploads, opened to agents. One command copies a file into the main checkout's
`artifacts/uploads/<sha256>.<ext>` (the project's artifacts folder, through `Project::artifact`)
and prints the line to post.

- **A link never moves.** `/artifacts/uploads/<sha256>.png` is answered from the main checkout on
  every page, on every branch's view (`/@tree/…`) and on the phone, before any tree is read. A
  worktree swept away takes nothing with it.
- **Removing gives the space back** at once.
- **The same picture taken twice is stored once.** The 1,453 pictures here are 1,384 contents.
- **Nothing reaches git, so a clone shows nothing.** That fails the requirement.
- **One disk, no backup.** If this disk dies, every attached file is lost: about 250 MiB here today,
  growing by up to about 125 MiB a day, and the consuming project's 7.35 GiB.
- **A reviewer elsewhere and a second machine see none of it.**

### F. The split (recommended)

![Proposed: what a document shows is committed; working evidence stays in the local store](attached-artifacts/split.svg)

What a document shows is committed, and the working evidence stays local, in E's store.

**The committed half.** A picture that a document or a decision card shows is committed beside it,
as card pictures are now.
- **A clone, GitHub and every past decision show it,** with no extra command.
- **On a card,** an agent posts a committed file, as today.
- **The owner's uploads on a register line** are committed with the verdict, at the very path the
  line names (`artifacts/uploads/<sha256>.<ext>`, added past the ignore). The owner's words stay as
  written, and the link resolves the same way in a clone and on GitHub as in the app.
  - **Only under the default folder name.** The line's route is `/artifacts/uploads/` whatever
    `[project] artifacts` names (`uploads.rs`), and the file is stored under that folder. A project
    whose artifacts folder has another name stores the file at `NAME/uploads/…`, where GitHub's
    `/artifacts/uploads/…` link finds nothing. There the verdict commits the file at the path the
    link names, `artifacts/uploads/…` from the repository root, so the owner's words still resolve.
    Task 2 (`tools/doc-pictures-committed`) writes and tests that case.
- **A picture that starts as evidence** is committed beside the document when a document comes to
  show it. Nothing else is committed.

**The local half.** Evidence that no document shows is attached to E's store in the main checkout,
under the sweep's cap. It is kept while something actively references it and removed by an agent's
check once nothing does ([Retention](#retention)). Thread posts name it by content, so a link holds
while the file is kept.

**What each half costs, at today's growth:**
- **The committed half, here.** 5.2 MiB of pictures in the history now (4.3 MiB on main), growing
  by about 2.5 MiB a day while the register is this busy: about 0.9 GiB a year at that pace. The
  rate is inferred from two days, and the first of them was the documents' first burst of pictures
  (41 files), so a year may hold less.
  - **Every checkout holds all of them.** Each worktree and each consumer's pinned Cargo checkout
    has the documents' pictures as the tree holds them at that commit: 4.3 MiB each today, about
    0.9 GiB each after a year at that pace. Across this repository's 40 worktrees that is about
    36 GiB, and across the 20 pinned checkouts the consuming project's Cargo keeps, about 18 GiB.
  - Every clone and every consumer's copy of the history carries the history's 5.2 MiB, growing at
    the same pace.
  - GitHub holds the whole repository at 129 MiB.
  - None of it is ever removed.
- **The committed half, for the consuming project.** Its documents show 1,310 pictures on disk, 2.47
  GiB, all distinct, mostly from September; 30 more are already lost. Committing them would:
  - take its repository on GitHub from 122 MiB to about 2.6 GiB, past the 1 GB GitHub calls ideal
    and under the 5 GB it strongly recommends;
  - put 2.47 GiB into every clone;
  - and put 2.47 GiB into every worktree's checkout: about 0.94 TiB across its 388 worktrees today.

  Under LFS instead, those pictures use 2.47 GiB of the free 10 GiB, its worktrees can skip them,
  and each fresh clone downloads 2.47 GiB of the free 10 GiB a month: about four fresh clones a
  month, after which a clone gets pointers unless the account pays. That per-worktree cost is what
  decides plain commits against LFS for the consuming project. Which to choose, and whether to
  shrink the PNGs first, is the consuming project's call.
- **The local half.** It adds 0 bytes to git. It is about 125 MiB a day of pictures here, held under
  5 GB on this one disk, with no copy elsewhere. A dead disk loses the working evidence, and no
  clone or other machine sees it.

**Why the committed half is plain commits and not LFS here.** That half is small here: 4.3 MiB on
main, at about 2.5 MiB a day. Plain commits need no tool on any machine, no quota, and no change to how the
server or the docs audit read a file. They also show in a clone with no condition. LFS would keep
the pictures out of consumers' Cargo copies, at the cost of git-lfs everywhere, a pointer reader in
the server and the audit, and storage on GitHub that is never freed. LFS is the better fit only
where a project's documents show a great deal, as the consuming project's do.

## Removing what reaches GitHub

If B, can pushed objects be overwritten to delete them? What can and cannot be removed once it is
pushed:

- **An LFS object on GitHub cannot be deleted by a push.**
  - **Only two ways out are documented:** "To remove Git LFS objects from a repository, delete and
    recreate the repository", and "If you need to purge a removed object and you are unable to
    delete the repository, please contact support for help"
    ([removing files](https://docs.github.com/en/repositories/working-with-files/managing-large-files/removing-files-from-git-large-file-storage)).
  - **Taking a file out of LFS frees nothing:** "After you remove files from Git LFS, the Git LFS
    objects still exist on the remote storage" (same page).
  - **Storage counts every object ever pushed:** it "is calculated based on all Git LFS objects
    associated with a repository, regardless of when they were uploaded"
    ([storage and bandwidth](https://docs.github.com/en/repositories/working-with-files/managing-large-files/about-storage-and-bandwidth-usage)).
  - **There is no push that overrides an object.** An object is named by the SHA-256 of its bytes,
    so a new version is a new object beside the old one. The LFS protocol's batch request takes an
    `operation` that "Should be `download` or `upload`"; it has no delete
    ([batch API](https://github.com/git-lfs/git-lfs/blob/main/docs/api/batch.md)).
  - **Locally, `git lfs prune` deletes only this machine's copies.**
  - **Not verified,** beyond that documentation: whether GitHub support purges objects on request,
    and how long that takes.
- **A plain commit is removed only by rewriting history and force-pushing main.**
  - **GitHub's documentation:** "Rewriting history requires careful coordination with collaborators
    to successfully execute, and has a number of side effects that must be managed". Each clone must
    "either discard their clone and re-clone". Cached views and pull requests keep the data until
    support removes them and runs "a garbage collection on the server"
    ([removing sensitive data](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/removing-sensitive-data-from-a-repository)).
  - **Here:** this repository's land "never forces" (AGENTS.md), and every consumer pins main's
    commits, so a rewrite would break their pins.
- **A ref of its own (C) is removed by deleting or replacing that one ref.** Main is never rewritten.
  When GitHub then frees the objects is **unknown**: its documentation says nothing of it, and
  nothing was pushed to find out.

So whatever reaches GitHub through main, as plain files or through LFS, stays there for good. That
is why F commits only what a document shows, which is meant to last, past decisions' included.

## Naming an artifact

- **In a document or on a card:** the committed path, relative to the document, as today
  (`analysis/x.png` from the register). A clone, GitHub and the app all resolve it.
- **On a thread:** the line the attach command prints,
  `![board on a phone](/artifacts/uploads/<sha256>.png)`, or `[report](/artifacts/uploads/<sha256>.pdf)`
  for any other kind.
  - It is the line the owner's uploads already post, and the app already draws it in an agent's
    thread post, which is drawn from its Markdown (`crates/symphony/src/uploads.rs`, `reference`;
    `apps/symphony-web/src/task-page.tsx`, `TaskMessage`).
  - A name by content never depends on where the bytes are kept.
- **The 14 posts that name a worktree's path stay dead text.**
  - Those paths are plain text in the posts, not links, so nothing the server serves could answer
    them, and nothing here rewrites them.
  - All 14 are on tasks now done, whose posts keep nothing under the rule. So no migration task is
    listed.
  - Of their 8 worktrees, 6 are gone; 2 of those 6 have a folder of the same name in the main
    checkout.

  A path in an open task's post still keeps the file it names, through the attach record and the
  sweep's record ([below](#the-reference-index)).

## Retention

This is the local half's: what is committed stays.

### The rule

The sweep's rule, which lands first: an artifact something still actively references is kept
until the space used becomes a problem that requires removing it. Under every option it reads:

- **What keeps a file.** "Actively references" means one of these:
  - a task not done or cancelled, for what it attached, what its own worktree set aside, and what
    its brief and its thread name;
  - a post on the thread of a task not done or cancelled;
  - a card not resolved, dismissed or rejected, wherever the branch scan reads it;
  - a document on the default branch, its history folders included, as the sweep already counts
    with one `git grep` of all of `docs/`; and a document on the branch of a task not done or
    cancelled, landed or not;
  - an open ask, until it is deleted a day after its last message (`asks.rs`).
- **What does not keep a file.** A post on a closed task does not keep a file by itself. The owner's
  uploads follow the same rule: one named only in an ask is kept while the ask is open, and after
  that while anything else names it. The check marks each upload as the owner's, so the agent
  looking knows whose it is.
- **Read today by that rule.** Open tasks' posts name 35 of this repository's 1,453 pictures
  (4.0 MiB, one task in review) and one upload. Everything else stays only while a task not done or
  cancelled holds it, or a document names it. Closed tasks' evidence is what becomes a candidate.

Under F, a document names committed pictures, so the document rule keeps local files only for links
made before F, such as the consuming project's, until they are committed.

### One cap

There is one cap: the sweep's `[board] swept_cap_gb` in the project's `symphony.toml`, 5 GB when
unwritten. It is counted in powers of ten, as `kept.rs` counts it, so about 4.66 GiB.

- **What it counts:** the folder the main checkout keeps. With attachments, that is the sweep's
  `swept/` and the store `uploads/` together, measured by the sweep's own measure, with committed
  files left out.
- **What it leaves out:** the main checkout's own task folders, as the sweep does. A file there
  comes under the cap only when it is attached. Until then their only bound is the board's
  low-disk warning, on every page while free disk is under `SYMPHONY_LOW_DISK_GB` gigabytes, 20 when
  unset (`crates/tasks/src/board_health/disk.rs`). They hold 234 MiB here, and 17.2 GiB in the
  consuming project's main checkout, 5.46 GiB of it pictures. Follow-up, not filed: bring them under
  the cap, or count them in its measure.
- **It is a project's setting.** Each project writes its own cap.

### One kept folder

No picture is stored twice. Once attachments exist, the sweep sets aside a finished worktree's
artifacts but leaves out each file whose bytes the store already holds. That file was attached, and
its name in the store is the one any post uses, so its copy goes with the worktree and no bytes are
lost. The sweep moves the rest into `swept/NAME/`, as now.
- **The store** holds what was attached, by the owner or by an agent.
- **`swept/`** holds what never was.
- **Attach costs no space either.** It copies with a copy-on-write clone where the disk allows it
  (APFS, as `tools/seed-target.sh` clones build folders).

### The check

The check runs when the sweep's measure finds the kept folder past the cap. Nothing runs on a timer
and nothing is removed automatically.

- **It lists the candidates**, which nothing actively references:
  - closed tasks' files first, then the rest, largest first;
  - each with its size, when it was attached or set aside, and the task it came from;
  - the owner's uploads marked as such;
  - and what removing them would free.
- **An agent looks and removes** with one command, which refuses any file the index finds
  referenced at that moment, and any file git tracks.
- **If what is referenced alone passes the cap,** the check says so and names the largest referenced
  files and what names them. What to drop is then the owner's question, on a card. Under this rule
  only open work can do that.
- **Scratch data is not evidence.** The sweep's rule for leftovers applies to it (build output is not
  kept), and nothing here attaches it.

### The consuming project on day one

Its 7.35 GiB of pictures sit in two places, and each comes under the design differently.

- **The 2.47 GiB its documents show** leave the local half under F once they are committed, by plain
  commits or LFS. That is its call.
- **The rest of the 5.46 GiB in its main checkout** stays in its own task folders, outside the cap,
  as under the sweep. Only the low-disk warning bounds them ([One cap](#one-cap)).
- **The 1.88 GiB in its 388 worktrees** goes with the rest of each finished worktree's artifacts.
  - Of the worktrees' 219 GiB, the 20.6 GiB in folders tagged as caches is build output, which the
    sweep drops.
  - The other 198.4 GiB at most, mostly coverage profiles and scratch data, is what its sweep sets
    aside under the sweep's rule.
  - The sweep drops compiled files too; they were not told apart here.

So its first check after that finds the kept folder far past any cap, and it lists the largest
candidates first: closed tasks' leftovers, which nothing actively references, such as one finished
worktree's 40 GiB of coverage profiles. Removing the largest few brings it under the cap. It is not
over the cap for good, asking each time: its documents' pictures are committed and leave the cap,
so what stays referenced is its open tasks' files.

`swept_cap_gb = 10` is proposed for the consuming project: room for its open tasks' evidence and
about a month of new pictures at October's pace (1.7 GiB that month, in its worktrees). The figure
is its own call.

## The reference index

### What counts as a reference

| Referrer | Keeps a file while | What names it |
|---|---|---|
| a task | it is not done or cancelled | what it attached (the attach record), what its worktree set aside (the sweep's record), its brief and its thread |
| a thread post | its task is not done or cancelled | its text |
| a card | it is not resolved, dismissed or rejected, on any branch the branch scan reads | its lines |
| a document | it is on the default branch (every file `Project::walk` lists and each entrypoint's history folders, the registers included), or on the branch of a task not done or cancelled | its text |
| an open ask | until it is deleted, a day after its last message | its messages |

**A name** is one of three things:
- **A stored name:** `/artifacts/uploads/` followed by 64 hex digits. It is exact, and so cheap to
  find.
- **An older path into the artifacts folder,** of the main checkout, a worktree or a `/@tree/` view.
  Two records resolve it: the attach record, which keeps the path each file was attached from, and
  the sweep's record, which keeps the worktree each `swept/NAME/` came from.
- **A folder:** a folder a post names (`artifacts/app/header-names/`) references every file under
  it, as the sweep's lookup does today.

### How it is found cheaply

- **The index is one file per project:** `references.json`, in the board's folder of the git common
  dir, beside the sweep's `swept.json` and the attach record. It maps each name to its referrers: a
  document's path and branch, a card's slug and branch, a task's slug and message id.
  - **It is rebuilt from the sources whenever it is lost,** so it is not state to keep safe.
  - **The references themselves are what the board already keeps:** its briefs and posts.
  - **It is not a board table.** A table of them on the board would be a schema raise, which refuses
    every older writer of the board until it is rebuilt
    ([plugin delivery](../../engineering/analysis/plugin-delivery.md#the-board)), all for data that can be derived.
- **It is updated by difference:**
  - **Documents:** `git diff --name-only` of the default branch, between the commit the index last
    read and the tip, reading only the Markdown files that changed.
  - **Open tasks' branches:** the worktrees the server's branch scan already lists
    (`crates/symphony/src/branches.rs`), with the documents each branch changed against main, read at
    its tip.
  - **The board:** posts whose id is above the last one read (ids only grow; this thread's first
    post is 2039), and briefs whose task changed since.
  - **A task's close** drops its thread and its own files from the referenced set.
- **A full rebuild is cheap, measured.** A regular expression over the consuming project's 143
  documents (22 MiB) took 0.07 to 0.12 s in three runs, at load 49 on 16 cores. Over this board's
  2,041 posts, once read (2.4 MiB), it took 0.03 s.
- **It is read only when needed:** by the cap's check, by the removal command, and by a listing of
  the store (`symphony-cli artifacts`: each file's size, when it was attached and by which task, and
  what names it). It is never read on a page view.
- **It replaces the sweep's own lookup:** `Measured::look_up` in `kept.rs`, which does one read of
  the open tasks' threads and one `git grep` of `docs/` on main. The sweep and the store then keep
  files by one lookup.

## Why F

F is the one option that meets the requirement without committing all evidence.

- **It meets the requirement unconditionally.** What a document or a card shows is in every clone,
  on GitHub and in every past decision, with no tool to install.
- **It commits only what is meant to last.** Nothing that reaches GitHub through main comes back
  off, so only that goes there. Committing everything would cost about 125 MiB a day of evidence,
  for good.
- **What the owner asked for already holds for the rest.** The working evidence stays where the
  sweep and its rule already put it: out of git, kept while actively referenced, and
  removed by an agent's check past the cap.
- **Its local half fixes the broken links.** It is E's store, so a thread's picture keeps its link
  for as long as the file is kept.
- **It commits the documents' pictures directly,** which the ask first hoped to avoid, attaching
  them by another process instead. The reason: nothing that reaches GitHub comes back off, by LFS
  or by a plain commit, so LFS buys no way back, and a plain commit needs no tool and no quota.
- **Its cost is plain:** the documents' pictures in every clone and every checkout, worktrees and
  consumers' pinned checkouts included, for good, and working evidence on one disk with no backup.

The other options:
- **B** passes the requirement too, but only while the account's LFS quota holds or is paid, and
  at the cost of git-lfs everywhere and a pointer reader.
  It fits the committed half only where a project's documents show a great deal.
- **A, C, D and E** fail it.

## The implementation tasks

The sweep that sets aside a finished worktree's artifacts has landed, with its rule, its cap
(`[board] swept_cap_gb`) and its record (`crates/tasks/src/sweep/kept.rs`). F is chosen, and the
four tasks below are filed on the board, ready, each landing
before the next reads it. Tasks 1 and 2 are first: they make a clone show the documents' pictures
and keep a thread's evidence reachable.

1. **Attach** (`tools/attach-evidence`). `just symphony-cli attach FILE… [--task SLUG]` copies each file into the main
   checkout's store under the uploads' rules, using a copy-on-write clone where it can.
   - **What it prints and records:** each file's Markdown line, and the attach record: hash, task,
     the path it came from, and when.
   - **From a linked worktree,** it finds the main checkout through the git common dir, as
     `crates/symphony/src/builds.rs` does.
   - **Shots:** `symphony-cli shot --attach` does the same for a page shot.
   - **The guidance:** AGENTS.md, the shipped skills and CHECKS say to attach evidence posted on a
     thread and to commit any picture a document or a card shows.
2. **What the documents show is committed** (`tools/doc-pictures-committed`, built:
   `crates/symphony/src/docs_audit.rs`, `crates/symphony/src/commit.rs`, and REVIEW's
   [Writing a card](../REVIEW.md#writing-a-card) and [Attaching files](../REVIEW.md#attaching-files)).
   The docs audit gets three checks, and an owner's verdict commits its uploads. A land runs no
   audit now (`[land] check = false`). The regenerator is to run it on main's tip, and a verdict
   save runs it on its own branch. So each check reads the tree and the links, never the store,
   and only the lines a branch wrote can refuse:
   - It refuses an agent's card line whose picture is not a file git tracks. As today's picture
     check does, it judges only the lines the branch wrote (`docs_audit.rs`, judged on
     `Fork::written`), so an older line of main's never refuses anything. With no fork, on main
     itself, it warns.
   - It warns, "not in a clone", of an owner's line naming an upload git does not track.
   - It warns of any other document's picture that points into the ignored artifacts folder.
   - As built, a document's own pictures (outside its cards and the owner's callouts) are judged
     as a card's are: refused on a branch's own lines, warned of on main, on disk or not. The
     picture reader takes every form: a title, a reference to a definition, raw `<img src>`, and
     this server's own URL for a file of the project. On main as it stood when this was built,
     nothing was refused or warned of.
   - As built, the verdict commits an upload only when the store holds a regular file there of at
     most the cap, whose SHA-256 is its name. Anything else is left out with a note, and the
     verdict is saved.
   - The server commits an owner's verdict itself (`crates/symphony/src/commit.rs`). That commit
     adds each upload the owner's new line names, at the path its link names from the repository
     root, whatever the artifacts folder is called.
3. **The reference index** (`tools/artifact-reference-index`). The file, its updates by difference,
   its rebuild, and `symphony-cli artifacts` listing the store with what names each file. It
   replaces the sweep's `Measured::look_up`, so `swept/` and the store are kept by one lookup.
4. **One kept folder and its check** (`tools/kept-folder-one-check`).
   - **The sweep** leaves out a worktree's copy of bytes the store holds.
   - **The measure** counts `swept/` and the store together against `[board] swept_cap_gb`.
   - **The check** lists the candidates, closed tasks' first and largest first, with the owner's
     uploads marked.
   - **The removal command** refuses a referenced or tracked file.

   Nothing runs on a timer.
5. **A note for the consuming project's board.** It is not filed from here, and nothing in the
   consuming project is changed from here. It is for its own board, to decide its own calls:

   > Symphony has chosen F: a picture a document or a decision card shows is
   > committed beside it, and working evidence stays in the main checkout's store under the sweep's
   > cap. Your documents show 1,340 pictures from your ignored artifacts folder: 1,310 on disk
   > (2.47 GiB, mostly from September) and 30 already lost. Your calls:
   > - **Plain commits or LFS.** Plain commits put 2.47 GiB in every clone and in each of your 388
   >   worktrees, about 0.94 TiB. LFS uses 2.47 GiB of GitHub's free 10 GiB of storage, lets
   >   worktrees skip the files (`GIT_LFS_SKIP_SMUDGE=1`), and needs git-lfs on every machine. Past
   >   the free 10 GiB of bandwidth a month, about four fresh clones, a clone gets pointer files
   >   unless the account pays.
   > - **Shrinking the PNGs first,** before anything is committed for good.
   > - **Your cap:** `[board] swept_cap_gb` in your `symphony.toml`. 10 is proposed: room for open
   >   tasks' evidence and about a month of new pictures at October's pace.

Not filed: a copy of the local half off this disk (an object store or release assets), and a second
machine's server asking this one for a picture. Each is filed when it is wanted.

## Open questions

1. **Size.** Attachments take the uploads' limit of 10 MiB a file, and one of the consuming
   project's 28,001 pictures is larger (12.5 MiB). Should agents get a higher limit, or keep this
   one? A larger file stays in `swept/`.
2. **The committed half under LFS here as well.** It would keep the documents' pictures out of
   consumers' Cargo copies (4.9 MiB of `docs/` per pinned checkout today). The cost is git-lfs on
   every machine, a pointer reader in the server and the audit, and GitHub storage that is never
   freed. Worth it only if the documents' pictures grow far past their pace of these two days.

## Verified and inferred

- **Read in code:**
  - each fact that names a file, in Symphony at `6029a28f`;
  - the sweep's `aside.rs` and `kept.rs`, and its paragraph in TASKS, on its branch at `2a934225`;
  - Cargo's `fetch` at its master, 2026-11-12.
- **Measured:**
  - sizes, counts and file times of every artifacts folder of this repository's 41 checkouts and of
    the consuming project's main checkout and 388 worktrees, with the folders among them tagged as
    caches (`du`, `find`, `stat`, `shasum`);
  - the 2,041 posts of this board's 307 tasks, and each task's stage (`just tasks list --all --json`
    and `show --json`);
  - The consuming project's document pictures (`grep` of its Markdown, then `stat` and `shasum`);
  - this repository's history (`git count-objects`, `git log --raw`, `git cat-file --batch-check`);
  - The consuming project's Cargo copies of Symphony (`du`, their `packed-refs`);
  - both repositories' size on GitHub (`gh repo view`, read only);
  - Time Machine's destinations (`tmutil`);
  - the scratch trials;
  - the full scan's time.
- **Inferred:**
  - growth per day from file times and two days of commits;
  - days to fill a cap or a quota, from those rates;
  - what every checkout holds after a year, and across today's worktrees and pinned checkouts:
    each checkout's share at that pace, times today's counts;
  - LFS's cost, from GitHub's published prices;
  - The consuming project's set-aside size, from its folders tagged as caches, without telling
    compiled files apart.
- **From outside:** each claim beside its URL, read 2026-11-12.
- **Unknown:** whether GitHub accepts and keeps a ref outside heads and tags, counts its objects in
  the repository's size, and frees them when the ref is deleted. Nothing here depends on it.
- **Not verified:**
  - whether another tool than Time Machine copies the artifacts folder off this disk;
  - whether GitHub support purges LFS objects on request, and how fast;
  - whether git-lfs copies or clones files at checkout on APFS, beyond what `git lfs dedup`'s help
    implies.
- **Not tried:** any option beyond the scratch trials. Nothing here was built, and nothing was
  pushed anywhere.
