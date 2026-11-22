# The built client out of git

What it would take for git to hold no built web client, the client travelling only inside a
package on crates.io, and when the commits could stop. The choice is the owner's, on
[its card](#client-in-the-crate). Read at main `4397600b` on 2026-11-15; nothing
here was built or tried, and [what is not known](#not-yet-known) says what a trial would have to
show.

## Today

- **In git.** The built client is `apps/symphony-server/client/public`, 108 files and 5.4 MB in
  the tree. Main's history holds 153.1 MiB, 132.2 MiB of it that folder's (the walk in
  [the code review](code-review-2026-11.md#2-the-built-client-is-committed-and-is-85--of-the-repository)),
  and 106 of main's commits regenerate it. The regenerator a land starts makes each
  ([TASKS](../../workflow/TASKS.md#the-regenerator)).
- **In the server.** The server's build script embeds that folder, so any build of the server has
  its pages with no build of the client: a worktree's tests, the release run, and a consuming
  project that builds the server from a release's Git tag.
- **In the package.** The server's package for crates.io already carries the folder, every build
  it keeps, 7.6 to 8.0 MiB compressed of the 10 MiB crates.io takes
  ([RELEASES](../../workflow/RELEASES.md#packages-on-cratesio)). So a server installed from crates.io needs
  no client from git.
- **Not published.** Nothing is on crates.io yet. The first publication waits on the owner's word
  (`[releases] publish`) and on whether the packages name a private repository. The consuming
  project takes every Symphony package from Git at a release's tag, and can move to crates.io
  only for all of them in one edit, since a kit from crates.io beside a library from Git is two
  copies of the kit ([the contract's analysis](consumer-contract.md#two-references-two-copies)).

So git can stop holding the client for whoever takes the server from crates.io, and not for
whoever builds it from Git: that build would find an empty folder.

## The options

### A, when nothing builds the server from Git

The commits go on as today until three things are done, then stop for good.

1. The first publication, and `[releases] publish` on, so each release publishes.
2. The consuming project takes Symphony from crates.io by version, in its own one edit, and the
   refresh moves versions in its manifest where it moves a tag today. That part of the refresh is
   not built.
3. The folder leaves git and is ignored; the server's build stops with one line naming
   `just client-update` where the folder is empty.

After that:

- **A project on a Git pin** gets a server that does not build, so none is left on one. A pin to
  a commit of main is no longer a way to take the server.
- **A checkout that needs pages**, an agent's server for pictures or a worktree whose tests read
  the embedded client, builds the client first: minutes while the Dioxus client remains, one Vite
  build after it. How many test targets need it is not counted.
- **The release run** builds the client in its own checkout, verifies with it, tags a commit that
  holds no client, then publishes the server's package with that client in it, before the
  refresh. Publishing stops being optional: a release whose upload failed has no client anywhere,
  so the refresh waits for it where today the release stands without it.
- **`just client-check`** has no committed client to compare a fresh build with. What is left to
  check is that the same sources give the same bytes twice, which the crates check then relies
  on: it compares the package made here with the published one, client included.
- **The regenerator goes**, 1,674 lines and 1,962 of tests, with the rule that a branch commits
  no client, the land keeping main's side of it and a pin's wait for it. With it goes main's
  only build of the client and audit of the documents between releases: a land that breaks
  either is found by the next task that builds the client, or by the release run.
- **The two older builds** a page may still be open on come from the previous release's package.

### B, a package of its own, now

The built client becomes a package with nothing else in it, which the server requires at an exact
version. Cargo takes it from crates.io even for a server built from a Git tag, and it shares no
type with the kit, so the consuming project stays on its Git pin and still gets its pages. Git
holds no client as soon as it works.

- **The release run** publishes the client's package before it tags, since the tag's manifest and
  lock name a version that must exist: an upload that cannot be taken back, made before the
  release is cut, and a commit of the release's own for the version and the lock.
- **The first publication** is this package's, at the next release, and needs the owner's word
  and the answer on the repository line now.
- **Main between releases** builds the server with the last published client, older than its
  sources, unless a checkout points the requirement at a client it built. How a worktree's tests
  get their own client is not designed.
- **The regenerator and `just client-check`** go as under A.

### C, A, with one commit a release until then

As A, and for the wait the release run starts the regenerator's round where a land starts it
today. Main gains one client a release, about three tenths of today's growth at one release in
twenty landings. It is a task. Between releases main's client is older than its sources, so a pin
to a commit of main waits for the next release, and a land's break of the client's build or of
the documents is found as under A.

None of them shrinks the history already there: that takes rewriting it, a choice of its own.

- [r] **client-in-the-crate** — Replaces: client-distribution-on-main. Git would stop holding the built web client: a release builds it, and it travels only inside a package on crates.io. Nothing on the pages changes. That works for a server taken from crates.io, and not for one built from a Git tag, as the consuming project builds it today: that build would find no client. So when do the commits stop? A: once nothing builds the server from Git: after the first publication and after the consuming project takes Symphony from crates.io; until then the client is committed as today. B: now, by giving the built client a package of its own, which even a server built from Git takes from crates.io; it needs a trial first. C: A, and until then the client is committed once a release instead of after every land. Recommended: A. Approve and say A, B or C in the note, or comment. [[analysis/client-off-git#The options]]
  [2026-11-15 11:12] agent — Posted for the board task `docs/owner-answers-applied`, from your rejection of **client-distribution-on-main**: "we should push those releases as crates to crates.io and not commit then at all if possible."

    **What is possible today.** The server's package for crates.io already carries the built client, so a server installed from crates.io needs none from git. Nothing is published yet: the first publication waits on your word and on the repository line, asked on the board task `tools/publish-to-crates-io`. The consuming project builds the server from a release's Git tag, and can move to crates.io only for every Symphony package in one edit.

    **What each costs.**
    - **A:** nothing is built for the wait. The client is committed after each land that changes it, about 27 MiB a day, until three things are done: the first publication, with `[releases] publish` on; the consuming project's edit from Git tags to versions, with the refresh moving a version where it moves a tag today (not built); then the folder leaves git. From then a project on a Git pin gets a server that does not build. A checkout that needs pages builds the client first, minutes while the Dioxus client remains. The release run builds the client, tags a commit with none in it and publishes the server's package before the refresh, which then waits for that upload. `just client-check` has no committed client to compare with, and checks only that two builds agree. The regenerator goes (1,674 lines, 1,962 of tests), and with it main's only build of the client and audit of the documents between releases.
    - **B:** the commits stop as soon as it works, and the consuming project stays on its Git pin. The release run publishes the client's package before it tags, an upload that cannot be taken back, so the first publication is at the next release and needs your word and the repository answer now. Main between releases builds with the last published client, older than its sources, unless a checkout uses one it built; how a worktree's tests get theirs is not designed. None of it has been tried.
    - **C:** A's end, plus a task now: the release run starts the commit a land starts today, so main gains one client a release, about three tenths of today's growth. Between releases main's client is older than its sources, a pin to a commit of main waits for the next release, and no land is followed by a build of the client or an audit of the documents.

    None of them shrinks the 132 MiB of client already in main's history: that takes rewriting it, a choice of its own.

    **Not yet known.** How long A waits: the time to your two answers on publication and to the consuming project's edit. Whether cargo packages a folder git ignores when the package's own list names it, which A and B both need; not tried. What a worktree pays to build the client before its tests; not measured. Whether B works at all.

    **Why A.** It is what you asked for, and every step of it is one the move to crates.io needs anyway, so nothing is built to be thrown away. B could stop the commits sooner, but it rests on things nobody has tried and puts an upload that cannot be undone before every release is cut. C is worth its task only if the wait runs to weeks.

    **Earlier verdicts checked.** **consumer-contract-versioning** (B) and your words under it, "I would like to publish crates (MIT license) to crates.io as the pacakge manager", put the packages on crates.io; they do not say whether git also holds the client. **release-delivery** (C) ships a release's client beside the server once `symphony update` installs releases; it does not say what git holds before then. **symphony-update-command** (B) has projects follow releases, which every option keeps. **pin-waits-for-current-client** (A) keeps a pin to a commit of main off a client older than its sources: A and B leave such a pin no client and C makes it wait for a release, so none changes that choice. None of them answers this card.
  [2026-11-15 11:38] owner · approved — A
  [2026-11-18 18:12] agent — A stands: the built client is committed as today. The first publication on crates.io is made (`inbox/submit-crates`). The commits stop once the consuming project takes Symphony from crates.io, which it has not yet; no task is filed for the stop until then.
  [2026-11-19 01:15] agent · resolved

## Not yet known

- **How long A waits.** It is the time to the first publication and to the consuming project's
  edit. At today's pace each day of it adds about 27 MiB to main's history.
- **Whether cargo packages a folder git ignores** when the package's `include` names it, without
  `--allow-dirty`. A and B both need it; not tried.
- **What a worktree pays** to build the client before its tests, and for how many of them. Not
  measured.
- **Whether B works at all**: a server from Git with its client from crates.io, the release's
  own commit for the version, and a checkout's own client in place of the published one. Each
  needs a trial before B is built.
