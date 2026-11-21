# The board's cutover to PostgreSQL, and the way back

The procedure that moves a live board from SurrealDB to PostgreSQL: the order, what is copied
when, how long a board cannot be written, how to go back, and what the two stores measured side
by side. [What a live move waits for](#the-choice) is decided: the pieces not built yet, a clean
rehearsal, and the memory peak found and answered: it is found, under the target on a board of
today's size, and over it on one ten times the size, which the card `board-pg-memory-at-ten-times`
([DECISIONS](../../DECISIONS.md)) decides: a board of today's size may move once the pieces and the
rehearsal are done, with the ten-times peak left open. The pieces are landed and
[the rehearsal](#the-rehearsal-2026-11-19) was clean. PostgreSQL serves the board whose home
names it ([POSTGRESQL](../POSTGRESQL.md)), and `tasks-move` moves a board's store there and back.
This repository's board [moved on 2026-11-21](#this-repositorys-board-moved-2026-11-21); the
other projects' boards are on SurrealDB until each moves after its own pin.

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
| A copy does not let an old task number or version be taken for a new one | landed with `tools/board-recovery-generation`, board schema 22: every copy of a whole board replaces its [recovery generation](../../workflow/TASKS.md#a-board-that-was-copied-or-restored), and a write naming a task read before the copy is refused. The cutover is itself such a copy |
| PostgreSQL starting for a board that is not a scratch one | landed with `tools/board-pg-cutover-pieces`: PostgreSQL serves any board whose home names it (`backend.json`), and a board outside a scratch place gets that name only by the move below; there its server outlives the command that started it, as SurrealDB's does |
| The cutover command: hold, export, copy, compare, switch, with each failure leaving the board as it was | landed with the same task, proved on scratch boards: `tasks-move ENGINE`, [one board's move](#one-boards-move) |
| Restoring a PostgreSQL snapshot over the board it was taken from | landed with the same task: `tasks-move --restore SNAPSHOT`, under the move's journal; the cluster it replaces is kept ([POSTGRESQL](../POSTGRESQL.md#holds-snapshots-and-recovery)) |
| The release refresh on a PostgreSQL board: its snapshot before a schema move | landed with the same task: the refresh reads the board's schema and writers on the engine its home names, snapshots it with `pg_dump` through its running server, and waits on no SurrealDB pin ([RELEASES](../../workflow/RELEASES.md#the-refresh)) |
| A copy of a board ten times today's size | measured by `tools/board-copy-at-ten-times`, landed. PostgreSQL takes it in 78.5 to 141.8 s of the 270 s its server gives a copy, the time going with the machine's load, and a quiet machine reaches the 270 s at about 32 times today's board; SurrealDB refuses it in one line under the shipped 256 MiB memtable and takes it in 68.1 s under 2 GiB, which no shipped setting gives ([the numbers](board-store-baseline.md#a-board-ten-times-the-size-measured-2026-11-18)). Today's boards copy on both; this bounds growth, and the way back for a board that has grown, not the first move |
| A PostgreSQL copy reported failed does not replace the board | landed with the same task: the server ends a copy at 270 s, thirty short of its caller's 300, with the board as it was. A copy whose commit and answer take those last thirty seconds still can |

Every row is landed or measured, each proved on scratch boards, and this repository's board
[moved](#this-repositorys-board-moved-2026-11-21) on them.

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
   not touched. Done on 2026-11-19, clean: [the rehearsal](#the-rehearsal-2026-11-19).
3. **This repository's board first.** Its programs are its own main's, so nothing is pinned
   behind. It is the trial for the others, and it runs a few days before the next. Done on
   2026-11-21: [this repository's board moved](#this-repositorys-board-moved-2026-11-21), so no
   other board moves before 2026-11-24.
4. **Each other project: its pin, then its board.** A project's pinned build speaks SurrealDB to
   its board until its pin moves. So the project first takes the release of step 1 by its own
   landing, and its board moves only once every program that wrote to it in the last week is of
   that release or later: the rule a schema move already follows
   ([the refresh](../../workflow/RELEASES.md#the-refresh)). A board never moves ahead of its project's pin.
5. **SurrealDB stays in the code** until a later decision removes it. That is what keeps the way
   back open.

## One board's move

`tasks-move postgresql --as HANDLE` does all of it, and `tasks-move surrealdb` is the same move
the other way (`Home::move_to`, `crates/tasks/src/server/moving.rs`). It is a program of its
own, the owner's to run from the main checkout as `tasks-reclaim` is, and no board command: the
`tasks` every session runs links none of it.

1. **Hold.** The board is held as for any maintenance: its database is stopped, and every board
   command is refused and says why. Agents' work in progress is untouched; their board commands
   and the board's pages wait for the release. A hold its caller set before
   (`just tasks database hold`) is used and left standing after; otherwise the move sets one
   and releases it at the end.
2. **Snapshot and export.** A snapshot of the held store is kept. The move then reads the board
   through a server of its own, which no record names: its schema, which must be this build's
   (a move migrates nothing), and the databases the store holds beside the board's own (the
   scratch boards of `just tasks --database NAME`), which are not moved: they stay in the old
   store, kept whole in `moved/STAMP-ENGINE/`, and the move names them. A scratch board on the
   new engine starts empty. The board is written out whole as `export.json`: the eleven
   tables of the dump, and the tables no copy holds, below. The export is written where it is
   kept, in `moved/STAMP-ENGINE/`, before anything of the board changes places.
3. **Copy.** The export goes into a new store of the other engine in a work folder of the move's
   own (`moving/`), the dump in one transaction, then the carried tables, every lease among them
   ended first.
4. **Compare.** The new board is read back and compared with what was read, row by row, the dump
   and the carried tables alike; a number is the same by what it counts (one engine writes a
   whole number of seconds `3.0` where the other writes `3`). Any difference stops the move.
5. **Switch.** Under a journal (`move.json` in the board's folder), each step durable before the
   next: the old store is moved aside, whole and unchanged, into `moved/STAMP-ENGINE/`; the new
   one is moved in; the board's folder is marked for the new engine; the journal is marked
   committed; the folder's recovery bindings are revoked; the journal is removed.
6. **Release.** The hold the move set is released, and the next command starts the new engine.

Nothing is deleted but the move's work folder. The old store, with every database it held
beside the board's, and the export stay in `moved/`, and the snapshot with the snapshots, until
the owner says otherwise.

**A failure.** Before the switch nothing of the board has changed places: the work folder is
removed, the export already written stays in `moved/STAMP-ENGINE/` beside nothing else, and the
board is on the engine it was on. Marking the journal committed is the commit point. A step
that fails before it is undone at once, and the board is as it was, its recovery bindings
included. After it the board is on the new engine, and the command says the move committed and
what is left. A move cut short at any step (a process killed, a machine that lost power) leaves
its journal standing: while it stands nothing reads the folder as either engine, no server starts,
and the hold is not released. Only the holder's next `tasks-move`, a move or a restore,
settles it, undoing a move that never committed and finishing one that did, each told by the
journal and by what is on disk; an ordinary command is refused and finishes nothing. A step that
can be neither finished nor undone leaves the hold and the journal, and says where the data is.

**What moves beside the eleven tables.** The dump does not hold everything a board keeps, so the
move carries these row by row, under the same key on both engines, and compares them too: each
session's callback address and what it has seen (`callback_state`), the lines not yet delivered
(`callback_event`, `chat_event`), who writes the board (`writer`), the artifact ledger, the
unanswered-wait restores, the leases (`callback_lease`: a recipient's, a session's close or
restore, a host's dispatch) and the counters (`counter`): the task holds, each recipient's place
in its outbox, which keys its next line past the ones carried, and the fences. A lease is carried
ended and its revision moved on by one. The revision is the token its holder's writes carry, so a
holder from before the move finds a lease that is no longer its own, the next one taken is past
both, and none stands that the hold did not see; this holds after a move back as well. What it
leaves, and why nothing is lost with it: the counters the copy sets from the rows it copies
(messages, tasks, asks); what the copy derives (each sender's last word, the search indexes); the
board's schema record and recovery generation, which the new board has of its own; and the
marker, which each engine keeps its own way and the move raises past the old board's. A test names every table of the board as copied, carried or left
(`crates/tasks/src/store/copy.rs`), and fails on one that is none.

**What a reader kept.** A reader holds the board's rows under its marker. Its connection ends
with the old server, which is what makes the Symphony's reader open the board anew; and the move
raises the new board's marker past the old one's, so a reader that reaches the board where it was
(PostgreSQL's socket is the same after a move there and back) never meets the same number over
other rows. The new board has a recovery generation of its own and its folder is bound to none:
a token read before the move is refused, and `just tasks recovery rebind` binds the board again.

**Under the board recipe.** `just tasks` sets `SYMPHONY_UNASKED`, and under it a move or a
restore of a board outside a scratch place is refused. That is a guard against accident: its
absence grants nothing, and a live board's move is the owner's call.

**How long the board cannot be written:** steps 1 to 6, which took 52 s on a copy of this
repository's board, and 42 s back ([the rehearsal](#the-rehearsal-2026-11-19)), and 74 s for the
board itself on a machine loaded far past its cores
([its move](#this-repositorys-board-moved-2026-11-21)). The Symphony's pages say it is held for
a while longer: a page's reader that found the board held then tried it again only after a wait
that doubled from 1 s to 60 s (`apps/symphony-server/src/live/retry.rs`), so they went on saying so
for about 100 s past the board's own release. Since, a reader refused by a hold tries again at its
first tick or page request after the release. The copy into PostgreSQL is ended at 270 s by its server, thirty
short of the 300 s its caller waits ([TASKS](../../workflow/TASKS.md#a-board-that-was-copied-or-restored)).

## The rehearsal, 2026-11-19

Step 2 of [the order](#the-order), run once by `tools/board-pg-cutover-rehearsal` with the
commands on that task's thread, on main at `8b56e0580`, a development build, board schema 24.

- **The board.** This repository's, read by `tasks-copy --board --out` (the main checkout's
  build, a read): 1,029 tasks, 16,079 messages, 1,749 sessions, 2,089 roles, 5,880 deliveries,
  21 MB. It was copied into a new SurrealDB board of a scratch repository under the temporary
  folder, and the move was the real command there.
- **To PostgreSQL.** `tasks-move postgresql`: 28,985 rows compared with what was read, none
  different. The hold stood 52 s by its own log, and the command took 53.8 s.
- **Back.** `tasks-move surrealdb`: the same 28,985 rows, a hold of 42 s, the command 42.3 s.
  The export read from PostgreSQL is the export read from SurrealDB byte for byte, and the
  board's dump after the round trip is the live board's export byte for byte.
- **Search.** Fifteen searches (one word, several, a hyphenated name, a number) answered the
  same on both engines: the same tasks in the same order with the same excerpts, the first
  twenty of each. Two of them, of accented and of Japanese words, found nothing on either, so
  [what does not come back the same](#the-way-back) was not met.
- **The machine** was loaded, at 41 to 48 by its one-minute average on sixteen cores, so a quiet
  one holds a board a shorter time.

What it does not show: one run each way; the tables no dump holds were all but empty, two
writers and nothing else, as a read-only export carries no callback, lease, counter or
undelivered line, so those are proved by the move's tests on generated boards alone; and on a
scratch board PostgreSQL ends with each command, where a live board's outlives it. The copy was
read back by the move's own export; `tasks-copy --board --out` reads a board on either engine,
the same document from each ([POSTGRESQL](../POSTGRESQL.md#the-whole-board-read-and-written)),
so a board that has moved keeps its read-only export.

## This repository's board moved, 2026-11-21

Step 3 of [the order](#the-order), by `tools/board-pg-this-board-moves`, whose thread has the
commands and the read-back: `tasks-move postgresql` from the main checkout at `aa2ae8142`, board
schema 28, then `just tasks recovery rebind` at once, under the owner's approval on the cards
`board-pg-cutover` and `board-pg-memory-at-ten-times` ([DECISIONS](../../DECISIONS.md)).

- **The move.** 41,891 rows compared with what was read, none different. The board was held
  74 s by its hold log, and the command took 76.4 s, at a load average of 158 on sixteen cores.
- **What it kept.** The SurrealDB store, whole, with its export, in
  `symphony/tasks/moved/2026-11-21T20-52-05Z-surrealdb/`, and its snapshot in
  `symphony/tasks/snapshots/`. The store held 46 scratch boards (`just tasks --database NAME`)
  beside the board; the move left each there and named it, so a scratch board starts empty on
  PostgreSQL.
- **Read back.** Every table of `tasks-copy --board --out` from PostgreSQL counted the same as
  the kept export: 1,138 tasks, 19,130 messages, 2,131 sessions among them. The board took
  writes at once, a median of 4.2 ms over the next 205.
- **What the agents met.** Callback dispatch failed 9 times, all within the hold. The machine's
  server answered the board's pages that the board was held for about 100 s after the release,
  until its reader tried again ([how long](#one-boards-move)).

The other projects' boards are on SurrealDB; each moves after its own pin, by step 4, and not
before 2026-11-24.

## The way back

- **Before the commit point (steps 1 to 4, and a switch that fails or is cut short before it).**
  The move undoes itself, or its holder's next run does. Nothing changed.
- **After the switch, before anyone wrote.** The kept SurrealDB store in `moved/` is the board as
  it was; putting it back is by hand, held: the PostgreSQL cluster moved out, the store moved
  back, the mark rewritten. No command does it, as the move in reverse loses nothing either.
- **After writes on PostgreSQL.** The same move in reverse, `tasks-move surrealdb`:
  hold, export from PostgreSQL, copy into SurrealDB, compare, switch, release. It costs a second hold of about the same length. The round trip
  (SurrealDB, PostgreSQL, and back to an export, every table compared) is tested on a generated
  board of the live one's shape. Going back to the old store as it was kept instead is quicker and
  loses everything written since the switch.
- **What does not come back the same.** Search for words in scripts other than Latin: SurrealDB
  spells them in Latin letters and PostgreSQL keeps them, so the same query can rank such a word
  differently on each.
- **A PostgreSQL board that breaks later.** Its snapshots are whole stopped clusters, and
  `tasks-move --restore SNAPSHOT` puts one back over the board it was taken from, keeping
  the cluster it replaces. What was written since the snapshot is in that kept cluster, not on
  the board. No lease revision comes back with the snapshot: the restore ends every lease past
  the ones the replaced cluster held, as a move does
  ([POSTGRESQL](../POSTGRESQL.md#holds-snapshots-and-recovery)).

## The choice

What the live move waits for, chosen on [its card](../../DECISIONS.md#board-pg-cutover): the
prerequisites, and the memory peak.

- **The prerequisites and the rehearsal.** What is not ready above is built, and
  [the rehearsal](#the-rehearsal-2026-11-19) on a copy was clean. This repository's board
  [moved](#this-repositorys-board-moved-2026-11-21) on 2026-11-21, under the owner's word on
  both cards; a board of today's size waits for nothing more but its own pin.
- **The memory peak.** PostgreSQL's server peaked at 1,752 MiB with thirty sessions, and at
  14.8 GiB on a board ten times the size, against the 512 MiB target. It is found and brought
  under on a board of today's size: 377 to 419 MiB with thirty sessions, by the same driver on
  scratch boards ([what the comparison showed](#what-the-comparison-showed)). It was no setting,
  and one connection per command only in that a backend keeps its memory for its command's
  life: the fix is in what the reads ask the server to build. On a board ten times the size it
  is 1.7 to 1.8 GiB, over the target, which is unchanged. The card `board-pg-memory-at-ten-times`
  ([DECISIONS](../../DECISIONS.md)) decides it, B: the memory peak no longer holds the move of a
  board of today's size, which waits for the prerequisites and the rehearsal alone, and the
  ten-times peak stays an open limit, not a target met.

The target was part of what the store was approved with. The cutover is the one step that takes
a hold on a live board and a second hold to undo. Everything before it is free to do and free to
drop.

The other option, not taken, was to move on the prerequisites alone and accept the measured
cost.
