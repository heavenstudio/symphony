# Runs

The board's recorder (`tasks run`, [MACHINE_WORK](MACHINE_WORK.md)) keeps a line for
each run of a pipeline and for each named step of one, in day files a project keeps
for 90 days. The Runs page, its roll-ups and a send to another machine's server read
them; nothing here is on the board, and the board's schema is unchanged. The wire
type is `symphony_api::RunLine`, schema 1.

## A run and its steps

A recorded command is a **run** when `--pipeline NAME` names one and it runs inside
no recorded command, and a **step** when `--step NAME` names one and it runs inside a
run or a step: a step of that run's pipeline, its `parent` the operation it ran
inside. Any other recorded command, inside a run or outside one, is an operation of
the work folder only, and writes no line. A recorder tells the commands it runs
which operation they run inside (`TASKS_RUN_PARENT`) and, for a run or a step, its
pipeline (`SYMPHONY_RUN_PIPELINE`); it removes the pipeline for any other, so a
`--step` inside such a command is no step.

| Pipeline | What runs it in this repository |
| --- | --- |
| `verify` | the `verify` recipe; its steps `lint` (the `lint-rs` recipe, with its own step `client-lint`, the `client-lint` recipe it runs last), `client-check` (the `client-check` recipe), `size-check` (the `size-check` recipe), `docs-audit` (the docs audit), `crates-check` (the `crates-check` recipe) and `tests` (its workspace tests), each run whatever the ones before it did |
| `lint` | the `lint-rs` recipe, with its step `client-lint` (the `client-lint` recipe, which it runs last); the `client-lint` recipe by itself; or a Cargo `clippy` run by itself |
| `test` | the `test-rs` recipe, or a Cargo `test`, `nextest` or `bench` run by itself |
| `client` | the `client-update` or `client-check` recipe, or a client build run by itself |
| `size` | the `size-check` recipe ([CHECKS](CHECKS.md#size-budgets)) |
| `build` | a Cargo `build`, `check`, `doc` or `rustc` no recipe runs |
| `review-callback` | the command `[board] on_review` names, as a task moves to review |
| `landing`, `release` | a project's landing and release run, when they name it |

A project names its own pipelines and steps through `tools/record.sh`
(`--pipeline NAME`, `--step NAME`) or `tasks run`: 1 to 64 ASCII letters, digits,
`-`, `_` or `.`.

**Trigger.** What started a run: `SYMPHONY_RUN_TRIGGER` when it names one (`on_review`
for the review command, which the board sets; `release` and `land` for what a project
starts so), else the session the record names, else `owner`. A step's comes from the
same environment, so it says what started its run.

**The tree it checked.** A run checks the commit checked out where it runs (`HEAD`),
or the one `--commit SHA` names with `--branch NAME` (the review command's tip, from
the main checkout): `tree` is that commit's tree, as `git rev-parse` says, and
`dirty` whether the checkout differed from it as the command started (`git status
--porcelain`: a tracked change, or a file git does not ignore). Both are read before
the command starts, so nothing the command writes makes it dirty, for at most 0.6 s:
what git has not said by then is null, and a git still running then is killed. A run
of a named commit is clean. Those two git
calls are the run's only added cost: some 0.05 s, half a second on a loaded machine.

**A rerun.** A clean run names in `rerun_of` the newest earlier run of the same
pipeline, step and label (the recipe and its arguments, or the command line) on the
same tree that had ended by the time it was asked for, also clean, in the day file of
that day or the day before. So a verify after a red on an unchanged tree names the
red one, and each of its steps the earlier step; a `test-rs` of one package does not
repeat a `test-rs` of another, nor `client-check` a `client-update`. A recorder takes
its operation's lease before its command starts, so a run started once an earlier
one's command ended finds that one in the work folder even while its end line is not
written yet. On macOS, where the recorder is apart from the command its caller waits
for, it waits up to 5 s for each such earlier run of the same pipeline, step, label and
tree to write its end line (and for an earlier operation whose record is not written
yet to say what it is) before it reads the day files. It waits for none whose command
still runs (the process its lease or its record names): that one had not ended when
this run was asked for, so it is not the run repeated. A recorder whose caller waits
on it (Linux) needs no wait, since an earlier run's end line was written before its
caller got its status.

**CPU.** `cpu_s` is the user and system CPU of the command and of every process under
it that it reaped, so a run's includes its steps'. `cpu_partial` says whether the
recorder knows that count leaves some out:

- Where the recorder runs the command as its child (Linux, or `tasks run` in a
  caller's process), `cpu_s` is what the reap reads, exactly, for every process the
  command's tree reaped. A process that left the tree while it ran (its parent ended
  first, so the system took it over: a daemon, the children of a killed browser) is
  not in that count, and the recorder does not watch for one: `cpu_partial` is absent.
- On macOS `tasks run` becomes its command and its recorder is not the command's
  parent: it reads the kernel's count of the command's process and of every process
  under it every 250 ms and once more as the command ends, and keeps the most it read.
  `cpu_partial` is `true` when a reading knew it left some out: a process under the
  command it was not allowed to read (a set-user-ID program's); one process with 512
  children or more, or more than 4,096 processes in all, of which it reads that many;
  or a process it read under the command before that still ran but no longer under it,
  whose time from then on no reading sees. It is `false` when none of those happened.
  Even then a process that started and left the tree between two readings is not seen
  at all.
- The last reading may come after the command's caller reaped it, so the count stops at
  the reading before. That alone sets `cpu_partial` only when the work since may have
  been more than a count may miss: 10 ms of CPU, or a hundredth of the count when that
  is more. The recorder reads, beside each reading, the CPU of the children the caller
  has reaped. After the reap that has grown by all the command took, and by other
  children's only more: grown by no more than the count and what it may miss, the count
  is whole. Else a command that took less than a hundredth of one CPU between its last
  two readings, and ended within twice that span of the last, is taken to have stayed
  idle. Else it may have kept every CPU busy since its last reading: `cpu_partial` is
  `true` when that is more than a count may miss.
- `cpu_s` is null when the command's own process could not be read (a set-user-ID
  command), and `cpu_partial` is then absent.

No queue wait is recorded: nothing queues for the recorder, and Cargo's own lock wait
is not counted.

## The day files

`symphony/runs/YYYY-MM-DD.jsonl` in the git common dir, so every worktree of a
repository writes where its Symphony reads: in the one `symphony/` folder of
Symphony's state there, from the start, with no old name to read. `symphony/runs.<database>/` for a board database other than the live one.
`SYMPHONY_RUNS` names another folder (a test's); a run whose work folder `TASKS_WORK`
names, or whose board `TASKS_SERVER` names, writes no line without it.
The runs' logs are in its `logs/` (below).

Each line goes to the file of its UTC day: the day it ended, else the day it
started, else the day it was asked for. A line is one JSON object and its newline,
appended in one write while the recorder holds `symphony/runs/lock`, an advisory lock.
A recorder apart from its command (macOS) waits at most 2 s for it; one whose command's
caller waits on it (Linux, a caller's process, a sweep) at most 50 ms, so a held lock
never holds up the command's status. Either then appends without it. One that finds
the file's last line cut short (a writer that died in its write) starts with a
newline, so that line stays apart; two that find it so without the lock leave an empty
line between theirs. A reader passes over a line that does not parse, an empty one
included. Then, under the lock only, each day file of a day more than 90 days before
the day of that line's write goes: a regular file named `YYYY-MM-DD.jsonl`, never a
link, a folder or another file; and each day folder of logs more than 14 days before
it, with every log in it: a folder named `logs/YYYY-MM-DD`, never a link or a file. The
rerun reader reads only a regular day file, never through a link or from a FIFO, and of
a file longer than 16 MiB its last 16 MiB.

## Logs

A run's or a step's command writes its stdout and stderr to pipes its recorder reads.
The recorder copies each read on to where that stream went before, as it came, and
into the run's log without terminal escape codes: `logs/YYYY-MM-DD/RUN.log` in the
day files' folder, the UTC day the run was asked for, its `run` the operation's id. So
the caller sees the same bytes, but the command sees no terminal: a program that
colours or draws progress only at a terminal does neither under a run. Any other
recorded command keeps the caller's streams as they are, and writes no log.

- **Plain.** The log holds what the command wrote less ECMA-48's escapes: its control
  sequences (`ESC [`, parameters, a final byte), its strings (`ESC ]`, `ESC P`,
  `ESC X`, `ESC ^`, `ESC _`, to `ESC \` or BEL; a string never ended stops at its
  line's end) and its other escapes, wherever a read cuts one. Text, UTF-8 and the
  other control characters (a carriage return among them) stay.
- **Order.** stdout and stderr go into the one log in the order the recorder read
  them. A program buffers its stdout in a pipe, so lines it wrote close together may
  come in another order than at a terminal. A step has a log of its own, and its
  output reaches its run's log too, through the step's recorder: it may come there
  after what the run wrote once the step's command ended.
- **Size.** The start line names the log (`log`), so the log of a run that still
  runs can be read. The end line names it with what it held then (`log_bytes`): once
  both streams ended, or 1 s after the command did when a process it left still holds
  one. What that process writes later is still copied on, and into the log. On macOS
  the recorder copies until both streams end; where it is its command's parent (Linux,
  a caller's process) the caller gets its status after those 1 s at most: `tasks run`
  then ends, so a process still writing finds its stream closed, while in a caller's
  process the copy goes on until the streams end.
- **A reader that went.** A stream whose caller's end can no longer be written (a
  `| head` that exited) is read no more, so the command's next write there fails as
  it would have; the other stream is still copied and logged.
- **Kept** 14 days: a day folder of logs more than 14 days old goes with the day files'
  own pruning (above).

`GET /api/run-log?run=ID` (`symphony_api::RUN_LOG_PATH`), on a project's server, reads
one: by default its last 64 KiB, `&tail=N` its last N bytes, `&from=A` (and `&to=B`) the
bytes from A (to B), never more than 1 MiB at once. It answers a `symphony_api::RunLog`:
`run`, `log`, `size` (the log's length as read), `from`, `to` and `text`, those bytes as
text. A `run` that is not 32 hex digits is refused (400); a log that is not there, or is
no regular file in a day folder of `logs/` (a link, a FIFO), is none (404). It reads the
project's day files' folder alone, never the board.

## Start and end

A run or a step writes a **start line** when its command starts (`end`, `ended`,
`exit`, `signal` and `cpu_s` null, `cpu_partial` absent), then an **end line** with the
same `run` when it ends. A reader takes each run once (`symphony_api::fold`): its
first end line, else its newest start line. A command that never started writes its
end line only (`incomplete`).

A recorder that ended before its command did leaves its run started: the next
recorder's sweep of the work folder writes its end line, `incomplete`, ended at its
last beat, in the folder the run's own record names (where its start line went),
whatever folder the sweeping recorder's own lines go to. A recorder ended between its
end line and its record's last write gets a second end line from that sweep; the first
stands. A start line with no end line is a run that still runs, or a lost one no
recorder of this kind has swept yet (a build older than these lines sweeps without
writing one): a reader that sees the work folder reads it as running while its
operation runs there, and as `incomplete` once that operation is finished or gone;
one that sees only the day files shows it as started, not as running.

## A line

| Field | Meaning |
| --- | --- |
| `schema` | `1`; a field added later is optional, read as absent by an older reader |
| `run` | 32 hex digits: the operation's ID, as its record in the work folder says it |
| `parent` | the run or step a step ran inside; null for a run |
| `project` | the project's id (its `symphony.toml`, else the machine's registry, else its main checkout's folder name) |
| `machine` | the installation ID, when the machine folder has one |
| `pipeline`, `step` | the run's pipeline, a step's too; the step's name, null for a run |
| `label` | `--label`, else the command line |
| `task`, `handle` | the task and session it ran for, as the work folder's record says, or null |
| `trigger` | what started it (above) |
| `branch`, `commit` | the branch and the commit it checked |
| `tree`, `dirty` | that commit's tree, and whether the checkout differed from it as the command started |
| `queued`, `started`, `ended` | seconds since the epoch on the recording machine's clock |
| `end` | `green`, `red`, `timed_out`, `interrupted` or `incomplete`; null on a start line |
| `exit`, `signal` | how its command ended; neither when that is not known |
| `cpu_s` | CPU seconds, to the millisecond, once it ended (above) |
| `cpu_partial` | beside `cpu_s`: `true` when known short, `false` when the recorder saw no loss, absent when it does not watch for one (above) |
| `rerun_of` | the run it repeats (above) |
| `log` | its log in the day files' folder ([Logs](#logs)), on its start line too; absent when its streams were not copied |
| `log_bytes` | on its end line, the bytes its log held then; absent from the end line a sweep writes for a recorder that died |
| `suites`, `failed` | left out until `tools/run-suites` fills them |

`end` reads its command's end: exit 0 is `green`; exit 124 (what `timeout` exits
with), `SIGALRM`, `SIGXCPU` or `SIGVTALRM` is `timed_out`; `SIGHUP`, `SIGINT`,
`SIGQUIT`, `SIGKILL`, `SIGPIPE` or `SIGTERM` is `interrupted`; an exit of 129 to 159
is read as a shell reports a command signal N ended (128 + N); any other exit or
signal is `red`; a command that never started, or whose end is not known, is
`incomplete`.

An end line of the workspace tests of a verify, a step, recorded on macOS:

```json
{"schema":1,"run":"8a21…","parent":"5f0c…","project":"symphony","machine":"9e1d…","pipeline":"verify","step":"tests","label":"tests","task":"tools/run-record","handle":"ember-osprey","trigger":"ember-osprey","branch":"tools/run-record","commit":"7ad7…","tree":"c41b…","dirty":false,"queued":1794400090.1,"started":1794400090.2,"ended":1794400412.9,"end":"red","exit":101,"signal":null,"cpu_s":1873.412,"cpu_partial":false,"rerun_of":"77e0…","log":"logs/2026-11-11/8a21….log","log_bytes":48211}
```

## The review command

A task moving to review starts `[board] on_review` through the `tasks` that moved it
(a test's `TASKS_BIN` on a test's board): a run of `review-callback`, trigger
`on_review`, for that task, on its branch's tip (`--commit`, `--branch`), with no
session and outside any recorded command the mover ran in. A program that cannot start
starts nothing, recorded or not, and the move says it did not start, as it says why.
Its line keeps the command's exit; its output still goes to `symphony/tasks/on-review.log`,
and to its log ([Logs](#logs)).
With no recorder it runs unrecorded.
