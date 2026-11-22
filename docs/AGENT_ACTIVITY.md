# Agent activity

The machine's one Symphony server keeps, for each project it serves, a line for each
change in an agent session's health and, once an hour, a line of what the hour held:
the sessions started, gone stale and gone lost and the time they spent so, the
sessions running and holding work by role and model, the live sessions by kind, and
the machine's load beside them. The Reports page reads them
([the analysis](analysis/reports-and-monitoring.md#agents)); nothing here is on the
board, the board's schema is unchanged, and no board read or write is made for them.
The lines' shape is schema 1, its types on the wire in
`crates/symphony-api/src/agent_activity.rs` and its writer in
`apps/symphony-server/src/live/agent_activity.rs`.

## What is watched

A session's health is worked out on each read of the board, never stored
([TASKS](TASKS.md#sessions)): `live`, `stale` after 90 s without a beat, `unknown` while
a native presence sweep is overdue, `lost` after ten minutes for one holding work
another session could take, `left` once it left or went quiet holding none. The
server's tick reads the board about once a second and works these out for its pages;
the recorder takes that same snapshot, so it costs the board nothing. Each line's
time is the server's clock at that tick.

The first snapshot after the server starts says a **start** and each session it finds
that has not left as **seen**, and says no change: what happened between the last
line of the server before and this start went unseen, and none of it is invented.
From then on, each session whose health differs from the snapshot before is a
**change**, a session new on the board is a change from nothing, and one gone from it
a change to nothing.

Three counts are kept at each snapshot:

- **Running**: a session that is live and holds an executor, reviewer or orchestrator
  role on a task in progress or in review. It is the agents making tool calls on work
  that is moving.
- **Holding**: the same roles on the same tasks, held by a session that is live, stale
  or unknown, not lost or left. It counts the agents still on that work, including
  those quiet for a while, such as an executor waiting on a long build. A quiet holder
  counts only until it reads lost, or left for an orchestrator: ten minutes without a
  beat, so a build longer than that drops its executor from holding.
- **Live**: every live session, by its kind (`orchestrator`, `worker`, `reviewer`),
  whatever it holds. It counts an orchestrator whose tasks are all ready or blocked,
  which running and holding miss.

A session counts once for each role it holds in running and in holding, however many
tasks it holds it on, and once among the running or holding agents.

**Time watched.** Each snapshot's state is taken to hold until the next one, when
they are at most 120 s apart; a longer gap (a board that did not answer, a server
that stopped) is not counted, so the time stale and lost, the means and
`observed_s` cover what the server saw. Means are over that time, each value
weighted by how long it held; an hour with one snapshot has that snapshot's values.
The recorder takes only snapshots the board answered, so an hour in which the server
ran but every board read failed has no line, as an hour it did not run.

**What it costs.** The recorder runs in the server's worker after the snapshot, never on
a request. It keeps the last snapshot's sessions in the board's order (by when they
connected, then handle), so a tick that adds none, or adds new ones after them, compares
each session's health in place and copies nothing for one that did not change; a
session gone or connected again matches the rest by handle that tick. A file is opened
only on a tick with lines to write. The ignored test `cost` in
`apps/symphony-server/src/live/agent_activity_tests.rs` measures a tick with a few
hundred tasks and sessions and with ten times as many, and one append.

## A reload's hand-off

The shared server reloads when the sources it serves from change, and at a release's
refresh ([RELEASES](RELEASES.md#the-refresh)), by handing its sockets to a replacement it
built and started (`apps/symphony-server/src/reload.rs`): the replacement reads the board
and says it is ready, the serving server commits the hand-off, and only then stops. Both
are alive and reading the board for that time, so each server generation has one gate,
shared by every project it serves and by both its recorders, the agents' and the board's
health hours ([BOARD_OPERATIONS](BOARD_OPERATIONS.md#board-health); `crates/tasks/src/recording.rs`, held in
`front::Shared`), and only one generation records at a time:

- The replacement records nothing until the hand-off commits. Its first snapshot after
  the commit says a start, as any start does.
- The serving server closes its gate just before it commits. Its hours being summed end
  then, and their lines are written at its next tick or as it stops, whichever is
  first; it records nothing after.
- A replacement that never becomes ready is stopped having recorded nothing; the
  serving server goes on as if no reload had begun.
- A commit that fails after the close opens the serving server's gate again. Its hour
  then has a line up to the close and another from its first snapshot after, and the
  time between is watched by no one.

So no two lines cover the same moment: the old generation's lines end at the close, and
the replacement's begin after the commit.

## The day files

`symphony/agents/YYYY-MM-DD.jsonl` in the git common dir, in the one `symphony/` folder
of Symphony's state there, as the runs' ([RUNS](RUNS.md#the-day-files));
`symphony/agents.<database>/` for a board database other than the live one.
`SYMPHONY_AGENT_ACTIVITY` names another folder (a test's); a board `TASKS_SERVER`
names keeps none without it. Only the shared server, the one that writes the board's
`health.json`, writes them ([BOARD_OPERATIONS](BOARD_OPERATIONS.md#board-health)): a private server keeps no
recorder and writes nothing.

A line is one JSON object and its newline. An hour's line goes to the file of the UTC
day its hour began, every other to the day of its time. A tick's lines for one day are
appended in one write to a file opened to append; one that finds the file's last line
cut short (a writer that died in its write) starts with a newline, so that line stays
apart, and a reader passes over a line that does not parse. No lock is taken, as one
server generation records at a time (above). A write that fails is said once on the
server's log, until it fails otherwise or succeeds; its lines are not kept. A day file
is never written or read through a link or from a FIFO. Once a day, at the day's first
snapshot that finds the folder, each day file of a day more than 90 days before goes: a
regular file named `YYYY-MM-DD.jsonl`, never a link, a folder or another file.

## The lines

Every line has `schema` (1), `type`, `at`, when it was written by the server's clock in
seconds since the epoch, and its writer: `pid`, the server's process, and `generation`,
when that server began recording (its start line's `at`). Two servers of one hand-off
have different pids; a pid used again on another day has another generation.

**start**: `sessions`, how many the board holds; `left`, how many of them had left,
which no seen line names.

```json
{"type":"start","schema":1,"at":1794400000.4,"pid":4242,"generation":1794400000.4,"sessions":2618,"left":2597}
```

**seen** and **change**: `handle`, `kind` and `model` from the session's row; `task`
and `role`, the task it holds a role on (of its executor, reviewer and orchestrator
roles: one on an unfinished task first, then executing before reviewing before
orchestrating, then the newest) or null; a seen line's `health`, a change's `from` and
`to`, each null for a session new on the board or gone from it.

```json
{"type":"seen","schema":1,"at":1794400000.4,"pid":4242,"generation":1794400000.4,"handle":"misty-hare","kind":"worker","model":"opus","task":"tools/x","role":"executor","health":"live"}
{"type":"change","schema":1,"at":1794400091.2,"pid":4242,"generation":1794400000.4,"handle":"misty-hare","kind":"worker","model":"opus","task":"tools/x","role":"executor","from":"live","to":"stale"}
```

**hour**, written at the first tick after the hour ends, and for the hour so far as the
server stops or closes its gate:

| field | what |
|---|---|
| `start` | the hour's start, a whole UTC hour in seconds since the epoch: the same `start` as the hourly roll-up's, on which the two files join |
| `from`, `to` | what the line covers: from the hour's start, or the writer's first snapshot in it, to the hour's end, or when the writer stopped or closed its gate |
| `observed_s` | the seconds of that the server watched (above) |
| `samples` | the snapshots taken in it |
| `sessions` | per `kind` and `model`: `started`, sessions whose row says they connected in the hour before `to`; `went_stale` and `went_lost`, the changes to each seen in it; `stale_s` and `lost_s`, the seconds sessions spent so, watched |
| `running` | per `role` and `model`: `peak`, the most running at any time in it, and `mean` |
| `running_roles` | the same per `role` |
| `running_agents` | the same for the running sessions, each once |
| `holding`, `holding_roles`, `holding_agents` | the same three for the sessions holding work |
| `live` | per `kind`: `peak` and `mean` of the live sessions |
| `load_1m`, `load_15m` | the machine's load average over 1 and 15 minutes, read at each snapshot: `peak` and `mean`; null when it could not be read |

```json
{"type":"hour","schema":1,"at":1794402000.7,"pid":4242,"generation":1794390000.2,"start":1794398400.0,"from":1794398400.0,"to":1794402000.0,"observed_s":3598.1,"samples":3582,
 "sessions":[{"kind":"worker","model":"opus","started":4,"went_stale":2,"went_lost":1,"stale_s":312.0,"lost_s":120.5}],
 "running":[{"role":"executor","model":"opus","peak":5,"mean":3.42},{"role":"orchestrator","model":"opus","peak":1,"mean":1.0}],
 "running_roles":[{"role":"executor","peak":5,"mean":3.42},{"role":"orchestrator","peak":1,"mean":1.0}],
 "running_agents":{"peak":6,"mean":4.42},
 "holding":[{"role":"executor","model":"opus","peak":6,"mean":4.9},{"role":"orchestrator","model":"opus","peak":1,"mean":1.0}],
 "holding_roles":[{"role":"executor","peak":6,"mean":4.9},{"role":"orchestrator","peak":1,"mean":1.0}],
 "holding_agents":{"peak":7,"mean":5.9},
 "live":[{"kind":"orchestrator","peak":2,"mean":1.6},{"kind":"worker","peak":6,"mean":3.8}],
 "load_1m":{"peak":21.3,"mean":14.07},"load_15m":{"peak":16.2,"mean":15.1}}
```

(shown on several lines here; in the file it is one). A kind and model, a role and
model, a role or a kind with nothing to count in the hour has no row. Means and seconds
are to the thousandth.

**Reading an hour.** An hour can have more than one line, each with its `start`: a
server that stops writes the hour so far and the next one writes the rest when the hour
ends, and a hand-off's old generation writes the hour up to its close and the
replacement the rest. A failed commit gives one writer two lines of the hour. No two of
them cover the same moment, so they are taken together the same way, whoever wrote
them:

- `observed_s`, `samples`, `went_stale`, `went_lost`, `stale_s` and `lost_s` add up,
  per kind and model;
- a peak is the largest of the lines';
- a mean is weighted by each line's `observed_s` (by `samples` where every line's
  `observed_s` is 0);
- `started` is the line with the latest `to`'s, as each counts from `start` to its `to`.

The change lines need no such rule: each change is written once, by the generation
recording then. An hour no server ran at its end and none stopped in, or in which every
board read failed, has no line.
