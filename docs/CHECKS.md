# Checks

What a task in this repository proves before review, who judges it, and where the full
verification runs. A consuming project keeps its own gates
([command ownership](../AGENTS.md#command-ownership)); the rule is RULES.md
`workflow/evidence-is-the-gate`.

The owner, 2026-11-11: "Do they need to run a full test suite? Why? If tests are so time
consuming why are they being required to run every time, instead of running tests more closely
associated with the tasks at hand, leaving a nighly run for a full verify?" And on what a task's
gate is for: "So the executors need to 'prove that the work they did work'. That is the concept
of the stamp based gating. As long as evidence is good enough, a full verification can be left to
be done later".

## The gate is evidence

Every task, parent or slice, feature or fix, proves that what it changed works:

- it runs the tests closest to its change ([picking them](#picking-the-focused-set)) and
  `just lint-rs`;
- a client change also builds the client for its tests and pictures (`just client-update`, which
  a server of the branch's own then serves) and commits no distribution: the land keeps main's
  `apps/symphony-server/client/public`, and the regenerator a land starts rebuilds it from main
  ([checks with no reviewer](#checks-with-no-reviewer)), so `git checkout --
  apps/symphony-server/client/public` puts main's back before the commit;
- before it moves to review it posts on its thread the exact commands, each one's result line
  (`test result: ok. 12 passed; 0 failed; …`), and the other evidence its kind of change asks for
  ([below](#evidence-by-kind-of-change)).

No task runs `just verify`, the whole browser target or the whole workspace's tests as its gate,
whatever its brief asks: that is [the release run](#the-release-run)'s.

A reviewer runs the same focused tests itself, never the suite, reads the diff against the
evidence and judges whether it is good enough for the change's risk. Evidence that falls short is
a `changes` verdict naming what to add. The reviewer's approve is the task's stamp: the board
verdict (`just tasks verdict SLUG approve "FULL_SHA: why"`, naming the tip it read, RULES.md
`workflow/reviewers-not-the-owner`), which the task [[task:tools/review-as-stamps]] records as a
stamp on the task it reviews. A slice's evidence is judged in its parent's review. A fix whose
recorded risk calls for no reviewer (RULES.md `workflow/fast-bugfixes`), filed as one with `just
tasks create SLUG TITLE --fast-fix WHY`, is landed on its posted evidence by its orchestrator,
which did not execute it: its approve, naming the tip, is the verdict the default land goes by on
such a task and on no other ([the default landing](TASKS.md#the-default-landing)). A fast fix its
orchestrator executed itself is landed by a fresh reviewer's approve, as any other task is, and so
is a red's fast fix that changes a test or a check (`[land] check_paths`): the land refuses its
orchestrator's approve on that, since a loosened test or check is how a red turns green unfixed.

**Higher risk asks for stronger evidence, never for the whole suite.** A change's risk is what a
mistake in it would break and how far that reaches: one view, every agent's board commands, the
wire every client reads. Stronger evidence is more of the tests the change could break (every
test target of the changed package, the browser target still by filter; the targets of the
packages that call a changed public item), a target only the release run runs that covers it, or
a measurement before and after.

## Evidence by kind of change

- **A pure function** (a parser, a formatter, a state transition): unit tests of its edge cases
  beside it, and their run (`just test-rs -p PACKAGE --lib MODULE`).
- **A CLI behaviour** (a `tasks` or `symphony` subcommand, its output, its refusals and exit
  code): a test of the command, on a scratch board or a temporary folder, for the success and
  each refusal, in the target that tests that command: the board's `cli` target
  (`just test-rs -p symphony-board --test cli COMMAND`), with the store tests under it when the
  store changed; for a `symphony` subcommand, the `symphony` target named for its area (`asks`,
  `edit`, `picks`, `register`, `render`, `tree`). A change that adds, renames or retires a
  `symphony` subcommand, or changes a test that lists them, also runs the two targets that list
  every command the binary has: `just test-rs -p symphony --test serve` and
  `just test-rs -p symphony --lib cli::commands`.
- **A server route** (a handler, its status, headers and body, or where its answer comes from,
  such as the hub's last board read): the route's tests in the server
  targets (`-p symphony-server --test after_http`, `-p symphony --test serve`), the wire target
  (`-p symphony-api --test wire`) when a payload's shape changed, and the route's answer, status
  and body, read with `curl` from a server of the task's own on a scratch board
  (`TASKS_DATABASE=NAME just symphony --port 0`,
  [the server guide](../apps/symphony-server/README.md)), so a branch's build never opens the
  live board.
- **A client page.** The owner, 2026-11-11: "for UI based work for example, this might entail
  screenshots of before and after, as well as some new added tests or updated tests that would
  likely break due to changes". So: before and after screenshots of each changed view, dark and
  light where colour changed, desktop and phone width where layout changed
  (`just symphony-cli shot URL --out FILE.png`, with `--phone` and `--scheme dark`;
  `just symphony-cli shot --help` says the rest); the new tests for the changed behaviour (host
  views in `-p symphony-ui --test view`, the page in `--test browser`); the existing tests the
  change was likely to break, updated where they pinned what changed, and green
  ([by page](#the-browser-target-by-page)); and `just client-update` for the pictures, its
  distribution not committed.
- **A schema or launcher change** (the board's schema and its migration, `tools/rs-bin.sh`,
  `tools/cargo.sh`, the justfile, `symphony init`, the hooks): a mistake here stops every agent,
  so it asks the most: the migration run from the previous schema on a scratch board (the board's
  `store` target and `--lib schema`), the `perf` target for a query or index change
  (`just test-rs -p symphony-board --test perf`), the launcher's and hooks' own targets
  (`-p symphony-board --test launcher`, `-p symphony --test hooks`, `-p symphony --lib init`),
  and the command run in a scratch worktree, never against the live board.
- **A doc** (a document, guide, skill or rule): `just symphony-cli docs-audit --check` (every
  document renders) and `just symphony-cli status --lint` (no card slug in prose, nothing the
  plugins' prose rules refuse) clean; for a shipped skill, `just symphony-cli init --root .` run
  so the installed copies and their stamps are current, and the shipped skills' tests
  (`just test-rs -p symphony --lib init`). It runs no other test.

## Picking the focused set

1. List what the branch changed: `git diff --name-only main...HEAD`.
2. Name each file's package: `crates/tasks` is `symphony-board`, `crates/symphony` is
   `symphony`, `apps/symphony-server` is `symphony-server`, `apps/symphony-ui` is `symphony-ui`,
   and any other `crates/NAME` is `NAME`.
3. For each package:
   - the unit tests of the changed modules: `just test-rs -p PACKAGE --lib MODULE`, the module
     path as a filter (`store::work` for `crates/tasks/src/store/work.rs`);
   - the integration targets that exercise them. A target is a file or folder under the
     package's `tests/` (`--test store` is `crates/tasks/tests/store/`).
     `just test-rs -p PACKAGE --test TARGET -- --list` prints its tests' names; a filter is any
     part of a name or module path (`priority::`, `queue`), and several after `--` run every test
     any of them matches;
   - when a public item's contract changed, the tests of the packages that call it (`git grep`
     its name).
4. `just lint-rs`, and, when `apps/symphony-ui` changed, the client built (`just client-update`)
   for its tests and pictures, its distribution never committed.
5. After merging main, the targets steps 1 to 3 pick run again at the new tip, before review,
   as main may have changed what the branch's code meets ([after main moves](#after-main-moves)).

A package's own run (`-p PACKAGE`) and the workspace's can differ, as Cargo turns on a
dependency's features for every package one build takes: the workspace and `-p symphony-server`
link `serde_json` with `arbitrary_precision`, `float_roundtrip`, `preserve_order` and `raw_value`,
as the server does, and so do the own runs of `symphony-board` and `symphony`, as the `tasks` and
`symphony` binaries do (the board's dependency asks for them), of `symphony-api` and
`symphony-plugin` (a dev-dependency asks for them, which never reaches the client or a plugin's
build) and of `symphony-schema` (`raw_value` aside, which changes no read), while the own runs of
`symphony-ui` and `chrome-devtools` link it with none of them, so a test of theirs that reads a
JSON object's key order or a number's text can pass alone and fail in the workspace's run.

Steps 1 to 3 without the filters are mechanical, and a recipe that runs them from the branch's
diff, `just check-fast`, is filed as the task [[task:tools/check-fast]]. Choosing the filters
stays the agent's, and the reviewer's to judge.

### The browser target by page

`just test-rs -p symphony-ui --test browser -- FILTER…` builds the release client once and
opens each test's page in headless Chrome. Most of its tests are named for the page or control
they open, so a page change runs the names that page's words match. Others draw several pages
each, under names no page's words match, and a change to a page they draw runs them too:
`polish::` (its popups, panels, headings and trouble blocks), `shot::` (the `shot` command's
own tests, which capture the tasks board), `projects::` (every link a page draws, under the
project's base) and `budget` (what an entry page loads). Each row below is the whole filter,
both kinds, to copy after `--`:

| What changed | Filters after `--` |
|---|---|
| the home queue (`/`) | `queue` `polish::` `projects::` `budget` `phone::` |
| the queue page (`/queue`) | `queue` `polish::` `projects::` |
| the tasks board (`/tasks`) and its cards | `board` `task_columns` `task_controls` `tasks_and` `a_revealed_column` `owner_` `machine_work` `run_records::` `polish::` `shot::` `projects::` `budget` |
| a task's page | `task_page` `task_panel` `task_priority` `notice_layout::` `polish::` `projects::` |
| the agents page (`/agents`) | `machine_work` `polish::` `projects::` |
| document and folder pages | `document` `docs_index` `restored::` `polish::` `projects::` |
| the header, on every page | `header::` `floating_controls::` `panel_open::` `polish::` `projects::` |
| shared controls (select, menu, button, tooltip, theme) | `components::` `select` `theme` `polish::` |
| the project switch and a project's look | `projects::` |
| the feed and its stream | `feed` `stream` |
| what an entry page loads (its bytes) | `budget` `phone::` |

## Worked examples

**A board command's refusal.** The branch rewords the refusal `just tasks priority` gives for a
value outside 0 to 4, in `crates/tasks/src/store/work.rs`, and updates the store test that pins
its text.

```
just test-rs -p symphony-board --lib store::work
just test-rs -p symphony-board --test store priority::
just test-rs -p symphony-board --test cli priority::
just lint-rs
```

The thread gets the four commands and their result lines. No `perf`: the statement the store
sends is unchanged, and a change to it would add `--test perf`. No client test: no client file
changed.

**A row on the home queue.** The branch shows each card's priority beside its title in the home
queue's rows, in `apps/symphony-ui/src/queue.rs` and `apps/symphony-ui/assets/queue.css`.

```
just symphony-cli shot URL --out queue-before.png   (desktop and phone, light and dark, before the change)
just symphony-cli shot URL --out queue-after.png    (the same four, after it)
just test-rs -p symphony-ui --test view queue::
just test-rs -p symphony-ui --test browser -- queue polish:: projects:: budget phone::
just client-update
just lint-rs
```

A new view test pins the priority in the row's markup; `queue` runs the browser tests of the
queue's counts, order, spacing and filter colours, which a taller row or new markup could break,
and the branch updates any that pinned the old row, naming it. `polish::` and `projects::` draw
the home page too: its panels and its one heading, and the link each row draws under the
project's base. `budget` and `phone::` are there because the home page is an entry page and its
bytes grew. The thread gets the eight pictures, the commands and their result lines and the
updated tests named; the commit holds the source alone, `just client-update`'s distribution put
back as main has it.

## The release run

`just verify` is the full verification: lint, the real client check, the
[size budgets](#size-budgets), the docs audit, and every test target of the workspace, the
headless browser acceptance and the board's `perf` and `child_guard` targets
([the board guide](../crates/tasks/AGENTS.md)) among them. It is the release run's, and no task's.
`just release` runs it on main's tip, as fetched from `origin`, in a clean checkout outside this
folder (`../symphony-wt/release`), then conciliates the projects this machine serves with that
commit. A green run tags a release; a red one files each red check as a task in the
`tests/release-red-*` series at p0 ([TASKS](TASKS.md#slots)), staffed first and filed as a fast
fix whose orchestrator lands only a fix changing no test or check (RULES.md
`workflow/release-reds-first`), one task for each red step of verify, naming the commands that
rerun each failure alone (each failed test, else the step's own command, a test run's a target at a
time), the load when it ended and the commits since the last green run
(`git log --oneline LAST_GREEN..TIP`), and the next green run closes it; a run that
stops short of deciding is the red `tests/release-red-run`. It is the one place every test runs
on main: `just land` runs none ([after main moves](#after-main-moves)). It may be scheduled more
often than daily, and its schedule is the owner's to install. It also runs every 20 landings:
a land starts `--due` on the main checkout's linked build of `symphony`, which counts with git
alone and builds nothing until that many tasks have landed since the last release or the last run
that checked something; then it builds in the release run's own checkout and runs on main's tip,
never beside a run in progress ([the cadence](RELEASES.md#the-cadence)). How it runs, what it
records and how it is scheduled: [RELEASES](RELEASES.md#the-release-run).

**Every step runs.** `just verify` runs its steps in order, lint, the client check, the size
budgets, the docs audit and the tests (`tools/verify.sh`), each whatever the ones before it did,
and is red when any is, so a red step never hides the ones after it. Each step is recorded as a step
of the verify run ([RUNS](RUNS.md)) and writes its output to a log of its own,
`build/verify/STEP.log` (a folder of the run's when the release run runs it), which `steps.tsv`
beside it lists with each step's exit status, seconds and command and the load average it ended
under (over 1, 5 and 15 minutes, with the processor count). Its own output says each step as it
starts and ends, with the last lines of a red one, and at its end each step with its log, then the
red ones. `just verify STEP...` runs those steps alone, in that order. An interrupt (Ctrl-C) stops
it once the step running ends: it runs none after it, says where it stopped and exits 130. The tests serve the client
the client check built in the same verify (`SYMPHONY_UI_DIST`), and the check's folder
(`build/verified-client`) is cleared before it runs, so a client an earlier check left there is
never served. When the client check is red on a client it built, the tests run against that one;
when it built none, or did not run, the browser tests build the client themselves, red if that
build fails too. The tests' log and the summary say which.

**Until the owner installs its schedule, only the cadence runs `just verify` on main.** A break
that no task's focused tests reach stays unseen until 20 landings have gathered: the gap is the
price of focused gates without their full run, and narrows once it also runs each day. The
first landing with no release and no run on record counts the line's whole history, and starts
the first run at once.

## Size budgets

The owner, 2026-11-12: "We certainly need to have a check for total binary size and improve to
reduce it further." Each binary this workspace ships (`symphony-server`, `symphony`, `tasks` and
`agent-send`) has a budget, and so does the client distribution the server embeds (every file
under `apps/symphony-server/client/public`): one line each in `tools/size-budget.toml`, in bytes.
`just size-check` builds the four as `cargo install` builds them, with `Cargo.toml`'s
`[profile.release]`, and runs `symphony size-budget`, which prints each size beside its budget.
Past a line it fails, naming the size, the budget and how to see what grew: `cargo bloat --release
--crates --bin NAME` (`cargo install cargo-bloat --root DIR`, never a workspace dependency) beside
the same at a commit within it (the last release, or before the first one, the commit that last
set the budget file), or the largest files of the distribution. A binary's line is its file; the
server's is its file less the client it embeds, which has its line of its own, so each line grows
for one reason. The sizes are this machine's (aarch64-apple-darwin), where the release run runs.

The shipped binaries are the ones `symphony init` links for a project (`BUILDS`,
`crates/symphony/src/init.rs`), and a test holds the recipe's build to them: a binary of them
with no line, or a line for any other, is an error, not a pass. So is a server built before the
client files beside it last changed (a hand run after `just client-update`), whose own bytes the
check cannot tell; it says to rebuild.

A binary links what it may run, and a test holds a library out of the two that never run it
(`tests/links.rs` of `symphony` and of `symphony-server`): the loopback that measures a compiled
plugin's calls through the protocol's JSON (installed with the plugins,
`symphony::plugins::install`, so a binary that installs none links none of it; a plugin answering
from its process needs only the host's side of the protocol).

The release profile leaves out the symbol table (`strip = true`). A panic in a shipped binary
still prints its thread, file:line:col and message, but a backtrace names nothing:
`RUST_BACKTRACE=1` prints no frames and `RUST_BACKTRACE=full` raw addresses. A build to debug or
profile sets `CARGO_PROFILE_RELEASE_STRIP=debuginfo`, which keeps the names.

It is a step of `just verify` (`size-check`), so [the release run](#the-release-run) runs it on
main's tip; no task's gate does, as it costs a release build of the whole workspace. A budget is
set a few percent above what its line measured when it was set; it is raised only in the commit
that needs it, saying why, and lowered in the commit of a reduction.

The release profile is this workspace's: a consuming project that builds its own host or board
binaries in a workspace of its own builds them with its own `[profile.release]`, and gets what
this one sets only by setting it there.

## Checks with no reviewer

`symphony.toml`'s `[verdicts] check` is what a generated branch (the owner's verdicts with the
outputs a generator makes from them) must pass before it lands with no reviewer
(`crates/symphony/src/project.rs`, `Verdicts`). It is `just symphony-cli docs-audit --check`:
such a branch changes documents and what is generated from them, and the docs audit is the check
closest to them. This project names no verdict generator, so no generated branch runs it yet; a
generator added later adds its outputs' own tests to it.

`just land` runs that check on the merge it lands unless `[land] check = false`, which this
project sets: a land here is its merge, its move and its push, in seconds
([the default landing](TASKS.md#the-default-landing)). The docs audit is evidence at a
document change's tip ([by kind](#evidence-by-kind-of-change)); once a land moves main, the
regenerator it starts runs it on what its round left main holding (its own commit once landed,
with whatever main gained during the build), and a red there is one task with one owner,
`tests/main-red-check` at p0, closed by the round it passes on
([TASKS](TASKS.md#the-regenerator)). The owner's verdicts, whose commit runs the same check on
main, are refused from that land on; the regenerator shortens the time to finding out why, to
within a round of the land that brought it. The audit is part of the release run's `just verify`
too.

A land runs none of its `[land] generators` either: the regenerator it starts makes them of
main's newest tip after it ([TASKS](TASKS.md#the-regenerator)). This project's one is the
client distribution. Its key (`tools/client-key.sh`, the release snapshot's name) skips a tip
whose client inputs and committed distribution passed before; otherwise `just client-check`
builds the client fresh and compares it with the committed one, and only when they differ does
`just client-update` rebuild it and `just client-check` compare again, the change landed on main
as one commit. A branch commits no distribution: the land keeps main's side of every
distribution file a branch changed, in a conflict or not, its approved tip unchanged; a conflict
in any other file still goes back to its executor. Between a land and the regenerator's commit
main's committed client is behind its sources: `just tasks regenerate --status` says how far,
and the release run waits for it and releases nothing of a main still behind.

## After main moves

A branch's evidence is of the commit it ran at, and main moves on meanwhile: what main gained can
break the branch's change, or be broken by it, in a test neither side's evidence ran. A branch
that adds a `symphony` subcommand and merges main's new test that lists the subcommands is red at
its own tip, in a 0.2 s test it never ran.

**After merging main, before review: the executor's and the reviewer's rule.** After a branch
merges main, the closest targets of the packages the branch itself changed run again on the
branch, at the new tip, before review, as main may have changed what they meet. They are the
targets [steps 1 to 3](#picking-the-focused-set) pick at the new tip, where `main...HEAD` still
lists the branch's own files alone; a package only main's commits touched is main's evidence, not
the branch's. The evidence posted names the new tip. The reviewer approves only a tip the
evidence names, and a branch that merges main during its review runs them again first.

**At the land: nothing automatic, and nothing that blocks.** A land's only check on the merge is
`[verdicts] check`, which this project turns off (`[land] check = false`, `symphony.toml`), for
the regenerator to run on main's tip after it ([checks with no reviewer](#checks-with-no-reviewer)),
and it runs no test and builds nothing: a test or a build in the land would have to run again
whenever main moved before the land finished, so lands would wait on each other. An orchestrator may run by hand, before it lands a task, what it
judges the land needs, such as the closest targets of a package both the branch and main changed
since the branch last merged it, in a scratch worktree of its own
(`git worktree add --detach DIR main`, then `git -C DIR merge --no-edit TIP`); no other land
waits on it, and a red goes back to `@executor`
([the default landing](TASKS.md#the-default-landing)).

**What evidence missed, the full run finds.** [The release run](#the-release-run) runs every target
on main's tip every 20 landings, and each day or more often once the owner schedules it, and
publishes a release when every check is green; each red it finds is one task, with one owner.
