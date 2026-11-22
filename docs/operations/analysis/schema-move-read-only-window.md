# The read-only window after a schema move

When a landing moves this repository's board to a new schema, the machine's server, the release
`current` names, reads the board one schema ahead and writes nothing to it until a release at that
schema serves it. [Its card](../../workflow/RELEASES.md#schema-landing-then-release) took the wait for the
release count out of that window: the orchestrator starts a release run right after such a
landing. This page weighs the two ways that card named to remove the window instead, and a third
met on the way, and recommends one.

A is the way chosen ([its card](#schema-move-keeps-pages-writing)), and it is
built ([CHECKS](../../workflow/CHECKS.md#a-schema-step-that-keeps-the-symphony-writing)). Building it was a
schema step of its own, 25, and it removes the window only once a release carrying it is
`current`. Until then, and for every step not declared, the window is as
[the window](#the-window) says.

## The window

The window comes from one rule: a program writes only a board at its own schema. A store opening
a board decides by `crates/tasks/src/schema.rs` (`decide`): a store opened to write takes a board
at its own schema, or moves an older one forward; one opened to read, the Symphony's, also reads a
board one schema ahead. Every write transaction then starts with the guard (`GUARD` there, and its
PostgreSQL twin in `crates/tasks/src/store/postgres/mod.rs`), which refuses unless the board is at
the store's own schema. In this repository main's build moves the live board at its first board
command once a schema lands (`crates/tasks/src/live.rs`, [BOARD_OPERATIONS](../BOARD_OPERATIONS.md#which-build-moves-a-live-board)), so
the board is ahead of the machine's server from that command until the refresh points `current` at
a release of the new schema and the server follows it ([the switch](../../workflow/RELEASES.md#the-switch)).

While it lasts, the server reads the board and writes nothing to it (`board_read` in
`apps/symphony-server/src/live/machine_work.rs`, its notice `read_only` in
`apps/symphony-server/src/live/health.rs`):

- the owner's saves and chat, with the asks they open, wait in the journal and reach no agent;
- the owner's message on a task, a priority, a field, a cancel, a quick task, a card's cancel of
  its task and a document save's move of its task are refused;
- its clock's callback pass (the PULL digests among it), its judging of sessions and its gives of
  unstaffed work are skipped.

The agents' own board commands go on: they run main's build, at the board's schema, and their
writes are delivered by the detached `tasks dispatch`.

**How long.** Until a release at the new schema is `current` and the server has followed it, which
no rule bounds. [The cadence's rule](../../workflow/RELEASES.md#the-cadence) for a landing that moves the
board's schema starts the release run at once, and that removes only the wait for the count. A
green run takes 17 to 37 minutes, the install and the refresh included, as
[its card](../../workflow/RELEASES.md#schema-landing-then-release) reads the runs since 2026-11-16. A red run
holds its commit as the release candidate until its fixes land and a rerun is green: 27 minutes to
16 hours from a counted run's start to its release, on that card. And the refresh points `current`
only once every writer seen on the other projects' boards is at the new schema, which held two
releases of 2026-11-18 for 1 h 50 and 1 h 08. A second schema landing waits until a release
serving the first is `current`, so the board is never two ahead, which the server would not read
at all.

**How often.** Nine schema steps landed from 2026-11-10 to 11-18, 14 to 22, and two tasks hold a
23 now.

## What the server writes

What "a schema that changed nothing the server writes" can mean is set by what the server writes,
and it writes most of the board:

| Write | Where | Tables it touches |
|---|---|---|
| The owner's saves and chat, the asks they open and close | `crates/symphony/src/save.rs`, `callbacks.rs`, `owner_asks.rs`, `verdicts.rs` | `message`, `delivery`, `ask`, `task` (moves) |
| A message, a priority, a field, a cancel from a task's page | `crates/symphony/src/taskview.rs`, `apps/symphony-server/src/write.rs` | `message`, `task` |
| A quick task | `crates/symphony/src/save/quick_task/outcome.rs` | `task`, `message` |
| A document save under delivery, moving its task | `crates/symphony/src/commit/pending.rs` | `task`, `message` |
| The clock's callback pass, sessions judged, gives | `apps/symphony-server/src/live/machine_work.rs`, `presence.rs` | `delivery`, the callback tables, `session`, `role` |

Each of these also moves the counters and the derived records the tables' events keep (the board's
marker, `session_revision`, `said`). So a schema step leaves the server's writes as they were only
when it touches no table that existed before it: no field, index, event or assertion added to or
changed on one, and no rule in code that writes to one must now follow. A step that only adds
tables of its own, which the server neither writes nor is expected to, leaves them alone.

## The steps so far

| Schema | Landed | What it changed | Only new tables? |
|---|---|---|---|
| 14 | 11-10 | two indexes on `task_session` | no |
| 15 | 11-11 | `session_revision`, and the event on `session` rewritten | no |
| 16 | 11-11 | every table's marker event rewritten for a new counter | no |
| 17 | 11-11 | `ask` | yes |
| 18 | 11-14 | `writer` | yes |
| 19 | 11-16 | `usage` | yes |
| 20 | 11-17 | `unanswered_restore` | yes |
| 21 | 11-17 | `artifact_ledger` | yes |
| 22 | 11-18 | `recovery` | yes |

Read from each step's commit to `crates/tasks/src/schema.rs` and, from 20 on, to
`crates/tasks/src/store/postgres/schema.rs`: on both engines each of 17 to 22 only added a table.
Of the two 23s waiting, `tools/durable-provider-attempts` adds four tables the server never writes,
and `tools/workflow-runtime` adds checks on every write of a task, which the server writes.

22 shows what "only new tables" does not say. Its table holds the board's recovery generation, and
it came as a schema step so that the guard would refuse an older build's copy of the board, which
would keep the generation. For the server, which copies nothing, it changed nothing. For an older
`tasks-copy` it was meant to change everything. A rule that let every older writer through on a
step of new tables alone would have undone it.

## A. The server writes a board one schema ahead

The release's server keeps writing a board one schema ahead of it when that schema's step says it
kept the server's writes as they were.

**The mechanism.**

- Each schema declares whether its step keeps a Symphony one schema behind writing: a list in
  `crates/tasks/src/schema.rs` of the versions that do, each with one line saying why. A version
  not listed does not; a schema bump that writes nothing there leaves the window as it is now.
- The build that moves a board writes, in the same transaction as the move, the oldest schema
  whose Symphony may write it (`meta:schema`, a field beside `version`): the version before when
  its own is listed, its own otherwise.
- The guard of the Symphony's store, opened to read, takes a board at its own schema, or one schema
  ahead whose record names its schema or older. Every store opened to write keeps the rule as it
  is: a release's `tasks`, `agent-send` and `tasks-copy`, a consuming project's programs, and a
  store opened before the move are refused one schema ahead whatever the record says. 22's refusal
  of an older copy stands.
- The server, on a board one ahead whose record lets it write, does what it does at its own: the
  journal flushed, the asks kept, the callback pass, the judging and the gives, and every route.
  Its notice says nothing of the schema. On one whose record does not, it is read-only as now.
- Two ahead is still read by nothing, so the hold on a second schema landing stays.

**What is declared, and who checks it.** One line per schema step, by the task that bumps it.

- A test checks it on the database. Each schema's tables, as the database defines them on each
  engine, are a committed file, one per version, which a test holds to a board made fresh at that
  version. For a listed version, a test holds every table of the version before to the same
  definition in this one. A listed step that changed one is red on its own branch, before review.
- Review checks what no test can: a rule in code that the step's writes to an older table must now
  follow. The line's reason says why there is none, and the schema task's reviewer reads it against
  the diff.
- The board checks it at every write: the server's guard reads the record in the write's own
  transaction, so a board without it, or one moved by a build before this mechanism, is read-only
  to the server as now.

**What a wrong declaration breaks.** A step listed although it changed an older table in the
database is red on its branch. Were it to land all the same, the database would apply the new
definitions to the server's writes: an assertion or a type refuses the write, which waits in the
journal or is refused as it is today, and an event added runs for them as for any write. The harm
is a rule held in code alone. A step that adds a table of workflow states and, in code only,
refuses a task's move its state does not allow, listed by mistake, would let the server move tasks
past that check while the window lasts: a card's cancel, a document save's move to review. The
newer build would then find tasks in states its rule says cannot be. Writes like these can break
what the rule keeps true of the board. What is lost, whether anything notices, and whether it can
be put right all depend on the rule: nothing here promises the newer build finds them, nor that a
fix can bring back what they changed. What the mechanism does bound is who: only the server's own
writes, for as long as the board stays ahead of it, which a red run or the refresh's wait on
writers can make hours ([the window](#the-window)).

**The writers involved.** One: the Symphony's store in the machine's server, at the schema of the
release `current` names. Every other writer is refused as now.

**How the tests prove a write safe one schema apart.**

- The guard, on both engines: the Symphony's store one behind writes a board whose record lets it,
  and is refused on one whose record does not, on one without the record, on a board two ahead and
  on a board behind; a store opened to write is refused one ahead whatever the record.
- The move writes the record, on both engines, for a listed version and an unlisted one, and for a
  move of two steps at once.
- The catalogue test above: each version's file matches a board made fresh at it, and a listed
  version leaves every older table as it was.
- The server: a tick on a board one ahead and listed flushes the journal, keeps the asks,
  dispatches and gives, and its routes write; one not listed is read-only and says so.
- Each listed step, in its own task: the Symphony's writes made through a store one schema behind
  (`Schema::before`, the tests' older build) on a board at the new schema, then read by the new
  build. That store runs this build's code, not the release's, so it proves the database and the
  new reads, not the older code: that part is the reason line's, read in review.

**What it costs.** Built once: the record and the migration step that adds it (a schema step of
its own, which changes `meta` and is not listed: the last window paid in full), the guard on both
engines, the server's tick and notice, the tests above and the words in
[BOARD_OPERATIONS](../BOARD_OPERATIONS.md#who-writes-a-board) and [RELEASES](../../workflow/RELEASES.md#the-switch). Then per schema
step: one line or none, one catalogue file, and the reviewer's read of the line.

**When it pays.** From the first schema step that lands once a release carrying the mechanism is
`current`, since only that release's server reads the record. Had it been there, 17 to 22 would each
have left the pages writing, and of the two 23s waiting, the first would and the second would not.
The release run after a schema landing still runs: it brings the server to the new schema, which
the next schema landing waits for. Whether a listed step still starts one at once is a question for
once it is built.

## B. This board moves at the refresh

This repository's board moves only at a release's refresh, by the release's own `tasks migrate`
under the writer rule ([the refresh](../../workflow/RELEASES.md#the-refresh)), as every other registered
project's does. The refresh's aside for this repository and main's move at its first command
(`crates/tasks/src/live.rs`) end. The pages then wait only for the refresh's own minutes, as every
other project's do: the boards moved, `current` pointed, the server's reload. And this board gets
the refresh's snapshot before each move.

From the landing to the refresh, though, main's build is one schema ahead of the board, and in
this repository every board command, every agent's and the land's, runs main's build. Either it
writes a board one schema behind, or it is not what writes this board.

**B1, main's build writes a board one behind.** The same rule turned round, for every writer here.

- What is declared: that a schema's builds write a board one behind correctly. That is true only of
  a step whose code writes and reads nothing it added until the board has it: each feature that
  comes with a schema checks the board's schema at its writes and reads, and its new tables stay
  empty until the refresh. The feature works on this board only from the release that carries it.
- Who checks it: the step's own tests, every changed write path run on a board one behind and on
  one at its schema, on both engines. No catalogue test can stand in: what must be proved is that
  the new code does not write what the old board lacks, which is in every path the step touched.
- What a wrong declaration breaks: a board command that writes what the board does not define
  fails, for every agent that runs it, until the refresh, which stops the pipeline, where a wrong
  line under A reaches only the server's own writes; or a write the older definitions take with a
  meaning the newer build changed.
- The writers involved: all of them here, `tasks`, `agent-send`, `tasks-copy`, `tasks api`, the
  land, the regenerator and the recorder, by every agent, for hours at each schema step.

**B2, this repository runs the release's board commands.** Its `just tasks` and `just agent-send`
run `current`'s build against the live board, as a project with no build of its own, and main's
build runs only on scratch boards and in tests. No build writes the board at another schema, so
nothing is declared and nothing new is tested for it. The cost is that a board change landed here,
a feature or a fix to a command that refuses, works on this board only from the release that
carries it: from a counted run's start to its release took 27 minutes to 16 hours in the runs
since 2026-11-16. The recipes and the launcher's way to the live board change, and the board's own
development loses its first user.

Either way, B changes what [board-schema-at-update](../../engineering/analysis/plugin-delivery.md#board-schema-at-update) keeps
for this board, which moves with main today.

## C. The server hands its writes to a build at the board's schema

Considered, not proposed. While the board is ahead, the server would hand each write to the
project's own `tasks` at the board's schema, here the main checkout's build, which moved the board,
through `tasks api` ([BOARD_API](../../engineering/BOARD_API.md#the-tasks-api)). Nothing would be declared, as every write
would be made by a build at the board's schema, and it would hold for every step, workflow-runtime's
too. But the api carries the owner's kind of write (a message, a move, a new task, an ask) and not
the clock's (the callback pass, the judging, the gives), which would stay paused; what the server
writes as the owner would be written by a program agents also run; the server would keep two ways
to write, each tested; and a project with no build of its own has none to hand to.

## Recommendation

A. It removes the window for the kind of step that has been most of them, six of the last nine, and
leaves the rest as now. Its declaration is one line per step, checked on the database by a test on
the step's own branch and on the code by its review, and failing closed: a step that says nothing
keeps the window. A wrong line reaches only the server's own writes, for as long as the board stays
ahead of it, though what those writes break depends on the rule it missed; and the one writer it
lets through is the server: every program that writes the board under its own name is refused one
ahead as now.

B removes the window for every step, but moves its cost onto everything else here: under B1 every
board command and every schema feature carries the board's schema for hours at each step, and a
wrong declaration stops the agents rather than the pages; under B2 every board fix waits for a
release. C needs no declaration, but it leaves the clock's writes paused, and it moves the owner's
writes out of the server into a program agents also run.

Without either, the cadence's rule stands alone: a release run started at once after each schema
landing, and the window until its release is `current`.

A is the way chosen ([its card](#schema-move-keeps-pages-writing)), and it is
built ([CHECKS](../../workflow/CHECKS.md#a-schema-step-that-keeps-the-symphony-writing)). Until a release
carrying it is `current`, and for every step not declared after that, a schema landing keeps the
window as above.

- [r] **schema-move-keeps-pages-writing** — After a landing that changes the board's layout (its schema), your pages save nothing, and the server sends no reminders to agents, until a release with that layout is on the machine: one release run, under 40 minutes when green, now that the orchestrator starts it at once. Most such changes only add tables of their own, which the server never writes: the last six did. Should the server keep writing through such a change? A: yes, when the change says it only adds tables of its own, a test checking that on its branch: the server keeps writing a board one layout ahead, and a change that touches anything it writes waits for the release run as now. B: this board moves only when a release brings its layout, as the other projects' boards do: your pages wait minutes, but here every board command runs one layout ahead of the board until then, and a board feature or fix landed here works on this board only from that release. C: as now, your pages wait one release run after each such landing. Recommended: A. Approve and say A, B or C in the note, or comment. [[analysis/schema-move-read-only-window#Recommendation]]
  [2026-11-19 08:05] agent — Posted for the board task `tools/no-read-only-window-after-a-schema-move`, the fourth way **schema-landing-then-release** proposed and left undecided. Nothing of it is built.

    **What A changes.** Each layout change says, in one line, whether it leaves the server's writes as they were; one that says nothing does not, and keeps the wait. A test on its branch refuses the line when the change touched a table that existed before, and its reviewer reads it for a rule in code alone. Only the server is let through: every other program that writes the board is refused one layout ahead, as now.

    **What a wrong line costs under A.** Caught on its branch when the change is in the database. A rule held in code alone, wrongly declared, would let the server's own writes skip it for one release run, such as a cancel the new rule forbids: tasks in a state the new build says cannot be, put right by a fix. Nothing is lost.

    **What it would have done.** 17 to 22, six layout changes from 2026-11-11 to 11-18, only added tables: none would have stopped your pages. 14 to 16 changed existing tables and would have. Of the two changes to 23 waiting now, `tools/durable-provider-attempts` would keep your pages writing and `tools/workflow-runtime`, which checks every write of a task, would not. A pays from the first change after a release that carries it; building it is a layout change of its own, with one last wait.

    **What B costs.** Either every board feature that comes with a layout change checks the board's layout at each write for hours, and a wrong line stops every agent's commands rather than your pages; or this repository's board commands run the release's build, as a project with no build of its own, and a board fix landed here waits for a release, 27 minutes to 16 hours since 2026-11-16.

    **Under every option** the release run after a layout change still runs, and a second layout change waits until a release serving the first is current.

    **Earlier verdicts checked.**
    - **schema-landing-then-release** (A) decides what follows such a landing, a release run at once, and leaves this way undecided by its own words: no option here changes that run.
    - **board-schema-at-update** (C) moves a board only once its writers are at the new layout, and keeps this board moving with main. A and C keep both; B ends the second, so it is asked here.
    - **live-board-schema-moves** (A) chose for two tasks and sets no rule.
  [2026-11-19 08:04] owner · approved — A
  [2026-11-19 08:06] agent — Your "A" chooses compatibility declared for each schema change: the server may keep writing one schema ahead only when that change preserves its writes; an undeclared or incompatible change stays read-only, and other writers keep their existing guard. We will carry that choice into implementation after this design is published. Two corrections to the proposal remain: a wrong declaration can violate data invariants, so no-loss or automatic repair is not guaranteed; and immediate release start does not bound the wait when verification or the refresh writer gate holds it. The immediate-release rule and the hold on a second schema change remain. Nothing is implemented yet.
  [2026-11-19 08:06] agent · resolved