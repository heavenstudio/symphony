# Reports and monitoring

What a project's runs, task pages and reports would show, where each figure comes from, and the
evidence behind the three reports cards in [DECISIONS](../DECISIONS.md). Each fact about the code
names the file and function it was read in. "Read 2026-11-11" marks the state of this machine
that day. Star Foundry's files were only read; nothing there was changed.

## The ask

The owner, 2026-11-11, on the board task `product/reports-and-monitoring`:

- "One entire thing missing from star foundry right now, is a report / monitoring bucket. Things
  like that cumulative report view, information on build costs etc." Then: "Oh, sorry, not
  star-foundry, I mean from symphony". It is a Symphony feature, for every project it serves.
- "considering how symphony now behaves like CI/CD, then info on the CI/CD aspects is what I mean
  is missing." The runs come first; pipeline flow, agents and board health second.
- "As well as general pipeline and defect pipeline information. We don't have stamp info in the UI
  for tasks, no 'graph' of the steps that tasks did and how much time each took, etc".

Three cards, each approvable alone: [Runs](#runs), [The task page](#the-task-page) and
[Pipeline and defects](#pipeline-and-defects).

## What exists today

- **Where a task's time went.** `tasks show` and `tasks stats` replay the thread's move, claim and
  verdict lines (`crates/tasks/src/stages.rs`, `stages`; `stages/stats.rs`, `summarize`), read in
  one request (`store/stage_lines.rs`, `Store::stage_records`). The replay walks every change
  (`Replay::take`) but keeps only a total and a visit count per status, so the order of the
  stages is lost. The task's wire record carries no stages (`crates/symphony-api/src/lib.rs`,
  `Task`), and the task page shows none.
- **Build and test now.** The machine panel and the Build and Test columns read native operation
  records (`crates/tasks/src/cli/machine_work/native.rs`, `Inventory::decode`) from Star Foundry's
  `sf-slots` reader, sampled once per board tick (`apps/symphony-server/src/live/machine_work.rs`).
  A finished operation retires the live one and is not kept: the projection holds ownership in
  memory only (`projection.rs`, `Projection::new`), and [MACHINE_WORK](../MACHINE_WORK.md) adds
  "no ... board persistence". This repository has no recorder: `tools/cargo.sh` execs Cargo and
  writes nothing, so no Symphony build or test is ever observed. `tools/build-test-recorded-generic`
  (in progress) adds the recorder and keeps finished operations.
- **Review callbacks.** `[board] on_review` starts detached and appends its output to
  `sf-tasks/on-review.log` (`crates/tasks/src/on_review.rs`, `trigger`, `spawn`); its exit and
  duration are not kept.
- **Board health.** Minute buckets over one hour, in the server's memory
  (`crates/tasks/src/board_health.rs`, `Passes`, `Queries`, `WINDOW`); the one server rewrites
  `sf-tasks/health.json` every ten seconds (`Recorder::persist`). No hour is kept after it passes.
  Read 2026-11-11: neither project on this machine has a `health.json` yet; the running server
  predates the record.
- **Gates.** Symphony's own: the reviewer's verdict lines on the thread, and `[verdicts] check`
  (`crates/symphony/src/project.rs`, `Verdicts`), which today runs only on a generated branch.
  Star Foundry's stamps: its landing answers `stamps` for "the Symphony's task page"
  (`tools/gate-review/src/land/stamps.rs`, `api`): the required set and tier, each stamp's state
  (collected, missing, or land's to check) and detail, and the waivers and requirements with why
  and by whom. A collected stamp and an edit carry no time (`Status`, `Edit`). Only a test calls
  that method; the plugin calls landing for the approve-to-land button alone
  (`tools/symphony/src/lib.rs`, `ApproveToLand`).
- **Plugin seams** (`crates/symphony-plugin/src/lib.rs`, `Plugin`): components, card kinds,
  renderers, routes, nav, home regions, board actions, board fields, watches, doc checks, prose
  rules, generators, commands, transports, theme. On a task a plugin can add a button
  (`BoardAction`) or a typed value (`BoardField`), nothing that lists what the task needs. A
  component without `cache_version` is rendered again on every read. The block vocabulary has
  tables and raw vector shapes (`Span::Graphic` in `crates/symphony-api/src/blocks.rs`), no chart.
- **Agents.** A session row keeps `connected`, `last_seen`, `left_at`, `kind` and `model`
  (`crates/tasks/src/rows.rs`, `SessionRow`). Live, stale and lost are worked out on each read;
  no count of them is kept, and of tool calls only the last one's time.
- **Tokens.** None on the board. `tools/board-token-usage` (the owner approved option B of
  `process-token-recording` on 2026-11-08: the beat hook adds each call's usage) is ready, not
  built. Read 2026-11-11: a Claude Code transcript, a subagent's too, carries on each reply the
  model and its input, output, cache-read and cache-write tokens; a Codex session log carries
  `token_count` events (input, cached, cache-write, output and reasoning tokens). **Neither carries
  a price, and Symphony keeps none: a cost in money cannot be shown today.**

## Measured

Read 2026-11-11, by `just tasks stats` and `just tasks list --json` on each board, `git log`, and
by listing and reading Star Foundry's record folders in its git common dir.

| source | Star Foundry | Symphony |
|---|---|---|
| tasks on the board | 2,676, about 1,220 top-level, since 2026-09-26 | 58, 57 top-level, since 2026-11-11 |
| top-level done | 899; 345 in the last 14 days | 7 |
| lead time, done in 14 days | median 6.1 h, p75 16.0 h, p90 46.4 h | median 0.4 h (7 tasks) |
| sent back from review at least once | 405 of 1,221; 148 of 497 in 14 days | 1 of 56 |
| red tasks filed | 16 `tests/main-red-…`, 54 `tests/nightly-red-…` | none |
| review tasks (`review/…`, `…/review`, `…/adversarial…`) | about 430 | none |
| merges on main | 1,218 in 14 days, no `Revert` commit | 248 since it began, no revert |
| native build/test operations | 32,311 since 2026-11-09 17:38, 245 MB; 2,605 of the newest 3,000 are `verify discovery` | none: no recorder |
| verify run summaries (Star Foundry's own, `sf-verify-summaries/`) | 510 runs since 2026-11-08, 100,173 check entries, 3 to 5 MB a day, kept 30 days | none |
| landing records (`sf-land/landed`) | 558; 498 certificates; 96 stamp logs | none: no land command |
| nightly records | 5 nights, 2026-11-01 to 11-07 | none |
| board health kept | the last hour, in memory | the same |
| tokens on the board | none | none |

The board's message count was not read: no read-only command counts it, and the database was not
queried. The board's own perf target describes the board of 2026-11-07 as 50k messages, 2.6k
sessions and about 3k roles (`crates/tasks/AGENTS.md`).

What Star Foundry's verify summaries say about its runs since 2026-11-08: 190 full runs, median
24 min, p90 2.7 h, median 43 CPU-minutes; 320 prechecks, median 2.2 min, p90 6 min. Of the 510:
410 green, 28 red, 52 timed out, 16 interrupted, 4 incomplete. On 2026-11-09 (UTC) alone: 276
runs, 139 wall-hours, 108 CPU-hours. Over 2026-11-09 and 10 the costliest check was
`rust/gate/test/verify`, 23.6 h over 145 full runs. That is the CI view the owner asks for; it
exists only in a consuming project's files, read by nobody.

**What the stats read costs.** `tasks stats --json` on Star Foundry's board, wall time including
the launcher, three runs each: 1 day 0.92 to 1.15 s, 14 days 1.41 to 2.10 s, its whole history
(`--days 60`, 1,221 tasks) 2.73 to 3.12 s. A command that fails before reaching the board took
0.58 s, so the read itself is about 0.3 to 2.5 s. The machine's load average was 20 to 26 from
other agents' work.

## Runs

What a project's Runs page shows, as a CI product shows it.

### What the owner opens

`▶ runs` in the project's header opens `/<id>/runs`:

1. **Now.** What runs and waits on this machine for the project, from the machine-work snapshot
   the server already keeps per tick: pipeline, task, who started it, how long it has run or
   waited.
2. **Runs**, newest first, one line each: pipeline (verify, lint, test, client build, review
   callback, landing, nightly), result, branch and task, commit, who or what started it, duration,
   queue wait. Filters: pipeline, result, task, branch.
3. **A run** opens on its steps in order, each with its time as a bar, its result and its log (the
   tail first, the whole on demand), and the tests that failed with their first failure line.
4. **Trends**, 14 days by default: per pipeline, runs and pass rate per day and the p50 and p90
   duration; per suite the same; the slowest suites and their trend; the flakiest tests; queue
   wait; machine minutes per day, per task (the ten costliest) and, on the machine view, per
   project.

On a phone, Now and the runs list are one column; a run is one screen of steps; Trends is a tab of
small charts one under another.

```
Runs · symphony                    2 running · 1 queued
● verify  tools/x       calm-gecko    12 min
✓ verify  tools/y       9ac5149       24 min
✗ test    main          a1b2c3d        8 min   2 failed
✓ client  tools/z       e4f5a6b        3 min   waited 40 s
```

### Where runs are kept

- **A, run files in the common dir, a core page.** The recorder appends one line per finished
  run and per step to a file for the UTC day, `sf-runs/DAY.jsonl` in the git common dir beside
  `sf-tasks/`, and keeps each run's log in `sf-runs/logs/`. The server reads the newest files for
  the list and a run, and rolls the trends up every hour ([Per view or rolled up](#per-view-or-rolled-up)).
  Cost: a page kind, an API, a roll-up, the log handling. Nothing touches the board and the
  schema stays. Star Foundry's verify summaries are this shape already: one framed line per run
  appended to a day file, pruned at 30 days (`tools/gate/src/verify/summary.rs`, `Run::finish`).
- **B, runs on the board.** `run` and `step` tables written by the recorder. Cost: a schema
  version, which makes every older server on the machine refuse writes until it updates; a write
  at each run's start, step and end on the board's write path, at Star Foundry's rate hundreds of
  runs a day, which the brief rules out; logs still need files.
- **C, a document of plugin components.** The same files, shown by components (`[[runs]]`,
  `[[run-trends]]`) on a page each project writes. Cost: a component is rendered again on every
  read, charts are raw vector shapes with no values on a tap, nothing updates while a run goes, and
  a project without the document shows nothing.

**Recommended: A.** Every project gets it without a plugin, including Symphony, which has none;
the board takes no load; it follows a format already proven on this machine.

### The run record

`tools/build-test-recorded-generic` plans to keep, per finished operation: task, handle, phase
(build or test), command label, start, end, exit and machine id. A run needs more:

| field | why |
|---|---|
| `run`, `parent` | a run and its steps: `just verify` is lint, client check and tests. Native rows already carry `operation_id` and `parent_id` (`native.rs`, `Row`); the finished record keeps them. |
| `pipeline`, `step` | which CI job (verify, lint, test, client, review-callback, landing, nightly) and which step of it; the phase says only build or test |
| `project` | one machine serves several projects |
| `commit`, `tree`, `branch`, `dirty` | the commit the run checked; the tree tells a flake (one tree red then green) |
| `trigger` | who or what started it: a handle, `owner`, `on_review`, `nightly`, `land` |
| `queued`, `started`, `ended` | queue wait apart from duration |
| `end` | green, red, timed out, interrupted (a signal), incomplete, beside the exit code |
| `cpu_s` | machine minutes: the CPU of the run's children, as Star Foundry's summary takes it (`summary.rs`, `cpu_s`) |
| `log`, `log_bytes` | where its output is kept |
| `suites` | per test binary: passed, failed, ignored and its time |
| `failed` | the failing tests' names and first failure line |
| `rerun_of` | a run started again on the same tree |

One line per run and per step, never per child process: of Star Foundry's newest 3,000 native
operations, 2,605 are `verify discovery` commands, and 32,311 operations took 245 MB in two days.
The recorder writes a run or step line only for a command a recipe names as one.

```json
{"schema": 1, "run": "4f1c…", "parent": null, "project": "symphony", "machine": "…",
 "pipeline": "verify", "step": null, "label": "just verify", "task": "tools/x",
 "handle": "calm-gecko", "trigger": "agent", "branch": "tools/x", "commit": "9ac5149…",
 "tree": "…", "dirty": false, "queued": 1794400000.0, "started": 1794400003.2,
 "ended": 1794401443.0, "end": "green", "exit": 0, "cpu_s": 2598.1,
 "log": "logs/2026-11-11/4f1c….log", "log_bytes": 812345,
 "suites": [{"name": "symphony-board::store", "passed": 549, "failed": 0, "ignored": 0, "s": 41.2}],
 "failed": [], "rerun_of": null}
```

A project with its own recorder writes these lines too (Star Foundry's verify summary holds most
of the fields already: run, branch, tree, started, ended, wall and CPU time, end, exit, and each
check's result, time, wait and rerun); the page reads one format.

### Tests from cargo's output

The recorder passes the command's output through unchanged and reads it as it goes. On the
pinned toolchain (1.99.0) a test binary refuses machine-readable output: `--format json` answers
"The \"json\" format is only accepted on the nightly compiler with -Z unstable-options" (tried on
a test binary of this workspace), and `--report-time` is nightly-only too. What stable prints:

- `Running …` before each test binary, naming it;
- `test NAME ... ok | FAILED | ignored` for each test;
- `test result: ok. 549 passed; 0 failed; … finished in 41.20s` per binary;
- the failures' output under `failures:`.

So per suite (binary) the counts and the time are known, and per test the result, but not each
test's time. The slowest *suites* can be shown; the slowest *tests* need nightly libtest or
cargo-nextest ([Open questions](#open-questions)). Passing tests are counted, not listed, which
keeps a line small: this workspace has several thousand tests.

A **flaky** test is one that failed and passed on the same tree, in a rerun or a later run. The
page counts each test's flaky and red runs over 14 days.

**Queue wait** is the time from queued to started. Star Foundry's slots queue work, and its
summaries keep each check's wait. This workspace has no pool (`tools/cargo.sh` supplies none), so
its runs wait only on Cargo's build-directory lock, which Cargo announces on stderr; whether the
recorder should count that is an open question.

### Kept, and for how long

| what | where | kept | size |
|---|---|---|---|
| run and step lines | `sf-runs/DAY.jsonl` | 90 days | at Star Foundry's rate 3 to 5 MB a day, so under 0.5 GB |
| logs | `sf-runs/logs/DAY/RUN.log` | 14 days, a red run's 30; at most 2 GB a project, oldest first | not measured |
| daily roll-ups | `sf-runs/rollup/DAY.json` | for good | a few KB a day |

The git common dir, not a checkout's `artifacts/`: a removed worktree takes its artifacts with it,
which is why Star Foundry keeps its summaries there (`summary.rs`, module comment).

## The task page

Today the page shows the brief, roles and thread. It gains two sections at the top.

### Gates

Every stamp or check the task needs to land, each one **met** (by whom, when, at which commit),
**missing**, **waived** (by whom, why), **waiting on nobody** (the owner's, a hold), or
**unknown** (the project's tool could not answer).

What Symphony shows without a plugin:

- **Review**: the board's reviewer verdict on each round, by whom and when.
- **Check**: the project's `[verdicts] check` (Symphony's: `just verify`) on the branch's tip,
  from the run files: green, red, or not run at this tip.
- **Review callback**: the `[board] on_review` run for the tip, from the run files.
- **Owner card**: a card the task posted that waits on the owner (the rule of the sign-off lane,
  [TASKS](../TASKS.md#tasks-and-roles)).

A project's gates come from its plugin. Star Foundry's are its stamps: certificate, nightly,
dependency, review, adversarial, owner and holds. A plugin's gate of the same name as Symphony's
takes its place (Star Foundry's review stamp is the approval its landing counts).

How a project's gates reach the page:

- **A, a typed gates seam.** A plugin trait answers a task's gates as data (name, state, by, when,
  commit, why, detail), a new wire type the client draws with one component, the same for every
  project. Symphony can then count them: how long tasks waited on each gate, which gate is missing
  most. Cost: one trait, one wire type, one component. Star Foundry maps its `stamps` answer to
  it, and adds when each stamp was collected and each edit made, which the answer lacks today.
- **B, a block panel.** A plugin returns generic blocks for a task, drawn as a document's are.
  Cost: the least Symphony work; each project lays its panel out its own way, and Symphony can
  neither count gates nor chart what they cost.

**Recommended: A**: "what is it waiting on" is the question asked of a task, and only a typed
answer can feed the defect report.

The gates are fetched only when the section nears the screen and kept by the branch's tip: Star
Foundry's answer starts its landing binary (`tools/symphony/src/lib.rs`, `land_call`).

### Timeline

What the task went through, each step with its duration: its stages (ready, in progress, review,
blocked), claims and releases, review rounds and their verdicts, builds and tests, waits
(unclaimed, blocked on a dependency, waiting on a gate). Tokens per round join it once
`tools/board-token-usage` records them.

No new recording: the replay keeps each segment (status, from, to) beside its totals, a change in
`stages.rs` alone; the run files give the task's runs; the thread gives who claimed and who gave
each verdict. A phone gets a vertical list with a bar per step; a wide screen gets lanes over time
(stages on one, runs on another, verdicts as marks).

```
tools/x · in review 3.1 h
Gates
  ✓ review       swift-merlin · 12:41
  ✗ certificate  verify red at 4f1c
  ~ dependency   waived: "no new crate" · silver-sparrow
Timeline                              6.2 h
  ready          ▇▇ 0.4 h
  in progress    ▇▇▇▇▇ 1.3 h   calm-gecko
    verify       ▇▇▇ 24 min   green
  review         ▇▇ 0.8 h     changes · swift-merlin
  in progress    ▇ 0.3 h
    verify       ▇▇ 21 min    red, then 26 min green
  review         ▇▇▇▇ 3.1 h   now
```

This design is the page half of `tools/stage-times`, and the stage durations and step diagram of
`tools/review-as-stamps`' second slice; that slice's diff and tokens stay with it.

## Pipeline and defects

A Reports page per project, `◔ reports`, after Runs.

### Flow

- **Cumulative flow**: tasks in each status per day, top-level by default. Open tasks made before
  the window count too, which the stats read leaves out (`Store::stage_records` reads the tasks
  made in the window and the done ones written in it); the roll-up reads open tasks as well.
- **Lead time** per week, median and p90; **where task-hours go** (unclaimed, in progress,
  review, blocked); **bounces**; **what sits longest**: the figures `tasks stats` prints today,
  as trends.

### Defects

Where a defect was caught, how long it took to fix, and whether it escaped.

| caught by | read from | today |
|---|---|---|
| review | `changes` verdicts, per round | on the board |
| a red verify before landing | run files, a red run on a task's branch | Star Foundry's summaries only |
| main red | `tests/main-red-…` tasks (`crates/tasks/src/red.rs`, `SERIES`) | on the board |
| nightly red | `tests/nightly-red-…` tasks | on the board |
| a revert | `Revert` commits on main | in git |
| reopened | no source: a finished task cannot reopen through the board, so follow-up work is a new task with no link | new recording |

- **Time to fix**: a red task's creation to done; a `changes` verdict to the next move to review.
- **Escape rate**: of the tasks landed in a window, how many a later main red, nightly red or
  revert traces back to. The trace needs the red's filer (the project's landing and nightly) to
  record which landing or commit it came from; Symphony shows reds per week beside landings per
  week until it does.
- **Which checks catch what**: per step or check, its reds before landing against those on main
  or at night (from the run files' branch). A project adds its own classification through its
  report section: Star Foundry's test ledger (`star-foundry-test-ledger.db`, 11 MB, not opened)
  sorts failures into real defects, flakes and infrastructure.

### Agents

Second in line: sessions started per day by kind and model (session rows); tokens per task, role
and review round (`tools/board-token-usage`); time working against waiting, which needs tool-call
times the board does not keep (the token rows, one per call, give them); stale and lost counts per
day, which the server can count as its clock says them. Money: unknown, see
[What exists today](#what-exists-today).

### Board health over days

The server appends, once an hour, the hour it already holds in memory (`Recorder::report`) to a
day file beside `health.json`: dispatch and sweep passes and failures, request counts, slow and
unanswered requests, the slowest caller, write p50 and p90. No board request. Kept 90 days.

### How the figures are computed

- **A, an hourly roll-up into files.** Each hour, and once after midnight for the day that ended,
  the server reads each project's window once (the stats read, 0.3 to 2.5 s on Star Foundry's
  board) and writes the day's figures to a file; the page reads files. Cost: a figure is up to an
  hour old; one board read an hour per project; nothing written to the board.
- **B, computed on each view.** Cost: the stats read on every open, up to 2.5 s on Star Foundry's
  board and longer as it grows, on its read path; no history for what the board does not keep.
- **C, a nightly report document.** A scheduled command writes a dated Markdown report. Cost: no
  live figure, and a commit a day.

**Recommended: A.**

## Core page or plugin blocks

- **Core pages with two seams (recommended).** Runs, Reports and the task page's sections are
  Symphony's, over Symphony's own records, so every project has them. A project adds what only it
  knows through two seams: its task gates (typed, above) and a report section (generic blocks,
  placed under Symphony's figures), Star Foundry's being certificate costs, art builds and its
  test ledger's classes. Both draw charts with one new block, a chart (bars or lines over labelled
  series), which a component draws at any width with each value on a tap. Cost: two route names,
  `runs` and `reports`, join the Symphony's own (`symphony_api::SYMPHONY_ROUTES`), so a project's
  entrypoint can no longer use them.
- **A built-in reports plugin of components on a document a project writes.** Cost: as C under
  [Where runs are kept](#where-runs-are-kept).
- **All in each project's plugin.** Cost: Symphony's own project has no plugin (`symphony.toml`
  names none) and would show nothing; each project writes the same figures again.

The chart block is a wire change, so it reaches main in a task of its own first (RULES
`workflow/shared-data-lands-first`).

## One machine or several

- **One machine, every project.** The machine's one server serves each registered project
  (`projects.json`; read 2026-11-11: `symphony` and `star-foundry`), and each project's run files
  are in its own common dir, so a machine view (beside the project picker) sums them: running
  now, machine minutes per project per day.
- **One project, several machines.** Not possible yet. The board listens on 127.0.0.1 alone
  ([TASKS](../TASKS.md#limits)), and machine work has no transport ([MACHINE_WORK](../MACHINE_WORK.md)),
  although its `Sample` type is shaped for authenticated publishers
  (`machine_work/projection.rs`). Every run line carries the machine id now, so lines from
  several machines can be merged later, by pushing finished lines to the hosting server over the
  tailnet or by carrying them in a git ref; the choice waits for a second machine.

## Per view or rolled up

No report writes to the board. The only board load is one read per project an hour.

| figure | when |
|---|---|
| running and queued now | per view: the server's per-tick machine snapshot |
| the runs list, a run, its steps and log | per view: the newest day files, indexed by run in memory |
| a task's gates | per view, when the section nears the screen; kept by the branch tip |
| a task's timeline | per view: the thread the page reads already, and that task's runs |
| pass rates, durations, slowest suites, flakiest tests, machine minutes | hourly, from the run files |
| flow, lead time, task-hours, bounces, defects, escape rate | hourly, one read of the window |
| board health per hour | appended hourly from memory |
| agents per day, tokens | hourly, from session rows and token rows |

## The phone

- **Runs**: the running count, then one line per run (result, pipeline, task, duration); a run is
  a list of steps with bars; logs open on demand, the tail first.
- **Task page**: the gates as a short checklist, then the timeline as a vertical list.
- **Reports**: four figures at the top (median lead time and its trend, red runs today, escapes
  this week, CPU-hours today), each opening its chart; charts one under another, 14 days wide.
- Each is a page kind of its own, so the 250 KB first-visit budget of `/` and `/tasks`
  (`apps/symphony-ui/AGENTS.md`) pays nothing; the board's header shows "N running" from data its
  page has already.

## What the verdicts file

Once approved, as board tasks, the shared data first:

1. The run line and its folder, agreed with `tools/build-test-recorded-generic`: the fields above,
   cargo's test output read, logs and their bounds.
2. The chart block in `symphony-api` and its component.
3. The Runs page and its API, with the machine view.
4. The replay's segments in `stages.rs`, and the task page's timeline.
5. The gates seam, its wire type and Symphony's own gates; Star Foundry's adapter is its task.
6. The hourly roll-up and the health history.
7. The Reports page, and the report-section seam.

## Open questions

1. Retention: runs 90 days, logs 14 (a red's 30, at most 2 GB a project), roll-ups for good?
2. The slowest tests need each test's time, which stable cargo does not print. Adopt cargo-nextest
   (a new tool), or show suites only?
3. A cost in money needs a price per model from the owner, and on a subscription per-token money
   is not what is paid. Show tokens only?
4. Should the project's landing and nightly record, when they file a red, which landing it came
   from, so escapes can be traced to tasks?
5. Symphony has no land command: its merges to main are made by hand. Count each merge commit on
   main as a landing run?
6. Queue wait where no pool queues work: show none, or count the time Cargo waits on its build lock?
7. Should a project's gates, once shown, also replace Symphony's review gate everywhere the board
   judges review (the sign-off lane, the PULL digest)? Until answered they replace it on the page
   alone.

## Verified and inferred

- **Read in code:** each fact above that names a file and function, in this repository and,
  read-only, in Star Foundry's `tools/gate-review`, `tools/gate` and `tools/symphony`.
- **Measured:** the table under [Measured](#measured) and the stats timings, on 2026-11-11, on a
  machine other agents were loading.
- **Tried:** a test binary of this workspace refusing `--format json` on the pinned toolchain; the
  usage fields of a Claude Code transcript and a Codex session log.
- **Inferred, not tried:** run-line sizes for Symphony's own runs; that Cargo's build-lock wait can
  be read from its stderr; log sizes; the cost of the hourly read on a board larger than today's.
