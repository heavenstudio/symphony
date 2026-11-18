# PostgreSQL board lifecycle

PostgreSQL boards run only in scratch homes: temporary folders or a checkout's artifacts folder.
The machine server and the board commands use the same home selection. Live activation and a
migration command are not enabled: a live board moves by
[the cutover procedure](analysis/board-pg-cutover.md), once what it waits for is there.

## The board home owns its engine

The home keeps `backend.json` beside `server.json`, `hold` and `lock`. Its complete shape is
`{"format":1,"engine":"postgresql"}` or `{"format":1,"engine":"surrealdb"}`. Without it a
home uses SurrealDB. Unknown formats, invalid JSON, unknown engines, or data of the wrong engine
refuse explicitly. PostgreSQL data without an identity is ambiguous, and never opens as SurrealDB.

`Home::initialize_backend` initializes a scratch home's identity under the starters' lock. It
accepts no existing identity of another engine and no data of the other engine. It writes and
syncs a temporary file, renames it into place, then syncs the home. Repeating the same identity
keeps it. The PostgreSQL cluster is `postgresql/`; a retained SurrealDB store belongs outside the
active home's `data/` before that home can identify itself as PostgreSQL.

`Home::board_server` selects the engine for repository discovery, including the machine server's
board reader. `[board] postgresql` names the machine's programs, never the selected engine; absent
that override, the first executable `postgres` on `PATH` supplies the folder. Discovery validates
PostgreSQL 18 or later and its same-folder companion tools before starting anything. A
PostgreSQL server listens on its private Unix socket alone and ends with the process that started
it. A command uses one connection for its lifetime; there is no pool.

## Holds, snapshots and recovery

`just tasks database hold WHY --as HANDLE` records the shared hold before stopping PostgreSQL
cleanly. A failed stop leaves the hold standing; its holder repeats the command to finish it.
Starts and reclaim refuse while held. `just tasks database release --as HANDLE` releases without
starting anything; another handle must name the holder with `--of HOLDER`. The history is the
same append-only `hold.log` as other board holds.

`just tasks snapshot` takes the complete stopped cluster into `postgresql-snapshots/`, including
its WAL. It refuses a running server or a status it cannot establish, and an external tablespace.
It prefers a copy-on-write clone and uses a plain copy where cloning is unavailable. A copy is
made under `.taking` and published by rename only when complete; the next attempt replaces an
interrupted copy. `just tasks snapshot --list` lists complete snapshots. PostgreSQL snapshots are
kept until explicitly removed. A whole stopped cluster is the unit of a
[PostgreSQL filesystem backup](https://www.postgresql.org/docs/18/backup-file.html).

`Postgres::restore` copies a snapshot into a new scratch home, with the same PostgreSQL major.
Initialize that home's PostgreSQL identity explicitly before using repository discovery on it.
It refuses an existing target cluster and a running source, and starts neither source nor target.
The next start writes the target's own socket settings and lets PostgreSQL recover its WAL. The
snapshot is unchanged. This is rehearsal and inspection, not an in-place rollback or acceptance
of an older live schema; the highest-seen schema record remains outside the database.
The identity selects an engine only. The copied cluster holds the
[recovery generation](TASKS.md#a-board-that-was-copied-or-restored) its source had, so the
restore revokes every recovery binding of the target home before the data is in place, and
publishes no data when that fails: the restored board refuses every bound write until `just tasks
recovery rebind` gives it a generation of its own, and then takes no token read on its source.
The source's own binding is not touched, and clients already open on the source keep writing the
source.

## Free space and versions

Autovacuum makes dead space reusable. `tasks-reclaim` on a PostgreSQL home runs
`VACUUM (FULL, ANALYZE)` on the board database, without an export or a data swap. This
[PostgreSQL rewrite](https://www.postgresql.org/docs/18/sql-vacuum.html) returns space to the
filesystem, needs temporary disk room, and locks each table exclusively while rewriting it.

The shared schema rule and writer inventory apply on both engines: a migration is atomic, recent
older writers must be accounted for, and older readers keep their one-version window. Each
PostgreSQL write reads the schema row `FOR SHARE`, so an older repeatable-read snapshot meets a
migration as a serialization conflict before it can write after that migration. A live-source
check fingerprints both the common schema source and the PostgreSQL definitions from the same
default-branch commit. The live board is identified by its PostgreSQL socket as well as by its
repository, preserving the refusal of tests and examples that reach their own live board.

On a writer program's first opening and its hourly inventory refresh, ended PostgreSQL backends'
marker counters are folded into one retained `['board']` counter in the same transaction. The
sum stays unchanged, active backends keep their own counters, and subsequent commits still raise
the marker. Readers do no compaction. A conflict or failed inventory write rolls the complete
refresh back; a later due opening can try again. This uses the existing counter table and adds no
schema version.

Measurements and their limits belong to [the store comparison](analysis/board-store-baseline.md).
