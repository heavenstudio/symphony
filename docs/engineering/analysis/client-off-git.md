# The built client out of git

What it would take for git to hold no built web client, the client travelling only inside a
package on crates.io, and when the commits could stop. The choice is the owner's, on
[its card](../../DECISIONS.md#client-in-the-crate). Read at main `4397600b` on 2026-11-15; nothing
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
