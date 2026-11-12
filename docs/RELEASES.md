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
   When the candidate is the main checkout's main too, the run asks the regenerator a land
   starts where main's generated files stand (`just tasks regenerate --status`,
   [TASKS](TASKS.md#the-regenerator)): while it makes them the run waits, within its time, and
   once they are current it goes on; red or stopped on the candidate, it is refused at once,
   recorded nowhere and filing nothing, since a main whose committed client is behind its
   sources is no release and the regenerator's own red is the task. A land starts the
   regenerator just before the `[board] on_land` that starts the cadence's night, so what the
   regenerator last wrote may be of an earlier tip, or nothing at all: with none making them
   and nothing written of the candidate, the run gives it a minute to take the candidate before
   it is refused the same way. Main moving on in the main checkout while the run waits (the
   regenerator landed what it made, or another land came in) is followed: `origin/main` is
   fetched again and what it moved to is the candidate from then on, waited for in its turn; a
   move that has not reached `origin` within that minute refuses the run the same way, as the
   regenerator was last seen not to have made the candidate's files. A
   project with no generators waits for nothing, and so does a candidate the main checkout does
   not hold as its main, whose own client check says. An out-of-cycle release (below) does not
   ask.
2. **Its own checkout.** It checks the candidate out at `../symphony-wt/nightly` (`nightly` in the
   `[agents] worktrees` folder of `symphony.toml`, or `--worktree PATH`): a detached worktree it
   made itself, locked with its own reason (`the Symphony nightly's checkout`) so the board's
   worktree sweep leaves it (its `.target/` the sweep clears only once nothing touched the
   checkout for 7 days), and marked in its git admin folder (`symphony-nightly`, holding the
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
3. **The full verification** runs there: `just verify`, which is lint, the real client check, the
   size budgets ([CHECKS](CHECKS.md#size-budgets)), the docs audit and every test target of the
   workspace, the board's `perf` and `child_guard` among them. Each check goes through the
   checkout's `tools/record.sh` as one test operation of the board's recorder, labelled
   `nightly NAME` ([MACHINE_WORK](MACHINE_WORK.md#recording-builds-and-tests)), and writes its
   output to a log of its own. It runs in a process group of its own and inherits no descriptor of
   the run's, its lock included (below); what its group still runs when it ends is killed with it.
   After each check, after each project's conciliation and before it tags, the run looks at its
   checkout again, and after a conciliation at that project's scratch copy too:
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
   `tests/nightly-red-NAME`, filed at p0 and marked red, so it takes no orchestrator slot and is
   offered before other work of its priority ([TASKS](TASKS.md#slots)). It is staffed before
   anything else (RULES.md `workflow/nightly-reds-first`), fixed in the code and never by a
   loosened test or check, and filed as a fast fix whose recorded risk is the bound the land holds
   a red's fix to (`symphony_board::fast_fix::RED`): its orchestrator's approve lands a fix that
   changes none of the project's tests and checks (`[land] check_paths`), the failing tests
   unchanged and green at its tip, and a fix that changes a test, a check, a budget or a run step
   lands only on a fresh reviewer's approve ([the default landing](TASKS.md#the-default-landing)).
   Its text says:
   - the command and how it ended;
   - its log, and the load average when it ended (over 1, 5 and 15 minutes, with the processor
     count, kept in the run's record);
   - the log's last lines;
   - the commands that rerun each failure alone, as the log names them
     (`crates/symphony/src/nightly/rerun.rs`). Each test target cargo says failed is rerun as
     `just test-rs ARGS -- NAME --exact` for each test libtest named `FAILED` in it, or whole when
     it named none, at most 12 lines. A conciliation's are its own command in its scratch copy. A
     verify log naming no failed test names the step that failed, the last recipe its log says
     failed that is one of verify's steps, rerun by its own command: `just lint-rs`, `just client-check`, `just
     size-check` or `just symphony-cli docs-audit --check`. One naming neither lists those steps
     and its tests' rerun, never `just verify`, which no task runs; any other check's names the
     check itself;
   - the commits since the last green run (`git log --oneline LAST_GREEN..TIP`).

   A check red again while its task is open adds a line to that task, with that night's load and
   reruns; a run green in it closes the task (`cancelled`, saying the check passed) when its
   candidate holds the commit the check was last found red on. The next red of a closed task's
   check is `tests/nightly-red-NAME-2`, then `-3`. A fix that lands starts no night of its own:
   the next is the cadence's or the schedule's.
6. **Stopped short:** a run that ends without deciding (a fetch that fails or does not answer, a
   checkout it cannot take, a git lock held, a process in the run's folders, a checkout or copy
   changed under a check, a main behind the newest release, a push that fails, a run told to
   stop, a lock or a check that outlives its run, below) is red too, as the task
   `tests/nightly-red-run`, filed, updated and closed by the next run that gets through, as a
   check's is, and filed the same way, but with no fast-fix mark when a process stopped it, since
   nothing lands for that. Its body says who fixes it: the owner when a process stopped it, else
   any free executor, then how a check's red is staffed and approved. A command
   line or
   `--release`/`--worktree` argument refused (exit 2, or exit 1 naming why) runs nothing and files
   nothing.
7. **Green:** it tags the candidate `release-<today>` as above and pushes that one tag:
   `git push --no-follow-tags --recurse-submodules=no origin refs/tags/NAME:refs/tags/NAME`, an
   explicit refspec, so a user configuration that pushes more (`push.followTags`, a
   `remote.origin.push` list) adds nothing to it. `--no-push` keeps the tag here. A push that
   fails keeps the tag here and is the red `run`; a later night on that same commit fails the same
   way, naming the push that delivers it, and `--status` names each release here that the last run
   found missing from the remote. No run pushes an older tag. A pushed release then refreshes the
   machine ([below](#the-refresh)).

**Its time.** A run has ten hours (`--timeout SECONDS`, at most 24 hours: a killed run's check
holds later runs off for its run's time). A check still running then is stopped (its
group sent `TERM`, then `KILL` after 10 s) and red, as is each check and each project's
conciliation not yet started. A git command stops at the run's time or after 15 minutes, whichever
is sooner. A run whose checks all ended green within its time keeps 5 more minutes to tag and push.
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

## The cadence

The owner, 2026-11-12: "Given the pace, It would be nice to release a new version every given
number of tasks done, say 20 and refresh server/clients. Random number I know." So a night also
runs once enough tasks have landed, whatever the hour (`crates/symphony/src/nightly/cadence.rs`):

- **The count** is `[releases] every_landings` in `symphony.toml`, 20 unless the project says. `0`
  turns the cadence off, and then only the night's run or `just nightly` releases.
- **A landing** is a merge on the main line's first-parent line whose subject names a branch:
  "Merge branch 'SLUG'", the merge `just land` makes, or "Merge branch 'SLUG' into BRANCH", git's
  own made by hand. A merge of the default branch itself ("Merge branch 'main' into SLUG", a task
  catching up, which reached the line), of another repository's branch or of a remote-tracking
  branch is none, and neither is any other commit.
- **Counted since** the newest release on that line, or a later run that checked something on it,
  green or red, whichever is nearer the tip. A run that stopped short before its first check (a
  fetch that failed, a process in its folders) is not counted from. So a red tip is counted from
  as a release is: the landings after it count afresh, and the next landing does not start the
  same red run again.
- **The first count.** With no release and no run on record, the count is from the line's start,
  so the first landing after the cadence lands takes the line's whole history and starts a night
  at once: the owner wants a release. It says so, in the land's output as in `--status`: "the
  first count, N landings from origin/main's start with no release and no run on record: a night
  on its tip starts now".
- **The line** is `origin/main` (`--remote`, `--branch`) as this checkout last fetched or pushed
  it, which a land's push moves; with no such ref, the local `main`. The count reads that ref, the
  release tags here and the run records with git and the files alone. It fetches, builds and
  writes nothing.
- **The trigger.** A land runs `[board] on_land` once the task is closed and pushed, after it
  started the regenerator ([TASKS](TASKS.md#the-default-landing)), which the night it starts
  waits for (*The candidate*, above), and shows what it said within its first seconds. This
  repository names there the build of `symphony` its main checkout links,
  `.target/bin/symphony nightly --due`, and the land appends the task's slug and its merge commit.
  Nothing slow sits on or behind a land: short of the count, `--due` prints the count and what the
  next run waits for and exits 0, having run git alone, no cargo and no recipe, so no `just tasks`
  or `just land` waits on a build lock of its. Once the count is reached it takes the lock, brings
  the nightly's own checkout (`../symphony-wt/nightly`) to `origin/main`'s tip, builds `symphony`
  there with `tools/rs-bin.sh --build symphony` in that checkout's own build folder, never the main
  checkout's, and becomes that build, which runs a night exactly as `just nightly` does: the full
  verification, the conciliation, the tag and its push, each red filed, then the refresh. A tip
  that cannot be built so is said, and its night runs from the linked build, which meets the same
  red in its checks. The record names what started it (`cause`: `20 landings since release-…, the
  last SLUG`). A red run files its reds, as a night does, and holds no later count back. A linked
  build older than the cadence knows no `--due`: the land shows its refusal, and
  `tools/rs-bin.sh --link symphony` in the main checkout renews the link.
- **One run at a time.** While a run holds the lock (a night's, one run by hand, an out-of-cycle
  release's, a refresh's or another landing's, its build included), `--due` starts nothing and
  says the count waits for it. The landings it saw meanwhile are not lost: once that run has
  checked a commit, the count starts again from it, and the next landing that brings it to the
  count starts the next run. Between `--due`'s look and the run's start, a run that took the lock
  first refuses the second, which is not doubled either (*One run at a time*, above).

## The refresh

Asked whether a green release should refresh the machine, the owner, 2026-11-12: "yes, a green
release should refresh". The clients are the projects the machine serves, Star Foundry among them.
Once a release's tag is pushed, the run refreshes the machine to it: one step `refresh-ID` for each
project the conciliation covers (step 4 above: registered, its `symphony.toml` naming plugins, this
repository aside) whose packages take one of the Symphony's from git
(`crates/symphony/src/nightly/refresh.rs`). A step moves what it can and nothing more:

- **Its pin.** While the project's default branch locks an older Symphony, the move to the
  release is made in a worktree of the project's repository at `../symphony-wt/nightly-refresh/ID`,
  detached at that branch's tip and locked (`the Symphony nightly's refresh`), never in its
  checkout. Each manifest of its workspace that takes the Symphony's git source names the release's
  tag (`tag = "release-…"` in place of `branch`, `tag` or `rev`), and `cargo update --workspace`
  moves the lock. Only `Cargo.toml` and `Cargo.lock` files may change, and each Symphony package
  must end at the release's commit. That is committed through the project's own hooks, its commit
  gate, never past them, on the branch `symphony/RELEASE` of its repository, never pushed. A red
  gate makes no branch. The worktree is removed either way. A branch made before is kept while it
  is a fast-forward of the default branch, and made again on its tip once that moved past it, when
  it is the refresh's own one commit; one that holds more is left as it is.
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
  commit is made there. Otherwise the branch is left, the step is red and its main stays where it
  was. When git refuses the move itself after the ask passed, it has already written the move's
  files into the checkout: the run reads where the default branch is, never assuming it, and puts
  each of those files back to the default branch's tip by itself (`git restore --source=TIP
  --staged --worktree -- PATH`) when and only when its index entry and its file still hold exactly
  what the move wrote, so nothing of its owner's, staged or not, is touched. One that holds
  anything else is left and named, with the command that puts that one path back (`git -C ROOT
  restore --staged --worktree -- PATH`).
- **Its board.** In every refresh, the project's live board is moved to the schema of the pin its
  main locks then: the release once its main took it, or an older pin whose schema it is still
  behind, so a refresh finishes what an earlier one left, whatever release is newest. The store is
  snapshotted first, as it runs (`sf-tasks/snapshots/before-schema-N-RELEASE.surql` in the
  project's git common dir, the newest 5 kept; `surreal export` has 5 minutes). Then `just tasks
  migrate` runs in its main checkout, the one command that moves a project's board, and the schema
  is read again. A board whose database is not running is refused, since nothing here starts,
  signals or stops a database. So is one running an older SurrealDB than the pin names, which the
  move would replace, and one whose versions cannot be read, the pin's or the database's: the guard
  fails closed. A project with no board folder has nothing to move.
- **The shared server.** Last, the run says which server answers on the machine's record, what it
  is built from and where that checkout's main stands. Nothing here stops, signals or restarts it.
  Its own reload (`apps/symphony-server/src/reload.rs`) fingerprints the checkout it serves, its
  `Cargo.lock` and sources: once that changes it builds and installs a new server
  (`crates/symphony/src/reload.rs`, `cargo install --force --root` into its install folder) and
  hands its sockets to it by exec, a build that fails leaving the previous one serving and saying
  why on its notice. When this refresh moved that checkout's main and a server built from it
  answered before the move, the run waits, 20 minutes at most, until another process answers for
  it, and says which; or that none has yet, and that `just symphony-status` says which answers
  later. With none answering for it before, it waits for none: which build answers after cannot
  be told against one before it, and it says so.

A step that is red says what it found, the release stands (its tag is pushed), and the run exits
1. A red before its main moved leaves that main as it was, with the
branch made and its board moved only to the pin that main already locks; a migrate that fails
leaves the snapshot taken before it. It is filed as
`tests/nightly-red-refresh-ID` (`tests/nightly-red-refresh` when the registry cannot be read),
saying what was left as it was and who fixes it, and filed as a fast fix as a check's red is: the
bound holds for a fix of the Symphony's landed on it, while a red in a consuming project's own
gate, hooks or code is fixed by a task on that project's board. A refresh green for it closes the
task.

What stays by hand, said under a red refresh in `--status` too:

- a project whose own gate or hooks refuse the move (Star Foundry's guard lets only its own
  landing move its main): it lands `symphony/RELEASE` through its own landing;
- a main checkout off its default branch or with tracked edits: its owner puts it right;
- then the retry, `just nightly --refresh`: an orchestrator's act, the owner having authorised the
  refresh itself. It runs the refresh again for the newest release here, as a run of its own under
  the lock, checking nothing, and finishes what the red step did not: the fast-forward, the board's
  move and the wait for the server.

A refresh has 2 hours past the run's checks. `[releases] refresh = false` turns it off. Each
project's step logs every command it ran (`logs/ID/refresh-ID.log`).

## Who runs it

The nightly is the one thing that pushes a tag to `origin`: the release that run cut, and
nothing else. An orchestrator pushes main and never a tag ([AGENTS](../AGENTS.md)). A release
cannot be taken back once a project fetches it, so running `just nightly` or
`just nightly --release` by hand and installing its schedule are the owner's call: an orchestrator
does either only on the owner's word. The cadence's run is that word, given on 2026-11-12 (*The
cadence*, above): a land starts `--due` without asking, and the release it runs is automatic. A
green release's refresh is that word too ("yes, a green release should refresh"), so its retry,
`just nightly --refresh`, is an orchestrator's act, run on a red refresh's task without asking.
Running `--due` by hand starts a release, and stays the owner's call.

## Status

`just nightly --status` prints the newest release, the last night (its result, when, how long ago,
what was red and the tasks filed), a later out-of-cycle release when there is one, the last
interrupted run while its logs are kept, the last green run, and the releases here that the last
run found missing from the remote, with the push that delivers one, and, in a project with
`[land] generators`, where the regenerator stands (`regenerator: …`). A night older than 36 hours
is marked overdue. Then it prints the cadence, the landings since what it counts from and what the
next run waits for, and the last refresh, with each line it said and each red step:

```text
landings: 7 on origin/main since release-2026-11-12 (91b39f209e72); the last tools/g (23e3da01bc80)
next run: after 13 more landings (7 of 20), or the night's run
last refresh: green, of release-2026-11-12 by the run 2026-11-12T1000-34446, 2026-11-12 10:00 (1 h ago)
  consumer: its pin to release-2026-11-12 is on the branch symphony/release-2026-11-12 (11bbf043d060), made through its own commit gate and not pushed
  consumer: its main is fast-forwarded to symphony/release-2026-11-12 (11bbf043d060) in its main checkout /work/consumer, through its own hooks and not pushed: its own next push carries it
  consumer: its board moved from schema 16 to 17, its store snapshotted first to /work/consumer/.git/sf-tasks/snapshots/before-schema-17-release-2026-11-12.surql
  the shared server (port 8765) is built from /work/consumer, whose main this refresh moved to release-2026-11-12: its own reload (apps/symphony-server/src/reload.rs) built it again, and process 51234 answers in place of 50111
```

Under a red refresh it names each red step and its log, then what stays by hand (*The refresh*,
above). The next run is `due` once the count is reached, `in progress` while a run holds the
lock, or `off` when `every_landings` is 0. It reads the record and this checkout's tags and refs,
fetches nothing and writes nothing.

## The schedule

Installing the schedule is the owner's call: nothing installs it, and an orchestrator installs it
only on the owner's word. The full run may be scheduled more often than nightly; the entry
`just nightly --schedule` prints runs once a day, and a schedule of more times than one is the
owner's to install as well: a run does not check again a tip released already, and one started
while another runs is refused (*One run at a time*, above). `just nightly --schedule` prints the
entry and makes nothing; `--at HH:MM` sets its time (03:00 when unsaid). On macOS it is a launchd
agent, `symphony.nightly.ID`, which once a day, or on waking when the Mac slept through the hour, runs one `sh -c` line with the `PATH`
of the shell that printed it: make the record folder, then `just nightly` in the main checkout,
appending what it prints to `.git/symphony/nightly/scheduled.log`. Elsewhere it is a crontab line
running the same. That line runs `just nightly` in the foreground, never as a background job, so
the run itself starts with every signal at its default too.

```sh
just nightly --schedule > ~/Library/LaunchAgents/symphony.nightly.symphony.plist
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/symphony.nightly.symphony.plist
launchctl bootout gui/$(id -u)/symphony.nightly.symphony   # to remove it
```

Until it is installed, only the cadence runs `just verify` on main.

## Never

The nightly never tags a commit that is not on `origin/main`, that does not descend from the newest
release or that a check found red, never moves or deletes a tag, and pushes nothing but its new
tag. It resets and cleans no checkout but its own, removes no folder it did not make, and itself
writes nothing to a consuming project's checkout; what a project's own tests do in its scratch
copy is theirs. Its refresh writes to a project only the branch its pin moves on, made through the
project's own commit gate, the worktree that makes it, removed after, the fast-forward of its main
to that branch's one commit in a main checkout ready for it, through its own hooks, putting back
only what that move wrote when git refuses it, and the snapshot of its store; it moves the
project's board only by the project's own `just tasks migrate`. It never
pushes a project's branch, makes a merge commit in a project, bypasses a project's hook, or
starts, signals, stops or restarts a board's database or a server; past its time it stops only
the `surreal export` client it started. Its tests run on a repository of their own with a bare
repository for its remote and a user git configuration that asks every push for more, a scratch
board and a machine folder of their own, with stand-in checks (`crates/symphony/tests/nightly/`),
and shorten the refresh's wait for the shared server with `SYMPHONY_NIGHTLY_RELOAD_SECONDS`,
which no flag sets. They set its clock with `SYMPHONY_NIGHTLY_NOW`, which no flag sets either: a
run on it is refused unless it pushes nothing (`--no-push`) or every place its remote pushes to
is a folder on this machine, so a release named for another day than the machine's never leaves
it.
