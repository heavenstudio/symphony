# Releases

How a Symphony release is cut, named and read. Projects that use the Symphony follow its release
tags, never main ([its card](DECISIONS.md#symphony-update-command)), since other projects use it. A
release comes from a run that verifies main in full; an urgent fix may be released out of cycle. It
is judged by the checks this repository owns, never by a consuming project's suite: a project's
own reds stay on its own board. The evidence and the update command that takes a release are in
[the analysis](analysis/symphony-update.md).

## A release

A release is an annotated tag on a commit of `origin/main` that every check found green:

- **Its name** is `release-YYYY-MM-DD`, the day's date on the machine that cut it, and
  `release-YYYY-MM-DD.N` for that day's Nth release, N from 2. Releases sort by day, then by
  number, so `.10` follows `.9` (`crates/symphony-build/src/release/tags.rs`).
- **Its message** begins `NAME: Symphony main at COMMIT`, then a `Board schema N` line and a
  `Pin N` line, each saying whether it changed since the release before (`Board schema 17,
  changed since release-2026-11-12.2 (16 then).`, `Pin 1, unchanged since …`), then the checks it
  passed, what it publishes on crates.io while the project publishes
  ([the packages](#packages-on-cratesio)), what a project does to take it ([its notes](#its-notes)),
  and the commits on main's first-parent line since that release. The first release says so instead. The schema is `VERSION` in
  `crates/tasks/src/schema.rs` and the pin `PIN` in `crates/symphony/src/machine.rs`, read at the
  tagged commit.
- **Reading one:** `git ls-remote --tags origin 'release-*'` lists them, and
  `git tag -l --format='%(contents)' NAME` prints a message. A project names the tag in its
  `Cargo.toml` (`tag = "release-…"` in place of `branch = "main"`) and its `Cargo.lock` binds the
  commit.
- **What a project's manifest changes:** a release whose pin move needs a consuming project to
  change more of its `Cargo.toml` than the tag says so here, with the line, until every project has
  taken it. The first release that holds the library's package as `symphony-app` (renamed from
  `symphony`, [the packages](#packages-on-cratesio)) needs the library's line to name it:
  `symphony = { package = "symphony-app", git = "https://github.com/heavenstudio/symphony", tag =
  "release-…" }` in place of `symphony = { git = "https://github.com/heavenstudio/symphony", tag =
  "release-…" }`. Until it does, that project's own move of its pin to such a release fails to
  resolve the package (the refresh moves no project's pin, [the refresh](#the-refresh)).

### Its notes

A release says what a project does to take it, for the agent that moves the project's pin
(`crates/symphony-build/src/release/consumer.rs`). Its sections, each left out when it says
nothing, and who writes each:

| Section | Derived from git at the two releases' commits | Said by a landing |
| --- | --- | --- |
| Cargo.toml and Cargo.lock | the line that names the release, as a tag or as the full commit for a project that pins one, the lock following it, and each workspace package whose version moves in `Cargo.lock` | `manifest`: any other line a project's manifest changes |
| Board | the schema's move (`crates/tasks/src/schema.rs`) with the steps the [writer rule](TASKS.md#who-writes-a-board) asks (the project's own build at the new schema first, each writer rebuilt and run, `just tasks migrate`, whose refusal is the one list of the writers, `--leave-behind` for a writer that cannot be rebuilt where it was, and a server's writes refused until it runs the new schema), and the pin's | `board`: what the move changes for a project |
| Public Rust items a consumer calls | the [consumer-API guards](CHECKS.md#consumer-api-changes) that changed | `api`: `OLD => NEW` for a replacement shipped beside the old item, `removed OLD => NEW` for a removal |
| symphony.toml | the keys [PROJECT_FILE](PROJECT_FILE.md)'s tables list anew, and those they list no more | `settings`: what a project writes instead, a file or a command's arguments |
| Installed recipes, skills, hooks and rules | the files under `crates/symphony/shipped/` that changed, which `symphony init` rewrites | `init` |
| Tools | the Rust toolchain (`rust-toolchain.toml`), SurrealDB (`tools/surreal.sh`), Node (`apps/symphony-web/.node-version`) and the oldest PostgreSQL the board's server runs on (`crates/tasks/src/server/postgres.rs`), each when it changed | `tools`: any other |
| Run once | | `once` |

- **An entry** is a trailer of a commit on the landing's own branch, one line each, which git
  reads from the message's last paragraph: `Consumer-Note: SECTION: TEXT`. The land's merge keeps
  the branch's commits, so a release reads the entries of the commits between it and the release
  before, and no branch edits a file another does. Notes are written for what a consumer can
  see, and nothing is written by hand at release time.
- **Composed.** An `api` entry `OLD => NEW` in one release and `removed OLD => NEW` in a later one
  read as one instruction: the first says "move to `NEW` before" the release that removes `OLD`,
  and the removal says since which release `NEW` has been there.
- **A check at the landing's tip.** A task's lint runs `symphony-build consumer-notes --check`
  ([CHECKS](CHECKS.md#what-a-landing-tells-a-consumer)): red
  when the branch, against the commit it left the default branch at (its own commits and their
  difference, however often it merged main), moves the board schema with no `board` entry, stops
  holding a call in a consumer-API guard with no `api` entry, or takes a key out of the settings
  reference with no `settings` entry, and for a trailer that reads as no entry. A guard's call
  that moved, was wrapped anew or gained a comment is the same call, and one added beside the old
  owes nothing. `Consumer-Note: api: none: WHY` answers for a guard changed with no consumer to
  move, and is left out of the notes.
- **After a release is cut**, what its landings left unsaid is said by a later commit naming it,
  `Consumer-Note: release-NAME SECTION: TEXT`. No tag and no installed release is rewritten: the
  entry is read as that release's in every notes file written from then on, marked with the commit
  that said it and the release that commit came in, for a project still behind the release it
  names. It is said again in the release whose commits hold it, in that release's section of the
  notes file and in its tag's message, under a heading that names the earlier release, so a
  project already at or past the one it names reads it when it takes the next. It answers nothing
  for the branch that says it.
- **Where they are read.** A release's own section is in its tag's message, after the checks and
  before the commits. Every release's, oldest first, each under `## release-NAME` with its commit,
  is `notes.md` in the release's folder on the machine ([the install](#the-install)), written from
  the history of the commit installed. `just symphony-cli release-notes [FROM] [TO]` prints the
  sections after FROM up to TO from the newest installed release that holds the file, with no
  clone and no network, so a project several releases behind reads one list in order. FROM is a
  release's name or its commit, by default the tag, else the commit, the project's `Cargo.toml`
  pins. A bound the notes do not hold is refused with what they hold, never answered with a part
  of the steps. `symphony-build consumer-notes [ROOT]` prints the same file from a clone.

## Packages on crates.io

The plugin kit, the helpers and the product are published on crates.io, each by its version,
under MIT: crates.io is the package manager their versions live in
([its card](DECISIONS.md#consumer-contract-versioning)).

- **The kit:** `symphony-plugin`, `symphony-blocks` and `symphony-docs`
  ([the kit](PLUGIN_PROTOCOL.md#the-kit)), one version for the three, from 1.0.0. A new kind of
  block, text span or media, or a new word one carries, is a minor of it (1.1.0), which a plugin
  written for 1.0.0 takes by itself and keeps building on: those lists are `#[non_exhaustive]`
  in the code, as they must be before the first publication, since marking one later is itself a
  new major ([what is marked and what holds it](PLUGIN_PROTOCOL.md#versions)).
- **The helpers:** `symphony-env`, `symphony-args`, `symphony-schema`, `process-cwd` and
  `repository-read`, each on a version line of its own from 1.0.0.
- **The product:** the library and its `symphony` command as `symphony-app`, the board
  (`symphony-board`: `tasks`, `agent-send`, `tasks-copy`, `tasks-reclaim` and `tasks-move`), the server
  (`symphony-server`), the Chrome driver `symphony shot` drives as `symphony-chrome`, and the app's
  wire types (`symphony-api`). One version for the five, from 0.1.0, each requiring the others at
  exactly it (`=0.1.0`), so they are always taken together; below 1.0, it promises no name.
- **The rest,** the client's browser and build tests (`symphony-web-tests`; the client itself is
  no Cargo package, and the server embeds it built), the Mac app
  (`symphony-desktop`), this repository's own build and release commands (`symphony-build`) and
  the tests' time (`test-time`), keep the workspace's `publish = false`.

Two names were another project's on crates.io, so those two packages alone are renamed (the board
task `tools/publish-to-crates-io`): the library's package is
`symphony-app`, its library still named `symphony` (`[lib] name`), and the Chrome driver's is
`symphony-chrome`, its library `symphony_chrome`. Their folders keep their names
(`crates/symphony`, `crates/chrome-devtools`), and so do the binaries. A dependent names the library
`symphony = { package = "symphony-app", … }`, so its code says `symphony::` as before; Cargo's
`-p` takes the package's name, `-p symphony-app`.

The product's commands install from crates.io with:

```sh
cargo install --locked --config profile.release.strip=true --config profile.release.codegen-units=1 symphony-app symphony-board symphony-server
```

The workspace's `[profile.release]` is read from its root manifest alone and is not in a published
one, so the install line sets both of its settings itself: it strips the symbol table and builds
each crate as one codegen unit, as the shipped binaries' size budgets assume
([CHECKS](CHECKS.md#size-budgets)). Without the second an install builds 16 units a crate and its
binaries are 0.5 to 2.7 MB larger than the lines measure. No member carries a profile of its own,
which Cargo would ignore with a warning on every build here. The server embeds its client at build time from
its package's `client/public`, so an installed server serves it with no build of the client; its
package carries every build that folder keeps, as it stands (three builds, 1.5 MB before
compression in November 2026; crates.io takes a package of 10 MiB). The day it would not fit, the
packages test fails, and the package is cut to the newest build.

A published package's manifest says `publish = true` with a description, the repository, a readme of
its own, keywords, categories and an `include` list (its sources, its build script, what it embeds,
readme and LICENSE: no `tests/` folder or fixtures, and none of the binaries only this repository
runs: `symphony-test-plugin`, `fake-hc`, `symphony-shelf`; `symphony-build`
is a package of its own, unpublished).
Its LICENSE is a copy of the root's, never a link, which a package cannot carry out of the
workspace. Its readme stands alone; the server's is `CRATE.md`, as its `README.md` is its guide. It
names a published sibling by `path` and `version`, and an unpublished one only as a development
dependency, by `path` alone, which publishing drops. A file a package's code reads from outside it
is a copy in the package, since the package must build alone: the server's `board.css` and
`frame.css` of the client's (`apps/symphony-web/src/styles/`, which the client's build reads in
its own package) and the Chrome driver's `fixture_process.sh` of `tools/` (where the board's fixtures find
the source root by it). `crates/symphony/tests/packages.rs` holds which members are published, that
each carries all of this, that each copy is its file byte for byte, and that the server's `.crate`
is one crates.io takes.

The thirteen packages were first published on 17 November 2026 from
[`d44d0e7d9fbb42160f047d5bd7b787a1933a6967`](https://github.com/heavenstudio/symphony/commit/d44d0e7d9fbb42160f047d5bd7b787a1933a6967):
the kit and helpers at 1.0.0, the product at 0.1.0. All thirteen downloaded registry archives match
the verified package checksums from that source. The installation settings above were checked
for 0.1.0 with `--version '=0.1.0'` and an empty private `--root`, using Rust 1.99.0 on macOS
arm64. All four commands answered `--version`; the installed server's static client assets and
notices also matched the published archive over HTTP.

A version is published once: crates.io can yank it but never removes or reuses it. Publication
runs on the owner's word, using Cargo's supported authentication; no agent logs in or reads a
token. Before upload, `cargo publish --dry-run --locked` naming the thirteen together packages
them in dependency order and builds each against the others as packaged. An interrupted
publication resumes with only the versions the index still lacks, respecting the registry's
retry time; the archives and source stay fixed. See
[crates.io's publishing rate limits](https://crates.io/docs/rate-limits).

**A release publishes them** only on the project's word: `[releases] publish = true` in
`symphony.toml`, off when unsaid. This repository's `symphony.toml` leaves recurring publication
off; the owner's one-time first publication did not turn it on. While it is off, a green release
says `release: crates.io: not published: [releases] publish is off`, and its record and `--status`
say the same. No `cargo publish` runs (`crates/symphony-build/src/release/crates.rs`). While it is on, a
green release, once its tag is pushed and before [the refresh](#the-refresh), publishes from its
own checkout brought to the release's commit, within an hour, the packages of the workspace that
publish (`publish` unset, or naming `crates-io`) whose version crates.io's index lacks. A version
the index lists already is skipped (`on crates.io already: …`), so a run again publishes only what
is left. The rest go up by one `cargo publish --locked -p A -p B …` naming only them: cargo orders
them, packages and builds each against the others as packaged before it uploads the first, so a
package that does not build uploads none, and waits for the index between uploads. The index is
read again after it: what it lists went up (`published …`), and the rest are left, with why. What
cargo said goes to `logs/ID/publish.log`. The tests publish to a registry folder of their own
through a stand-in for cargo, never to crates.io.

- **The token** is cargo's own: `CARGO_REGISTRY_TOKEN` in the run's environment, cargo's
  credentials or a credential provider, read by cargo and never by the run. With none, cargo
  refuses the first upload, the step says `not published: no token: …` with the packages left; the
  release stays green and the run's exit is unchanged.
- **A failed command** says `not published: cargo publish failed (…)`, with cargo's line and the
  log, then what the index read again lacks and that the release stays tagged. The release stands,
  its tag pushed; the run exits 1 and nothing is filed: the record and `--status` keep it
  (`last publish: red, …`). `just release --refresh`, an orchestrator's retry once the cause is put
  right, publishes what is left before it refreshes, and so does the next release's run.
- **A version crates.io had already**, which cargo refuses as existing though the first read of the
  index lacked it, is no red: the step says what is left, and the next publication finishes it.
- **`--no-push`** publishes nothing: a run under it pushes no tag and so publishes none, and a
  refresh under it says `not published: --no-push publishes nothing`.
- **Its notes**, written before its tag, say after the checks it passed what it publishes: each
  package and version crates.io lacked then (`Publishes to crates.io: NAME VERSION, ….`), or that
  crates.io holds each (`Publishes nothing: …`).

**A version moves with its sources.** `just crates-check` (`symphony-build crates-check`, the step
`crates-check` of the release run's `just verify`) packages, for each published package whose
version crates.io lists, that package here (`cargo package --no-verify`, offline, building nothing)
and compares every file with that version's `.crate` from crates.io, Cargo's own
`.cargo_vcs_info.json` and `Cargo.lock` aside. A difference is red, naming the package and the
files that differ: its version must move before a release publishes it. A version crates.io does
not list yet is green, with nothing to compare, and a crates.io it cannot read is red too
(exit 2): a read that got no answer, or a server's error, is tried three times before it is. It
reads crates.io and publishes nothing, so a task that changes a published package runs it at its
own tip: `just lint-rs` ends its Rust steps with `symphony-build crates-check --branch`, the same
check of the published packages the branch changes since it left the default branch (a tracked
file in the package's folder, or the workspace's `Cargo.toml`, whose settings each packaged
manifest inherits; a file moved out of the folder too) and of no other. A branch that changes
none reads nothing, of crates.io or of a package. One that changes any reads crates.io, waiting
20 seconds at most for what it lists and again for each version it holds: where crates.io gives
no answer in that time (none at all, or a server's error) the step says on stderr that the
packages it names were `not checked` and exits 0, since a lint is never red on the machine's
network, and that line is no evidence either. An answer that stands as an error, a package that
cannot be made and a red comparison stay red, and `just crates-check` itself fails closed as
before. On the default
branch's own tip, where the release run lints, that step has nothing to read, and the whole check
is the run's own step, whose red is a step's (`tests/release-red-verify-crates-check`). The kit's
three packages moved to 1.0.1 this way, a doc comment of `symphony-plugin` having changed after
1.0.0 was published (`tests/main-red-crates-check-plugin-sources`).

**The API decides the size of a bump.** `just semver-check`, the `semver-check` step of release
verification, runs [cargo-semver-checks](https://github.com/obi1kenobi/cargo-semver-checks) 0.51.0
on the three kit packages and five helpers, with all features and each manifest's actual version.
It compares each with the latest eligible published version from crates.io, including the current
version when already published. Removals, added constructor arguments and other detected breaks
require a major; additive changes require the tool's minor where applicable. There is no
`--release-type` override. A missing baseline, failed download or failed rustdoc build is red.
The product's five packages, below 1.0, keep their consumer guards instead.

`tools/semver.sh` fetches the checksum-pinned upstream executable for Intel or ARM macOS or glibc
Linux into the checkout's build folder (`CARGO_TARGET_DIR`, else `.target`), under
`tools/cargo-semver-checks/0.51.0/`. It writes nothing to the user's tool installation. It needs
Bash, curl, tar and `sha256sum` or `shasum`; a missing download or unsupported host fails with setup
guidance. The pinned tool supports this workspace's Rust 1.99.0. The registry baseline and its
rustdoc JSON are cached by the tool in that build folder's `semver-checks/`.

Release verification already needs crates.io for the source comparison and does not run wholly
offline. The semver step likewise retrieves its baseline from crates.io on a cold cache: HTTPS
access and the dependencies of both versions are required. The caches avoid repeated builds;
offline release verification is not supported. A registry lookup or baseline build that fails is
red. No vendored rustdoc snapshot needs updating at each publication. The checker does not
compare arbitrary argument, return or field types, serialization behaviour or the two-release
consumer migration rule. It also permits a new non-exhaustive enum variant in a patch; this
project requires a minor for a vocabulary addition. The typed guards and review still hold these
([consumer API changes](CHECKS.md#consumer-api-changes)).

**How a bump cascades.** A package names a published sibling at exactly the version the sibling is
at (`crates/symphony/tests/packages.rs`), so a sibling's new version changes the manifest of each
published package that builds with it. That package then packages other sources at its own
version, and the crates check is red for it until its version moves too, and so on to each package
that builds with it in turn. A helper's new version so moves each kit or product package that names
it, the kit's three together and the product's five together, each line by its own version.

A consuming project can keep its Git release tag and Cargo lockfile pin, or take the published
packages together from crates.io. Change the complete Symphony dependency group in one edit: a
kit from crates.io beside a library from Git would be two copies of the kit, whose types do not
match ([the analysis](analysis/consumer-contract.md#two-references-two-copies)). A project's own
landing moves its Git pin; the machine refresh never does
([the refresh](#the-refresh)).

## The release run

`just release` (`symphony-build release`, `crates/symphony-build/src/release.rs`) cuts the
releases, and is the one place the full verification runs ([CHECKS](CHECKS.md#the-release-run)).
It is a command of `symphony-build`, this repository's own binary in a package of its own, which no
project is given and neither the library nor the shipped `symphony` compiles
([CHECKS](CHECKS.md#size-budgets)):

1. **The candidate.** It fetches `origin/main` (`--remote` and `--branch` name another remote and
   main line), the commit a project can fetch, and takes its tip; while a release candidate is
   held it takes that one's tip instead, or runs nothing ([below](#the-release-candidate)). A tip
   that is a release already is not checked again. A tip that does not descend from the newest
   release is neither checked nor tagged: a release only moves forward, so a main that went back
   stops the run.
   When the candidate is the main checkout's main too, the run asks the regenerator a land
   starts where main's generated files stand (`just tasks regenerate --status`,
   [TASKS](TASKS.md#the-regenerator)): while it makes them the run waits, within its time, and
   once they are current it goes on; red or stopped on the candidate, it is refused at once,
   recorded nowhere and filing nothing, since a main whose committed client is behind its
   sources is no release and the regenerator's own red is the task. A land starts the
   regenerator just before the `[board] on_land` that starts the cadence's run, so what the
   regenerator last wrote may be of an earlier tip, or nothing at all: with none making them
   and nothing written of the candidate, the run gives it a minute to take the candidate before
   it is refused the same way. Main moving on in the main checkout while the run waits (the
   regenerator landed what it made, or another land came in) is followed: `origin/main` is
   fetched again and what it moved to is the candidate from then on, waited for in its turn; a
   move that has not reached `origin` within that minute refuses the run the same way, as the
   regenerator was last seen not to have made the candidate's files. A
   project with no generators waits for nothing, and so does a candidate the main checkout does
   not hold as its main, whose own client check says. An out-of-cycle release (below) does not
   ask, and neither does a rerun of the release candidate, whose fixes commit their client.
2. **Its own checkout.** It checks the candidate out at `../symphony-wt/release` (`release` in the
   `[agents] worktrees` folder of `symphony.toml`, or `--worktree PATH`): a detached worktree it
   made itself, locked with its own reason (`the Symphony release run's checkout`) so the board's
   worktree sweep leaves it (its `.target/` the sweep clears only once nothing touched the
   checkout for 7 days), and marked in its git admin folder (`symphony-release`, holding the
   checkout's real path). Each run resets it to the candidate and removes every untracked and
   ignored file but `.target/`, so the build stays incremental; a `.target/` built on for 7 days
   is cleared. It resets nothing else. The client's `node_modules` goes with the ignored
   files, so each run installs it again from npm's cache on the machine (a network
   fetch only for what that cache lacks), and the run needs the client's pinned Node, on PATH or
   by fnm ([its guide](../apps/symphony-web/AGENTS.md#the-tools)), as it needs Chrome and the
   machine's PostgreSQL in the folder `[board] postgresql` names, whose tests otherwise fail
   naming it ([PROJECT_FILE](PROJECT_FILE.md)). A link in that place, a path that is, holds or lies inside
   the main checkout or another worktree (compared by real path), a worktree it did not lock and
   mark, or any other folder is refused and left as it is, and the run stops, naming it. An
   `index.lock` or `HEAD.lock` that an interrupted git left in the checkout's admin folder is
   removed when `lsof` finds no process holding it; one held stops the run. Before it resets
   anything, any process but the run's own that `lsof` sees working in the checkout, or holding a
   file inside it open, stops the run. The run names it by its pid and command, and one working in
   a package's folder by that package too, since cargo runs a package's tests there and what one of
   them left works there (`working in apps/symphony-web-tests, where cargo runs the tests of the package
   symphony-web-tests`): that is as near to the test that left it as the run can tell. It could change
   the checkout under the run's checks. It may be anyone's: a shell, a terminal tab, an editor's
   terminal, a Finder window or a `tail -f` left there counts as much as what an earlier check
   left. So the red `run` it files is
   the owner's, not an executor's, since an executor ends only a process it started (RULES.md
   `delete-by-name`). End the process, or move it out of that folder, and the next run goes. A
   process found is listed again for up to 5 s, and counts only if it is still there, so one on
   its way out is not counted. This look sees a process only while it works in the checkout or
   holds a file there open. One that moved elsewhere (a daemon that changed to `/`) and writes
   into the checkout by path is not seen.
3. **The full verification** runs there: `just verify`, which is lint, the real client check, the
   size budgets ([CHECKS](CHECKS.md#size-budgets)), the docs audit, the crates check
   ([the packages](#packages-on-cratesio)) and every test target of the workspace, the board's
   `perf` and `child_guard` among them. Each check goes through the
   checkout's `tools/record.sh` as one test operation of the board's recorder, labelled
   `release NAME` ([MACHINE_WORK](MACHINE_WORK.md#recording-builds-and-tests)), and writes its
   output to a log of its own. It runs in a process group of its own and inherits no descriptor of
   the run's, its lock included (below); what its group still runs when it ends is killed with it.

   **The runner's setup belongs to the release run.** Before `tests`, verification runs the
   explicit `nextest-setup` step (`just nextest-setup`), on a full run and a tests-only rerun.
   `tools/nextest.sh` verifies this target's cached cargo-nextest first, then the main checkout's
   target from a linked worktree. When neither holds the pin, setup downloads it into the main
   cache, once for the machine's worktrees; a main checkout uses its own target. Both the upstream
   archive and executable are checked against committed SHA-256 values, and every later use
   rechecks the executable's hash and version. It needs Bash, curl, tar, `sha256sum` or `shasum`,
   and HTTPS to GitHub for a cold cache; macOS and glibc Linux have pinned builds. A failed fetch,
   hash or install is red at `nextest-setup`, with its reason in that step's log; `tests` does not
   start. No test downloads the runner, and no manual setup in the release checkout is owed
   before a land. Focused Cargo tests need no runner: the live runner fixture is explicitly
   ignored there, and the tests step runs it once by name, where absence fails.

   **Its steps.** Each check is given a folder of its own among the run's logs
   (`SYMPHONY_CHECK_STEPS`, `logs/ID/KEY.steps/`), where `just verify` lists the steps it ran, each
   with its exit status and a log of its own ([CHECKS](CHECKS.md#the-release-run)). The run records
   each listed step under its check (`verify/tests`) and tells a check by its steps
   (`crates/symphony-build/src/release/steps.rs`): each red step is a red of its own (step 5), keyed by
   the check and the step, `tests/release-red-verify-tests`, with that step's log; the check files
   a red of its own only for a red no step it listed holds, when it was stopped, ended by a signal,
   never ran, or ended red with every step green. While its steps hold its red, an open red of the
   check gets a line naming them, and it closes once the check is green; a step's red filed while
   that one is open names it in turn, since the check's older red can be the same failure, so the
   two are read together and fixed as one where they are. A list that does not read
   is said in the run's output, and the check is told as one, as is a check that lists nothing. It
   is one task for each red step, not one listing them, because each red then has one owner
   (RULES.md `tests/a-red-on-main-has-one-owner`): a size budget and a failing test are fixed apart
   and at once, and each task closes as its own step turns green.

   After each check and before it tags, the run looks at its checkout again:
   - HEAD must be the candidate;
   - `git status --porcelain` must name nothing outside `.target/`;
   - no process may be in it, as at the start (step 2). The check's own killed group and the
     board's recorder writing down how the check ended are on their way out, so they are not
     counted.

   Before it tags, nothing has run in the checkout since the last look, so only HEAD and
   `git status` are looked at again. Anything else stops the run, naming what changed and what
   is there, and nothing is tagged. Nothing the check that has ended found green counts, since it
   may have checked other bytes, and it closes nothing. Each red it found is still filed, a red
   step as a task of its own (step 5), before the stop ends the run, and the red `run` names
   them, so no red hides behind the stop. A process found makes the red `run` the owner's, as at
   the start. A change with no process there is an executor's: a check that leaves a file outside
   the ignored folders wants an ignore rule.

   **What the looks do not see.** Each look sees one moment, and only what git does not ignore:
   - **Ignored files.** A file git ignores (`build/`, `artifacts/`, `.target/`) is never named,
     though the checks read such files: `just verify`'s tests serve `build/verified-client/public`
     and run binaries from `.target/`.
   - **A change undone.** A change made and undone between two looks leaves nothing to see.

   So a process that rewrites an ignored file a check reads, or that changes a tracked file and
   puts it back before the check ends, can make a check pass on other bytes, and the commit is
   still released. In practice the run is protected from both in three ways:
   - the checkout is the release run's own, made, locked and marked by it, outside every other
     checkout, so nothing else works there by design;
   - each check runs in a process group of its own that is ended with it, so nothing it started
     in that group outlives it;
   - a process working in the checkout or holding a file there open, as what a check leaves
     usually does, is found at the next look or at the next run's start.

   What none of that finds is a process that left both the check's group and the checkout's
   folder (a daemon that changed to `/`) and writes into the checkout by path. Only what it does
   to files git names gives it away.
4. **What judges a release** is the checks this repository owns, the full verification above,
   never a consuming project's suite. The run builds and tests nothing of a project the machine
   serves, and reads the machine's registry only to refresh ([below](#the-refresh)): a project's
   own reds stay on its own board and never hold a release, and a Symphony change that breaks a
   project is found by that project's own gate when its own landing moves its pin to the release.
   Since nothing here changes its code, which was written against its older pin, a public name its
   code calls changes only with a consumer release plan, and those names are held at every task's
   gate instead, by a
   `tests/consumer_api.rs` that the workspace's lint compiles in each package a consumer's code
   calls: `crates/tasks` (`symphony_board`), `crates/symphony` (with `symphony_plugin`,
   `symphony_api`, `symphony_docs`, `symphony_schema` and `symphony_args`),
   `apps/symphony-server`, `crates/process-cwd` and `crates/repository-read`. Each holds the
   functions and methods it names, most by signature (coerced to their pointer type), some by a
   call written as a consumer writes it, and a few, a generic one among them, by name alone; the
   structs a consumer builds by the fields it builds them with; the variants a consumer builds,
   compares or matches, as it does; and the plugin traits by the methods a
   plugin writes. The exhaustive product enums are also matched there by every variant, the
   guards' own choice: a variant added fails the guard, and waits for a consumer release plan only
   where a consumer matches that enum whole, while a variant that only that match names goes or
   changes with it. Vocabulary variant inventories wider than known consumer calls are left
   to the semver step. Known names remain for the two-release rule. Their guards retain typed consumer calls and cross-crate type identity,
   which the checker does not compare, and the kit's `Reply` exhaustive match because a known
   consumer matches it whole. A major bump never waives the two-release migration rule.
   A field read or method called on a value that no guard names is the project's to meet when it
   moves its pin: its own gate builds it on the new one.
5. **Red:** no tag. Each red check, and each red step of a check that lists them (step 3), is a
   task on this repository's board, `tests/release-red-NAME`, filed at p0 and marked red, so it
   takes no orchestrator slot and is offered before other work of its priority
   ([TASKS](TASKS.md#slots)). It is staffed before anything else (RULES.md
   `workflow/release-reds-first`), fixed in the code and never by a loosened test or check (a test
   red on load is discarded instead, below), and filed with no fast-fix mark: what its fix needs
   to land is read at review by its assessor, as any task's ([needed
   stamps](TASKS.md#needed-stamps)): its tests, the failing tests unchanged and green at its tip,
   for a fix that changes none of the project's tests and checks (`[land] check_paths`), and a
   fresh reviewer's approve as well for one that changes a test, a check, a budget or a run step
   ([the default landing](TASKS.md#the-default-landing)). Its text says, a red step's as
   its line in its check's list recorded it:
   - the command and how it ended;
   - its log, and the load average when it ended (over 1, 5 and 15 minutes, with the processor
     count): a step's, the one its line says it ended under; a check's, the run's reading, kept in
     its record; and under it, when the machine slept under the run, for how long ([its
     time](#the-release-run)), as a wait or a timeout that failed across a sleep failed on the
     sleep and not on load;
   - what the log names failed (`crates/symphony-build/src/release/failures.rs`), then a few of its last
     lines; its last 40 lines alone when it names nothing it knows, and for a check that listed its
     steps, whose log is its own lines and its steps' tails. A test run's, a log in which cargo
     started a test target, names each test libtest said `FAILED`, with where it panicked (its own
     thread's panic before that of a thread it started) and the first line of what it said, then
     cargo's list of the targets that failed (`error: N targets failed:`, or each target's own `to
     rerun pass` line when there is no list), and nothing else of it: the tests' own output, what
     a failed test printed among it, says the errors, refusals and failed tests of what they run;
     what a failed test printed ends at libtest's list of the failed tests, or, where that list
     never comes (a test binary died while cargo went on), at its next target, so no failure
     after it is lost. One that names no failed test or target (stopped at the run's time, hung or ended by a
     signal) names only the errors its build said before cargo started its first target, so its
     last lines say where it stopped. Any other log names each line with a word starting `error`
     or `refus` (never one inside a name: `thiserror`, `gix-error`, `std::error`), or the docs
     audit's `✖`, a compiler's `-->` line after an error, and the step's own last word, the line
     before just's saying its recipe failed (`client-check: …`), with each line its tool starts
     the same way (`size-budget: …`), and never a line of cargo's progress (`Compiling gix-error
     v0.3.2`). Each stops at 40, then says how many more, and a quote is fenced past every run of
     backticks its lines hold;
   - what of it is the environment's (`crates/symphony-build/src/release/gone.rs`): a test target that
     named no failed test and never ran or never compiled because a file its own build had made
     was gone (its executable, cargo's `could not execute process … (never executed)` for no such
     file; a library it links, rustc's `extern location for … does not exist`; a build script's
     output, `couldn't find file` in a `…/build/UNIT/out/`) failed for its build folder, not for a
     test of its own. The red says so, an environment failure, naming those targets and files,
     the target cargo started last before the first that found one gone (what removed them ran
     then: that target, something it started, or something outside the run), and what the build
     folder holds when the red is filed, each entry with when it was made. Found gone while the
     tests' lanes ran side by side (after `tools/test-lanes.sh`'s lines naming them, more than one
     at once), they went while those lanes ran, which the red names, and no one target: the lanes'
     outputs follow one another in the log, but a test of any of them may have removed them, and
     the log holds no times to tell which. Those targets are no failures to rerun: with no other
     failure, the rerun is a run of the target they went while, or of each of those lanes alone,
     one after another, or of its build, with a look at its build folder after each;
   - the commands that rerun each failure alone (`crates/symphony-build/src/release/rerun.rs`), never
     `just verify`, which no task runs. Each test target cargo says failed in the log is rerun as
     `just test-rs ARGS -- NAME --exact` for each test libtest named `FAILED` in it, or whole when
     it named none, at most 12 lines. A log naming no failed target reruns by what it ran: a step,
     by the command its line recorded, and a test run, each target alone (`just test-rs -p PACKAGE
     --test TARGET`, `--lib` for a package's own tests), the one its last lines name, never the
     whole run again. A check red beyond the steps it listed names each step by its recorded
     command, and one it did not reach by what its last lines name; one that listed none names the
     last recipe its log says failed other than `verify` and a private one (`_NAME`), its tests'
     a target at a time, else says its last lines name the step. Any other check's is its own
     command;
   - the commits since the last green run (`git log --oneline LAST_GREEN..TIP`), and the same
     range on a line of its own by the two commits' full names, `between: LAST_GREEN..TIP`
     (`symphony_board::red::BETWEEN`), from which the Reports roll-up lists the landings the red
     may trace back to ([the day files](analysis/reports-and-monitoring.md#the-day-files)). With
     no green run on record the range starts at the release before; with neither there is none.

   **A test red on load** is discarded: a rerun neither fails nor passes on the machine's load
   (RULES.md `tests/no-test-fails-on-load`). A test that fails a run, or the release candidate's
   rerun, because of load (it passes alone, or its failure is a wall-clock bound, a sleep-ordered
   step or a timeout under load) is removed from the tree in its red's own fix, with one line on
   the red's thread naming the test, the evidence it is load (its runs alone and the failing line)
   and what it proved. That rule is the approval: no card is put to the owner to discard it. If
   what it proved matters, a follow-up task may bring it back written on a condition, a count, CPU
   time or an injected clock; the release does not wait for that. No rerun is spent on it, no
   rewrite of it is waited for, and nobody lowers the machine's load to help a run pass: an
   orchestrator holds no other work back to keep the load down for a run. A test that fails alone
   on what its code did, or fails the same way without load, is not this case and is fixed in the
   code.

   A check red again while its task is open adds a line to that task, with that run's load and
   reruns; a run green in it closes the task (`cancelled`, saying the check passed) when its
   candidate holds the commit the check was last found red on. The next red of a closed task's
   check is `tests/release-red-NAME-2`, then `-3`. A run of main's tip that is red in a check
   holds that commit as the release candidate ([below](#the-release-candidate)), and its reds'
   tasks say how a fix of one starts, merges and lands; their fixes' lands, not main's tip, are
   what the next run checks.
6. **Stopped short:** a run that ends without deciding (a fetch that fails or does not answer, a
   checkout it cannot take, a git lock held, a process in the run's checkout, a checkout changed
   under a check, a main behind the newest release, a push that fails, a run told to
   stop, a lock or a check that outlives its run, below) is red too, as the task
   `tests/release-red-run`, filed, updated and closed by the next run that gets through, as a
   check's is, and filed the same way. Its body says who fixes it: the owner when a process
   stopped it, for which nothing lands, else any free executor, then how a check's red is
   staffed and what its fix needs to land. A command
   line or
   `--commit`/`--worktree` argument refused (exit 2, or exit 1 naming why) runs nothing and files
   nothing.
7. **Green:** it tags the candidate `release-<today>` as above and pushes that one tag:
   `git push --no-follow-tags --recurse-submodules=no origin refs/tags/NAME:refs/tags/NAME`, an
   explicit refspec, so a user configuration that pushes more (`push.followTags`, a
   `remote.origin.push` list) adds nothing to it. `--no-push` keeps the tag here. A push that
   fails keeps the tag here and is the red `run`; a later run on that same commit fails the same
   way, naming the push that delivers it, and `--status` names each release here that the last run
   found missing from the remote. No run pushes an older tag. A pushed release then publishes its
   packages on crates.io while the project says so ([above](#packages-on-cratesio)), and refreshes
   the machine ([below](#the-refresh)).

**Its time.** A run has ten hours (`--timeout SECONDS`, at most 24 hours: a killed run's check
holds later runs off for its run's time). A check still running then is stopped (its
group sent `TERM`, then `KILL` after 10 s) and red, as is each check not yet started. A git
command stops at the run's time or after 15 minutes, whichever is sooner. A run whose checks all
ended green within its time keeps 5 more minutes to tag and push.
That time is counted while the machine is awake, and what a record says a check or a run took is
the wall clock's, as a log's times and a step's own seconds are: a sleep of the machine takes none
of the run's time and is counted in what it took. What the wall clock counted of a run past the
awake one, once it is 5 s, is how long the machine slept under it, kept in its record (`slept`,
in seconds; left out of a run that did not sleep) and said by the run, by
[`--status`](#status) and in each red it files (`crates/symphony-build/src/release/span.rs`); no
power log is read. On macOS a run holds an assertion against idle sleep for as long as its
process lives (`caffeinate -i -w PID`), which changes no power setting; a closed lid on battery
sleeps all the same.
A run told to stop (`HUP`, `INT` or `TERM`, as launchd's stop or a terminal sends them) stops its
check's group the same way, and its git, records why, files the red `run` (the stopped check is no
check's red) and releases the lock. A signal it was started ignoring stays ignored, and `KILL`
stops it where it is (below). That holds for the run alone: each check starts with every signal at
its default and none blocked, as launchd or cron starts a job, so a run started as a background
job of a non-interactive shell (`INT` and `QUIT` ignored) or under `nohup` (`HUP`) never hands
that to the tests a check runs, which end their children by those signals.

**One run at a time.** `run.lock` in the record folder is held by the run's own process alone: no
check inherits it, so nothing a check leaves running holds it. `running.json` beside it names the
run, its time and, while a check runs, that check's process group and when its leader started. A
second run is refused while the lock is held; one that finds it held more than 15 minutes past the
holder's time, or held with no living run saying so within 2 s (what a run that took the lock a
moment before takes to say so), files the red `run`, naming the processes `lsof` sees holding
it. A run killed while its check ran leaves its `running.json`; a later run that finds that
check's group alive (a group of that id led by a process started at another time is not it) is
refused while the killed run's time lasts, and past it files the red `run`, naming the group.
Neither stops anything.

A run that ended without recording itself (killed, or the machine stopped) is recorded by the
next run that takes the lock, before it checks anything: the run `running.json` named, and any
whose logs no record names. Each is recorded `interrupted`, from the record it kept beside its
logs while it ran (`logs/ID/run.json`): what it was doing (the check that ran, or the last that
ended), when it was last heard of and how it was found. The run says so, and the record keeps its
logs among the newest 14 runs'; nothing is filed for it.

Each run is recorded in the git common dir: `.git/symphony/release/runs/ID.json` (the commit,
each check with its exit and log, the result, the tag, what it did on the board) and each check's
log under `logs/ID/`. The newest 60 records and the logs of the newest 14 runs stay. Its exit is 0
green (or nothing new), 1 red, stopped short or refused, or a green release whose publication or
refresh failed, 2 a command line refused.

## Out of cycle

`just release --commit COMMIT` releases an urgent fix without waiting for the next run: the same
checks on COMMIT, then the same tag. It refuses a commit that is not on
`origin/main`, one released already and one that does not descend from the newest release, and
tags nothing when a check is red (filing its red as a release run does). A green release closes a
red only when COMMIT holds the commit that red was found on, so an urgent fix from before a red
leaves it open. It holds no release candidate and runs whatever one is held; a release of it that
the candidate does not descend from drops the candidate ([below](#its-bounds)).

## The release candidate

A release may never come if each run judges main's tip: a red's fix lands with everything else
that landed meanwhile, and the next run judges a different tree, whose new landings can be red in
turn. So a run of main's tip red in a check holds the commit it judged as the release candidate
while its reds are fixed, and the release that comes is that commit and its fixes alone, however
fast main moves (`crates/symphony-build/src/release/candidate.rs`; the design and the options weighed:
[the analysis](analysis/release-candidate.md), A).

- **Held.** Its record is `candidate.json` beside the runs (`.git/symphony/release/`): the commit
  it holds now (its tip), the commit first judged and by which run, the runs of it that decided
  (its rounds), each red with its task and how it was answered, each check, step and lane with the
  commit and run it was last green on, the digest of its inputs there and the environment of that
  run, the advances, and, once it ends, how. The local branch
  `release-candidate` names its tip. Only the run moves that branch, and only from the commit it
  last set it to: one moved by someone else, or checked out in a worktree, is left as it is and
  said. Nothing pushes it, and it goes when the candidate ends. One candidate at a time. A run
  that stopped short holds none, an out-of-cycle release holds none, and `[releases]
  candidate_reruns = 0` or `candidate_hours = 0` holds none at all. A record this build cannot
  read, or of a newer version, stops the run as the red `run` and is left as it is.
- **A red's fix** is the red's own task, whose text says so. Its branch, named for its slug,
  starts at `release-candidate`, never at main; it merges `release-candidate` again before review,
  as that branch moves when another red's fix lands, and never merges main before its land
  ([CHECKS](CHECKS.md#after-main-moves)). When the fix changes what the client is
  built from it commits the distribution its own sources make (`just client-update`), the one kind
  of branch that commits it, since no regenerator builds the candidate; the land keeps main's side
  of it ([TASKS](TASKS.md#the-default-landing)). Its approve, where it needs review, names the
  full tip it read, and it lands by `just land`, as any task. A red fixed by another red's fix is closed as covered by that
  landing (`just tasks done SLUG NOTE --covered-by OTHER`), never landed. A fix made outside this
  repository (a consumer's) answers its red by the task cancelled, saying where. A test red on load
  is discarded by its red's fix ([step 5](#the-release-run)): the candidate's rerun runs without
  it, and no rerun is spent to see it pass.
- **The advance.** The land's `--due` (`[board] on_land`) settles the candidate first, under the
  run's lock: a landing whose slug is a red's task in the record moves it to the newest commit an
  approve on that task named, or its landing's line says its needed stamps stood on where it
  needed no review, that is the landed branch's tip or behind it, descends from the
  candidate's tip, and holds beyond it nothing main held before the land (`git rev-list --count
  X ^TIP` equal to `git rev-list --count X ^TIP ^MERGE^1`). A land that went straight in gives its
  tip; one that conflicted, whose executor merged main and was approved again, gives the commit
  approved before that merge, and one that needed no review gives none, and drops it. A task forked at the candidate passes the same tests, which is why
  only a red's own land moves it, and a land of any other task, stray or not, moves nothing. A
  landing counts only once `origin/main`, as this checkout last fetched or pushed it, holds its
  merge: one landed in the main checkout and not pushed (its push failed) waits, said, until its
  retry pushes, and the next count or run reads it. A red's task done (approved) waits for its
  land, unless it was closed as covered by another's landing (its thread's last verdict says so):
  covered by a landing that advanced the candidate, it is answered, that fix holding its own;
  covered by a red's task whose land has not advanced it yet, it waits for that land; covered by
  any other landing, its fix is on main and not in the candidate, which is dropped. One cancelled
  with no landing is answered, its fix made outside this repository.
- **The advance of a fix a pull request delivered.** There the host squashes the fix into main
  and the candidate still takes the fix's own commits, both read from the land's one receipt: the
  squash is the landing, its one parent is main before the land, and the head the receipt kept
  (reachable under `refs/symphony/delivered/SLUG`, as no history of main holds it) is what
  landed. The same test picks the commit, among the commits an approve named and that head, which
  vouches for itself: the squash was asked for under evidence that stood on it, so a land cut
  before it closed the task advances the candidate all the same. Main is never merged into the
  candidate: a head that merged main gives the commit approved before that merge, or none, which
  drops it. The record's advance keeps the squash (`merge`), the commit taken (`to`) and, under
  `delivered`, the receipt's head, parent, repository, pull request and evidence revision. A
  receipt whose head is gone from the repository makes the candidate wait, said. The candidate's
  tip is then a commit main does not hold in its history, and neither is the release tagged on it:
  each is on the line by the squash that delivered it there, read from the receipts, which is how
  the candidate stays open, how its rerun is allowed, how the count goes on from that release and
  how the next release of main's tip is one that moved forward. A pull request that did not
  merge leaves no receipt, and the red waits as for any task still open. Once the candidate
  advanced so, `release-candidate` names a head main's history does not hold, and a delivery
  refuses a head that merges a commit the remote's default branch lacks, so on such a line a
  red's fix never merges `release-candidate`. A fix that starts at `release-candidate` after the
  advance holds no such merge and is delivered as any head. One started before the advance and
  not yet published or approved is rebased onto `release-candidate` before its assessment and
  review: that is a new head, so its assessment, each stamp and its approve are of the rebased
  head, and nothing gathered on the old one counts. One already published, or approved, is left
  as it is: nothing is rewritten behind an approval or a pull request, which a delivery never
  forces, and neither is main or a release's tag. Its land then answers no advance, as it does
  not descend from the candidate's tip, and drops the candidate, so main's tip, which holds
  both squashes, is what is run. Fixing a candidate's reds one after the other, each started at
  the candidate as the one before left it, keeps it. One limit is the receipts' own: a task has
  one receipt, so a slug delivered a second time replaces it; the count then misses the first
  squash, and a release tagged on the first head is held by nothing, which a person judges until
  [[task:tools/github-delivery-receipt-history]] keeps each delivery's receipt: a repeated
  delivery of one task is not supported here. The entries a delivered landing said
  ([its notes](#its-notes)) are read from the head its receipt kept, as the squash carries none,
  and a release tagged on a delivered head stays in the notes of every release after it by the
  same reading. A fix that changes what
  the client is built from is not delivered so yet: a delivery refuses a head that changes a
  generator's outputs, and the candidate's fix must commit them, so that fix waits for the
  delivery of generated outputs and its red stays open, or the candidate is dropped.
- **The rerun.** Once every red is answered, the candidate's tip is rerun: by the `--due` of the
  land that answered the last red, else by the next landing's, the next scheduled run, or `just
  release --rerun`, an orchestrator's retry. It waits for no regenerator, as a fix commits the
  client its own sources make. Of each check it runs what was red, what did not run and what a fix
  changed, and keeps the green of the rest without running it (*carried*). A check, a step it
  listed or a part of a step (a lane of the tests) is carried when it was green in a run of this
  candidate, on the environment the rerun runs on ([below](#what-a-run-ran-on)), and either that
  run's commit holds the tip's tree, or the digest of its inputs there is the one the tip's tree
  tells ([its inputs](#a-steps-inputs)). Anything else runs: a red, a step or lane whose digest
  changed, one the tip names that no run of the candidate listed (a step known only by its lanes
  among them), one its check listed as not run, and one whose inputs cannot be told. The tests a
  red `nextest-setup` kept from starting are such a step: `just verify` lists them as not run, no
  red of their own and no task, so the rerun runs them with the setup, and no rerun of the setup
  alone releases a commit whose tests never ran. A check that lists its steps lists every one,
  run or not: that list is all a rerun knows of a tree that tells no inputs. A step or check
  green in a rerun that ran only some of its parts stands green by those parts, each on the
  commit and run it was green on, never as if run whole there. So a check that lists its steps reruns those alone (`just verify lint tests/browser`),
  and one that lists none, or whose red was beyond its steps, runs whole. The commit tagged is
  then not itself run in full, the trade the candidate's design names
  ([the analysis](analysis/release-candidate.md#the-trades)), bounded by the candidate: no green
  is carried from before it, from another candidate, from a task's own recorded run or from
  another machine, and its first run is whole. Green, it tags the tip and refreshes the machine as
  any release; red, it is one more round, its reds filed as any run's and their tasks saying the
  candidate again.
- **Its release notes** say, between the checks and the changes, the commit first judged and by
  which run, how many reruns it took, each red's task and how it was answered (the commit it
  advanced to, the landing that covered it, or outside this repository), and each green carried:
  its commit and run where that commit's tree is the release's, and its commit, run and the digest
  of its inputs where it was carried by them, under a line that says it was not run on this
  commit. The run's record names them the same way (`rerun.carried`).

### A step's inputs

The tree says what its checks read: `tools/check-inputs.sh`, which the run asks in its checkout
before the checks of every run (`crates/symphony-build/src/release/inputs.rs`), prints a line for
each step of `just verify` and each lane of its tests, `verify/STEP` or `verify/tests/LANE`, with
a digest of its inputs at that commit. The run's record keeps them (`inputs`), and a candidate's
green stands with the digest it was green on.

- **What each reads** is said beside it, never in a list of its own: `tools/verify.sh` for its
  steps and `tools/test-lanes.sh` for its lanes (`reads`; `--inputs` prints them). A lane's
  packages are derived: the packages that hold its test target, and every package of the workspace
  they depend on by path, from `cargo metadata`, each by its folder. A lane whose tests run another
  package's binary names that package (the browser tests, the server's), and a path its tests read
  outside those is declared (the client's sources).
- **Every digest** holds the tree outside `apps/` and `crates/` as well, each entry of the root:
  the check itself (`tools/`, the justfile, the lock file, the toolchain pin, the Cargo and nextest
  configuration, `symphony.toml`) and every document, guide and skill, which any test may read.
  It holds the toolchain installed too, as `rustc` and `cargo` say their versions. So a change to
  `tools/verify.sh`, `tools/check-inputs.sh`, the justfile, the lock file or the toolchain pin
  carries nothing.
- **The whole tree** is what lint (it reads the branch's commits), the docs audit, the crates
  check, the nextest lane (every target that is no lane of its own), the doctests and the hooks
  tests declare: those are carried only from a commit of the same tree, as every step was before
  a step had inputs.
- **What cannot be told is run.** A tree with no such script, a script that fails (a package or
  a target no manifest names, a cargo that does not answer) or prints what does not read, and a
  name it leaves out carry nothing by a digest, and the rerun says which. The digest is the tree's
  own word, never computed by the run, so a tip whose script differs from the green's has other
  digests.

Its one risk is a path a test reads that no input names: its green is carried across a change
that broke it. A test that reads a file outside its packages and outside the root's other entries
declares it beside its lane. The hosted board's step declares the board's package, so it is run
again on any change to the board or a package it builds with. The PostgreSQL 16 and 18 programs
it starts are the machine's, which no digest holds: they are named in the environment a run ran
on (below), each by the version it says, and a green of another environment is never carried.

### What a run ran on

No tree pins the system, Chrome or PostgreSQL the checks run on, so each run's record names them
(`environment`): the system and its version (`sw_vers` where there is one, and `uname -srm`), and
Chrome and PostgreSQL each as it says its own version (the Chrome the tests find, the `postgres`
of `[board] postgresql`, else the first on `PATH`), or that none was found. It names the
PostgreSQL 16 of the hosted board's check as well, found as that check finds it (the folder
`SYMPHONY_POSTGRESQL_16` names, else Homebrew's keg), by the version it says or why it says
none. A candidate's green stands with the environment of its run, and a rerun on another carries
none of them and says so; a record written before the 16 was named names none, so its greens are
of another environment and are run again.

### Its bounds

The candidate is dropped, and a run of main's tip is due at once, whatever the count, when:

- as many reruns as `[releases] candidate_reruns` allows (3) came back red;
- it was first judged `[releases] candidate_hours` (24) hours ago;
- a red's task landed with no approved commit that passes the advance, as when its executor merged
  main before any approve;
- a red's task was closed as covered by a landing that did not advance it, its fix on main and not
  in the candidate;
- a release it does not hold was tagged (one on main passed it), or its tip is not on
  `origin/main`, each by its history or by the squash that delivered it there;
- or `just release --drop`, an orchestrator's act, drops it.

A release of its tip tagged by other means ends it as released. Each bound is read whenever it is
settled, by each `--due` and at the start of each run; the reruns bound also after each red rerun.

The run of main's tip a drop makes due follows it once that run reaches its first check, or finds
that tip released already: one stopped before, as by a fetch that failed, leaves it due, and the next
landing asks for it again. That run is a run of main's tip as any other: red, it holds its commit
as a new release candidate.

### Main while it is open

While a candidate is open no run checks main's tip: a `--due` whose candidate waits for a red says
so (`next run: held for the release candidate …`) and runs nothing, and so does a scheduled run or
`just release`, which exits 0 recording no run; `--rerun` is refused. The count resumes once it
ends: after its release, from that release, so a busy day's landings start a run of main's tip at
the next landing; after a drop, at once.

## The cadence

Tasks land fast, so a release run also starts once a given number of them have landed, whatever
the hour, and its green release refreshes the server and its projects
(`crates/symphony-build/src/release/cadence.rs`):

- **The count** is `[releases] every_landings` in `symphony.toml`, 20 unless the project says. `0`
  turns the cadence off, and then only the scheduled run or `just release` releases.
- **A landing** is a merge on the main line's first-parent line whose subject names a branch:
  "Merge branch 'SLUG'", the merge `just land` makes, or "Merge branch 'SLUG' into BRANCH", git's
  own made by hand. A merge of the default branch itself ("Merge branch 'main' into SLUG", a task
  catching up, which reached the line), of another repository's branch or of a remote-tracking
  branch is none, and neither is any other commit but a squash a receipt names.
- **A land delivered by a pull request** (`[land] delivery`,
  [TASKS](TASKS.md#a-land-delivered-by-a-pull-request)) is one squashed commit of one parent,
  which is no merge, so it is a landing by its receipt alone: the file the land keeps once it
  proved the commit the host made (`receipts/SLUG.json` in the lands' folder), read by
  `crates/symphony-build/src/release/receipts.rs`. A commit on the line counts as the landing of
  the receipt's task when it is the receipt's squash and git bears the receipt out: its one parent
  and its tree are the ones recorded. No subject, patch or pull request title makes a landing, so
  a squash with no receipt is none. A receipt that names a commit on the line which is not the
  one it recorded stops the count by name (`landings: cannot be counted: the receipt of SLUG …`)
  and no run starts by landings until a person has judged it; `just release` by hand still runs.
  Merges and squashes count on one line together, and a line with no receipt counts as it always
  did.
- **Counted since** the newest release on that line, or a later run that checked something on it,
  green or red, whichever is nearer the tip. A run that stopped short before its first check (a
  fetch that failed, a process in its folders) is not counted from. So a red tip is counted from
  as a release is: the landings after it count afresh, and the next landing does not start the
  same red run again. A red tip held as the release candidate holds the count instead (*The
  release candidate holds it*, below).
- **The first count.** With no release and no run on record, the count is from the line's start,
  so the first landing after the cadence lands takes the line's whole history and starts a run at
  once: the owner wants a release. It says so, in the land's output as in `--status`: "the first
  count, N landings from origin/main's start with no release and no run on record: a run on its
  tip starts now".
- **The line** is `origin/main` (`--remote`, `--branch`) as this checkout last fetched or pushed
  it, which a land's push moves; with no such ref, the local `main`. The count reads that ref, the
  release tags here and the run records with git and the files alone. It fetches, builds and
  writes nothing.
- **The trigger.** A land runs `[board] on_land` once the task is closed and pushed, after it
  started the regenerator ([TASKS](TASKS.md#the-default-landing)), which the run it starts
  waits for (*The candidate*, above), and shows what it said within its first seconds. This
  repository names there the build of `symphony-build` its main checkout links,
  `.target/bin/symphony-build release --due`, and the land appends the task's slug and its merge
  commit. Nothing slow sits on or behind a land: short of the count, `--due` prints the count and
  what the next run waits for and exits 0, having run git alone, no cargo and no recipe, so no
  `just tasks` or `just land` waits on a build lock of its. Once the count is reached it takes the
  lock, brings the release run's own checkout (`../symphony-wt/release`) to `origin/main`'s tip,
  builds `symphony-build` there with `tools/rs-bin.sh --build symphony-build` in that checkout's
  own build folder, never the main checkout's, and becomes that build, which runs exactly as
  `just release` does: the full verification, the tag and its push, each red filed, then the
  refresh. A tip that cannot be built so is said, and its run goes on from the linked build, which
  meets the same red in its checks. The record names what started it (`cause`: `20 landings since
  release-…, the last SLUG`). A red run files its reds, as any release run does. The link comes
  from `just release`: every run of the recipe in the main checkout, `--status` too, builds
  `symphony-build` and makes the link or renews it (`tools/rs-bin.sh --link symphony-build` does
  it alone). A main checkout that links none starts no count: the land shows its refusal. No
  landing is lost meanwhile: the next count reads them all from git.
- **A landing whose command never ran.** The count is read from the line and the receipts each
  time, never added to by the command, so a land cut between its merge and its `on_land` (a
  delivered land's receipt is kept before the task is closed and the command started) loses no
  landing: the next count asked takes it, and one told twice is counted once. What it does lose is
  the asking. Nothing counts by itself, so when that landing was the one that made a run due and
  no other landing follows, no run starts until something asks: `--status` says
  `next run: due: N of M landed; the next landing starts it, or `just release --due``, and
  `just release --due`, an orchestrator's retry, starts it, as the scheduled run does at its hour.
  A land run again after such a cut does not start the command a second time
  ([TASKS](TASKS.md#a-land-delivered-by-a-pull-request)): a project's command need not be safe
  to run twice.
- **The release candidate holds it.** Before it counts, `--due` settles the release candidate
  under the run's lock ([the advance](#the-release-candidate)), unless a run holds the lock, which
  settles it itself. While one is open and a red of it waits, the count starts nothing, however
  many have landed (`next run: held for the release candidate …`); once every red is answered, the
  rerun of its tip is due, and once it is dropped, a run of main's tip is, each whatever the count
  (`cause`: `the release candidate …'s reds answered` or `the release candidate dropped: WHY`). A
  record the counting build cannot read (a newer build's) starts the run, whose build of the tip
  reads it or stops on it. `every_landings = 0` starts none of these either.
- **One run at a time.** While a run holds the lock (a scheduled run's, one run by hand, an
  out-of-cycle release's, a refresh's or another landing's, its build included), `--due` starts
  nothing and says the count waits for it. The landings it saw meanwhile are not lost: once that
  run has checked a commit, the count starts again from it, and the next landing that brings it to
  the count starts the next run. Between `--due`'s look and the run's start, a run that took the
  lock first refuses the second, which is not doubled either (*One run at a time*, above).
- **A landing that moves the board's schema** (it raises `VERSION` in
  `crates/tasks/src/schema.rs`, which its `Consumer-Note: board` trailer says,
  [CHECKS](CHECKS.md#what-a-landing-tells-a-consumer)) is followed at once by a release run,
  whatever the count ([the owner's A](DECISIONS.md#schema-landing-then-release), 2026-11-19).
  Main's build moves this repository's board at its first command after the landing
  ([TASKS](TASKS.md#where-it-lives)), and the machine's server, a release's, then writes nothing
  to it until a release at the new schema serves it ([the switch](#the-switch)), unless the step is
  declared to keep the Symphony writing and that server's release reads the record the declaration
  writes ([CHECKS](CHECKS.md#a-schema-step-that-keeps-the-symphony-writing)). A declared step
  changes nothing below: the run still starts at once, and a second step still waits. The run starts
  now rather than at the count's, and nothing else is shortened: a red one is held as the release
  candidate as any run is ([the release candidate](#the-release-candidate)), and `current` then
  moves under the refresh's rule on writers as for any release ([the refresh](#the-refresh)).
  The orchestrator who landed it runs `just release` once the land is done ([who runs
  it](#who-runs-it)), unless the land's own `--due` started a run, which is that run. Such a
  landing waits, approved, while a run or the release candidate holds the release (`just release
  --status`: `next run: in progress`, or a candidate open), since a run started then judges
  nothing of main's newest commit ([main while it is open](#main-while-it-is-open)), and lands
  once neither does. A second one waits until `current` names a release serving the first's
  schema (`--status`'s `machine:` and `last refresh:` lines), so the board is never two schemas
  ahead of the server, which reads nothing of a board that far ahead.

## The refresh

A green release refreshes the machine: it installs the release and brings the machine to it, and
writes nothing in any project's checkout (`crates/symphony-build/src/release/refresh.rs`). No
branch is made in a project, no commit, no pin moved and no main fast-forwarded: a project takes
the release's kit and helpers by its own landing, which moves its pin through its own gate.

**What the run does with a consuming project.** It builds and tests nothing of a consuming
project's, and reads the machine's registry (`projects.json`) only to refresh. A Symphony change
that breaks that project's build or tests is found by its own gate when its own landing takes the
release, not by the release. Of a project, the refresh moves its live board, under the writer rule
below, and nothing else.

Once a release's tag is pushed, the run does this, in order:

- **The install.** The release built once and installed in the machine folder, the step
  `refresh-install` ([the install](#the-install), below). One that fails is red, and nothing after
  it runs: `current` stays where it was.
- **The switch.** While a registered project names a plugin linked into its host ([the
  switch](#the-switch), below), the release is installed and nothing more: no board moves and
  `current` is not pointed, and the run names the project that holds it (`the switch to RELEASE is
  held: …`), as `--status` does. The machine's server is then a project's build at that project's
  own pin, and a board moved past it would leave that server only reading it.
- **The boards.** A release that keeps the board schema of the release `current` names moves no
  board (`RELEASE serves board schema N, as the release current names serves N: no board moves`),
  and nor does one behind it, as no board moves back (`RELEASE serves board schema N, behind the
  release current names, which serves M: no board moves, as none moves back`).
  One that raises it, or the first, with no `current`, judges the live board of each project in the
  registry, this repository's aside (its board moves with its own main), one step `refresh-ID` for
  each. A project whose id is not a plain name, whose checkout is gone, or that is not a git
  checkout is left out, and the refresh says so (`left out: ID: …`). Every board is judged before
  any moves, each read through its running database:
  - A project with no board folder, a board not made yet, and one at the release's schema or past
    it have nothing to move.
  - **The writer rule.** A board moves only when every writer seen there in the last week (its
    writer record, [who writes a board](TASKS.md#who-writes-a-board)) is a release's own or was
    built at the release's schema. A release's own is a binary of a release installed in this
    machine folder (`releases/NAME/bin/`) on this host, which pointing `current` takes on. A board
    with any other writer behind stays where it is, and the step names each writer to rebuild, at
    its schema, by its path and host and when it was last seen: the project's to finish. The
    machine's server is never counted, as it takes the new schema only by its reload.
  - A board whose database is not running is red: nothing here starts, signals or stops a
    database, and once it runs (any `just tasks` in its checkout starts it) the retry moves it. So
    is one running an older SurrealDB than the release pins, which the move would replace, and one
    whose versions cannot be read, the release's or the database's: the guard fails closed. So is
    a board whose schema or writers cannot be read.
  - A board below schema 18, from before the board recorded who writes it, has no record to judge
    and is never moved here: its project's own `just tasks migrate`, in its main checkout, brings
    it up. It is the project's to finish, and holds nothing, as it is behind every release.

  A board that lags, red or the project's to finish, holds the release: no board moves and
  `current` is not pointed, and each board that could move says it waits (`ID: its board can move
  from schema N to M, and stays at N while X holds the release`). Otherwise every board is
  snapshotted through its running database, all of them before the first move
  (`symphony/tasks/snapshots/before-schema-N-RELEASE.surql` in the project's git common dir, the
  newest 5 kept; for a board on PostgreSQL `before-schema-N-RELEASE.sql`, `pg_dump`'s text of
  the board's database, which `psql` restores): a snapshot that fails is red and no board moves.
  A board on PostgreSQL is read on that engine, by the PostgreSQL programs its project names, and
  waits on no SurrealDB version, as a release pins none of the machine's PostgreSQL. Then each board is moved in
  turn by the release's own `tasks migrate` (`releases/RELEASE/bin/tasks`) run in the project's
  main checkout, the one command that moves a project's board, and its schema is read again. Where
  the only writers behind are a release's own, it runs with `--leave-behind`, which names them;
  otherwise the migrate's own check of its writers is a second guard.

  **A move that fails partway.** The boards move one at a time, so a release can be left
  half-moved: the first migrate that fails is red and stops the rest, and `current` is not
  pointed, as it is not when it cannot be pointed after every board moved. The boards moved before
  it stay at the release's schema, each line saying so; the one that failed keeps the schema it
  had, its snapshot kept; each after it stays where it was (`ID: its board can move from schema N
  to M, and stays at N while the move of ID's board failed`). Until the retry gets through, a
  moved board is ahead of the release `current` names (none, before the first): one schema ahead,
  that release's server reads it and writes nothing to it, saying so on its pages, unless the step
  keeps it writing ([CHECKS](CHECKS.md#a-schema-step-that-keeps-the-symphony-writing)), and two
  ahead it refuses it ([the switch](#the-switch)); that release's `tasks` and `agent-send` refuse it
  either way, naming themselves as the program to rebuild ([who writes a
  board](TASKS.md#who-writes-a-board)). The retry, `just release --refresh` once the failure's
  cause is fixed, judges every board again: one moved already is at the release's schema, with
  nothing to move, and the rest are snapshotted and moved, then `current` is pointed. The way back
  to the schema a board had is its snapshot, restored by hand ([the way back](#the-way-back)).
- **`current`.** Once no board holds it, `current` is pointed at the release ([the
  install](#the-install), below): every recipe of a project with no build of its own runs it from
  then on. One that cannot be pointed is the install's red, `current` as it was.
- **The machine's server.** Last, the run says which server answered on the machine's record
  before `current` moved. Nothing here starts, stops, signals or restarts it. A release's server
  follows `current` by its own reload ([the install](#the-install)): the run waits, 5 minutes at
  most, until the machine's server answers as the release, and says which process now answers; or
  that it still answers as the release before, with the last line its reload wrote in the
  server's log since `current` moved, which names why the release did not take over and when it
  is tried again. With none since, it quotes the reload's last line before, as a reload that
  waits to try a release again writes nothing meanwhile; with none in the log's newest part, it
  says the reload may not be running, never that it is not
  (`just symphony-status` says which answers later). A project's build as the machine's server never
  follows `current`, and the run says so: a stop and a start, the owner's word for the shared
  server, make the release its successor. With none answering, the next start of the one server
  runs the release when it serves every registered project. The wait is said, never red.

Each step ends in one of three:

- **Moved**, or nothing to move. Green.
- **The project's to finish**: a writer seen on its board in the last week is behind the release's
  schema, each named with what to do (rebuild it at that schema and run it once, which records it,
  then an orchestrator's `just release --refresh` moves the board); or its board is below schema
  18 (`cd ROOT && just tasks migrate`, its own, brings it up). Green here and filed nowhere: it
  files no task and closes none, and the run's exit is not changed by it. The step says which, with
  what finishes it and the command that says what taking the release asks of the project (`just
  symphony-cli release-notes`, [its notes](#its-notes)), in the run's output (`release: refresh:
  ID: the project's to finish: …`, then
  `release: refresh-ID the project's to finish`), the run's record (the step named in
  `refreshed.theirs`, its line in `refreshed.said`) and `--status`.
- **Red**: the refresh's own work failed. The registry cannot be read; a board's database is not
  running, could be replaced by the move, or its schema or writers cannot be read; a snapshot or a
  migrate fails, or a board ends short of the release's schema. A refusal met once the run's time
  was up or it was told to stop is red, as the run's own.

A red step says what it found, the release stands (its tag is pushed), and the run exits 1.
`current` stays where it was, a migrate that fails leaves the snapshot taken before it, and the
boards moved before it stay moved (*A move that fails partway*, above). It is
filed as `tests/release-red-refresh-ID` (`tests/release-red-refresh` when the registry cannot be
read), saying what was left as it was and who fixes it, and filed as a check's red is. A refresh
green for it closes the task.

What stays by hand, said in `--status` under a refresh with a step that is its project's to
finish:

- each writer named behind the release: rebuilt at its schema and run once, which records it;
- a board from before the writer record: brought up by its own `just tasks migrate` in its main
  checkout;
- then the retry, `just release --refresh`: an orchestrator's act, the owner having authorised the
  refresh itself, as it is after a red step's fix. It runs the refresh again for the newest release
  here, as a run of its own under the lock, checking nothing, and finishes what the steps did not:
  the install, the boards' moves, `current` and the wait for the server.

A refresh's install has an hour, its build within it, and the boards, `current` and the wait for
the server have another hour after it. `[releases] refresh = false` turns it off, the install with
it. Each project's step logs every command it ran (`logs/ID/refresh-ID.log`), and the install its
build's output and what it said (`logs/ID/refresh-install.log`).

### The install

A release is built once for the machine, never per project, and installed in the machine folder
(`~/Library/Application Support/Symphony/` on macOS, `$XDG_DATA_HOME/symphony` elsewhere), beside
the registry (`crates/symphony-build/src/release/install.rs`,
`crates/symphony/src/machine/release.rs`):

```text
releases/release-2026-11-20/
  manifest.json        the release and its commit, the board schema, the plugin protocol, the
                       tasks api version and the Symphony pin it serves, each artifact's path
                       and size
  notes.md             what a project changes to take each release up to this one
  bin/symphony-server
  bin/tasks
  bin/agent-send
  bin/symphony
  client/HASH/         the client, named by the hash of its files
current -> releases/release-2026-11-20
```

- **The build.** In the run's own checkout, at the release's commit and in its build folder, the
  run builds the release profile of the four binaries (`tools/cargo.sh build --release --locked`,
  each named with `--bin`, of the packages `symphony-server`, `symphony-board` and
  `symphony-app`). The server
  is built with no client compiled in (`SYMPHONY_CLIENT_BESIDE=1`, which its build script reads):
  run from a release's folder it serves `client/HASH/` from beside it. The client is the
  distribution the commit holds (`apps/symphony-server/client/public/`), which the release's own
  `client-check` compared byte for byte with a build of its sources, so nothing builds it again.
- **The manifest.** What the release serves is asked of the binaries that build made, never read from
  the program that installs, which a retry runs from a later main: `symphony-server --serves`
  prints its plugin protocol, its board schema and the Symphony pin it serves, and `tasks api`
  answers the method `version` with its api version and its schema. A server and a board built at
  different schemas are never installed. Each binary is an artifact with its size in bytes, and
  the client one, with the bytes and the count of its files.
- **Whole or not there.** The folder is written under a temporary name beside it
  (`.NAME.installing.PID`) and renamed into place, so `releases/NAME/` is either the whole release
  or absent. A build that fails, a binary that is missing or cannot say what it serves, or a
  commit with no client distribution leaves no folder and `current` where it was, and is the
  refresh's red, filed as `refresh-install`; `just release --refresh` is the retry. A folder in
  place is never written into, since a binary in it may be running: a retry that finds the
  release installed from the same commit builds nothing and goes on to the boards and `current`,
  and one that finds that name holding another commit, or no whole release, refuses and names the
  folder to remove by hand.
- **`current`.** The install points nothing. The refresh points the machine's `current` link at
  the release once nothing holds it ([the refresh](#the-refresh), above): a new link made beside
  it and renamed over the old, never the link rewritten, so a reader finds one release or the
  other. Recipes and the board's own starts find a binary in this order: the
  checkout's `.target/bin/NAME`, the main checkout's from a linked worktree, `current/bin/NAME`
  in the machine folder, then `PATH` (`crates/symphony/src/builds.rs`,
  `crates/symphony/shipped/symphony.just`). So a project with no build of its own runs the
  release, and one that builds its own, this repository among them, keeps running its own, but for
  the one server, which a start runs from the release when nothing holds the switch (below).
- **Earlier releases stay.** Nothing removes a release's folder, so the way back is there
  ([the way back](#the-way-back), below).

Nothing of the install starts, stops or signals a server. A release's server follows `current`
by its own reload (`apps/symphony-server/src/reload.rs`): it reads the link as a watcher of the
machine folder says it moved, and once every 10 s besides for a move the watcher missed, and,
when it names another release, hands its sockets by exec to that release's server, by its own
folder's path, as a project's build hands them to its next build. No build runs in it. A release
not installed whole, or whose server does not become ready, leaves the running one serving and
says why on its notice, tried again 5 minutes later, the wait doubling with each failure of it in
a row up to an hour; `current` pointed back at the running release clears it. A project's build as
the machine's server keeps serving the build it was started from. What the install built, each
artifact's size and how long the build took are among the refresh's lines, in the run's record
and in `--status`.

### The switch

A release's server serves a project's plugin only from its process (`[plugins.process]`,
[PLUGIN_PROTOCOL](PLUGIN_PROTOCOL.md#turning-it-on)), never one linked into a host. So the
machine's server becomes the release's once every registered project's plugins answer from a
process, and a registered project naming a linked plugin holds the switch: the rule is
`crates/symphony/src/machine/switch.rs`, which a start and the status read. While a release is
installed and a project's build is the machine's server, each project naming a linked plugin says
so on its pages, naming the command that lets it go, and `just release --status` says which holds
it. While something holds it, a refresh installs the release and points nothing ([the
refresh](#the-refresh)), so `current` names the release before it, or none. A start that finds no
server running starts the release's server when it serves every registered project, at its pin
and board schema or older. While something holds the switch, a start by the release's own binary (`just symphony` in a project with no build of its own) starts
nothing: it registers its project and names, in one line, the project that holds the switch and
that `just symphony` in that project's checkout, by its own build, starts the machine's server.
A running project's build keeps serving until a stop and a start make the release its successor,
which is the owner's word for the shared server ([AGENTS](../AGENTS.md)). No start replaces a
release's server. A board one schema ahead of the release (main's build moved it first) is read
by its server, which writes nothing to it and says so on every page until a release at that schema
serves it. The exception is a step declared to keep the Symphony writing, which a release from
schema 25 on writes through
([CHECKS](CHECKS.md#a-schema-step-that-keeps-the-symphony-writing)). A board two ahead, or a
project at a newer pin, is refused until `current` names a release that serves it.

### The way back

A release is taken back on the machine by hand, by pointing `current` at the release before it,
whose folder is still there. In the machine folder:

```sh
ln -s releases/NAME current.new && mv -fh current.new current   # macOS
ln -s releases/NAME current.new && mv -fT current.new current   # Linux
```

which renames the new link over the old and never into the folder it names. A release's server
follows it by its own reload, as it follows any move of `current`, and every recipe of a project
with no build of its own runs that release from then on. Nothing else goes back with it: a board
the newer release moved stays at that release's schema, which the older release's server reads
and writes nothing to ([the switch](#the-switch)) and the older release's `tasks` refuses. Its
store as it was before the move is the snapshot the refresh took
(`before-schema-N-RELEASE.surql`, or `.sql` of a board on PostgreSQL), and a board put back to it is one that went back, which the
owner accepts ([TASKS](TASKS.md#a-board-that-went-back)). Pointing `current` on the machine whose
server is the shared one is the owner's word, as any switch of that server is. The next green
refresh points `current` at the release it installs.

## Who runs it

The release run is the one thing that pushes a tag to `origin`: the release that run cut, and
nothing else. An orchestrator pushes main and never a tag ([AGENTS](../AGENTS.md)). A release
cannot be taken back once a project fetches it, so running `just release` or
`just release --commit` by hand and installing its schedule are the owner's call: an orchestrator
does either only on the owner's word. The cadence's run has that word (*The cadence*, above): a
land starts `--due` without asking, and the release it runs is automatic. So has the run that
follows a landing that moves the board's schema (*The cadence*, above): the orchestrator who
landed it runs `just release` without asking each time, the owner's standing word for those runs
alone, and any other run by hand stays the owner's call. So has a green release's
refresh (*The refresh*, above), so its retry, `just release --refresh`, is an orchestrator's act,
run without asking on a red refresh's task once it is fixed, or once a project finished what its
step left to it (each writer named rebuilt and run once). The machine's server is not the
refresh's to switch: the first time the shared server becomes a release's own server, by a stop
and a start while nothing holds the switch, is the owner's word, and so is pointing `current` back
on that machine ([the way back](#the-way-back)); a release's server that already serves follows
each `current` the refresh points. Turning `[releases] publish` on is the owner's word, the first publication's
([the packages](#packages-on-cratesio)); once it is on, `just release --refresh` after a failed
upload's cause is put right is an orchestrator's act as well.
Running `--due` by hand starts a release, and stays the owner's call. The release candidate's two
retries are an orchestrator's acts as well: `just release --rerun`, once every red of it is
answered and no landing has started its rerun (a red answered outside this repository, say), and
`just release --drop`, for a candidate its fixes cannot reach, after which the next landing runs
main's tip.

## Status

`just release --status` prints the newest release, the last run (its result, when, how long ago,
how long the machine slept under it when it did, what was red and the tasks filed), a later out-of-cycle release when there is one, the last
interrupted run while its logs are kept, the last green run, and the releases here that the last
run found missing from the remote, with the push that delivers one, and, in a project with
`[land] generators`, where the regenerator stands (`regenerator: …`). A release run older than 36
hours is marked overdue, and a rerun of the release candidate is named as one (`(round 2 of the
release candidate first judged at …, 1 carried green)`). Then the release candidate as last
settled: open, its tip, the commit first judged, by which run and how long ago, its runs and red
reruns against `candidate_reruns`, when `candidate_hours` drops it, each red with its task, round
and answer (`open (in_progress)`, `open (landed here, not pushed)`, `answered: its land … advanced
the candidate to …`, `answered: its task closed as covered by …`), and what its rerun waits for;
ended, how and when, its red runs, and whether the run of main's tip its drop made due has reached
its checks; `none held`, or `off` while a bound is 0. Then it
prints the cadence, the landings since what it counts from and what the next run waits for, the
last publication on crates.io with each line it said ([the packages](#packages-on-cratesio)),
the last refresh, with each line it said and each step red or its project's to finish, and last
where the machine's server stands ([the switch](#the-switch)): the release `current` names, or
the releases installed while it names none, whether the machine's server runs it, and, while a
project's build serves, what holds the switch. A refresh that moved the boards, pointed `current`
and saw a release's server follow it:

```text
landings: 7 on origin/main since release-2026-11-20 (91b39f209e72); the last tools/g (23e3da01bc80)
next run: after 13 more landings (7 of 20), or the scheduled run
last publish: of release-2026-11-20 by the run 2026-11-20T1000-34446, 2026-11-20 10:00 (1 h ago)
  not published: [releases] publish is off
last refresh: green, of release-2026-11-20 by the run 2026-11-20T1000-34446, 2026-11-20 10:00 (1 h ago)
  installed release-2026-11-20 in /machine/releases/release-2026-11-20, built in 660 s: symphony-server 31,402,112 bytes, tasks 31,675,888 bytes, agent-send 5,601,824 bytes, symphony 22,668,544 bytes, client/5f0c2d9a41b7e386 6,204,518 bytes in 322 files
  release-2026-11-20 serves board schema 19, plugin protocol 0.3 and tasks api 1.15, at Symphony pin 1
  alpha: its board moved from schema 18 to 19 by release-2026-11-20's own `tasks migrate --leave-behind`, its store snapshotted first to /work/alpha/.git/symphony/tasks/snapshots/before-schema-19-release-2026-11-20.surql
  beta: it keeps no board (/work/beta/.git/symphony/tasks is not there): nothing to move
  /machine/current names release-2026-11-20 (it named release-2026-11-12, which stays installed): a project with no build of its own runs it
  the machine's server (process 50111, port 8765), the release release-2026-11-12, followed current by its own reload (apps/symphony-server/src/reload.rs): process 51234 on port 8765 answers as release-2026-11-20
machine: current names release-2026-11-20; the machine's server (port 8765, pid 51234) runs it
```

One held by a project's linked plugin, which installed the release and nothing more:

```text
last refresh: green, of release-2026-11-20 by the run 2026-11-20T1000-34446, 2026-11-20 10:00 (1 h ago)
  installed release-2026-11-20 in /machine/releases/release-2026-11-20, built in 660 s: …
  release-2026-11-20 serves board schema 19, plugin protocol 0.3 and tasks api 1.15, at Symphony pin 1
  the switch to release-2026-11-20 is held: consumer names the linked plugin consumer-board, and a release's server serves a plugin only from its process: a [plugins.process] command for it in /work/consumer/symphony.toml lets the release serve every project (docs/PLUGIN_PROTOCOL.md); release-2026-11-20 is installed, no board moves, and current is not pointed at it (it names release-2026-11-12)
machine: current names release-2026-11-12; the machine's server (port 8765, pid 50111) is the build of /work/consumer
machine: the switch to release-2026-11-12 is held: consumer names the linked plugin consumer-board, and a release's server serves a plugin only from its process: a [plugins.process] command for it in /work/consumer/symphony.toml lets the release serve every project (docs/PLUGIN_PROTOCOL.md)
```

It names each red step and its log, and each step that is its project's to finish (`refresh-ID
the project's to finish, filed nowhere here; log …`), counted on its first line (`last refresh:
green, 1 step left to its project, of …`), then, under one of those, what stays by hand (*The
refresh*, above) and the command that says what taking the release asks of the project (`just
symphony-cli release-notes`, [its notes](#its-notes)). The next run is `due` once the count is reached, or the release
candidate's rerun or drop makes it so, `held for the release candidate` while a red of it waits,
`in progress` while a run holds the lock, or `off` when `every_landings` is 0. It reads the record
and this checkout's tags and refs, fetches nothing and writes nothing, so a land the candidate has
not been settled with yet is the next count's or run's to read.

## The schedule

Installing the schedule is the owner's call: nothing installs it, and an orchestrator installs it
only on the owner's word. The full run may be scheduled more often than daily; the entry
`just release --schedule` prints runs once a day, and a schedule of more times than one is the
owner's to install as well: a run does not check again a tip released already, and one started
while another runs is refused (*One run at a time*, above). `just release --schedule` prints the
entry and makes nothing; `--at HH:MM` sets its time (03:00 when unsaid). On macOS it is a launchd
agent, `symphony.release.ID`, which once a day, or on waking when the Mac slept through the hour, runs one `sh -c` line with the `PATH`
of the shell that printed it: make the record folder, then `just release` in the main checkout,
appending what it prints to `.git/symphony/release/scheduled.log`. Elsewhere it is a crontab line
running the same. That line runs `just release` in the foreground, never as a background job, so
the run itself starts with every signal at its default too.

```sh
just release --schedule > ~/Library/LaunchAgents/symphony.release.symphony.plist
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/symphony.release.symphony.plist
launchctl bootout gui/$(id -u)/symphony.release.symphony   # to remove it
```

Until it is installed, only the cadence runs `just verify` on main.

## The rename

The release run's old names are `nightly`, the recipe and the command, and the flag `--release
COMMIT` ([the names](analysis/release-candidate.md#the-names)). The first run of a build that knew
the new ones moved what a build that knew only the old left on disk: its records from
`.git/symphony/nightly/` to `.git/symphony/release/`, and its checkout from
`../symphony-wt/nightly` to `../symphony-wt/release`. Releases have been cut under the new name
since, and what is left of the old is this (`crates/symphony-build/src/release.rs`, `release/record.rs`,
`release/board.rs`):

- **The old recipe and command.** Neither the justfile nor `symphony` nor `symphony-build` has a
  `nightly`, so each refuses it as any name it lacks: this repository's `on_land` names
  `symphony-build release --due` ([the trigger](#the-cadence)). `symphony-build release --release
  COMMIT` says "`--release COMMIT` is now `--commit COMMIT`", exit 2, and runs nothing.
- **What an older build makes again.** Its records' folder and its checkout, locked with `the
  Symphony nightly's checkout`, are neither read nor moved: the run records in its own folder and
  checks in its own checkout, whatever the old ones hold or whoever works in them. The checkout
  named as `--worktree` is refused as one the release run did not lock, and the board's sweep
  keeps it as it keeps any worktree locked for a reason it does not know. None of it happens while
  `on_land` names only the linked `symphony-build`, which `just release` keeps current.
- **Records of an older build.** One of version 1 reads as this build's (version 2): its kind
  `nightly` as `release`, its `release` (an out-of-cycle release) as `out-of-cycle`, and each log
  under the old folder where the move put it. The Runs page's day files keep the label `nightly
  NAME` a check of an older build is recorded under, a label nothing reads more of. No schedule is
  installed, so none moves.
- **Open reds.** The board's red series keeps `tests/nightly-red-` beside `tests/release-red-`
  (`symphony_board::red::SERIES`) while a red of the old name is open: the run writes to, and
  closes, a key's open red of the old name before it looks at the new, so the next green closes it
  and nothing is filed beside it. The old prefix goes once none is open.

## Never

The release run never tags a commit that is not on `origin/main`, that does not descend from the
newest release or that a check found red, never moves or deletes a tag, and pushes nothing but its
new tag: never the branch `release-candidate`, which it moves only from the commit it set it to
and never while a worktree has it checked out. It resets and cleans no checkout but its own,
removes no folder it did not make, builds and tests nothing of a consuming project's, and itself
writes nothing to a consuming project's checkout. Its install writes in the machine folder only a
release's own new folder under `releases/`, never one already there, and the `current` link, and
removes there only what an install of that same release left unfinished. Its refresh writes in a
project's checkout nothing at all: no branch, no commit, no worktree, no pin and no move of its
main. It writes to a project only the snapshot of its store, in its git common dir, and moves the
project's board only by the release's own `tasks migrate` run in its main checkout, and only when
every writer seen there in the last week is a release's own or at the release's schema. It never
moves a board below schema 18, never moves one board of a schema-raising release while another
holds it, and never points `current` while a board holds the release, a project's linked plugin
holds the switch or a move failed. It never starts, signals, stops or restarts a board's database
or a server, a release's server taking the `current` it points by its own reload; past its time it
stops only the `surreal export` client it started. Its tests run on a repository of their own with a bare
repository for its remote and a user git configuration that asks every push for more, a scratch
board and a machine folder of their own, with stand-in checks (`crates/symphony-build/tests/release/`),
and shorten the refresh's wait for the shared server with `SYMPHONY_RELEASE_RELOAD_SECONDS`,
which no flag sets. They build nothing: a run on their clock installs only through the stand-in
for cargo `SYMPHONY_RELEASE_BUILDER` names, which no flag sets. They set its clock with `SYMPHONY_RELEASE_NOW`, which no flag sets either: a
run on it is refused unless it pushes nothing (`--no-push`) or every place its remote pushes to
is a folder on this machine, so a release named for another day than the machine's never leaves
it.
