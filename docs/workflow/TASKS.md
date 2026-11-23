# Task board

How several agent sessions share work, and how they, and the owner, talk
about it. Sessions may run in any worktree of a repository; the board is the
same for all of them.

This is the board's full reference. [Getting started](../operations/GETTING_STARTED.md) walks
a first task from filing to landing, and [the commands](../operations/COMMANDS.md) says which
command to reach for. The commands shown are the recipes `symphony init`
installs in a project, the default landing among them, `just land SLUG`
([the default landing](#the-default-landing)). A project with a landing policy
of its own (its verification, certificates and stamps, as the `land-approve`,
`land-stamps` and `land-request` recipes of the examples here) writes its own
`land` recipe, which replaces the default everywhere for it; Symphony bundles
no such policy.

What the owner sees of it in the app is told in two product specs:
[the task board](../product/BOARD.md) and [a task's page](../product/TASK.md).

## Where it lives

A PostgreSQL database (`tasks`) on the machine's own PostgreSQL, 18 or later
([the setting](../operations/PROJECT_FILE.md#board)), whose server keeps its data and its log in
the git common dir, `.git/symphony/tasks/`, so every worktree reaches the same
board, a claim is one transaction, and none of it shows up in `git status`. The
first `just tasks` makes the board there, and is refused with the setup guidance
on a machine without PostgreSQL. Whatever needs a board that is made starts its
server when it is not running: a `just tasks`, `just symphony` as it starts, a page
of the Symphony, which never makes a board. It starts detached, so it outlives
whoever started it, and that is also the restart: a server that was killed is
started again on the same data. A start says so in one line on stderr, as it
takes seconds. A board made before PostgreSQL was a new board's runs on SurrealDB
(namespace `sf`, database `tasks`, its address recorded beside its store) until
its store is moved ([POSTGRESQL](../operations/POSTGRESQL.md)).
The heartbeat hook never starts it. `TASKS_SERVER` (an address) and
`TASKS_DATABASE` reach another board, as the tests do. It is working state, not
history: `just tasks export` writes a readable snapshot to
`docs/history/tasks.md` when one is worth keeping.

Each project has a board of its own. A run's project is named by the nearest
`symphony.toml` from its folder up, never past the top of the checkout it is in;
with none, by the machine registry's entry for its repository's main checkout,
else by that checkout's folder name. Its board is that repository's, in its
common dir, under the same namespace and database. The Symphony serves each
registered project's board, and reconnects to one whose server restarted. A
project's heartbeat keys, sweep state and staffing lock stay in its common dir.

How the board is run is a document of its own, [BOARD_OPERATIONS](../operations/BOARD_OPERATIONS.md): the
schema contract between the builds that read and write a board and
[which build moves a live one](../operations/BOARD_OPERATIONS.md#which-build-moves-a-live-board), who writes it, its store,
server, snapshots and holds, what the command may write, and the recovery of a board that went
back or was copied. A program reads and writes the board through [its JSON APIs](../engineering/BOARD_API.md).

The command is the `tasks` binary (`crates/tasks`). In a project `symphony init`
set up, `just tasks` is the recipe it installs in `symphony.just`: the project's
build linked at `.target/bin/tasks`, else, from a linked worktree, the main
checkout's link, else `tasks` on `PATH`; it builds nothing, and says which link
to make when it finds none. The Symphony's own board clients find it the same
way. This repository's `justfile` has a `tasks` recipe of its own, which wins:
`tools/rs-bin.sh` builds the binary before a run once its sources or settings
changed, so the first command after such a change takes a minute or two, and
later ones run the build without asking Cargo ([the launcher](../../AGENTS.md#rust)).
On the live board it is the main checkout's build, from
whichever worktree it runs; a branch's own build runs only on a scratch board
(`--database NAME`). The heartbeat hook finds the build as the recipe does, and
builds nothing. The sweeps work finishing starts, and the Symphony's reap and
finish, run the main checkout's build found that way, never a linked worktree's.
There is no Python board adapter. The server is the pinned
SurrealDB at `symphony/surreal/VERSION/surreal` in the git common dir, or the binary
`SYMPHONY_SURREAL` names. In this repository `bash tools/surreal.sh` fetches it there,
checked against its pinned SHA-256 (the Rust tests fetch it the same way). A
project fetches the same tarball there, checked against the same SHA-256, by the
line `symphony init` lists among the steps left, which the board also says when
it is missing; a machine no build is pinned for puts SurrealDB's release binary
there. A new project's board is made by its first board command, `just tasks list`
say, which `symphony init` lists among the steps left too while the board is not
made: the Symphony only reads a board, and its board page says so, in the
refusal's words, in place of the board until a read answers.

| | |
|---|---|
| `just tasks …` | everything an agent does: `just tasks help` lists every command with a line on what it does, `just tasks help COMMAND` its arguments; a mistyped command, value or slug names the nearest, a mistyped flag the line to type, and a refused move or role the command that does it |
| `just tasks list [--all] [--mine] [--status S]` | the open tasks as a tree; `--all` adds the done and cancelled ones |
| `just tasks search QUERY [--status S] [--limit N] [--json]` | ranked full-text matches in task titles, briefs, slugs and threads; task rows with matching excerpts (default: 20); query capped at 1024 UTF-8 bytes and 32 terms (word/number runs and punctuation/symbols, after lowercasing) |
| `just tasks stats [--days N] [--json]` | where the pipeline's time goes: lead time, time in each status, bounces, tasks done per day and what sits longest ([Where the time goes](#where-the-time-goes)) |
| `just tasks status [--json]` | the board's health over the last hour, as its Symphony server recorded it: failed dispatches and sweeps, the slowest request, write latency ([Board health](../operations/BOARD_OPERATIONS.md#board-health)) |
| `just tasks asks [--all] [--json]` | what the owner waits on an agent for: the open asks, oldest first, each with its age and who holds it; `--all` adds those answered in the last day ([Asks](#asks)) |
| `just tasks asks close ID "WHY" --as HANDLE` | close an ask that needs no answer, saying why |
| `just tasks migrate` | move the board forward to this build's schema, or make it, and say which; a live board moves by it only from its main checkout (any checkout of a repository with none), and in a project whose main checkout holds no board source, or that has none, only by it; it moves nothing while a writer seen in the last week is behind the schema it would move to, naming each, unless `--leave-behind` ([Who writes a board](../operations/BOARD_OPERATIONS.md#who-writes-a-board)) |
| `just tasks database [hold WHY \| release [--of HOLDER]] [--as HANDLE]` | the board's database: whether it is held, what runs on its data, what a start or replay under way does, and the snapshots kept until the owner removes them; `hold` stops its server cleanly and keeps every board command and the Symphony from starting it, naming the hold, until `release` by its holder (or by another naming it with `--of`), which starts nothing |
| `just tasks schema [--accept VERSION]` | the board's schema and the highest its live board was seen at; exit 1, in the rollback's words, while it went back below it; `--accept VERSION` (`none` for a store with no board), the owner's, accepts an older board they meant ([A board that went back](../operations/BOARD_OPERATIONS.md#a-board-that-went-back)) |
| `just tasks snapshot [--list]` | a copy-on-write clone of the board's store, taken only while no server has it open; one whose write-ahead log holds writes is kept until the owner removes it, and the newest three of the others; `--list` names them ([Snapshots](../operations/BOARD_OPERATIONS.md#the-store-and-its-server)) |
| `just tasks callback [ADDRESS] [--chat] [--if-free] [--off] --as HANDLE` | register a native channel; addressed events start a turn directly (`just tasks help callback`) |
| `just tasks callback --session-url URL --as HANDLE` | give the session the web address its Go to session control links to; empty clears it ([Going to a session](#going-to-a-session)) |
| `just tasks pull --as HANDLE` | the whole PULL digest as HANDLE hears it now; a callback carries only what changed since the last PULL it received |
| `just tasks wait --as HANDLE [--timeout S]` | for a session with no channel: exits printing what a callback would have pushed it since its last read, once there is any; run it as a background command and again when it exits |
| `just agent-send HANDLE "text" [--as SENDER]` | message an agent directly through its registered channel |
| `just symphony` → **Board**, **Agents** | the board, each task's page and thread, and who is connected |

## Tasks and roles

A task is a slug (`sim/jump-departure`, the same slug the branch and commits
use), a title, a markdown body, a priority from `0` (most urgent) to `4`, and
optional links: `--link docs/SPEC.md#Heading` to the spec it implements,
`--depends` for tasks that must be done first, `--parent` for a larger task
or an instruction to an agent ([Subtasks](#subtasks)).

A cancelled dependency does not satisfy that requirement. Cancellation moves its
unfinished dependents to `blocked` and records the cancelled prerequisite on their
threads, with the status each held (`(was ready)`). Creating or moving a task with
a cancelled dependency blocks it too.
`next` excludes that work; PULL lists it as `dependency_blocked` with the reason,
including older tasks still in their previous stage. Done and cancelled dependents
keep their status. An orchestrator must replan the dependent work before it resumes.

`just tasks depends SLUG --replace OLD NEW [--as HANDLE]` points a task at a
successor; `--add DEP` and `--remove DEP` add or drop one, and each flag repeats.
It refuses a task the board does not hold, a finished task, a cancelled
prerequisite, a dependency already there or not there, and a cycle, and records
the change on the task's thread. A task a cancellation blocked that then waits on
nothing cancelled goes back to the status its block line recorded, or to `ready`
from `in_progress` when nobody executes it; a block line written before the
status was recorded sends it to `ready` (`in_progress` with an executor). A task
blocked by hand stays blocked.

`just tasks priority SLUG N [--as HANDLE]` changes an existing task's priority,
from `0` to `4`, and records the actor and both values on its thread. `next`,
`list` and the PULL digest use the new priority immediately; the change sends
no callback. A value outside that range or a task the board does not hold exits 2;
an actor it does not know, or one that left, exits 1, as every command refuses one.

The pipeline is `backlog → concept → design → ready → in_progress → review →
done`. The board shows the moving stages in one row, and where work ends
up (`done`, `blocked`, `cancelled`) in a row below it. A task
starts wherever it belongs (`--stage concept`, `--stage design`, or `ready` by
default). Work with no concept or design step goes straight to `ready`.
Claiming concept or design work keeps the task in its stage; the executor
moves it on when the concept or design is done. The card shows what waits in
each document the task links ("N waiting" undecided cards), for information
only: nothing waits on it.

Between `in_progress` and `review` the board draws **Build** and **Test**: an
in-progress task whose build or test run is going sits there until it ends, then
returns to its column. They are display stages, never statuses: no command moves a
task there, and a task in any other status (a task in review whose reviewer runs
its tests) keeps its column and shows the run as a chip. A run for a subtask (a
slice, or an instruction its agent holds) moves the card of the task it is under.
Runs come from the board's recorder (`tasks run`, which this repository's
`tools/cargo.sh` and recipes use) or
the project's own reader; until a machine reports either, the two stages are one
narrow rail that says so once. When a machine reports, the rail opens its Build and Test lists
([Machine build and test work](MACHINE_WORK.md#recording-builds-and-tests)).

**Review waiting on nobody is blocked.** A task that only waits on an approval,
with no work happening, says so by its status. A task
in review whose branch has every stamp it needs to land but ones nobody is
producing (the owner's approval, a dependency only that approval collects, a
hold its orchestrator names: the consuming project's stamp policy)
goes to `blocked`, with a line on its thread naming them. `just land`, `just
land-approve` and `just land-stamps` make that move when they find it, and move
the task back to `review` once one arrives: the owner's approval (the
Symphony's approve-to-land button, or relayed), a waiver, or a stamp someone
produces going missing. A role holder may move blocked work back to review, and
so may the owner; a command run by a session with no role on the task says the
move it could not make, and leaves it to the next run by a role holder. A
landing finishes work still held blocked as it does work in review. It
refuses work in progress, but finishes a task taken back to `in_progress`
after its last read of it, as main moved: that branch is on main, so the
landing tells its executor that further changes go in a new task. No other
command closes work in progress but a subtask's `done` and a close as covered by another
task's landing ([above](#a-task-another-landing-covers)). `landed` takes the statuses this landing
closes (`from`; review and blocked when it names none) and checks them against
the status its write reads; its answer names the one it found, and a close from
`in_progress` addresses the verdict to `@executor` in that write.

### What a task waits on

**A block names a thing, never a person.** `just tasks move SLUG blocked --on THING [--why TEXT]
--as HANDLE` says what the task waits on, THING one of `decision:NAME` (a card the owner
decides), `check:NAME` (a stamp or a run someone gives), `task:SLUG` (a task the board holds,
never itself) or `parked` (put aside on purpose), a name one word; `--why` is a few words, at most
120 characters. The command line refuses a block that names nothing, saying what to pass, and
`--on` or `--why` on a move anywhere else, and moves nothing: a card blocked on nothing is not
blocked. The block keeps its record on the task (`fields._board.blocked`: the thing, the words,
who, when and the status it was blocked from) in the move's own write, and, with no `--reason`,
posts `blocked: on THING; WHY` after the move's line. A task blocked already is named again the
same way: the record changes in place, by a session holding a role on it, and the task stays
blocked, its time in the status kept. Every write that takes a task out of blocked (any move, the
owner's, a dependency change that unblocks it, a land) drops the record. The library's
`Store::move_on` and `Store::hold_blocked_on` take the thing, as `tasks api`'s `move` and `hold`
take `on` and `why` (api version 15); without one they block as before and record nothing, and
the Symphony's own callers name theirs: the default landing's hold is `check:owner`, the owner's
approve of the tip ([below](#the-default-landing)), which the line reads as his decision. A cancellation's block is read from the
task's links instead, its cancelled dependencies.

An explicit `task:SLUG` block clears automatically once that task and every dependency are
`done`, using the same completion rule as `claim` and PULL, including approval before landing.
The task returns to its recorded stage; work from `in_progress` without an executor returns to
`ready`. A block without a recorded stage returns to `ready`, or `in_progress` with an executor.
Resuming work respects the slot cap and retries on dispatch when a slot frees. The move is
recorded once on the thread and reaches the current orchestrator through native STATUS/PULL
delivery. Dispatch also reconciles a completion whose writer stopped before delivery. Cancelled or
missing prerequisites never satisfy a task block.

The owner's word ends the two blocks that wait on the owner, the same way: back to the recorded
stage, under the slot cap, the move recorded once and told as STATUS. A `decision:NAME` block ends
once card NAME carries the owner's verdict that decides it, approved, rejected or dismissed, in an
owner's line dated in or after the minute the block was recorded, on any page's copy of the card:
main's documents or a branch's, so whichever checkout the owner's save wrote, main's or the
worktree of the task that posted it, committed or not. The owner's newest verdict there is the
card's: a question, or the card set back to pending, ends nothing. Its line names the card and the
verdict, `unblocked: the owner decided NAME (reject); back to in_progress`, so the executor acts on
what the card says a reject does. An owner's line since the block that decides nothing (a
message, a question) leaves it blocked and is posted once on the task's thread to
`@orchestrator`, with its card, stamp and words. A `check:owner` hold, the default landing's, ends
once the land's `owner` stamp stands on the tip of its branch (`refs/heads/SLUG`), read by its one
reader ([Needed stamps](#needed-stamps)): an owner's approve on the task's thread naming the tip,
`unblocked: the owner approved 1a2b3c4; back to review`, or the owner's approve of every card
the branch posted, committed, with nothing after it but the owner's lines and merges of the
default branch, `unblocked: the owner approved card NAME, at 1a2b3c4; back to review`; an approve
of another commit, or an owner's line no commit carries, ends nothing. The machine's one server
judges both on its board clock,
from the cards its pages hold and the board it read that tick (`crates/symphony/src/decided.rs`):
a save needs no hook, as the page that reads the saved document again brings the line to the next
tick, and a verdict written while no server ran, or saved by another build, is read at the first
tick that reads the documents. The move is fenced on the block the server read
(`Store::resume_judged`): a move or a block anew in between, the owner's or a session's, wins. A
private server, and a board no server reaches, leave them blocked until the one server runs.
Other check, parked and unknown blocks stay blocked.

**The line.** Each open task waits on one thing, said as `KIND · WHO · WHY` since the time it took
its status (`crates/tasks/src/waits_on.rs`), KIND one of:

- `working`: a session at work on it, live or stale (`stale` after its name), its executor, or in
  review its reviewer, or an executor of a slice under it;
- `decision`, `check`, `task` or `parked`: what its block names, a dependency still open, the
  stamps its landing lacks, or a review waiting on the owner's sign-off ([above](#tasks-and-roles):
  the cards it posted, or `sign-off` where a project's owner gate holds it with none). What waits
  on the owner is a decision, never a check: a block on `check:owner`, or a landing lacking the
  owner's stamp alone, waits on `decision`, named by his approve of the tip and never by him;
- `queued`: nothing missing and nobody to blame, as a ready task waiting to be staffed or one in
  review that waits only to land;
- `nobody`: what the board can name no one for, a lost or left executor (`executor X lost`), a
  review no reviewer holds, a dependency cancelled, or a block with no record (`no reason
  recorded`).

`just tasks show SLUG` prints it, `Waits: on KIND[: WHO] · WHY · AGE`. The listings send it as
each task's `waits_on`, a blocked task's `from` the status it was blocked from, and the board's
`orchestrators` (each session holding the orchestrator role on an open task, with its health) and
`unstarted` (the ready tasks no executor that is there holds, every dependency done): the wire
types `WaitsOn`, `Orchestrator`, `Tasks` and `TaskListing` (`crates/symphony/src/taskview/waiting.rs`).
When each task took its status is its thread's replay, read for tasks in one round trip
and kept, so only a task whose status or write moved is read again. Open and finished rows also
carry optional `review_rounds`: one round per replayed stretch in review, including an active
round, exactly as the task timeline counts them. Reviewer roles, stamps and review subtasks do
not add rounds. Zero means no review stretch; an absent count means no summary is available
(including an older server), never a guessed zero. Finished summaries stay cached until their
task changes or leaves the snapshot. Filtering and paging copy the summaries without more
history reads. The feed sends a changed known count in `task-fields`; if a previously known
summary becomes unavailable it replaces the task record to clear it.

**A block from before.** A task blocked by hand before a block named its thing has no record. It
is not blocked on the wire: the listings send it in the stage it stopped in, waiting on `nobody ·
no reason recorded`, and the board itself still holds it blocked, slots and moves alike. Naming
its thing (`move SLUG blocked --on THING`) sends it blocked again; no migration writes one.

**The waiting-column listing.** `/api/task-board?view=waiting` opts into the board's
blocked-in-place projection. Without it the endpoint keeps its lifecycle, holder, hierarchy,
sign-off and cursor behavior. It uses the same search and finished window as those listings:
`q`, `days=1|7`, `older=1`, `open=1` and `all=1`. `activity=1` places ordinary in-progress work
in Build or Test from the machine's current report; a blocked task stays in its original
lifecycle stage. `work=0` omits that report without changing placement.

A named block goes in `from`, or Ready when that legacy source stage is missing or invalid.
An absent or invalid block line becomes ordinary work in that same stage, with
`nobody · no reason recorded`; nothing writes the board. The existing `waits_on` line is the
classification: a named missing dependency can still read `nobody`, while `parked` needs no
name. Unknown wait kinds count as `nobody`. The projection has no Blocked or sign-off column;
combining it with `status=blocked|signoff`, `parent`, `holder`, `role` or `roots` is refused.

Within each column, named blocks come first in `WAITS_KINDS` order and oldest first within
that kind, then ordinary work oldest first, then parked work oldest first. Finished work is
newest first. Search selects matching rows but does not replace this order with relevance.
Equal times use the numeric slug order (`item-2` before `item-10`), so every tie is stable.
`wait=KIND` selects one of `WAITS_KINDS` across stages as one list in that same order, with no
stage partition; finished tasks have no wait kind and are left out. An unknown kind is refused.
`status=STAGE` further selects a column's rows.

The ordinary `counts` include each column's ordinary and blocked tasks, and `lifecycle_counts`
keep their normalized lifecycle statuses. Both are complete after search, window and wait-kind
selection, before a status selection or pagination. The optional `waiting` response holds:

- `kinds`: `[kind, count]` for each of `WAITS_KINDS`, in its order, counting open tasks after
  search and window selection but before `wait`, so the other filter chips keep their totals;
- `columns`: `{status, ordinary, blocked, oldest}` for every nonempty column after `wait`,
  before `status` and pagination. `oldest` is the earliest status-entry timestamp in seconds
  since the epoch, so the client calculates an age without another read.

`counts=1` sends no rows, with the same totals. A page is 30 rows; `rows=N` can request more
but never fewer, and `all=1` requests all matching rows. `offset=N` skips rows after any cursor.
The opaque `next` cursor records the complete ordering key, not an offset: stage (empty for a
wait-kind list), ordinary/blocked/parked rank, block-kind rank, status-entry time (negated update
time for finished work) and slug. It is passed back only with the same view, search, window,
activity, status and wait kind. A malformed cursor or one from the default view is refused.
An empty page has no cursor. The task board opts into this projection for its stage columns and
wait-kind strip; other listing callers retain the default view.
On a board a custom workflow holds tasks on, this projection puts a held task in its phase's
column (`phase:ID`, which `status=` selects in this view alone), `waiting` adds the active
generation and the open tasks by state, and `wait=state:ID` lists a state's
([WORKFLOW](WORKFLOW.md#the-app)); the default view counts and selects a held task by its
status.

The default listing retains its legacy **Waiting for your sign-off** selection. The waiting
board keeps these tasks in their own stages, with their named decision on the card, rather
than drawing a separate lane. The legacy selection needs all of:

- the task is not `done`, `blocked` or `cancelled`, and someone executes it;
- its body or thread names a card slug whole (a card named `x` is not
  named by a mention of `x-2`);
- that card is still undecided in a document the task links: the cards a
  document page counts as "N decisions waiting" (`docs_tree.pending`).

Deciding the card removes the task from that selection. Work nobody holds, or that
posted no card, is not selected however its documents stand.

A project's plugin that answers a gate named `owner` for the task ([Gates](#gates))
decides in that rule's place: the open, executed task is in the lane while that
gate is held, card or none, and out of it while the gate is met, waived or
missing. The lane reads the answer the server keeps for the task's status and
tip and never asks the plugin for it, so only a read of the task's gates (its
page) starts the plugin's work. Only a known gate kept for that task decides:
with no answer kept for it as it is now (before its page is read, after a new
tip or a new status), an error kept, an answer with no `owner` gate or one
answered `unknown`, the rule above holds, so the lane errs toward showing the
owner what may wait on them. The lane is read again when the board, a message,
a waiting card or a kept answer changes.

**`done` is reached only by a reviewer's `approve`, a landing, for a subtask its executor's `done`, or a close as covered by another
task's landing ([above](#a-task-another-landing-covers)).** A session
that ever executed a task, or a work subtask of it at any depth, gives it no verdict, whatever
role it holds since. Nor does a session that requested the work: one that
orchestrates the task or ever did, or that filed it. The board refuses it the reviewer's role
and the verdict at once, naming the command a fresh reviewer takes the role with, by the rule
the land goes by ([below](#the-default-landing)), so no approve leaves a task done that the land
then refuses. A reviewer judges the task itself: it takes the reviewer's role on `SLUG`, its
verdicts are `SLUG`'s review rounds ([Gates](#gates)), and a later round is the same reviewer's,
resumed, with no subtask ([Subtasks](#subtasks)). An instruction subtask (`--instruction`: a
question, an errand) puts nothing on the task's branch, so its executor is not counted: one
that claimed an instruction under `SLUG` may still give `SLUG` its verdict, unless it executed
`SLUG` or a work subtask of it too.
Work on the branch is a slice, filed without `--instruction`. `done` is left only by an approved task no landing has closed and whose tip the
default branch does not hold, back to `review` by its executor, reviewer or orchestrator,
for a fresh approve of a tip the approve does not cover
([below](#the-default-landing)); the move itself tells `@executor` and `@reviewer` on the thread, as
a finished task's status is followed no more, so a reopen edited into the board by hand tells
nobody. A move of a done task the board refuses names that move
back to review only while the approve stands; once a landing closed the task, it says what
the task still needs goes in a new one. Several sessions
can hold one task at once, one role each:

| role | how | may |
|---|---|---|
| orchestrator | `just tasks role SLUG orchestrator`, or `just tasks create` by an orchestrator session | coordinate: spawn the others, follow the thread; close it as covered or cancel it; one a task |
| executor | `just tasks claim SLUG` | do the work, move it to `review`; close it as covered or cancel it |
| reviewer | `just tasks role SLUG reviewer` | give the verdict: `approve` (done) or `changes` (back to the executor); one a task: another is refused, naming it, until it is lost or left, or joins beside it saying why (`--beside WHY`, a post of its own after its join line; `just tasks launch --role reviewer` takes it too) |
| reserved | `just tasks reserve SLUG` | hold it for later, for an hour, so nobody else claims it |

A branch reaches `main` only through `just land`: the Symphony's default
([below](#the-default-landing)), or the consuming project's own, whose recipe replaces it.
Under a consuming project's landing policy its
reviewer gives one verdict, not two: `just land-approve BRANCH --tip SHA` with
the full commit it checked, from its own
session (`TASKS_SESSION`), holding the reviewer role on the branch's task,
recorded on the branch's patch (`--adversarial` for the second reviewer of a
tier 3 change, which takes the role beside the first with `--beside WHY`). The landing then finishes the task in review for it: no board
`approve` is needed. A branch with no task gets `review/BRANCH` from
`just land-request BRANCH`, and the session that ran that (or the task's
executor) is the one that lands it.

### A task another landing covers

A task whose fix landed with another task's branch has no branch of its own to land, and can
still be closed. Its executor or its orchestrator, the session holding that role on it, closes it
in any open status with no reviewer's verdict:

```sh
just tasks done SLUG "what came of it" --covered-by OTHER --as HANDLE
```

- **Covered by a landing.** OTHER is done and landed: its last verdict starts `landed `, its first
  line names a commit after its first ` at `, and the default branch holds that commit. The
  default land writes `landed SLUG on DEFAULT at SHA`; a project's own land, through
  `Store::landed`, may say more after the commit (`landed BRANCH on main at SHA; stamps counted:
  …`). A task in review, done by an approve no landing closed, or landed by a line naming no
  commit covers nothing, and the refusal says which.
- **No work of its own.** It refuses, naming what it found (the first five and how many more),
  while the task has work the default branch lacks where the board records its executors working:
  commits on the branch named as its slug, on the branch a launch of its executor recorded
  (`tasks launch`, `restore`), or on the branch an in-process executor's binding places it on (the
  checkout its last tool call ran in, as `just tasks agents` shows it); and, in such a linked
  worktree still there, commits at its HEAD, a detached one too, or changes no commit has. That
  work needs its own review and land; a branch the default branch holds whole is no work of its
  own. It does not read the main checkout's changes, which are every session's, an in-process
  executor's earlier checkouts, the binding of a session that has left the board, stashes, or
  branches under other names: the closer checks those.
- **Said on the thread.** Its line, the task's verdict, names the covering task, the commit and who
  closed it: `covered by OTHER, whose landing at SHA holds its work; closed by HANDLE: NOTE`. The
  task is done, and the orchestrator hears it as it hears a verdict. The task page's review gate
  shows it waived, `Covered by OTHER, landed at SHA`, its rounds before it as its past: never as
  approved, nor as landed with no review verdict.
- **No approve.** A covered close is not a verdict a landing goes by. Being the task's last verdict,
  it leaves no approve standing, so `just land SLUG` refuses it (`SLUG is done with no approving
  verdict from a reviewer`, or `SLUG was closed after its approval: covered by …` where an approve
  came before it and the task went back to review since), and it never goes back to review. Who may
  approve work is unchanged: a fresh reviewer, by the rule above.
- **Read in the repository.** As a move back to review does, the command line and the API read the
  repository first (`crates/tasks/src/covering.rs`), refusing from outside a checkout of it; the
  board itself checks the roles, the statuses and the landing's line (`Store::close_covered`). A
  refused `done` of a top-level task, and a refused move to done, name the command.

Work that is no longer wanted is cancelled instead, by its executor or its orchestrator, in any
open status: `just tasks move SLUG cancelled --reason "why" --as HANDLE`. Once work has started
(in progress or past it, or with an executor), only a session holding a role on the task moves it;
the owner cancels any open task. The reason is a post of the mover's right after the move's own
line, `cancelled: why`, in the move's one write; any move takes one (`blocked: waits on …`), and a
reason with no words is refused. A move to blocked names what it waits on as well (`--on THING`,
[what a task waits on](#what-a-task-waits-on)).

## Talking on a task

Every task has a thread, and the thread is the way to reach whoever is on the
task, whatever worktree or machine it runs in:

```
just tasks say sim/jump-departure "the dock ring is at 900" --to @reviewer
```

A long line, such as a request for review with its evidence, is written as a file
in the project's artifacts folder under the task's slug (`artifacts/SLUG/` where the
project names no other), which git ignores and the sweep sets aside with the
worktree, never a file the worktree would hold untracked or commit. It is posted
with `--file PATH` in place of the text (`--file -` reads stdin), so no quote,
backtick or `$(…)` in it meets a shell; the board trims its ends as it trims any
line, and a line given both ways, or neither, is refused. `--file PATH` is plain words,
which the allow rules pass in a Claude or a Codex session alike; stdin fed by a
heredoc is a redirection, which no Codex rule matches.

`--to` takes a handle, a role (`@executor`, `@reviewer`, `@orchestrator`,
`@reserved`), or `owner`. Addressed messages reach the matching native callback
as plain `MSG` event lines. Without an address, an agent's post stays on the
thread; the orchestrator reads it in the app or with `just tasks show SLUG`.
Verdicts reach the executor and orchestrator. The owner's word, its posts, its
verdicts and its moves (the thread's `owner moved it FROM → TO`, which a channel,
`just tasks wait` and the inbox carry as a `MOVE` line, never again as a
`STATUS`), reaches everyone on the task and the task's orchestrator, with no
orchestrator relaying it. On a task no live orchestrator holds it reaches every
orchestrator too: one nobody holds (none on it or above it, or only a lost or left
one, in the interval before the board gives it one, [Tasks and roles](#tasks-and-roles)), its copy marked `[unowned: no orchestrator holds it]`, and one whose nearest
holder is stale or of unknown liveness, marked with that holder,
`[unowned: no live orchestrator holds it: HANDLE (stale)]`; that holder hears it too,
unmarked (`Store::inbox`, `crates/tasks/src/store/messages.rs`). Only a live holder
keeps it to itself. The owner's cancel adds `; it moves no further: stop work on it`
to its `MOVE` line, so the executor and reviewer stop at once (the owner's `done` is
an approve verdict, which says no more). The thread keeps every line as written. Post milestones only, address what someone must act on, never
acknowledge a message, and never repeat a post on a subtask and its parent.
A session without a channel keeps `just tasks inbox`.

In the Symphony an agent's post or verdict, a task's brief and an ask's answer are drawn as a
document's body is: bold, italics, inline code, links, lists, tables, fenced code, pictures,
`[[wiki links]]`, a card named in bold and `[[name:argument]]` components all show. Raw HTML
does not: `Vec<String>`, `<TokensPart key=x>` or a `<details>` named in a post is the words
typed, drawn once, never an element or a notice of unsupported content, as in a card's note
(`convert_post`, `crates/symphony/src/render/markdown.rs`). The owner's words are the text typed,
but for a line naming a file they attached, which shows the file.

The owner reads the board and every thread in the Symphony, sends messages
to anyone on a task, changes its priority from p0 to p4 and cancels open tasks.
Cancellation asks for confirmation; done and cancelled tasks cannot be cancelled
again. The board records these writes as `owner`. In the shared app, cancellation
also closes the task's sessions and starts the worktree sweep, as the CLI does.
An agent message addressed to `owner` rings the app's 🔔. Creating, claiming,
other moves and verdicts go through agents. The app's **+** files a task from a
line the owner writes, in `concept` under `inbox/`, for an orchestrator to brief
([REVIEW](REVIEW.md#asking)); anything else the owner says that waits on an agent
is an [ask](#asks).

## The default landing

`just land SLUG --as HANDLE` (`tasks land`, `crates/tasks/src/land.rs`) lands the
task's branch, named as its slug, on the default branch. `just tasks claim` says so of a top-level
task (a slice works on its parent's branch, and an instruction subtask's slug is none), an approve
of one says that the task is done on the board but not on the default branch until it lands, and
the line that lands it (or the project's own `land`), or, where the project
[delivers by pull request](#a-land-delivered-by-a-pull-request), that the host merges it, an
approve of a subtask that its work lands with the task it is under, and a land
with no branch so named names the branch whose tip is the commit the approve names, and the rename
(`git branch -m BRANCH SLUG`). It is the convention a project gets without configuring anything, and a lower level
replaces a higher one: a project's own `land` recipe replaces it, everywhere. It runs only as `just land`, whose recipe sets `SYMPHONY_LAND`: `tasks
land` without it refuses, so `just tasks land` run by itself never lands. The marker is a
guard against accident, not the gate: anyone can set an environment variable, and what
lands is decided by the checks below, every stamp the task needs standing on the tip, a fresh
reviewer's approve among them where it needs review. So `symphony init` allows `just land` with no permission prompt, and writes no ask
rule; one an older init wrote, for the marker or for `just tasks land`, a run removes. A
project's own `land` recipe gets the same allow rule, and its own landing policy decides
what it lands. A task a custom workflow holds lands by its graph alone, through the prepare and
finalize edges of the adapter `default_land`; its approve is read from the workflow, the head it
read being the `head` input kept on that transition's receipt and never a verdict message, which
a workflow's approve writes none of ([WORKFLOW](WORKFLOW.md#external-effects)). An orchestrator lands an approved task without asking the owner. In a project
whose justfile runs a `land` of its own (`just --show land` is not the Symphony's),
`tasks land` refuses by any route, naming that justfile. A land is its merge, its move,
its push and its close: it runs no generator and no sweep, and nothing slow is on its
path or behind its lock, since every merge could make a slow run start again, and a lock
or queue around one would hold the whole pipeline back
([its card](CHECKS.md#land-tests-changed-packages)). One land of a
repository runs at a time (`land.lock` in `symphony/tasks/`), the lock held for those
seconds and its check; a second waits its turn and says whose, with no timeout,
since the lock is freed when its holder's process ends however it ends, and a signal
stops the wait. The settings are read from the default branch as the main checkout
has it committed, never from the branch landing, so a branch cannot change its own
check, nor what counts as one (`check_paths`, below). A land of this repository runs no check either (`[land] check = false`) and
takes seconds; what the client distribution needs, the regenerator it starts makes
afterwards, and runs the check, the docs audit, on what it made ([below](#the-regenerator)).

- **What it needs.** The land goes by the stamps the task needs ([needed
  stamps](#needed-stamps), `crates/tasks/src/land/needs.rs`), read by the board's own readers:
  its newest assessment's, on the tip it read; on another tip, or from the brief alone, those
  and the catalogue's defaults; with none, the defaults, `review` alone in the Symphony's
  catalogue, so a task nobody assessed lands as it did before stamps. Where it needs `review`,
  the task is done by an approve the checks below go by. Where it does not, it lands from
  review or blocked (its executor's move to review says the work is ready, and the landing
  closes it with no verdict) or done by an approve standing; in progress or ready it is
  refused to `@executor`, to move it to review. Each other stamp is read at the tip: `tests`
  the task's recorded runs there, by [what clears what](#what-clears-a-red-run), `pictures` a post naming the tip with a picture, `owner` the owner's approve of the
  tip, or of every card the task's branch posted ([Needed stamps](#needed-stamps)). A missing one refuses, naming each and who gives it: `tests` or `pictures` to
  `@executor`, at that tip; `review`, or an assessment of another tip, to `@orchestrator`, to
  start an assessor or staff a fresh reviewer; one waiting on `owner` alone, to
  `@orchestrator`, holds the task blocked from review (`Store::hold_blocked_on`, keeping its
  slot, on `check:owner`, the owner's approve of the tip, which reads as his decision: [what a
  task waits on](#what-a-task-waits-on)), or leaves it done by its approve, until the owner approves the tip
  on the task's page, or the cards its branch posted with their lines committed, which returns a
  held task to review by itself, told as its STATUS. A stamp a [gate decision](#gate-decisions) waived for the tip is not
  needed there; a raise one gave must be carried in its file at the tip. A land of a task that needed no review says on its landing line the stamps and the
  whole tip they stood on (`landed SLUG on main at MERGE; its needs (tests) stood at SHA`),
  the commit it vouched for, as an approve names one.
- **Approved by someone else.** Where it needs review, the task's last verdict is an
  `approve`, which left it done, given by a fresh reviewer: never a session that ever executed the task or a
  work subtask of it (claimed it, whatever role it holds now), nor one that orchestrates or
  ever orchestrated it (joined as its orchestrator, or was handed the role or took it
  over), nor its filer, which requested it (RULES.md `workflow/reviewers-not-the-owner`).
  The board refuses each of those verdicts itself, by the same rule (`Held::unfresh`): a
  former executor's, a work subtask's included, its orchestrator's, a former orchestrator's and its
  filer's, which it refuses the reviewer's role as well. An approve the land refuses for whose it
  is is one an older board took; the executor of an instruction subtask is none. Stored marks give
  no approval privilege, and completed tasks and their recorded verdicts are left intact.
- **What counts as a check.** A change to a test or check needs a fresh reviewer according to
  the catalogue's `review` guideline. `just tasks stamps SLUG` gives its assessor the paths the
  branch changes that `[land] check_paths` names, a list of
  entries read as `.gitignore` lines (a final `/` a folder, a `/` at the start or in the
  middle anchoring it at the root, `*` and `?` within one part of a path), `PATH [TABLE]`
  naming a table of a TOML file, dotted for one inside another, which counts only where its
  value differs. With none written it is `["tests/", "fixtures/", "testdata/",
  "snapshots/", "*_tests.rs", "tests.rs", "*.snap", "Cargo.toml [lib.test]", "Cargo.toml
  [lib.doctest]", "Cargo.toml [lib.harness]", "Cargo.toml [test]", "Cargo.toml [bench]",
  "Cargo.toml [bin]", "Cargo.toml [example]", "Cargo.toml [package.autotests]",
  "Cargo.toml [package.autobenches]", "Cargo.toml [features]", "Cargo.toml
  [dev-dependencies]", "/justfile", "/symphony.just", "/tools/", "clippy.toml",
  ".clippy.toml", "rustfmt.toml", ".rustfmt.toml", "/.cargo/", "/Cargo.toml
  [workspace.lints]", "Cargo.toml [lints]", "/symphony.toml [verdicts]", "/symphony.toml [land]", "/symphony.toml [releases]",
  "/crates/symphony-build/src/commands/size_budget.rs",
  "/crates/symphony-build/src/commands/crates_check.rs",
  "/crates/symphony-build/src/release/crates.rs", "/crates/symphony/src/docs_audit.rs",
  "/crates/symphony/src/docs_audit/", "/crates/symphony-build/src/release.rs",
  "/crates/symphony-build/src/release/checks.rs",
  "/crates/symphony-build/src/release/steps.rs", "/crates/tasks/src/land.rs",
  "/crates/tasks/src/land/", "/crates/tasks/src/stamps.rs", "/crates/tasks/src/stamps/",
  "/crates/tasks/src/red.rs",
  "/crates/tasks/src/store/work/red.rs"]`: the tests and what they read, the Cargo tables
  that say which tests run and how (a `[[test]]`, `[[bench]]`, `[[bin]]` or `[[example]]`
  target whole, its `test`, `harness` and `required-features` with it, and the features, whose
  defaults decide what `#[cfg(feature = …)]` and `required-features` leave out of a test run)
  and what tests alone build with (`[dev-dependencies]`), the recipes and the scripts and budget under
  `tools/` they run, the lint and format settings, the project file's tables that say what
  checks, lands and releases, and the checks' own code: the size budgets, the crates check, the
  docs audit, the release run's checks and their steps, this land with its list, the stamp
  catalogue and readers, and the red marks. A project's own list replaces it. Beside the list a Rust file counts where
  the diff reaches a test-only module (its file saying `#![cfg(test)]`, declared under a
  test's attribute, by `#[path]` too, or declared by a test-only module), an item a test's
  attribute marks inline in a source file (`#[test]`, `#[bench]`, one whose last name says
  `test`, or a `cfg` naming `test` but as `not(test)`, `#[cfg(any(test, …))]` among them),
  from the first attribute of its run to what ends the item in the file's code, its
  comments, strings and characters left out, or a line added or removed that sets a lint's
  level. What it cannot tell counts as a check: a file whose code does not lex, or where a
  marked item runs unclosed to the end, counts whole, as does a module such a file declares,
  and a path git fails to read or a branch with more than one merge base makes the changed
  checks unreadable. What it does not see is product: a module
  declared inside an inline module is looked for beside its file, and a test-only file named
  only by a macro or a build script, or a lint set in a file no entry names, is product. The
  same list is the fact `just tasks stamps SLUG` gives an assessor (CHECK PATHS), whose
  `review` guideline keeps a fresh reviewer for a fix that changes one. The release run
  (RULES.md `workflow/release-reds-first`, [RELEASES](RELEASES.md#the-release-run)) and the
  regenerator (`tests/main-red-regenerate`, `tests/main-red-check`) file their reds with no
  fast-fix mark: what a red's fix needs is its assessor's, as any task's.
- **Covering the tip.** The approve names the branch's tip: a reviewer approves as
  `just tasks verdict SLUG approve "FULL_SHA: why"`, and the land goes by the named
  commit over any commit time. An approve naming none of the branch's commits covers
  each commit made before the verdict and before the task last went to review; the
  landing's line on the thread then says it judged by time. A commit after the named
  one, after the verdict, or during the review without a named tip refuses, naming it.
- **An approve over one merge.** An approve naming a commit stands at a tip that is one
  merge of the default branch into that commit, where the merge settles conflicts in
  documents alone (`crates/tasks/src/land/carried.rs`): the merge an executor makes when a
  land conflicts in a document, which no reviewer reads again. Three things hold of it. The
  tip is a merge of two parents, the first the commit the approve names and the second a
  commit the default branch holds, with nothing on top. Every path git's own merge of those
  two leaves in conflict (`git merge-tree`, by its status and the paths it lists, whatever
  the conflict: both sides changed, one deleted, a mode) is a document: a Markdown file
  (`*.md`) the tip holds as a regular file (mode `100644`, no symbolic link or submodule)
  that instructs no agent, so none under a `skills` folder and none named `AGENTS.md`,
  `CLAUDE.md` or `RULES.md`, where a wrong settling misleads every agent and no test says so.
  And the tip differs from that merge's tree in the conflicted paths alone. So what nobody
  read is the settling of a conflict in a document: every other path, a document that did
  not conflict among them, is as git merges what the reviewer read with what the default
  branch landed, which is what the land itself makes of a tip. The land then needs the
  task's `tests` green at that tip whatever its needs name, and says what it went by: `land
  SLUG: approved by HANDLE at TIME, naming A, which stands over the one merge T of main at M
  into it, which settles conflicts in documents alone (PATHS) and is git's own merge of the
  two elsewhere; its tests are read at T`, the landing's line on the thread adding `its
  approve of A stands over the one merge T of main into it`. A merge that settles a conflict
  anywhere else (code, a file one side deleted, a guide, the rules, a skill) or changes a
  path that did not conflict, each named with its paths, a commit on top of the merge, a
  merge made on another commit than the approved one or of a commit the default branch does
  not hold, and an approve naming no commit each go to the reviewer as before. The other
  stamps read at the tip are as they were: a picture's post and the owner's approve name the
  merge, and an assessment that read the approved commit is of another tip there.
- **No owner's answer left behind.** A worktree at a commit the land brings (on the branch,
  on another branch or detached at one of its commits) whose documents hold an owner's dated
  line under a card that no commit has (the owner's answer on a card only the branch holds,
  which the Symphony saves into that worktree's register:
  [REVIEW](REVIEW.md#one-app-every-worktree)) refuses the land, naming the worktree, its
  branch or commit, and its files: a land of the branch's commits would leave the answer
  behind. The documents are those in the folders the project file's `[[entrypoints]]` serve
  them from (`docs/` with none written), and a line counts per card against the copies the
  worktree's HEAD and the tip hold of the documents it changed, so a line a regroup only
  moved is none. It is said to `@executor`, which commits it on the branch (from a worktree
  not on it, copying the line onto its card there) and has the new tip approved, as for a
  commit after the approved one. A worktree git cannot read refuses the land to the one landing alone, as
  it may hold the answer; the land runs again once git reads it. The answers are asked again
  once the merge is checked, before the default branch moves, so one saved meanwhile refuses
  as well. One saved while the default branch moves is said to `@orchestrator` once it moved
  (`land SLUG: PATH holds the owner's answer on CARD saved while SLUG landed, which main does
  not hold`), to copy onto that card on the default branch and commit there; the sweep keeps
  that worktree meanwhile.
- **Back to review.** An approve that does not do (someone else's to give, or not
  covering the tip) sends the task back to review, said to `@orchestrator` or
  `@reviewer`. A conflict in documents alone (every conflicted path a document, as above)
  under an approve that names the tip is
  said to `@executor`, which merges the default branch into its branch once, settling
  the conflict in those documents and changing nothing else, runs its tests at the new tip
  (the task's by its branch, done as it is, with nothing named:
  [MACHINE_WORK](MACHINE_WORK.md#recording-builds-and-tests), "For which task")
  and lands it again: the task stays done, its approve standing over that one merge
  ([above](#the-default-landing), "An approve over one merge"), and a move back to review
  would ask for a fresh one. Any other conflict, one under an approve that names no commit
  or stands over a merge already, or a red check on the merge of the tip as
  approved, is said to `@executor`, which merges the default branch into its branch and
  fixes it there, then moves the task back to review itself (`just tasks move SLUG
  review`) for a fresh approve naming the new tip, and the land runs again. The one done
  task that may go
  back is an approved one no landing has closed whose tip the default branch does not
  hold (one landed or merged by hand stays done, and what it still needs is a new
  task), moved by its executor, reviewer or orchestrator; the command line and the API
  read the repository for that, and refuse from outside one. The board then says so to
  `@executor` and `@reviewer`, in their inbox and PULL digest though their panes are
  closed, and with no live executor `@orchestrator` hears it, which restaffs the
  executor or reviewer the task needs. Either move, the land's or the command line's,
  starts `[board] on_review` as a move to review does.
- **A release candidate's red.** While the release run holds a candidate
  ([RELEASES](RELEASES.md#the-release-candidate)), a red of it is fixed on a branch started
  at the local branch `release-candidate`, which merges that branch again before review and
  never merges main before its land, its approve, where it needs review, naming the full tip,
  and it lands by this same land. Nothing in the land changes for it: its `[board] on_land`,
  the release run's `--due`, reads the land afterwards and moves the candidate to the newest
  commit of the branch an approve named, or the tip the landing line says its needed stamps
  stood on, that holds the candidate and the fix alone. A conflict goes back to `@executor` as
  above, who merges the default branch then and lands again on its approve where the merge
  settles documents alone, or asks for a fresh approve; the candidate keeps
  the commit approved before that merge, and one that needed no review has none, and drops
  it. A red whose fix landed with another
  red's task is closed as covered by that landing ([above](#a-task-another-landing-covers)),
  which answers it once that land advanced the candidate; one covered by any other landing has
  its fix on main and not in the candidate, which is dropped. A red fixed outside this
  repository is answered by its task cancelled, saying where; one done otherwise waits for its
  land. Where a pull request delivers it ([below](#a-land-delivered-by-a-pull-request)), a fix
  that changes what the client is built from is the one branch that merges main before its land:
  once from a second head, after the head that commits its own outputs is approved
  ([RELEASES](RELEASES.md#the-release-candidate)).
- **Merged and checked.** In a worktree of its own (`land` in the project's `[agents]
  worktrees`, else `symphony/tasks/land/worktree`), never the main checkout: one `--no-ff`
  merge commit, "Merge branch 'SLUG'", committed through the project's git hooks, those git
  runs on a merge committed after `git merge --no-commit`: `pre-commit`, `prepare-commit-msg`
  and `commit-msg` (`pre-merge-commit` is `git merge`'s own; its sample runs `pre-commit`), from
  the repository's hooks folder or `core.hooksPath`, which, relative, names the hooks the merge
  holds. A hook that refuses refuses the land as a red check does, naming the hooks git found
  (`the project's git hook pre-commit refused the merge of SLUG into main at SHA`). Then
  `[verdicts] check` on it, unless `[land] check = false`. What the hooks and the check print
  goes to `symphony/tasks/land/SLUG.log` (the slug's `/` written `%2F`, its `%` `%25`, so no
  two slugs share a file). This repository turns the check off: its check,
  the docs audit, is evidence at a document change's tip, run on main's tip by the
  regenerator a land starts ([below](#the-regenerator)), and part of the release run's full
  verification ([CHECKS](CHECKS.md#checks-with-no-reviewer)).
- **Generated paths.** A generator's `outputs` are the paths it owns: the path itself
  and, for a folder, what is inside it (`gen/` and `gen` own `gen/a`, never `gen-x/a` or
  `gen.txt`). What they hold is the regenerator's, made of main's own sources, so the
  merge keeps main's side of every one of them the branch changed, in a conflict or
  not, and commits, its second parent still the approved tip, so no new approve is
  needed and no branch's own build of them (its client distribution) ever lands. The
  thread's line before the move, the record and the landed line name those paths (`its
  generated paths (PATHS) taken from main, for the regenerator to make`). A conflict
  anywhere else, alone or beside generated ones, refuses as any conflict does. What
  `origin` gained there is main's own (a regenerator's commit pushed from elsewhere),
  kept but where it conflicts.
- **Moved.** The main checkout, on the default branch with no staged or tracked edit,
  fast-forwards to what passed, so the default branch's new tree is the tree checked.
  Before it moves, the land records the landing (the task, the tip, the merge, the
  commit checked and the generated paths it took) in `symphony/tasks/land/SLUG.merged` and on
  the thread (`land SLUG: merged TIP as MERGE, checked at SHA; moving main there`, with no
  `checked at` where the land runs no check). The
  fast-forward runs in a process group of its own, which no terminal's signal reaches
  and the land forwards none to, within the land's timeout, so it is never stopped part
  way but by that; `symphony/tasks/land/moving` names the move while it runs. Where the
  default branch is once git ends says what happened, whatever git answered: at what
  passed, the move happened and the record stays; still where it was, the move did not
  happen, the record is dropped and the land restores what git half-wrote in the main
  checkout, saying each path: one the two commits differ in whose index entry and file
  are each as before or as after the move is put back as main has it (`restored PATH as
  main at SHA has it`), one main lacks is removed (`removed PATH, which main at SHA
  lacks`), and one changed otherwise is left as found (`left PATH as it found it`).
  When the default branch moved meanwhile, the merge, its hooks and the check run again on
  its new tip; a red then is said on the thread to `@orchestrator`.
- **Pushed.** Unless `[land] push = false` or the repository has no `origin`, the
  default branch alone is pushed, no other branch and no tag, never forced: what
  `origin` gained since is fetched first, merged and checked the same way, and the
  push names the commit checked (`SHA:refs/heads/main`), never what the branch holds
  by then. A push that fails leaves the landing done here and says so in its line,
  with the command to retry; a merge with `origin` that conflicts is said to
  `@orchestrator` too.
- **Closed and swept.** `landed` writes the line `landed SLUG on main at SHA`, with the
  generated paths it took from main, the tokens the task and its subtasks took where the
  board recorded any (`; 71.6k tokens in 6 calls (executor 51.5k, reviewer 20.1k; round 1
  51.5k, round 2 20.1k)`, [Where the tokens go](#where-the-tokens-go)) and whether it
  pushed, and closes the approved
  task, which stays done; a task sent back to review since stays there. The land then
  starts the sweep detached, which removes the task's finished worktrees after it, its
  output in `symphony/tasks/sweep.log` (`land SLUG: its finished worktrees are swept in the
  background`), and a project with `[land] generators` has the regenerator started
  ([below](#the-regenerator)). The land waits for neither, and neither runs under its
  lock; while a regenerator runs, the land starts none and signals none.
- **Then started.** Last, after the regenerator, the land starts the command `[board]
  on_land` names in the main checkout's `symphony.toml` (none unless the project says),
  with the task's slug and its merge commit appended (`crates/tasks/src/on_land.rs`), so
  what it starts finds the regenerator started already. It runs detached from
  the main checkout in a session of its own, outside the land's recorded run, with
  neither the lander's session nor the marks of how the land was asked for, `SYMPHONY_LAND`
  and the `tasks` recipe's `SYMPHONY_UNASKED`: what it starts, a release run among them, is
  no command an agent ran unasked, and lands nothing. The land listens to it for 3 seconds at most
  and shows what it said by then, then goes on, never waiting on it longer. Its output goes to a
  file of its own, `symphony/tasks/on-land/STAMP-SLUG.log` (the newest 40 kept), which the land's output
  names, and a line naming that file to `symphony/tasks/on-land.log` (set aside as `on-land.log.1` past
  256 KiB). A command that cannot be read or started is said there and on the land's output, and
  the land still stands.
  A board that `TASKS_SERVER` names (a test's) starts it only under `TASKS_ON_LAND=on`;
  `TASKS_ON_LAND=off` starts none anywhere. The Symphony's own repository names its
  release cadence there, whose run waits for that regenerator
  ([RELEASES](RELEASES.md#the-cadence)).

The merge of the task's tip is one step of the land, the one a hosted pull request's
merge takes the place of under `[land] delivery`: the approval before it and the record, the move,
the push and the close after it stay as they are. There no land needs running at all: the task's
move to review dispatches it, the host merges once its last stamp stands, and the watcher closes
the task ([a land dispatched at review](#a-land-dispatched-at-review)); `just land SLUG` arms one
that reached done without a dispatch and settles a merge no watcher saw.

**Bounded.** The merge's commit, with its hooks, and the check each run in a process group
of its own, which the land ends whole past `[land] timeout_seconds` (an hour unless the project says;
`0` is refused, as is any `[land]` key the land does not know); a fetch or a push has
five minutes at most, never asking for a password. A land's worktree carries the pid
of the land that made it and when that process started (`symphony land: SLUG (pid N,
started S)`), and a later land replaces one left behind once that pid no longer names
the process of that start (a land killed outright, its number since another's) and
`lsof` says no process works in it; when `lsof` cannot say, the refusal names
`lsof +D PATH` to look and the command that removes it once nothing works there. The
group a land runs is named in `symphony/tasks/land/running` while it runs, with when its
first process started. While that group has any process alive, a later land refuses
and rebuilds nothing; it says to end the group (`kill -TERM -N`) only while its first
process is the one the land started, by its start, and for the fast-forward it says
to wait for it instead. A first process of another start under the number means the
land's group is gone (no pid is reused while a group of that number lives), and the
name is forgotten; with the first process gone, or never dated (a land before this
one), nothing proves the processes in the group the land's, and the refusal says how
to list them (`ps -A -o pid,pgid,lstart,command`) and to remove the file once those
of the land's are ended, never to end the group.

**Interrupted.** Ctrl-C, a hang-up or `kill` (SIGINT, SIGHUP, SIGTERM) does not reach
the commit's hooks, the check or the fast-forward, which run in groups of their own: the
land takes the signal, ends the running group (SIGTERM, then SIGKILL five seconds on) but the
fast-forward, which it waits for, removes its worktree, says one line and ends by that
signal; a land waiting for another's lock ends at once, nothing landed. Before the default branch moved, nothing landed. After it moved (while it
pushed, say), the approve still stands and the task is open, and running the land
again finishes it, by the record it wrote before the move alone: its tip the branch's
still, its merge holding that tip, its checked commit on the default branch. The run
checks that the approve covers that tip and finishes the push of the recorded checked
commit, never of a newer tip no check passed on, the landed line, marked as finishing
an interrupted land, and the sweep, with nothing merged or checked again. A sweep
meanwhile keeps the task's branch while that record names it; the worktrees on it may
go, as the run makes a worktree of its own. A land killed outright while the main checkout moved leaves the move running and named: the next
land waits for it, and once it ended without moving the default branch, restores what
it half-wrote before it reads the main checkout, saying each path here and on that
task's thread, and drops its record. A tip on the default branch with no such record
(merged by hand, or a branch that never left it) refuses, `merged outside land:
nothing to finish`, with nothing pushed, closed or said; the line says how to finish by
hand a land whose record was lost: push the default branch once a check passed on it,
say the landing on the thread (`landed SLUG on main at MERGE`, the merge that brought
the tip named) and sweep.

Each refusal is one line, on the land's stderr and on the task's thread, and leaves
the default branch where it was: no approval standing, one that is not a fresh
reviewer's, a commit it does not cover, a main checkout with edits or another branch
checked out, a conflict outside the generators' outputs (never resolved), a git hook
that refused the merge, a fast-forward that did not move the default branch (what it
half-wrote restored), and a red check.

**A task has one orchestrator.** Taking the role on a task another session
holds while it is live, stale or of unknown liveness is refused, naming the
holder and the command that moves it: `just tasks handover SLUG HANDLE`, run by
the holder, drops its role and gives the role to HANDLE in one write. The
thread says `A handed orchestrator over to B; address B or @orchestrator`,
and B hears one `ROLE SLUG B orchestrator, STATUS (from A)`, whatever other
role B holds there; A made the move and is not told it. HANDLE must still be there (live, stale or
of unknown liveness); a hand-over to a session gone quiet, or leaving as it is
written, is refused, naming its health; under a cap, so is one that would raise
HANDLE's counted tasks past it ([Slots](#slots)), and, as a take of the role is, one to an
orchestrator at its limit ([The limit on the tasks an orchestrator
holds](#the-limit-on-the-tasks-an-orchestrator-holds)). Taking the role over a holder that is
lost or left drops that one in the same write, and the thread says `B took
orchestrator over from A (lost)`, which A hears and B, who made it, does not; so
does taking it again where a left holder still sits beside you. A task is its orchestrator's when that session holds the
role on it, or on the nearest task above it that has a holder still there, and
**every open task has an orchestrator**, or waits for one under its limit: the board gives the
role wherever a task would come to have none (`crates/tasks/src/orchestration.rs`,
`crates/tasks/src/store/work/given.rs`), to an orchestrator holding fewer open tasks than the
limit, a red to any ([The limit on the tasks an orchestrator
holds](#the-limit-on-the-tasks-an-orchestrator-holds)):

- **Filed.** An orchestrator session that files a task with `just tasks create` holds its
  orchestrator role from that write (`created SLUG (ready); HANDLE orchestrates it`), unless the
  task is already another's under a parent it holds; a subagent's claim of it leaves that role
  where it is. `--no-role --for HANDLE` files it for HANDLE, a live orchestrator, which takes the
  role instead; `--no-role` without `--for`, or with one naming no live orchestrator, is refused.
  Any other filer's task (a worker's or a reviewer's follow-up, a filer's red, `tasks api`'s
  `create`) goes to the orchestrator of a task that session holds another role
  on, else to its parent session (`register --parent`) while that is a live orchestrator, else by
  the rule below; the owner's task by the rule alone. One filed under a task an orchestrator holds
  is that one's by the tree, and nothing is written. `create` names who took it. An orchestrator
  its story names that is at its limit is passed over, and `create` says so, then who took it:
  `created SLUG (ready); amber cannot take orchestrator on SLUG: amber orchestrates 16 open tasks,
  the limit of 16 an orchestrator holds ([limits] orchestrator_tasks): it gains none until one of
  them is done or cancelled or it lets one go; cobalt orchestrates it`, or `…; it waits for an
  orchestrator under its limit that stays` with none under it; `--for` one at its limit files the
  task the same way, named as passed over though the board gives the task to the filer.
- **Left.** A `leave` gives each open task the leaver orchestrates, and no other holder keeps, to
  another orchestrator by the rule, in the leave's own write, one thread line each:
  `B was given orchestrator from A (left)`. Only a leaver holding the orchestrator role reads the
  board for it.
- **Moved.** A `just tasks parent` move that takes a task out from under the task its orchestrator
  holds keeps it with that orchestrator: the move gives it the role on the moved task in the same
  write (`B was given orchestrator (SLUG moved out from under PARENT)`), so an orchestrator
  restructuring its own tree keeps its work, and the move's cap read counts the moved work as
  B's, in a slot of its own ([Slots](#slots)). Only such a move reads more than the tree: the two
  lines of parents and the moved tree, once.
- **Lost, or nobody's.** Every read of the whole board that shows an open task nobody orchestrates
  gives it, as the board (`Store::give_if_due`): each look of an orchestrator's `just tasks wait`
  and of its `attach` monitor, `just tasks pull`, each pass of the detached dispatcher a
  command-line write starts while a channel is registered, the machine server's clock for every
  project it serves, and the idle sweep (`just tasks reap`, from the server's maintenance of each
  project it serves that has a board). So a lost or left holder's task (`B was given
  orchestrator from A (lost)`), one filed by a build or a tool that gives none, or one on a board
  copied in (`B was given orchestrator (nobody orchestrated it)`) is given at the next of them, on
  every board, with no sweep run. Each read is bounded: one that shows every open task held reads
  nothing more, one that shows only tasks waiting with none that stays reads one record of who
  listens, and a give is made once (fenced on what it read, one thread line per give).
  `reap --dry-run` says what it would give.
- **Released.** `release SLUG --role orchestrator`, and `release` of every role, is refused while
  it would leave open work with none and no orchestrator holds a task above it, naming
  `just tasks handover SLUG HANDLE` to the orchestrator the rule picks, or saying no other that
  stays and holds fewer open tasks than the limit is there (a `leave` then leaves the work
  waiting). An orchestrator past its limit lets go of work not started (none of it in progress or
  in review) and the task waits for the next under its limit.
- **Reopened.** A done task moved back to open work after its orchestrator went is given by the
  rule in that move, and waits with none under its limit.
- **Approved, not landed.** A task its reviewer approved reads `done` before its land, and its
  land is still somebody's to make: the approve marks it in its own write (the board's own field
  `unlanded`, when it was approved), and the landing's line, or a move back to review, lifts the
  mark. While it carries the mark the board keeps an orchestrator on it as on open work: a leaver's
  or a lost holder's is given on by the rules above, it waits marked with none that stays, and any
  orchestrator takes the role on it or hands it over, though no other role is taken on a done
  task. It takes no place under the limit and no slot. Ten minutes after its approve, its
  orchestrator's PULL lists it as `approved_unlanded` until it lands ([Sessions](#sessions)). Its
  assessor still writes what it needs ([Needed stamps](#needed-stamps)). A task done by a landing, by a close
  as covered by another's landing or by a subtask's `done` never carries the mark.

A task's own story (`--for`, the filer's work, its parent session, the task above it, the one it
had) names its orchestrator while that one is live and under its limit. Otherwise the rule: the
orchestrator holding the nearest task above it, else, among the orchestrators that stay and hold
fewer open tasks than the limit, the one holding the fewest (ties by handle), one task at a time,
each counted as it is given, so a pass fills none past the limit. An orchestrator **stays** when it is live, has been
since past the first live line (registered 90 seconds or more before), and either listens (a native
channel, a `just tasks wait` in progress, an `attach` monitor) or already holds the orchestrator role
on an unfinished task. A session that only registered is given nothing by the board's own choice,
so a throwaway that registers and goes moves nothing. With none that stays under its limit, the
task is marked waiting (`just tasks show` says `waits for an orchestrator under its limit that
stays, for N min`, naming the limit and the `role` command any such orchestrator takes it with,
and `create` that it waits, while any orchestrator that has not left is on the board: a new
project's has none, and says nothing of it), and the first
orchestrator to come to stay, or to go under its limit, is given it at the next whole-board read, told in one line however
many it was given: `GIVEN 2 open tasks are yours to orchestrate: sim/a, sim/b` (the first ten
named), in its wait, its monitor or its channel. The board's gives reach their receiver as that one
line, never a `ROLE` line per task, and a given orchestrator has orchestrated the task, for the
fresh reviewer rule. So a task with no orchestrator is the moment before the board gives it one,
or the wait while none under its limit stays: a well state, which every orchestrator hears and any
under its limit takes with `just tasks role SLUG orchestrator`, counted on the board's health and
warned of nowhere ([Board health](../operations/BOARD_OPERATIONS.md#board-health)). The cap ([Slots](#slots)) refuses no give but a move's: a given task in
progress counts for its new orchestrator as it did for the one it had, and a ready one counts for
none.

Each orchestrator hears its own tasks, and every orchestrator hears a task with none, for that
interval (`crates/tasks/src/orchestration.rs`): its NEW lines, PULL digest rows and the STATUS of
its move to `blocked`, which no digest row shows; only its orchestrator hears an orchestrated
one's. A post to `@orchestrator`, and a verdict, go to the nearest holder still there, passing one
gone quiet. The owner's word, a post, a verdict or a move, asks more: on a task whose nearest
holder is not live (stale, or of unknown liveness, as well as a task with none) it reaches every
orchestrator, marked, since none is there to relay it ([Talking on a task](#talking-on-a-task)). A
session gone stale or lost, or waiting in its terminal, is said to the
orchestrator of the work it holds, its LOST line naming only that
orchestrator's tasks and its BLOCKED line only a launch for one of them; a
session whose work has no orchestrator yet, or who holds none, is
said to its parent, as before. The digest's idle sessions leave out the
children of another orchestrator still there. Another orchestrator's task
reaches it only in a post to its own handle. `just tasks list --doubled` lists the tasks
that still hold two orchestrators from before the rule; one of them settles it
with `handover` or `release SLUG --role orchestrator`. No board data is
rewritten for them.

The executor can never review its own task, and the reviewer cannot claim it; nor does the task's
orchestrator, a former one or its filer take the reviewer's role.
Of any number of simultaneous claims exactly one wins. `just tasks next` picks
work for you: your own reservation first, then priority, then a red task
([Slots](#slots)) before other work of its priority, then the areas you
already work in (`--affinity`, and the slug prefix of tasks you hold).

### What clears a red run

The `tests` stamp reads, of each build and test command the task ran at the tip, its newest run
there (`crates/tasks/src/stamps/read.rs`), a command being its recorded label, the recipe and
its arguments as typed:

| At the tip | What it is | What clears it |
| --- | --- | --- |
| a command's newest run red (a failing test, a compile error, a timeout, a kill) | a red: the stamp does not stand | the same command, typed the same, run again green at the tip |
| a red run of a command that cannot pass at this commit (it names a test target or a package that is not there) | a red | a new commit only, where each stamp the task needs is given again |
| a run its runner refused before it built or ran anything (a usage error of Cargo, nextest or just, a recipe the justfile does not hold) | no run of its command: no red and no green | nothing to clear; a red of that command before it there still stands, and so does a green |
| a run of the command at another commit | not the tip's | nothing at the tip is changed by it |
| a run still going or queued | not yet either | its end |

A green run of another command never clears a red, and a refused run never stands for a test: the
stamp still needs one test run green at the tip. Nobody marks a real red as not counting, and the
land takes no waiver: a refusal is what the run's own log says, read by its recorder as it ended
([MACHINE_WORK](MACHINE_WORK.md#a-command-its-runner-refused)).

### A land delivered by a pull request

With `[land] delivery` naming a command ([PROJECT_FILE](../operations/PROJECT_FILE.md#land)), `just land SLUG`
makes no merge and no push of the default branch: the host of the project's repository squashes
the task's head into one commit there, and the main checkout follows it. A head that imports the
project's upstream is the one exception, merged there by a merge commit
([below](#an-import-delivered-by-a-pull-request)). With no such key the land is the one above, and
nothing here is reached. This repository sets one: its tasks are delivered to
`heavenstudio/symphony` by [`symphony-github deliver`](../operations/GITHUB.md#delivering-a-land), built in the
main checkout ([AGENTS](../../AGENTS.md#this-repository-as-its-own-project)).

Nobody lands a task there, so an approve of one names no landing to run: it says that the task is
done on the board and not on the default branch until the host merges it, that the host merges
its pull request once its checks pass (named by its number, repository and address once a
dispatch bound one, from its journal) and the board closes the task, and names `just land SLUG
--as HANDLE` only as what settles it by hand. A project whose `land` is a recipe of its own keeps
the line of [the default landing](#the-default-landing).

The board names no host. The command is the project's own program, run in the main checkout as
`COMMAND VERB --request FILE`, FILE one JSON object and its standard output another whose `state`
says what it came to; [`symphony-github deliver`](../operations/GITHUB.md#delivering-a-land) is one. Its verbs
are `publish`, `arm`, `merge`, `disarm`, `close`, `result` and `landed`
(`crates/tasks/src/land/delivery.rs`, `crates/tasks/src/land/dispatch.rs`), and it decides
nothing: what a task needs and whether it stands is
[the evidence](../engineering/BOARD_API.md#the-evidence-of-a-task-at-its-head) the land reads and hands it.

It delivers nothing past a check the project asks for. The default land runs `[verdicts] check`
on its merge before main moves (`[land] check`, on unless the project turns it off); a commit the
host squashes is run through nothing here. So with the key set and that check to run, the land
refuses by name, to the orchestrator, until the project's owner says `[land] check = false`,
the pull request's required checks taking its place, or takes the key out.

A land keeps a journal of the delivery in the lands' folder, written before each step:

1. **Prepared.** The remote default branch is fetched, and the project file it commits must be
   the one committed here. A head that changes a generator's outputs is refused, unless it is
   [what the regenerator made](#what-the-generators-make-delivered-by-a-pull-request), as is one
   that merges a commit the remote default does not hold and is not
   [an import of the upstream](#an-import-delivered-by-a-pull-request): another history, which a
   squash would flatten ([UPSTREAM_DOWNSTREAM](../operations/UPSTREAM_DOWNSTREAM.md)). The refusal of
   generated outputs tells `@executor` to put them back as the default branch has them, but for a
   release candidate's fix, a head that holds `release-candidate` and beyond it no commit the
   remote default holds: that head keeps the outputs its own sources make, approved naming it,
   and the branch delivers a merge of the default branch that keeps the default branch's
   ([RELEASES](RELEASES.md#the-release-candidate)). With a candidate held that the head does not
   hold, the refusal says how a fix of it is rebased first. A task that is not
   eligible at its head is refused before anything is published.
2. **Eligible.** `publish` makes the head and its evidence public and answers the pull request,
   which the task is bound to from then on. The evidence is read again: of another revision, it is
   published again or refused, never merged as it was.
3. **Armed.** `arm` turns on the host's own merge of that head, by the squash and the text a
   merge asks: the host merges it once every check it requires passes, `symphony/eligible` among
   them, and the watcher closes the task ([below](#a-land-dispatched-at-review)). A pull request
   the host would merge now (its evidence standing, no other check to wait on) takes no such
   arming, answered `mergeable`, and the land asks the merge itself.
4. **Merge requested.** One `merge`, a squash of that pull request at that head, or a merge
   commit for an import.
5. **Merged remotely.** The commit the host made is proved by git alone: on the fetched default
   branch, of one parent, its tree the merge of that parent and the head (an import's proof is
   [its own](#an-import-delivered-by-a-pull-request)). A receipt of the head and the commit is kept
   for good (`receipts/` in the lands' folder), and the head stays reachable under
   `refs/symphony/delivered/SLUG`, since a squash leaves it in no history; a head with a receipt
   does not go back to review. A slug is delivered once: the release run counts, holds a release
   and reads its notes by that receipt and that head, so neither is ever replaced
   ([below](#a-slug-is-delivered-once)).
6. **Reconciled.** A clean main checkout whose default branch the remote's descends from is
   fast-forwarded. A dirty or diverged one is left as it is, the land answering "landed remotely,
   reconciliation pending" once on the thread, and a land run again closes the task once the
   checkout can follow. Nothing here is reset, merged or pushed for it.
7. **Closed**, then what follows a land: the landed line names the commit, the head and the pull
   request, once; `[board] on_land` runs with the commit, once; and `landed` tells the program on
   every run that reaches here, as the verb is safe to ask twice and a run cut short before it
   told nobody.

A pull request a run before bound the task to is read before the land judges anything, on every
run, by `result`:

| The bound pull request | What the land does |
| --- | --- |
| Open, the evidence standing | goes on: publishes and asks for the merge |
| Open, the evidence not eligible now (a red run, a head nobody assessed) | publishes the snapshot that blocks, at the head the branch is at, so the check the host requires fails there; then refuses as any land does |
| Merged at the branch's head, the evidence standing (the host finished a merge whose lost answer was read as open, or a person merged it) | proves the commit and adopts it, or holds it, as any merge; nothing is published to it and no second merge asked |
| Merged while the evidence does not stand, or with the branch at another head, or closed unmerged | held for reconciliation, by name; nothing closed |

| What the land finds | What it does |
| --- | --- |
| The publication or the merge refused for the branch's protection (`policy`: [what the program reads of it](../operations/GITHUB.md#the-protection-a-delivery-needs)) | nothing is pushed or merged, and the refusal names what is missing to the orchestrator; a land run again reads it again |
| The merge refused (a conflict, a moved head, checks, permission) | the task stays open, the refusal goes to who acts on it, and the next land validates again before asking again |
| No answer to the merge | it reads the pull request (`result`): merged at the head, the commit is proved and adopted; open, the next land asks again; unknown, the land says so and the next one reads it first |
| A commit that is not the squash of the head (a merge commit, another tree), an import the host squashed, or the pull request closed unmerged | held for reconciliation: said once to the orchestrator, nothing closed, no receipt |
| A land run again after it closed | refused: the task landed once |

One limit holds by design: evidence withdrawn after the last reading and before the host acts on
a request already made does not recall it. The receipt names the revision that was read.

#### A land dispatched at review

Nobody runs a land of a task the host delivers: a write to the board that can change its evidence
starts its dispatch, the host merges it once its last stamp stands, within seconds, and the
watcher closes the task (`crates/tasks/src/land/dispatch.rs`).

- **What dispatches.** A top-level task's move to review, and after it each later move,
  assessment (`tasks needs`), verdict and gate decision, starts `tasks land SLUG --dispatch`
  detached from the main checkout, as the one who wrote, with the land's mark; the write returns
  at once and its lines go to `dispatch.log` in the lands' folder. The move to review leaves
  `SLUG.dispatching` there, naming who moved it, until a dispatch binds the task's pull request,
  and from then on its binding is what a later write dispatches it by: so a first dispatch that
  stopped before it published is made again by the next write, and a task never moved to review,
  or delivered and closed, starts none. A dispatch that stops says why on the task's thread, to
  `@orchestrator`, with the `just land SLUG --dispatch` that dispatches it again: nobody reads its
  log. A board that `TASKS_SERVER` names (a test's) starts one only under `TASKS_DISPATCH=on`;
  `TASKS_DISPATCH=off` starts none anywhere. One dispatch runs at a time under the land's lock,
  and one asked while another runs runs after it.
- **What a dispatch does.** What a land does up to the publication, the refusals at the head
  above among it (each goes to the thread, to whom it names, and nothing is pushed); then the head
  pushed, never forced, its pull request opened ready and the checks written from the evidence at
  that head, whatever it says, and the host's merge armed. `symphony/eligible` is pending while a
  stamp is not given yet (an assessment, a review, the owner's approve), fails where one is red at
  the head or evidence that stood there was withdrawn, and passes once every stamp stands: so a
  reviewer's approve, or the owner's, is what lets the host merge, and a task in review stays
  there until it does. A check belongs to its commit, so a green eligible on one head merges
  nothing at another; a new commit is pushed to the same pull request at the next dispatch, the
  merge armed across it and pending until the approve names the new head. One whose head and
  evidence are as last published asks the host nothing more. Neither does one that runs once the
  task landed at that head (a write's dispatch that waited for the land of its merge): its log
  says it landed already, and the thread nothing. One whose publication the host refuses because
  it merged that head meanwhile says so in its log alone as well, as the watcher's next round
  closes the task; a pull request merged at another head is the refusal it was. Armed only while
  the task is in
  review, blocked or done: a task moved back to work (a changes verdict) has its merge turned off
  first and its checks said again after, so no check published meanwhile lets the host merge it,
  and stays so until it moves to review.
- **A head moved under an approve.** A delivery takes an approve naming the head alone, so a task
  approved and not landed whose branch is at a commit its approve does not name (a merge of the
  default branch that settles a conflict, a fix, or an approve that named no commit) waits on a
  fresh one, which only a task in review is given. The dispatch that finds it, a write's or the
  watcher's round, moves it back to review itself, by the board's own move from done ([Tasks and
  roles](#tasks-and-roles)) made as its orchestrator, whoever's write started the dispatch (an
  assessor's holds no role that may make it), and says so once to `@orchestrator`, naming the head
  and the commit the approve named: its orchestrator starts its assessor and resumes its reviewer as
  at any move to review, and nobody moves it by hand. Then it is published as any head: the pull
  request shows the new head with `symphony/eligible` pending, its merge armed across the push,
  until an assessment and an approve name it. An approve that comes before the assessment leaves the
  task done, and the assessment is written all the same ([Needed stamps](#needed-stamps)). One that
  does not go back (a task with no orchestrator, whose dispatch moves as who started it, may be one)
  is refused, naming `just tasks move SLUG review`.
- **Who watches.** One watcher per project, `tasks land --watch` (`watch.lock` in the lands'
  folder, `watch.log` its lines), started by each arming where none runs, which reads each armed
  task every `[land] watch_seconds` (30) and ends once none is left. A round writes a task's line
  to the log only where it differs from its round before, so a task that waits days for its
  approve is one line there and the log grows by what changed, never by how long the watcher ran;
  it is set aside past 1 MiB (`watch.log.1`) when a watcher starts. Armed means the host answered
  an arming and has not answered a disarm since: a publication between keeps it so, as the host
  keeps its merge armed across a push, and one that stopped before the host answered its arming
  again (an answer lost, a host away) has its pull request read every round, whatever its checks
  said, and is published and armed again. A refusal a round stops at (a head refused at the head,
  an arming refused) holds until what it names changes, so it is said on the thread once at the
  branch's head, however many rounds read it again (`SLUG.said` in the lands' folder keeps the
  last one said, by a land, a dispatch or a round, until the delivery ends, landed or cancelled;
  a land that merges here keeps none). A task whose evidence does not
  stand yet costs it no read of the host, which merges nothing while eligible is pending; it reads
  its evidence again, so a write no command dispatches for (the owner's through the Symphony, a run
  recorded at the head) is published at its next round. One whose evidence stands has its pull
  request read: merged, the commit is proved and the task closed from review or done alike, the
  checkout fast-forwarded and `[board] on_land` and the regenerator started, as a land closes one
  ([above](#a-land-delivered-by-a-pull-request)). A task whose merge a run asked for or found and
  whose land did not end (the board lost between the host's merge and the close, a crash) is the
  watcher's as well, armed or not: each round goes on from where its journal stands, as a land run
  again does, the task closed once and what follows a land started once, and the watcher does not
  end while one is there. A round that cannot finish it (the board or the host away, the main
  checkout dirty or diverged) says why in `watch.log`, once while the reason is the same, and the
  next tries again; one still not
  finished `[land] merge_within_minutes` after the first round that stopped is said once on the
  thread to `@orchestrator`, naming `just land SLUG`. One held for a person stays held, read by no
  round. A merge no watcher saw (a reboot, which ends the watcher) is settled by the next dispatch
  or `just land SLUG`, and a watcher any arming starts finishes every such journal it finds.
- **A merge that does not come**, once the evidence stood (before, the task is in review and its
  evidence says what is missing). The host's merge is turned off first, so a check that passes
  later merges nothing behind the board, and one line says why, to `@orchestrator`:

| What the watcher reads | What follows |
| --- | --- |
| A check that is not the Symphony's red at the head (a CI the repository adds), or the host reporting the pull request in conflict with the default branch | the task back to in progress (from done by review): its executor merges the default branch once, or puts the check right, and moves it to review, which dispatches it again |
| The pull request closed unmerged, by someone | back to in progress, naming it, the move made as its orchestrator whoever started the dispatch that found it, an assessor too; the next dispatch opens another |
| Neither merged nor red once `[land] merge_within_minutes` (30) passed since its evidence stood (review time counts for nothing) | the task stays where it is, the work not at fault: `just land SLUG` arms it again |
| Any of these where the task cannot go back (cancelled or closed, a document save, which stays pending and shown, or what the regenerator made, which it cancels and makes again) | `tools/delivery-failed-SLUG` filed to `@orchestrator`, naming the pull request and why; a second failure is a line on the one open |

A task cancelled while its pull request is open has it closed unmerged, its merge off and its
branch left. A close the host does not make is asked again by the watcher's next round while its
merge is armed; one no longer armed, which nothing merges, is said to `@orchestrator` with the
`just land SLUG --dispatch` that closes it. One the host merged at a head its branch has moved
past is held for reconciliation, by name, as any merge of another head.

#### A slug is delivered once

A second delivery of a slug is refused, never made over the first. A landed task moves no
further on the board, so the one that reaches a second delivery is a slug filed again as another
task (a board made again over a repository that keeps its receipts), or a delivery command that
answers a new pull request for one.

| What the land finds | What it does |
| --- | --- |
| A receipt of the slug whose head is not the one its branch is at, or of that very head with no word of this task's own that it landed it (no landed line on its thread naming the squash, no journal of that head bound to the receipt's pull request) | refused before anything is published, to the orchestrator, naming the commit and the head it was delivered as and its pull request: what it still needs goes in a new task, whose slug is its own. A task's id is no such word, as a board made again numbers its tasks again |
| A file kept as the slug's receipt that does not read as it (cut short, or naming another slug) | refused before anything is published, naming the file and why: a person judges it, and a land never writes over it |
| A merge a run before asked for, proved beside a receipt of another delivery of the slug or a retained head that is another commit | held for reconciliation, by name; the receipt and `refs/symphony/delivered/SLUG` stay as they were, and nothing is closed |
| The same delivery proved again (a run cut short after its receipt was kept) | the receipt it has is kept as it was written, and the land goes on to close the task once |

What else writes the default branch lands nothing there while the key is set. The regenerator
commits and pushes nothing: what it makes is a task of its own
([below](#what-the-generators-make-delivered-by-a-pull-request)). A save of a document from the
app is kept pending and delivered as a task of
its own ([below](#a-document-save-under-delivery)), the commit on the default branch itself
refused whoever asks for it. The release cadence counts
a squash by its receipt, and a release candidate advances to the head the receipt kept
([RELEASES](RELEASES.md#the-cadence)); a land run again after it was cut while dispatching starts
the regenerator, which makes nothing twice, and no release run a second time, so a release run
its landing made due is asked for by the next landing or the release run's `--due`, run by hand
([RELEASES](RELEASES.md#the-cadence)). The sweep takes the worktree and branch of a squashed
head by that receipt, once git bears it out and the main checkout holds the squash
(the worktree sweep, under "Finished worktrees go by themselves" in [Sessions](#sessions)); the retained head stays.

### An import delivered by a pull request

A downstream project brings its upstream's history in by an import
([UPSTREAM_DOWNSTREAM](../operations/UPSTREAM_DOWNSTREAM.md#the-import)): a task whose branch merges an upstream
commit **U** (`git merge --no-ff U`), then any fixes. A squash would put U's files on the default
branch and leave U out of its history, so the next import would merge it all again. Such a head
is delivered as an import, merged by the host with a merge commit, and only where three things
agree:

- **The project file** on the remote default branch names the upstream: `[land.upstream] url` and
  `branch` ([PROJECT_FILE](../operations/PROJECT_FILE.md#landupstream)). With none, no import is delivered, and
  every head is a task.
- **The assessment** at that head names U whole:
  `just tasks needs SLUG STAMP… --tip H --import U` (`--import` resolves U to its whole commit,
  and only with `--tip`). An import named at another head binds nothing at this one, so the head
  is judged as a task. The evidence carries it (`assessment.import`), so its revision binds it.
- **The history** fetched now from the upstream's address: U on the upstream's branch; the
  default branch **B** sharing a commit with it; where B and that branch last met an ancestor of
  U, so U is newer than all B holds of it; U not in B already; U merged by a commit on H's own
  first-parent line; and every other commit H adds either that line's own or U's. An upstream
  whose branch was rewritten meets B at an older commit, which this does not tell from new
  history: the import's own check that U continues the last import finds it
  ([UPSTREAM_DOWNSTREAM](../operations/UPSTREAM_DOWNSTREAM.md#the-import), step 2), and its review.

Each refusal names who acts, before anything is published. With an upstream named, a task head
that holds any of its commits is refused, never squashed, and the refusal names the newest
upstream commit it holds and the command that assesses it as an import. The program is told the
operation, the upstream's address and U on every publication and merge; it holds the upstream to
the repository its own settings pin (for GitHub, [`upstream_id`](../operations/GITHUB.md#delivering-an-import))
and answers that repository's number, which the land requires: a program that does not deliver
imports pins nothing, and nothing merges.

To tell an import from a task, the land fetches the upstream for every head it delivers, not
only for an import. It judges the head by the upstream's branch as it is then, never as it was
last fetched, so a task head never carries an upstream commit the land did not see. The cost is
that an upstream that does not answer refuses every land of the project, ordinary tasks
included, before anything is published. The causes include the network, its host, the
machine's credentials for it, or an address that moved. The line is said to the lander alone:
`the upstream URL was not fetched:`, then git's reason or `no answer within N s` (the land's
network timeout), then `every land reads it while symphony.toml on main names it`. The lander
runs the land again once `git fetch URL` answers from the main checkout. An address on the
default branch that no longer answers is that branch's to correct, by the project's owner at the
host: the land of that correction is refused like any other.

The merge commit is titled `Merge branch 'SLUG'`, as the land's own merge is, so the release
cadence counts it as one. It is proved by git alone: on the fetched default branch, of two
parents, the base it was made on then the head, its tree the merge of the two, and U in its
history. Its receipt keeps, besides a squash's fields, `import`: the upstream's address, branch
and number, U, the upstream's tip as fetched, where B and the upstream met before, the commit's
parents, and where they meet after. An import the host squashed is held, and so is a task the
host merged by a merge commit: neither closes. The repair of a squashed import is forward, a new
import task of the same U, whose merge changes no file and puts U in the history.

### A document save under delivery

While the key is set, a save of a document from the app (an answer on a card, a comment, a
dismissal) writes neither the file nor the default branch. The save is answered
`{"ok": true, "delivery": "pending"}`, never as committed, once its edit is in the committer's
journal (`verdict-publications/` in the server's state folder), and it is kept there, with the
owner's words, until its delivery is proved (`crates/symphony/src/commit/pending.rs`). Every
refusal a save has without the key stands: text that changed under it, a card it does not find,
words it would lose, and the same tap or answer again is the repeat it always was.

| Step | What it is |
| --- | --- |
| Shown | The page, its source and the queue show the document as the default branch has it with each kept save of it merged in, in the order saved, under a note that names each one and where it stands. The file in the checkout is never written, so the checkout stays clean for the land's reconciliation. |
| A branch | The committer commits the edit as one commit on `docs/owner-save-N`, N the journal entry's number, on the default branch here. A later save of the same document waits until the one before it is delivered, cancelled or refused, and is then committed on the default branch as it stands: no branch stands on another's undelivered commit, and a head is never rebuilt. It is shown meanwhile. |
| A task | The branch is filed as the owner's task of that slug, once, under a receipt of the branch and its commit, and put in review. Its assessor says what it needs from its diff as for any task, and its orchestrator lands it, which delivers it by its pull request. The app runs no land and asks the host for nothing. A conflict with the default branch is settled on the branch by merging the default branch into it, never by rebasing it, as the delivery is proved by a head that holds the save's commit, and the new head is assessed again. |
| Proved | The save is delivered once the land's receipt for its own task names a head that holds the save's commit and a squash that is on the fetched remote default branch, and its entry is dropped once the default branch here holds that squash. The proof is that identity, never the text, so a save whose conflict was settled on its branch is proved like any other. Until the checkout follows, it is still shown. |

Text alone proves nothing. A save whose text another task's delivery brought is dropped once
that task's receipt names the commit that introduced it, a commit newer than the default branch
was when the save was kept, after its own task is told so, once, with that receipt's pull
request and head; closing that task is its orchestrator's. Equal text an earlier delivery
brought is another owner action's and covers nothing. A save whose text is on the remote default
branch with no receipt that explains it is held for a person, who removes its journal entry by
name once it is settled.

| What stops a save | What is kept |
| --- | --- |
| The board does not answer, or the task cannot be filed | the entry and its branch; the next pass files it |
| Its edit no longer applies to the default branch | the entry, with why, and no branch; later saves of the document do not wait for it |
| Its task is cancelled, or closed by a landing or by hand with no receipt that proves the save | the entry and the owner's words, said as not delivered, with why; later saves of the document do not wait for it. A task its reviewer approved is done on the board before its land: while that approve stands with no landing after it, the save is still waiting for its pull request, and later saves still wait for it. Once its land has asked the host for the merge the note says so, and a delivery the land holds for reconciliation is said with its why, both read from the land's delivery journal |
| The default branch moved under a save on its branch, so its words no longer apply here | the entry and its branch; the note says the save is in conflict and that the page cannot show its words until the conflict is settled on the branch, and opens saying the page keeps its saves rather than shows them |
| A receipt that is not this save's delivery: of another head, or of a commit the fetched remote does not hold | the entry: it proves nothing |
| The server stops between two steps | the entry: a branch made and not journaled is taken up if it holds the edit, a task filed under its receipt is found again, and nothing is made twice |

Each pass of the committer takes every kept save up again: at a server's start, after each save,
and after each refresh of the documents, which the land's fast-forward of the checkout causes.
Those asked while a pass is pending or running ask for one more pass together, never one each.
A save kept by a build that did not know the key is delivered the same way. A project's
`[verdicts]` generators are not run for a kept save, on its branch or after its delivery: what
they generate stays as it is.

An agent's reply (`just symphony-reply`) and a tidy (`just symphony-status --tidy`) run in the
checkout on the default branch are kept the same way, one save for each file they change. Each
is written against the page as the saves still waiting leave it, under the lock the app's saves
take. So a reply follows the owner's line it answers on the page, and it is committed only
once the owner's save before it is delivered. Its entry names the agent's session
(`TASKS_SESSION`, or the reply's `--as`; `agent` with none), and its task is filed as an agent's
save rather than the owner's. The server watches the journal's folder, so
a save another process keeps there is shown and taken up at once, with no document changed.

One pass delivers an agent's saves as one task. The first agent's save whose turn has come
takes with it every later agent's save whose turn has come too, of any document: one whose
every earlier save of its document is delivered, on no way to the default branch, or going with
it. They are committed in the order kept, as one commit on the first's branch, and filed as one
task; each entry names that branch and its commit (`members` in the first's), so each is proved,
held and closed by that branch's receipt, land and task. An owner's save never goes with one, so
a pass splits only where one document's saves meet an owner's save still on its way, or a batch
already on its way: the later saves of that document wait for it, and go with the next pass's
batch. A save left out of a batch, because it no longer applies after those before it, waits for
the next pass with every later save of its document. The next pass tries it alone: one that
cannot be committed then is on no way to the default branch, says why on its page, and holds
back no later save.

The saves one tidy keeps go to the default branch in one commit or not at all: its ledger never
reaches it without the pages whose comments it holds, nor a page without the ledger
(`together` in each entry: the first's number and how many). They are led by the first of them,
which waits while any of them would: a page's save waits behind its ledger's, and the ledger's
behind a save of that page still on its way. A batch takes them all or none, and leaves them all
out where one no longer applies.

A tidy whose saves cannot go is on no way as a whole, as a save alone that cannot be committed
is. That is the case when its first is held (one of them conflicts with the default branch, and
so its commit fails), when its task closed with no delivery, or when it is no longer whole once
kept, one of them dropped as another delivery brought it. Each of its saves then says why on its
page, and none holds back a later save of its files: an owner's save of one of its pages goes,
and so does a later tidy's ledger. The tidy writes its first entry last, so a tidy that could
not keep all of its saves has no first in the journal. Those it did keep are never shown or
delivered and hold back nothing, and the tidy says on its error output that nothing of it was
kept.

### What the generators make, delivered by a pull request

With `[land] delivery` set, [the regenerator](#the-regenerator) makes the outputs of the default
branch's tip as it does anywhere, and then commits nothing there and pushes nothing. What it made
is one commit on that tip, of the generators' outputs alone, and it becomes a task:

- **A record first.** `symphony/tasks/land/regenerate.pending` names the task, the tip, the
  commit and each generator's key, and is written before anything else. Then the branch
  `generated/outputs-at-TIP-COMMIT` is pointed at the commit (the tip's first twelve letters and
  the commit's first seven, so no two generations share a name; a branch of that name at another
  commit stops the round, which says so) and the task of that name is filed,
  claimed by the session that files it and moved to review. Each step is made only where it is
  not there, so a run cut short at any of them is gone on from by the next, which makes nothing
  again.
- **Assessed as any task.** The regenerator names no stamp and waives none. An assessor reads the
  head, the catalogue decides what it needs, and `just land generated/outputs-at-TIP-COMMIT` delivers
  it: the same journal, the same squash, the same proof and receipt, whose `operation` says
  `generated`. The host is asked nothing a task's land does not ask.
- **One generation for one set of inputs.** While the record stands, a round whose tip holds the
  inputs it was made of makes nothing and says where the task stands: each keyed generator's
  key is the one recorded, and for a generator with no key no path outside the generators'
  outputs changed.
- **Stale is cancelled, and never delivered.** A round that finds other inputs cancels the task,
  saying why, and makes the newest tip, a task of its own. One the board never took (a run cut
  short before it filed it) has no task to cancel: its branch is deleted with the record, where
  it is still at the commit made. A pull request the cancelled task was
  bound to is told by a snapshot that blocks, so the check the host requires fails there, unless it was closed or merged meanwhile; while
  that could not be told (`regenerate.withdrawn` names the task) its checks may still pass,
  the round says so and why (the host's reason, or the task's status where its evidence still
  stands, as it does for one named and not yet cancelled), and each later round tells it again. A task whose merge was asked for is
  never cancelled: the host may hold its commit, and its land settles it.
- **What the land checks.** A task the record names is delivered only as made: its branch at the
  recorded commit, that commit one commit on the recorded tip, which the remote default branch
  holds, changing the generators' outputs alone, and the inputs at the fetched remote default
  branch those it was made of, each key read again there in the regenerator's worktree, under
  its lock. The project file here must be the one the remote default branch commits, as for any
  task: the generators and keys it is judged by are read from it. It is judged twice, before
  anything is published and again right before the merge is asked for, and a record that no
  longer names the task by then (a round cancelled it meanwhile) asks for no merge. A stale one
  is refused to the orchestrator and the regenerator started. A task the record does not name
  that changes a generator's outputs is refused as before.
- **A base that moves under the request.** The host merges onto the default branch it has then,
  which no check here pins. Where that base gained an input after the last reading, what the
  default branch then holds of the outputs is stale: the record stays until the round the land
  starts reads the receipt's parent, says so, and makes the newest tip. Until a round finds the
  outputs current, `just tasks regenerate --status` does not say they are.
- **A start that was lost.** The regenerator keeps no list of builds: a round reads the default
  branch's tip, the keys it passed on and its record. So a land killed before it started the
  regenerator loses nothing that the next start does not make: any later land's, the same land
  run again where its journal still says it was dispatching, and `just land SLUG` of a task
  already delivered, which starts the regenerator by its receipt and answers as it did before.

Local reconciliation is the land's, as for any delivered task: a round takes the task as
delivered only once the default branch here holds the commit the host made.

### A delivery whose host is gone

Once a land has journaled a merge as requested, no land merges that task here, whatever
`[land] delivery` says by then: with the key removed the land refuses and names the journal. That
is not fallen back from by a flag. With the host back, set the key again and run the land, which
reads the pull request. With the host gone for good, a person does this by hand:

1. Fetch what remains of the remote and look for the commit: one parent on the default branch,
   its tree the merge of that parent and the journal's head (`git merge-tree --write-tree PARENT
   HEAD`).
2. If it is there, the head landed: fast-forward the main checkout to it and close the task with
   the board's ordinary commands, saying on its thread the commit it landed as.
3. If it is not, nothing landed: remove the journal by its name (`rm -f` of the path the refusal
   gives), and the next land without the key merges here.

### The regenerator

What a project's `[land] generators` make (this repository's client distribution) is
made after a land, never on its path. Where the project
[delivers by pull request](#a-land-delivered-by-a-pull-request) it lands
nothing itself, a commit of its own on the default branch being one the host never made: what
it makes is [a task the host lands](#what-the-generators-make-delivered-by-a-pull-request). One build for main runs at a time, and once it is
built the next is main's newest tip: a build in progress is never cancelled but let
finish, and there is no queue, so some merges are never built on their own. A land that
closed its task starts
`tasks regenerate` detached (`crates/tasks/src/land/regenerate.rs`), by the land's own
build, its output appended to `symphony/tasks/land/regenerate.log` (set aside as
`regenerate.log.1` past 1 MiB by a start, never while one runs), and waits for nothing it does.

- **One build of main at a time.** It holds `symphony/tasks/land/regenerate.lock`, taken
  without waiting and naming its process and that process's start, as the land's lock
  names its land. A land that finds it held starts nothing and signals nothing: main's
  ref is all it leaves, and no list of builds to make is kept anywhere. One started
  anyway (two lands' starts at once) finds it held and ends at once, and one that finds
  main's tip already made, current or red, makes nothing.
- **Never cut short.** A build is never stopped or begun again because main moved: it
  finishes, and then lands what it made of its commit or discards it (below).
- **Then the newest tip, once.** Once it lets go of its lock it reads main once: on
  another tip than the one it finished with, it builds that tip, the newest, and every
  merge that came in between is never built on its own; else it ends. Three lands
  during one build cost one more build, of the last.
- **A builder that died.** Its lock is free once its process is gone, the file still
  naming it, and the next land's start takes it over and builds main's tip.
- **A worktree it keeps.** Each round checks main's tip out in a worktree of its own
  that stays between rounds (`regenerate` in `[agents] worktrees`, else
  `symphony/tasks/land/regenerate-worktree`, locked from the sweep), so what its builds leave
  there stays warm, and runs the generators there in order, a `when_changed` one only
  after an earlier one changed something.
- **A key it passed on before.** A generator with a key (`[land] keys`, by its name, beside
  the generators so a Symphony older than keys still reads the file), a command whose last
  line names everything it reads, runs it first: a key and committed outputs it passed on
  before (`symphony/tasks/land/regenerated.json`) need nothing made. This repository's key
  is the hash of the client's package
  (`tools/client-key.sh`), so a land that changed no client input builds no client. A key names one generator, so the file is refused when
  two `[land] generators` share a name, keyed or not.
- **One commit, landed in seconds, or discarded as stale.** What the generators change,
  inside their outputs alone, is one commit, `Regenerate NAMES for main at TIP`. When
  main moved during the build, it is made on main's tip as it is then, with those
  outputs replaced, only when what they are made of is the same there as at the tip it
  built: no path outside the generators' outputs changed, or each one's key, read again
  on main's tip, is the one it built on. Otherwise what it made is stale: discarded,
  said in its log, never committed, and the newest tip is built next. The commit lands
  under the land's lock: the fast-forward of the main checkout and the push, as a land
  makes them, its interrupt safety included. It needs no reviewer's approve: it changes
  the generators' outputs alone, made by them of what main holds. Nor does it run the
  project's git hooks (`git commit --no-verify`, or `git commit-tree` on a newer tip, which
  runs none), the land's merge being where they judge what a branch brings.
- **A red, one task.** A round whose generator fails, runs past `[land]
  timeout_seconds` or writes outside its outputs lands nothing and files
  `tests/main-red-regenerate` at p0, marked red, or adds a line to the one open while
  it stays red; the next round that passes closes it, and a later red files
  `tests/main-red-regenerate-2`, and on.
- **The check no land runs.** Where `[land] check = false` keeps `[verdicts] check` out
  of the land, as this repository keeps the docs audit, each round runs it on what it left
  main holding: the commit it landed, which holds whatever main gained during the build
  beside the outputs, so a land that came in then is checked too; else the tip it took (a
  red round's as committed, or one whose outputs were already current). It runs in its
  worktree, within `[land] timeout_seconds`, its output in the regenerator's log, once
  what the round made has landed, so it never holds back the generators' commit or a land.
  It is part of the round, under its lock, and never cut short because main moved: only
  its time limit or a signal ends it, and a signal ends the regenerator with it, nothing
  judged. A round discarded as stale, or one that stopped, is not checked: the round after
  it checks the newer tip. Nor is a commit the same check last passed on, which a round
  whose generated task waits for its pull request takes again at each start. A check that fails, runs past its time or cannot run files
  `tests/main-red-check` at p0, marked red, a task of its own beside the generators' reds,
  or adds a line to the one open while it stays red; its text says no more than is known
  (one past its time or one that could not run neither passed nor failed). A later round
  it passes on closes it, and a later red files `tests/main-red-check-2`, and on. So a red
  a land let through is on the board within a round of that land, not first found by a
  release run, while whatever runs that check on main refuses (the Symphony's commit of the
  owner's verdicts). Where the land runs the check, every merge passed it, and the
  regenerator runs none.

`just tasks regenerate --status` says where it stands, from
`symphony/tasks/land/regenerate.json`: current on main at SHA; making them of main at SHA
since when, by which process, how far main is past it, and how far behind the outputs
main holds are (the commit they were last found current at, and how many commits ago);
behind, how far main is past what they were last made of; red; or stopped. Where the
regenerator runs the check, the line ends with what it last came to, from
`symphony/tasks/land/regenerate-check.json` (`the check CMD passed on main at SHA (HH:MM)`, or
`is red on main at SHA (HH:MM, tests/main-red-check)`), or that it runs (`checking main at
SHA since HH:MM (pid N)`), or that one begun ended unjudged. It exits nonzero but when current,
by the outputs alone: a red check holds back no pin or release. With `--wait` it first
waits while the regenerator makes them, and then says the same, so a release or a pin
waits on it. `tasks regenerate`
itself runs only as a land starts it (`SYMPHONY_LAND`, which the land hands it alone of its
children, and never `SYMPHONY_UNASKED`), since it moves and pushes main.
A test's board (`TASKS_SERVER`) starts none unless `TASKS_REGENERATE=on`, and `off`
starts none anywhere.

Between a land and the regenerator's commit, main's committed client is older than its
sources, and a server built from main then embeds the older client. The release run waits,
within its time, while the regenerator makes main's outputs, and releases nothing of a
main still behind, filing nothing for it: the regenerator's red is the task then
([RELEASES](RELEASES.md)). A consuming project's pin moved to a commit of main waits the
same way ([its card](#pin-waits-for-current-client)): it moves only once `just tasks
regenerate --status --wait`, run in the Symphony checkout, exits 0, and then to the
commit it names. The pin is the project's own tooling, which runs it first; a pin to a
release needs no wait, as no release is cut on a main whose client is behind. A branch
commits no distribution of its own: the land keeps main's. The one that does is a release
candidate's fix that changes what the client is built from, since no regenerator builds the
candidate: it commits the distribution its own sources make (`just client-update`), which the
candidate's rerun checks, and the land keeps main's side of it all the same
([RELEASES](RELEASES.md#the-release-candidate)).

- [r] **pin-waits-for-current-client** — A land now merges and pushes in seconds. The client the server embeds is rebuilt after it by one background run, which commits it to main a few minutes later: about 14 minutes when the land changed the client, none when it did not. In between, main's committed client is older than its sources. A release already waits for it: the nightly releases nothing while main's client is behind. May a project's pin be moved to a commit of main whose committed client is behind? A: no; the pin waits until `just tasks regenerate --status` says the client is current, as a release does. B: yes; the pin moves, and the project's server serves the older client until its next move. Recommended: A. Approve and say A or B in the note, or comment. [[TASKS#The regenerator]]
  [2026-11-12 14:20] agent — Posted for the board task `perf/land-batches-approved-tips`.

    **Why it matters.** A change to the wire types, the server and the client lands its server at once and its client one round later. A server built from main in that window embeds the older client against the newer server, so a page can fail to read what the server sends.

    **Where it applies.** Your verdict on how projects take a newer Symphony (they follow release tags, never main) already keeps a behind client out of every pin to a release, since no release is cut on one. This is for a pin moved by hand to a commit of main, which is every pin move until the first release is tagged. The check belongs to the project's own pin tooling. Symphony gives it the signal: the status command exits nonzero unless main's client is current.
  [2026-11-12 14:31] owner · approved — A
  [2026-11-12 14:33] agent — Building A now. A new form of the status command, `just tasks regenerate --status --wait` run in the Symphony checkout, waits while main's client is being rebuilt and succeeds only once it is current, the way the nightly waits before a release. A pin moved to a commit of main moves only after it succeeds, and the guides now say so. The pin itself is moved by the project's own tooling, which runs this command first. A pin to a release needs no wait, since no release is cut while main's client is behind.
  [2026-11-12 16:36] agent · resolved

## Gates

A task's gates are every stamp or check it needs to land, answered for its recorded
branch at its tip: `GET /api/task-gates?slug=SLUG`, the wire type `TaskGates` of
`symphony_api::gates`, built by `crates/symphony/src/gates.rs`. Each gate has a
name, a state (`missing`, `held`, `unknown`, `waived` or `met`), who and when
(server time, seconds), the commit it holds for, why for a waiver or a hold, a
one-line detail, a link to its evidence, and the states it took before, oldest
first, for the timeline. They come in that order of states: what the task waits
on first. The task's page draws them in one section under its head, with what
the task waits on and how many gates are in each state. What it waits on follows
its status: in review, the gates missing or held; in concept, design or progress,
its executor, said `lost` or `left` when it is gone, coloured as missing when nobody
works it; blocked, the tasks it depends on; before it starts (backlog, ready),
nothing; and after those, while it is open, each gate held (a card waiting on the
owner). A done or cancelled task waits on nothing. A needed stamp of a task not
started yet (backlog to ready) with no branch says `No branch yet`, a started one's
that its branch is not in the repository. On a phone a gate is
three lines, wide one row of bounded columns, a part whose words need more room
wrapping within its own, so every word shows and none waits on a hover
(`apps/symphony-web/src/gates.tsx`). A link is kept only for a path of the
app or an `http(s)` address (`symphony_api::linkable`): the server drops any
other where it puts the answer together, and the page draws any other as words,
so a plugin cannot hand a page a `javascript:` link in the origin every project
shares. A path of the app is written under the project's base, as every link the
client draws is. Until it is read, the section is its own outline, a shadow row
for each gate it expects: the shape of the last answer the server gave for the
task (how many gates, how many waited on, how many with no who and when, whose
rows draw none), which the task page carries (`TaskPage::gates`) and the server
forgets once the task closes, else what this browser last drew, at the height it
drew it, else four, and nothing when it knows there are none. It is read as the
page mounts, behind no other read, and again when the task's record changes, one
of its builds or tests starts or ends, the branch it was answered for has a new
tip (the feed's `branches` region, the branches whose refs moved), a verdict lands
on its thread or a card in a document it links comes or goes; never for a plain
post. It is read once at a time: an event that asks while a read is out joins it,
and the section is read once more after it only when the answer was read before
that event (the cursor every JSON read names, `symphony_api::VERSION_HEADER`). A
read that fails is tried again by itself, the last answer shown meanwhile.

The Symphony answers its own. A top-level task that is open, or that has an assessment
([Needed stamps](#needed-stamps)), has a gate for each stamp of the project's catalogue, in
its order (`crates/symphony/src/gates/stamps.rs`), each read at the branch's tip by its
reader (`crates/tasks/src/stamps/read.rs`). A stamp the task needs there is met, missing
or, for `owner`, held. A stamp its assessment left out is waived, the assessment's reason
its why, its assessor the who and its line the link; with no assessment, a stamp the
catalogue does not need by default is waived, saying so. A stamp stands on the tip alone:
evidence naming another commit, a merge of the default branch included, is stale, not met,
and the gate names the commit it stood on. A commit is named by a word of 7 to 40 hex
digits, one digit at least, that the tip's id starts with. A done task whose branch the sweep
removed is read at the tip its land merged: the second parent of the merge its landing's line
names (`landed SLUG on main at MERGE`), else the commit the land's own line names (`land SLUG:
merged TIP as MERGE`). With no tip, an open task's needed stamp is not met, and a closed task's
is unknown, never missing, each saying so. A catalogue the board cannot read leaves the
gates read as below, for a task with no stamps, and adds one gate, `stamps`, unknown, saying
why.

- **review**: the reviewer verdicts on the task's thread, one round each
  (`gates::review_rounds`, the Symphony's one reader of review rounds; the board's
  own commands in `crates/tasks` read the verdict words apart): met by an `approve`
  in the last round that names the tip, missing until then, the earlier rounds its
  past; an approve naming another commit, or none, is missing, naming what it
  approved, the commit a merge of the default branch was made on among them: only the
  land, which reads the repository, tells that an approve stands over that merge
  ([the default landing](#the-default-landing)). A task done with no round, its assessment not having waived `review`,
  landed without review: missing. A task done by a covered close
  ([below](#a-task-another-landing-covers)) says so.
- **tests**: the board's recorder's runs ([MACHINE_WORK](MACHINE_WORK.md#recording-builds-and-tests))
  of the task and the work under it, at the tip: met when the newest run there of each
  build and test command the task ran is green and one test run is among them; missing
  while one is red, still going or queued, or when none ran at the tip, the last run
  elsewhere named.
- **pictures**: a post on the task's thread naming the tip and showing a picture (`png`,
  `jpg`, `jpeg`, `gif` or `webp`) under the task's artifacts folder (`artifacts/SLUG/` by
  default) or uploaded (`/artifacts/uploads/`, the owner's or an agent's `symphony attach`):
  met; missing while the newest such post names another commit or none, or there is none.
- **owner**: the owner's approve naming the tip, a line of the owner's whose first word is
  `approve`: met; held until then. Needed or not, it is held while a card the task posted
  waits on the owner, by the sign-off lane's rule above. Until the task page is ported
  ([the design](analysis/needed-stamps.md#the-task-page)), its approval offers the stamp:
  where no plugin's board action offers anything, the Symphony's own, `owner-approve`,
  offers `Approve SHORT` while the task needs the stamp at its tip and has none there, open
  or done by an approve no landing has closed (where a land waiting on the owner leaves it),
  and posts the owner's line `approve TIP`, the whole tip, through `POST /api/task-action`,
  refused unless its payload is one commit's id.

A subtask, and a task done or cancelled with no assessment, keep the gates they had before
stamps, so no task's history is judged again:

- **review**: met by an `approve` in the last round, missing until then, the earlier
  rounds its past. A task done with no round landed without review, as a bug fix filed
  before stamps might: waived. A subtask has none: only its top-level
  task goes to review.
- **owner**: held while a card the task posted waits on the owner, by the sign-off
  lane's rule above; absent otherwise.

A task with work of its own to check has one more of the Symphony's:

- **check**: the project's `[verdicts] check`, its words joined by spaces, as the
  board's recorder labels a run of it ([MACHINE_WORK](MACHINE_WORK.md#recording-builds-and-tests)),
  at the branch's tip: met when the newest such run there was green, missing when it
  was red or is still going, unknown when none ran at the tip. There is none for a
  task whose branch holds no commit of its own (no such branch, one made at the default
  branch and not yet committed to, or one already landed there), for an instruction
  subtask, whose work lands with its task, or for a project with no check.

The server keeps what its own gates are read from, each part by what tells it has not moved
(`crates/symphony/src/gates/inputs.rs`), and puts the gates together from them on every read:

- the task, its thread and the tasks under it, by the task's stamp on the board
  (`Store::task_stamp`): its row, links, roles and executors' recorded branches, the tasks
  right under it and its thread's newest line, in one small read;
- the recorder's runs, by their files: the history and each operation's record are read again
  once the file is another, by its length, when it changed and its inode, and an unfinished
  operation's lease is asked on every read;
- the stamps catalogue, by the commit the main checkout's head is at;
- whether the tip holds work of its own, by that tip and the default branch's.

So a read with nothing new asks the board once, for the stamp, resolves the three refs, and reads
nothing else again; a post, a role, a recorded run, a new tip or a commit on the default branch
shows at the next read. One thing shows later: a task moved under one of the task's subtasks
joins the runs its `tests` stamp counts at the task's next post, role, status or subtask. The
land reads its own, never what the page keeps.

A project's plugin answers its own through the gates seam (`symphony_plugin::Gates`,
`Plugin::gates`) as the same typed data, so every project's gates are drawn the
same way and can be counted. A plugin's gate with the same name as one of the
Symphony's takes its place. The server asks all of a project's answerers at once
and keeps each answer by the task's status, its branch's tip and what the answerer
says it reads besides (`Gates::kept_while`), so an answer that starts a landing
binary runs once a tip. A read waits 10 s for an answer. Past that, or when the
answerer errs or panics, each gate it answered before shows `unknown`, the error as
its detail, in place of the Symphony's of that name (before it ever answered, one
gate named after the answerer), and the rest of the answer still arrives. A late
answer is kept for the next read, and an error is asked again after 30 s. A call
still running after 60 s is abandoned and asked again; while four of one
answerer's calls run, abandoned ones among them, none starts, so an answerer bounds
its own work.

## Needed stamps

What a task needs to land is the stamps its risk calls for, named by an assessor from one
catalogue ([the design](analysis/needed-stamps.md)). The board holds the catalogue's reading,
each task's needs (`crates/tasks/src/stamps.rs`) and each stamp's reader at a tip
(`stamps/read.rs`); the task page's [gates](#gates) draw them, and the default landing lands a
task once every stamp it needs stands on its tip, by the same readers ([the default
landing](#the-default-landing)).

- **The catalogue** is the project's `[stamps]` ([PROJECT_FILE](../operations/PROJECT_FILE.md#stamps)), read
  from the main checkout's committed `symphony.toml`, so neither a branch nor an edit not yet
  committed changes what a task is held to; with none written, the Symphony's four: `tests`,
  `review` (needed by default, today's rule), `pictures` and `owner`. A command that reads a
  catalogue the board cannot read (a stamp with no reader, a key no stamp has, a table naming
  none) is refused, naming it.
- **A change of settings** needs no reviewer. The Symphony's `review` skips a change of the
  values the project file gives settings the Symphony already reads (a model, an agent, a
  limit, a count, a name), written, changed or removed, with the guide's and the rules' words
  that repeat those values and nothing else; its `tests` applies to it, met by the settings
  check ([PROJECT_FILE](../operations/PROJECT_FILE.md#the-settings-check)) recorded green as a test run at
  the tip, and by no other suite. So its assessment is `tests` alone and the land lands it on
  that run. `review` still applies to a setting the Symphony does not read yet or reads another
  way, which is code, and to a value that is a command or a program the Symphony runs or that
  decides what lands: `[project]`'s `cargo` (the command the server rebuilds itself with) and
  `default_branch` (the branch tasks land on), `[plugins]`'s `crates` and `process` (the code
  and the command that answer for the project), `[verdicts]`, `[land]` and `[releases]`, which
  `[land] check_paths` names, `[stamps]`, the workflow's three tables (`[workflow]`,
  `[[workflow_mappings]]` and `[[workflow_authorities]]`: its stages, its guards and the
  authorities whose word meets them), and `[board]`'s `on_review`, `on_land`,
  `machine_work_reader` (an executable the server runs), `postgresql` (the folder whose
  PostgreSQL programs the board runs), `external_postgresql` (where a hosted board is kept and
  the budget of its connections) and `assessments` (where a project's landing assessments
  are read). Every other key is a name, a number, a switch or a folder of no program the
  Symphony runs. The `tests` reader takes
  any green test run at the tip: that the run is the settings check is the assessment's reason
  and the thread's evidence.
- **`just tasks stamps [SLUG]`** prints the catalogue, where it was read and each stamp's
  `met`, `applies` and `skips`, the stamps a task nobody assessed needs marked `(needed by
  default)` and those a gate decision may waive `(waivable at one tip by a gate decision)`.
  Given a task, it adds what an assessor reads first: `NEEDS`, the needs as they
  stand (the newest assessment, its tip, who and why, or the catalogue's defaults with none);
  `TIP`, its branch and that branch's tip in the main checkout (none before the branch is made,
  when an assessment reads the brief alone); `GATES`, the task's gate decisions that stand
  ([below](#gate-decisions)); and `CHECK PATHS`, the paths the branch changes
  since it left the default branch that `[land] check_paths` names, which the `review` guideline asks
  for, so an assessor need not find the tests in a diff.
- **`just tasks runs SLUG [--tip SHA]`** prints what the recorder kept of the task at its
  branch's tip in the main checkout, or at the commit `--tip` names there, read as the `tests`
  stamp reads it (`crates/tasks/src/cli/commands/runs.rs`): `TIP`; `RUNS`, a row for each build
  and test command the task and the work under it ran there, its newest run's result (`green`,
  `red`, `running`, `queued`), when, how long, who, why it is red, and how many runs of it came
  before there, then, for each red run there that kept a log (a run of a pipeline,
  [RUNS](RUNS.md)), the newest of its command or one before it, a `LOG` line with when it ran and
  the log's path, where the command's own output says what failed, at its end, whatever its
  runner; `REFUSED`, when there are any, a row for each run there its runner refused, with
  the refusal's line ([what clears what](#what-clears-a-red-run)); `TESTS`, the `tests` stamp met or not with its reader's line; `CHECK`, the
  project's `[verdicts] check` as the task page's check gate reads it, its newest run there by
  anyone or `no run`; and `ELSEWHERE`, how many of the task's runs are at other commits, none of
  them the tip's. It is what a reviewer reads in place of building again (RULES.md
  `workflow/no-second-build-in-review`, [CHECKS](CHECKS.md#no-second-build-in-review)), and what
  an executor reads before it asks for review. It is refused for a task the board does not hold,
  outside a checkout of its repository, and with no tip to read.
- **The `owner` stamp** stands on a tip T by one reader, which the land, the task page's gate
  and its Approve offer, and the end of a `check:owner` hold all go by (`read::owner_at`, the
  cards' part `crates/tasks/src/stamps/posted.rs`): the owner's `approve SHA` naming T on the
  task's thread, or the owner's approve of the cards the task posted for the owner's look, read
  from git in the main checkout, never from a worktree's files. The cards' approve stands when
  the branch posted a card (one in a project document at T, inline where it decides or in a
  register, that the merge base of T with the default branch holds no copy of in any document, the
  owner's lines and its mark aside, as the pages read a branch's posted cards), each card it posted
  has `approved` as its newest owner verdict at T, committed (a reject, a question, pending,
  dismissed or none, on any of them, and it does not), and nothing came after the newest of those
  approves but the owner's lines: every commit on T's first-parent line after the one the owner's
  page showed when the owner gave it (the commit before the first that carries the approve) adds the
  owner's lines under those cards and nothing else, or is a merge of the default branch that is
  git's own merge of its two parents. Any other commit after it, code, a document or a card's text,
  keeps it from standing, whoever reviewed it: the owner asked for a look of their own, and its
  reading names that commit. Met by the cards, the reading names them, `Approved by the owner on
  card NAME, at 1a2b3c4`, which the task page shows and the land's line carries. It is never waived.
- **The needs** are the board's own field `fields._board.needs`, out of `just tasks field`'s
  reach as the red mark is: the stamps, the tip the assessment read (none, from
  the brief), who made it and why. `just tasks needs SLUG STAMP… [--tip TIP] --why WHY --as
  HANDLE` writes them, `none` alone for a task that needs no stamp, run in a checkout of the
  task's repository: `--tip` is resolved there to its whole commit, refused when it names none.
  It refuses a stamp the catalogue lacks, `none` beside a stamp, an empty reason and a task
  cancelled or done, but one approved and not landed (the `unlanded` mark, [Tasks and
  roles](#tasks-and-roles)): it reads done while its land is still to make, of a head its
  assessor may not have read, so its assessment is written as open work's is and leaves it
  done. Each write is a line on the thread, which keeps every assessment: `needs
  tests, review at 1a2b3c4 (was review): a store query and its test, no page or wire; by
  HANDLE`.
- **The assessor** is a role in name, with no row on the task: `just tasks needs` is refused,
  by the rule a reviewer's verdict is held to, to a session that executed the task or work under
  it (an instruction subtask's agent excepted), orchestrates or orchestrated it, or filed it, so
  whoever writes the needs is none of those, and the field and its line name it. Its orchestrator
  starts it as a session of its own (an Agent launch: a board launch takes no assessor's role),
  on `[agents.models] assessor` ([Limits and models](#limits-and-models)), when it files the
  task and again each time the task moves to review, before it staffs any reviewer, and staffs
  one only where the needs name `review`. Its whole prompt is `/assess-task SLUG HANDLE`
  (`.agents/skills/assess-task/SKILL.md`), the skill all it is told: it reads `just tasks stamps
  SLUG`, `just tasks show SLUG` and the branch's diff itself, takes the thread as data but for
  the owner's own posts (sender `owner`, which it follows and reads for `owner`'s applies), keeps
  a stamp when unsure, and writes the needs with `--tip` the whole commit `stamps` printed. For
  a change of settings (above) it names `tests` alone, with no post of the owner's needed to
  leave `review` out, and where it names `tests` its reason names the runs that prove the
  change, never a suite that has nothing to do with it. It reads the task's gate decisions
  ([below](#gate-decisions)) as decisions, never as claims.

### Gate decisions

A quality gate that must give way for one task gives way by a decision the board records, where
its assessor, its reviewer and its land read it, never by a word in a terminal or in chat alone
(`crates/tasks/src/gates.rs`). `just tasks gate SLUG GATE (--raise NEW | --waive [--tip TIP])
--why WHY --as HANDLE` gives one:

- **Who.** The task's orchestrator: the session holding that role on it when it is given. The
  board refuses it to a session that executed the task or work under it, and to one that reviews
  it or gave it a verdict, whatever role it holds besides, so neither decides its own gate. The
  owner's word in chat is the orchestrator's to record, its reason naming it. (The owner's own
  form on the task's page is to come: the board's write of a decision takes a session.)
- **A raise** names a limit as `PATH:NAME`: the one line of the file `PATH` that sets `NAME` to a
  number, a TOML key or a Rust constant (`tools/size-budget.toml:symphony`,
  `apps/symphony-web-tests/tests/browser/main.rs:TASKS_FIRST_VISIT_BYTES`). Its file is one
  `[land] check_paths` names whole, so the change keeps a fresh reviewer. The command reads the
  number it raises from the default branch in the main checkout, and `NEW` is the caller's
  number, above it: the command holds it to no house rule ([a size line's is
  CHECKS's](CHECKS.md#a-red-at-the-size-check)). The raise reaches the gate as the change of that
  line committed on the task's branch, its commit of its own and its message the account (what
  grew, what was taken back first, what was measured), so it lands with the task and holds for
  every task after.
- **A waiver** names a stamp the catalogue marks waivable ([PROJECT_FILE](../operations/PROJECT_FILE.md#stampsname),
  `just tasks stamps` marks it), for one tip: `--tip`, else the task's branch's tip, resolved to
  the whole commit in the main checkout. In the Symphony's catalogue that is `pictures` alone:
  `tests` (a red or missing test run), `review` (a missing review where the stamps name one) and
  `owner` (the owner's own word, given by approving) are never waived, and a catalogue cannot
  mark them so.
- **The record** is the board's own field `fields._board.gates`, every decision kept, the newest
  on a gate standing, out of `just tasks field`'s reach; each is a line on the thread: `gate
  tools/size-budget.toml:symphony raised 12,390,000 -> 12,400,000: WHY; by HANDLE`, `gate
  pictures waived at 1a2b3c4: WHY; by HANDLE`. `just tasks stamps SLUG` lists those that stand
  (`GATES`). It refuses an empty reason, a raise that names no limit set once in a check's file
  or no number above the default branch's, `--tip` on a raise, a waiver of a stamp the catalogue
  does not let it waive, a task cancelled, and a task done with no land of it still to come:
  an approve leaves its task done until its land (the `unlanded` mark, or a custom workflow's
  pair it does not mark terminal), which still takes a decision. No schema moves: the field is
  the task's own.
- **The land** ([below](#the-default-landing)) reads them at the tip. A waiver for the tip lifts
  its stamp there, and the land's line says so; one for another tip lifts nothing, and the
  refusal of the stamp names it. A raise lands only where the tip sets its limit to the number
  decided, its line naming the decision; at another number above the default branch's it
  refuses to `@executor`; where the tip holds the default branch's number still, or lowers it
  below that (a limit lowered needs no decision), the raise raised nothing, and it lands saying
  so. The land of a task a custom workflow holds reads them before its prepare edge, and a
  decision is part of the evidence its intent binds; a land delivered by a pull request reads
  them the same way ([its evidence](../engineering/BOARD_API.md#the-evidence-of-a-task-at-its-head)). The task page draws a stamp a decision waived
  at the tip as waived, with the decision's author, reason and tip.
- **The assessor and the reviewer.** The assessor names no `owner` stamp for a limit a raise
  names, and keeps `review` for it; the reviewer still judges that the cut came first and the
  account is true, and its verdict names each decision at the tip, a raise it judged or a waiver
  it saw. A limit raised in a diff that no decision names is a changes.

## Subtasks

A task's own executor and reviewer need no subtask: the task's brief and thread are their
whole assignment. The executor's whole prompt is `/work SLUG HANDLE`
(`.agents/skills/work/SKILL.md`) and a reviewer's `/review-task SLUG HANDLE`
(`.agents/skills/review-task/SKILL.md`), on the task itself, never a brief in a file;
anything more for either that is this task's alone goes on the task's thread (`--to @executor`,
`--to @reviewer`). What a project tells every executor or every reviewer is no part of a prompt
either: it is the project's guides and rules, and for one role alone a `project.md` of the
project's own beside that role's skill (`.agents/skills/work/project.md`), which the skill reads
first and `symphony init` never writes. A
reviewer takes the reviewer's role on the task, so its verdicts are that task's review rounds,
and each later round goes to the same reviewer, resumed with the new tip on the thread (a
subagent by its parent, a `claude -p` run by `--resume`, a terminal by `just tasks launch SLUG
--role reviewer`, which resumes the task's recorded reviewer and is refused while it still runs,
or `just tasks restore SLUG HANDLE`), never to another reviewer or a review subtask. Resumed, it
is still a fresh reviewer for the land: an earlier round of its own makes no session unfresh
([the default landing](#the-default-landing)). Only when it cannot be resumed (no conversation recorded, so
`restore` refuses it; a Codex thread that never arrived; a reviewer that left) does another
reviewer take the role on the task.

A **subtask** is a task made with `--parent`: a slice of its parent's work, or an instruction
to an agent that is neither the parent's own work nor its review (a question, an errand), whose
description is the instruction, word for word. Its agent's whole prompt is `/work SLUG HANDLE`
on the subtask. A parent's subtasks nest to any depth; a task is never its own ancestor (`just
tasks parent SLUG PARENT` moves one, and refuses a cycle).

```
just tasks create perf/x/bisect "Find the commit that slowed perf/x's bench" --parent perf/x --for amber-heron --instruction --body-file - <<'EOF'
(the instruction, verbatim)
EOF
```

`--body-file -` reads the body from stdin, so a long instruction needs no file.
A consuming project's optional brief guard refuses writing a brief to a file: to
`artifacts/orchestrator/` (which must not exist), or to a brief in a session scratchpad (a file named
`brief*` or `*-brief.*`, or under `briefs/` or `followups/`): a brief is a task's
description, and anything more for its agent goes on the task's thread.
A consuming project's optional wait guard refuses a loop waiting on machine-wide process state
(`pgrep`/`ps` for verify, test-sim, godot): wait on your own run's log or PID.

- `--for HANDLE` reserves it for that handle with no lapse of the hour (the
  handle is one the orchestrator registered for the agent). `next` offers it to no
  one else, and only that handle can claim it. The reservation still drops when
  that handle reads `lost` or `left` (ten minutes without a sign of life, holding
  nothing claimable yet), after which anyone may claim the task: start the agent
  promptly, or register the handle again and re-reserve.
- Every subtask closes with its executor's `just tasks done SLUG "integration commit and focused result"`, including ordinary implementation slices. Only the top-level parent enters review, gathers stamps and lands. `--instruction` still marks an agent instruction, but is not required for lightweight completion. It does say whose work is on the parent's branch: a slice's executor wrote it and gives the parent no verdict, an instruction's did not ([Tasks and roles](#tasks-and-roles)).
- Source slices start from their parent feature branch, commit there or return commits for integration, and run only their focused checks. A separate agent is justified only by useful parallelism. No independent slice review or landing. A parent's gate is evidence too, the tests closest to its change, which its reviewer judges; the full verification is no task's, and is the release run's ([CHECKS](CHECKS.md)).
- `just tasks show` lists a task's subtasks with each one's status and holder;
  `just tasks list` and the Symphony's **tasks** page, under its **Task
  groups** heading, show the same tree, at any depth (`list` leaves finished
  tasks out unless `--all`). The Symphony's tree and a task page's
  **Subtasks** leave out a finished instruction to do or review the task
  itself (`SLUG/work`, `SLUG/review`, the staffing pass's
  `SLUG/staff-review-N`), filed before a task's executor and reviewer worked
  on the task itself: its verdicts stand on the task's thread, and the task is
  its row. Open, it is listed, where its agent is found
  (`crates/symphony/src/taskview.rs`, `listed_under_parent`).
- Events climb: a message to `@orchestrator`, any verdict and any
  status change on a subtask reaches the orchestrator of the nearest task, at
  or above it, that has one, so the orchestrator takes the role on the root
  only. A subtask handed to another orchestrator is that one's, with what is
  below it. A message to a handle or to another role stays with them.

## Limits and models

Every work-in-progress limit, and the model each role is delegated to, is a setting, so a change
of conditions or of mind is one edit of one file. Each is a key of the project's `symphony.toml`,
read from the repository's main
checkout as `on_review` is, so a branch cannot change its own (`crates/tasks/src/limits.rs`,
`crates/tasks/src/launch/defaults.rs`):

```toml
[limits]
orchestrator_slots = 8  # the tasks in progress an orchestrator holds (Slots, below)
orchestrator_tasks = 16 # the open tasks an orchestrator holds the role on (The limit…, below)
low_volume = 4          # PULL's low_volume line below this many open slots; 0 says it to none
machine_agents = 12     # the agents this machine runs

[agents.models]
orchestrator = "fable"
executor = "opus"
reviewer = "opus"
assessor = "haiku"
```

- **Defaults.** A key not written takes its default: `orchestrator_slots` none (no cap on an
  orchestrator's slots), `orchestrator_tasks` 16, read from `[limits]` alone and at least 1,
  `low_volume` 4, `machine_agents` none (no limit on the agents a
  machine runs), `orchestrator`, `executor` and `reviewer` none (a launch takes `[agents]
  launch`'s `model`, else the board's own, as **Models** below says), and `assessor` the
  smallest model its agent offers (Claude's `haiku`; for Codex, whose smallest the board does
  not name, as for the others). `orchestrator_slots` is
  also read from `[board]` where `[limits]` names none, where a build that does not know
  `[limits]` reads it; written in both with two values, `[limits]`'s holds and the other is
  said on stderr.
- **`machine_agents`** counts every agent the machine runs, whoever started it, as the staffing
  pass counts: each hc terminal running an agent, and each live worker, reviewer or
  orchestrator session of this board with no terminal of its own
  (`crates/tasks/src/launch/running.rs`). `just tasks launch` refuses at the limit (`no
  launch: this machine runs 12 agents, at its limit of 12 ([limits] machine_agents in PATH)`),
  and when hc cannot be asked to count them. `just tasks next` warns past it on stderr and
  still names the task: whoever asks runs already, and refusing it work frees nothing. The
  staffing pass defers at the lower of it and its own `max_sessions`.
- **Models.** A launch of an executor or a reviewer, by hand or by the staffing pass, starts
  `[agents] launch`'s agent on its role's model, else on `[agents] launch`'s `model`, else on
  the board's own; `--model` and `--kind` win, and a role's model goes with the project's
  agent. No board launch starts an orchestrator: with no model written for it, whoever starts
  one chooses. An assessor ([Needed stamps](#needed-stamps)) decides which of a few stamps
  apply, from a diff and their guidelines, so with no model written it runs on the smallest its
  agent offers. An agent starting a subagent passes the model `just tasks limits` names for the
  subagent's role.
- **`just tasks limits`** prints each limit, the pipeline alarm's thresholds
  ([The pipeline alarm](#the-pipeline-alarm)) and each role's model, each with where it came
  from (`[limits] low_volume, line 8`, `[agents] launch model, line 12`, `the board's store
  [limits] low_volume, over 4 ([limits] low_volume, line 8)`, `the smallest claude offers`, or
  `default`), the
  machine's count of agents under `machine_agents`, and each key of `[limits]` this build does
  not know, left out.
- **A change holds at once.** Every command reads the file as it stands, and a store kept open
  (the Symphony's server keeps its reader and its data worker's) reads it again at its next
  read whenever the file's length or time changed: no restart.
- **The board's store.** A preference stored on the board (`just tasks settings set KEY VALUE`)
  is read after the project file and before the local file: a limit, `[agents] launch`'s agent,
  model or effort, a role's model or `[agents] panes`. It holds from the next command, `just
  tasks limits` names the store for it with the committed value it stands over, and `just tasks
  settings remove KEY` puts the file's value back. Each change is kept in the entry's record
  (`just tasks settings list`); what the store may hold, and how a value is judged, is
  [PROJECT_FILE](../operations/PROJECT_FILE.md#the-boards-store) (`crates/tasks/src/settings.rs`).
- **The local file.** `symphony.local.toml` beside the main checkout's project file, which git
  does not version, is read last: a key of `[limits]`, `[agents] launch` or
  `[agents.models]` it writes holds on this machine in the place of the committed one and of a
  stored one, and `just tasks limits` names that file and its line for it. It holds no other
  setting, and one that is refused is said on stderr and read by nobody, the committed settings
  holding
  ([PROJECT_FILE](../operations/PROJECT_FILE.md#the-local-settings-file), `crates/tasks/src/local.rs`).
- **A value the board cannot read** is said on stderr with its line and takes its default
  (`⚠ PATH: [limits] low_volume (line 3) is -1; it is at least 0; it takes its default, 4`): a
  broken project file must not stop the board, nor the landing that mends it. A load of the
  project and `just symphony-status --lint` refuse it in the same words, as they refuse a key
  the Symphony does not know, by name and line (`[limits] wip (line 4) is not a setting the
  Symphony knows`).
- **Staging a new key.** A build older than a key does not know it: its `just symphony-status
  --lint` and its load of the project refuse the file, naming the key, its server reads the rest
  and says the key is left out, and its board never reads it. So a project writes a new key only
  once every build that reads its file knows it: the machine's shared server, and the `symphony`
  and `tasks` its recipes and lands run (its main checkout's `.target/bin/` links), which a
  project taking the Symphony as a Git dependency builds at the Symphony commit its
  `Cargo.lock` pins, so that pin must be one that knows the key. Until then the setting stays
  where those builds read it (a cap in `[board] orchestrator_slots` holds for all of them); once
  they all know the key, the value moves, and while builds are mixed it can be written in both
  tables with one value, the older key dropped after.

## Slots

An orchestrator holds at most a set number of tasks being built, its slots, so
work is pushed to review and done before new work starts
(`crates/tasks/src/slots.rs`).
The number is the project's: `orchestrator_slots` in the `[limits]` table of
`symphony.toml` ([Limits and models](#limits-and-models)), else in its `[board]` table,
where builds before `[limits]` read it, from the repository's main checkout, as
`on_review` is, so a branch cannot raise its own cap. With no key, or no file, nothing is
capped. A value that does not parse, or one below 1, is said on stderr with its line
(`⚠ …; no cap on an orchestrator's open tasks applies`) and caps nothing.

- **A slot** is held by a task `in_progress`, or `blocked` under a hold that
  keeps the slot (below). A task in `review` is open and holds none (below);
  other `blocked` work, `backlog`, `concept`, `design` and `ready` hold none
  either. A slot frees when its task moves to `review`, is `done` or
  `cancelled`, or leaves the statuses that hold one another way.
- **Work in review takes no slot.** The cap counts the tasks being built, not
  the review of them: a task's slot frees when it moves to `review`, and a move
  into `review` is never refused. The work under a task in review goes with it,
  judged by the task whose work it is and the tasks above it, as red work is: a
  reviewer's review mirror or instruction subtask in progress takes no slot
  either, and starts at any count. The task's return to `in_progress`, on a
  changes verdict or a move, goes between open statuses and starts nothing: it
  is never refused, and counts again from then, as it was being worked already.
- **Each task being built, one slot.** A subtask holds a slot of its own, so
  gathering work in progress under one task frees none. Two kinds of task share the
  slot of another: a review mirror (`review/BRANCH`, `review-N/BRANCH`,
  `SLUG/review`) counts with the task it reviews when the board holds that
  task, and an instruction subtask (`instruction: true`) with the nearest
  task above it that is not one.
- **Whose slot.** The orchestrator of the task, or of the nearest task above it
  that has one whose session still holds ([Subtasks](#subtasks)); a task two
  orchestrators hold counts against both, and a start is refused if either is
  full. A task no orchestrator holds yet, until the board gives it one
  ([Tasks and roles](#tasks-and-roles)), counts against the parent session
  (`register --parent`) of the session starting it, or of its executors once
  open. No cap refuses the board's give: the task then counts against the orchestrator given it.
  The one give a cap decides is a move's, which keeps the moved work with the orchestrator it had
  (below), decided by that move's own cap read.
- **Where it bites.** A claim into `in_progress`; a move into `in_progress`
  from a status that holds no slot; a dependency change that sends blocked
  work back to `in_progress`; taking the `orchestrator` role on a task with
  work in progress at or below it, or being handed it (`handover`, refused as the
  receiver's take would be); and a change of parent that brings work in progress at
  or below the task under an orchestrator's tree, or takes it out from under one, which keeps it
  that orchestrator's in a slot of its own. Each is refused when it would raise an
  orchestrator's counted tasks past the cap, naming them and their status, a
  shared slot's tasks joined by `with`, and then its work in review apart:
  `sim/x cannot start: cobalt-otter holds 8 open tasks, the cap of 8 an
  orchestrator holds: sim/a (in_progress) with sim/a/build (in_progress), …; a
  slot frees when one of them goes to review or is done or cancelled; not
  counted, as work in review takes no slot: sim/b (review)`. The count is the
  number of entries listed before the work named apart. `tasks api` holds the
  same cap. A start inside a slot already open (a mirror or an instruction of a
  task in review, an instruction for work in progress) is never refused.
- **A landing's hold.** A landing that holds a task in review blocked while
  every stamp its branch lacks is one nobody is producing does it with a hold
  (`Store::hold_blocked`, `tasks api`'s `hold`, api version 4; naming what it
  waits on, `Store::hold_blocked_on` and `hold`'s `on`, api version 15): the task
  stays blocked, as any blocked task does to everyone on it, and keeps its
  slot, shown as `(blocked, held)` and counted as work in progress is, so its
  move back to `in_progress` starts nothing and is never refused, as its move
  back to `review`, which takes no slot, never is. The hold is the record
  `counter:['hold', SLUG]`, stamped with the task's `updated` as the hold left
  it, and ends with the task's next move (a landing's close and the owner's
  moves included: every status write deletes it), change of parent or of
  dependencies.
  Work blocked by any other move holds no slot, and takes one to come back to
  `in_progress`.
  The PULL header and the agents API do not read holds: they count a held task
  as the blocked task it is.
- **Red work takes no slot.** A task filed for a red that stops every
  landing is marked so where it is filed (`create`'s `red`, naming what is
  red, `tasks api` version 7; kept as the board field `fields._board.red`,
  `crates/tasks/src/red.rs`). Only a red's filer marks: a session of
  kind `filer`, which `tasks api`'s `register` takes and the command line's
  `register` does not, as the project's tools that file reds register. A
  create naming `red` from any other session is refused, and `set_field`
  refuses the board's own `_board` fields. The mark goes only on a task of a
  series reds are filed in (`red::SERIES`: `tests/main-red-*`, a red verify
  of main, `tests/release-red-*`, a red release run, and `tests/nightly-red-*`,
  its name before the rename and a consumer's nightly's), whoever marks it, so
  no feature takes it; never on a task named under one (`red::filed`: no `/`
  past the series' prefix), which the release run's reading of its reds and
  land's of the standing reds take for ordinary work too. `mark_red` marks an open red its
  filer filed before it marked reds, and makes it p0, once: what it judges is
  read in the transaction that writes, so two at once post one line. While the
  marked task is open, it and the work under it take no slot: subtasks,
  review mirrors and instructions are judged by the task whose work they are
  and the tasks above it. Red work starts at any count and frees none. A red
  done or cancelled exempts nothing: the work under it counts again, new
  work there included. Its close is never refused, though it raises its
  owners' counts: whether or not the closing store caps starts (the
  release run's does not), it holds their slot counters and the red's own,
  `counter:['red', SLUG]`, which every start of work under the open red holds
  too, so a start decided on a board read before the close is decided again,
  counted. A close decides what it holds on the task as it read it, and its
  write goes on only from that version, so a red marked between the two
  (an earlier red the release run or main's verify marks) is read and decided
  again, and a task it read as not on the board is refused there, never
  closed unfenced as a red filed since. `unmark_red`, from a filer too, lifts
  the mark of an open red that stays open as ordinary work (main's green
  reached it once work on it started): it and the work under it count from
  then as any work does, none while it is in review, decided and held as a
  close is, never refused; a task unmarked or
  closed is left as it is. A refusal names the owner's open red work after its
  counted slots: `…; a slot frees when one of them goes to review or is done or
  cancelled; not counted, as red work takes no slot: tests/main-red-abc
  (in_progress)`. The
  mark alone tells; a slug never does. The PULL header's `slots=` and the
  agents API count no red work. As with review, nothing authenticates a
  session: a filer is honour-based, guarding against a mistake, not malice.
- **What it leaves alone.** The owner's moves, changes of parent and
  hand-overs are not capped.
- **Not retroactive.** An orchestrator above the cap keeps every task and
  works them; it starts nothing new until it is below the cap.

Two starts at once cannot both take the last slot: every start that changes an
orchestrator's open tasks, one into a slot already open included, holds
`counter:['slots', HANDLE]`, read in the same transaction as the task it
starts, and the write that loses is decided again on the new board. The cap
reads the open tasks through the `task_status` index, the holds as a range of
record ids and their tasks by id, the tasks above them and the tasks they
review by id, a level a round trip, and their roles through `role_slug`; it
reads no table whole (`tests/store/slots/cost.rs`). Each
orchestrator's PULL header shows `slots=OPEN/CAP`, and the agents API its
`slots` ([The agents API](../engineering/BOARD_API.md#the-agents-api)), the slots the cap counts: no
work in review and no red work.

## The limit on the tasks an orchestrator holds

An orchestrator holds the orchestrator role on at most a set number of open tasks, so one session
does not answer for the whole board while others wait for work
(`crates/tasks/src/orchestration.rs`, `crates/tasks/src/store/work/given.rs`). The number is the
project's: `orchestrator_tasks` in the `[limits]` table of `symphony.toml` ([Limits and
models](#limits-and-models)), 16 with none written, read from the main checkout as the cap is. A
value that is no whole number, or one below 1, is said on stderr with its line and takes 16, and
the settings check ([PROJECT_FILE](../operations/PROJECT_FILE.md#the-settings-check)) refuses it (`[limits]
orchestrator_tasks (line 2) is "sixteen"; not a whole number`).

- **What counts.** Each task not done or cancelled whose orchestrator role the session holds
  itself: in review and blocked too, as the limit bounds what one orchestrator answers for, not
  what is being built ([Slots](#slots)). The work under those tasks, its own by the tree, counts
  for nothing, nor does a red ([Slots](#slots), **Red work takes no slot**), which goes to an
  orchestrator that stays whatever it holds and is taken at any count, so no red waits.
- **Where it bites.** At the limit, `just tasks role SLUG orchestrator` is refused, and so is a
  hand-over to it (`handover`), one line naming the setting: `amber cannot take orchestrator on
  sim/x: amber orchestrates 16 open tasks, the limit of 16 an orchestrator holds ([limits]
  orchestrator_tasks): it gains none until one of them is done or cancelled or it lets one go`.
  The board gives it none: a filing passes it over and says so ([Tasks and
  roles](#tasks-and-roles), **Filed**), and the board's give at a leave, a reopen and every read
  of the whole board takes the next that stays and is under it, else marks the task waiting. Where
  the places left are fewer than the tasks with no orchestrator, a pass gives first the tasks that
  wait on nothing, each dependency the board holds being done, and then the ones that still wait on
  another, which nobody can start whoever orchestrates them, each group in the board's order
  (`crates/tasks/src/orchestration.rs`). The owner's hand-over, and a move that keeps moved work
  with the orchestrator it had, are held to no limit, as the owner's moves and hand-overs are not
  to the cap.
- **A task with no orchestrator is well.** It waits for one under its limit that stays: `just
  tasks show` says so and names the `role` command any orchestrator under its limit takes it with,
  every orchestrator hears it, and the board's health counts it and warns of none ([Board
  health](../operations/BOARD_OPERATIONS.md#board-health)).
- **Past the limit.** The limit takes nothing: an orchestrator holding more (one from before the
  limit, or the limit lowered) keeps every task and works it, gains none but a red until it is
  under, and is told in its PULL header (`orchestrates=66/16`, shown at the limit and past it) and
  once in a line of its own until it is under again: ``over_limit HANDLE orchestrates=66 limit=16 —
  50 past it: it is given and takes no task but a red until it is under; let the unstarted ones go
  with `just tasks release SLUG --role orchestrator` ``. Past the limit, such a release of a task
  with nothing of it or under it in progress or in review goes though no other orchestrator could
  take it: the board gives it at its next read to one under its limit, and it waits while none is.
  Work started stays its own until it is handed over or done.

Two writes at once cannot both take the last place: a take and a hand-over held to the limit read
the taker's open tasks with its slot counter, `counter:['slots', HANDLE]`, in a read of their own
between the rule's read and its write, which holds that counter as read, and the board's give
holds each receiver's counter as it read it before the board, so the write that loses is decided
again on the new board.

## Asks

An **ask** is one thing the owner waits on an agent for: a verdict with a note or
a rejection, a message on a card, a comment in a document, a chat, a message on a
task's thread. It is a record of its own (`ask:N`, schema 17), never a task: it has
no stage, no claim and no review, and no task count includes it. Each one says
what it is about (the card on its document, the commented block, the chat's
thread or the task, and the worktree's branch when the owner said it on one), the
owner's first words (cut at 200 characters), when they were said, how many lines
followed, who holds it now and since when, and each earlier holder with when it
had it and why it moved on.

While an ask is open, another line from the owner about the same thing joins it:
the line is counted, and the ask keeps its first words and its age. A verdict and
a message on one card are one thing. A line after the ask was answered opens a new
one. The first answer closes an ask, whoever holds it, and a later answer changes
nothing. An agent closes an ask that needs no answer, such as a thank-you, with
`just tasks asks close ID "WHY"`. An open ask shows as late from ten minutes
(`LATE_SECONDS`, the same for every kind), and an answered one never does: its
age is how long it waited for its answer. The
owner's words and the answers stay where they were written: the ask records only
what it is about, who has it and whether it is answered.

`just tasks asks` lists the open asks, the oldest first:

```
ask 3  verdict  card on docs/DECISIONS.md @tools/x  14 min, late  held by umber-stoat for 12 min
  "why not B?"
```

`tasks api` opens, holds and answers them: `open_ask` (the kind, what it is about,
the words and, if not now, `at`), `ask`, `asks` (`since`: answered ones too),
`hold_ask` (`holder`, none for nobody, and `why`), `answer_ask` and `answer_about`
(`by`, an agent's handle, and `answer`, where the answer is), and `close_ask`.
The [design](../product/analysis/owner-asks.md) has who holds an ask and when it moves.

Who opens and closes them:

- **A save in the Symphony.** A verdict that leaves its card waiting on an agent
  (a rejection, a note, an approval its card kind says an agent applies), a
  message on a card, a comment and a chat question or follow-up each open their
  ask, or join the open one, in the same board write that queues the save's chat
  event: the save's journal record carries both, and the shared server hands
  them over once (`Store::owner_save`). A rejection with no note queues no chat
  and opens its ask alone.
- **A message on a task's thread.** The owner's message from the app opens the
  task's ask in the message's own write. A line on the thread by a session
  holding a role on the task, or by the one the owner's last line there was
  sent to, answers it. A line the app posts as the owner for a board action with
  no note opens none.
- **The shared server's clock.** It closes an ask once its answer can be read:
  one on a card once main's page holds the card, on the branch's page the owner
  saw it on or as main's, waiting on no agent; one on a comment once the comment
  is resolved, in its document (the worktree's for a branch's) or in the review
  ledger the tidy archived it to; a chat once its thread in the ask store is
  answered. Main's copy of a card answers, or keeps waiting, an ask said on a
  branch only when that copy carries the owner's line, one of the owner's lines
  dated from the minute before the ask on: the branch merged, or its line laid
  on main's copy for a finished branch or a card the branch did not post.
  Without it, main's copy says nothing of the branch's agent. It names who
  answered: a chat's answering session, and on a card or a comment the session
  that wrote the answer with `just symphony-reply PATH
  SLUG TEXT --as HANDLE` (a comment's slug is `L` and its callout's line). The
  register's lines name only `agent`, so an answer written into the file by hand
  is credited to no one. It looks when the documents were read again or the ask
  store changed, and a minute after it last looked
  (`owner_asks::RESCAN_SECONDS`). An ask the owner said more to after the
  documents were read waits for the next read. A private server opens and
  closes none.
- **Gone is no answer.** An ask whose card, comment, document, worktree or chat
  thread is gone (the task cancelled, its worktree removed, the document
  deleted) stays open, and `just tasks asks` says what is gone and for how long
  (`gone` and `gone_since` in `--json`). A branch's card whose worktree was
  removed unmerged is gone even where main holds the card: main's copy, without
  the owner's line, marks it `the worktree of BRANCH is gone, and main's copy of
  card SLUG does not carry the owner's line`. The mark comes off when the card or
  comment is found again; the ask closes when its answer can be read, or by
  hand with `just tasks asks close`. A gone ask is open, so it counts with the
  open ones and never in the answered figures. A read that says nothing changes
  nothing: a `git worktree list` that failed leaves every ask on a branch as it
  was; a listed worktree git could not read (`Scan::failed`) leaves every ask on
  a card of its branch as it was; and so does a document that could not be read.

The `verdicts/…` tasks the app filed before asks are history: no save files one
any more, and no server start touches them. `just symphony-cli verdict-asks`,
run once in a project's main checkout, turns each one still open into asks:
every card it lists that still waits on an agent becomes an ask keeping the
owner's time, and the task is cancelled, as the app's own `app` session, then
gets a line naming the asks and saying so. One an executor holds is not
cancelled: it stays with its holder, and its line says that and why. Run again,
it turns nothing more, as each turned task's line says so. On
a project's live board it is refused outside the main checkout before it reads,
as `just tasks migrate` is; a scratch board it turns from anywhere. Until it
runs, an open verdict task stays the task it was, and an agent can still finish
it. Comments left open before asks are asks only once the owner says more on
them, unless `owner_asks::COMMENTS_OPEN_BEFORE` is set: then the same command
opens one for each, aged from the commit that wrote it (the design's second open
question, left to the owner).

**In this repository the landing is step 1.** Main's first build that writes
the board moves the live board to schema 17, and from then until the shared
server restarts on main's build, the server's board writes are refused, the
owner's messages on tasks among them; that restart needs the owner's own word.

A consuming project whose pin moves to a build with asks takes three steps in
its main checkout, in this order:

1. Relink the project's builds at `.target/bin/` (`tasks` and `symphony`) to
   the new pin, then run `just tasks migrate`, which moves its board to schema 17
   (its checkout holds no board source, so no other command does). Before the
   migration the new build is refused the board, naming `just tasks migrate`, and
   the old one still runs. From the migration the old `tasks` build is refused
   the board, as it writes only a board at its own schema, 16: a link, a session
   or a tool still on it fails until it is relinked to the new pin.
2. Restart the shared server on the new host build, a separate act that needs its
   own word.
3. Run `just symphony-cli verdict-asks` once.

Between step 1 and the restart (in this repository, from the landing), the old
server reads the board and its writes are refused:

- **The owner's messages on tasks are refused**, as every other write of the
  server is; the owner sends them again after the restart.
- **The owner's saves open no ask.** They are written into the documents and
  wait in the server's journal, and the new server delivers them on its start
  to the chat recipient as chat only: the old build's journal records carry no
  ask, and the new server opens none for them. Their asks are lost, not the
  saves: a card or comment saved then waits on an agent in the app's agent queue
  and in `just symphony-status --open`, and a chat question in `just symphony-ask
  list --open`. Each gets an ask once the owner says more on it.

From the restart, saves open asks, and the open verdict tasks wait as they were
until the last step turns them.

## Sessions

Every agent that works from the board has a **handle** (`amber-heron`), a
kind (`orchestrator`, `worker`, `reviewer`) and, for a subagent, a parent. The
handle is printed by `just tasks register`; later commands take it from
`--as`, else `TASKS_SESSION`. An agent writes it as `just tasks … --as HANDLE`:
no allow rule passes a command with an env prefix, so
`TASKS_SESSION=HANDLE just tasks …` stops at a prompt, while a session launched
with the variable already set needs neither. Then run `just tasks callback --as HANDLE`: it detects
the Claude inbox socket or Codex thread. An explicit `uds:/absolute/socket` or
`codex:THREAD-UUID` works too; through `just tasks` a `codex:` one only as the
session's own thread ([what the command may write](../operations/BOARD_OPERATIONS.md#what-the-command-may-write)). A session with no channel is listed as such and
keeps the inbox fallback. hc terminals are for inspection, never delivery.

`just agent-send HANDLE "text"` addresses an agent without a task. The sender
comes from `TASKS_SESSION` or `--as`; delivery is one plain
`AGENT from SENDER to HANDLE — text` event line. Reply with
`just agent-send SENDER "text" --as HANDLE`. Whitespace is flattened as in board callbacks.
It uses the callback outbox, identity checks, retries and inbox fallback. Output
distinguishes sent from queued; sent confirms native transport only. An agent
without a channel is refused with the command to register one and the option
to send on a task with `just tasks say SLUG TEXT --to HANDLE` for its inbox.
The Agents page shows each native address and the command to reach it. Scratch
boards use `just agent-send --database NAME HANDLE "text" --as SENDER`.

`just tasks callback --chat` registers the app's chat recipient. Chat selects
the explicit seat, then an `--if-free` fallback, then a live orchestrator with
a native callback, choosing the lexicographically smallest handle among plain
orchestrators. A normal `just tasks callback --as HANDLE` is enough for that
last fallback; `--chat --if-free` opts into the higher-priority fallback without
displacing the seat. Sweep-confirmed native presence counts as live. A recipient
suspended after delivery failures waits for the sweep to restore it. While no
channel holds the seat, an orchestrator reading by `just tasks wait` holds it:
a session is waiting from its first `wait` until it registers a channel (or
turns one off) or leaves, and of the live waiting orchestrators the smallest
handle receives chat at its next read; one that stops waiting holds it while it
stays live, so what the owner says waits for that read. While nobody holds the
seat, every orchestrator's `just tasks pull` ends with what the owner said that
nobody received, and its `just tasks inbox` takes it, after which nobody reads
it again: an owner's word never goes to no one unseen.
The app names the recipient, and says when nobody receives chat. Saves queue
asks, follow-ups, card messages, verdict notes and callouts directly; an outbox
beside the asks preserves them across a board outage. No file watcher discovers
chat. `--off` or leaving the board unregisters delivery.

Board writes dispatch addressed messages and role/status/new-work changes. A
command line's write queues what each recipient hears, as the write left the
board, and answers without delivering it: a detached `tasks dispatch` delivers,
one a host at a time, which makes one more pass for each write that commits
while it runs. One that finds another running waits until that pass has ended,
and takes over if the other's lease lapses first.
The shared Symphony server schedules PULL digests and STALE/LOST transitions and
retries failed deliveries. After the machine sleeps, its clock judges no session until a round of
the idle sweep begun after the wake has ended, answered or failed, or a round's timeout and the
slack it may start late within (240 + 30 s) of awake time have passed: the sleep's gap is nobody's silence, and the sweep first confirms each listener
that still answers, so a quiet child whose listener answers is not said lost again at every wake,
while one the round finds gone is said lost once (`apps/symphony-server/src/live/presence.rs`).
Per-recipient leases prevent concurrent writers
from sending the same queued event. A successful native send acknowledges the
inbox; failure retries three times, is recorded on the task thread, and retains
the event in the inbox. As with any external queue, a process crash after the
native send but before acknowledgement can repeat delivery; messages carry
stable board ids. No normal agent event remains a stream; the browser's live
feed remains a stream because it updates the owner's page.

The Claude wire envelope is newline-delimited
`{"type":"user","message":{"role":"user","content":"…"}}`, observed in the
installed Claude Code 2.1.308 socket example. Its body format is undocumented;
this is the compatibility assumption. On macOS the board sends it without
authentication and never reads the session's messaging token. Claude's inbound
controls can hold or refuse it; a successful socket write confirms transport,
not that Claude accepted the message. No project inbound setting is changed.
Codex delivery uses the installed `codex queue --thread UUID --message TEXT`.
For isolated runs use `tasks --database NAME ...` and
`symphony-server --scratch NAME --port N --root ROOT`; the latter keeps
its asks in `artifacts/callbacks-NAME/asks.db`.

**Agent sessions behind a task** (`task_sessions`): `just tasks launch SLUG` starts the
agent the project's `[agents] launch` in its main checkout's `symphony.toml` names (`default`,
`codex` or `claude`, with its `model` and `effort`), on the model `[agents.models]` names for
its role (`executor`, or `reviewer` under `--role reviewer`) when there is one
([Limits and models](#limits-and-models)), else a Codex agent (`--no-daemon`; model
`gpt-6.1-sol`, effort `xhigh`, workspace-write with network access), in an hc terminal of its
own, `hc run --bg --no-parent --outlive-parent --keep`: no child of the caller's terminal,
which the app would show under it rather than in a tab. `--kind` and `--model` win over the
project's; its models and effort go with its agent, so `--kind` naming the other agent takes the
board's model for that one (Codex `gpt-6.1-sol` at `xhigh`, Claude `sonnet` at its own effort).
A table that does not parse, or names another agent, refuses the launch, and so does a machine
running as many agents as `[limits] machine_agents` allows. The `launched` line
names the agent, model and effort it started, and `launch --help` ends with what a bare launch
starts where it runs (`crates/tasks/src/launch/defaults.rs`). The
terminal shows in its stream tab (`code` for an executor, `review` for a reviewer, or
`--tab` such as `3d` or `infra`) of the project's space in the Herdr app, found by name and
made when missing (the labels `herdr.space` and `herdr.tab`); the owner's tabs and other
projects' terminals are never a destination. Once hc calls the agent ready (`idle` or
`done`), its first message goes in by `hc send`, never forced, and the launch is confirmed
only by that send's receipt (`started`): an agent waiting on a person (a folder-trust
dialog), never ready, or not starting on the message fails the launch naming the terminal,
which is killed and removed. The refusal of an agent waiting on a person carries hc's word
for what it waits on and, where that word is not on the screen (hc names some dialogs only
as open), the screen's last lines, since the terminal is gone by the time anyone reads it
(`crates/tasks/src/launch/startup.rs`, `shows`). It writes its handle, role, agent session
id, worktree, branch, terminal and start time; `just tasks show` lists them under
`Sessions:`. No terminal the owner may be looking at is closed: one in the tab the Herdr
app shows, the agent it shows, or held over them (`herdr-app status`, read last, right
before the kill, after a last `hc get` that must still find it finished, running the program
cleanup judged: the same pid and foreground); when the window cannot say or the terminal
works again, it stays and cleanup names why. A shell closes only with the shell itself in
its foreground: a job it runs in front, or one it became by `exec`, is work. When a terminal is closed (`just
tasks cleanup`, or the task becoming done or cancelled: `hc kill`, then `hc rm`) the
record stays and says why and how to bring it back,
`just tasks restore SLUG [HANDLE]`, which resumes the conversation in the same
worktree, in a new terminal; through `just tasks`, only a worktree in the project
([what the command may write](../operations/BOARD_OPERATIONS.md#what-the-command-may-write)). A reviewer launch (`--role reviewer`) goes back to the
task's recorded reviewer the same way: the newest that never said `just tasks leave` with a
conversation recorded is resumed in its own worktree, agent and model, told `/review-task SLUG
HANDLE` (the `resumed` line names it), and the launch is refused while a recorded reviewer still
runs in a terminal; a new reviewer starts only where none can be resumed (no conversation
recorded, a Codex thread that never arrived, a reviewer that left), or with `--beside WHY`, which
resumes none and starts a second reviewer beside the one still there. One quiet since its task went
back to `in_progress` reads `left` by its health and is resumed all the same. A reviewer launch on
an instruction subtask is refused, naming the task it judges. A record from before hc names a
herdr pane (`w1:p…`): that terminal is gone, so cleanup records it closed with its restore line, and restore brings
the agent back in an hc terminal. `just tasks close SLUG HANDLE` closes one agent's terminal
whatever role it holds (a reviewer waiting for a new tip), records it the same way once the
terminal is killed, bound to that terminal, and retires the callback registered from it; it is
refused for the caller's own session or terminal, one the owner may be looking at, and one whose
program is not at rest (work under it as the idle sweep reads it, any test run included, or
another program in its foreground). A close and a restore of one session each hold it while they
run, so neither starts while the other runs; one whose hold lapsed and was taken over writes
nothing more, and a restore that lost it kills the terminal it started. A Codex record
takes the thread its callback registered whichever is written first; one closed with no thread
says on its thread that restore cannot resume it. See the [tasks skill](../../crates/symphony/shipped/skills/tasks/SKILL.md#launch-restore-clean-up).

An initialized JavaScript REPL can retain `kernel.js` and `trusted-worker.js` under the
ChatGPT app's bundled `node`. Their command lines do not distinguish waiting for a cell
from executing JavaScript or a browser operation, so an agent's `idle` status alone does
not make them safe to close. Coordinate recovery with the session's owner: finish or
cancel its active work, then have that original session call its native
`node_repl.js_reset` or `cua_repl.js_reset`, for the REPL it used. Reset discards its
JavaScript bindings. Wait for the reset's result and read its process tree again before
retrying `just tasks close SLUG HANDLE` and then `just tasks restore SLUG HANDLE` on
that project's board; the close still checks the foreground, remaining children and
the owner's window. If the original session cannot execute the reset, coordinate a
normal exit from that session before retrying. An accepted message queue is no proof
that a reset ran, and retained kernels are no reason to bypass a close refusal.

A consuming project chooses its agent models (`[agents.models]`), staffing policy and terminal
lifecycle. The board records those sessions and roles; it imposes no model preference.

**Optional automatic staffing:** `just tasks staff [--dry-run]` makes one pass, highest
priority first. Enable it in `tools/task_staff.json` in the consuming checkout: `enabled`
defaults to false, `max_sessions` to 8 and `min_free_memory_gib` to 4. `only` is an
optional list of task slugs whose subtrees may be staffed; an empty list allows all
orchestrated work. The config belongs to the checkout running the command (main for
the Symphony). `reap` runs staffing before its five-minute idle sweep. A shared
`symphony/tasks/staff.lock` keeps concurrent staffing passes from dispatching twice. A review
is given on the task itself, never as a subtask: its reviewer takes the reviewer's role there,
its launch is recorded there, and the pass's line on the thread names the tip. A review whose
task has a recorded reviewer to resume goes to it, by the launch's resume (`would resume HANDLE`
in a dry run), never to another idle reviewer, and fails as that launch would while the reviewer
still runs, in a dry run too. Otherwise it goes to an idle reviewer only when
the board would take its verdict, by the rule the verdict holds
(`Held::unfresh`, [the default landing](#the-default-landing)); a reused reviewer whose launch
was not recorded lets the role go, so the task waits on no reviewer that has no job, and a
launch that fails takes back what it took, as every launch does. Through `just tasks` a pass
launches only where a launch there may ([what the command may write](../operations/BOARD_OPERATIONS.md#what-the-command-may-write)): from a linked
worktree, one continuing a branch checked out in the main checkout (its
`.claude/worktrees/…` among them) or anywhere else outside that worktree and the worktree base
is refused.

`reap` also recovers missing agents with addressed work left unanswered for thirty
minutes, even when staffing is disabled. It resumes a recorded conversation once
per handle/post, after the hc terminal inventory confirms its absence, and posts the
restore on the task thread. With no recorded conversation it only flags the work;
it never starts a fresh conversation on its own. A failed automatic attempt stays
flagged for manual action. `reap --dry-run` starts no recovery.

Only ready tasks with dependencies done and no executor or reservation qualify;
review tasks with no reviewer get one, on the task itself. A land-request task's opening
`Review` line records the branch under review;
an explicit `Branch:` or `Worktree ..., branch ...` line does too. Otherwise reviews
use the executor's recorded branch, falling back to the task slug.
Dry runs resolve that branch and show its tip too. A task whose target or dispatch
fails prints its failure and leaves later tasks eligible; staffing errors never
prevent the idle sweep. Work already staffed through a child is left alone. A live orchestrator on
the task or an ancestor is required. Owner-only work is excluded by `owner-only`,
`owner only`, `owner tier`, `sign-off`, `signoff` or `bless` in its slug/title, or a body
line beginning `Owner-only:`, `Owner only:`, `Tier: owner` or `Review tier: owner`.
Use these explicit markers for owner decisions.

Before launching, the pass prompts an idle worker/reviewer holding no unfinished
work in a suitable open terminal: an executor already on the task's own branch, or a
reviewer or an instruction's worker in a detached checkout. A checkout with a live or stale
executor is never reused. Otherwise it launches what a bare `just tasks launch` of the role
would (the project's `[agents] launch` on the role's `[agents.models]` model, else Codex) in a
separate worktree: the task's own branch, or detached main for an instruction or review. Live
agent terminals and live board agent sessions without a terminal count toward the cap, the
lower of `max_sessions` and `[limits] machine_agents`; reuse adds no session. Memory below the
floor defers work. Every proposal, dispatch, deferral and failure prints one line. Only a successful ping or launch is posted on
the task; deferrals and dry runs post nothing. A dry run previews even while
disabled and creates no task, session or terminal.

**Idle agents are closed by the board, not by a reminder.** A task
that becomes done or cancelled closes its sessions at once, but for the executors of a task an
approve left done and not landed: its land may still ask its executor for a stamp of its at
the approved tip, a conflict settled or a red check mended, so their terminals stay, each a line
of the verdict's output, and close when a land run here closes the task, or at the sweep once
idle with nothing asked of them, as after a merge the host made
(`crates/tasks/src/launch/cleanup.rs`, `approved`). The rest is a sweep the
Symphony's process (`just symphony`) runs every five minutes for each project it serves that
has a board, each on a schedule of its own in its own checkout, started as the project is
registered and stopped as it is removed, so one project's failing or hung sweep holds up no
other's; each writes its own `reap.json` and its own board's health, which `just tasks status`
reads. A lock in `symphony/tasks/`, beside the board's server, keeps two sweeps (a second
app's, or `just tasks reap` by hand) from running at once, and nothing goes into any monitor.
A server of its own (`just symphony --port N`) runs no sweep, since a preview's board may be a
copy that names another board's terminals, and says so as it starts (a preview in a linked
worktree whose file does not ask says nothing): a project no other server serves, as one on a
source build with no release on the machine, sets `[board] own_sweep = true` in its project
file, and a server of its own in its main checkout then sweeps for it as the one server would,
unless its environment names another board (`TASKS_SERVER`, `TASKS_DATABASE`;
`sweeps_of_its_own` in `apps/symphony-server/src/front.rs`). Without either, idle terminals stay
until `just tasks clean`.
It closes (`hc kill`,
then `hc rm`) one of this board's terminals (label `symphony.board`) whose named agent was
idle (or stuck on the folder-trust prompt) at two sweeps at least five minutes apart,
however many apps sweep, when neither its handle nor a subagent session under it holds
an executor role on an in-progress, blocked or review task, a reviewer role on a task in
review, an unanswered addressed post, or a reviewer awaiting re-review after changes
returned its task to in-progress. That last keep includes landing changes messages,
on the branch task or any numbered review task. A pending post keeps the terminal
immediately, before the thirty-minute recovery threshold. A post, status move or verdict
by its addressed handle answers earlier work; reading its inbox does not.
Nothing runs under it but MCP helpers (Codex's configured `node_repl` and the ChatGPT
app's `cua-repl.mjs` under its own `node` among them, each by its exact path, and
Codex's `codex-code-mode-host` in a standalone release's `bin` folder), caffeinate and
board commands: anything else is work, a `codex exec` job included. The roles above
keep it for an hour at most, since only a session doing work, or about to within the hour, stays
open: idle at its
prompt that long, from its last tool call or the last change of its role or task
status, it closes saying what brings it back; only an unanswered addressed post keeps
it longer. A terminal whose program ended is removed at the first sweep, the open
records naming it closed first with their restore line.
Status, roles and the window are read again right before the close, and the terminal and the window once
more after the close is recorded, right before the kill: work resumed by then (its agent
working, or work under it) or the window showing it reopens the record. Never an unnamed terminal or another board's (it may be the owner's), an
orchestrator or a chat, the caller's (`HC_TERMINAL_ID` or its session), or one on screen in the Herdr app, which
starts its idle wait again (as does a window that cannot say what it shows).
Each close is one line on the handle's task thread, with its restore line, written
before the terminal is closed. `just tasks reap --dry-run` names what the next sweep would close
and why each terminal stays; a sweep tells its log why it keeps an idle one, once per
reason. `just tasks clean` runs cleanup and one sweep at once, never closing the caller's.
Code: `crates/tasks/src/launch/reap.rs`.

**Finished worktrees go by themselves**, without asking, so they hold no disk once their tasks
are complete. The worktree sweep (`just tasks sweep`, the rules in
the consuming project's worktree policy) starts, detached, whenever work
finishes: a task going to done or cancelled (its closed sessions' worktrees then go without
the 2 h wait), `just tasks leave`, `just tasks reap`, and at most once an hour `just tasks
inbox`, by a stamp. The command that finished the work never waits on it and says nothing of
it; each sweep runs from the main checkout, by the main checkout's build of `tasks` (never
the caller's, whose sweep may not have landed; with none built it says so and sweeps nothing),
and appends what it did to `symphony/tasks/sweep.log` in the git common dir. Status writes through
`tasks api` (for example, a release run or reap caller writing status) do not start this CLI
trigger; the next hourly sweep covers them. The Symphony's explicit finish action starts its own
sweep.
A worktree goes, locked or not, when its HEAD is in main (detached or on a branch), nothing in
it did not land, no live work holds it and its lock holds nobody; the branch it had checked out
goes with it once merged, if no other worktree holds it and it still points where it did. Three
things keep that branch, each said as `kept branch NAME: WHY`: a land's record of its merge
naming it, as `just land SLUG` run again finishes an interrupted land from that record and the
branch, in a worktree of its own; a delivery of it by pull request not yet asked to merge, whose
land run again reads the branch's tip (from the merge's request on, a land run again goes on from
its journal alone, and the branch goes); and its task done by an approve no landing has closed, as
what keeps such a task from going back to review once it is on main is its branch's tip.
A head [a pull request squashed](#a-land-delivered-by-a-pull-request) is in no history of main,
and is in main by its receipt alone: the HEAD, or the branch's tip, is exactly the head a receipt
kept under its own task's name records; `refs/symphony/delivered/SLUG` still names that head, so
no commit is lost with the branch; the squash the receipt names is the commit it recorded, of
that one parent and that tree, the tree git makes of merging the head into that parent; and that
squash is on `origin/main` as last fetched and in main here. The sweep reads all of it from git
and the kept file, fetches nothing and asks no host. Anything less is a head not in main, kept as
one: a branch that moved on after its delivery, a receipt that does not read, is another task's
or names another commit, a retained head gone or moved, a squash the remote was never seen to
hold or the main checkout has not followed. Such a worktree and branch are said as `squashed
into main as COMMIT by pull request #N` where a merged one says `merged into main`, and every
rule below holds for them as for any other. The sweep deletes no retained head, no receipt and
no branch of the remote.
Nothing did not land when it has no tracked edit, no untracked file but what a file manager
writes into a folder a person opens (`.DS_Store`, `Thumbs.db`, `desktop.ini`, which go with the
worktree whether or not its ignore file names them) and no ignored file but
build output and caches (the board's own list, and the ignored paths the project names in
`[board] sweep_disposable` of the main checkout's `symphony.toml`, where its stack keeps its
dependencies and builds elsewhere, such as `node_modules/`), and holds no owner verdict no commit
has (an owner's dated line under a
card, in a document of the project's entrypoint folders, that its HEAD does not hold; a worktree
whose documents git cannot read for one is kept, saying so). No live work holds it when no
live or stale session is bound to it (a subagent's session is bound to its own `agent-ID`
worktree as well), no live task is tied to it (its branch names the task's slug, or the slug
with its last `/` as `-`), no process works inside it and nothing touched it within the 2 h
window. Its lock holds nobody when it has none, or names a process that is gone: Claude Code's
(`claude agent agent-X (pid N start …)`) once N is not running or runs with another start, a
land's (`symphony land: SLUG (pid N, started S)`) once N is not the process that started at S
(or, with no start, once N is not running). Claude Code's lock names the
session that started the agent, its orchestrator, which outlives every agent it started, so a
lock whose process runs says nothing of the agent; the board says the agent finished when a
session is bound to the worktree (by its worktree, or as the agent the worktree's name and the
lock give), none of those sessions is live or stale, and every task tied to the worktree, at
least one, is done or cancelled. A live agent's worktree is never touched: its session, live or
stale, keeps it, and so does a lock the board cannot answer for (an agent it knows nothing of, a
land that runs, the release run's checkout, any other reason, a start it cannot read). Every
worktree git lists is the sweep's, the `../symphony-wt/*` trees the land and the release run make
included: a land's goes by the same rule once its process is gone; the release run's checkout,
which it resets and builds on each day, stays. A worktree with tracked edits or commits not
in main is never removed, whatever its task's stage: an orchestrator decides. What git ignores
in a finished worktree's artifacts folder (the project's `[project] artifacts`, `artifacts/`
when it names none: the pictures its thread names, a test's leftovers), less its build output, is
moved into the main checkout's `FOLDER/swept/NAME/` right before the worktree goes, whatever came
there up to then, and back if git refuses the removal; while a task tied to it is open, it keeps
the worktree. A move the sweep cannot make (the folder on another disk, a name there taken, even a
moment before: the rename itself refuses it) keeps the worktree and says why, and so does an entry
there the sweep cannot read, which would otherwise go with the worktree: evidence is moved, never
copied, replaced or removed. Nothing empties
`FOLDER/swept/` (below). The pass a land
runs for its task visits, without the window, the worktrees the task's closed launches ran in,
those its executors' and reviewers' sessions are bound to once none of those is live or stale (an
in-process agent has no launch record), and those on the task's branch (an agent that left has
released its roles). Before each removal the sweep refreshes
sessions, launches, locks and activity and searches that tree for process working directories;
a lock it lifts goes back if the removal is refused. A detached agent's merged or squashed
`worktree-agent-ID` branch stays while its live or stale session still binds its worktree.
In-process agents work only in their own `agent-ID` worktree; a different directory they create
is not bound by their agent id. Full sessions register from the worktree they use. Each removal
says what the worktree held on disk, and the summary what the sweep freed; a sweep started
detached (a land's, work finishing's) writes them to `symphony/tasks/sweep.log` under a line saying when
and why.
A board `TASKS_SERVER` names (a test's) starts no sweep unless
`TASKS_SWEEP=on`; `TASKS_SWEEP=off` starts none anywhere. Code: `crates/tasks/src/sweep.rs`.

**What the sweep sets aside stays, its build output never, and past a cap the sweep asks for a
check by hand.** A finished worktree's pictures and other artifacts are kept in the main checkout
and its build output is not; past a size cap, agents are asked to check by hand and remove what
nothing uses any more ([its card](#sweep-artifacts-set-aside)). What is set aside
stays in the main checkout's artifacts folder, which
git ignores, and nothing commits it; attaching it to the repository without a commit on main is a
design of its own. Build output in a worktree's artifacts folder (a folder tagged as a cache by a
`CACHEDIR.TAG` that begins with the convention's signature, `Signature:
8a477f597d28d172789f06886806bc55`, as Cargo writes it, a file of that name without it tagging
nothing; or a file of compiled code, known by its first bytes: ELF, Mach-O, wasm, an archive) is
never set aside and goes with the worktree. Nothing removes what
was set aside, ever: past its cap, `[board] swept_cap_gb` in `symphony.toml`, in gigabytes (5 when
unwritten; a value that is no number over 0 is said and 5 taken), the
sweep still sets aside and refuses nothing. Each sweep that may set aside measures `FOLDER/swept/`
and keeps the measure in `symphony/tasks/swept.json` in the git common dir, beside the tasks tied to each
folder's worktree when it was set aside. Past the cap, the sweep's output (the log's, for the
sweep a land starts) and a dry run's say so in one line, `set aside in PATH: SIZE, over its CAP GB
cap ([board] swept_cap_gb): …`,
and `tasks status` warns of it, once, from that record, standing while the last measure is past
the cap: `the artifacts set aside in PATH hold SIZE, over their CAP GB cap ([board] swept_cap_gb),
as the sweep measured them AGO ago, said until a sweep measures them again (…): …`, so a check by
hand clears it once a sweep measures again, a dry run recording nothing. Both ask for the check by
hand and name the five largest
folders there, each with the tasks tied to it and their stage and what still references it:
`in use: WHERE` for a task tied to it that is not done or cancelled (`task SLUG is open`), a post
on an open task's thread naming a path under it (`the thread of SLUG`) and a document of `docs/` on
main naming one (`docs/PATH`, cards included); `no reference found` for the rest, which alone are
suggested, and which the sweep does not remove either. A path is named under a folder there as
`FOLDER/swept/NAME/…`, or as the worktree's own artifacts folder held it, `FOLDER/…`, when the
folder holds that path now. A source that cannot be read is said, and then none is suggested. The
references are looked up once for each measure past the cap: one read of the open tasks' threads
and one `git grep` of `docs/` on main. Under the cap nothing is looked up, the sweep says nothing of
the folder and a dry run says `set aside in PATH: SIZE, under its CAP GB cap ([board]
swept_cap_gb), …`. Code: `crates/tasks/src/sweep/kept.rs`.

**Build output goes once the work is past its executor, the worktree staying.** A worktree the
sweep keeps (in review, its work not landed, holding files that did not land) loses its build
folders (`.target/`, `symphony/.target/`, each gigabytes, and what git ignores in `build/` and
`symphony/build/`) once every task tied to it is in review, done or cancelled, merged or not (an
in-process agent's worktree is tied to its task by its session), and nothing ran there for an
hour: no agent bound to it beat or made a tool call, the board's recorder recorded no build or
test there (a reviewer may build in its executor's worktree), nothing touched its git files or
its top folder, and no process works inside it. The worktree a land left when its process
ended loses it after the same hour, and the release run's checkout once nothing touched it for 7
days, as long as the release run itself keeps one. Any other worktree tied to no task keeps its
build: the owner's, or one made by hand. A task in concept, design, ready, in progress or
blocked keeps it, as do a land that runs and a lock the sweep does not know. Right before it
removes, the sweep reads the worktree listing, the board and the recorder's records again and
decides again, and asks again whether a process works inside; then it takes each Cargo build
lock in the folder without waiting (a build holding one keeps the folder), renames the folder
aside under them and removes it; of `build/`, what git tracks or holds untracked stays. A folder
reached through a link (`symphony` or `.target` linking elsewhere) is never touched. Each clear
says `cleared build output PATH (why; SIZE)`. Code: `crates/tasks/src/sweep/build_output.rs`.

**`just tasks sweep --dry-run` says what a sweep would do, and does none of it.** One line per
worktree git lists, `worktree PATH: SIZE; TASKS; MERGED; VERDICT`: what it holds on disk (the main
checkout's line has no size), the tasks tied to it with their status (`task SLUG (STATUS)`, or `no
task`), `merged into main`, `squashed into main as COMMIT by pull request #N` or how many commits
it has that main has not, and what the sweep would
do with it and why (`would remove: …`, `kept: …`, or `kept for an orchestrator to decide: …` for
tracked edits, files that did not land and commits not in main). Then `set aside in PATH: SIZE`
says what the main checkout's `FOLDER/swept/` holds against its cap (above), and its summary ends
with what a sweep would free of what the worktrees it measured hold, which a sweep on the same
state frees.

**A linked worktree's first build starts from the main checkout's.** `tools/cargo.sh`, which
every build here goes through, makes a worktree's missing build folder as a copy-on-write clone
of the main checkout's `.target/debug` (`tools/seed-target.sh`: clonefile on APFS, a reflink on
Linux; nothing on a volume that cannot clone), taken under Cargo's own lock on that folder and
without the workspace's own packages, so the worktree compiles those from its own sources and
reuses every dependency. While the clone holds that lock, a build in the main checkout and every
board command (each builds the main checkout's launcher) waits: on macOS for one clonefile call
of the whole folder (`tools/seed-clone.rs`, compiled by rustc on the spot), else for `cp -c`
cloning each file. A clone takes disk only as a build replaces what it holds; `du` counts each
clone in full, `df` says what the disk holds. It is staged as `.target.seed.PID`, which a signal
removes with the clone and a later seed or sweep removes once its process is gone.
`SYMPHONY_SEED_TARGET=off` turns it off. Worktrees never share one build folder: Cargo names the same
package of two worktrees the same, so each would run the other's build of it. What it costs and
saves, measured: [the analysis](../engineering/analysis/worktree-build-folders.md).

**Health** is derived from when the session was last seen. Every command
counts as a sign of life, and so does every tool call of a subagent (below):

| health | meaning |
|---|---|
| live | seen in the last 90 s, inside one long tool call that could still be running, or waiting on a recorded build or test of its own that still runs |
| stale | quiet, or its monitor has stopped: its work is still held, nobody may take it |
| lost | gone for 10 minutes holding claimable work: that work can be claimed, on the record |
| left | it said `just tasks leave` (its roles were released, the open work it orchestrated given to another orchestrator, or waiting for one while none stays), or it has been gone 10 minutes holding nothing anyone could claim |

**Claimable work** is what a session executes in `concept`, `design`, `ready`
or `in_progress`, and what it reviews that is still in `review`. A session
whose part is finished (its task done, or waiting in review for someone else)
ends quietly as `left`; if its task comes back with `changes`, it is `lost`
again, and that task can be claimed. `just tasks who`, the Symphony's
**Agents** and the attach monitor read the same health.

Callback registration does not expire. The legacy attach stream remains only
for sessions with no native channel. Tool calls are still the heartbeat.

A **subagent** has no monitor; its own tool calls are its heartbeat. The
project's `PreToolUse` (Bash) and `PostToolUse` hooks
(`.target/bin/symphony hook tasks-beat`, which runs the board binary's `tasks beat`
directly, not through `just`, so unmarked) see the Claude Code agent id
on each of its tool calls. A `just tasks …` command the subagent runs binds it
to the handle that command acts as, and so does one naming a justfile or a
folder before `tasks` (`--justfile PATH`, `-f PATH`, `--working-directory DIR`
or `-d DIR`, as a worktree's `just --justfile MAIN/justfile tasks …`): its own
`--as <handle>` flag, else a leading `TASKS_SESSION=<handle>` on that same
command. The flag is the form to write: the allow rules pass no env prefix, and
the prefix binds nothing the flag does not. A handle anywhere else
in a command (a grep pattern, text inside a quoted argument, another recipe
such as `just land` run under someone's `TASKS_SESSION`) binds nothing. From
then on each of its tool calls beats for that handle, until a later board
command names another. A full session with no agent id (one started in an hc terminal, or a
`claude -p` run) is bound and beats the same way, keyed by its session id, but keeps the first handle
it names: naming another (registering or relaying for a child) moves nothing. A launched
Claude session's beat also registers the channel its agent did not: before a board command of
the handle bound to it, where an open launch of that handle records the session's conversation,
the hook registers the session's own inbox socket (`CLAUDE_CODE_MESSAGING_SOCKET`, which Claude
Code gives a session's hooks) as `just tasks callback --as HANDLE` would have, so an agent that
skips that command still hears its messages. It does so once for each socket: a channel the
session turns off stays off, and a restored conversation, a new process on a new socket, is
connected again. A session no launch records, a subagent, a registration holding the chat seat and
a Codex thread are left as they are (`crates/tasks/src/cli/beat.rs`, `connect`). A handle
with a monitor of its own (an orchestrator) is never bound: its monitor beats for it,
and a session that names such a handle is barred from binding at all, so the calls of
an orchestrator never keep a child it registered alive. The hook looks at the board
only for a board command or a key already bound. A foreground Bash call
reads live for as long as its timeout allows (at most 10 minutes), so a long
`verify-full` does not look like silence; of calls running side by side, the
one that may run longest keeps the mark, and only its own end clears it. A
build or test that outlives its call (one started in the background, or moved
there when its call's timeout ran out) keeps its session live by its recorder:
while its command runs, the recorder of a run whose record names the session
says it was seen, every 20 s and once more as the run ends, so the session
is neither stale nor lost nor orphaning its task while it waits, and ages from
the run's end as from any last sign of life
([its session is there](MACHINE_WORK.md#recording-builds-and-tests)). Each
tool call of a bound subagent also records, on its binding (never its session
row, which keeps where its orchestrator ran `register`), the checkout that call
ran in: the top folder above the hook's `cwd` and the branch checked out there,
read from the `.git` files, and, when that checkout changed, the time it came
there; a call with no `cwd`, or one outside any checkout, moves neither (the
last checkout stands: such a call says nothing of where it works). A call there
that reads a time before any other there did (the clock went back) keeps that
earliest time too, never moving when it came. On Linux, whose kernel dates a
process's start on the clock since boot, the hook also reads that clock (its
boot's id and `/proc/uptime`): the binding keeps the reading of the call that
came to the checkout and of its latest call anywhere, a later boot's replacing
an earlier one's, and none when a call read none. Setting the wall clock moves
neither those readings nor a start dated on that clock. A
subagent waiting on a background command of its own (a `cargo test`, a build)
makes no calls; while a live process it started works in that recorded linked
worktree it reads `running_test` in the agents API, and is neither stale, lost
nor orphaning its task ([The agents API](../engineering/BOARD_API.md#the-agents-api)).
A subagent that stops makes no more calls, leaves nothing working there, and
ages to stale and lost as before. Each beat also hands the board the tokens the
session's own transcript gained since its last beat, in the beat's one write
([Where the tokens go](#where-the-tokens-go)).

Orchestrators can share the board, each holding its own slug prefixes. A decision
one side makes that the other must act on becomes a short board task for that side,
and a side that approves documents commits them regularly, because worktree agents
see only what is committed.

### Going to a session

Every agent on the agents page, and every role chip on a task's page, carries a **Go to session**
control when the board knows a way into that agent's session. The server works out each agent's
way in at each tick of its clock (`symphony_api::resolve_sessions`; the Symphony's wire carries it
as the agent's `go`, `{"handle", "url", "pane"}`, and a task page's `sessions` for its role
holders), and what the control does depends on the session and on where the page was asked:

- **A web session.** A session that gave a web address (a cloud agent, a Claude Code session with a
  remote link) is an ordinary link to it, opened in a new tab, from any device. The session gives
  it with `just tasks register --session-url URL`, or later `just tasks callback --session-url URL`
  (alone, or beside a channel; an empty one clears it). It is `https://` or `http://` with a host,
  with no space or control character, at most 2048 bytes (`crates/tasks/src/session_url.rs`),
  checked when it is set and again when the board reads it, so a row written past the check (a
  copied dump, a hand write) is left out; a page links only what `symphony_api::web_address`
  passes, so no other scheme ever becomes a link. `callback --session-url` keeps the address only
  once its registration is accepted. The board keeps it in `callback_state` beside the session's
  row, never on it, moving no board marker, and `leave` deletes it. It is the agents API's
  `session_url`.
- **A pane, from the server's machine.** Where the project addresses its panes (`[agents] panes =
  "hc"`) and the agent has one (`pane`), a page asked from this machine (loopback, or one of its
  own tailnet addresses as `tailscale status` gives them) shows a button. The server has the Herdr
  app show that terminal, `herdr-app do agent.focus --terminal T` (`pane.focus --terminal T` for
  one that runs no agent), and brings the app's window forward with `open -a` on the app bundle
  its program lies in (`crates/tasks/src/launch/go.rs`, `POST /api/agent-focus`); the page says
  what was done, or why not. Herdr has no URL scheme a page could link to (neither `herdr-app`'s
  help nor its commands name one), so the server asking the app on its own machine is the only
  way, and the button is offered only to a page there; the route refuses any other.
- **A pane, from another machine.** A phone or another computer on the tailnet cannot focus
  anything on the Mac: its button shows the pane's visible screen, read only (`hc read T --lines
  0`, `GET /api/agent-screen?handle=H`), and reads it again on the next click. With both a web
  address and a pane, this machine focuses the pane and any other follows the web address.
- **A subagent.** An in-process agent (`in_process`: its binding says it runs inside another
  session) has no pane of its own, so its control goes to its parent's session (the first parent
  in turn that has a way in, at most eight up) and says "in PARENT's session". A launched agent
  with a parent is no subagent: its parent's pane is not its session.
- **Nothing known.** No web address, no pane the project addresses, no parent with either: no
  control.

Decided with it:

- **One call shows a pane.** The terminal's address alone is enough: the app finds the space, tab
  and pane that hold it, so no workspace or tab is named in turn.
- **What names a pane across a Herdr restart.** The control names the agent by its handle, never a
  terminal. Each tick finds the agent's pane anew, by its task session's terminal, the
  conversation its program runs or its handle as the terminal's name or label ("Which terminal" in
  [the agents API](../engineering/BOARD_API.md#the-agents-api)), so once Herdr restarts and its terminals take new addresses, the next tick finds the
  pane again; a terminal's address is never kept as the pane's name.
- **What the screen may show.** A pane's screen shows whatever its agent printed, a secret among
  it, and so does a session's web address to whoever can open it. Both reach only a request the
  gate admits, this machine or a tailnet peer `tailscale whois` confirms (the gate refuses every
  other address before any route), and the screen is read when asked, never kept, and answered
  `no-store`.
- **Nothing is typed.** Neither a focus nor a screen sends a key, a message or a paste to a pane:
  the only calls are `herdr-app do agent.focus` or `pane.focus`, `open -a` and `hc read`.
- **One deadline, one at a time.** A focus (up to three calls: `agent.focus`, `pane.focus` and
  `open -a`) and a screen read each have 10 s for all their calls together
  (`symphony_board::launch::go::DEADLINE`), under the 15 s a page waits for its answer: a call
  still running when that is spent is killed and reaped, and one not yet started is not run. The
  kill reaches that call's own process, which stays in its caller's process group, so a signal to
  the caller's group (the server ending a `tasks reap`, Ctrl-C in a terminal) ends it too. hc and
  `herdr-app` are single binaries. A process one started that still held its output would keep
  the call's reader, and the killed process unreaped, until it ended, though the answer has
  already been given. While
  a focus, or a screen read, of a session is under way, another of the same kind for that session
  is refused (409), so pressing the button again starts no second chain of calls.

## The pipeline alarm

Work piling up in review is said, not left for someone to notice
(`crates/tasks/src/alarm.rs`). Review holding more than twice the work
in progress for 4 hours raises the alarm; it clears once review has held no
more than twice in progress for an hour, so a short dip does not lower it and
a short rise does not raise it. Exactly twice is not over; review with nothing
in progress is.

- **Thresholds.** The project's, in the `[board]` table of `symphony.toml`, read
  from the main checkout as the cap is and printed by `just tasks limits`
  ([Limits and models](#limits-and-models)):
  `review_alarm_ratio` (2), `review_alarm_hours` (4) and
  `review_alarm_clear_hours` (1), whole numbers of at least 1. A key not
  written takes its default; a value that does not parse, or one below 1, is
  said on stderr with its line (`⚠ …; the pipeline alarm takes its default
  thresholds`) and the defaults apply.
- **What counts.** Work only: a review mirror counts with the task it reviews
  and an instruction subtask with the task it serves, as for
  [Slots](#slots), neither on its own, as the tree stood at each moment: a
  mirror counts on its own until the task it reviews is made, and an
  instruction subtask from when it leaves its parent (a move's line names the
  parent it left, `HANDLE put it under PARENT (was PARENT)`), so a task made or
  moved changes the counts from then on, never the hours before. A task in
  review whose branch's tip a reviewer asked changes of is rework (below),
  counted in progress.
- **From the record.** The board keeps no history of counts. Every change of a
  task's status writes a line on its thread (a claim, a move, a verdict, a
  landing, a block by a cancelled dependency and its end), so each reader walks
  the last day's lines (or twice the raise and clear spans, if longer) back
  from each task's status now and replays the alarm over the counts. What held
  when that window opened is read, never assumed: a task's status then, where
  its oldest line in the window is a claim or a landing, and the record of
  what its landing waits on then, for one in review some time in the window,
  come from its newest such lines before the window, read back past any field
  value whose text only reads like one; and when the window holds neither a
  raise span over nor a clear span under, so the alarm's state at its start
  decides, a window twice as long is replayed, until one does or it opens
  before the board's first task, when the alarm was lowered. Every reader at
  one moment finds the same alarm, and
  nothing writes it. The window's lines are read through the message time
  index and those before it from each task's own thread; no table is read
  whole.
- **What it says.** While it stands, every orchestrator's PULL header ends
  `review_alarm=REVIEW/IN_PROGRESS`, the whole board's whoever the digest is
  for, and the lines after the header say the counts and since when, then the
  5 tasks longest in review, longest first, each with when it went in and
  what its landing waits on:

  ```text
  PULL 14:05 executing=4 … idle=0 review_alarm=10/4
    review_alarm: review 10, in progress 4, more than 2× since 09:40
    review_alarm p1 tools/x — X   (in review since 11-07 21:12; missing certificate, review)
  ```

  In the hour that clears it, the line says `at most 2× since HH:MM; it clears
  after 1 h of that`. Its raising is an entry of its own, so the monitor
  delivers it once, as it delivers any new entry.
- **What a landing waits on.** The board holds no stamps; the tool that lands
  a task records what its landing lacks in the board field
  `fields._board.waits` (`Store::record_waits`; `set_field` is a plugin's): the tip it judged, `: `, then
  each name, `, ` between them (`0123abc…: certificate, review`), none after
  the colon when it lacks nothing. The alarm shows the names as written
  (`missing certificate, review`), `missing nothing` for none, and `its tip is
  newer than its landing's last look` when the branch has moved past the tip
  the record judged. A task with no record shows no note. Each record names
  the assessment it states, kept with when it was written
  (`fields._board.assessed`): the same assessment retried writes nothing,
  and a fresh one writes its record again however like the last it reads.
  Stamp and approval reassessment, refused landing attempts and completed tip
  verification submit these assessments through a detached, owned delivery.
  It checks the current branch and task state, evaluates stamps and merge
  conflicts, and records `changes` only for an effective reviewer veto. A
  superseded interrupted verification supplies no completed assessment.
  Local submissions and receipts live in the folder the project's `[board]
  assessments` names (a `.git/` prefix names the common directory; none when
  unset, and an assessment is then refused); the delivery child is stopped and
  reaped after 30 seconds or checkout removal. A delayed board, an older API, an unknown method, an
  incompatible payload or an unconfirmed reply leaves pending evidence: the API's version proves no
  method, only a reply echoing the publication's witness and canonical receipt exactly confirms one,
  and a retry's receipt confirms its occurrence, never that it still stands (`current: false` once
  a newer assessment does). `just land-stamps BRANCH` retries the latest
  submission, preserving a committed occurrence's identity, time and line.
  Pending evidence alone does not confirm a board write.
- **Rework.** The name `changes` in a record says a reviewer asked changes of
  that tip. While the branch's tip is still the one judged and the branch has
  not moved past it since the record, the task is rework: PULL counts it in progress (its live executor as executing, and one
  nobody executes as `in_progress_orphaned`, noted `in review; changes asked
  of its tip`), not as `review_unreviewed`, and the alarm counts it in progress
  from the record to the next; a change of status ends nothing, since the
  record judged a tip, which a move into progress and back keeps. The first
  commit the branch moved to from the tip judged, after the record, ends it,
  from the moment that commit was made or the branch moved to it, however late
  its record comes and whatever follows (more commits, the task taken back
  into progress): the task waits on review again. The moves come from the
  repository the reader runs in, as the branch's reference log tells them: the
  next commit made on the tip judged, or what replaced it, amended or rebased
  away. Without that log, the first commit after the tip judged on the
  branch's line of first parents ends it, never one that reaches it through a
  merge's other parent; a reader that cannot see a task's branch takes each
  record as judging the tip of its time, and one that cannot place the tip
  judged takes the tip now as the first after it. The board keeps the first
  end it learns of each record (`fields._board.fixed`), and nothing later
  moves it: not another commit, an amend or a rebase, nor what the repository
  tells after. A new record of changes on a tip the branch came back to, from
  a fresh assessment, is a request of its own, whatever its text: it waits on
  the branch's next move from that tip. The record answered, retried, is no
  new request: PULL and the alarm count the task in review alike.
- **No review waited on.** A record that judged the branch's tip now and names
  neither `review` nor `changes` says the project's own review gate, as its
  landing keeps it, is met there or not needed, and that replaces the
  Symphony's: PULL does not list the task as `review_unreviewed`, though no
  reviewer holds it. A record of another tip, or none, says nothing of the tip
  now, and a review with no reviewer is listed. A task whose tip the reader
  cannot tell (no repository, or a branch not named for its slug or gone) is
  listed too, its record unread there, unlike rework: the row errs toward
  listing.

## Where the time goes

The board keeps no table of status changes. Where a task's time went is read
back from what it already stores: the task's `created`, `updated` and `status`,
and the `system` and `verdict` lines its changes left on its thread
(`crates/tasks/src/stages.rs`). The store builds its move and claim lines
with that module's functions and the release, block and unblock lines from its
constants; the verdict words and the `(was STATUS)` ending are written out on
both sides.

| line | what it changes |
|---|---|
| `HANDLE moved it FROM → TO` | the move it names, the owner's too |
| `HANDLE claimed it[, handed over from …]` | ready work is in progress; a claim of work in another status changes none |
| `HANDLE released executor (released)`, `HANDLE released every role (left)` | work in progress whose last executor let go is ready again |
| `blocked: cancelled dependencies: … (was STATUS)` | blocked |
| `unblocked: no cancelled dependency left; back to STATUS` | the status it names |
| a `changes` verdict | review back to in progress: a bounce |
| any other verdict (`approve`, a subtask's `done`, a landing) | done |

The lines are replayed in the order of their message ids, the order they were
written in: a line's time is its writer's clock, so one dated before the line
written ahead of it is taken at that line's time. A line naming the status
the task already holds changes nothing, and nothing after the line that closed
the task is read. The status a task was made in is the one the first line that
names a status says it left; with no such line it is the task's own status
where a task can be made in it, else `ready`. Where the replay ends in another
status than the task holds (a change the thread does not record), that change
is dated at the task's `updated`. An open task is counted up to now; a done or
cancelled one stops at the line that closed it, and the closing status itself
counts no time. The statuses' times add up to the time from creation to close
(or to now).

`just tasks show SLUG` prints it in one line under the task's roles and links:

```
  Stages: ready 0.4 h · in_progress 1.3 h (2 visits) · review 0.8 h · 1 bounce; first claimed after 0.4 h
```

Each status is in the order the task first held it, with its visits when more
than one; a bounce is review sending the task back to in progress, by a
`changes` verdict or a move. After the `;` comes the wait from the task's
creation to its first claim (`never claimed in 20.0 h` when nobody claimed it:
to its close, else to now). That wait is a part of the statuses before it,
usually the time in `ready`, and is never added to them.
`just tasks show SLUG --json` carries the same as `stages`, every duration in
seconds and every time in epoch seconds:

```json
"stages": {
  "statuses": {
    "ready": {"first_entered": 1800000000.0, "seconds": 1440.0, "visits": 1},
    "in_progress": {"first_entered": 1800001440.0, "seconds": 4680.0, "visits": 2}
  },
  "segments": [
    {"status": "ready", "from": 1800000000.0, "to": 1800001440.0, "by": "glacial-merlin"},
    {"status": "in_progress", "from": 1800001440.0, "to": 1800003600.0, "by": "quiet-wren"},
    {"status": "review", "from": 1800003600.0, "to": 1800006000.0, "by": "quiet-wren"},
    {"status": "in_progress", "from": 1800006000.0, "to": null, "by": "lucid-heron"}
  ],
  "unclaimed_seconds": 1440.0,
  "first_claimed": 1800001440.0,
  "bounces": 1,
  "current": {"status": "in_progress", "since": 1800006000.0},
  "closed": null
}
```

`segments` are the replay's stretches in the order the task held them, each
with who moved it there (`null` where its line names nobody, as a block's or
an unblock's): they follow one another from the task's creation, the one it
holds has no `to`, and their times add up to the statuses'. No table is added
for them; they are the same read.

`just tasks list --json` carries, on each task it lists, `in_status_since`:
when the task took the status it holds, the replay's `current.since` (its close
for a done or cancelled task; the task's `updated` where its thread does not
record the change). `just tasks search --json` carries it too, its rows being
list's, and `tasks api`'s `in_status_since` (api version 14) answers it for the
tasks its `slugs` name, by slug, refusing a slug no task has. Each reads the
tasks it names and their status and verdict lines in one request however many
there are (`Store::in_status_since`; `tests/cli/stats.rs` counts them over 300
tasks); `list` and `search` read nothing more without `--json`.

`just tasks stats [--days N] [--json]` (14 days unless `--days` says, from 1
to 3650; no session needed) is the same reading over every top-level task
made in the window, subtasks left out. Every figure is of those tasks but the
tasks done per day, which its header says:

- how many hold each status;
- for the done ones, the lead time (creation to close) and the time in each
  open status, each as median, p75 and p90 (a value between two tasks is
  taken proportionally between them); a status is spread over the done tasks
  that held it, and says how many did;
- on a line of its own, the done ones' wait before their first claim: a part
  of the statuses above it, not a row to add to them;
- how many tasks review sent back how often (`0 × 41 · 1 × 6 · 2 × 1`);
- top-level tasks done on each day of the window, whenever they were made (a
  task made a month ago and closed yesterday counts yesterday), by this
  machine's time zone, every date of the window named once: each task done
  now, once, on the day of its last close (`Stages::done_at`), so one approved,
  sent back to review by its landing and approved again counts once, at the
  second approve, and one cancelled or reopened since counts on no day. The
  Reports page's Tasks done counts the same tasks by the same rule, by UTC
  day (its day files' `done`, beside `entered.done`, which counts every entry
  into done);
- the ten open tasks that have held their status longest, with that status
  and how long;
- how many tasks are blocked and how many are ready with no executor, each
  with the median time they have been there.

`--json` carries the same figures as `by_status`, `lead_seconds`,
`stage_seconds` (by status; each spread is `tasks`, `median`, `p75`, `p90`),
`waiting_before_first_claim_seconds` (beside `stage_seconds`, never in it),
`bounces`, `done_per_day`, `slowest`, `blocked` and
`ready_unstaffed` (`tasks`, `median_seconds`), every duration in seconds.

**Arms.** A process experiment compares tasks filed on its sides. A task's
arm is that side's label, one word of lowercase letters, digits and dashes
(at most 32), given where the task is filed: `just tasks create SLUG TITLE
--arm control`. It is written with the task, among the board's own fields
(`fields._board.arm`, `crates/tasks/src/arm.rs`), so no table or schema
field holds it; no command changes it after, and `tasks api`'s `create`
takes none. `just tasks show SLUG` prints `arm: NAME` above the stages. The
board assigns no arm, applies no rule for one and names no winner: whoever
files the work says which side it is on.

Where a task of the window was filed on an arm, `stats` ends with each arm
by name: its tasks, how many are done, the median lead time and the median
time in review of the done ones (`-` with none done, or none reviewed), and
how often review sent the arm's tasks back, the open ones' too:

```
Arms (the tasks filed with --arm; lead and review are the medians of the done):
  control      tasks 2 · done 2 · lead 9.0 h · review 5.0 h · bounces 1
  no-review    tasks 2 · done 1 · lead 3.0 h · review 1.0 h · bounces 2
```

A task filed on no arm is in none of them and in every figure above them.
`--json` carries them as `arms`, by name (`tasks`, `done`, `lead_seconds`
and `review_seconds`, each a spread or null, and `bounces`), `{}` where the
window has none. The arm is read in the window's one read.

`stats` reads the window in one round trip (`Store::stage_records`): the
top-level tasks made since the window's start and the done ones written since
(a task closed in the window is one of them), who executes them, and each one's thread
by its own range of keys with the posts left out, never a thread at a time.
With the read that opens the board that is two requests whatever the board
holds (`tests/cli/stats.rs` counts them over 300 tasks).

## Where the tokens go

The heartbeat records what agents' model calls take (`crates/tasks/src/usage.rs`, the `usage`
table, schema 19). Each beat of the hook ([Sessions](#sessions)) reads what the session's own
transcript gained since its last beat: a subagent's `SESSION/subagents/agent-ID.jsonl` (the
event's `agent_transcript_path`, else beside its `transcript_path`), never its parent's; a full
session's `transcript_path`. The kinds a price tells apart are kept apart: `input` (Claude Code's
`input_tokens`, the uncached remainder), `cache_write_5m` and `cache_write_1h` (the cache writes
by lifetime, `cache_creation.ephemeral_5m_input_tokens` and `ephemeral_1h_input_tokens`; what the
split leaves, or all of it with none, is the five-minute kind), `cache_read` and `output`, with
the model calls counted as `calls`, by `message.model` and `usage.speed` (`standard`, or `fast`,
priced apart). Claude Code writes a message's usage on each of its lines, so a message id counts
once; a `<synthetic>` message is no call. A fork (a subagent whose `agent-ID.meta.json` beside
its transcript says `"isFork": true`) begins its transcript with its parent's history, under the
parent's message ids, which the parent's beats count: read from its start, it is passed over up
to the tool result answering the parent's call that made the fork (the meta's `toolUseId`), and
counted from there.

The board adds what a beat read in the beat's own one transaction to the row of the handle bound
to the key, the task and role it works then, the model, the speed and the round. The task is
the one the agents API shows it working: an open task it executes before one it reviews before
one reserved for it, then by priority and slug; orchestrating is no task's work, so a session
working none spends on no task (slug `""`, round 0) in its kind (`worker`, `orchestrator`). The
round is 1 and the moves into review the task's thread holds then (`HANDLE moved it FROM →
review`): the first round is the work up to the first review, and the review of round *n*'s work
and the fixes after it are round *n + 1*. A beat with nobody bound to its key (a subagent before
its first board command) leaves what it read waiting, and the first beat after a board command
binds it hands it on: a subagent's work before it named its handle is that handle's.

What a key read is kept in its ledger, `symphony/tasks/usage/KEY.json` in the git common dir
(`TASKS_USAGE` names another folder; a board `TASKS_SERVER` names reads no transcript without
it): the offset after the last whole line read, the last 32 message ids counted and what the
board has not taken. So a beat reads only what the transcript gained, at most 32 MiB of it (a
long transcript met first is read over as many beats), and a line still being written waits for
the next. A beat holds the key's `KEY.lock` while it reads and writes: a beat of the same key
meanwhile reads nothing, and the next one reads what it left. A ledger that does not parse
starts again at the transcript's end, so what cannot be read is lost, never counted twice; the
ledger is replaced whole (written beside it, then renamed). A ledger is removed only once nothing
wrote to it for 7 days and the transcript it names is gone, so a session resumed after a week
reads on from where it stopped; one that does not parse is kept. Reading fails quietly: a
transcript or ledger that cannot be read hands nothing, and the beat goes on.

What is not recorded:

- **Codex.** A Codex session runs no Claude hook, so it has no beat to record by: its tokens
  wait for a beat of its own.
- **A session with a monitor** (an orchestrator in a terminal the board launched) is never bound
  to its key, so its beats record nothing.
- **The last answer.** The beat runs at a tool call, so the reply after a session's last tool
  call is never read: replayed through the beats they would have had, a subagent's run of 242
  calls (46.7 MB of transcript) left the board 241 calls and 99.8 % of the transcript's tokens,
  and one of 2,705 calls (75.1 MB) 2,704 and 99.95 %, the last reply's being the rest; a parent
  and its four forks, short-lived, bound to one handle, 189 of 194 calls and 96.6 %.
- A beat run through `just tasks beat` (its event one an agent wrote, not the hook's) reads no
  transcript.
- **A beat that timed out after its write**, the other way round, is recorded twice: when its
  board call outlasts the beat's 2 s wait (a loaded machine) after the server committed it, the
  beat keeps what it read as not taken, and the next beat hands it again. A transaction retried
  on a conflict is safe, as the conflict aborted it.

`just tasks show SLUG` sums the task's rows, its subtasks' with them, under its stages line:

```
  Tokens: 71.6k in 6 calls (input 120 · cache write 4.5k · cache read 65k · output 2k)
    by role: executor 51.5k · reviewer 20.1k
    by round: round 1 51.5k · round 2 20.1k
```

each amount in three figures at most (`950`, `12.3k`, `7.8M`), rounded down, and their cost
below them ([What the tokens cost](#what-the-tokens-cost)); a task with none prints no lines.
`--json` carries them as `tokens`: `total`, `by_role` (`[{"role", "tokens", "cost"}]`),
`by_round` (`[{"round", "tokens", "cost"}]`) and the `rows` themselves (`handle`, `slug`,
`role`, `model`, `speed`, `round`, `tokens`, `cost`, `first`, `last`), with the `cost` of the
total beside it; each `tokens` is `{"input", "cache_write_5m", "cache_write_1h", "cache_read",
"output", "calls"}`. The agents API gives each agent's `tokens` and `cost`, all its rows summed
([The agents API](../engineering/BOARD_API.md#the-agents-api)). A landing's closing line names the task's after the commit
(`landed SLUG on main at SHA; 71.6k tokens in 6 calls (executor 51.5k, reviewer 20.1k; round 1
51.5k, round 2 20.1k), $0.08 estimated at API list prices read 2026-11-16`), each amount with
its unit, so no number in it reads as a commit. A copy of the board carries the rows
(`Dump.usages`).

The pages draw the same figures, each with the day its prices were read and as an estimate, never
money spent ([What the tokens cost](#what-the-tokens-cost)):

- **A task's page** says the cost after the time, on its timeline, from
  `/api/task-tokens?slug=SLUG` (`show --json`'s `tokens`, its `rows` among them): the total after
  the task's time (`1 d 3 h in all $80.02 · 6 review rounds`), whose words, pointed at or focused,
  say that it is an estimate and of what day's prices, its split by role and by kind, the tokens
  and the calls; and each stage stretch's after its own (`3 h 14 m $30.75`). Each row is on
  exactly one stretch, never a share by time, so the stretches add up to the total: an
  in-progress stretch carries the executor's rows of its round and what was spent while the task
  was reserved (the last of two in one round carries both), a review the reviewer's rows of the
  round it opens, and the executor's where no in-progress stretch follows; a row this places
  nowhere (a subtask's, round 0, another role) goes to the stretch its first call fell in. A
  stretch's words say whose calls it carries, when they ran and how many. A task with none draws
  no cost.
- **A board card** shows its task's cost all told, its subtasks' with it, before its time (`$1.23 · 1 h`;
  `$1.23+` where some tokens had no price, `unpriced` where none had, its title saying the rest),
  read apart from the board's
  listings, which carry none of it: the cards drawn in one turn are asked for together
  (`/api/task-costs?slugs=[…]`, at most `TASK_COSTS_MOST`, 200, a read: `Store::spent_under`, one
  round trip), each once until its record changes, so no heartbeat that adds tokens reads again.
  A task with none shows no figure.
- **The Agents page** gives each agent its tokens and cost under its line, from the agents API.
- **Reports** has *Tokens and cost*: the cost and the tokens of each hour, day or week by role
  (orchestrators, executors, reviewers and the rest), and below them the window's own: tokens and
  cost all told, each top-level task landed in the window priced once with its subtasks (the
  median and the ten dearest, each a link to its page), and the review rounds after the first
  against the first rounds, from the [day files](../product/analysis/reports-and-monitoring.md#the-day-files).
  An hour's tokens are what the board's `usage` gained between two hourly roll-ups, shared among
  the hours between them by time: an hour no two roll-ups span is a gap, never a zero.

`just tasks stats` ends with the same reading of its window's day files: tokens and cost by day
and role, the landed tasks' median and ten dearest, and the review rounds against the first, a
day no two roll-ups span saying `no record`; `--json` carries it as `tokens`: `prices`, `per_day`
(each `day` with its `by_role`, `null` for no record), and `landed` (`tasks`, `without`,
`median_dollars`, `dearest`, `first_round`, `later_rounds`, `again`). Without day files (a private server's board, or a board that
`TASKS_SERVER` names) it says there are none and prints the rest.

The table moves the board to schema 19, which a build on schema 18 reads unharmed (the
Symphony, the shared server among them): it opens a board one schema ahead to read
(`schema::decide`'s `Opened::Ahead`), and none of its reads or its dump names `usage`; what it
would write waits or is refused until it reloads, as for any server one schema behind ([Who
writes a board](../operations/BOARD_OPERATIONS.md#who-writes-a-board)). As a writer (every `tasks` command, `list`, `show` and
`agents` included) an 18 build is refused before any write, naming itself as the program to
rebuild. Its beat writes nothing either, so an agent whose hook runs an 18 build reads quiet
while it works until that link is renewed.

## What the tokens cost

Wherever the tokens show, their cost shows beside them, in US dollars, estimated as if the calls
were paid at the providers' API list prices: a subscription is not billed by the token, but a
cache read, a cache write and an uncached input token are priced so far apart that a count of
tokens alone does not say what the work took. Every figure says it is an estimate, and the day
the prices were read.

The prices are one table, `crates/tasks/src/usage/prices.toml`, embedded in every build of
`tasks` and of the Symphony, so an installed binary has it and reads nothing from disk, and an
edit takes effect with the next build. It holds the day its prices were read (`read`), the page
they were read from (`page`), and a row for each model id and speed (`standard`, or `fast` where
the provider prices it apart) with its US dollars per million tokens of each kind: `input`,
`cache_write_5m`, `cache_write_1h`, `cache_read` and `output`, each as the page gives it, never
as a multiple of another (a cache read is 0.025 of the input price on Claude Fable 5.2 and 0.05
on Claude Opus 5.5). The file's own header says how to edit it, and the board crate's price tests
read it as the builds do: a table that does not read prices nothing, so every model reads
unpriced rather than at a wrong price. It is one price for each model and speed, so an edit
reprices every call recorded, the earlier ones too.

A row of the board's `usage` table is priced by its exact model and speed
(`crates/tasks/src/usage/price.rs`). A model or a speed the table has no row for is unpriced: its
tokens are named beside the cost and are in no dollar figure, never priced at zero. Fast mode has
rows of its own; Claude Haiku 5.5 is priced at its price for prompts up to 100,000 tokens, as no
call's prompt size is kept. Money is counted exactly, in millionths of a millionth of a dollar,
and each figure is rounded to the cent where it is said, half a cent up, so the four kinds'
figures may not add up to the total's by a cent.

`just tasks show SLUG` prints the cost under the tokens, all told with its four kinds (the two
cache writes as one), by role and by round:

```
  Cost: $0.08, estimated at API list prices read 2026-11-16 (input $0.00 · cache write $0.02 · cache read $0.01 · output $0.04)
    by role: executor $0.05 · reviewer $0.02
    by round: round 1 $0.05 · round 2 $0.02
```

Where some tokens were unpriced, the first line ends `; none for gpt-x 1.2M tokens, in no
figure` (a speed other than `standard` after the model), and a role or round that holds some
reads `$1.23 + unpriced`, or `unpriced` where none of its tokens had a price; where none of the
task's had one, the line is `Cost: unpriced: the API list prices read 2026-11-16 have none for
gpt-x 1.2M tokens`. Each `cost` in `--json` and in the agents API is `{"dollars", "input",
"cache_write_5m", "cache_write_1h", "cache_read", "output", "unpriced": [{"model", "speed",
"tokens"}], "prices"}`: the dollars of the priced tokens all told and by kind, the tokens no price
was found for, and `prices`, the day the table's prices were read; an agent with no tokens has
`cost: null`. A landing's closing line says `$0.08 estimated at API list prices read 2026-11-16`
after the tokens, then `, none for gpt-x 1.2M tokens` where some were unpriced, or `no API list
price read 2026-11-16 for gpt-x 1.2M tokens` in its place where none had a price.

## The timeline

A task's page draws what the task went through under its [gates](#gates) and
subtasks, above the brief:
`GET /api/task-timeline?slug=SLUG` (400 with no slug, 404 for a task not on the
board) answers the wire type `TaskTimeline` of `symphony_api::timeline`, built by
`crates/symphony/src/timeline.rs` from the task alone, never the whole board, from
one read of its thread and of the recorder's history that the gates answer shares.
The page reads it as it mounts, behind no other read (the timeline's code alone
waits for its box to near the screen), again at once when the task
changes, and each `every` seconds (a minute) while the task is open and the page is
shown, since a build or a gate moves without the task changing; a hidden page reads
nothing, and once shown again reads once. Each read names the last answer's
validator, which leaves out the answer's `now`, so a minute in which nothing moved is
a `304` and the page counts `now` on by its own clock. A read that fails leaves the
last answer shown; with none, the box says why in a sentence, grows to show its Try
again, and is read again by itself, a second on, then two, doubling to thirty,
until it answers. Only the box of the task the page shows draws
its timeline: an answer that arrives after the page moved to another task is
dropped. Every time is server time in seconds; `zone` gives the server's
offset from UTC at the task's making and at each change of it after (summer time),
so each time reads on the server's local clock as it then stood, the thread's.

- **stages**: the segments of the replay above, which goes on past a close a move
  out of `done` or `cancelled` reopens, so a reopened task keeps its history and
  the bounces after it. A stretch in review is a round (`round`, from 1). Where the
  round's verdict did not close the task (a project's review gate turning met while
  the task waits on its landing), the stretch after the approval is a stage of its
  own, `approved`: the wait to land.
- **claims**: who held the executor role while the task was in progress, from the
  claims, hand-overs and releases on its thread (`stages::holds`).
- **rounds**: each review round, from the task going to review: `taken` when a
  reviewer took the role up (its start when one held it already), the time before
  it the wait for a reviewer; `to` and `verdict` the round's end, by the board's
  `approve` or `changes` verdict or the review gate's turning met; `by` the reviewer.
- **runs**: the builds and tests the board's recorder kept for the task
  ([MACHINE_WORK](MACHINE_WORK.md#recording-builds-and-tests)), in its history or
  running now, a step inside one of them left to that one: phase, label, who,
  commit, host, when queued, started and ended, and the result (`queued`,
  `running`, `green`, `red`, or `unknown` when its recorder ended before it could
  say). None until the recorder keeps one for the task: the page then draws no
  builds or tests at all, and no machine time. Where the recorder's history no
  longer reaches back to the task's start, `runs_from` says since when it keeps
  every run, and the page says so.
- **gates**: each gate's states from the [gates](#gates) answer, its past and its
  current, a lane each by name; a state taken before the task was made counts from
  its creation, and the review gate's from the task's first move to review. A stamp
  left out, by the task's assessment or by default with none, has no lane: the gates
  list it, waived. A gates answer that cannot be read leaves the lanes out, never the
  timeline.

The page keeps a box for it (`apps/symphony-web/src/timeline-slot.tsx`) as tall as
this browser last drew the task's timeline, with placeholder rows in it, so nothing on
the page moves when the timeline comes. On a phone the box is at most 240 px (the
owner's choice, so the brief and the thread start near the page's top), and a read that
adds a step moves nothing either: the steps fit the box, what does not fit fading out
(the latest steps in view), and "Show all N steps", on the legend's line, lets it grow.
From 900 px wide, where the lanes replace the list, the timeline is shown whole, with no
"Show all" (the owner's choice: a collapse there saves little room and hides the lanes):
the box grows to hold every lane and keeps that height for the next visit, a task never
drawn there taking a typical whole timeline's, 508 px. The whole of the lanes' part the
focus is on (a click focuses one too) is said in the legend's place while one has it, so
the box does not grow for it.
The timeline itself is a chunk of its own (`apps/symphony-web/src/timeline.tsx`),
which its box loads once it nears the screen and its read has something to draw, so the
page's first draw does not wait for it. On a phone it is a list: a row per
stage with its start, its label, its length and a bar as long, the wait in it drawn
lighter; under each stage the builds, tests and gate changes that began in it (the
review gate's are its rounds), a build or test named by its command, the machine's by
what it ran and its commit; the verdict after a review's bar, and a striped bar for a
run going on now; a stage left within half a minute has no row. From 900 px wide it
draws lanes over the same hours: the stage, the executor, the reviewer, the builds and
the tests, each a lane of its own (a row each for runs that overlap, or where one is
queued so soon after another's end, half a percent of the hours drawn, that the two
would read as one bar: no two runs are drawn as one; a lane that needs more than six
rows, a burst of runs on a timeline of days, draws them thin with no words under them),
and a thin row per gate, each state by its pattern as well as its colour (met solid,
missing dotted, waived dashed, held double), with a line at now. A word in the lanes is drawn only where it fits; every
part of them is said to assistive technology and reached by the arrow keys, and the
list stays for assistive technology beside them. A run's words name its command, its
wait to start and its run time apart (to the second under a minute), how it ended, its
commit, who ran it and where: a pointer resting on its part reads them; wide, its part
in its lane with the focus (the arrow keys, a click or a tap) says them in the legend's
place, and on a phone its row a tap focused says them under its bar, growing upward in
a box that clips, the rows under it where they were. The pictures it follows are
`docs/product/analysis/reports-timeline-phone.svg` and
`docs/product/analysis/reports-timeline-wide.svg`.

- [-] **timeline-burst-drawing** — A burst of runs, dozens of tests minutes apart in a task that lasted days, makes the task's wide timeline 12 px taller per run, with no limit: tools/board-token-usage's is 911 px tall, and its Tests lane is 37 thin rows that read as a dashed line, with no words. How should a lane draw a burst, every run kept its own and reachable? A: as it is, a thin row per run, the box growing. B: the same rows, the lane scrolling in a box of its own past 256 px, the height of six rows with words. C: each burst one mark that says how many runs and how many went red ("37 tests · 4 red"), opened in place to its runs by a click, a tap or Enter; on a phone, one row of the list the same way. Recommended: C. Approve and say A, B or C in the note, or comment. [[TASKS#The timeline]]
  [2026-11-17 00:43] agent — Posted for the board task `app/timeline-burst-drawing-card`, from the review of `app/timeline-builds-and-tests-apart`. tools/board-token-usage at 1280 wide, on a private server of main's build (the shared server runs a release from before builds and tests had lanes of their own): A as it is, then B and C drawn on that page by hand, not built. ![The burst wide, option A](../design/analysis/timeline-burst/a-wide.webp) ![The burst wide, option B](../design/analysis/timeline-burst/b-wide.webp) ![The burst wide, option C](../design/analysis/timeline-burst/c-wide.webp)

    **A, as it is.** The box is 911 px; a typical whole timeline is 508. The Tests lane alone is 449 px: 12 tests on 11-15 and 37 on 11-16, each a sliver on five days of hours, each in a row of its own so that none is drawn over another. A run's words are only in its hover, its focus and its row on a phone.

    **B, the lane scrolls.** The box is 719 px. The lane stops at 256 px and scrolls the rest, its scroll bar in the margin past the hours. The rows are A's, no easier to read, and the runs below its edge are a scroll away inside a page that scrolls too.

    **C, one mark per burst.** The box is 509 px, about a typical timeline's. The lane is one row: a stacked mark with its count, its words under it. Opened, the burst draws its runs under the mark as A does, and closes the same way. The mark is no run's bar: its count says it holds several, so it never reads as one long run. A burst is runs each queued so soon after the one before it ended (half a percent of the hours drawn, as today) that they would need more than six rows; every other run is drawn as now.

    **What each costs.** A, nothing. B, a few lines of style, and a box that scrolls inside the page, which a trackpad catches as it passes over. C the most: the bursts found in the layout, a mark the keyboard and a tap open and close, its words, the phone's folded row, and their tests.

    **Why C.** What a burst says is how many ran and whether any went red: C says it on one line, wide and on a phone, where the thin rows say neither. It gives back 402 px wide and 2,140 px on a phone's Show all, and every run is still one press away, each with its words in its hover, its focus and its row.

    **Earlier verdicts checked.** **task-page-timeline-place** (C) keeps a phone's box at 240 px behind Show all; no option changes that. Your note on `inbox/timeline-should-not-be-collapsed-by-defa` had the wide timeline shown whole: "Desktop collapse save almost no space and hides really good information from view." It weighs against B and C, which hide runs until scrolled or opened, and does not decide this card: that collapse cut every task's lanes for little room, where a burst costs 402 px of rows that show no words. `app/timeline-builds-and-tests-apart`, from your question whether a 55 min build was several, never draws two runs as one bar, and no option does. None of them decides how a burst is drawn.
  [2026-11-17 00:43] agent — On a phone the box opens at 240 px on the latest steps under every option (here round 3 and Done), so the burst shows only after Show all. There A and B list every run, 61 steps in 3,143 px, each row reading "Test, just test-rs -p sympho…"; C folds more than six runs in a row of the list into one row, 16 steps in 1,003 px. The list puts a run under the stage it began in, so 11-15's burst, which spans two stages, folds its first seven runs and leaves the last six as rows. ![A and B on a phone, after Show all](../design/analysis/timeline-burst/phone-all.webp) ![C on a phone, after Show all](../design/analysis/timeline-burst/c-phone-all.webp) C wide, in dark: ![C wide, dark](../design/analysis/timeline-burst/c-wide-dark.webp)
  [2026-11-17 00:43] agent — **Not asked here: the word under a bar.** It is the command's first word, so every `cargo install` reads "cargo 55 m" and every `just test-rs` "test-rs", and a phone's row is cut before what tells two runs apart. Better: the command's distinguishing words, the command without `just`, its flags and the test filter after `--`. So `just test-rs -p symphony-board --test cli -- beat` reads "test-rs symphony-board cli", `cargo install symphony-server (cold-full)` keeps all four words and `just lint-rs` stays "lint-rs"; under a bar with too little room for them, the first word as now, so no word goes where one shows today. It is not built here: it changes the drawing this card's answer changes, so it is built with that answer.
  [2026-11-17 12:15] owner · question — why whould this be vertical instead of being horizontal reflecting actual ran times?
  [2026-11-17 12:28] agent — Time already runs horizontally on the wide timeline: each bar is positioned by its recorded times. The vertical pile comes from a layout rule that gives runs separate rows not only when they overlap, but also when they are close together (within 0.5% of the whole time range). Across five days that is 36 minutes, so even sequential short tests get stacked. That avoids bars visually joining, but creates the height problem this proposal then tries to hide. Your suggestion addresses the cause: keep sequential runs on the same horizontal time axis, reserve extra rows for actual overlap, and let a closer time view reveal short runs at their real duration. The proposal should explore that before folding bursts; A/B/C overlooked it. The phone view is a separate chronological list because of its narrower width.
  [2026-11-17 12:35] owner · rejected — yes. rework the proposal to be more horizontal based.
  [2026-11-17 12:35] agent — Revised proposal: keep Builds and Tests on a shared horizontal time axis. Sequential runs reuse the same row; only genuinely overlapping queued/running intervals need another row. Each run keeps its actual start, end and duration, with waiting distinguished from execution. For a dense period, zoom into that time range and pan horizontally, with a clear return to the whole task; every lane uses the same scale so comparisons remain meaningful. Selecting a run shows its full command, exact times and result without adding a row per run. Very short runs stay individually reachable by keyboard and selection, without stretching their duration to fit a label. This replaces the vertical stacking, inner scrolling and folded-burst proposals above. The earlier pictures illustrate rejected options, not this revision; the revised layout still needs visual validation, including a narrow-screen treatment consistent with your existing phone-height decision.

## Links with the documents

A task's `--link` lists it in the **Related tasks** panel of that document's page in the
Symphony, beside the document (below it on a narrow screen): open work first, each row its
title, slug and status, and finished work folded under its count. The `verdicts/…` tasks answer
the owner, not the document, and are left out. A link stays as it was filed: once its document
moves folders, the task's page still opens it, and its page still lists the task, by its file
name, as a wikilink to that name finds it; a name two documents share opens neither, and
**Document not found** offers both. A heading the link names by its text (`#The order`) opens
the page at that heading, as its anchor (`#the-order`) does. In the other direction, write
`[[task:sim/jump-departure]]` in any document: it renders as a link to the
task, with its live status beside it.

## Limits

- **One machine.** The board's server listens on this machine alone
  (`127.0.0.1`) and keeps its data in `.git/`; a session on another machine or
  in the cloud cannot reach it.
- **The owner's review of a task is the reviewer's verdict**, not a web button.
  The owner steers through the thread.
- **Nothing is reassigned automatically.** A lost session's work becomes
  claimable, and the claim that takes it over says so in the thread.
- **A subagent is seen through its tool calls and what it left running.** One
  waiting on a background job without calling a tool reads `running_test` while
  a process it started works in the linked worktree its last tool call ran in;
  one with nothing of its own working there (or in the main checkout, which says
  nothing of whose a process is) goes stale after 90 s, and lost after 10 minutes if it holds
  claimable work. The board's own health (`just tasks show`, a claim) still ages
  by its beats alone.

## Operation and APIs

What follows is said in full in two documents of their own, each under the heading it has here.

### Who writes a board

The line a board keeps for each program that opens it to write, and what `just tasks migrate`
reads of them before it moves a board: [BOARD_OPERATIONS](../operations/BOARD_OPERATIONS.md#who-writes-a-board).

### Stuck servers

A board server that outlasts its one ask to end, or a private replay nobody can tell is
through, and what the owner does about each: [BOARD_OPERATIONS](../operations/BOARD_OPERATIONS.md#stuck-servers).

### A board that went back

A board refused because its store holds less than the schema it was seen at: what it means,
accepting an older board that was meant, and recovering what was lost, step by step:
[BOARD_OPERATIONS](../operations/BOARD_OPERATIONS.md#a-board-that-went-back). Do not run `just tasks migrate` on it.

### A board that was copied or restored

A copy of a whole board, or its files put back, starts another history: the generation, the
recovery binding and `just tasks recovery`: [BOARD_OPERATIONS](../operations/BOARD_OPERATIONS.md#a-board-that-was-copied-or-restored).

### Board health

`just tasks status` prints the board's health over the last hour as its Symphony server
recorded it, and every page and the project picker say when it is unwell. What is counted, the
record and its hour files, and each warning: [BOARD_OPERATIONS](../operations/BOARD_OPERATIONS.md#board-health).

- [r] **sweep-swept-retention** — Before the sweep removes a finished worktree, it moves what git ignores in its artifacts folder (the pictures its thread names, a test's leftovers) into the main checkout's `artifacts/swept/NAME/`, and nothing ever empties that folder. Any rule that empties it deletes evidence, so the rule is yours. How long is what was set aside kept? A: for good, every dry run saying how much it holds (built). B: 30 days after the worktree's task closed, then deleted, the dry run naming what goes next. C: only the files a task's thread names; the rest is deleted as the worktree goes. Recommended: A. Approve and say A, B (and the days, if not 30) or C in the note, or comment. [[TASKS#Board health]]
  [2026-11-12 14:31] agent — Posted for the board task `tools/sweep-removes-nothing`. A dry run on a scratch repository after one sweep set a worktree's evidence aside; the highlighted line is the new one: ![A sweep's dry run, ending with what the main checkout holds set aside](analysis/sweep-dry-run-set-aside.svg)

    **What it would hold.** On 2026-11-12, 16 of this repository's 40 worktrees had an artifacts folder, 4.6 GB in all: 3.5 GB of it one review's build, 0.9 GB one test's proof, the rest a few MB each. Each move is a rename on the same disk, so it takes no new space: it is what the worktrees held already.

    **A, keep for good.** Nothing a thread names is ever lost. Cost: the folder grows with every task that leaves evidence, a review's build kept in its artifacts included; the dry run says how much, and the board warns on every page when free disk drops under 20 GB.

    **B, keep 30 days.** The disk comes back by itself. Cost: a link on an older thread stops working, and a task reopened after that has lost its pictures.

    **C, keep what a thread names.** Keeps the evidence and drops the leftovers, such as that 3.5 GB build. Cost: a picture named by another spelling, or only in a card, is deleted; the sweep reads the threads of every task tied to the worktree before each removal.

    **Why A.** It deletes nothing until a size worth acting on shows, and B or C can follow as one rule over the same folder.
  [2026-11-12 14:39] owner · question — images should be kept, build should not. Images and other artifacts can be pushed into a main tree. However there should be a cap to the size of the folder, after which it should ask/warn agents to manually check and remove stale images and artifacts not in use. Maybe 5 GB limit?
  [2026-11-12 15:14] agent — Built as you said, for the board task `tools/sweep-removes-nothing`. Your words, 2026-11-12: "images should be kept, build should not. Images and other artifacts can be pushed into a main tree. However there should be a cap to the size of the folder, after which it should ask/warn agents to manually check and remove stale images and artifacts not in use. Maybe 5 GB limit?" and, the same day: "Instead of directly commiting, artifacts could be attached some with some different process. Either nodes, or git files? Forgot the name but I think there was something for this type of challenge in particular. But if there's something that still actively references the artifact, then it should be kept until space used becomes a problems requiring it to be remove."

    **Kept, never committed.** The orchestrator's reading of "pushed into a main tree", labelled as theirs: kept in the main checkout's artifacts folder (ignored by git), not committed to the repository. Attaching artifacts without a commit on main (Git LFS, notes or a ref of its own) is a design task of its own, not built here.

    **Build output is never kept.** A folder tagged with `CACHEDIR.TAG`, as Cargo's build folders are, or a file of compiled code, known by its first bytes (ELF, Mach-O, wasm, an archive), goes with the worktree. A correction to the figures above: the 3.5 GB I called one review's build is no build but a scratch repository's `.git` (`artifacts/hg-review/sf/.git` in that worktree), which these rules keep; the test's proof holds 280 MB server binaries, which are compiled code and now go.

    **The cap, and nothing removed.** `[board] swept_cap_gb` in `symphony.toml`, 5 GB when unwritten, your suggested figure. The sweep never removes anything it set aside. Past the cap it still sets aside, and the sweep's output, the land's and a dry run's say so in one line, and `just tasks status` warns of it until it is under again, naming the size, the cap and the five largest folders with their tasks and stages. Each folder is marked `in use: WHERE` while a task tied to it is not done or cancelled, a post on an open task's thread names a path under it, or a file of `docs/` on main does (cards included), and `no reference found` otherwise; only those are suggested for the check by hand. The lookup is one read of the open tasks' threads and one search of `docs/`, made once each time the folder is measured past its cap. Rules: [[TASKS#Board health]].
  [2026-11-12 17:43] owner · approved
  [2026-11-12 17:59] agent · resolved

- [r] **sweep-artifacts-set-aside** — A worktree whose task is done or cancelled used to stay whole for as long as anything sat in its `artifacts/` folder: the pictures its thread names, a test's leftovers, a review's build. That was the reason in 729 of the sweep log's 850 "kept" lines. What should the sweep do with that folder before it removes the worktree? A: move it into the main checkout's `artifacts/swept/NAME/`, and back if git refuses the removal. B: delete it with the worktree. C: keep the worktree, as before. Recommended: A, which is built. Approve and say A, B or C in the note, or comment. [[TASKS#Board health]]
  [2026-11-12 12:15] agent — Posted for the board task `tools/sweep-removes-nothing`.

    **What the folders hold today.** 16 of the 40 worktrees have one, 4.6 GB in all: 3.5 GB is one review's build (`hg-review`), 0.9 GB one test's proof, and the rest is a few MB each.

    **A, move.** Nothing is lost, and every picture a thread names can still be found by its worktree's name. The move is a rename on the same disk, so it copies nothing. Cost: `artifacts/swept/` in the main checkout keeps growing until someone empties it, about 4.6 GB today.

    **B, delete.** All the disk comes back at once. Cost: a task's evidence pictures are never committed, so a thread's link to one stops working once its worktree goes.

    **C, keep.** Nothing changes. Cost: the 16 worktrees and their build output stay until someone empties each folder by hand. That is part of why the sweep removed 0 of 38.

    **Why A.** It loses nothing, frees everything else in the worktree, and switching to B later is one line. The task's report has the other causes of "removed 0".
  [2026-11-12 12:38] owner · approved — A
  [2026-11-12 12:42] agent — Built as A says, nothing else changed. Before the sweep removes a worktree whose work is finished, it moves what git ignores in that worktree's artifacts/ into the main checkout's artifacts/swept/NAME/, NAME being the worktree's folder (NAME-2, NAME-3 when that name is taken). If git then refuses the removal, everything goes back. Nothing empties artifacts/swept/ by itself. It goes to review with the board task tools/sweep-removes-nothing now.
  [2026-11-12 12:42] agent · resolved

### The tasks API

`tasks api`, the board's methods as JSON for a program rather than a person, a versioned
contract: [BOARD_API](../engineering/BOARD_API.md#the-tasks-api).

### The agents API

`GET /api/agents` and `just tasks agents --json`, who is doing what, with the rules a port
copies, and what a callback and the PULL digest push: [BOARD_API](../engineering/BOARD_API.md#the-agents-api).
