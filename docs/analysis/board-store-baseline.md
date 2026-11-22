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
listening recipients nor durability under a crash; the board ten times the size is measured
[below](#a-board-ten-times-the-size-measured-2026-11-18).

The retained JSON, invocations and frozen binaries are under `artifacts/tools/board-pg-lifecycle/solar-heron-274-final-paired-2026-11-18/`.
The runtime source is `d2a39d23be553f82fe027377e3c7bc969d896601`; later comparison prose does not change those sources.
SHA-256 provenance:

- Export: `6db727b4877041e24029a022293d4bdd60c105da627703130bae11e1d67b91fa`.
- tasks: `b85706a52d4f00d8b88a2fd05ebc8df9762c4712c8825f982a9fcd2c546a3211`.
- board_load: `e27888377070deab5ad6363adb9f4c2017d53a07eb071f9fc56db6494349f874`.
- Fixture/harness: `5a9b9c8033e9be01c505a43952d08da7ad42959313a00122a76d7442be252ba5`.

## A board ten times the size, measured 2026-11-18

The same day's export has 931 tasks, 13,067 messages, 1,412 sessions and 1,827 roles. The
ten-times fixture duplicates the typed keys and references of all eleven exported tables,
preserving document text and opaque plugin fields: 9,310 tasks, 130,670 messages, 14,120 sessions
and 18,270 roles, 31.8 million characters of thread text. Its SHA-256 is
`9178df95ebaca7fbac0be6db3b28380a19e93a42e2ee7b295b87f1c394af871b`. Every figure below is one
observation on a scratch board, from a development build, on a machine other agents were loading;
the load averages are the one-minute ones.

### The copy on PostgreSQL

`Store::copy` takes the fixture in one request. The server gives it 270 s, thirty short of the
300 s its client waits, and ends it there with the board as it was
([BOARD_OPERATIONS](../BOARD_OPERATIONS.md#a-board-that-was-copied-or-restored)).

| copy | load before → after | seconds |
|---|---|---|
| seed-only | 10.5 → 82.6 | 141.8 |
| seed-only | 27.7 → 13.6 | 83.5 |
| seed-only | 11.9 → 12.9 | 87.1 |
| before the paired workload | 14.7 → 56.8 | 78.5 |

The time is the search's, not the copy's own: the messages' terms are counted a row at a time by
the `message_words` trigger and written to a GIN-indexed table. `EXPLAIN ANALYZE` of the insert
of the 130,672 messages alone, every trigger on, at load 11 to 10:

| part | seconds |
|---|---|
| the insert, whole | 70.7 |
| `message_words` (the terms of each message's text) | 61.8 |
| `said` (each sender's last word) | 2.2 |
| `message_made` (the marker) | 0.5 |

Set apart, tokenizing the 31.8 million characters with no write took 26 to 31 s, and writing the
130,672 rows of terms with their index kept 19.5 s; the row-at-a-time trigger adds about ten
over the two. Each is work in proportion to the text. The sender with most words has 4,920 and
the next 937, so `said` rewrites no long chain: filling `said` once before the insert, tried back
to back with the unchanged copy, changed nothing that the load did not (93.4 s at load 13,
262.2 s as the load went from 82 to 28) and was not kept.

So the copy fits the wait on a machine at load 10 to 15 with about three times to spare, and
the spare is the machine's: at load 82 the same copy took 262 s of a 300 s wait, and a run of
the lifecycle task's that day passed it. At 83 s for ten times today's board, a copy on a
machine as quiet as these runs found it meets the server's 270 s at about thirty-two times
today's board: some 420,000 messages, or 100 million characters of thread text. A loaded
machine meets it sooner, at any size. No wait was changed.

Without that limit a copy that outlasts its caller's wait is reported failed and then replaces
the board: the request is sent whole, so the server finishes it after the client has gone.
`a_postgresql_copy_held_past_its_callers_wait_is_ended_and_does_not_replace_the_board`
(`crates/tasks/tests/store/copy.rs`) holds a copy at its first insert by a lock, lets its client
stop waiting, lets the lock go and reads the board once every other backend is idle; with the
server's limit taken out it reads the new board. The limit narrows that window and does not
close it: a copy that finishes inside the 270 s and whose commit and answer then take more than
the thirty left still replaces the board after its caller gave up.

### The copy on SurrealDB

The copy stages its rows (178.8 MiB for the fixture) and swaps them in in one transaction, which
must fit the memtable of the server it runs on. Under `Storage::BOARD`'s 256 MiB the server does
not commit it: the server's log in the lifecycle task's run says `transaction commit failed
keys_written=2208845 bytes_written=617476954`, 3.3 times the staged bytes. The copy then refuses
in one line, about 70 s into the run, with the rows and bytes it staged and the setting, the
board as it was and its staging removed. A 1 GiB memtable failed the same way in that task's
runs and 2 GiB took it, so a copy asks for a memtable of more than five and a half times what
it stages and at most eleven and a half. `board_load
--seed-memtable-mib 2048` gives a scratch server that memtable for the copy alone and measures
on a restart under the shipped one; the copy took 68.1 s there. No shipped storage setting
changed, so a board of this size is copied into a server started with a larger memtable, which
only `server::Home::storage` gives today.

A swap whose client stops waiting does not commit afterwards, as PostgreSQL's did: the server
drops the statement with its connection. The probe held the swap at its first task insert by an
event asking a listener of the test's every 5 ms, with a client waiting 4 s: the listener was
asked 524 times while the client waited and not once in the 2 s after it gave up, and 20 s
after the listener said to go on the board was the old one with no staging table left. It is
kept as a measurement and not as a test, since what it shows is an absence, which only a clock
can wait out. To repeat it, in a test of `crates/tasks/tests/store/`, on a board seeded with
one dump and copied with another:

```rust
let gate = crate::gate::Gate::new();
f.other_client().write(
    &format!(
        "DEFINE EVENT fixture_held ON task WHEN $event = 'CREATE' THEN {{ {} }};",
        gate.wait_here()
    ),
    serde_json::json!({}),
)?;
// A store over `f.other_client().waiting(Duration::from_secs(4))`:
let copied = impatient.copy(&other_dump); // the client's "no answer within 4s"
// Count the listener's questions now and again 2 s later (`Gate`'s `reached`), then:
gate.open();
// and read `f.store.dump()` until it is `other_dump`, or 20 s have passed.
```

`gate.wait_here()` is the event's body: `FOR $look IN 0..N { IF
http::get('http://127.0.0.1:PORT/gate') == 'open' { BREAK; }; sleep(5ms); };`, the listener
answering `wait` until the test opens it.

### Paired board work on it

Thirty sessions of forty calls, seed 1, the command mix of the paired runs above, from one
frozen development build at `135f89987`; SurrealDB seeded under the 2 GiB memtable and measured
under the shipped one. All 6,000 commands succeeded and every session's beats were verified.

| engine | commands / writes | failed | wall | tasks CPU | server CPU | store grew | load before → after |
|---|---|---|---|---|---|---|---|
| SurrealDB | 3,000 / 2,787 | 0 | 61.5 s | 336.5 s | 246.5 s | -281.2 MB | 24.1 → 75.7 |
| PostgreSQL | 3,000 / 2,787 | 0 | 72.1 s | 322.8 s | 297.5 s | 2.2 MB | 9.8 → 56.8 |

Memory, as `server_memory_mib` in MiB with the RSS sum's peak last:

| engine | opened | median | peak | after | samples | RSS peak |
|---|---|---|---|---|---|---|
| SurrealDB | 539.3 | 2,451.3 | 3,284.8 | 1,970.6 | 181 | 3,591.9 |
| PostgreSQL | 198.0 | 6,353.2 | 14,817.3 | 217.9 | 119 | 13,938.9 |

Per command, mean CPU of the command's process, and median / 99th percentile wall:

| command | count | SurrealDB tasks CPU | PostgreSQL tasks CPU | SurrealDB wall | PostgreSQL wall |
|---|---|---|---|---|---|
| beat | 2,400 | 5.9 ms | 4.9 ms | 19.5 / 78.6 ms | 18.4 / 557.9 ms |
| inbox | 172 | 57.2 ms | 41.4 ms | 476.8 / 1741.7 ms | 172.6 / 382.1 ms |
| say | 129 | 7.1 ms | 5.9 ms | 32.7 / 163.0 ms | 31.3 / 137.9 ms |
| show | 100 | 10.0 ms | 8.0 ms | 102.4 / 320.3 ms | 55.6 / 116.2 ms |
| cycle | 86 | 456.0 ms | 398.8 ms | 1024.3 / 3950.5 ms | 1417.3 / 6789.9 ms |
| list | 38 | 1374.3 ms | 1282.3 ms | 4739.4 / 6543.9 ms | 6281.4 / 8949.9 ms |
| next | 38 | 26.6 ms | 24.5 ms | 583.6 / 2917.0 ms | 85.9 / 300.0 ms |
| pull | 37 | 5894.1 ms | 5905.5 ms | 15804.2 / 30371.0 ms | 27525.0 / 42350.9 ms |
| all | 3,000 | 112.2 ms | 107.6 ms | 22.4 / 9665.1 ms | 19.7 / 10526.4 ms |

Against the thirty-session run on today's board above, a `list` and a `pull` cost about ten
times the command's CPU on either engine and a `cycle`'s step five. PostgreSQL's observed peak
was 14.8 GiB against SurrealDB's 3.3 GiB, and it fell back to 218 MiB where SurrealDB kept
2.0 GiB; what held it is [found below](#postgresqls-memory-peak-measured-2026-11-18), and
PostgreSQL's peak there is now 1.7 to 1.8 GiB. SurrealDB's folder was smaller after the run than
before it. These are single observations under load and not a cap, a rate or a latency
promise.

The reports, invocations, loads and frozen binaries are under
`artifacts/tools/board-copy-at-ten-times/`, with the copy timings, the trigger profile and both
probes' output. SHA-256: tasks
`89e1089d9f2e0e6ab6a0f6557a53c16ba43a5a11049420eacc6156abb8be628b`, board_load
`ec0ffb7055e1b0431dfdb0786bd6ab15a1aa6d3efae39f91a2e672ce8ebeec01`.

## PostgreSQL's memory peak, measured 2026-11-18

The paired runs above left PostgreSQL's server at 1,752 MiB with thirty sessions and 14.8 GiB on
the board ten times the size, against the 512 MiB the store was approved with
([its card](board-pg-cutover.md#board-pg-cutover)). The cause is one, and the store's reads no longer
have it.

**The cause.** A command is one connection, and its backend lives as long. A read of a whole
board was one statement whose answer was one `jsonb` value: every task, link, role and session
aggregated by `jsonb_agg` into an object. PostgreSQL builds such a value whole in the backend
that answers it, several times over (the aggregate's state, the value, its text, the message
sent): a 3.7 MB answer for this board took the backend from 6 MiB to 62 to 66. The backend then
keeps that memory: PostgreSQL frees it at the statement's end, where the memory contexts read
2.2 MiB, but the allocator does not give it back to the system while the process lives (a
backend that allocated and freed 200 MB once still held 387 MiB). A `pull` reads the board twice
and each sender's last message, and its backend stood at 111 MiB until the command ended,
seconds later. So thirty commands at once held thirty boards, and the server gave it all back
when they ended, which is the 53 MiB it read after each run. It was no setting: `shared_buffers`, `work_mem` and the connection count read as written, and a smaller
`work_mem` moved a backend by 2 to 3 MiB. It is the price of one connection per command only in
that the backend, and what it holds, lasts the command.

**What changed.** A read of long lists is sent as a statement for each list, answered an item a
row, with one more statement for the object's other fields, all in the request's one
transaction and so of one snapshot (`Pg::Listed`, `crates/tasks/src/store/postgres/mod.rs`); the
client joins the rows into the same JSON text the one value was. The server sends each row as it
makes it, so the backend holds a sort's memory (`work_mem`, 4 MiB, then a temporary file) and
not the board. The whole-board read, the reads that are one plain list of rows (the messages to the
owner, each sender's last message, the launches, the usage rows, the reviews waiting) and the
bound sessions are read so. A list statement's condition is the one its value had; no plan of
either was kept, so that they scan alike is not shown here. A write's read of its one task is
still one value, and so are a thread, several threads, the task tree, an inbox's lines since
its last look, the open asks and `tasks sessions`' list of every session: none of them was
moved or measured here.

**Thirty and ten sessions on this board**, by the paired driver, `--seed 1`, forty calls a
session, from one export of this repository's board read that day (953 tasks, 13,793 messages,
1,519 sessions; 18,964,211 bytes) and two frozen development builds: main at `97a4d7dc9`, and
that commit with the change. All memory is `server_memory_mib`, in MiB; the load is the
one-minute average as the measured run began and ended, on the 16 cores of the Mac above with
other agents' builds and tests running throughout.

| sessions | build | failed | opened | median | peak | after | samples | RSS peak | server CPU | `tasks` CPU | load |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 30 | main | 0 | 53.2 | 512.0 | 1,627.1 | 54.9 | 31 | 2,295.0 | 56.2 s | 50.0 s | 33 → 53 |
| 30 | main | 0 | 53.0 | 540.8 | 1,845.7 | 56.6 | 37 | 2,442.1 | 62.8 s | 57.4 s | 61 → 87 |
| 30 | changed | 0 | 52.4 | 239.2 | 381.0 | 56.2 | 55 | 916.6 | 83.7 s | 65.9 s | 23 → 37 |
| 30 | changed | 0 | 52.8 | 141.0 | 376.6 | 54.5 | 33 | 844.2 | 54.6 s | 57.3 s | 30 → 44 |
| 30 | changed | 0 | 53.1 | 138.2 | 418.6 | 56.8 | 37 | 834.8 | 57.3 s | 59.0 s | 39 → 54 |
| 10 | main | 0 | 53.8 | 354.8 | 600.7 | 55.2 | 36 | 808.5 | 20.0 s | 18.4 s | 34 → 51 |
| 10 | main | 0 | 53.2 | 348.6 | 605.3 | 54.6 | 36 | 811.8 | 21.3 s | 19.5 s | 41 → 50 |
| 10 | changed | 0 | 53.0 | 104.2 | 161.3 | 54.3 | 32 | 348.5 | 18.2 s | 18.7 s | 46 → 45 |
| 10 | changed | 0 | 52.6 | 96.2 | 164.8 | 54.0 | 29 | 325.0 | 17.0 s | 17.5 s | 36 → 39 |

Each run is 3,000 commands and 2,787 writes for thirty sessions, 1,000 and 928 for ten; none
failed, and every session's beats were found in its board. Two more runs of main at thirty, made
first while the meter was being set up, peaked at 1,589.9 and 1,719.7 MiB (load 33 → 37 and
33 → 47); their reports were not kept.

- **Thirty sessions peak at 377 to 419 MiB**, where main peaked at 1,590 to 1,846: under the
  512 MiB target in each of three runs. SurrealDB was not run that day: its 448 MiB is the
  paired run's above, on another export, build and load.
- **Ten sessions peak at 161 to 165 MiB**, where main peaked at 601 to 605.
- **The CPU is not shown equal.** The server's is 54.6 to 57.3 s against 56.2 to 62.8, with one
  run of the change at 83.7 s that the other two do not repeat. The `tasks` processes' is 57.3 to
  65.9 s against 50.0 to 57.4, leaning higher: two runs of main and three of the change under
  loads of 23 to 87 cannot say by how much; a `pull`'s own is
  708 to 774 ms against 554 to 726.
- **The peak is the meter's, as before**: a reading in every sixteen commands, so a backend that
  lived between two readings is not in it, and a run is seconds long.

**On the board ten times the size** (this export with each row made ten times, every copy's
slugs, handles and numbers its own: 9,530 tasks, 137,930 messages, 15,190 sessions), thirty
sessions, the same seed and builds:

| build | failed | opened | median | peak | after | samples | RSS peak | server CPU | `tasks` CPU | wall | load |
|---|---|---|---|---|---|---|---|---|---|---|---|
| main | 0 | 207.7 | 8,346.0 | 14,922.4 | 288.2 | 119 | 14,344.5 | 416.5 s | 439.6 s | 200.1 s | 15 → 69 |
| changed | 0 | 207.0 | 766.0 | 1,787.2 | 317.4 | 113 | 4,354.1 | 337.4 s | 460.9 s | 109.1 s | 15 → 71 |
| changed | 0 | 205.6 | 730.8 | 1,727.7 | 315.8 | 103 | 4,188.3 | 359.8 s | 498.7 s | 126.5 s | 25 → 72 |

The peak there is an eighth of what it was. SurrealDB was not run on this copy; its 3.3 GiB is
the figure of the section above's fixture, another board, and no paired comparison. It is not
under 512 MiB, and at that size it cannot be by the reads alone: the
server opens at 207 MiB, its shared buffers full of the board, and thirty backends then hold
about 50 MiB each. What a backend still holds there, read from one `pull` replayed a statement
at a time on such a board: 16 to 21 MiB after its first read of the board, whose sorted rows are
then ten times as many, and 8 more after the lines posted since its last look, which are still
one value. No setting was changed for it.

To read a backend's own memory, as these were: `footprint -p PID` on macOS, whose `MALLOC_*`
lines are the backend's allocations, beside an untagged `VM_ALLOCATE` that grows as it reads
tables; the server's `log_statement = all` with `log_parameter_max_length = -1` gives a
command's statements to replay one at a time in `psql`, each bound with `\bind`. The reports,
the scripts that sampled and replayed, and the one that made the ten-times export are among the
task's artifacts (`perf/board-pg-memory-peak/`). SHA-256: the export
`27e4e92f45da562f70e8a23d79e4ecd1a0626bd2625b50c225b9bfe43d39b8f3`, the ten-times export
`5eb82344dc1ec4cd09c13bf7f4ee1f86bd0aa1ce9534e9ea75109ac5ef53ee06`, main's `tasks`
`873e3b776d8be60f3f56b5f543c2902933259518414b92743fcc12e22ac91bf6` and `board_load`
`012bf5135d47a0e1d2a4535c5d921767114c5f0772eb2496f24d7f1deb818254`, the change's `tasks`
`82eee538fbe990b204bc92cc6a40f93acdd2e7e20deaef46740a62d31113163a` and `board_load`
`45efde74aa929d0a547c7d9ead1d80da146664e2fb15c8c6eea68aa418812549`.

- [r] **board-pg-memory-at-ten-times** — You approved moving the live boards to PostgreSQL only after its memory peak was found and brought under 512 MiB, or shown it cannot be ([[analysis/board-pg-cutover#board-pg-cutover]], B). It is found and fixed for a board of today's size: with thirty sessions writing at once the server now peaks at 377 to 419 MiB over three runs (1,590 to 1,846 before), and at 161 to 165 with ten. On a board ten times today's size it is not under: 1.7 to 1.8 GiB over two runs, down from 14.9. That larger board is a copy of today's made ten times, measured on scratch boards on a busy machine. The 512 MiB target is unchanged and was not met there. May the live boards move at today's size? A: no, the move stays held until more work brings the ten-times peak down. B: yes, the move may go ahead once its other conditions are met, with the ten-times result left open as known. Approve and say A or B in the note, or comment. [[analysis/board-store-baseline#PostgreSQL's memory peak, measured 2026-11-18]]
  [2026-11-19 01:47] agent — Posted for the board task `perf/board-pg-memory-peak`. Nothing live moves before you answer.
  [2026-11-19 02:40] owner · approved — B
  [2026-11-19 02:48] agent — B recorded: the live move may proceed at today’s board size once the other cutover conditions are met. The ten-times peak of 1.7–1.8 GiB remains an open limitation; the 512 MiB target is unchanged. This lifts the memory-decision hold, and I have routed it to the cutover work. No live board was moved by this reply.
  [2026-11-19 02:48] agent · resolved

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
