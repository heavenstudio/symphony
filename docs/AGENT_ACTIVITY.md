# Agent activity

The machine's one Symphony server keeps, for each project it serves, a line for each
change in an agent session's health and, once an hour, a line of what the hour held:
the sessions started, gone stale and gone lost and the time they spent so, the
sessions running work by role and model, and the machine's load beside them. The
Reports page reads them ([the analysis](analysis/reports-and-monitoring.md#agents));
nothing here is on the board, the board's schema is unchanged, and no board read or
write is made for them. The lines' shape is schema 1, its types in
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

A session is **running** while it is live and holds an executor, reviewer or
orchestrator role on a task in progress or in review. It counts once for each of those
roles it holds, however many tasks it holds it on, and once among the running agents.

**Time watched.** Each snapshot's state is taken to hold until the next one, when
they are at most 120 s apart; a longer gap (a board that did not answer, a server
that stopped) is not counted, so the time stale and lost, the means and
`observed_s` cover what the server saw. Means are over that time, each value
weighted by how long it held; an hour with one snapshot has that snapshot's values.

**What it costs.** The recorder runs in the server's worker after the snapshot, never on
a request. It keeps the last snapshot's sessions in the board's order (by when they
connected, then handle), so a tick that adds none, or adds new ones after them, compares
each session's health in place and copies nothing for one that did not change; a
session gone or connected again matches the rest by handle that tick. A file is opened
only on a tick with lines to write. The ignored test `cost` in
`apps/symphony-server/src/live/agent_activity_tests.rs` measures a tick with a few
hundred tasks and sessions and with ten times as many, and one append.

## The day files

`symphony/agents/YYYY-MM-DD.jsonl` in the git common dir, in the one `symphony/` folder
of Symphony's state there, as the runs' ([RUNS](RUNS.md#the-day-files));
`symphony/agents.<database>/` for a board database other than the live one.
`SYMPHONY_AGENT_ACTIVITY` names another folder (a test's); a board `TASKS_SERVER`
names keeps none without it. Only the server that writes the board's `health.json`
writes them ([TASKS](TASKS.md#board-health)): a private server keeps no recorder and
writes nothing.

A line is one JSON object and its newline. An hour's line goes to the file of the UTC
day its hour began, every other to the day of its time. A tick's lines for one day are
appended in one write; one that finds the file's last line cut short (a writer that
died in its write) starts with a newline, so that line stays apart, and a reader
passes over a line that does not parse. The one server writes them from its worker
alone, so no lock is taken. A write that fails is said once on the server's log,
until it fails otherwise or succeeds; its lines are not kept. A day file is never
written or read through a link or from a FIFO. Once a day, at the day's first snapshot
that finds the folder, each day file of a day more than 90 days before goes: a regular
file named `YYYY-MM-DD.jsonl`, never a link, a folder or another file.

## The lines

Every line has `schema` (1), `type` and `at`, when it was written by the server's
clock, in seconds since the epoch.

**start**: `pid`, the server's process; `sessions`, how many the board holds; `left`,
how many of them had left, which no seen line names.

```json
{"type":"start","schema":1,"at":1794400000.4,"pid":4242,"sessions":2618,"left":2597}
```

**seen** and **change**: `handle`, `kind` and `model` from the session's row; `task`
and `role`, the task it holds a role on (of its executor, reviewer and orchestrator
roles: one on an unfinished task first, then executing before reviewing before
orchestrating, then the newest) or null; a seen line's `health`, a change's `from` and
`to`, each null for a session new on the board or gone from it.

```json
{"type":"seen","schema":1,"at":1794400000.4,"handle":"misty-hare","kind":"worker","model":"opus","task":"tools/x","role":"executor","health":"live"}
{"type":"change","schema":1,"at":1794400091.2,"handle":"misty-hare","kind":"worker","model":"opus","task":"tools/x","role":"executor","from":"live","to":"stale"}
```

**hour**, written at the first tick after the hour ends, and as the server stops for
the hour so far:

| field | what |
|---|---|
| `hour` | the hour's start, a whole hour in seconds since the epoch |
| `from`, `to` | what the line covers: from the hour's start, or the first snapshot in it after a start, to the hour's end, or when the server stopped |
| `observed_s` | the seconds of that the server watched (above) |
| `samples` | the snapshots taken in it |
| `sessions` | per `kind` and `model`: `started`, sessions whose row says they connected in the hour before `to`; `went_stale` and `went_lost`, the changes to each seen in it; `stale_s` and `lost_s`, the seconds sessions spent so, watched |
| `running` | per `role` and `model`: `peak`, the most running at any time in it, and `mean` |
| `running_roles` | the same per `role` |
| `running_agents` | the same for the running sessions, each once |
| `load_1m`, `load_15m` | the machine's load average over 1 and 15 minutes, read at each snapshot: `peak` and `mean`; null when it could not be read |

```json
{"type":"hour","schema":1,"at":1794402000.7,"hour":1794398400.0,"from":1794398400.0,"to":1794402000.0,"observed_s":3598.1,"samples":3582,
 "sessions":[{"kind":"worker","model":"opus","started":4,"went_stale":2,"went_lost":1,"stale_s":312.0,"lost_s":120.5}],
 "running":[{"role":"executor","model":"opus","peak":5,"mean":3.42},{"role":"orchestrator","model":"opus","peak":1,"mean":1.0}],
 "running_roles":[{"role":"executor","peak":5,"mean":3.42},{"role":"orchestrator","peak":1,"mean":1.0}],
 "running_agents":{"peak":6,"mean":4.42},
 "load_1m":{"peak":21.3,"mean":14.07},"load_15m":{"peak":16.2,"mean":15.1}}
```

(shown on several lines here; in the file it is one). A kind and model, or a role and
model, with nothing to count in the hour has no row. Means and seconds are to the
thousandth.

**Reading an hour.** A server that stops writes the hour so far, and the next one
writes the rest of that hour when it ends, so an hour can have more than one line.
Taken together: `observed_s`, `samples`, the changes and the seconds add up; a peak is
the largest; a mean is weighted by each line's `observed_s` (by `samples` where that is
0); and `started` is the line with the latest `to`'s, as each counts the hour from its
start. An hour no server ran at its end and none stopped in has no line.
