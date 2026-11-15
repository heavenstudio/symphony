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
  resident memory counts the pages they share once each, so such a server reads higher here than
  it holds, and the CPU of a process that ended before the run did is not in the server's
  reading. A store of that shape needs both said beside its numbers, or the meter taught to read
  them.
- **Durability is whatever the store's server does as the board starts it.** This page did not
  measure what a commit's sync costs or what it survives. A store compared with these numbers
  commits at least as durably as SurrealDB does under the board's settings, and says how it was
  set to.

## What this does not measure

- **A board ten times this size.** Every run is this board or an empty one.
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
tools/cargo.sh build -p symphony-board --bin tasks --example board_load
tools/rs-bin.sh tasks-copy --board --out > "$DUMP"      # a read; the main checkout's build
.target/debug/examples/board_load --home "$SCRATCH" --surreal "$(bash tools/surreal.sh)" \
    --tasks "$PWD/.target/debug/tasks" --dump "$DUMP" --sessions 30 --calls 40 --seed 1
```

`$SCRATCH` is under the temporary folder or a checkout's `artifacts/`, a new one for each run:
the board stays in it, and a second run there would start from the first's rows. The report is
one JSON object on stdout. The example refuses the live database's name, starts its own server
and stops it; nothing here reaches the live board but the export, which reads it.
