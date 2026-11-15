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
  number, so `.10` follows `.9` (`crates/symphony/src/release/tags.rs`).
- **Its message** begins `NAME: Symphony main at COMMIT`, then a `Board schema N` line and a
  `Pin N` line, each saying whether it changed since the release before (`Board schema 17,
  changed since release-2026-11-12.2 (16 then).`, `Pin 1, unchanged since …`), then the checks it
  passed, what it publishes on crates.io while the project publishes
  ([the packages](#packages-on-cratesio)), and the commits on main's first-parent line since that
  release. The first release says so instead. The schema is `VERSION` in
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
  "release-…" }`. Until it does, the refresh's move of that project's pin is red
  ([the refresh](#the-refresh)).

## Packages on crates.io

The plugin kit, the helpers and the product are made to be published on crates.io, each by its
version, under MIT: crates.io is the package manager their versions live in
([its card](DECISIONS.md#consumer-contract-versioning)).

- **The kit:** `symphony-plugin`, `symphony-blocks` and `symphony-docs`
  ([the kit](PLUGIN_PROTOCOL.md#the-kit)), one version for the three, from 1.0.0.
- **The helpers:** `symphony-env`, `symphony-args`, `symphony-schema`, `process-cwd` and
  `repository-read`, each on a version line of its own from 1.0.0.
- **The product:** the library and its `symphony` command as `symphony-app`, the board
  (`symphony-board`: `tasks`, `agent-send`, `tasks-copy` and `tasks-reclaim`), the server
  (`symphony-server`), the Chrome driver `symphony shot` drives as `symphony-chrome`, and the app's
  wire types (`symphony-api`). One version for the five, from 0.1.0, each requiring the others at
  exactly it (`=0.1.0`), so they are always taken together; below 1.0, it promises no name.
- **The rest,** the client's sources (`symphony-ui`, which the server embeds built), the Mac app
  (`symphony-desktop`) and the tests' time (`test-time`), keep the workspace's `publish = false`.

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
package carries every build that folder keeps, as it stands (7.6 to 8.0 MiB compressed in
November 2026, of the 10 MiB crates.io takes). The day it would not fit, the packages test fails, and the package is cut
to the newest build: no include list can name it, as the server's `Cargo.toml` is a release input,
so a list that followed each build would move the next build's asset names.

A published package's manifest says `publish = true` with a description, the repository, a readme of
its own, keywords, categories and an `include` list (its sources, its build script, what it embeds,
readme and LICENSE: no `tests/` folder or fixtures, and none of the binaries only this repository
runs: `symphony-build`, `symphony-test-plugin`, `fake-hc`, `symphony-shelf`, `symphony-ui-dev`).
Its LICENSE is a copy of the root's, never a link, which the client's release inputs refuse
(`symphony-api` and `symphony-blocks` are among them). Its readme stands alone, with no link into
this repository, which is private; the server's is `CRATE.md`, as its `README.md` is its guide. It
names a published sibling by `path` and `version`, and an unpublished one only as a development
dependency, by `path` alone, which publishing drops. A file a package's code reads from outside it
is a copy in the package, since the package must build alone: the server's `board.css` and
`frame.css` of the client's (the client reads its own through `asset!`, which takes no file outside
its crate) and the Chrome driver's `fixture_process.sh` of `tools/` (where the board's fixtures find
the source root by it). `crates/symphony/tests/packages.rs` holds which members are published, that
each carries all of this, that each copy is its file byte for byte, and that the server's `.crate`
is one crates.io takes.

A version is published once: crates.io can yank it but never removes or reuses it. So nothing here
has been published yet. The first publication is the owner's act, or runs on the owner's word,
with the owner's crates.io token in the owner's environment; no agent logs in or reads a token. It
waits on one more answer of the owner's, asked on the board task `tools/publish-to-crates-io`: each
manifest names this repository as its `repository`, which answers 404 to anyone but the owner while
the repository is private, so either the repository is made public first or the packages are
published with no `repository` line. Up to then, the packages are proven by `cargo publish
--dry-run` naming the thirteen together, which packages them in dependency order and builds each
against the others as packaged, before any is on crates.io.

One more thing that answer touches: `symphony init`'s `Next:` names the project file's reference,
docs/PROJECT_FILE.md, by its github.com URL in this repository (`init::PROJECT_FILE_REFERENCE`),
true while a clone is the only way to the binaries. Once the packages are on crates.io with the
repository still private, that link answers 404 to whoever installed them, so the reference then
travels in `symphony-app`'s package or the binary prints it.

**A release publishes them** only on the project's word: `[releases] publish = true` in
`symphony.toml`, off when unsaid. This repository's `symphony.toml` leaves it off, so turning it on
is the owner's word for the first publication. While it is off, a green release says
`release: crates.io: not published: [releases] publish is off`, its record and `--status` say the
same, and no `cargo publish` runs (`crates/symphony/src/release/crates.rs`). While it is on, a
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
(exit 2). It reads crates.io and publishes nothing, so it is cheap enough for the evidence of a
task that changes a published package; in the release run, its red is a step's
(`tests/release-red-verify-crates-check`).

**How a bump cascades.** A package names a published sibling at exactly the version the sibling is
at (`crates/symphony/tests/packages.rs`), so a sibling's new version changes the manifest of each
published package that builds with it. That package then packages other sources at its own
version, and the crates check is red for it until its version moves too, and so on to each package
that builds with it in turn. A helper's new version so moves each kit or product package that names
it, the kit's three together and the product's five together, each line by its own version.

Nothing is on crates.io yet, so a project takes all of Symphony from Git at its pin: today the
consuming project names each package at a release tag (`tag = "release-…"`) and its `Cargo.lock`
binds that tag's commit, which [the refresh](#the-refresh) moves to each new release. Its one line
for the library changes with the rename, as [the refresh](#the-refresh) says. Once every package it
names is published, it can take them all from crates.io in one edit; not before: a kit from
crates.io beside a library from Git would be two copies of the kit, whose types do not match
([the analysis](analysis/consumer-contract.md#two-references-two-copies)).

## The release run

`just release` (`symphony-build release`, `crates/symphony/src/release.rs`) cuts the releases, and
is the one place the full verification runs ([CHECKS](CHECKS.md#the-release-run)). It is a
command of `symphony-build`, this repository's own binary, which no project is given and the
shipped `symphony` does not carry ([CHECKS](CHECKS.md#size-budgets)):

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
   is cleared. It resets nothing else. The React client's `node_modules` goes with the ignored
   files, so each run installs it again from npm's cache on the machine (a network
   fetch only for what that cache lacks), and the run needs the client's pinned Node, on PATH or
   by fnm ([its guide](../apps/symphony-web/AGENTS.md#the-tools)), as it needs Chrome. A link in that place, a path that is, holds or lies inside
   the main checkout or another worktree (compared by real path), a worktree it did not lock and
   mark, or any other folder is refused and left as it is, and the run stops, naming it. An
   `index.lock` or `HEAD.lock` that an interrupted git left in the checkout's admin folder is
   removed when `lsof` finds no process holding it; one held stops the run. Before it resets
   anything, any process but the run's own that `lsof` sees working in the checkout, or holding a
   file inside it open, stops the run. The run names it by its pid and command, and one working in
   a package's folder by that package too, since cargo runs a package's tests there and what one of
   them left works there (`working in apps/symphony-ui, where cargo runs the tests of the package
   symphony-ui`): that is as near to the test that left it as the run can tell. It could change
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

   **Its steps.** Each check is given a folder of its own among the run's logs
   (`SYMPHONY_CHECK_STEPS`, `logs/ID/KEY.steps/`), where `just verify` lists the steps it ran, each
   with its exit status and a log of its own ([CHECKS](CHECKS.md#the-release-run)). The run records
   each listed step under its check (`verify/tests`) and tells a check by its steps
   (`crates/symphony/src/release/steps.rs`): each red step is a red of its own (step 5), keyed by
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
   project is found by that project's own gate when it takes the refresh's branch. Since a refresh
   moves a consumer's pin and none of its code, a public name its code calls changes only with a
   consumer release plan, and those names are held at every task's gate instead, by a
   `tests/consumer_api.rs` that the workspace's lint compiles in each package a consumer's code
   calls: `crates/tasks` (`symphony_board`), `crates/symphony` (with `symphony_plugin`,
   `symphony_api`, `symphony_docs`, `symphony_schema` and `symphony_args`),
   `apps/symphony-server`, `crates/process-cwd` and `crates/repository-read`. Each holds the
   functions and methods it names, most by signature (coerced to their pointer type), some by a
   call written as a consumer writes it, and a few, a generic one among them, by name alone; the
   structs a consumer builds by the fields it builds them with; the variants a consumer builds,
   compares or matches, as it does, the blocks' among them; and the plugin traits by the methods a
   plugin writes. Each enum whose variants a guard holds is also matched there by every variant, the
   guards' own choice: a variant added fails the guard, and waits for a consumer release plan only
   where a consumer matches that enum whole, while a variant that only that match names goes or
   changes with it. The signature of a function a guard names alone, and a field read or a method
   called on a value that no guard names, are the project's to meet when it moves its pin: its own
   gate builds it on the new one.
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
     its record;
   - what the log names failed (`crates/symphony/src/release/failures.rs`), then a few of its last
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
   - what of it is the environment's (`crates/symphony/src/release/gone.rs`): a test target that
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
   - the commands that rerun each failure alone (`crates/symphony/src/release/rerun.rs`), never
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
fast main moves (`crates/symphony/src/release/candidate.rs`; the design and the options weighed:
[the analysis](analysis/release-candidate.md), A).

- **Held.** Its record is `candidate.json` beside the runs (`.git/symphony/release/`): the commit
  it holds now (its tip), the commit first judged and by which run, the runs of it that decided
  (its rounds), each red with its task and how it was answered, each check and step with the
  commit it was last green on, the advances, and, once it ends, how. The local branch
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
- **The rerun.** Once every red is answered, the candidate's tip is rerun: by the `--due` of the
  land that answered the last red, else by the next landing's, the next scheduled run, or `just
  release --rerun`, an orchestrator's retry. It waits for no regenerator, as a fix commits the
  client its own sources make. Of each check it runs what was red, what did not run and what a fix
  changed. Until the stamps give a step inputs of its own, a step's
  inputs are the whole tree: a check or step green on a commit whose tree is the tip's keeps that
  green and is not run (*carried*, named in the run's record and the release's message), and any
  other runs. So a check that lists its steps reruns its red ones alone (`just verify tests`) when
  the tree is the same, as when every red was answered outside this repository, and runs whole
  otherwise; one that lists none runs whole. Green, it tags the tip and refreshes the machine as
  any release; red, it is one more round, its reds filed as any run's and their tasks saying the
  candidate again.
- **Its release notes** say, between the checks and the changes, the commit first judged and by
  which run, how many reruns it took, each red's task and how it was answered (the commit it
  advanced to, the landing that covered it, or outside this repository), and what was carried green
  from which run.

### Its bounds

The candidate is dropped, and a run of main's tip is due at once, whatever the count, when:

- as many reruns as `[releases] candidate_reruns` allows (3) came back red;
- it was first judged `[releases] candidate_hours` (24) hours ago;
- a red's task landed with no approved commit that passes the advance, as when its executor merged
  main before any approve;
- a red's task was closed as covered by a landing that did not advance it, its fix on main and not
  in the candidate;
- a release it does not descend from was tagged (one on main passed it), or its tip is not on
  `origin/main`;
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
(`crates/symphony/src/release/cadence.rs`):

- **The count** is `[releases] every_landings` in `symphony.toml`, 20 unless the project says. `0`
  turns the cadence off, and then only the scheduled run or `just release` releases.
- **A landing** is a merge on the main line's first-parent line whose subject names a branch:
  "Merge branch 'SLUG'", the merge `just land` makes, or "Merge branch 'SLUG' into BRANCH", git's
  own made by hand. A merge of the default branch itself ("Merge branch 'main' into SLUG", a task
  catching up, which reached the line), of another repository's branch or of a remote-tracking
  branch is none, and neither is any other commit.
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

## The refresh

A green release refreshes the machine and its clients, the projects the machine serves, the
consuming project among them.
Once a release's tag is pushed, the run refreshes the machine to it: one step `refresh-ID` for each
project in the machine's registry (`projects.json`) whose `symphony.toml` names plugins, this
repository aside, and whose packages take one of the Symphony's from git
(`crates/symphony/src/release/refresh.rs`, its projects found by
`crates/symphony/src/release/consumers.rs`). A registered project whose checkout is gone, or is
not a git checkout, is left out, and the refresh says so (`left out: ID: …`).

**What the run does with a consuming project.** It builds and tests nothing of a consuming
project's, and reads the machine's registry only to refresh. The refresh moves what the steps below
say: the pin's move, made on the branch `symphony/RELEASE` through the project's own commit gate;
its main, fast-forwarded only where its hooks allow (a project whose guard lets only its own
landing move its main takes that branch through its landing); its board, moved to the schema of
the pin its main locks; and the wait for the shared server's own reload, since the machine's
server is built from a project's checkout. A Symphony change that breaks that project's build or
tests is found by its own gate when it takes the branch, not by the release, and is the project's
to meet: a step its own gate, hooks or main checkout stops is the project's to finish, never red
here (below). That holds until the release is installed on the machine
(`tools/machine-runs-the-release`); from then the refresh installs the release and moves each
board, and moves no project's pin, makes no branch in a project and fast-forwards no main: a
project moves its tags through its own landing, when it chooses.

A step moves what it can and nothing more:

- **Its pin.** While the project's default branch locks an older Symphony, one the release
  descends from, the move to the release is made in a worktree of the project's repository at
  `../symphony-wt/release-refresh/ID`, detached at that branch's tip and locked (`the Symphony
  release run's refresh`), never in its checkout. Each manifest of its workspace that takes the
  Symphony's git source names the release's tag (`tag = "release-…"` in place of `branch`, `tag`
  or `rev`), and `cargo update --workspace` moves the lock. Only `Cargo.toml` and `Cargo.lock`
  files may change, and each Symphony package must end at the release's commit. That is committed
  through the project's own hooks, its commit gate, never past them, on the branch
  `symphony/RELEASE` of its repository, never pushed. A gate that refuses it makes no branch, and
  its last lines are kept in the step's log. The worktree is removed either way. A branch made
  before is kept while it is a fast-forward of the default branch, and made again on its tip once
  that moved past it, when it is the refresh's own one commit; one that holds more is left as it
  is.

  A pin moves only forward, to a release that descends from it. A lock at the release or past it
  takes it already. One beside it, neither ahead nor behind, such as a pin moved by hand to a
  commit of main that a release off main's first-parent line lacks, is left as it is, its files
  and its board with it: no branch, no commit, no snapshot and no migrate. The step is green and
  names both commits, the pin's and the release's, and the run's record and `--status` repeat it
  (`consumer: its main's lock pins the Symphony at P, neither ahead of release-… (R) nor behind
  it: …`). The next release that descends from that pin moves it.

  A package renamed here is a move the refresh does not make: it rewrites each line's tag and not
  the package the line names, so `cargo update` finds no package of the old name at the new tag,
  and the step is red for that project until its own manifest names the new one. The first
  release that holds the library as `symphony-app` ([the packages](#packages-on-cratesio)) is
  one: the consuming project's workspace manifest changes its
  one line for the library from

  ```toml
  symphony = { git = "https://github.com/heavenstudio/symphony", tag = "release-…" }
  ```

  to

  ```toml
  symphony = { package = "symphony-app", git = "https://github.com/heavenstudio/symphony", tag = "release-…" }
  ```

  in the commit that moves its pin, landed through its own landing as any move its gate refuses,
  and nothing else changes: the key stays `symphony`, and so does the library's name, so no
  member's manifest and no `use` moves. It names no Chrome driver, the other package renamed.
- **Its main.** Then the project's default branch is fast-forwarded to that exact commit, by its
  full hash and never by a name a moved branch or a tag of the same name could answer, in its main
  checkout (`git merge --ff-only COMMIT`, its hooks running). That happens when and only when all
  of these hold:
  - the commit is the move and nothing more, read as git holds it after the project's hooks ran,
    which may have rewritten or staged more, and the same for a branch kept from before, which its
    owner may have added to: one commit whose only parent is the default branch's tip, changing
    only `Cargo.toml` and `Cargo.lock` files, every Symphony package its lock takes at the release;
  - the main checkout is on its default branch at that tip and has no tracked edits;
  - its hooks allow it, asked first by a ref transaction that is prepared and then aborted
    (`git update-ref --stdin`: `start`, `update`, `prepare`, `abort`), so a refusal they can tell
    writes nothing into its checkout, whose `Cargo.lock` the shared server's reload may watch.

  It is never pushed (the project's own next push carries it), no hook is passed by, and no merge
  commit is made there. Otherwise the branch is left and its main stays where it was. When git
  refuses the move itself after the ask passed, it has already written the move's files into the
  checkout: the run reads where the default branch is, never assuming it, and puts
  each of those files back to the default branch's tip by itself (`git restore --source=TIP
  --staged --worktree -- PATH`) when and only when its index entry and its file still hold exactly
  what the move wrote, so nothing of its owner's, staged or not, is touched. One that holds
  anything else is left and named, with the command that puts that one path back (`git -C ROOT
  restore --staged --worktree -- PATH`). One that holds what the move wrote and that the run could
  not put back, or could not read, is named with the command that puts it back (`git -C ROOT
  restore --source=TIP --staged --worktree -- PATH`), and the step is red.
- **Its board.** In every refresh but one that leaves its pin beside the release or whose commit
  gate refused the move, the project's live board is moved to the schema of the pin its main locks
  then: the release once its main took it, or an older pin whose schema it is still behind, so a
  refresh finishes what an earlier one left, whatever release is newest. The store is snapshotted
  first, as it runs (`symphony/tasks/snapshots/before-schema-N-RELEASE.surql` in the project's git
  common dir, the newest 5 kept; `surreal export` has 5 minutes). Then `just tasks migrate` runs in
  its main checkout, the one command that moves a project's board, and the schema is read again. A
  board whose database is not running is refused, since nothing here starts, signals or stops a
  database. So is one running an older SurrealDB than the pin names, which the move would replace,
  and one whose versions cannot be read, the pin's or the database's: the guard fails closed. A
  project with no board folder has nothing to move.
- **The shared server.** Last, the run says which server answers on the machine's record, what it
  is built from and where that checkout's main stands. Nothing here stops, signals or restarts it.
  Its own reload (`apps/symphony-server/src/reload.rs`) fingerprints the checkout it serves, its
  `Cargo.lock` and sources: once that changes it builds and installs a new server
  (`crates/symphony/src/reload.rs`, `cargo install --force --root` into its install folder) and
  hands its sockets to it by exec. A build or a hand-over that fails leaves the previous one
  serving and says on its notice why, and when it tries that version again: 5 minutes after its
  first failure, the wait doubling with each failure of it in a row up to an hour, so a hand-over
  that failed under load is tried again though the sources do not change again. A build that
  installs the very bytes the serving one runs, with no client to
  publish, hands over to nothing: the serving server stamps the install as that build's and keeps
  serving. When this refresh moved that checkout's main and a server built from it
  answered before the move, the run waits, 20 minutes at most, until another process answers for
  it, and says which; or that none has yet, and that `just symphony-status` says which answers
  later. With none answering for it before, it waits for none: which build answers after cannot
  be told against one before it, and it says so. The wait is said, never red.

Each step ends in one of three:

- **Moved**: all its project could take, it took. Green.
- **The project's to finish**: its own commit gate refused the move (its hook made `git commit`
  exit 1; no branch made, the gate's last lines kept in the step's log); or its hooks refused the
  fast-forward with its main still where it was read, or its main checkout
  is off its default branch or holds tracked edits (the branch `symphony/RELEASE` left for its own
  landing or its owner, its main as it was, its board moved only to the schema of the pin that main
  locks). What stopped it is the project's own, a Symphony change its gate refuses among it, met
  through its own landing as any change it takes is. Green here and filed nowhere: it files no task
  and closes none, and the run's exit is not changed by it. The step says which, with what finishes
  it, in the run's output (`release: refresh: ID: the project's to finish: …`, then `release:
  refresh-ID the project's to finish`), the run's record (the step named in `refreshed.theirs`, its
  line in `refreshed.said`) and `--status`.
- **Red**: the refresh's own work failed. The registry or the release's packages cannot be read, a
  worktree cannot be made, a lock cannot be moved to the release, the move's commit fails otherwise
  than by its gate's exit 1 (git's own failure, exit 128, or a commit that could not run), a branch
  is found changed under it (more than the move, or its main moved on while the move was made or
  asked for), where its main checkout stands cannot be read, a file the move wrote into that
  checkout cannot be put back, a snapshot or a migrate fails, or a board's database is not running
  or could be replaced by the move. A refusal met once the run's time was up or it was told to stop
  is red, as the run's own, and so is a move its project stopped whose board then fails to move.

A red step says what it found, the release stands (its tag is pushed), and the run exits 1. A red
before its main moved leaves that main as it was, with the branch made and its board moved only to
the pin that main already locks; a migrate that fails leaves the snapshot taken before it. It is
filed as `tests/release-red-refresh-ID` (`tests/release-red-refresh` when the registry cannot be
read), saying what was left as it was and who fixes it, and filed as a check's red is. A refresh
green for it closes the task.

What stays by hand, said in `--status` under a refresh with a step that is its project's to
finish:

- a project whose own gate refused the move: it meets that itself, on its own board;
- a project whose hooks kept the move from its main: it lands `symphony/RELEASE` through its own
  landing;
- a main checkout off its default branch or with tracked edits: its owner puts it right;
- then the retry, `just release --refresh`: an orchestrator's act, the owner having authorised the
  refresh itself, as it is after a red step's fix. It runs the refresh again for the newest release
  here, as a run of its own under the lock, checking nothing, and finishes what the step did not:
  the move, the fast-forward, the board's move and the wait for the server.

A refresh has 2 hours past the run's checks. `[releases] refresh = false` turns it off. Each
project's step logs every command it ran (`logs/ID/refresh-ID.log`).

## Who runs it

The release run is the one thing that pushes a tag to `origin`: the release that run cut, and
nothing else. An orchestrator pushes main and never a tag ([AGENTS](../AGENTS.md)). A release
cannot be taken back once a project fetches it, so running `just release` or
`just release --commit` by hand and installing its schedule are the owner's call: an orchestrator
does either only on the owner's word. The cadence's run has that word (*The cadence*, above): a
land starts `--due` without asking, and the release it runs is automatic. So has a green release's
refresh (*The refresh*, above), so its retry, `just release --refresh`, is an orchestrator's act,
run without asking on a red refresh's task once it is fixed, or once a project finished what its
step left to it. Turning `[releases] publish` on is the owner's word, the first publication's
([the packages](#packages-on-cratesio)); once it is on, `just release --refresh` after a failed
upload's cause is put right is an orchestrator's act as well.
Running `--due` by hand starts a release, and stays the owner's call. The release candidate's two
retries are an orchestrator's acts as well: `just release --rerun`, once every red of it is
answered and no landing has started its rerun (a red answered outside this repository, say), and
`just release --drop`, for a candidate its fixes cannot reach, after which the next landing runs
main's tip.

## Status

`just release --status` prints the newest release, the last run (its result, when, how long ago,
what was red and the tasks filed), a later out-of-cycle release when there is one, the last
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
last publication on crates.io with each line it said ([the packages](#packages-on-cratesio)), and
the last refresh, with each line it said and each step red or its project's to finish:

```text
landings: 7 on origin/main since release-2026-11-12 (91b39f209e72); the last tools/g (23e3da01bc80)
next run: after 13 more landings (7 of 20), or the scheduled run
last publish: of release-2026-11-12 by the run 2026-11-12T1000-34446, 2026-11-12 10:00 (1 h ago)
  not published: [releases] publish is off
last refresh: green, of release-2026-11-12 by the run 2026-11-12T1000-34446, 2026-11-12 10:00 (1 h ago)
  consumer: its pin to release-2026-11-12 is on the branch symphony/release-2026-11-12 (11bbf043d060), made through its own commit gate and not pushed
  consumer: its main is fast-forwarded to symphony/release-2026-11-12 (11bbf043d060) in its main checkout /work/consumer, through its own hooks and not pushed: its own next push carries it
  consumer: its board moved from schema 16 to 17, its store snapshotted first to /work/consumer/.git/symphony/tasks/snapshots/before-schema-17-release-2026-11-12.surql
  the shared server (port 8765) is built from /work/consumer, whose main this refresh moved to release-2026-11-12: its own reload (apps/symphony-server/src/reload.rs) built it again, and process 51234 answers in place of 50111
```

It names each red step and its log, and each step that is its project's to finish (`refresh-ID
the project's to finish, filed nowhere here; log …`), counted on its first line (`last refresh:
green, 1 step left to its project, of …`), then, under one of those, what stays by hand (*The
refresh*, above). The next run is `due` once the count is reached, or the release
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
since, and what is left of the old is this (`crates/symphony/src/release.rs`, `release/record.rs`,
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
writes nothing to a consuming project's checkout. Its refresh writes to a project only the branch
its pin moves on, made through the
project's own commit gate, the worktree that makes it, removed after, the fast-forward of its main
to that branch's one commit in a main checkout ready for it, through its own hooks, putting back
only what that move wrote when git refuses it, and the snapshot of its store; it moves the
project's board only by the project's own `just tasks migrate`. It never
pushes a project's branch, makes a merge commit in a project, bypasses a project's hook, or
starts, signals, stops or restarts a board's database or a server; past its time it stops only
the `surreal export` client it started. Its tests run on a repository of their own with a bare
repository for its remote and a user git configuration that asks every push for more, a scratch
board and a machine folder of their own, with stand-in checks (`crates/symphony/tests/release/`),
and shorten the refresh's wait for the shared server with `SYMPHONY_RELEASE_RELOAD_SECONDS`,
which no flag sets. They set its clock with `SYMPHONY_RELEASE_NOW`, which no flag sets either: a
run on it is refused unless it pushes nothing (`--no-push`) or every place its remote pushes to
is a folder on this machine, so a release named for another day than the machine's never leaves
it.
