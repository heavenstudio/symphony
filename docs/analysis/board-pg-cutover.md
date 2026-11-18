# The board's cutover to PostgreSQL, and the way back

The procedure that would move a live board from SurrealDB to PostgreSQL, for the owner to judge
before anything live moves: the order, what is copied when, how long a board cannot be written,
how to go back, and what the two stores measured side by side. It is a proposal. Today PostgreSQL
serves only scratch boards ([POSTGRESQL](../POSTGRESQL.md)), and no command here moves a live one.

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
- **PostgreSQL's memory peaks higher and falls back.** It peaked at 626 MiB with ten sessions and
  1,752 MiB with thirty, where SurrealDB peaked at 327 and 448. After the run it held 53 MiB;
  SurrealDB kept 392. The move was approved with a target of 512 MiB, backends included: at rest
  PostgreSQL is far under it, and with ten or more sessions writing back to back it is over it.
  Nobody has looked for the cause yet. The readings are one run each, sessions back to back,
  which is the ceiling and not the rate agents write at.
- **Most commands answer sooner** on PostgreSQL (a beat 23 ms against 36, an `inbox` 74 against
  194, a `show` 45 against 100), and the two heaviest later (`list` 1.0 s against 0.4, `pull` 3.8 s
  against 2.7). Walls are stretched by the machine's load, 75 to 168 that day.

Not measured: a crash's durability, delivery to listening sessions,
days at rest.

## What is not ready

Each is needed before a live board moves. None is built by this page.

| what | state |
|---|---|
| The live-board guard covers PostgreSQL's tables and finds a PostgreSQL board | landed with the lifecycle |
| Holds, stopped-cluster snapshots, reclaim and the schema fence on PostgreSQL | landed, proved on scratch boards |
| A copy does not let an old task number or version be taken for a new one | `tools/board-recovery-generation`, in progress. The cutover is itself such a copy |
| PostgreSQL starting for a board that is not a scratch one | not built: it is refused by design until this is answered |
| The cutover command: hold, export, copy, compare, switch, with each failure leaving the board as it was | not built |
| Restoring a PostgreSQL snapshot over the board it was taken from | not built: a snapshot restores into a new scratch home only |
| The release refresh on a PostgreSQL board: its snapshot before a schema move is SurrealDB's export | not built |
| A copy of a board ten times today's size | PostgreSQL takes it in 84 to 142 s of the 270 s its server gives a copy, the time going with the machine's load; SurrealDB refuses it in one line under the shipped 256 MiB memtable and takes it under 2 GiB ([the numbers](board-store-baseline.md#a-board-ten-times-the-size-measured-2026-11-18)). Today's boards copy on both; this bounds growth, not the cutover |

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
today's export into PostgreSQL ran in every paired run without meeting its 300-second limit, so
the copy itself is under five minutes; the rehearsal gives the real figure, for the whole of it,
before any live board is held.

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

The procedure is the same either way. The choice is what the live move waits for.

- **A: the prerequisites alone.** Once what is not ready is built and the rehearsal is clean, the
  boards move in the order above. The measured cost is accepted: more server CPU, and a memory
  peak over the 512 MiB target while ten or more sessions write at once.
- **B: the prerequisites, and the memory peak.** As A, and the live move also waits for one task:
  find why PostgreSQL peaks at 1,752 MiB with thirty sessions and bring it under the target, or
  show why it cannot be, measured by the same driver. The prerequisites and the rehearsal go
  ahead meanwhile, so B delays only the last step.

B is recommended. The target was part of what was approved, the run misses it by more than three
times, and nobody knows the cause yet: it may be a setting, or it may be what one connection per
command costs. The cutover is the one step that takes a hold on a live board and a second hold to
undo. Everything before it is free to do and free to drop.
