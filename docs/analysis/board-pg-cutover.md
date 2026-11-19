# The board's cutover to PostgreSQL, and the way back

The procedure that moves a live board from SurrealDB to PostgreSQL: the order, what is copied
when, how long a board cannot be written, how to go back, and what the two stores measured side
by side. [What a live move waits for](#the-choice) is decided: the pieces not built yet, a clean
rehearsal, and the memory peak found and answered: it is found, under the target on a board of
today's size, and over it on one ten times the size, which the card `board-pg-memory-at-ten-times`
([DECISIONS](../DECISIONS.md)) puts to the owner before any live move. Today PostgreSQL serves only scratch boards
([POSTGRESQL](../POSTGRESQL.md)), and no command here moves a live one.

## What the comparison showed

Both stores ran the same work on 2026-11-18, from one export of this repository's board (931
tasks, 13,067 messages) and one build. The tables, the method and their limits are in
[the store comparison](board-store-baseline.md#paired-board-work-measured-2026-11-18).

| thirty sessions, 3,000 commands | SurrealDB | PostgreSQL |
|---|---|---|
| commands that failed | 0 | 0 |
| server CPU | 34.5 s | 60.0 s |
| `tasks` CPU | 54.9 s | 56.8 s |
| server memory, peak | 448 MiB | 1,752 MiB |
| server memory, after the run | 392 MiB | 53 MiB |
| folder grew | 15.0 MB | 1.6 MB |
| a command's wall, median and 99th | 41 ms, 1.9 s | 28 ms, 2.7 s |

- **Nothing failed on either**, in 14,600 commands across the four cases.
- **PostgreSQL costs more CPU**: 1.7 times the server's at thirty sessions, 1.9 times at ten,
  2.4 times on an empty board. The `tasks` process, most of the total, costs the same.
- **PostgreSQL's memory peaked higher in that run, and a later build's is under the target on
  this board.** It peaked at 626 MiB
  with ten sessions and 1,752 MiB with thirty, where SurrealDB peaked at 327 and 448, and held
  53 MiB after the run where SurrealDB kept 392. The move was approved with a target of 512 MiB,
  backends included. The cause was a read of the whole board answered as one value, which each
  command's backend built whole and kept until the command ended; the store's long reads now
  answer a row at a time, and the same work peaks at 377 to 419 MiB with thirty sessions (three
  runs) and 161 to 165 with ten (two), nothing failing. Those runs are another day's, export's
  and load's than the table's, so they say nothing of SurrealDB's peak beside it. They do not
  show the CPU equal either: the server's was 54.6 to 57.3 s and once 83.7 against 56.2 to 62.8,
  and the `tasks` processes' 57.3 to 65.9 s against 50.0 to 57.4, which leans higher
  ([the measurement](board-store-baseline.md#postgresqls-memory-peak-measured-2026-11-18)).
  Sessions back to back are the ceiling, not the rate agents write at.
- **Most commands answer sooner** on PostgreSQL (a beat 23 ms against 36, an `inbox` 74 against
  194, a `show` 45 against 100), and the two heaviest later (`list` 1.0 s against 0.4, `pull` 3.8 s
  against 2.7). Walls are stretched by the machine's load, 75 to 168 that day.
- **On a board ten times the size the memory gap is wider.** Measured since, by
  `tools/board-copy-at-ten-times`, on a generated board of 9,310 tasks and 130,670 messages with
  the same thirty sessions and 3,000 commands, none failing: PostgreSQL's server peaked at
  14.8 GiB against SurrealDB's 3.3 GiB, and fell back to 218 MiB where SurrealDB kept 2.0 GiB
  ([the numbers](board-store-baseline.md#paired-board-work-on-it)). One run each on a loaded
  machine, a development build, SurrealDB seeded under a 2 GiB memtable and measured under the
  shipped one. With the store's long reads answered a row at a time, PostgreSQL peaks at 1.7 to
  1.8 GiB on a board of that size (two runs, on a ten-times copy of a later export): an eighth of
  what it was beside the same day's run of main (14.9 GiB), and over 512 MiB, since its server
  opens at 207 MiB there and thirty backends hold about 50 MiB each. SurrealDB was not run on
  that copy, so its 3.3 GiB is another fixture's figure and no paired comparison.

Not measured: a crash's durability, delivery to listening sessions,
days at rest.

## What is not ready

Each is needed before a live board moves. None is built by this page.

| what | state |
|---|---|
| The live-board guard covers PostgreSQL's tables and finds a PostgreSQL board | landed with the lifecycle |
| Holds, stopped-cluster snapshots, reclaim and the schema fence on PostgreSQL | landed, proved on scratch boards |
| A copy does not let an old task number or version be taken for a new one | landed with `tools/board-recovery-generation`, board schema 22: every copy of a whole board replaces its [recovery generation](../TASKS.md#a-board-that-was-copied-or-restored), and a write naming a task read before the copy is refused. The cutover is itself such a copy |
| PostgreSQL starting for a board that is not a scratch one | not built: it is refused by design |
| The cutover command: hold, export, copy, compare, switch, with each failure leaving the board as it was | not built |
| Restoring a PostgreSQL snapshot over the board it was taken from | not built: a snapshot restores into a new scratch home only |
| The release refresh on a PostgreSQL board: its snapshot before a schema move is SurrealDB's export | not built |
| A copy of a board ten times today's size | measured by `tools/board-copy-at-ten-times`, landed. PostgreSQL takes it in 78.5 to 141.8 s of the 270 s its server gives a copy, the time going with the machine's load, and a quiet machine reaches the 270 s at about 32 times today's board; SurrealDB refuses it in one line under the shipped 256 MiB memtable and takes it in 68.1 s under 2 GiB, which no shipped setting gives ([the numbers](board-store-baseline.md#a-board-ten-times-the-size-measured-2026-11-18)). Today's boards copy on both; this bounds growth, and the way back for a board that has grown, not the first move |
| A PostgreSQL copy reported failed does not replace the board | landed with the same task: the server ends a copy at 270 s, thirty short of its caller's 300, with the board as it was. A copy whose commit and answer take those last thirty seconds still can |

## The order

Three projects are registered on this machine: this repository and two others. Each has a board
of its own, and each board moves by itself.

1. **A release that speaks both.** The prerequisites above land, and a release holding them is the
   one installed on the machine. From then every program of that release reads a board's engine
   from the board's own folder (`backend.json`), so the machine's one server serves SurrealDB
   boards and PostgreSQL boards side by side. No board has moved.
2. **A rehearsal, on a copy.** This repository's board is exported, which only reads it, and the
   export is copied into a scratch PostgreSQL. Every table is compared with the export, search is
   compared, and the copy is timed. The result goes on the board before step 3. The live board is
   not touched.
3. **This repository's board first.** Its programs are its own main's, so nothing is pinned
   behind. It is the trial for the others, and it runs a few days before the next.
4. **Each other project: its pin, then its board.** A project's pinned build speaks SurrealDB to
   its board until its pin moves. So the project first takes the release of step 1 by its own
   landing, and its board moves only once every program that wrote to it in the last week is of
   that release or later: the rule a schema move already follows
   ([the refresh](../RELEASES.md#the-refresh)). A board never moves ahead of its project's pin.
5. **SurrealDB stays in the code** until a later decision removes it. That is what keeps the way
   back open.

## One board's move

1. **Hold.** The board is held (`just tasks database hold`), as for any maintenance: its database
   is stopped, and every board command is refused and says why. Agents' work in progress is
   untouched; their board commands and the board's pages wait for the release.
2. **Export.** The held SurrealDB board is written out whole, all eleven tables.
3. **Copy.** The export goes into a new PostgreSQL cluster beside the old store, in one
   transaction.
4. **Compare.** Every table of the new board is compared with the export, row by row. Any
   difference stops the move: the new cluster is removed, the hold released, and the board is on
   SurrealDB as before.
5. **Switch.** The SurrealDB store is moved aside, whole and unchanged, out of the board's active
   folder. The board's folder is marked PostgreSQL. Both are renames.
6. **Release.** The hold is released, and the next command starts PostgreSQL.

Nothing is deleted. The export and the old store are kept until the owner says otherwise.

**How long the board cannot be written:** steps 1 to 6. It has not been timed. The copy of
today's export into PostgreSQL ran in every paired run without meeting its limit, then the 300 s
its caller waits and since 270 s at the server
([TASKS](../TASKS.md#a-board-that-was-copied-or-restored)), so the copy itself is under five
minutes; the rehearsal gives the real figure, for the whole of it, before any live board is held.

## The way back

- **Before the switch (steps 1 to 4).** Release the hold. Nothing changed.
- **After the switch, before anyone wrote.** Hold, remove the PostgreSQL mark, move the SurrealDB
  store back, release. Nothing is lost.
- **After writes on PostgreSQL.** The same move in reverse: hold, export from PostgreSQL, copy
  into SurrealDB, compare, switch, release. It costs a second hold of about the same length. The round trip
  (SurrealDB, PostgreSQL, and back to an export, every table compared) is tested on a generated
  board of the live one's shape. Going back to the old store as it was kept instead is quicker and
  loses everything written since the switch.
- **What does not come back the same.** Search for words in scripts other than Latin: SurrealDB
  spells them in Latin letters and PostgreSQL keeps them, so the same query can rank such a word
  differently on each.
- **A PostgreSQL board that breaks later.** Its snapshots are whole stopped clusters. Restoring
  one over the live board is among what is not ready above; until then the way back from a broken
  PostgreSQL board is the export route, or the kept SurrealDB store.

## The choice

What the live move waits for, chosen on [its card](../DECISIONS.md#board-pg-cutover): the
prerequisites, and the memory peak.

- **The prerequisites and the rehearsal.** What is not ready above is built, and the rehearsal
  on a copy is clean. They go ahead now.
- **The memory peak.** PostgreSQL's server peaked at 1,752 MiB with thirty sessions, and at
  14.8 GiB on a board ten times the size, against the 512 MiB target. It is found and brought
  under on a board of today's size: 377 to 419 MiB with thirty sessions, by the same driver on
  scratch boards ([what the comparison showed](#what-the-comparison-showed)). It was no setting,
  and one connection per command only in that a backend keeps its memory for its command's
  life: the fix is in what the reads ask the server to build. On a board ten times the size it
  is 1.7 to 1.8 GiB, over the target, which is unchanged: the live move stays held until the
  owner answers the card `board-pg-memory-at-ten-times` ([DECISIONS](../DECISIONS.md)), and the result on
  today's board does not stand in for it.

The target was part of what the store was approved with. The cutover is the one step that takes
a hold on a live board and a second hold to undo. Everything before it is free to do and free to
drop.

The other option, not taken, was to move on the prerequisites alone and accept the measured
cost.
