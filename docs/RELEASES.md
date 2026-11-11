# Releases

How a Symphony release is cut, named and read. Projects that use the Symphony follow its release
tags, never main. The owner, 2026-11-11: "B. There will be other projects using it. New releases
can be pushed for urgent bugfixes, but generally should be generated based on nightly builds that
would do a full verification cycle and conciliation." The evidence and the update command that
takes a release are in [the analysis](analysis/symphony-update.md).

## A release

A release is an annotated tag on a commit of `origin/main` that every check found green:

- **Its name** is `release-YYYY-MM-DD`, the day's date on the machine that cut it, and
  `release-YYYY-MM-DD.N` for that day's Nth release, N from 2. Releases sort by day, then by
  number, so `.10` follows `.9` (`crates/symphony/src/nightly/tags.rs`).
- **Its message** begins `NAME: Symphony main at COMMIT`, then a `Board schema N` line and a
  `Pin N` line, each saying whether it changed since the release before (`Board schema 17,
  changed since release-2026-11-12.2 (16 then).`, `Pin 1, unchanged since …`), then the checks it
  passed and the commits on main's first-parent line since that release. The first release says
  so instead. The schema is `VERSION` in `crates/tasks/src/schema.rs` and the pin `PIN` in
  `crates/symphony/src/machine.rs`, read at the tagged commit.
- **Reading one:** `git ls-remote --tags origin 'release-*'` lists them, and
  `git tag -l --format='%(contents)' NAME` prints a message. A project names the tag in its
  `Cargo.toml` (`tag = "release-…"` in place of `branch = "main"`) and its `Cargo.lock` binds the
  commit.

## The nightly

`just nightly` (`symphony nightly`, `crates/symphony/src/nightly.rs`) cuts the releases, and is the
one place the full verification runs ([CHECKS](CHECKS.md#the-nightly)):

1. **The candidate.** It fetches `origin/main`, the commit a project can fetch, and takes its tip.
   A tip that is a release already is not checked again.
2. **A clean checkout.** It checks the candidate out at `../symphony-wt/nightly` (`nightly` in the
   `[agents] worktrees` folder of `symphony.toml`), never in the main checkout: a detached
   worktree, locked so the board's worktree sweep leaves it. Each run resets it to the candidate
   and removes every untracked and ignored file but `.target/`, so the build stays incremental.
3. **The full verification** runs there: `just verify`, which is lint, the real client check and
   every test target of the workspace, the board's `perf` and `child_guard` among them. Each check
   goes through the checkout's `tools/record.sh` as one test operation of the board's recorder,
   labelled `nightly NAME` ([MACHINE_WORK](MACHINE_WORK.md#recording-builds-and-tests)), and
   writes its output to a log of its own.
4. **Conciliation**, as read until the owner says otherwise (the question is on the board task
   `tools/release-nightly`): each project in the machine's registry (`projects.json`) whose
   `symphony.toml` names plugins, other than this repository, is built and tested with its
   Symphony moved to the candidate. That runs in a scratch copy at
   `../symphony-wt/nightly-conciliate/ID`, a clone sharing the project's objects at its default
   branch as committed, never in its checkout or on its board. Every package of the candidate's
   workspace is patched in over each git source the project takes one from
   (`cargo --config 'patch."URL".NAME.path=…'`, so no file of the project changes), and
   `cargo test --no-fail-fast` runs its packages that depend on one, under its own toolchain pin,
   as the check `conciliate-ID`. A registered project whose checkout is gone is named in the
   run's output and record.
5. **Red:** no tag. Each red check is a task on this repository's board,
   `tests/nightly-red-NAME`, filed at p0 and marked red, so it takes no orchestrator slot
   ([TASKS](TASKS.md#slots)). It says the command, how it ended, its log and the log's last lines,
   and the commits since the last green run (`git log --oneline LAST_GREEN..TIP`). A check red
   again while its task is open adds a line to that task; the run that finds it green closes the
   task (`cancelled`, saying the check passed). The next red of a closed task's check is
   `tests/nightly-red-NAME-2`, then `-3`.
6. **Green:** it tags the candidate `release-<today>` as above and pushes that tag alone
   (`git push origin refs/tags/NAME`); `--no-push` keeps it here.

Each run is recorded in the git common dir: `.git/sf-symphony/nightly/runs/ID.json` (the commit,
each check with its exit and log, the result, the tag, what it did on the board) and each check's
log under `logs/ID/`. The newest 60 records and the logs of the newest 14 runs stay. One run at a
time: a second is refused while `run.lock` is held. Its exit is 0 green (or nothing new), 1 red or
unfinished, 2 a command line refused.

## Out of cycle

`just nightly --release COMMIT` releases an urgent fix without waiting for the night: the same
checks and conciliation on COMMIT, then the same tag. It refuses a commit that is not on
`origin/main` and one released already, and tags nothing when a check is red (filing its red as a
night does).

## Status

`just nightly --status` prints the newest release, the last night (its result, when, how long ago,
what was red and the tasks filed), a later out-of-cycle release when there is one, and the last
green run. A night older than 36 hours is marked overdue. It reads the record and this checkout's
tags, fetches nothing and writes nothing.

## The schedule

Scheduling is the owner's, and nothing installs it but the owner. `just nightly --schedule` prints
the entry; `--at HH:MM` sets its time (03:00 when unsaid). On macOS it is a launchd agent,
`symphony.nightly.ID`, which runs `just nightly` in the main checkout once a day, or on waking when
the Mac slept through the hour, with the `PATH` of the shell that printed it, and appends what it
prints to `.git/sf-symphony/nightly/scheduled.log`. Elsewhere it is a crontab line.

```sh
just nightly --schedule > ~/Library/LaunchAgents/symphony.nightly.symphony.plist
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/symphony.nightly.symphony.plist
launchctl bootout gui/$(id -u)/symphony.nightly.symphony   # to remove it
```

Until it is installed, nothing runs `just verify` on main.

## Never

The nightly never tags a commit that is not on `origin/main` or that a check found red, never
moves or deletes a tag, pushes nothing but its new tag, and never writes to a consuming project's
checkout or board. Its tests run on a repository of their own with a bare repository for its
remote, a scratch board and a machine folder of their own, with stand-in checks
(`crates/symphony/tests/nightly/`).
