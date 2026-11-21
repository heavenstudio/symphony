# Machine build and test work

Task cards, task detail and agents share build/test phase labels. Queued means an
actual resource wait for known build/test work; running requires a successful
native spawn. Coding, review, setup, generic commands and slot reservations do
not become build/test execution. A resource phase does not change a task's lifecycle.

The machine panel shows each observed operation and its machine. Explicit native
completion retires that operation only. Missing, unreadable, abandoned or expired
evidence retains last-known work and uncertainty. Local native inventories are
always partial: even an empty one has **unknown occupancy**, never free capacity.
An expired phase is labelled stale. A broken browser feed marks retained evidence
disconnected. Producer phase timestamps are display information, never freshness.

The task pipeline places in-progress work in **Build** or **Test** when connected,
current evidence identifies its queued or running operation. Positive evidence in a
partial inventory can place a task while occupancy stays unknown. Running work wins
over queued work, then Test over Build. Expired, conflicting or disconnected evidence
keeps the task in its lifecycle column with the last observed phase chip. A run for a
subtask is its parents' work too: the Symphony reads from the board which tasks its
task is under (`under` on the operation; `task` stays the recorder's), so the card
drawn for the feature moves and carries the chip. Only in-progress work moves: a reviewer's run on a task in review is a chip on its card in
**review**, which stays where the verdict is awaited. Until any machine reports this
project's work (nothing recorded, no project reader), the Build and Test columns are drawn
empty, each saying that no observations are reported; with machines reporting and no task
in one, it says no task has fresh activity there.

Open **Advisory targets** to find a machine's installation ID. Configure optional
targets in the project's `symphony.toml`, using that ID as the table key:

```toml
[board.machine_wip."replace-with-machine-id"]
build = 2
test = 1
```

Each target applies to that machine alone. Missing configuration means no
recommendation; unavailable targets display as unknown, never zero. An explicit
zero is also advice. Orchestrators decide when to start work. These values do not change native
slot admission, Cargo room/lock controls, cancellation or verification certificates.

One opaque ID is persisted as `machine-id` in the existing Symphony machine folder
(`SYMPHONY_MACHINE_DIR` in private fixtures). Hostname is only a display label.
An unreadable or malformed existing ID leaves identity unknown and is not replaced.
Task attribution comes from the recorder's local context. In order, that is the task
or session named, the live session the beat hook placed in its worktree, the
checkout's branch, then the live session its Claude Code session binds
([For which task](#recording-builds-and-tests)). It never picks between several
candidates, and it is bound once for an operation: later role changes do not
reassign it.

Each server scans its local repository's native records once per existing board
tick; HTTP pages and the feed share the cached result. It reads the board recorder's
work folder (below) directly, and beside it a project's own reader when there is
one: the already compiled executable its `symphony.toml` names (`[board]
machine_work_reader`, a project-relative path; none when unset), or the absolute
`SYMPHONY_MACHINE_WORK_READER` a direct host process supplies. Both are one
inventory of one machine. The project's own tool commands prepare the executable
it names. The selected path is retained; sampling never
searches caches or invokes a build or source hash. A named reader must answer; the
prepared path may hold none. An unsupported reader, one with no answer within 2 s
(it is ended then, so a reader that hangs never holds the tick), an invalid
response or an unreadable record leaves observations unknown. A machine is first
claimed (its ID made) once something was recorded or a project reader answered.

## Recording builds and tests

A project without a recorder of its own records its builds and tests with the
board's:

```sh
tasks run --phase build|test [--label TEXT] [--task SLUG] [--as HANDLE]
          [--pipeline NAME] [--step NAME] [--commit SHA] [--branch NAME] -- COMMAND [ARGS...]
```

It is the board's binary that runs it (`.target/bin/tasks run`, as `tools/record.sh` does for
the recipes and the release run, and the board's own `tasks` for a review callback), never
`just tasks run`: the `tasks` recipe `symphony init`
installs, and this repository's own, refuse `run` as `tasks` reads it, first after any
`--database NAME`, and run nothing. An agent runs `just tasks` unasked under the allow rules,
and the recorder runs whatever command it is handed. `just tasks help run` still prints its
help.

COMMAND runs with the caller's arguments (any bytes), streams, environment, working
directory and limits, and its status is the caller's. On macOS `tasks run` becomes
COMMAND (`execvp`): the process the caller started, waits for and signals is
COMMAND's, so every descriptor (a jobserver's included), the signal mask and the
signals the caller ignores reach it as they would without the recorder, and whatever
ends that process, `SIGKILL` included, ends COMMAND; only `SIGPIPE` starts at its
default, as a shell's command does. Before it does, it forks its recorder, detached in
a session of its own with none of the caller's streams open and out of its folder,
at the root, which watches that
process through a kqueue (its exec, its exit and its status) and writes the record;
COMMAND waits at most 1 s to hear that the recorder watches it, and runs unrecorded
otherwise. Elsewhere `tasks run` runs COMMAND as its child and waits for it: a
hang-up, interrupt, quit or terminate signal it receives, unless the caller ignores
it, reaches COMMAND, and it ends by the signal that ended COMMAND; there a `SIGKILL`
of `tasks run` leaves COMMAND running, and the signal mask is not passed on.

That recorder outlives COMMAND for as long as the record takes to write, the board's
answer included, and is never a process working in COMMAND's checkout: it writes nothing
there, and names what it reads there by COMMAND's working directory, as it names a
`TASKS_WORK` that is no whole path. So what asks which processes work in a checkout (the
release run's look after each check, which stops a run on one
([RELEASES](RELEASES.md)); the worktree sweep; a land) counts COMMAND and what it started,
and no recorder, whatever the machine's load makes of the time it takes.

It prints nothing of its own except for a command line it cannot read (status 2,
with its usage) or a command that cannot start (127 or 126, as a shell says them).
A record it cannot write leaves COMMAND running unrecorded. `tasks run --probe`
prints each version of the command line it takes, a line each: `run 1`, then
`run 2`, which adds `--pipeline`, `--step`, `--commit` and `--branch`. A wrapper asks
first, since an older `tasks` lacks the command or the options; one that reads only
the first line takes the first version.

A recorded command that is a run of a pipeline, or a step of one, also writes a line
to the project's day files when it starts and when it ends ([RUNS](RUNS.md)):
`--pipeline` names a run's, `--step` a step's inside a run. `--commit` and `--branch`
name the commit it checks when that is not its checkout's (a review command's tip).
Its log is kept by what the caller's stdout and stderr are ([RUNS](RUNS.md#logs)). At a
terminal COMMAND keeps the caller's streams and the run keeps no log. Where either is a
regular file (an agent's tool call; each step of the release run, into the log file
`tools/verify.sh` keeps of it; the board's `on_review` and `on_land` commands) COMMAND
keeps them as well, and its recorder copies what COMMAND added to the files, plain, into
the run's log once it ended. Otherwise they are the one exception to the descriptors
above: each is a pipe its recorder reads, copying what comes on to the caller's stream
and, plain, into the run's log; a recorder that dies while COMMAND runs leaves it writing
to a pipe with no reader, so its next write ends it by SIGPIPE (exit 141).

**Where.** The work folder is `symphony/tasks/work/` in the git common dir of the working
directory, so every worktree of a repository records where its Symphony reads;
`work.<database>/` for a board database other than the live one. `TASKS_WORK` names
another folder. A board named by `TASKS_SERVER` without `TASKS_WORK` (a test's board)
records nothing.

**A test's waits.** `TASKS_RUN_WAIT_SCALE=N`, a whole number from 1 to 60, makes each
wait the machine's speed meets N times as long: the 1 s COMMAND waits to hear its
recorder watches it, the 1 s the board is asked its task (below), the 0.6 s git is
asked a run's tree, the 1 s a run's recorder waits for its command's piped streams to end
before its end line says what its log holds, and, for a recorder apart from its
command, the 5 s it waits for an earlier run it may repeat to end and the 2 s it waits
for the day files' lock ([RUNS](RUNS.md)). So a loaded machine never decides what a test's run records. It is taken only where the board the run reaches is on a database other
than the live board's `tasks` (a test's `TASKS_DATABASE`, or `--database NAME`), which the
board never takes for a live one either. A server `TASKS_SERVER` names is reached whether
or not it answers, so a database of a test's own there takes it, on one that refuses
(`127.0.0.1:1`) too. A run on the `tasks` database, by whatever server
or folder it reaches it and whether or not that server is up, or one that reaches no
board (as with no server named and none recorded for its repository), keeps the bounds as
they are, whatever it names. `tools/record.sh` waits `TASKS_RECORDER_PROBE_WAIT` seconds for
its recorder's probe instead of 2 (below): a test's, taken wherever it is named, since the
probe reaches no board and a live board's waits are the recorder's own.
`TASKS_RUN_END_MARK=FILE` makes the
recorder create FILE (a relative name from the run's folder) once it has stamped its command's
end, whether or not the board has answered which task the run is for: a test's mark of that
moment, so what it does next comes after the stamp whatever a loaded machine runs first. It is
made only where no file is, and a run that names none makes none.

What each test that waits on a record takes: the board's recorder tests
(`crates/tasks/tests/cli/recorder.rs`) take the scale 20 on boards of their own, and 60, the
largest, in the three that ask a board that never answers (`ASKED`) and the one of a held lock
(`LOCK_SCALE`); the script's (`record_script.rs`) take 20 on one that refuses, with a probe's
wait of 120 s but in the case of that deadline. The server's test of what its machine panel
reads takes the scale 20 on its board's scratch database
(`apps/symphony-server/src/live/machine_work/tests`). The release run's test
(`crates/symphony-build/tests/release/recorder.rs`) and the browser test of the board's Build and
Test columns (`apps/symphony-web-tests/tests/browser/run_records.rs`) take both, the scale and the
probe's 120 s, on their scratch boards, which every check and run of theirs inherits. Each
waits for a record on its condition, with a backstop on the wall only a stall reaches (the
browser test's are the Chrome driver's waits, which keep the same rule). What the scale and
the long probe would hide, a recorder grown slow, two checks turn red at half the real bound:
the quickest of several runs of a recorder test started within 0.5 s of being asked for
(`one_started_promptly`, which two recorder tests check), and a probe of the real recorder
answers within 1 s, probed until one does
(`record_script::the_recorder_answers_its_probe_well_within_the_scripts_own_wait`).

**What.** Each operation is `operations/<id>.json` in the work folder, rewritten
whole (a file of its own renamed over it): first once the board said which task it is
for, queued, or running when COMMAND started by then; running once COMMAND started,
rewritten every 2 s while it runs (its beat, with its latest `reading` on macOS), and
finished with how it ended. Every
recorder, of any build, locks `operations/<id>.lease` before it writes its record and
holds it until it finished: the sweep (below) relies on that across builds. This one
takes it before COMMAND starts, waiting at most 50 ms, and on macOS writes in it the
process COMMAND is (`PID` and a newline), so a run its caller starts once COMMAND ended
finds it in flight, and one started while COMMAND runs sees that it still runs
([RUNS](RUNS.md)).
Its times are stamped as they happen: queued when `tasks run` was called, started
when COMMAND did (on macOS, when the recorder told it to exec), ended when its end
came, each seen by a thread of the recorder's own while the board is asked which task
it is for; the record is written once that is known. The board's schema is unchanged.

| Field | Meaning |
| --- | --- |
| `version` | `1`, and still `1` for a field added later: a reader ignores a field it does not know, and takes a record of another version for one it cannot read |
| `operation_id` | 32 hex digits, also the file name |
| `parent_id` | the recorded operation that started this recorder, if any |
| `phase`, `label` | `build` or `test`; `--label`, else the command line |
| `state` | `queued`, `running` or `finished` |
| `handle`, `task` | the session and task it ran for, or null |
| `checkout`, `branch`, `commit` | the working directory, its branch and `HEAD`, or the `--branch` and `--commit` named |
| `machine_id`, `host` | the installation ID if one exists (never made here), the host name |
| `pid`, `child_pid` | the recorder and COMMAND |
| `queued_at`, `started_at`, `ended_at`, `beat_at` | seconds since the epoch |
| `exit_code`, `signal` | how COMMAND ended; neither when that is not known |
| `reason` | why it ended without a status, or never started, else empty |
| `run` | for a run or a step only, else left out: its `pipeline`, `step`, `trigger`, `project`, `tree`, `dirty`, `rerun_of` and, once finished, `cpu_s` and `cpu_partial`, as its line in the day files says them ([RUNS](RUNS.md)), `runs`, the absolute path of the day files' folder its lines go to, and, once finished, `refused`, the line its runner refused its command line with, left out of a run that built or ran something ([below](#a-command-its-runner-refused)) |
| `reading` | on macOS, where the recorder watches COMMAND's processes, the latest reading of them it wrote with a beat, else left out: `at`, when it read them (seconds since the epoch), `cpu_s`, the most CPU seconds any reading so far found they had taken, `partial`, whether a reading so far knew it missed some (as `cpu_partial` says), and `processes`, how many it read then, COMMAND's and each under it; a finished record keeps the last |

**For which task.** The first of these that answers. Unless a task is named, the
board is read once for it, without starting its server, for at most 1 s, after
COMMAND started; no time the recorder stamps waits for that answer, and the read
writes nothing.

1. **Named.** `--task SLUG`, else `TASKS_TASK`. Else the session given (`--as`, else
   `TASKS_SESSION`), and the task it holds.
2. **Placed.** The session the beat hook (`symphony hook tasks-beat`) placed here,
   and the task it holds. The hook binds a Claude Code session's tool calls to the
   handle a `just tasks … --as HANDLE` names, and places a subagent in the checkout
   its last tool call ran in. That session is the one live session whose subagent's
   last tool call ran in this linked worktree. Two live there give no session. The
   main checkout places no one: every agent runs its board commands there.
3. **Branch.** The checkout's branch, when it is the slug of a task, whatever its
   status. An executor's worktree is on its task's branch, and stays its task's once
   an approve left the task done: an executor holds no done task, so what it runs
   there after a land's conflict (main merged, its tests run at the merged tip) is
   credited by the branch, with no task to name.
4. **Claude Code session.** The live full session bound by the Claude Code session
   the command runs in (`CLAUDE_CODE_SESSION_ID`), and the task it holds. A subagent
   shares its parent's id, so a subagent not yet placed would be credited to its
   parent: the branch wins over a task found only this way.
5. **None.** No task is recorded.

A session counts only while the board reads it live (its heartbeat recent, or in a
tool call that may still run; not stale, lost or left), at the clock the board's
commands read. A `/work` or `/review-task` agent never runs `just tasks leave`, so a
finished one's binding stays; it stops counting once it goes quiet, wherever it last ran. A
session holds a task when it executes it in concept, design, ready or in progress,
or reviews it in review. Of several, it holds the one all the others are under (a
feature's executor that also holds a subtask of it, a slice or an instruction, holds the
feature). None, or several with no
one above the rest, is no task.

Each step wins over the ones after it: a named task or session over a placed one, a
placed one over the branch, the branch over the Claude Code session's. A session
named or placed stays the record's handle when the branch gives its task; one found
through the Claude Code session is the record's handle only when its task is. A
reviewer in a detached checkout has no branch: its runs are its task's while the
hook places it, live, in that checkout, else no task's. An orchestrator running a
build or test for a task from another checkout names the task (`TASKS_TASK=SLUG`
before the command); its own unnamed build in the main checkout is no task's only
while it holds none (one that claimed its own fast fix is that task's), and a
subagent working in the main checkout, where no branch wins and it shares its
parent's Claude Code session, names itself with `TASKS_SESSION`. A board that
does not answer records the session given and no task: nothing is guessed. The task
a record names is the recorder's binding; the Symphony never rewrites it.

### A command its runner refused

A command line its runner refuses before it reads the tree (test names typed after the first
with no `--`, an option it does not have, a recipe the justfile does not hold) builds and runs
nothing, so it says nothing about the commit it was typed at. As a run or a step ends red (an
exit that is no timeout's and no signal's), its recorder reads its log once, as it reads its tests
([RUNS](RUNS.md#tests)), and keeps the refusal's `error:` line, 240 characters at most, as
`refused` in the run's record (`crates/tasks/src/runs/refused.rs`). The readers of the recorded
runs at a tip, the `tests` stamp's, the land's, the task page's and `just tasks runs`, then pass
it over: it is no run of its command there, neither a red nor a green
([TASKS](TASKS.md#the-default-landing)).

It is told by the runner's own words, never by how long the run took or by its exit code: a test
that fails at once is a red. The log, 64 KiB at most, ends in one of:

- clap's usage error, as `cargo test`, `cargo build`, `cargo clippy`, `cargo nextest run` and
  `just` say it: `error: …`, any `  tip: …` lines, `Usage: …`,
  `For more information, try '--help'.`;
- just's `error: justfile does not contain recipe …`, alone or with its `Did you mean …?`;
- just's `error: recipe … got N arguments but takes M`, with its `usage:` and the recipe's line;

and no line of it says work was done: a status line of Cargo's or nextest's (`Compiling`,
`Finished`, `Running`, `FAIL`, a word ending at the twelfth column), or libtest's
`running N tests` or `test result:`. So a compile error, a failing test, a test binary given an
option it does not take (it was built first) and a command that said such a block and went on are
reds. So is a run that keeps no log (its caller at a terminal), one a sweep finished for a
recorder that died, and a refusal that reads the tree (Cargo's `no test target named`, a package
it does not find). A command that did its work in words none of these lines knows, then ended on
a refused runner's block, reads as refused: the one case the log cannot tell.

The run's line in the day files keeps the `end` its exit says, `red`
([RUNS](RUNS.md#a-line)): the Runs page and the roll-up count it as a red run.

**Nested runs.** COMMAND receives `TASKS_RUN_PARENT`, its operation's ID; a recorder
it starts records that as its `parent_id`. A run's or a step's COMMAND also receives
`SYMPHONY_RUN_PIPELINE`, its pipeline, which a recorder it starts with `--step` records
a step of; any other COMMAND has it removed.

**Kept and bounded.** A finished operation is appended to `history.jsonl` in the
work folder, one record per line in one write that waits for no other recorder,
before its record says it finished. A history that reaches 2 MiB becomes
`history.1.jsonl`, replacing the one before, so the two files hold some 3,000 to
6,000 operations, about 4 MiB; a reader takes each operation once, oldest first. Its
record stays in `operations/` for an hour after it last changed, so the Symphony sees
it finish, and the next recorder's sweep then removes it, as it removes anything else
there an hour old that no live recorder holds: a write's own file cut short, a lease
with no record. A record no live recorder holds that the sweeping build reads whole
and cannot take for one (too long, malformed, or of another version), which leaves
its machine unknown, goes at the next sweep, with its lease; one it could not open or
read is left for a later sweep. So an older build's sweep removes a newer version's
record as soon as its recorder lets it go: a finished one's history line stays, and a
lost one has none. Work whose recorder died (an unfinished record whose lease nobody
holds) reads as uncertainty until the next recorder's sweep finishes it without an
exit code or signal, its end at its last beat, its reason "its recorder ended before
saying how its command did", keeps it in the history, and, for a run or a step,
writes its end line in the day files its record's `run` names, where its start line
went, whatever folder the sweeping recorder's own lines go to. One sweep or rotation
runs at a time (`history.lock`); none waits for another.

**As the Symphony reads it.** A queued or running operation counts while its
recorder holds the lease; a finished one retires it. Each beat changes what the
operation says (how long after it started its recorder last said it runs), so long
work stays current while its recorder beats, and goes stale ten seconds after its
beats stop. `tasks run` waits for no resource: queued lasts only until COMMAND
starts. A finished operation of the recorder's, once swept from `operations/`, can
never return, so the Symphony forgets its ID: a long-lived server's bound on the
operation IDs it remembers per machine is spent on live work and a project reader's,
never on the recorder's history.

**In this repository.** `tools/cargo.sh` sends a Cargo build (`build`, `check`,
`clippy`, `doc`, `rustc`) or test (`test`, `nextest`,
`bench`) through `tools/record.sh`, which it sources, as a run of `build`, `lint`
or `test`, and the `lint-rs`, `test-rs`, `client-check`, `client-update`,
`size-check` and `verify` recipes run through it as one run each, of `lint`, `test`,
`client`, `client`, `size` and `verify`. So do the docs checks a doc's evidence names
([CHECKS](CHECKS.md#evidence-by-kind-of-change)), `just symphony-cli docs-audit --check` and
`just symphony-cli status --lint`, each a test run of `docs` labelled as typed, so the task
page's check gate reads the docs audit, the project's `[verdicts] check`, from the record;
every other `just symphony-cli` command, either check typed with other words among them, runs
unrecorded. Inside a recorded run only a step a recipe names
is recorded again: `verify` records `lint-rs` as its step `lint`, `client-check` as
`client-check`, `size-check` as `size-check`, the docs audit as `docs-audit`, `crates-check` as
`crates-check` and its workspace tests as `tests`, each whatever the ones before it did
(`tools/verify.sh`); every other command inside it runs as it is (`tools/record.sh [--pipeline NAME] [--step NAME]
build|test LABEL COMMAND...`). A tree without `tools/record.sh` runs them as they are.
Its recorder is `TASKS_RECORDER` when it names one, else this checkout's prepared
`.target/bin/tasks`, else that of the main checkout a worktree belongs to
(`tools/rs-bin.sh --link tasks` prepares it); finding it starts no process. With
`TASKS_RECORDER=off`, or no recorder that answers the probe within 2 s (none built, an
older one, or a hung or slow one, as the first run of a binary linked a moment ago can
be; a test's `TASKS_RECORDER_PROBE_WAIT` names other seconds), the command is exec'd as it is; a recorder that answers `run 1` alone records it
with no pipeline at top level, and no step inside a run. The freshness build before
`just tasks` or `just symphony` is not recorded.

Configuring another machine's ID supplies advice, not telemetry. An unobserved
machine stays unknown. This feature adds no remote worker, reporting transport,
board persistence or scheduler.
