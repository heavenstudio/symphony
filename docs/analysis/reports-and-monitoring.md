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

**Decided: A, run files in the common dir, a core page** (the owner, 2026-11-11), with the send of
[Several machines, one board](#several-machines-one-board). The recorder appends one line per
finished run and per step to a file for the UTC day, `sf-runs/DAY.jsonl` in the git common dir
beside `sf-tasks/`, and keeps each run's log in `sf-runs/logs/`. The server reads the newest files
for the list and a run, and rolls the trends up every hour
([Per view or rolled up](#per-view-or-rolled-up)). Cost: a page kind, an API, a roll-up, the log
handling. Nothing touches the board and the schema stays. Star Foundry's verify summaries are this
shape already: one framed line per run appended to a day file, pruned at 30 days
(`tools/gate/src/verify/summary.rs`, `Run::finish`).

The work is on the board, in the order it lands: `tools/run-record` (after
`tools/build-test-recorded-generic`), `tools/run-logs`, `tools/run-suites`, `tools/run-send`,
`tools/runs-rollup` and `tools/runs-page`, with `tools/nextest` before `tools/run-suites`. The
six smaller choices they met are answered ([Open questions](#open-questions)).

Not chosen:

- **B, runs on the board.** `run` and `step` tables written by the recorder. Cost: a schema
  version, which makes every older server on the machine refuse writes until it updates; a write
  at each run's start, step and end on the board's write path, at Star Foundry's rate hundreds of
  runs a day, which the brief rules out; logs still need files.
- **C, a document of plugin components.** The same files, shown by components (`[[runs]]`,
  `[[run-trends]]`) on a page each project writes. Cost: a component is rendered again on every
  read, charts are raw vector shapes with no values on a tap, nothing updates while a run goes, and
  a project without the document shows nothing.

**Why A.** Every project gets it without a plugin, including Symphony, which has none; the board
takes no load; it follows a format already proven on this machine.

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

Filed as `tools/run-record`, a follow-up to `tools/build-test-recorded-generic` that leaves that
task's scope alone. Read 2026-11-11, while it was in review: its record already keeps the operation
and parent ids, branch, commit and host; inside a recorded run it runs commands plainly, so a
verify's steps are not yet recorded apart.

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
cargo-nextest.

**Decided: cargo-nextest** (the owner, 2026-11-11: "can absolutely adopt cargo nextest"). This
workspace's tests run through it at a pinned version (`tools/nextest`), and its report gives each
test's time. Doc-tests, which nextest does not run, keep libtest's plain output above. The run line
keeps per-suite counts and the failed tests, so it stays small with several thousand tests; each
test's time is kept beside the run's log, and the roll-up gives the slowest and flakiest tests from
it.

A **flaky** test is one that failed and passed on the same tree, in a rerun or a later run. The
page counts each test's flaky and red runs over 14 days.

**Queue wait** is the time from queued to started. Star Foundry's slots queue work, and its
summaries keep each check's wait. This workspace has no pool (`tools/cargo.sh` supplies none), so
its runs wait only on Cargo's build-directory lock, which Cargo announces on stderr. Decided (the
owner, 2026-11-11): where nothing queues work, no queue wait is shown, and the lock's wait is not
counted.

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

Every stamp or check the task needs to land. Each one is in one of these states:
- **met**: by whom, when, at which commit;
- **missing**;
- **waived**: by whom, and why;
- **held**: waiting on the owner, or a hold;
- **unknown**: the project's tool could not answer.

What Symphony shows without a plugin:

- **Review**: the board's reviewer verdict on each round, by whom and when.
- **Check**: the project's `[verdicts] check` (Symphony's: `just verify`) on the branch's tip,
  from the run records: green, red, or not run at this tip.
- **Owner card**: a card the task posted that waits on the owner (the rule of the sign-off lane,
  [TASKS](../TASKS.md#tasks-and-roles)).
- **Review callback**: the `[board] on_review` run for the tip, once its exit is recorded. Today
  it runs detached and its exit is not kept (`crates/tasks/src/on_review.rs`).

A project's gates come from its plugin. Star Foundry's are its stamps: certificate, nightly,
dependency, review, adversarial, owner and holds. A plugin's gate with the same name as Symphony's
takes its place. Star Foundry's review stamp, for example, is the approval its landing counts.

**The rule: the more specific level's answer replaces the general one's** (the owner, 2026-11-11:
"lower level replaces higher level. Should replace it everywhere for itself."). A project's gate
replaces Symphony's same-named gate for that project everywhere the board judges it: the task page,
the sign-off lane, the PULL digest orchestrators get, the move to blocked and the default land
command. A project with no answer keeps Symphony's gates. The page's part is
`tools/task-gates-seam`; the rest is `tools/project-gates-everywhere`.

**Decided: A, a typed gates seam** (the owner, 2026-11-11, after the pictures below).
- A plugin trait answers a task's gates as data: name, state, by, when, commit, why, detail, link
  and past states. That is a new wire type, which the client draws with one component, the same
  for every project.
- Symphony can then count the gates: how long tasks waited on each one, and which is missing most.
- Star Foundry maps its `stamps` answer to the seam. It also adds when each stamp was collected and
  each edit made, which the answer lacks today.

Not chosen: **B, a block panel**, generic blocks a plugin lays out for a task. Each project would
lay its panel out its own way, and Symphony could neither count gates nor chart what they cost.

The gates are fetched only when the section nears the screen, and kept by the branch's tip,
because Star Foundry's answer starts its landing binary (`tools/symphony/src/lib.rs`, `land_call`).

The work is on the board:
- `tools/task-gates-seam`: the seam, its wire type and Symphony's own gates;
- `tools/task-gates-panel`: the component;
- `tools/star-foundry-gates-adapter`: Star Foundry's adapter, as a note to copy to its board.

### Timeline

What the task went through, each step with its duration: its stages (ready, in progress, review,
blocked), claims and releases, review rounds and their verdicts, builds and tests, waits
(unclaimed, blocked on a dependency, waiting on a gate). Tokens per round join it once
`tools/board-token-usage` records them.

It needs no new recording:
- the replay keeps each segment (status, from, to) beside its totals, a change in `stages.rs`
  alone;
- the run records give the task's runs;
- the thread gives who claimed and who gave each verdict;
- the gates' past states give the gate rows.

On a phone it is a vertical list with a bar per step. On a wide screen it is lanes over time: the
stage, the executor, the reviewer, builds and tests, and one row per gate. The pictures below are
the look to build to.

It is filed as `tools/task-timeline`, after the gates seam and the run recorder
(`tools/build-test-recorded-generic`). It replaces the page half of `tools/stage-times`, and the
stage durations and step diagram of `tools/review-as-stamps`' second slice. That slice's diff and
tokens stay with it.

### Pictures

**The task shown is made up** in Star Foundry's shape: `tools/verify-stops-when-moved`, in review.
- It needs four stamps: certificate, nightly, dependency and review.
- It went through two review rounds, a build and three test runs. A fourth test run is going now,
  because main moved after its certificate.
- Its times are invented.

The pictures are SVG written by hand. They use the client's tokens
(`apps/symphony-ui/assets/board.css`) and the task head as `task_detail.rs` draws it, and they
follow the reader's light or dark setting.

The A pictures and the timeline pictures are the look the filed tasks build to; the B pictures
show what was not chosen.

**Gates, A: a typed seam (chosen).** Symphony draws the plugin's answer. What the task waits on comes first,
then the waived and met gates, each with its detail, who and when.

![Gates on a phone, option A](reports-gates-a-phone.svg)

![Gates on a wide screen, option A](reports-gates-a-wide.svg)

**Gates, B: a block panel (not chosen).** The plugin lays out a paragraph, a table and two more paragraphs, in
its own words. Star Foundry's stamps answer has `stamp`, `collected` and `detail` and no times, so
the table has none. On a phone it scrolls sideways.

![Gates on a phone, option B](reports-gates-b-phone.svg)

![Gates on a wide screen, option B](reports-gates-b-wide.svg)

**Timeline.** It is the same under A and B.
- On a phone: one row per step, with a bar as long as the step took and its queue or wait drawn
  lighter.
- On a wide screen: lanes over the same hours, for the stage, the executor, the reviewer, builds
  and tests, and each gate's state.

![Timeline on a phone](reports-timeline-phone.svg)

![Timeline on a wide screen](reports-timeline-wide.svg)

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
day, which the server can count as its clock says them.

**Money, estimated at API prices** (the owner, 2026-11-11: "tokens are hard to measure, since
cache vs non cache hits counts differently. This should be counted "as if we were paying API costs"
for it. Best assumption/guess on pricing actual cost"). Cost is input, output, cache writes and
cache reads, each priced apart per model from a dated price table in the repository, taken from
the providers' published prices, and always labelled an estimate. A model the table lacks takes its
family's newest price, marked as a guess; one with no family is shown unpriced and named. The work
is `tools/agent-cost-estimate`, after `tools/board-token-usage`.

### Board health over days

The server appends, once an hour, the hour it already holds in memory (`Recorder::report`) to a
day file beside `health.json`: dispatch and sweep passes and failures, request counts, slow and
unanswered requests, the slowest caller, write p50 and p90. No board request. Kept 90 days.

### How the figures are computed

**Decided: A, an hourly roll-up into files** (the owner, 2026-11-11). Each hour, and once after
midnight for the day that ended, the shared server reads each project's window once and writes the
day's figures to a file in that project's git common dir. The stats read takes 0.3 to 2.5 s on Star
Foundry's board. The page reads the files. A figure is up to an hour old. The cost is one board read
an hour per project, and nothing is written to the board. Board health and agent activity are
appended to their own day files from what the server holds in memory.

Not chosen:

- **B, computed on each view.** The stats read would run on every open: up to 2.5 s on Star
  Foundry's board, longer as it grows, and on the board's read path. There would be no history for
  what the board does not keep.
- **C, a nightly report document.** No live figure, and a commit a day.

The work is on the board: `tools/reports-rollup`, `tools/reports-page`,
`tools/board-health-history`, `tools/agent-activity-history` and `tools/red-landing-origin`.

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

The core pages are decided (Runs and Reports, both A) and the gates seam is filed. The
report-section seam and the chart block wait until a project asks for its own section (the owner,
2026-11-11); the core pages draw their own charts.

## One machine or several

- **One machine, every project.** The machine's one server serves each registered project
  (`projects.json`; read 2026-11-11: `symphony` and `star-foundry`), and each project's run files
  are in its own common dir, so a machine view (beside the project picker) sums them: running
  now, machine minutes per project per day.
- **One project, several machines.** Not possible yet; below is how it will work.

### Several machines, one board

The owner asked how A works once separate machines share a single board.

**As written, A shows only one machine's runs.** The run files live in the git common dir of the
clone where the run happened. A clone on another machine has its own common dir. Today it also
has its own board:
- the board is started `--unauthenticated` and bound to loopback (`crates/tasks/src/server.rs`,
  `START`; `crates/tasks/src/port.rs` binds `Ipv4Addr::LOCALHOST`);
- `TASKS_SERVER` can name another board's address (`crates/tasks/src/cli/mod.rs`, `reach_board`),
  but nothing listens beyond loopback;
- machine work has no transport either: MACHINE_WORK says it "adds no remote worker, reporting
  transport, board persistence or scheduler". Its `Sample` type is shaped for authenticated
  publishers (`machine_work/projection.rs`).

So one board across machines is itself unbuilt, and runs must travel the way the board's other
traffic will.

**The recommended form: local first, then sent to the hosting server.**

1. **Local first.** The recorder appends each line to the local day file before anything else, as
   on one machine. That file is the outbox: nothing is lost while the network is down.
2. **Sent.** It POSTs the run's start line and end line to the Symphony server that hosts the
   board.
   - A clone names that server in the machine folder's registry entry for the project
     (`projects.json`, `crates/symphony/src/machine/registry.rs`), since the address differs per
     machine and does not belong in the committed `symphony.toml`.
   - The coordinator's starting point sent finished runs only. That would hide what runs elsewhere
     from "running now", so the start line goes too.
3. **Stored by machine.** The server appends the lines to its own run files, keyed by the sending
   machine's id (`crates/symphony/src/machine/identity.rs`, the `machine-id` file). It files a line
   under the day it received it, so the newest files stay the ones to read. It answers with the
   run ids it stored.
4. **Re-sent until acknowledged.** A per-day mark of the last acknowledged line says what to send
   again. Lines are re-sent at the start and end of the next run, and by that machine's own server
   tick if it runs one. Run ids make a repeat harmless: the server keeps the window's ids and stores
   a line once.

**Who may send.**
- The server's gate already admits only loopback and tailnet addresses that `tailscale whois`
  names as a node of this tailnet, and it checks the Host
  (`apps/symphony-server/src/gate.rs`, `allows`, `allows_host`). Writes also pass `write.rs`'s Host
  check; a recorder is no browser, so no Origin is involved.
- The machine id is "neither hostname nor authentication" (identity.rs). Any peer could claim any
  id. So the server records the whois node with each line, binds a machine id to the first node it
  arrived from, and refuses that id from another node until the owner clears it.
- This trusts the tailnet as the phone already does; no new secret is needed.

**When the server is unreachable.** Runs wait in the outbox and arrive late. A late line is
marked with its delay, and each machine's last contact shows on the page. A start line that never
gets its end within the run's own timeout reads "lost contact", never "running".

**Clocks.**
- Durations are measured on the machine with its monotonic clock, so a wrong wall clock cannot
  make them wrong.
- Ordering, freshness and retention use the server's receipt time, as MACHINE_WORK already rules
  for samples: "Producer phase timestamps are display information, never freshness."
- Each POST carries the sender's wall time, so the server measures that machine's offset. Start
  times are shown corrected by it, with the offset named when it is over 30 s. The raw value stays
  in the line.

**Logs.**
- Logs stay on their machine.
- A red run's log travels with its end line, cut to its head and tail (a few hundred KB at most),
  which is what is read first.
- A green run's log is fetched on demand through that machine's own server when it runs one,
  behind the same gate. Otherwise the page names the machine that holds it.
- Test results per suite are in the line itself, so trends never need a log.

**Against B, runs as board tables.**
- Every machine would need to reach the board's database. It has no login and listens on loopback,
  so it would need authentication, or a proxy through the server, which is the same HTTP endpoint
  with more load on the write path.
- An offline machine would still need a local outbox and log files.
- And B still costs its schema change.

**Considered and set aside: lines carried in a git ref.** Each machine would push its lines to
`refs/…/runs/<machine>`. That needs a shared remote, and pushes contend with each other. Lines would
arrive minutes late, and logs would grow history.

**Decided: A, with the send** (the owner approved A after this answer, 2026-11-11). It changes
nothing on one machine. It adds one endpoint on the server and a send in the recorder: filed as
`tools/run-send`.

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

1. The run line and its folder, after `tools/build-test-recorded-generic`: the fields above,
   cargo's test output read, logs and their bounds, and the send to the hosting server: filed as
   `tools/run-record`, `tools/run-suites`, `tools/run-logs` and `tools/run-send`, with the tests
   run through cargo-nextest first (`tools/nextest`).
2. The chart block in `symphony-api` and its component: not filed, until a project asks for its
   own section.
3. The Runs page and its API, with the machine view, and the runs' hourly roll-up: filed as
   `tools/runs-page` and `tools/runs-rollup`.
4. The replay's segments in `stages.rs`, and the task page's timeline: filed as
   `tools/task-timeline`.
5. The gates seam, its wire type and Symphony's own gates, and the component: filed as
   `tools/task-gates-seam` and `tools/task-gates-panel`. Star Foundry's adapter is a note to copy
   to its board, `tools/star-foundry-gates-adapter`.
6. The hourly roll-up and the health history: filed as `tools/reports-rollup`,
   `tools/board-health-history`, `tools/agent-activity-history` and `tools/red-landing-origin`.
7. The Reports page (filed as `tools/reports-page`), and the report-section seam (not filed,
   until a project asks for its own section).
8. From the answers below: agents' cost in money (`tools/agent-cost-estimate`), a default land
   command (`tools/land-command`), and a project's gates replacing Symphony's everywhere
   (`tools/project-gates-everywhere`).

## Open questions

None is open. The owner answered questions 2, 3, 5, 6 and 7, and whether to build a project's own
report section now, on 2026-11-11 (16:22).

1. Settled by the approval of `reports-ci-runs`: runs 90 days, logs 14 (a red's 30, at most 2 GB a
   project), roll-ups for good.
2. The slowest tests need each test's time, which stable cargo does not print. Adopt cargo-nextest
   (a new tool), or show suites only? **Answered:** "can absolutely adopt cargo nextest"
   (`tools/nextest`).
3. A cost in money needs a price per model from the owner, and on a subscription per-token money
   is not what is paid. Show tokens only? **Answered:** "nope", money as if paying API prices, a
   best guess where a price is unknown ([Agents](#agents), `tools/agent-cost-estimate`).
4. Settled by the approval: a red names the commits it came between (`tools/red-landing-origin`).
   Star Foundry's landing and nightly passing them is Star Foundry's change, on its own board.
5. Symphony has no land command: its merges to main are made by hand. Count each merge commit on
   main as a landing run? **Answered:** "add a land command, should be offered by default on init.
   Remember -> convention over configuration approach" (`tools/land-command`). Landings are counted
   from its runs, not from merge commits.
6. Queue wait where no pool queues work: show none, or count the time Cargo waits on its build lock?
   **Answered:** none.
7. Should a project's gates, once shown, also replace Symphony's review gate everywhere the board
   judges review (the sign-off lane, the PULL digest)? **Answered:** yes, everywhere for that
   project ([Gates](#gates), `tools/project-gates-everywhere`).

## Verified and inferred

- **Read in code:** each fact above that names a file and function, in this repository and,
  read-only, in Star Foundry's `tools/gate-review`, `tools/gate` and `tools/symphony`.
- **Measured:** the table under [Measured](#measured) and the stats timings, on 2026-11-11, on a
  machine other agents were loading.
- **Tried:** a test binary of this workspace refusing `--format json` on the pinned toolchain; the
  usage fields of a Claude Code transcript and a Codex session log.
- **Drawn, not built:** the six pictures under [Pictures](#pictures), each rendered in headless
  Chrome in light and dark to check it; their task and times are made up.
- **Inferred, not tried:** the multi-machine send under
  [Several machines, one board](#several-machines-one-board); run-line sizes for Symphony's own runs; that Cargo's build-lock wait can
  be read from its stderr; log sizes; the cost of the hourly read on a board larger than today's.
