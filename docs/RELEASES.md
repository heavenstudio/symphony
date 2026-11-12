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

1. **The candidate.** It fetches `origin/main` (`--remote` and `--branch` name another remote and
   main line), the commit a project can fetch, and takes its tip. A tip that is a release already
   is not checked again. A tip that does not descend from the newest release is neither checked
   nor tagged: a release only moves forward, so a main that went back stops the run.
2. **Its own checkout.** It checks the candidate out at `../symphony-wt/nightly` (`nightly` in the
   `[agents] worktrees` folder of `symphony.toml`, or `--worktree PATH`): a detached worktree it
   made itself, locked with its own reason (`the Symphony nightly's checkout`) so the board's
   worktree sweep leaves it, and marked in its git admin folder (`symphony-nightly`, holding the
   checkout's real path). Each run resets it to the candidate and removes every untracked and
   ignored file but `.target/`, so the build stays incremental; a `.target/` built on for 7 days
   is cleared. It resets nothing else. A link in that place, a path that is, holds or lies inside
   the main checkout or another worktree (compared by real path), a worktree it did not lock and
   mark, or any other folder is refused and left as it is, and the run stops, naming it. An
   `index.lock` or `HEAD.lock` that an interrupted git left in the checkout's admin folder is
   removed when `lsof` finds no process holding it; one held stops the run. Before it resets
   anything, any process but the run's own that `lsof` sees working in the checkout or in the
   conciliation copies' folder (below), or holding a file inside one open, stops the run. The run
   names it by its pid and command: it could change those folders under the run's checks. It may
   be anyone's: a shell, a terminal tab, an editor's terminal, a Finder window or a `tail -f` left
   there counts as much as what an earlier check left. So the red `run` it files is the owner's,
   not an executor's, since an executor ends only a process it started (RULES.md
   `delete-by-name`). End the process, or move it out of that folder, and the next run goes. A
   process found is listed again for up to 5 s, and counts only if it is still there, so one on
   its way out is not counted. This look sees a process only while it works in those folders or
   holds a file there open. One that moved elsewhere (a daemon that changed to `/`) and writes
   into the checkout by path is not seen.
3. **The full verification** runs there: `just verify`, which is lint, the real client check and
   every test target of the workspace, the board's `perf` and `child_guard` among them. Each check
   goes through the checkout's `tools/record.sh` as one test operation of the board's recorder,
   labelled `nightly NAME` ([MACHINE_WORK](MACHINE_WORK.md#recording-builds-and-tests)), and
   writes its output to a log of its own. It runs in a process group of its own and inherits no
   descriptor of the run's, its lock included (below); what its group still runs when it ends is
   killed with it. After each check, after each project's conciliation and before it tags, the
   run looks at its checkout again, and after a conciliation at that project's scratch copy too:
   - HEAD must be the candidate (in a copy, the project's commit it was made ready at);
   - `git status --porcelain` must name nothing outside `.target/` (in a copy, nothing but its
     `Cargo.lock`, which the patched build rewrites);
   - no process may be in it, as at the start (step 2). The check's own killed group and the
     board's recorder writing down how the check ended are on their way out, so they are not
     counted.

   Before it tags, nothing has run in the checkout since the last look, so only HEAD and
   `git status` are looked at again. Anything else stops the run, naming what changed and what
   is there. The check that has ended counts for nothing, green or red, since it may have checked
   other bytes, and nothing is tagged. A process found makes the red `run` the owner's, as at the
   start. A change with no process there is an executor's: a check that leaves a file outside the
   ignored folders wants an ignore rule.

   **What the looks do not see.** Each look sees one moment, and only what git does not ignore:
   - **Ignored files.** A file git ignores (`build/`, `artifacts/`, `.target/`) is never named,
     though the checks read such files: `just verify`'s tests serve `build/verified-client/public`
     and run binaries from `.target/`.
   - **A change undone.** A change made and undone between two looks leaves nothing to see.

   So a process that rewrites an ignored file a check reads, or that changes a tracked file and
   puts it back before the check ends, can make a check pass on other bytes, and the commit is
   still released. In practice the run is protected from both in three ways:
   - the checkout is the nightly's own, made, locked and marked by it, outside every other
     checkout, so nothing else works there by design;
   - each check runs in a process group of its own that is ended with it, so nothing it started
     in that group outlives it;
   - a process working in the checkout or holding a file there open, as what a check leaves
     usually does, is found at the next look or at the next run's start.

   What none of that finds is a process that left both the check's group and the checkout's
   folder (a daemon that changed to `/`) and writes into the checkout by path. Only what it does
   to files git names gives it away.
4. **Conciliation**, as read until the owner says otherwise (the question is on the board task
   `tools/release-nightly`): each project in the machine's registry (`projects.json`) whose
   `symphony.toml` names plugins, other than this repository, is built and tested with its
   Symphony moved to the candidate. That runs in a scratch copy at
   `../symphony-wt/nightly-conciliate/ID`, a repository the nightly made (marked
   `symphony-nightly-copy`, borrowing the project's objects) at the project's default branch as
   committed, never in its checkout or on its board. Every package of the candidate's
   workspace is patched in over each git source the project takes one from
   (`cargo --config 'patch."URL".NAME.path=…'`, so no file of the project changes), and
   `cargo test --no-fail-fast` runs its packages that depend on one, under its own toolchain pin,
   as the check `conciliate-ID`, with `SYMPHONY_MACHINE_DIR` naming a folder that holds a copy of
   the registry (in the run's logs), not the machine's own. A folder in a copy's place that the
   nightly did not make, unless empty, is left as it is, and that project's check is red; a copy
   whose project left the registry is removed, and a copy's `.target/` built on for 7 days is
   cleared. A registry that cannot be read is the red check `conciliate`: nothing is conciliated,
   so nothing is released. A registered project whose checkout is gone, or that takes nothing of the
   Symphony's from git, is named in the run's output and record and in the release's message
   (`Left out of conciliation`).
5. **Red:** no tag. Each red check is a task on this repository's board,
   `tests/nightly-red-NAME`, filed at p0 and marked red, so it takes no orchestrator slot
   ([TASKS](TASKS.md#slots)). It says the command, how it ended, its log and the log's last lines,
   and the commits since the last green run (`git log --oneline LAST_GREEN..TIP`). A check red
   again while its task is open adds a line to that task; a run green in it closes the task
   (`cancelled`, saying the check passed) when its candidate holds the commit the check was last
   found red on. The next red of a closed task's check is `tests/nightly-red-NAME-2`, then `-3`.
6. **Stopped short:** a run that ends without deciding (a fetch that fails or does not answer, a
   checkout it cannot take, a git lock held, a process in the run's folders, a checkout or copy
   changed under a check, a main behind the newest release, a push that fails, a run told to
   stop, a lock or a check that outlives its run, below) is red too, as the task
   `tests/nightly-red-run`, filed, updated and closed by the next run that gets through, as a
   check's is. Its body says who fixes it: the owner when a process stopped it, else any free
   executor. A command line or
   `--release`/`--worktree` argument refused (exit 2, or exit 1 naming why) runs nothing and files
   nothing.
7. **Green:** it tags the candidate `release-<today>` as above and pushes that one tag:
   `git push --no-follow-tags --recurse-submodules=no origin refs/tags/NAME:refs/tags/NAME`, an
   explicit refspec, so a user configuration that pushes more (`push.followTags`, a
   `remote.origin.push` list) adds nothing to it. `--no-push` keeps the tag here. A push that
   fails keeps the tag here and is the red `run`; a later night on that same commit fails the same
   way, naming the push that delivers it, and `--status` names each release here that the last run
   found missing from the remote. No run pushes an older tag.

**Its time.** A run has ten hours (`--timeout SECONDS`, at most 24 hours: a killed run's check
holds later runs off for its run's time). A check still running then is stopped (its
group sent `TERM`, then `KILL` after 10 s) and red, as is each check and each project's
conciliation not yet started. A git command stops at the run's time or after 15 minutes, whichever
is sooner. A run whose checks all ended green within its time keeps 5 more minutes to tag and push.
A run told to stop (`HUP`, `INT` or `TERM`, as launchd's stop or a terminal sends them) stops its
check's group the same way, and its git, records why, files the red `run` (the stopped check is no
check's red) and releases the lock. A signal it was started ignoring stays ignored, and `KILL`
stops it where it is (below).

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

Each run is recorded in the git common dir: `.git/symphony/nightly/runs/ID.json` (the commit,
each check with its exit and log, the result, the tag, what it did on the board) and each check's
log under `logs/ID/`. The newest 60 records and the logs of the newest 14 runs stay. Its exit is 0
green (or nothing new), 1 red, stopped short or refused, 2 a command line refused.

## Out of cycle

`just nightly --release COMMIT` releases an urgent fix without waiting for the night: the same
checks and conciliation on COMMIT, then the same tag. It refuses a commit that is not on
`origin/main`, one released already and one that does not descend from the newest release, and
tags nothing when a check is red (filing its red as a night does). A green release closes a red
only when COMMIT holds the commit that red was found on, so an urgent fix from before a red leaves
it open.

## Who runs it

The nightly is the one thing that pushes a tag to `origin`: the release that run cut, and
nothing else. An orchestrator pushes main and never a tag ([AGENTS](../AGENTS.md)). A release
cannot be taken back once a project fetches it, so running `just nightly` or
`just nightly --release` by hand and installing its schedule are the owner's call: an orchestrator
does either only on the owner's word.

## Status

`just nightly --status` prints the newest release, the last night (its result, when, how long ago,
what was red and the tasks filed), a later out-of-cycle release when there is one, the last
interrupted run while its logs are kept, the last green run, and the releases here that the last
run found missing from the remote, with the push that delivers one. A night older than 36 hours
is marked overdue. It reads the record and this checkout's tags, fetches nothing and writes
nothing.

## The schedule

Installing the schedule is the owner's call: nothing installs it, and an orchestrator installs it
only on the owner's word. `just nightly --schedule` prints the entry and makes nothing; `--at HH:MM`
sets its time (03:00 when unsaid). On macOS it is a launchd agent, `symphony.nightly.ID`, which
once a day, or on waking when the Mac slept through the hour, runs one `sh -c` line with the `PATH`
of the shell that printed it: make the record folder, then `just nightly` in the main checkout,
appending what it prints to `.git/symphony/nightly/scheduled.log`. Elsewhere it is a crontab line
running the same.

```sh
just nightly --schedule > ~/Library/LaunchAgents/symphony.nightly.symphony.plist
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/symphony.nightly.symphony.plist
launchctl bootout gui/$(id -u)/symphony.nightly.symphony   # to remove it
```

Until it is installed, nothing runs `just verify` on main.

## Never

The nightly never tags a commit that is not on `origin/main`, that does not descend from the newest
release or that a check found red, never moves or deletes a tag, and pushes nothing but its new
tag. It resets and cleans no checkout but its own, removes no folder it did not make, and itself
writes nothing to a consuming project's checkout or board; what a project's own tests do in its
scratch copy is theirs. Its tests run on a repository of their own with a bare repository for its
remote and a user git configuration that asks every push for more, a scratch board and a machine
folder of their own, with stand-in checks (`crates/symphony/tests/nightly/`). They set its clock
with `SYMPHONY_NIGHTLY_NOW`, which no flag sets: a run on it is refused unless it pushes nothing
(`--no-push`) or every place its remote pushes to is a folder on this machine, so a release named
for another day than the machine's never leaves it.
