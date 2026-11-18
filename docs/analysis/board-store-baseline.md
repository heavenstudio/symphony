# The board's store, measured before it moves

What the task board costs on SurrealDB today, written down before the store moves to PostgreSQL,
with the meter the two stores are compared by. "Measured 2026-11-15" marks this
machine that day: an Apple M3 Max (Mac15,8), 16 cores, 64 GB, macOS 26.6.2, its disk 89% full,
shared with other agents' builds and tests at a one-minute load of 111 to 149 while the first
five runs were made and 213 to 256 for the rest.

The store was chosen on a comparison made in the consuming project, on its board and with a bench
of its own that sent ported statements to five engines. Those numbers stay where they are. This
page measures something narrower and repeatable here: the board as this repository builds and
starts it, driven through the `tasks` binary, by a meter that does not know which store answers.

## The result

One copy of this repository's board (720 tasks, 6,962 messages, 719 sessions, 1,315 roles, 1,822
deliveries), on SurrealDB 3.3.0 over `surrealkv://` under the board's storage settings, and for
contrast an empty board. Each run is a fixed amount of work: every session makes its tool calls
back to back, all sessions at once.

| run | sessions × calls | commands | writes | failed | wall | writes a second | `tasks` CPU | server CPU | server memory: opened, median, peak | store grew |
|---|---|---|---|---|---|---|---|---|---|---|
| one session | 1 × 120 | 300 | 279 | 0 | 13.1 s | 21.2 | 4.6 s | 2.3 s | 72, 127, 198 MiB | 3.5 MB |
| ten | 10 × 40 | 1,000 | 928 | 0 | 12.5 s | 74.1 | 16.6 s | 8.3 s | 73, 263, 296 MiB | 3.9 MB |
| thirty, first | 30 × 40 | 3,000 | 2,787 | 0 | 24.8 s | 112.2 | 53.8 s | 30.3 s | 73, 380, 422 MiB | 11.9 MB |
| thirty, second | 30 × 40 | 3,000 | 2,787 | 0 | 24.1 s | 115.8 | 52.8 s | 26.7 s | 72, 390, 407 MiB | 16.2 MB |
| thirty, third, at twice the load | 30 × 40 | 3,000 | 2,787 | 0 | 27.9 s | 100.0 | 54.5 s | 28.1 s | 73, 412, 432 MiB | 17.3 MB |
| thirty, empty board | 30 × 40 | 3,000 | 2,787 | 0 | 11.2 s | 249.0 | 25.6 s | 12.0 s | 67, 174, 214 MiB | 4.1 MB |

The third run of thirty was made last, with the example as committed, at a load of 213 to 241.

- **Nothing failed.** No command of the 13,300 ended in an error, at any size: a write that meets
  another is retried inside its command, and none ran out of retries.
- **The server takes 8 to 10 ms of CPU a command on this board**, whatever the number of sessions
  (2.3 s for 300 commands, 8.3 s for 1,000, 26.7 to 30.3 s for 3,000), and 4 ms on an empty one.
  The three runs of thirty lie within 13% of each other.
- **The `tasks` process takes more than the server**: 15 to 18 ms a command on this board. A
  `pull` takes 0.4 to 0.5 s of its own CPU, a `list` 90 ms, a `show` 50 ms, a beat 7 ms.
- **The server opens in 72 MiB and holds what its requests took**: 198 MiB after one session's
  300 commands, 296 after ten sessions, 407 to 432 after thirty. It gave none back while it ran:
  the reading once the last command had ended is the highest of every run. The peak is the
  highest of every reading, that last one among them. Thirty sessions at this board's size stay under
  512 MiB; the run does not show where the growth stops.
- **The store grows 12 to 17 MB for 3,000 commands** on a board of 55 MB, and 4 MB on an empty
  one.

Each command with thirty sessions, the first run and the second. The CPU is the `tasks`
process's own, which the machine's load does not stretch; the walls are stretched by it.

| command | count | `tasks` CPU, mean | wall, median | wall, 99th |
|---|---|---|---|---|
| beat | 2,400 | 6.5, 6.6 ms | 89, 45 ms | 244, 229 ms |
| `inbox` | 172 | 16.4, 15.6 ms | 385, 251 ms | 979, 794 ms |
| `say` | 129 | 8.1, 7.7 ms | 130, 70 ms | 1,266, 292 ms |
| `show` | 100 | 50.0, 50.6 ms | 290, 284 ms | 655, 630 ms |
| claim, move, release | 86 | 112.2, 80.5 ms | 539, 332 ms | 3,543, 2,586 ms |
| `list` | 38 | 90.1, 93.0 ms | 294, 411 ms | 762, 789 ms |
| `next` | 38 | 31.7, 32.7 ms | 172, 173 ms | 807, 429 ms |
| `pull` | 37 | 404.4, 447.7 ms | 2,460, 2,836 ms | 3,904, 5,878 ms |
| every command | 3,000 | 17.9, 17.6 ms | 106, 54 ms | 2,283, 2,127 ms |

One session alone, the median wall of each: a beat 18 ms, a `say` 24, an `inbox` 45, a `next` 70,
a `show` 107, a claim, move or release 120, a `list` 167, a `pull` 1,310 (two of them).

The claim, move and release are the one command whose own CPU moved between the two runs (112
and 80 ms): they are the writes that meet on a task and its orchestrator's slots, and a write
that meets another runs again in its process.

### A bigger board, with recipients listening

The `perf` test target's timings, run the same day: a generated board several times this one (50,000
messages, 36,000 deliveries, 2,600 sessions, 2,200 tasks) with 100 recipients whose callbacks
listen, each command run alone three times, its dispatch made in the write. The median run of
each, the CPU of the `tasks` process and of the server for it:

| command | `tasks` CPU | server CPU | bound on their sum |
|---|---|---|---|
| `say`, dispatched to 100 recipients | 6.9 s | 2.4 s | 20 s |
| `claim`, dispatched to 100 recipients | 7.2 s | 2.7 s | 24 s |
| `inbox` | 70 ms | 136 ms | 1 s |
| the first look of `wait`, the whole digest | 8.4 s | 4.0 s | 45 s |
| a later look of `wait` | 0.7 s | 0.3 s | 3 s |
| `api tasks` | 77 ms | 36 ms | 0.5 s |
| `api sessions` | 112 ms | 30 ms | 0.5 s |
| `api inbox` | 51 ms | 98 ms | 0.5 s |
| `api get` | 68 ms | 36 ms | 0.5 s |
| `api last_messages` | 58 ms | 30 ms | 0.5 s |
| `api landings_posted` | 7 ms | 22 ms | 0.5 s |
| `api to_owner` | 7 ms | 20 ms | 0.5 s |
| `api thread` | 8 ms | 2 ms | 0.5 s |
| `api ever_executed` | 7 ms | 6 ms | 0.5 s |

The copy of that board took 19.8 s of the server's CPU. These are the release run's bounds, kept
as they are; a store that replaces this one runs the same target, whose plan checks read
SurrealDB's plans and are the store's own to replace.

## What was run

The runs of the first two tables are `crates/tasks/examples/board_load.rs`, whose module doc is
the method in full:

- **The board.** A read-only export of this repository's board (`tasks-copy --board --out`, by
  the main checkout's build), copied into a scratch board in a temporary folder. The scratch
  board's server is started by `server::Home`, the code that starts the live one, so it runs the
  pinned SurrealDB under the board's own storage settings; it is stopped and started again after
  the copy, so a run finds it as a board's server is after a restart. The export is 11,669,727
  bytes, SHA-256 `9e81247f9d0a21976a95d94db4a5b0c693e38b2ead69af5b314af703728b70f2`. It is board
  text, so the repository does not carry it: it is among the task's artifacts
  (`tools/board-postgresql/board-export-2026-11-15.json`), which the sweep sets aside in the main
  checkout's artifacts folder and nothing removes.
- **The work.** Each session makes its tool calls back to back. A tool call beats twice through
  the hook (busy before it, ended after), and every other one also runs one board command: `inbox`
  30 in a hundred, `say` 20 (280 characters to `@orchestrator`), `show` 15, its own task's claim,
  move to review, move back and release in turn 15, `list` 7, `next` 6, `pull` 7. That is the mix
  the comparison took from the live board. The commands a session runs follow from the seed
  alone, so two runs with one seed do the same work: 3,000 commands and 2,787 writes for thirty
  sessions of forty calls, on any store.
- **The commands.** Each is the `tasks` binary, a process of its own, as an agent runs it: the
  development build, which is what `just tasks` runs on this machine. Its dispatch is made in the
  write, so a write's cost ends with its command.
- **The meters.** A command's wall, and the CPU its process took with every child it reaped
  (`wait4`). The server's CPU over the run, its own process's (`proc_pid_rusage`). The server's
  resident memory, read once in every sixteen commands ended, with every process under it. The
  bytes of the board's folder before and after. The machine's load average before and after. A
  session whose beats never reached the board fails the run: a beat's exit is always 0, so the
  run reads the board for them.

The meter moves with the work and not with the load: the server's CPU follows the commands run
(2.3, 8.3, 26.7 to 30.3 s) and the board's size (12.0 s on an empty board for the same 3,000),
and the three runs of thirty, the last at twice the load of the others, agree to 3% on the
`tasks` CPU, 13% on the server's and 6% on its peak memory.

## Reading it beside another store

- **Both stores are run the same day, from one export and one build.** These figures are the
  record of what the board cost at this size on this day, not one side of a later table: by the
  time another store answers, the board has grown and the `tasks` binary has changed, and that
  binary is two thirds of the CPU measured here (53 s of 82 for thirty sessions). The comparison
  is the meter run on SurrealDB and on the other store back to back, each on a copy of one export
  taken that day, with one build of `tasks`, `--seed 1` and the sessions and calls of the table.
  The export kept from these runs replays them where a build still reads it.
- **Compare the counts and the CPU; read the walls with the load.** The failures, the server's
  CPU, the `tasks` CPU and the memory are the work, which a loaded machine does not multiply. The
  walls and the writes a second are what the work took on a machine at a load of 111 to 241 on
  16 cores: another run moves them (the median beat took 89 ms in one run of thirty and 45 in the
  next) and moves the rest little.
- **A server that answers from several processes** is counted with every process under it. Their
  resident memory counts the pages they share once each, so such a server reads higher there than
  it holds. The meter reads such a server whole: its CPU is each process's own with what the
  children it reaped took, so a backend that ended during the run is in its postmaster's, and the
  report's `server_memory_mib` beside `server_rss_mib` counts what they share once (macOS's
  `footprint` of them all, the memory they dirtied; Linux's proportional set sizes). SurrealDB's
  server is one process with no children, so its CPU and `server_rss_mib` are that process's
  alone, as in the tables above; its `footprint` is not its resident size, so the two stores
  compare by the same field.
- **Durability is whatever the store's server does as the board starts it.** This page did not
  measure what a commit's sync costs or what it survives. A store compared with these numbers
  commits at least as durably as SurrealDB does under the board's settings, and says how it was
  set to.

## The marker on PostgreSQL

The board's marker moves with every commit a reader can see. On SurrealDB it is one counter every
write moves. On PostgreSQL that row would make any two writes at once meet, so the marker there
is the sum of one row for each writer's connection, each moved once by its own write, plus the
sessions' revisions (`crates/tasks/src/store/postgres/schema.rs`). A sequence or a log's highest
id would be no marker: a commit that ends later can hold a lower number than one a reader has
already seen. A sum read in the reader's own snapshot moves for every commit that has become
visible to it.

Measured 2026-11-16 on the machine above, on a scratch PostgreSQL 18.7 initdb'd in a temporary
folder, its socket in a private folder, stopped and removed after each sitting. The runs used
pgbench, a new connection per transaction, REPEATABLE READ, and 30 clients × 100 transactions
each, with up to 50 tries and no backoff. Each transaction moves one of 720 tasks at random, then
moves the marker as each row says. The load was 6 to 20 for the first run and 23 to 27 for the
other two.

| each write's marker | retried, of 3,000 | retries | failed after 50 tries |
|---|---|---|---|
| one counter row every writer moves | 2,839 / 2,896 / 2,890 | 9,434 / 12,258 / 8,916 | 0 / 5 / 1 |
| a row of its own writer's | 58 / 58 / 57 | 59 / 58 / 57 | 0 / 0 / 0 |
| a row inserted | 60 / 65 / 64 | 61 / 68 / 65 | 0 / 0 / 0 |
| a row inserted, all thirty writers on one task | 2,845 / 2,918 / 2,886 | 17,576 / 17,054 / 12,832 | 9 / 14 / 2 |

With a row of its own, a write met another only on a task, where two of thirty on one of 720
tasks meet. The one counter row made nearly every write meet. All thirty on one task meet
whatever the marker is. The probe tried again at once, where the store's backoff (`retried`)
spaces its tries.

The probe's writes moved a task and the marker, nothing else. A board write that posts a line on
a thread (a task made, a line said, a priority set, each move) also moves the one message
counter, which numbers and dates the line (`logged` in `crates/tasks/src/store/postgres/mod.rs`,
`counter:message` on SurrealDB). That counter is one row on both databases. So the second row's
2% holds only for a write that posts no line, such as a session's beat (`Store::touch`). Two
writes that post at once meet on the message counter, as the first row's writes meet on the one
counter row. For thirty sessions posting back to back, a write that posts costs what the first
row's writes cost: nine in ten or more tried again, three or four tries each, with a few still
meeting after fifty tries at once, which the store's backoff spaces out. It stays so until a line's
number comes from something two writes do not both move. The thirty-session run of
tools/board-pg-lifecycle reads its retries for the writes that post against the first row, and
those for the writes that post none against the second.

## Paired board work, measured 2026-11-18

The two stores ran on private scratch homes on the same Mac described above, from one
November 18 read-only export and one frozen development build of `tasks` and `board_load`.
The export is 18,156,893 bytes: 931 tasks, 13,067 messages, 1,412 sessions, 1,827 roles and
4,648 deliveries. Both use seed 1 and the original command mix. This is a new comparison;
the November 15 table remains the historical SurrealDB baseline.

| case | engine | sessions × calls | commands / writes | failed | wall | tasks CPU | server CPU | store grew |
|---|---|---|---|---|---|---|---|---|
| solo | SurrealDB | 1 × 120 | 300 / 279 | 0 | 14.5 s | 4.4 s | 3.3 s | 10.6 MB |
| solo | PostgreSQL | 1 × 120 | 300 / 279 | 0 | 14.7 s | 4.7 s | 5.7 s | 0.1 MB |
| ten | SurrealDB | 10 × 40 | 1,000 / 928 | 0 | 13.2 s | 16.8 s | 10.9 s | 5.8 MB |
| ten | PostgreSQL | 10 × 40 | 1,000 / 928 | 0 | 13.9 s | 18.7 s | 20.3 s | 0.2 MB |
| thirty | SurrealDB | 30 × 40 | 3,000 / 2,787 | 0 | 13.2 s | 54.9 s | 34.5 s | 15.0 MB |
| thirty | PostgreSQL | 30 × 40 | 3,000 / 2,787 | 0 | 18.6 s | 56.8 s | 60.0 s | 1.6 MB |
| empty | SurrealDB | 30 × 40 | 3,000 / 2,787 | 0 | 9.0 s | 23.8 s | 12.8 s | 4.1 MB |
| empty | PostgreSQL | 30 × 40 | 3,000 / 2,787 | 0 | 9.3 s | 22.3 s | 31.1 s | 1.2 MB |

All 14,600 commands succeeded; every session's beats were verified in its board. The one-minute
load ranged from 75 to 168 on 16 cores. Each cell is one observation, without a confidence
interval; wall times and rates are not latency promises.

At thirty sessions on the export, PostgreSQL used 60.0 seconds of server CPU against 34.5 for
SurrealDB, and 56.8 seconds of command CPU against 54.9. It did not reduce the total CPU cost
of this workload. Folder growth includes reusable pages and preallocated WAL in the PostgreSQL
cluster; that column is not a measure of bytes written, compression or durability.

Memory compares `server_memory_mib`, counting shared pages once across the server tree,
rather than summing each PostgreSQL backend's RSS. The final column separately gives that
RSS sum's observed peak. All values are MiB; the sample count excludes the opened and
after readings, which also participate in the reported peak.

| case | engine | opened | median | peak | after | samples | RSS peak |
|---|---|---|---|---|---|---|---|
| solo | SurrealDB | 43.4 | 129.9 | 195.0 | 163.9 | 19 | 249.5 |
| solo | PostgreSQL | 51.4 | 51.8 | 52.6 | 52.6 | 19 | 95.3 |
| ten | SurrealDB | 41.5 | 265.6 | 326.6 | 297.3 | 60 | 391.1 |
| ten | PostgreSQL | 51.6 | 280.7 | 626.4 | 52.9 | 32 | 785.2 |
| thirty | SurrealDB | 41.6 | 394.5 | 447.8 | 392.4 | 103 | 518.4 |
| thirty | PostgreSQL | 51.5 | 434.6 | 1751.9 | 53.3 | 33 | 2280.5 |
| empty | SurrealDB | 33.2 | 137.3 | 173.4 | 160.7 | 72 | 221.3 |
| empty | PostgreSQL | 35.0 | 35.2 | 35.4 | 35.2 | 13 | 279.0 |

PostgreSQL's thirty-session observed peak was 1,751.9 MiB, against SurrealDB's 447.8 MiB, and
its after reading returned to 53.3 MiB while SurrealDB retained 392.4 MiB. These observations
show a larger transient footprint and a smaller final footprint in this run, not a memory cap.

The thirty-session export run, per command. Counts are identical on both engines; CPU is the
mean of the command processes, with their reaped children. Walls are median / 99th percentile.

| command | count | SurrealDB tasks CPU | PostgreSQL tasks CPU | SurrealDB wall | PostgreSQL wall |
|---|---|---|---|---|---|
| beat | 2,400 | 6.1 ms | 5.5 ms | 35.5 / 139.3 ms | 23.3 / 86.2 ms |
| inbox | 172 | 16.9 ms | 14.6 ms | 194.3 / 701.0 ms | 74.4 / 272.6 ms |
| say | 129 | 7.6 ms | 6.6 ms | 56.9 / 267.2 ms | 40.1 / 201.3 ms |
| show | 100 | 10.2 ms | 9.0 ms | 99.5 / 270.7 ms | 44.5 / 136.8 ms |
| cycle | 86 | 89.7 ms | 74.0 ms | 200.0 / 3032.8 ms | 403.2 / 1789.1 ms |
| list | 38 | 147.6 ms | 158.3 ms | 444.3 / 791.3 ms | 1009.4 / 1542.8 ms |
| next | 38 | 10.4 ms | 10.2 ms | 70.0 / 454.0 ms | 45.3 / 127.7 ms |
| pull | 37 | 587.2 ms | 718.1 ms | 2692.6 / 4643.1 ms | 3816.6 / 8390.0 ms |
| all | 3,000 | 18.3 ms | 18.9 ms | 41.3 / 1892.8 ms | 27.5 / 2697.0 ms |

The commands connect to the driver's explicit socket/address. Cold program discovery, version
probes, initialization, copying, restart and lifecycle operations are outside these workload
figures; separate scratch tests cover them. The exported counts precede the driver's own
worker/task setup, also outside the measured interval. Native callbacks and the desktop app
are disabled, dispatch is inline, and each run has a private machine folder and home.

CPU includes the PostgreSQL postmaster and its descendants, including exited children charged
to their reaping parent. Memory requests occur every sixteen completed commands and coalesce
while a reading is in flight. A short-lived backend can disappear between readings; the
observed peak is not an upper bound, and a median from few readings has limited meaning.
The November 15 memory table is RSS and must not be compared directly to this footprint table.
Retries remain internal to each command: these runs report their cost and terminal failures,
without measuring retry counts or separating contention from query cost. They exercise neither
listening recipients nor durability under a crash, and do not establish performance at ten-times scale.

The retained JSON, invocations and frozen binaries are under `artifacts/tools/board-pg-lifecycle/solar-heron-274-final-paired-2026-11-18/`.
The runtime source is `d2a39d23be553f82fe027377e3c7bc969d896601`; later comparison prose does not change those sources.
SHA-256 provenance:

- Export: `6db727b4877041e24029a022293d4bdd60c105da627703130bae11e1d67b91fa`.
- tasks: `b85706a52d4f00d8b88a2fd05ebc8df9762c4712c8825f982a9fcd2c546a3211`.
- board_load: `e27888377070deab5ad6363adb9f4c2017d53a07eb071f9fc56db6494349f874`.
- Fixture/harness: `5a9b9c8033e9be01c505a43952d08da7ad42959313a00122a76d7442be252ba5`.

## The ten-times seed, measured 2026-11-18

The same day's export has 931 tasks, 13,067 messages, 1,412 sessions and 1,827 roles. The
ten-times fixture duplicates the typed keys and references of all eleven exported tables,
preserving document text and opaque plugin fields: 9,310 tasks, 130,670 messages, 14,120 sessions
and 18,270 roles. Its SHA-256 is
`9178df95ebaca7fbac0be6db3b28380a19e93a42e2ee7b295b87f1c394af871b`.

Neither engine seeded this fixture through the board's logical copy under shipped settings,
so there is no paired workload number for it here. This is a copy/recovery limit found before
the workload, not a command failure during a completed workload:

- SurrealDB 3.3.0's seed transaction failed under `Storage::BOARD`'s 256 MiB memtable. Its owned
  server log reported `transaction commit failed keys_written=2208845 bytes_written=617476954
  write_ops=2208856`. Copy and logical dump restoration use that one transaction, which exceeds
  this setting at this board size. A driver-only 1 GiB seed also failed; a 2 GiB seed succeeded.
  That successful probe then stopped cleanly and restarted with the shipped 268,435,456-byte
  memtable and unchanged 67,108,864-byte value-log threshold, verified from both processes'
  actual settings. It ran no workload commands. No shipped storage setting changed.
- PostgreSQL 18.7's copy of the identical fixture exceeded the client's existing 300-second
  read wait, returning `Resource temporarily unavailable (os error 35)`. Closing the driver's
  owned supervision lifeline stopped its server. The shutdown log places the interrupted
  backend inside `board_said()`'s `INSERT ... ON CONFLICT` during the bulk message insert.
  Repeated updates of the same sender's latest-message row are a candidate cause; this log
  establishes the executing statement, not a complete cost attribution. The wait was not
  widened and no copy optimization is included here.

The failure logs, invocation records, scaled fixture and seed-settings observation are retained
under `artifacts/tools/board-pg-lifecycle/`: `ten-times-surreal-shipped.log`,
`ten-times-surreal-1gib.log`, `ten-times-postgresql-seed.log`, the paired pilot folder and
`solar-heron-274-seed-probe-settings.json`. This finding concerns logical
copy and dump restoration; it does not establish a limit on stopped-cluster physical recovery.
The follow-up `tools/board-copy-at-ten-times` owns the repair and the paired ten-times workload.
The lifecycle task's accepted comparison is solo, ten, thirty and empty, as scoped on its thread.

## PostgreSQL's replacement for the plan checks

The SurrealDB `perf` target's plan parser remains specific to SurrealQL. Its PostgreSQL
replacement is a separate scratch run using PostgreSQL 18's
[`auto_explain`](https://www.postgresql.org/docs/18/auto-explain.html), with nested statements
enabled. It observes statements inside the PL/pgSQL blocks and trigger functions as well as the
outer request, which an `EXPLAIN` of the outer `DO` alone cannot cover.

After copying the board, run `ANALYZE` and enable `auto_explain.log_min_duration = 0`,
`auto_explain.log_nested_statements = on`, `auto_explain.log_format = json`,
`auto_explain.log_analyze = on`, `auto_explain.log_timing = off` and
`auto_explain.sample_rate = 1` on the owned scratch server. Exercise the per-write,
per-recipient and per-tick cases the current `perf` plan check names, retaining each nested plan
and its actual row/loop counts. Check selective message, delivery, role, session and
callback-state lookups against the rows requested and their indexes at both export and ten-times
scale; distinguish intended whole-board reads. A sequential scan on a tiny table is a legitimate
planner choice, so forcing `enable_seqscan = off` or rejecting every sequential scan would not
prove the production plan.

This names the replacement method; it does not claim the existing SurrealDB parser accepts
PostgreSQL plans or that every dispatch path has already been audited. Instrumented plan runs
are separate from the CPU and memory comparison: explaining every statement changes its cost.

## What this does not measure

- **A workload on the ten-times board.** The original November 15 runs use the export or an
  empty board. The November 18 seed failures above precede workload measurement; their repair
  and paired workload belong to `tools/board-copy-at-ten-times`.
- **How often a write was retried.** A retry is inside its command; the run sees its cost (the
  claim's CPU and its tail), not its count.
- **Delivery to listening sessions.** The run's sessions register no callback, and the copied
  board carries none, so a write's dispatch finds nobody to send to. `examples/dispatch_load.rs`
  measures that, and the `perf` test target bounds a write's cost with 100 recipients listening.
- **Search and embeddings**, which `examples/search_bench.rs` and `examples/embedding_cpu.rs`
  measure.
- **A paced load.** The sessions run back to back, which is the ceiling at each size, not the
  rate agents write at.
- **The memory a server holds at rest for days.** A run is seconds long.

## Running it again

From a checkout, with a dump of the board and a scratch folder that exists:

```sh
tools/record.sh build 'tasks and board_load for paired scratch smoke runs' tools/cargo.sh build -p symphony-board --bin tasks --example board_load
tools/rs-bin.sh tasks-copy --board --out > "$DUMP"      # a read; the main checkout's build
.target/debug/examples/board_load --home "$SCRATCH" --surreal "$(bash tools/surreal.sh)" \
    --tasks "$PWD/.target/debug/tasks" --dump "$DUMP" --sessions 30 --calls 40 --seed 1
```

For the same board work on PostgreSQL, replace `--surreal BIN` with `--postgresql DIR`, the
folder of programs `[board] postgresql` names, and keep the same `--tasks`, `--dump`, seed,
sessions and calls. Without `--tasks` or `--dump`, `--postgresql DIR` instead measures the bare
server's opening and one `psql` connection running `select 1` per call; that is a different
workload and not a board comparison.

`$SCRATCH` is under the temporary folder or a checkout's `artifacts/`, a new one for each run:
the board stays in it, and a second run there would start from the first's rows. The report is
one JSON object on stdout. The example refuses the live database's name, starts its own server
and stops it; nothing here reaches the live board but the export, which reads it.
