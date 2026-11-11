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
pipeline (`SYMPHONY_RUN_PIPELINE`); it removes the pipeline for any other.

| Pipeline | What runs it |
| --- | --- |
| `verify` | `just verify`; its steps `lint`, `client-check` and `tests` |
| `lint` | `just lint-rs`, or a Cargo `clippy` run by itself |
| `test` | `just test-rs`, or a Cargo `test`, `nextest` or `bench` run by itself |
| `client` | `just client-update` or `just client-check`, or a client build run by itself |
| `build` | a Cargo `build`, `check`, `doc` or `rustc` no recipe runs |
| `review-callback` | the command `[board] on_review` names, as a task moves to review |
| `landing`, `nightly` | a project's landing and nightly, when they name it |

A project names its own pipelines and steps through `tools/record.sh`
(`--pipeline NAME`, `--step NAME`) or `tasks run`: 1 to 64 ASCII letters, digits,
`-`, `_` or `.`.

**Trigger.** What started a run: `SYMPHONY_RUN_TRIGGER` when it names one (`on_review`
for the review command, which the board sets; `nightly` and `land` for what a project
starts so), else the session it ran for (`--as`, else `TASKS_SESSION`), else `owner`.
A step's comes from the same environment, so it says what started its run.

**The tree it checked.** A run checks the commit checked out where it runs (`HEAD`),
or the one `--commit SHA` names with `--branch NAME` (the review command's tip, from
the main checkout): `tree` is that commit's tree, as `git rev-parse` says, and
`dirty` whether the checkout differed from it when the run began (`git status
--porcelain`: a tracked change, or a file git does not ignore). A run of a named
commit is clean. Those two git calls are the run's only added cost: some 0.05 s, half
a second on a loaded machine.

**A rerun.** A clean run names in `rerun_of` the newest earlier run of the same
pipeline and step on the same tree that had ended by the time it was asked for, also
clean, in the day file of that day or the day before. So a verify after a red on an
unchanged tree names the red one, and each of its steps the earlier step.

**CPU.** `cpu_s` is the user and system CPU of the command and of every process under
it that it reaped, so a run's includes its steps'. Where the recorder runs the
command as its child (Linux, or `tasks run` in a caller's process), that is what the
reap reads, exactly. On macOS `tasks run` becomes its command and its recorder is not
the command's parent: it reads the kernel's count of the command's process and of
every process under it every 250 ms and once more as the command ends, and keeps the
most it read. The command's caller can reap it before that last reading: the count
is then short of what ran after the reading before, at most a quarter second of the
work then running. No queue wait is recorded: nothing queues for the recorder, and
Cargo's own lock wait is not counted (DECISIONS: reports-small-choices).

## The day files

`symphony/runs/YYYY-MM-DD.jsonl` in the git common dir, so every worktree of a
repository writes where its Symphony reads: in the one `symphony/` folder of
Symphony's state there, from the start (DECISIONS: symphony-own-names; no old name to
read). `symphony/runs.<database>/` for a board database other than the live one.
`SYMPHONY_RUNS` names another folder (a test's); a run whose work folder `TASKS_WORK`
names, or whose board `TASKS_SERVER` names, writes no line without it.
`symphony/runs/logs/` is not the recorder's (`tools/run-logs`).

Each line goes to the file of its UTC day: the day it ended, else the day it
started, else the day it was asked for. A line is one JSON object and its newline,
appended in one write while the recorder holds `symphony/runs/lock` (an advisory lock it
waits at most 2 s for; it then appends without it). One that finds the file's last
line cut short (a writer that died in its write) starts with a newline, so that line
stays apart and a reader passes over it. Then, under the lock, each day file of a day
more than 90 days before the day of that line's write goes: a regular file named
`YYYY-MM-DD.jsonl`, never a link, a folder or another file.

## Start and end

A run or a step writes a **start line** when its command starts (`end`, `ended`,
`exit`, `signal` and `cpu_s` null), then an **end line** with the same `run` when it
ends. A reader takes each run once (`symphony_api::fold`): its first end line, else
its newest start line. A command that never started writes its end line only
(`incomplete`). A recorder that ended before its command did leaves its run started:
the next recorder's sweep of the work folder writes its end line, `incomplete`, ended
at its last beat. A recorder ended between its end line and its record's last write
gets a second end line from that sweep; the first stands.

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
| `tree`, `dirty` | that commit's tree, and whether the checkout differed from it |
| `queued`, `started`, `ended` | seconds since the epoch on the recording machine's clock |
| `end` | `green`, `red`, `timed_out`, `interrupted` or `incomplete`; null on a start line |
| `exit`, `signal` | how its command ended; neither when that is not known |
| `cpu_s` | CPU seconds, to the millisecond, once it ended (above) |
| `rerun_of` | the run it repeats (above) |
| `log`, `log_bytes`, `suites`, `failed` | left out until `tools/run-logs` and `tools/run-suites` fill them |

`end` reads its command's end: exit 0 is `green`; exit 124 (what `timeout` exits
with), `SIGALRM`, `SIGXCPU` or `SIGVTALRM` is `timed_out`; `SIGHUP`, `SIGINT`,
`SIGQUIT`, `SIGKILL`, `SIGPIPE` or `SIGTERM` is `interrupted`; an exit of 129 to 159
is read as a shell reports a command signal N ended (128 + N); any other exit or
signal is `red`; a command that never started, or whose end is not known, is
`incomplete`.

```json
{"schema":1,"run":"5f0c…","parent":null,"project":"symphony","machine":"9e1d…","pipeline":"verify","step":null,"label":"just verify","task":"tools/run-record","handle":"ember-osprey","trigger":"ember-osprey","branch":"tools/run-record","commit":"7ad7…","tree":"c41b…","dirty":false,"queued":1794400000.1,"started":1794400000.2,"ended":1794400412.9,"end":"red","exit":101,"signal":null,"cpu_s":1873.412,"rerun_of":null}
```

## The review command

A task moving to review starts `[board] on_review` through the `tasks` that moved it
(a test's `TASKS_BIN` on a test's board): a run of `review-callback`, trigger
`on_review`, for that task, on its branch's tip (`--commit`, `--branch`), with no
session and outside any recorded command the mover ran in. Its line keeps the
command's exit; its output still goes to `sf-tasks/on-review.log`. With no recorder it
runs unrecorded.
