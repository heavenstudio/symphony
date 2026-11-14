# Checks

What a task in this repository proves before review, who judges it, and where the full
verification runs. A consuming project keeps its own gates
([command ownership](../AGENTS.md#command-ownership)); the rule is RULES.md
`workflow/evidence-is-the-gate`.

A task does not run the whole suite: it proves that the work it did works, with the tests closest
to it, and the evidence good enough for its risk is its stamp. The full verification is left to
the release run, on main.

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
  every command the binary has: `just test-rs -p symphony-app --test serve` and
  `just test-rs -p symphony-app --lib cli::commands`.
- **A server route** (a handler, its status, headers and body, or where its answer comes from,
  such as the hub's last board read): the route's tests in the server
  targets (`-p symphony-server --test after_http`, `-p symphony-app --test serve`), the wire target
  (`-p symphony-api --test wire`) when a payload's shape changed, and the route's answer, status
  and body, read with `curl` from a server of the task's own on a scratch board
  (`TASKS_DATABASE=NAME just symphony --port 0`,
  [the server guide](../apps/symphony-server/README.md)), so a branch's build never opens the
  live board.
- **A client page.** Before and after screenshots of each changed view, dark and
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
  (`-p symphony-board --test launcher`, `-p symphony-app --test hooks`, `-p symphony-app --lib init`),
  and the command run in a scratch worktree, never against the live board.
- **A package's publication** (a member's `publish`, version, crates.io metadata, `include` list
  or a published package's dependency on a sibling): `just test-rs -p symphony-app --test packages`
  and `cargo publish --dry-run` naming every published package together, from the committed tip
  (no `--allow-dirty`), its output on the thread ([RELEASES](RELEASES.md#packages-on-cratesio));
  never a publication. The packages test is also the closest one for a change to a file a package
  carries a copy of (the client's `board.css` and `frame.css`, `tools/fixture_process.sh`): it
  fails until the copy is made again, naming the `cp`.
- **Weight added to a shipped binary** (a dependency or a dependency's feature, a library the
  binary did not reach before): `just size-check` at the tip, its lines on the thread, every binary within its
  line ([a red at the size check](#a-red-at-the-size-check)).
- **A doc** (a document, guide, skill or rule): `just symphony-cli docs-audit --check` (every
  document renders, and every recipe a text of this repository names is one a project has,
  [below](#the-recipes-a-text-names)) and `just symphony-cli status --lint` (no card slug in prose,
  nothing the plugins' prose rules refuse) clean, both run at the branch's final tip, after its
  last edit: a run before an edit is evidence of the text before it. For a shipped skill,
  `just symphony-cli init --root .` run so the installed copies and their stamps are current, and
  the shipped skills' tests (`just test-rs -p symphony-app --lib init`). It runs no other test.

### The recipes a text names

A `just NAME` in what this repository says is a recipe `symphony init` installs
(`crates/symphony/shipped/symphony.just`), the consumer's landing, named as the project's own, a
word of prose, in one of its guides a recipe of its justfile, or, in a text about this repository,
one of its own recipes or one filed to be made that the lists name for that file: a freshly
initialised project has no other. The lists are `tools/named-recipes.txt`, this repository's data,
a line to a list (`consumers NAME...`, `prose WORD...`, `own FILE NAME...`, `filed FILE NAME
TASK`), which one reader (`crates/symphony/src/docs_audit/recipes.rs`) reads from the checkout, so
no binary carries them. The word after `just` is read past the options that leave a recipe to run
(a justfile or folder named, the quiet, verbose, dry-run and yes switches), across one line break
but never a blank line. One rule, two places:

- the docs audit, in the Symphony's own checkout alone (its project file names it `symphony`, and
  it holds the lists), judges its documents, the Markdown under `docs/` but the decision register
  (`docs/DECISIONS.md`), `docs/analysis/` and `docs/history/`, which are records; and, of the files
  git's index holds (so a new one once added), every file of what it delivers, under
  `crates/symphony/shipped/` (the skills, the Codex rules and the recipes), and its guides, each
  `AGENTS.md`, `README.md` and `RULES.md` at the root or in a package under `crates/` or `apps/`.
  A consuming project's texts name its own recipes, and its audit judges none of them, the
  Symphony's crate vendored in it or not;
- the hooks test (`named_recipes`) judges what the Symphony delivers too, and the strings the
  binaries print, which no docs change touches: a change to such a string runs it
  (`just test-rs -p symphony-app --test hooks named_recipes`).

Each name either refuses is an error naming its file and line. Each entry of the lists is kept
current, still named in its file and a filed one beside its task (`[[task:SLUG]]` on the line that
names it), by the one place that judges its file: the audit for the texts above, the test for every
other. Neither judges the records, the tests, `tools/`, or the installed copies of the shipped
skills under `.agents/skills/`, which `symphony init` keeps the same as what it delivers.

So a text that names one is red at its own tip, and should it land, the regenerator's run of the
audit on main after the land's round files it within minutes
([checks with no reviewer](#checks-with-no-reviewer)), not the next release.

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
as the server does, and so do the own runs of `symphony-board` and `symphony-app`, as the `tasks` and
`symphony` binaries do (the board's dependency asks for them), of `symphony-api`,
`symphony-blocks` and `symphony-plugin` (a dev-dependency asks for them, which never reaches the
client or a plugin's build) and of `symphony-schema` (`raw_value` aside, which changes no read),
while the own runs of `symphony-ui` and `symphony-chrome` link it with none of them, so a test of
theirs that reads a JSON object's key order or a number's text can pass alone and fail in the
workspace's run.

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
| the reports page (`/reports`) | `reports::` `header::` `polish::` `projects::` `budget` |
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
folder (`../symphony-wt/release`), and judges the commit by that alone: it builds and tests nothing
of the projects this machine serves, whose own gates meet a change when they take a release. A
green run tags a release; a red one files each red check as a task in the
`tests/release-red-*` series at p0 ([TASKS](TASKS.md#slots)), staffed first and filed as a fast
fix whose orchestrator lands only a fix changing no test or check (RULES.md
`workflow/release-reds-first`), one task for each red step of verify, naming the commands that
rerun each failure alone (each failed test, else the step's own command, a test run's a target at a
time), the load when it ended and the commits since the last green run
(`git log --oneline LAST_GREEN..TIP`), and the next green run closes it; a run that
stops short of deciding is the red `tests/release-red-run`. A test red on load is answered by its
discard ([time in tests](#time-in-tests)). A run red in a check holds its commit
as the release candidate while its reds' own tasks fix it on the branch `release-candidate`, then
reruns what was red and what the fixes changed, and releases that commit and its fixes alone
([RELEASES](RELEASES.md#the-release-candidate)). It is the one place every test runs
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

## Time in tests

A test passes or fails on what the code did, never on how long a loaded machine took to let it
(RULES.md `tests/no-test-fails-on-load`). The machine the release run runs on also runs every
agent's builds, at load averages of 50 to 700, so a test reads time in one of three ways, through
the workspace's one helper, `crates/test-time` (a development dependency only):

- **A cost** is the CPU the work took, or a count of the work, never its wall.
  `test_time::cpu::spent(|| work)` bills a closure the CPU of its thread; `thread()` and `process()`
  read this thread's and this process's CPU; `of(pid)` reads a live process's
  (`proc_pid_rusage` on macOS, `/proc/PID/stat` on Linux) and `reap(pid)` what a child took in all
  at its reap. The board's `perf` target bills its server this way (`crates/tasks/tests/perf/cpu.rs`).
- **A wait** looks at its condition until it holds:
  `test_time::until(WHAT, test_time::backstop(ALLOWANCE), || condition)`, or `until_true` for a
  condition that holds or does not. Its only time is a wall-clock backstop, six times the wait's
  allowance and never under two minutes. The Chrome driver's own waits keep the same rule
  (`crates/chrome-devtools`). Past it, the wait fails with a `Stall`: `WHAT: stalled: still not so
  after N looks, and the wait's wall-clock backstop of B ran out after W; load average a b c over 1,
  5 and 15 minutes`. A loop of `sleep`s with a count, or an `Instant` deadline asserted, is a wait
  written by hand: rewrite it. A sleep followed by a check that something did not happen proves
  nothing on a loaded machine, where the sleep may end before the thing would have. It waits on a
  positive barrier instead.
- **Real time** stays only where the product's promise is elapsed time: a deadline, a backoff, a
  page's two seconds on a phone. Its clock is injected where it can be: an environment variable,
  a setting the test passes, or a page clock the test drives. A test that cannot inject it stays
  and runs in `just verify` until the owner says otherwise.

**Who takes a test out of the release run: the owner, and no one else.** An executor who finds a test
that none of the three ways can hold proposes it on its task's thread: the promise the test guards
and why no CPU bound, count, wait or injected clock stands in for it. The orchestrator puts it to
the owner, and the test leaves `just verify` only on the owner's word. A test the owner lets go on
such a proposal is not dropped but moved: to a recipe a quiet machine runs, which still runs the
test. No such recipe exists yet, so nothing is `#[ignore]`d for a quiet machine; a test the owner
lets go so waits in the release run until that recipe lands, and is then listed below with its
promise and the owner's word.

**A test red on load in the release run is discarded.** A rerun neither fails nor passes on the
machine's load, so a test that fails a release run, or the release candidate's rerun, because of
load (it passes alone, or its failure is a wall-clock bound, a sleep-ordered step or a timeout
under load) is answered by its red's own fix removing it from the tree, with one line on the red's
thread naming the test, the evidence it is load (its runs alone and the failing line) and what it
proved. This rule is the owner's approval: no card is put to them to discard it. If what it
proved matters, a follow-up task may bring it back written on a condition, a count, CPU time or an
injected clock; the release does not wait for that. No rerun is spent on it, no rewrite of it is waited for, and
nobody lowers the machine's load to help a run pass: an orchestrator holds no other work back to
keep the load down for a run. A test that fails alone on what its code did, or fails the same way
without load, is not this case and is fixed in the code.

No budget, allowance or tolerance is widened to make a loaded run pass. Evidence for a test
rewritten under this rule is the test run alone five times on the loaded machine, the load average
stated with each result, and once more under a load of the agent's own (a few busy loops in its own
process group, started and stopped by their PIDs).

**Out of the release run for a quiet machine:** none. Proposed and waiting for the owner's word:
`phone::the_board_is_interactive_within_two_seconds_on_a_mid_phone` (the board interactive within
2 s on a mid phone, which a loaded machine's wall cannot stand in for); it runs in the release run
until then.

## Size budgets

What Symphony ships is checked for size, so it never grows unseen and can be brought down. Each
binary this workspace delivers (`symphony-server`, `symphony`, `tasks` and
`agent-send`) has a budget, and so does the client distribution the server embeds (every file
under `apps/symphony-server/client/public`): one line each in `tools/size-budget.toml`, in bytes.
`just size-check` builds the four as `cargo install` builds them, with `Cargo.toml`'s
`[profile.release]`, and runs `symphony-build size-budget`, which prints each size beside its
budget (`symphony-build`, the commands only this repository's build and release run use, is no
binary it delivers, and has no line).
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
from its process needs only the host's side of the protocol). The same test of `symphony` holds
the JSON Schema validator out of it, which would add a fifth to its size: a schema check is a
library call a consumer's code makes (`symphony::schema::validate`), and `symphony serve` answers
`validate` as a function it lacks.

On macOS every binary of the three packages links with `-no_exported_symbols` (each package's
`build.rs`), and the same `tests/links.rs`, with the board's, holds each shipped binary to it. A
link writes by default an export trie, a name for each global symbol, for code that loads the
binary to find them by, and no code loads these: the trie the flag leaves out is about 360 KB of
the server and 330 KB of `symphony` and of `tasks`. Two dependencies build in one codegen unit
(`Cargo.toml`), comrak and regex-syntax, whose large constant tables each unit that reads one
would otherwise hold a copy of: about 200 KB of the server and of `symphony`, and 100 KB of
`tasks`.

The release profile leaves out the symbol table (`strip = true`), and with no export trie either,
nothing in a shipped binary names a function. A panic still prints its thread, file:line:col and
message, but `RUST_BACKTRACE=1` prints no frames and `RUST_BACKTRACE=full` raw addresses, and
lldb and atos (so `sample`, a crash report and Instruments) name no frame. A build to debug or
profile sets `CARGO_PROFILE_RELEASE_STRIP=debuginfo`, which keeps every name.

The release profile, the two codegen units and the link flag are this workspace's: a consuming
project that builds its own host or board binaries in a workspace of its own gets them only by
setting them there. That is `strip = true` under `[profile.release]` and a
`[profile.release.package.NAME]` with `codegen-units = 1` for comrak and for regex-syntax in its
workspace's `Cargo.toml`, and, on macOS, `cargo:rustc-link-arg-bins=-Wl,-no_exported_symbols`
printed by its host package's own `build.rs`, as Cargo passes a link argument only to the bins of
the package that prints it, never to a dependent's.

It is a step of `just verify` (`size-check`), so [the release run](#the-release-run) runs it on
main's tip. No task's gate runs it, as it costs a release build of the whole workspace, except a
task whose change adds weight to a shipped binary ([a red at the size check](#a-red-at-the-size-check)).

### Where a line stands

A binary's line is its size, as `just size-check` measures it in the commit that sets the line,
plus 4 %, rounded down to a multiple of 10,000 bytes. The commit of a reduction sets the lines: it
lowers the line of each binary it made smaller to that, and leaves a line where it stood when the
rule would put it higher. The 4 % is the room the landings after a reduction share until the next
one. Within its line, `symphony-build size-budget` prints where the rule brings down a binary's
line that stands higher (`its line comes down to …`), so the commit of a reduction takes that
number, not a choice of its own. A line is raised only as [a red at the size check](#a-red-at-the-size-check)
allows, by the same rule: to the binary's size plus 4 %, never higher. The client's line is set by
the same rule from the distribution main holds when it is set, and the check prints no such number
for it: what the distribution holds rises and falls with the builds `publish-client` keeps.

### A red at the size check

A binary past its line is a red of the landing that took it there, not of the release that finds
it. A task whose change adds weight to a shipped binary (a dependency or a dependency's feature, a
library the binary did not reach before) runs `just size-check` at its tip and posts the sizes with
its evidence. A binary past its line there is the task's own to take back before review, in its
branch, by a reduction first: bytes of its own code, or of anything else that binary links. A
raise of its line is the exception RULES.md `workflow/release-reds-first` names for a budget: in a
commit of its own that names the growth, with the task's thread saying why no reduction will do, to
the binary's size plus 4 % and never higher, landed only on a fresh reviewer's approve
([the default landing](TASKS.md#the-default-landing)).

The landings that each stayed under a line can still cross it together, and the release run finds
that: a red of its size step, fixed the same way, a reduction first. To find what grew, the red's
task reads a link map of the binary at the last green tip and at the red one, by crate and by
module: every live symbol with its size and the object it came from. After `just size-check`,
touch the binary's own source file and run the recipe's build again, its packages and `--bin
NAME`, with a `RUSTC_WRAPPER` that adds `-C link-arg=-Wl,-map,FILE` to that binary's link alone:
Cargo relinks that binary and nothing else, its bytes as they were. Built with fewer packages
(`-p symphony-app` alone, or `cargo rustc` of one), the dependencies take other features and the
binary is another one. `cargo bloat` needs an unstripped rebuild of everything.

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
and the release run waits for it and releases nothing of a main still behind. The one exception
is a release candidate's fix that changes what the client is built from: no regenerator builds
the candidate, so it commits the distribution its own sources make (`just client-update`), which
its rerun's client check proves, and the land still keeps main's side of it
([RELEASES](RELEASES.md#the-release-candidate)).

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

**A release candidate's fix merges `release-candidate`, never main, before its approve.** A red's
own task while the release run holds a candidate ([RELEASES](RELEASES.md#the-release-candidate))
starts at the local branch `release-candidate` and merges it again before review, as that branch
moves when another red's fix lands; the same rule then holds with `release-candidate` in place of
main: its closest targets run again at the new tip, and the evidence names it. Its evidence is
the red check's failures run alone at its tip, the commands its task names. Main is merged only
when its land conflicts, after an approve, and then the fresh approve the merged tip needs leaves
the candidate on the commit approved before the merge. One that merges main before any approve
gives the candidate nothing it can take, and drops it.

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
