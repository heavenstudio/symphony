# PostgreSQL board lifecycle

PostgreSQL serves the board whose home names it. A scratch home (a temporary folder, or a
checkout's artifacts folder) is made a PostgreSQL board in place; a board anywhere else becomes
one only by a move of its store, `tasks-move postgresql`
([the cutover procedure](analysis/board-pg-cutover.md#one-boards-move)), and with no identity
PostgreSQL makes and starts nothing there. The machine server and the board commands use the same
home selection. No live board has moved: that waits for what the cutover's
[choice](analysis/board-pg-cutover.md#the-choice) names.

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

An identity changes only by a move of the board's store. While a move's or a restore's journal
(`move.json`) stands in the home, nothing reads it as either engine: every start and reach is
refused, naming the journal, until its holder settles it.

`Home::board_server` selects the engine for repository discovery, including the machine server's
board reader. `[board] postgresql` names the machine's programs, never the selected engine; absent
that override, the first executable `postgres` on `PATH` supplies the folder. Discovery validates
PostgreSQL 18 or later and its same-folder companion tools before starting anything. A
PostgreSQL server listens on its private Unix socket alone. On a scratch home it ends with the
process that started it; on any other it outlives its starter, as the board's SurrealDB server
does, and the next command finds it by its cluster's own record. On a scratch home a command
run right after another meets that one's server on its way out: its start waits for the
server's end, which the kernel reports, and then starts its own. That holds whether the
cluster's record already says the server is stopping, or still says it is ready because the
shutdown signal has not reached it yet: the start records each server it starts with the
process that started it (`postgresql.starter` in the board's folder), and a server whose
starter has ended is on its way out. The board's patience is that
wait's backstop, and a server still stopping by then is left to finish, the refusal saying the
machine's load and to run the command again. A server that is starting is never waited for:
its start is refused, naming the retry. A command uses one connection
for its lifetime; there is no pool. Its backend lives as long
and keeps what memory its largest statement took, so a read of long lists (a whole board, the
messages to the owner) is sent a statement a list and answered an item a row, all in one transaction
and one snapshot: the server's memory under thirty commands at once is
[measured](analysis/board-store-baseline.md#postgresqls-memory-peak-measured-2026-11-18).

## The whole board, read and written

`tasks-copy --board --out` writes the dump of the board the run reaches, and `tasks-copy
--board` copies a dump into it, in the database the board's home names: one board read out of
PostgreSQL and out of SurrealDB is one document, byte for byte. `tasks-copy ADDR` takes a
PostgreSQL server by its socket's folder, `postgresql:/PATH`. A PostgreSQL board that cannot be
read is refused as PostgreSQL's, naming its database and socket and `just tasks database`,
which says what serves it and whether it is held.

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
[recovery generation](BOARD_OPERATIONS.md#a-board-that-was-copied-or-restored) its source had, so the
restore revokes every recovery binding of the target home before the data is in place, and
publishes no data when that fails: the restored board refuses every bound write until `just tasks
recovery rebind` gives it a generation of its own, and then takes no token read on its source.
The source's own binding is not touched, and clients already open on the source keep writing the
source.

`tasks-move --restore SNAPSHOT --as HANDLE` (`Postgres::restore_over`) puts a snapshot
back over the board it was taken from: one of that board's own snapshots, by its name or path.
The board is held for it (its caller's hold, left standing, or one set now and released after)
and its server stopped. The cluster in place is served once, by a server of the restore's own,
and its leases and each recipient's committed revision read; the snapshot is copied into a work
folder (`restoring/`), served there once, and every lease written ended, its revision past the
greater of the snapshot's and the replaced cluster's. A holder that took its lease after the
snapshot holds a revision the snapshot never saw, which the next taker would otherwise be handed
too: as after a move, it finds a lease that is no longer its own. A cluster in place that cannot
be served or read does not stop the restore, which is the way back from one that broke: each
lease is then moved on by more takes than any holder made, and the command says so. The copy is
then switched in under the
journal a move runs under: the cluster in place is moved into `moved/STAMP-postgresql/`, the copy
moved in, the journal marked committed, the home's recovery bindings revoked. Nothing is deleted,
the snapshot included. A failure before the commit leaves the board on the cluster it had; one
cut short is settled by its holder's next `tasks-move`, a restore or a move, undone before
the commit and finished after. The restored board refuses every bound write until
`just tasks recovery rebind`. The record of the highest schema the board was seen at stays
outside the database, so a snapshot of an older schema restores a board that went back, which
every opening refuses as it does any other.

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

## A hosted board

A hosted board is kept in a PostgreSQL the platform runs, reached over TCP with verified TLS:
a database made beforehand for one project, owned by that project's login, which can make no
database or role. It is a build's choice, the board's `hosted-postgresql` feature, which no
default build and no installed release has: the release's `tasks`, `symphony` and server link
none of the driver, and a hosted deployment builds the board with the feature named. No
command reaches a hosted board yet, and none moves a board to one: the library opens one
(`Store::opening_hosted` over `pg::hosted::Client`), and reaching it from a command is the
cutover's, with its own decision. [The design](analysis/hosted-postgresql.md) says why each
rule below is what it is, and [the matrix](history/hosted-postgresql-matrix.md) what was
proved, on which servers, and what was not.

**Servers.** PostgreSQL 16 and 18; any other major is refused before a statement of the
board's runs. The local board's own server stays 18 or later. On 16 the search analyzer is a
definition of its own, built from 18's Unicode data, so a search reads the same words on
both; text outside ASCII costs about six times 18's to index there.

**Settings and secrets.** [`[board.external_postgresql]`](PROJECT_FILE.md#boardexternal_postgresql)
names the variables that hold the URL (or the path of a mounted file of it), the certificate
authority's file and the process's class. No credential is written in a project file, passed
as an argument or said in a refusal: a refusal says a key's name, a fixed reason, PostgreSQL's
SQLSTATE, and the backend and second its log has the rest under. The secret is read again for
each new connection, so a rotated one is taken up and the connections made with the old one
are retired.

**The connection.** TLS is always on and always verified against that authority alone, for
the name the URL gives; the login is SCRAM bound to the TLS channel, and a server that offers
less (trust, a clear-text or MD5 password, no TLS) is refused. The login must be no superuser,
hold no right to make databases or roles, replicate or pass row security, be no member of
another role, and own the database, which must be UTF-8.

**Transactions.** A request's statements run in one explicit `REPEATABLE READ` transaction on
one connection, with the transaction's own variables, as the socket client's do; a migration's
script says its own. Nothing is sent twice. A connection lost before the commit was sent
committed nothing and says so; one lost after it says the outcome is not known, and the caller
reads the board before deciding. A conflict the server reports is decided again by the store,
as on any board.

**Capacity.** A process keeps one pool for a board and class, of that class's `pool_max`, with a bounded
line; a request past the line or its wait is refused, never given a connection outside the
pool. The budget in the settings is the deployment's: every class's processes times its pool,
with the maintenance connections, the role's other users and headroom, within the role's cap,
or the table is refused. Each connection takes one of its class's slots in the database (a
session advisory lock), so a class's total holds across processes, revisions and instances
whatever each one counts; the role's own cap, where the server refuses first, is a refusal of
capacity too. A pool is not used across a fork, and a thread that holds a connection is
refused a second.

**Making and moving.** A hosted board is made, and moved to a newer schema, only by an
opening that says it is the deployment's migration (`Opening::migrating`), which runs as the
class `maintenance`; any other opening of an empty or older board is refused before anything
is written. A database that holds tables or functions and no board's record is never taken
for an empty one. The writers' record and the schema guard are the same as on any board: a
writer seen within the week holds a migration, and a writer behind a board moved on is
refused at its next write.

**Its check.** `just hosted-check` builds the board with the feature and runs its tests and
its matrix against PostgreSQL 16 and 18 servers of its own on this machine; `just lint-rs`
builds it under clippy, and the release run runs the check as its `hosted` step. It needs
PostgreSQL 16's programs beside 18's (`brew install postgresql@16`, which starts nothing and
changes no `postgres` on `PATH`; `SYMPHONY_POSTGRESQL_16` names another folder), and `openssl`
for the certificates its servers present. Without them it is red, never skipped.
