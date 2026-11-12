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
- a client change also runs `just client-update` and commits `apps/symphony-server/client/public`
  with its source;
- before it moves to review it posts on its thread the exact commands, each one's result line
  (`test result: ok. 12 passed; 0 failed; …`), and the other evidence its kind of change asks for
  ([below](#evidence-by-kind-of-change)).

No task runs `just verify`, the whole browser target or the whole workspace's tests as its gate,
whatever its brief asks: that is [the nightly](#the-nightly)'s.

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
orchestrator executed itself is landed by a fresh reviewer's approve, as any other task is.

**Higher risk asks for stronger evidence, never for the whole suite.** A change's risk is what a
mistake in it would break and how far that reaches: one view, every agent's board commands, the
wire every client reads. Stronger evidence is more of the tests the change could break (every
test target of the changed package, the browser target still by filter; the targets of the
packages that call a changed public item), the nightly-tier target that covers it, or a
measurement before and after.

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
  ([by page](#the-browser-target-by-page)); and `just client-update`.
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
4. `just lint-rs`, and `just client-update` when `apps/symphony-ui` changed.
5. Before the land, when main gained commits in one of those packages since they ran, the same
   targets again on the merge ([rerunning on the merge](#rerunning-on-the-merge)).

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
| the tasks board (`/tasks`) and its cards | `board` `task_columns` `task_controls` `tasks_and` `owner_` `machine_work` `run_records::` `polish::` `shot::` `projects::` `budget` |
| a task's page | `task_page` `task_panel` `task_priority` `notice_layout::` `polish::` `projects::` |
| the agents page (`/agents`) | `machine_work` `polish::` `projects::` |
| document and folder pages | `document` `docs_index` `restored::` `polish::` `projects::` |
| the header, on every page | `header::` `floating_controls::` `polish::` `projects::` |
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
bytes grew. The thread gets the eight pictures, the commands and their result lines, the updated
tests named, and the commit with the client distribution.

## The nightly

`just verify` is the full verification: lint, the real client check, and every test target of
the workspace, the headless browser acceptance and the board's `perf` and `child_guard` targets
([the board guide](../crates/tasks/AGENTS.md)) among them. It is the nightly's, and no task's.
`just nightly` runs it on main's tip, as fetched from `origin`, in a clean checkout outside this
folder (`../symphony-wt/nightly`), then conciliates the projects this machine serves with that
commit. A green night tags a release; a red one files each red check as a task in the
`tests/nightly-red-*` series at p0 ([TASKS](TASKS.md#slots)), naming the commits since the last
green run (`git log --oneline LAST_GREEN..TIP`), and the next green run closes it; a run that
stops short of deciding is the red `tests/nightly-red-run`. How it runs,
what it records and how it is scheduled: [RELEASES](RELEASES.md#the-nightly).

**Until the owner installs its schedule, nothing runs `just verify` on main.** A break that no
task's focused tests reach stays unseen until then: the gap is the price of focused gates without
their nightly, and closes when the nightly runs each night.

## Checks with no reviewer

`symphony.toml`'s `[verdicts] check` is what a generated branch (the owner's verdicts with the
outputs a generator makes from them) must pass before it lands with no reviewer
(`crates/symphony/src/project.rs`, `Verdicts`). It is `just symphony-cli docs-audit --check`:
such a branch changes documents and what is generated from them, and the docs audit is the check
closest to them. This project names no verdict generator, so no generated branch runs it yet; a
generator added later adds its outputs' own tests to it.

`just land` runs the same check on every merge it lands, reviewed or not, in a worktree of its
own before main moves ([the default landing](TASKS.md#the-default-landing)), after its
`[land] generators`. This project's one regenerates the client distribution: `just
client-check` builds the client fresh and compares it with the committed one, and only when they
differ does `just client-update` rebuild it, committed after the merge. A branch that committed
its own distribution with its client change therefore lands with no second commit.

## Rerunning on the merge

A branch's evidence ran at its tip, and `just land` lands the merge of that tip with main as
main stands then. When main gained commits since the tip was tested that touch a package the
branch changed too, that package's closest targets, the ones the branch's evidence ran, run
again on the merge before main moves: two branches each green alone can be red together, as a
branch that adds a `symphony` subcommand and one that adds a test listing the subcommands are.
The reviewer's last look runs them when main moved so during its review; else the lander does,
before `just land`. Which packages both sides changed is mechanical, with BASE the commit
`git merge-base main TIP` names:

```
git diff --name-only BASE TIP     (the branch's files)
git diff --name-only BASE main    (main's, since)
```

Each file is named by its package as in [step 2](#picking-the-focused-set): a package on both
lists is rerun, and with none, nothing is.

**Where it runs: by hand.** The land's check on the merge is `[verdicts] check`, which in this
project is the docs audit alone (`symphony.toml`), so the land reruns no test target. The lander
or the reviewer merges the tip into main's tip in a scratch worktree of its own
(`git worktree add --detach DIR main`, then `git -C DIR merge --no-edit TIP`), runs the targets
there and removes the worktree; a red goes back to `@executor` as a red check on the land's
merge does ([the default landing](TASKS.md#the-default-landing)). Whether the land's check runs
the changed packages' fast targets itself is the owner's to decide, on a card in the
[register](DECISIONS.md).
