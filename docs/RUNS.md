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

A run's or a step's log is what its command wrote on its stdout and stderr, without
terminal escape codes: `logs/YYYY-MM-DD/RUN.log` in the day files' folder, the UTC day
the run was asked for, its `run` the operation's id. How its recorder reads it follows
what the caller handed the command, and the caller sees the same bytes whichever it is:

- **A terminal** (a person at it), on stdout or stderr: the command keeps the caller's
  streams as they are, its colours and progress unchanged, and the run writes no log,
  its lines naming none, as any other recorded command does.
- **A regular file**: an agent's tool call, which hands its command files; each step
  of the release run, which `tools/verify.sh` runs as `"$@" >"$log" 2>&1` into a log
  file of its own that it keeps; the board's `[board] on_review` and `on_land`
  commands, into theirs. The command keeps the caller's streams here too, nothing
  between it and its file, so its last bytes are there before its status reaches the
  caller, and a recorder that dies costs the record and the log alone. The recorder
  marks where each file ends as the run starts, and once the command ended copies what
  each gained past its mark into the log: a file both streams name (`>f 2>&1`) once,
  two files one after the other, a stream that is no file not at all. Until then the
  log is empty, the caller's file being where a running run is read. What anything
  else wrote to the file meanwhile is in the log too, and on macOS, where the recorder
  reads the file as soon as it sees the command end, so may what the caller wrote at
  once after it be. A file its caller cut below its mark gives nothing.
  Runs that append to one file at once include each other's lines in their logs.
- **Anything else** (a pipe, a socket, `/dev/null`): the command writes to pipes its
  recorder reads, which copies each read on to where that stream went before, as it
  came, and into the log. A recorder that dies while its command runs leaves those
  pipes with no reader: the command's next write ends it by SIGPIPE (exit 141), as a
  pipeline's command whose reader went.

Of a log kept either way:

- **Plain.** The log holds what the command wrote less ECMA-48's escapes: its control
  sequences (`ESC [`, parameters, a final byte), its strings (`ESC ]`, `ESC P`,
  `ESC X`, `ESC ^`, `ESC _`, to `ESC \` or BEL; a string never ended stops at its
  line's end) and its other escapes, wherever a read cuts one. Text, UTF-8 and the
  other control characters (a carriage return among them) stay.
- **Order.** From one file, the order the command wrote it; from two, stdout's then
  stderr's. From pipes, stdout and stderr go into the one log in the order the
  recorder read them: a program buffers its stdout in a pipe, so lines it wrote close
  together may come in another order than at a terminal. A step has a log of its own,
  and its output reaches its run's log too: from a file where the step wrote it, from
  pipes through the step's recorder, so it may come after what the run wrote once the
  step's command ended.
- **Size.** The start line names the log (`log`). The end line names it with what it
  held then (`log_bytes`): from files once the copy is done, so what a process the
  command left writes later stays in the caller's file alone; from pipes once both
  streams ended, or 1 s after the command did when a process it left still holds one.
  What that process writes later is still copied on, and into the log. On macOS the
  recorder copies until both streams end; where it is its command's parent (Linux, a
  caller's process) the caller gets its status after those 1 s at most: `tasks run`
  then ends, so a process still writing finds its stream closed, while in a caller's
  process the copy goes on until the streams end.
- **A reader that went.** A pipe whose caller's end can no longer be written (a
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

## Tests

A run's or a step's end line says what its log says of its tests, read once as the
line is written (`crates/tasks/src/runs/suites.rs`), when its log holds what
`log_bytes` says. Stable libtest writes no machine-readable report (`--format json`
is nightly's), so the recorder reads cargo's and libtest's words:

- `     Running unittests src/lib.rs (…/deps/NAME-HASH)`, `     Running tests/a.rs
  (…)` and `   Doc-tests NAME` start a test binary. Its name in `suites` is those words
  with the executable's folder and hash cut: `unittests src/lib.rs (symphony_board)`,
  `tests/cli.rs (cli)`, `Doc-tests symphony_board`, the same whichever build or
  checkout ran it. Two binaries of one name (two packages' `tests/links.rs`) are one
  suite, their counts and times added.
- `test result: ok. 8 passed; 0 failed; 1 ignored; …; finished in 0.41s` gives its
  `passed`, `failed`, `ignored` and `s`, added up where it says several (merged
  doc-tests say two); doc-tests' `s` is what `all doctests ran in` says, their build
  with them. A sum stays at its most (a count at 4294967295, a time at the largest
  finite seconds), so the line always reads back.
- `failures:`, once the binary said a test failed (`test NAME ... FAILED`), starts
  libtest's report of the failed tests, each one's output under
  `---- NAME stdout ----`, then their names under `failures:` again. Each named is in
  `failed` with its binary (`suite`) and `line`: the first line of what its panic said
  (`panicked at FILE:LINE:COL` when it said nothing), else the first its output holds,
  else empty, 240 characters at most. Passing tests are counted, never listed.

A binary with no result line before the next one starts or the log ends, one that
crashed or whose run was stopped, has `"error": true`: never a pass, its counts those
of the results it said. What a failed test printed starts and ends no binary, and a
report it printed ends none: the list that ends the real one names the test whose
output comes first. Anything else in the log is passed over, and the command's output
is never changed. A log that names no test binary (a lint's, a build's) leaves both
fields out, and so do a run with no log (its caller at a terminal), a start line and
the end line a sweep writes for a recorder that died.

The order is the log's ([Logs](#logs)). From one file, as an agent's tool call and the
release run's steps hand it, it is cargo's. From two files stdout's results come before
stderr's lines naming the binaries, so a log with a result before any binary started
gives neither field. From pipes a line may come late, so a binary may be named by
another's line there.
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
| `log` | its log in the day files' folder ([Logs](#logs)), on its start line too; absent when it keeps none (its caller at a terminal) |
| `log_bytes` | on its end line, the bytes its log held then; absent from the end line a sweep writes for a recorder that died |
| `suites`, `failed` | on an end line, each test binary its log names and the tests that failed ([Tests](#tests)); absent when it names none |

`end` reads its command's end: exit 0 is `green`; exit 124 (what `timeout` exits
with), `SIGALRM`, `SIGXCPU` or `SIGVTALRM` is `timed_out`; `SIGHUP`, `SIGINT`,
`SIGQUIT`, `SIGKILL`, `SIGPIPE` or `SIGTERM` is `interrupted`; an exit of 129 to 159
is read as a shell reports a command signal N ended (128 + N); any other exit or
signal is `red`; a command that never started, or whose end is not known, is
`incomplete`. A command its runner refused before it built or ran anything ends `red` here as
its exit says; that it never ran is its record's `refused` in the work folder, which the stamps'
readers go by ([MACHINE_WORK](MACHINE_WORK.md#a-command-its-runner-refused)).

An end line of the workspace tests of a verify, a step, recorded on macOS, two of its
suites shown:

```json
{"schema":1,"run":"8a21…","parent":"5f0c…","project":"symphony","machine":"9e1d…","pipeline":"verify","step":"tests","label":"tests","task":"tools/run-record","handle":"ember-osprey","trigger":"ember-osprey","branch":"tools/run-record","commit":"7ad7…","tree":"c41b…","dirty":false,"queued":1794400090.1,"started":1794400090.2,"ended":1794400412.9,"end":"red","exit":101,"signal":null,"cpu_s":1873.412,"cpu_partial":false,"rerun_of":"77e0…","log":"logs/2026-11-11/8a21….log","log_bytes":48211,"suites":[{"name":"unittests src/lib.rs (symphony_board)","passed":935,"failed":0,"ignored":0,"s":41.2},{"name":"tests/cli/main.rs (cli)","passed":642,"failed":1,"ignored":0,"s":301.52}],"failed":[{"suite":"tests/cli/main.rs (cli)","name":"recorder::a_run_says_how_it_ended","line":"assertion `left == right` failed"}]}
```

## The Runs page

`/runs` (the web client's `runs` kind, `apps/symphony-web/src/runs.tsx`) shows a
project's runs on the server that serves it; what it is for and how it behaves, with its
pictures, is [its spec](product/RUNS.md). What it reads:

- **Now:** each machine's word (its freshness and how long ago it came), then each
  operation running or queued, with its recipe, task, who started it and since when.
  This is the machine work the board reads (`/api/machine-work`), read every 5 s.
- **Schema updates:** each registered project's board reaching a new schema, from the
  machine folder's ledger a release's refresh writes, the open ones first, marked
  waiting: its board and schema, how long from reaching the schema until the machine's
  server wrote it and how much of that came after the release serving it was installed,
  when each came (the record each comes from named under them, its words on hover), and
  a failed one's refreshes, the last one's words and its task. Read every 15 s.
- **Every run,** newest first, a line each: its result, label (on one line, a long one
  cut short), task or branch and commit, who started it, how long it took and how many
  tests it failed. It can be filtered by pipeline, result (an end, or `started` for
  none), task and branch, each matched whole, and this browser keeps the filters
  chosen. Its first page is read
  every 15 s, naming the last answer's tag, and Load more reads the page from the
  last run shown on. A line with no end reads as running while an operation of the
  machine's work has its id, else as started ([Start and end](#start-and-end)).
- **A run** (`/runs?run=ID`, a step's too): its facts, its steps (and theirs) in
  order, each with its time as a bar on the run's span, the tests its lines failed
  with the line each failed on, its suites' counts (a binary that ended with no result
  line saying so), and its log. The log shows its last 64 KiB, with the output before it
  1 MiB at a time on demand; a log the server no longer keeps is said so, quietly. While
  the run or a step of it has not ended, the run is read again every 5 s and its log is
  read on from where it stopped.

Each of these reads waits while the page is hidden (another tab in front) and is made
again as soon as it shows.

`GET /api/runs` (`symphony_api::RUNS_PATH`) answers a `symphony_api::RunsPage`: up to
50 runs (`RUNS_PAGE`), steps left out, each run once as `fold` keeps it, newest asked
for first; `more` when more match; the pipelines read, for the filter; and `unread`,
each day file not read, by its name, and why. Its query names `pipeline`, `end` (an end
or `started`), `task`, `branch` and `before` (seconds: only runs asked for at or before
it, so the page after one holds its last run again and none asked for in the same
second is lost between them; the client shows each run once).
It reads the day files newest first, back 14 days (`RUNS_DAYS`) from the moment
`TASKS_NOW` holds the board's clock at when the server's environment sets it, else
from the server's clock, and stops after the day that filled the page. A day file over 16 MiB (`RUNS_FILE_BYTES`) is not read, nor
is any once 64 MiB were read for the one answer (`RUNS_READ_BYTES`); each is named in
`unread`. `GET /api/run?run=ID` (`symphony_api::RUN_PATH`) answers a
`symphony_api::RunPage`: the run, and every step under it, depth first, each after
the one it ran inside, read from the newest day back to the one the run was asked for.
A `run` that is not 32 lower-case hex digits is refused (400), and one in none of those
days is none (404). Both read only regular files, never through a link, and never the
board; a project that keeps no runs where its server reads has none (404).

`GET /api/schema-updates` (`symphony_api::SCHEMA_UPDATES_PATH`) answers a
`symphony_api::SchemaUpdates`, the same from any project's server: the ledger's updates
newest first, its 20 newest (`SCHEMA_UPDATES_SHOWN`) and every open one, each with its
times and where each comes from, its waits in the owner's words, its failures and the
page of its task on that server (under the one server while its project is registered;
on a server of its own, its own project's alone). A ledger that cannot be read, or no
machine folder, is said in `unread` in place of any update. The hosted board refuses it.

The server keeps what it folded of each day file in memory, a project's own, by the
file's length, modification time and inode: of each run only what the filters, the fold
and the steps' order read, and where its line is in the file. An answer's lines are read
again from the file, and a line no longer the run kept there has its day parsed again. A
file that only grew, on the same inode and still holding the last line read, is read on
from there, and any other change parses it again: a closed day is parsed once, today's a
line at a time as the recorder writes them, however many answers read it at once (one
waits for the other and takes what it kept). An earlier line changed in place as the file
grew, the last line read left where it was, is seen only once an answer reads it back or
the file is parsed again: the recorder only appends, so only a hand edit makes one. It
keeps the days of the 14-day window alone, and nothing of a file gone or refused
(`apps/symphony-server/src/api/runs.rs`, `Index`).

## The roll-up

Once an hour the machine's one server rolls each project's runs up into
`symphony/runs/rollup/YYYY-MM-DD.json` beside the day files, one file a UTC day,
kept for good (`symphony_api::RunsDay`, schema 1;
`crates/tasks/src/runs/rollup.rs`). This happens in the same hour as the board's day files
and on the same thread (`apps/symphony-server/src/live/reports.rs`). It reads no
board, so an hour whose board read failed rolls the runs up all the same. Today's
file is written again each hour. Yesterday's is written once more after midnight,
`closed`, and never again. A private server writes none.

A day's figures come from the runs that ended in it, each run once (`fold`):

- **Per pipeline:** its runs, how many ended each way, the pass rate (green over
  green, red and timed out), and the median and p90 of their time from start to
  end. Only runs count here, not their steps.
- **Per suite:** each test binary a line reports (`suites`): its runs, those in
  which a test of it failed or it had an error, and the median and p90 of its time. A run whose steps
  report their tests counts them by the steps alone. `slowest` names the ten
  slowest by median; a suite's trend is its line in each day's file.
- **Flaky tests,** over the day and the 13 before it: a test that failed in one
  run and passed in another of the same pipeline, step, label and tree, both
  clean, as a rerun is matched. A test passed in a run that ended green, or red
  with the test's binary in its `suites`, with no error, and the test not in its `failed`. Each
  flaky test counts its `flaky` runs (failed on a tree it passed on) and its `red`
  runs (failed on a tree it never passed on). A test failed on a changed or dirty
  tree is never flaky.
- **Cost:** the runs' CPU (`cpu_s`, steps included) and their time from start to
  end. `cpu_partial` is set when a run's CPU is known short or not known. The ten
  costliest tasks are named by CPU.

There is no queue wait, as nothing queues for the recorder. There are no landings
apart from the `landing` pipeline's own figures, from whichever project records
one. Suites and flaky tests come from the lines that carry `suites` and `failed`
([Tests](#tests)): none from a run its caller ran at a terminal.

A day file that is there but cannot be read, or is not a regular file, writes no
roll-up that hour, and each one stays as the last hour left it. A line that does
not parse is skipped. Each file is written aside and then renamed, while its server
generation's gate stands open, as the board's day files are.

## The review command

A task moving to review starts `[board] on_review` through the `tasks` that moved it
(a test's `TASKS_BIN` on a test's board): a run of `review-callback`, trigger
`on_review`, for that task, on its branch's tip (`--commit`, `--branch`), with no
session and outside any recorded command the mover ran in. A program that cannot start
starts nothing, recorded or not, and the move says it did not start, as it says why.
Its line keeps the command's exit; its output still goes to `symphony/tasks/on-review.log`,
and to its log ([Logs](#logs)).
With no recorder it runs unrecorded.
