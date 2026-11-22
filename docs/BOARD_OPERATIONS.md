# Operating the board

What whoever runs a board needs, and what a program that opens one must keep: the store and the
schema contract between the builds that read and write it, which build moves a live board, who
writes it, its server, snapshots and holds, what the command may write, a server that will not
end, a board that went back or was copied, and the health the server records of it. Using the
board, its tasks, roles, landing and commands, is [TASKS](TASKS.md); a program reads and writes
it through [its JSON APIs](BOARD_API.md); a PostgreSQL home is [POSTGRESQL](POSTGRESQL.md).

## The schema contract

Each build of the board writes one contract, its schema version, which the board records with the
additive steps on it the board has ([CHECKS](CHECKS.md#the-board-is-a-contract)). The version
counts breaking steps alone: a step that only adds (a table of its own, an index that is not
unique, an optional field on a table every build reads and writes past what it does not know)
leaves it as it is. A build opens a board of its own contract with one read and no write, whatever
additive steps either has. One with fewer steps than the board reads and writes it, keeping on each
row the fields it does not know. One with more adds them where it may move the board (below), in
one transaction, as any move, holding no writer; where it may not, it uses the board as it is: a
write naming a table of a step the board lacks is refused before anything is written, and the
board refuses a field or a table it does not define. A whole-board copy, export or move to another
engine runs only on a board with the build's own steps. A newer contract migrates an older board
forward once, its steps and the new version in one transaction before any other write. An older
build is refused
before it writes, naming the project, the program to rebuild (its file name and
where it is) and `just symphony`. A Symphony one version
behind still reads the board, and every write it would send is refused the same
way. A store opened before a newer build migrated is refused at its next write.
The Symphony never moves a board forward or makes one, so a branch's command line
or preview server with a newer schema leaves the live board as it is: a board
behind its build, or not made yet, it refuses before it reads, naming `just tasks
migrate` in the project's main checkout. A branch tries a newer schema on a
scratch board (`TASKS_DATABASE=NAME`), which its own `just tasks --database NAME`
moves.

### Which build moves a live board

A project's live board, the `tasks` database on the server of the data in its
common dir, however a run reaches it (by an address, it is known by the server
listening there, from any folder), moves forward only by a build its default
branch says is its own, by what that branch commits: never what a checkout
holds, nor the branch the main checkout has out. The default branch is
`[project] default_branch` in the main checkout's `symphony.toml`, else `main`.
For a scratch PostgreSQL home's durable engine selection, holds, snapshots and recovery, see
[PostgreSQL board lifecycle](POSTGRESQL.md). PostgreSQL activation on a live board is not enabled.

Where it commits the board's schema source (`crates/tasks/src/schema.rs`, as this
repository does), that is any command of a build of that very file, byte for
byte, wherever it was built: main's build moves the board at its first command
once a schema lands, and only such a build makes the board when it is not made
yet. A PostgreSQL board also checks `crates/tasks/src/store/postgres/schema.rs` from the same
default-branch commit. Where it commits none (a consuming project, which pins a build), it is only
`just tasks migrate` run in the main checkout, which says whether it made the
board, moved it from one version to the next, added the additive steps it lacked, or found
nothing to move; there
any writer makes a board not made yet. A repository with no main checkout (a
bare repository's worktrees, a submodule's working tree) moves its board, where
its default branch commits no source, only by `just tasks migrate`, run in any
checkout of it. A project whose default branch is not `main` names it in
`default_branch`; one that does not is judged by `main`, and with no branch of
that name its board moves only by `just tasks migrate`, as a consuming project's.
A default branch that cannot be read (one `default_branch` names that does not
exist, a ref that names no commit) moves the board by no writer, `just tasks
migrate` included, and the refusal says why.
Every other writer, a branch's build by its path, through the launcher, its
`tasks api`, sweep, `agent-send` or `tasks-copy`, is refused before it writes,
naming `just tasks migrate` in the main checkout and a scratch board as that
program takes one: `tasks` and `agent-send` by `--database NAME`, and `tasks-copy`
by `TASKS_DATABASE=NAME` with `--board` or its own `tasks-copy ADDR NAME`, each of
which its `--help` names. Any
writer moves a scratch board. A test, an example or `cargo run` (a process Cargo runs),
is refused the live board of the repository it runs from before anything is read
or its server started, and so is every process under a test whatever environment
it was handed, under `cargo test` or `cargo nextest run`. A child of `cargo run` or
of an example, of a test binary run by its path, or a process that outlived its
parent, started with a cleared environment, is not: nothing then says Cargo runs
it. Builds of commits from before this rule move a live
board as they always did.

### The record of the highest schema seen

A live board's highest schema is also kept outside its database, in `symphony/tasks/schema-seen.log`
beside its data (`crates/tasks/src/live/seen.rs`), so a store that loses its newest writes cannot
take the record with it: a line `VERSION SECONDS seen DATE` for each version the board is first
seen at, and the record is the highest line since the last `accepted` one. A line that does not
read as one (cut short, edited by hand) is passed over: it neither raises nor lowers the record,
and the next line is appended after the newline it lacks, never glued onto it. A board that reads
below the record, or a store that holds no board where one was seen, went back. Every program that
opens the live board refuses it before it reads or writes, in its words: the board went back from
schema N to M (or to no board at all), when it was seen at N, that nothing was read or written,
that `just tasks migrate` must not be run, and where the recovery is described
([A board that went back](#a-board-that-went-back)). They are:

- every store that opens it as its project's, which reads the record before it decides anything by
  the board's schema: the `tasks` commands and `tasks api`, `agent-send`, the Symphony's readers,
  `tasks-copy --board` into the board and `--board --out` out of it, and `tasks-copy ADDR` when
  the server at ADDR is a project's board server;
- `tasks migrate`, before it asks where it runs, so a worktree's refusal never sends anyone to
  migrate in the main checkout;
- the recorder (`tasks run`) and the detached dispatcher (`tasks dispatch`), which take the
  board's tables as they are: once there is a record they read the board's schema first, and the
  recorder then runs its command all the same, asking the board nothing;
- a consuming project's own code that reaches the board through the library
  (`symphony_board::cli::board_client`) and opens its store on the client it is handed: once there
  is a record the board's schema is read first, and no client is handed over for a board below it;
- the heartbeat (`tasks beat`, the hook's), quietly, since it never refuses a tool call: it binds
  nothing and writes no key. It reads the board's schema only when the record is above its build's:
  below that, a board that went back is at another schema than its build's, and the schema guard
  refuses its one write;
- `tasks-reclaim`, while the board's server runs, before it stops it, and with none running, on
  the reclaim's own start on the store.

Nothing else reads the record: `just tasks schema` reports what it says (exit 1 while the board
went back), `just tasks snapshot` opens no board, nor do `just tasks database`, its hold and its
release (the hold is step 2 of [the recovery](#a-board-that-went-back), so it stops a board that
went back as any other). Otherwise the store adds the version it found to the record when it is
higher, and the one it migrated to. A scratch board keeps no record, nor does a server an address
names that is no project's board server.
Builds from before the record neither read nor write it: the protection holds for a project only
once each of its readers and writers runs a build that carries it, the main checkout's
`.target/bin/tasks`, the shared Symphony server and a consuming project's host.

## Who writes a board

A board keeps a line for each program that opens it to write (`crates/tasks/src/store/writers.rs`,
the `writer` table): the program's file name, where it is, its host, the schema it was built at
and when it was last seen, by the machine's clock, which `TASKS_NOW` does not set. The board's own
programs (`tasks`, `agent-send`, `tasks-copy`) keep one, and so does a project's own program that
opens the board to write through the board's crate. The heartbeat and the recorder, which take the
board's tables as they are, keep none: the `tasks` build they run keeps its line by its other
commands. The line is read in the opening's one read of the board's schema and written only when
it is missing, names another schema, or is an hour old, so a board command sends no request for it
most of the time and "last seen" is good to the hour. A reader keeps none, and the Symphony opens
its board to read. A writer refused a board behind its build (a project's live board,
which only `just tasks migrate` moves) still writes its line, so a writer rebuilt for the next
schema counts as rebuilt once it has run; one refused a board ahead of its build writes nothing,
refused before any write. The table holds no event, so a line written moves no board marker, and
builds of any schema write it.

`just tasks migrate` reads the lines before it moves anything. A writer seen in the last week
that is behind the schema the move would bring the board to, other than the migrate's own build,
would be refused the board from then on: the migrate names each (its program, schema, path, host
and when it was last seen), moves nothing, and exits 2. Rebuild each at the new schema and run it
once, which records it, then migrate again; `just tasks migrate --leave-behind` moves the board
all the same, and each writer left behind is refused, naming itself as the program to rebuild,
until it is. A writer not seen for a week is not counted. Only the migrate asks: where a project's
default branch commits the board's source, as this repository's does, main's build moves the board
at its first command once a schema lands, and names no writer. A move of additive steps alone, to
a board of the build's contract, names none either: every build of the contract writes the board
after it ([CHECKS](CHECKS.md#the-board-is-a-contract)).

The shared server is not counted. The Symphony opens a board to read
(`crates/symphony/src/taskview/connect.rs`), though it writes through that store (the owner's
saves and chat, asks, a task's priority, fields and cancels), and only an opening to write keeps a
line. Counting it would stop every move: the server takes a build of the new schema only by its
reload after the move ([RELEASES](RELEASES.md#the-refresh)), so its line would always be behind
when the migrate reads it, and every refresh's migrate would refuse. Meanwhile a server one schema
behind its board reads on, and every write it sends is refused: the owner's saves and chat, with
the asks they open, wait in its journal, which it tries again each tick until a server that can
write hands them over, while its other writes (the owner's message on a task, a priority, a field,
a cancel, an ask it would close) are refused until it has reloaded. Every page of the project
says so in the server's notice, and that a verdict or a message saved meanwhile is kept and
reaches no agent until then. A server two schemas behind
reads nothing of its board.

The exception is a step declared to keep the Symphony writing
([CHECKS](CHECKS.md#a-schema-step-that-keeps-the-symphony-writing)). Its migration records beside
the version that the Symphony one schema behind may write the board (`meta:schema`'s
`writable_from`), and a store the Symphony opened to read then writes the board as at its own
schema. That covers its tick's chat journal, owner's asks, callbacks and gives, and the owner's
saves. Its pages say nothing of the schema. The guard reads the record in each write, so a later
step that is not declared stops those writes at once, even for a store opened before it. That role
is only the Symphony's reader's, the one `symphony::taskview::connect` opens. Every other program
is refused a board one schema ahead as before: `tasks`, `agent-send`, `tasks-copy`, a project's
own program, a copy's staging, a recovery's rebind, and a store opened unchecked. A record that is
missing, or not an integer, grants nothing, and neither does a board two schemas ahead. The
record arrives with schema 25. A Symphony older than that does not read it and refuses every write
one schema ahead.

The record arrives with schema 18, a raise of its own, and a build below it keeps no line, so the
move to 18 names no one: a board at 17 has no table of writers to read. In this repository main's
build moves the board at its first board command once the raise lands, as for any schema here,
with no migrate and no writer check. A consuming project's board below 18 is brought up by its own
`just tasks migrate` in its main checkout, never by a release's refresh, which finds no record
there to judge ([RELEASES](RELEASES.md#the-refresh)): that move names no one. From then on every
program of the project's still built below 18 (a linked `tasks` or `agent-send` not rebuilt, a
program of its own that opens the board through the board's crate) is refused the board, naming
itself as the program to rebuild; the project rebuilds each, and each keeps its line once it has
run. From 18 on, every build that can write a board keeps its line, so each later move names the
writers it would leave behind.

From 18 on, a release's refresh moves a consuming project's board to the release's schema, by the
release's own `tasks migrate` run in the project's main checkout, its store snapshotted first:
where a project's default branch commits no board source, its live board is moved only from its
main checkout, by whichever build runs there (`crates/tasks/src/live.rs`). The refresh reads the
lines first and moves the board only when every writer seen there in the last week is a release's
own (a binary of a release installed in the machine folder, `releases/NAME/bin/`, on this host,
which the `current` the refresh then points takes on) or was built at the release's schema; when
the only writers behind are a release's own, its migrate runs with `--leave-behind`, naming them.
A line on this host whose program is gone from its path, nothing there or a file written since
the line's last possible write, is not counted, and is left behind the same way
([a writer gone from its path](RELEASES.md#the-refresh)); so is one whose program is no build the
project runs now (none its main checkout's `.target/bin` links name) and that has not run for
longer than the project's `[board] stale_writer_hours`, a day by default.
Any other writer behind holds that board alone: the refresh names each to rebuild and leaves that
board where it was, until each is rebuilt and run once and a later refresh, or the machine's
server's next round, moves it. No board holds another or `current`: each board is snapshotted
just before its own move, and a migrate that fails is that board's red alone
([RELEASES](RELEASES.md#the-refresh)). A release that keeps the schema `current` names moves only
a board left behind it.

## The store and its server

The store takes about what it holds. Every server the board starts keeps each
value beside its key, where compaction drops the versions a rewrite leaves, and
logs at most 256 MiB of writes before it moves them to an sstable. SurrealKV's
default had sent every value over 4 KiB to a value log that only grows: 102 GiB
held a board of 0.5 GiB. The `tasks-reclaim` binary, the owner's to run from the main checkout,
rebuilds a store that grew that way. It stops the server and keeps the store as a
[snapshot](#the-store-and-its-server) before any server of its own starts on it: a clone, a plain copy where
the volume cannot clone, and no reclaim where it can be neither (nothing exported or removed, the
server started again on the store). On a volume that cannot clone, that copy needs free room equal
to the whole store, beside what the exports and the new store take: make that room first, as the
refusal says, with the store's size. It then exports every database, imports them into a new store,
and checks that store's exports byte for byte against the old ones. Only then does it swap the
stores and remove the old one, which that snapshot keeps; `tasks-reclaim` says where, and until
when. It is kept as any snapshot is: until the owner removes it while its write-ahead log holds
writes (a store no server ran on that ended uncleanly or went back, whose log the reclaim's own
server would replay and its clean stop remove), else among the newest three whose log is empty;
the room the old store takes comes back only once it is removed. The exports stay in
`symphony/tasks/reclaim/exports/`. `--dir FOLDER` rehearses on a copy of `symphony/tasks/` (`cp -c`, a clone).

The `tasks-move` binary, the owner's to run the same way, moves the board's store to the other
engine (`tasks-move postgresql --as HANDLE`, `tasks-move surrealdb --as HANDLE`) and puts a
snapshot of a PostgreSQL board back over it (`tasks-move --restore SNAPSHOT --as HANDLE`): held,
exported, copied, every row compared, then switched under a journal with a commit point. It
moves the board's own database alone: a scratch board (`--database NAME`) in the same store
stays in the old store, which is kept whole in `symphony/tasks/moved/`, and the move names each,
so a scratch board on the new engine starts empty. A failure before the commit point leaves the
board on the engine it was on, nothing is deleted, and a move cut short is settled by its
holder's next `tasks-move`
([one board's move](analysis/board-pg-cutover.md#one-boards-move),
[POSTGRESQL](POSTGRESQL.md#holds-snapshots-and-recovery)).

**Snapshots.** `just tasks snapshot` clones the board's store, `symphony/tasks/data`, into
`symphony/tasks/snapshots/YYYY-MM-DDTHH-MM-SSZ` (the time it was taken, in UTC; `.N` after another taken
in the same second); `--list` names those kept, the newest first
(`crates/tasks/src/server/snapshot.rs`). It takes the lock every start of the board's server
takes, so no start begins while it clones, and it holds the store's own lock, `symphony/tasks/data/LOCK`,
the one SurrealKV holds while a server has the store open, whatever path that server was given (a
trailing slash, a link, a relative path): it is refused while the recorded server or any other
process has the store open, and no server opens it while the clone is made. A snapshot is never a
copy of files a server is writing, so a running board's store is snapshotted only once it is
stopped (under the hold, below). The clone is copy-on-write, one `clonefile` on macOS and
`cp -R --reflink=always` on Linux, and a plain copy only before a replay (below) or a reclaim:
elsewhere a volume that cannot clone takes none. It is made under `snapshots/.taking` and renamed
once whole, so a snapshot listed is whole; one cut short is removed by the next. The launcher takes
one before each start of a board's server, under the same lock, once it has found the board not
held: outside a scratch place before every start (`Home::snapshot_before_start`), and on every board
before a replay (`Home::keep`). `tasks-reclaim` keeps one of the old store the same way, on every
board, once it has stopped the board's server and before its own servers start on the store.
Each is the store as the last server left it, its write-ahead log not yet replayed, which a start
that loses its log, and a later clean stop on the older state, take with them. A start that finds
the store as a snapshot already holds it, its log and manifest file for file (an earlier start that
failed left it so), takes none more. The server's log, `symphony/tasks/server.log`, says the snapshot
each start took, or why it took none; a snapshot that cannot be taken keeps no start from going on
but a replay's. A board `TASKS_SERVER` names has no store here to snapshot, and the command opens
no board.

Which are kept: a server that stops cleanly leaves its store's `wal/` empty, so a snapshot whose
write-ahead log holds anything was taken after a server ended uncleanly (killed, crashed, cut off),
before a replay, or of a store that went back, and it may be the only copy of what that log holds.
Each such snapshot is kept until the owner removes it, and the taking, `--list`, the server's log
and `just tasks database` say so (`kept until the owner removes it`). Of the others, the newest
three are kept, and each taking removes the older ones by name. A snapshot shares every block with
the store when it is taken and keeps the old blocks as the store's server rewrites its tables and
value log, so each grows towards the size of the whole store as it was then: `df` shows what they
cost, while `du` counts a clone's shared blocks as its own. A `tasks-reclaim` frees nothing while
snapshots of the old store are kept: the room comes back once they are removed. The owner removes
one with `rm -rf symphony/tasks/snapshots/NAME`; nothing runs on a snapshot, so that is safe at any time.

The server is never killed. Whatever stops it asks it once to end and waits
while it writes what it holds to disk and closes its store. It is never asked
twice, by any command: SurrealDB takes a second signal for an immediate
shutdown, before its store is closed, so the board writes each ask down first
(`symphony/tasks/ending/PID`) and a later command only waits for it. One that has not
answered yet is still starting, replaying its log, and is never signalled, ended
or joined by a second: a command that waited a minute for it says so, and the
next one waits again. A write the board answered for is on disk, and survives a
kill or a power cut. A start whose log may not fit the board's memory (one a
server started without the board's settings left) replays it first in a private
server and stops that one cleanly, so none of it is lost to a later kill.

A replay is the one start that can remove what no server applied. A store that
went back (a start that replayed its log in pieces and ended before its next
flush, as an older build's kill of a starting server does, or a crash in a
private replay of a log over 2 GiB, which its largest memtable splits too) holds a log segment
its manifest counts as written: every server on it skips that segment, and its
clean stop removes it. So a replay never runs without a snapshot of the store as
it is, taken before the private replay's server starts, on every board, scratch
or not: a clone in `symphony/tasks/snapshots/`; a plain copy, said as one, on a volume
that cannot clone; neither, and the start is refused and nothing replayed. Its
write-ahead log holds the log the replay applies, so it is kept until the owner
removes it once the board reads as it should. The start says where on its stderr
and in the server's log, and `just tasks database` lists it. In the server's log
the replay says itself after that snapshot's line: `· a private replay of the
store's log begins`, then its server's own lines and its clean stop's `Goodbye!`,
then `· the private replay is through`, then the board's server's lines. A replay
runs only on a log a server left without its clean close, so they are few. A
board that reads older than it was after a replay went back: hold it, and
recover from a clone of that snapshot, never from the snapshot itself nor the
board's own data ([A board that went back](#a-board-that-went-back)).

A replay's server is never the board's server. The start records the replay in
`symphony/tasks/replay.json`, with its server's pid and address as soon as it has
them, until that server has closed the store cleanly. A starter that ends in the
middle of a replay (a Symphony stopped while its start replays, a command
killed) leaves the replay's server running, detached, to finish alone under the
replay's memtable. While the record stands, every server on the data is taken
for the replay's, whichever pid the record names and whether or not
`server.json` names it, so none is adopted as the board's and none started
beside it. Until it answers it is never signalled: a command waits for it as
for any server still starting, and the Symphony's reads are refused naming the
replay, so its pages say the board is starting. Once it answers, the next start
stops it cleanly, which writes what it replayed to the store's tables and
empties the log, and starts the board's own server under the board's memtable,
with nothing left to replay. In the server's log `· the private replay's record
stands, and …` names which server it was (the one the record names, or another
on the data, and whether `server.json` names it), then come its `Goodbye!` and
`· the private replay is through`, before the board's server's lines. The hold
and a reclaim stop it the same way. The record is removed only once nothing it
may name runs: the pid it names has ended, or it names none, and every listing
of every process (`ps`) for a second shows no server on the data (`· the private
replay begun above has no server running and was not said through`); the next
start then replays what log is left. The record writes down when its server's
process started, as each ask to end does: a pid that a process started at
another time has now, after a reboot in the middle of a replay say, is not taken for
that server, so a record or an ask left behind holds nothing up and needs no
hand. Where whether its server runs cannot be told for two seconds, a `ps` that
fails or the named server running off the data, the command is refused saying
so, nothing is adopted or removed, and the next command asks again; `just tasks
database` says the same, and what the owner does should it last ([Stuck
servers](#stuck-servers)). A build from before the record takes such a server
for the board's.

A command that waits on another's start, stop, hold or snapshot (the starters'
lock, `symphony/tasks/lock`) says on stderr, after a second and then every 30 s, what
it waits on: the holder's pid, since when, and what it does, a private replay
with its log's size among them (`symphony/tasks/lock.json`, while it holds the lock).
The Symphony's reads wait at most 5 s, then fail naming the same, and wait no
longer for a server on the data that runs and does not answer (a replay whose
starter has gone, as a Symphony restarted in the middle of one leaves, which the
refusal names as that replay). When the
Symphony is itself the one to start the board, and the start is to replay the
log, it runs that start on a thread of its own: the read that found it to make
is refused at once, naming it, as is each read after it until the start is
through; a start that fails is said to the next read, once, and the read after
that starts it again. Either way its pages answer 503 while a replay runs, rather
than waiting minutes, and say the board is starting, with the log's size ([Board
health](#board-health)).

A server that does not answer is replaying its log or hangs, and the refusal
says which to look for: how long it has run and on how much CPU, and the size of
the log. A replay works the CPU all along (a log of gigabytes takes minutes) and
answers once it is through; a server whose CPU time (`ps -o time= -p PID`) stands
still across a minute hangs. The board never ends one, as it cannot tell the
two apart, and a replay cut off can lose what it had not yet written: the owner
holds the board (below), which leaves that server running, says it, and keeps
anything from starting another, ends it (`kill PID`), and releases the hold.
Nobody else signals, starts or restarts the board's server by hand.

Work on the data itself, a snapshot, a replay on a clone or a swap, holds the
database stopped: `just tasks database hold "WHY" --as HANDLE` stops the server
cleanly and writes `symphony/tasks/hold`, and until its release every board command
and the Symphony are refused, naming who holds it, since when and why. Only its
holder releases it (`just tasks database release --as HANDLE`), or another
handle naming the holder (`--of HOLDER`), which it says: a release by mistake in
the middle of a swap is what the hold is there to stop. No role decides it, as
the board, which holds the roles, is not read while it is held. Each hold and
release is a line of `symphony/tasks/hold.log`. The hold reads no record of the
schema the board was seen at, so it stops a board that went back as any other:
it is step 2 of that recovery. A clean stop removes the log segments the store's
manifest counts as written, the hold's included: on a store that went back, keep
the log first (step 1 there), the snapshot taken before the start on the older
state, else a clone of `data/` (`cp -c -R`) while its server still runs.

Only builds that carry these rules keep them: an older build starts a server
under a hold, and ends one that has not answered within a minute, in the middle
of its replay, which is how a store goes back. They hold once each build that
reaches the board carries them: this repository's main checkout's
`.target/bin/tasks`, relinked by its next `just tasks` after the landing; the
shared Symphony server, which reads every project's board, at its restart (the
owner's word); and each consuming project's host, once its pin moves past the
landing and the host is rebuilt and restarted.

## What the command may write

`symphony init` allows the `just tasks` recipe ([TASKS](TASKS.md#where-it-lives)) unasked, so it
refuses the recorder
([MACHINE_WORK](MACHINE_WORK.md#recording-builds-and-tests)) and, like this
repository's own, marks what it runs (`SYMPHONY_UNASKED`): under the mark
`just tasks export --out PATH` writes only into the checkout it runs in or, from a
linked worktree its repository lists, the main checkout's artifacts folder, never
into a git folder, with the path's links and `..` resolved, and replaces only an
earlier snapshot; it refuses any other place, or a file of the project's own,
with one line naming it, writing nothing (`crates/tasks/src/confine.rs`). An
agent the board starts under the mark stays in the project the same way
(`crates/tasks/src/launch/bounds.rs`): `launch --worktree DIR`, the worktree a
launch makes and the one a restore makes again and starts in, an older record's
too, lead only into those places or under the project's worktree base (`<repo>-wt/`
beside a main checkout that lists the command's checkout as its own, so a worktree's
`.git` file rewritten to name another repository has none, or `SYMPHONY_WT_DIR`), never
into a git folder, or the command refuses with one line naming the folder and starts
nothing. A submodule, a `--separate-git-dir` checkout or a bare repository's worktree
has no main checkout (its common dir is no `.git` folder), so under the mark a launch
there makes its worktree only under `SYMPHONY_WT_DIR`, or takes `--worktree DIR` in the
checkout. The launcher's git commands run in that checkout itself (`root_of` in
`crates/tasks/src/launch/mod.rs`; with no mark and no `SYMPHONY_WT_DIR` its worktrees go
beside it), so what a launch makes there is of its own repository, never of the one
holding its common dir, a submodule's superproject. A launch under the mark takes no
`--prompt`: an
agent's first message reads as its person's own word, so it is `/work SLUG HANDLE`
alone (a reviewer's `/review-task SLUG HANDLE`, each with `$` for Codex), and the
rest goes on the task's thread, which the skill reads. A restore
resumes the conversation its record names. Under the mark `tasks api`'s
`record_launch` records neither a worktree outside those places nor a conversation
id; no agent id is bound to a handle (`tasks api`'s `bind_agent` is refused, and
`just tasks beat` binds nothing: a heartbeat binding is the hook's, which runs
`tasks beat` itself on the session's own tool calls); and `just tasks callback`
registers a Codex thread only as the caller's own (`CODEX_THREAD_ID`, which it
detects with no address), which the board writes into the handle's open Codex
record. So a record's conversation is one the board's launcher recorded (the id it
chose at a launch, or one it read from the agent's terminal, the hook's binding or
a callback) or the Codex thread an agent registered from its own process. Three
limits stay: `--as` names any handle, so an agent may give another handle its own
conversation (the hook binds the session to the handle its board command names,
and a callback registers its thread for it), whose restore then resumes that
agent's conversation; a record an older build wrote is not told apart; and a
folder is checked before the agent starts in it, so a folder on its path replaced
by a link in between still leads out. The
reply recipes keep their writes to the same places
(AGENTS.md, "Agent integration"). The binary run by itself, which no rule allows,
writes and starts agents wherever it is told. The mark is the one command's: no child the board starts keeps it
(`sys::MARKS`), so the review command, a land's `on_land` and the regenerator write
where the project says.
`launch`, `restore`, `cleanup`, `close` and `reap` are its own, hc's terminals and all
(`crates/tasks/src/launch/`); the board calls no `herdr`.

## Stuck servers

The board asks a server once to end and kills none, so two things can outlast
its commands, and both are the owner's.

A server still running ten minutes after its one ask (`symphony/tasks/ending/PID`
names it, with when it was asked and when its process started) is left to
finish, and the stop says so, naming its pid, that file and its log; every later
stop only waits on it again. A server writing a large memtable takes minutes. One
whose CPU time (`ps -o time= -p PID`) stands still across a minute has hung: the
owner holds the board (`just tasks database hold "WHY" --as HANDLE`, which holds
from the moment it begins, while its stop still waits on the server, and after
it says the server still closes, so nothing starts another), ends it by hand
(`kill -9 PID`: the writes it held are in its log, which the next start replays)
and releases the hold. The ask is forgotten by the next stop or start once its
process has ended, or once another process, which started at another time, has
its pid.

A private replay's record (`symphony/tasks/replay.json`) whose server cannot be told
to run or not (a `ps` that keeps failing, a kernel that does not say when the
process with its pid started, or the server it names running while neither it
nor any other process is a server on the data) refuses every command
that would start or stop the board's server, and `just tasks database` says why.
Once `ps` can tell, the next command settles it alone. Should it never, the owner
removes the record by hand once `ps -ww -axo pid,command` lists no `surreal
start` on the board's `data`.

## A board that went back

A board refused as one that went back holds an older store than the one it was seen at: what was
written since is not in it, and may still be in the files the store left. Run nothing that writes,
`just tasks migrate` above all, which would write on top of the older state. `just tasks schema`
says what the store holds and what the record says, and exits 1 while the board went back; every
page of the project says it on the server's notice, the project picker under its name, and
`just tasks status` first among its warnings ([Board health](#board-health)).

When the older board is meant, a fresh store or a restore the owner chose, the owner accepts it:
`just tasks schema --accept VERSION`, `none` for a store with no board, is refused unless the
board holds exactly that version, and adds an `accepted` line to the record, which counts from
there. The board is then read, and moved forward, as any board behind its build is. A restore
from a snapshot puts a clone of it in place with the database held (below), `symphony/tasks/data`
renamed aside first: `cp -c -R symphony/tasks/snapshots/NAME symphony/tasks/data`, never the snapshot itself.

**Recovering what was lost** is the owner's, or done on the owner's word, by hand, in the board's
folder (`symphony/tasks/` in the git common dir). `CLONE` below is a clone of the copy step 1 keeps, and
every server started is one of your own, on a clone, on a port of your own.

1. **Keep the log before anything stops the server.** A server on the older state that stops
   cleanly removes every write-ahead log segment below the manifest's log number as obsolete
   (`Cleaned up … obsolete WAL files during shutdown`), the unapplied ones among them, and the hold
   stops it cleanly. The copy to recover from is the oldest snapshot whose write-ahead log holds
   writes among those taken since the board's server last stopped cleanly: the launcher took it
   before the first start after the unclean end, with the log the server before it left, not yet
   replayed. `just tasks snapshot --list` marks those snapshots (`kept until the owner removes
   it`), and `symphony/tasks/server.log` names the snapshot taken before each start, in order, with a
   clean stop's `Goodbye!` between two starts. A snapshot taken before a private replay is such a
   copy, whether the store had gone back before it or the replay itself was cut off: in
   `server.log` its line is followed by `· a private replay of the store's log begins`, the replay
   server's own lines and its clean stop's `Goodbye!`, then `· the private replay is through`; a
   replay whose starter ended before it was through has its end said by the next start instead,
   `· the private replay's record stands, and …` just before that `Goodbye!`, naming the server it
   stopped. That `Goodbye!` is the replay's and never counts as the board's server
   stopping cleanly: it is the stop that removes a segment no server applied, so the snapshot
   before it holds what the board lost. Nor do a reclaim's: after its keep's line (`· a snapshot
   of the store before this reclaim`), the `Goodbye!` lines up to the next start's snapshot line
   are its own servers', on the old store and on the new one. Where a build wrote none of these
   lines, the times tell. SurrealDB dates each of its lines in UTC but its `Goodbye!`, which
   follows its stop's last dated line, and each server's first line is its warning that
   authentication is disabled; a snapshot is named by the UTC second it was taken, just before the
   start whose lines follow it, and a start by a build that takes snapshots, outside a scratch
   place, always writes its snapshot line first. A build from before the replay's two lines shows
   a replay as two servers' lines after one snapshot line, the second server's first line dated
   within a second of the first's last, with no snapshot line between. A clean stop and then a
   start by a build that writes no snapshot line show the same lines, the start's first line
   dated when that start came; one that came within a second of the stop reads as a replay, and
   `symphony/tasks/hold.log` dates each hold and release, whose stop a start may follow at once. Clone
   it out of `snapshots/` at once
   (`cp -c -R symphony/tasks/snapshots/NAME symphony/tasks/data.evidence-DATE`; `cp -R --reflink=always` on
   Linux, `cp -R -p` where the volume cannot clone, as the board's own plain copy is made). With
   none (no build that takes snapshots made that start), clone the data folder the same way at
   once, while the server on the older state still runs: it keeps the segments below
   its log number until it stops, and no build that reads the record writes to the board meanwhile
   ([the record](#the-record-of-the-highest-schema-seen) says which do). Never start anything on that copy: each step
   below works on a clone of it (`cp -c -R`), so each try starts again from it.
2. **Hold the database.** `just tasks database hold "WHY" --as HANDLE` stops the recorded server
   cleanly and writes `symphony/tasks/hold`, and no start of the database happens until `just tasks
   database release --as HANDLE`, so no board command starts it on the old path between the
   swap's two renames (step 8).
3. **Read the log number.** `CLONE/manifest/00000000000000000000.manifest` records it: the first
   segment a start replays, every segment below it taken to be in sstables already and skipped.
   It is the big-endian 8-byte number at bytes 10 to 17 of the file, after its format version (2
   bytes) and next table id (8): `printf '%d\n' 0x$(xxd -s 10 -l 8 -p
   CLONE/manifest/00000000000000000000.manifest)`. `ls -l CLONE/wal` lists the segments, each named
   by its number in 20 digits (`00000000000000000017.wal`). In a store that went back the log
   number reads N+1 while segment N is still in `wal/`: the engine moved past a segment it never
   applied. What servers wrote on the older state is in the segments from N+1 up, and may be at
   the end of segment N as well: the server whose replay split N goes on writing to N, the segment
   it has open (in one reproduction it appended 923 bytes there and wrote no N+1), while a server
   started later on the older state writes from a segment of its own. Writes at the end of N are
   replayed with it in step 5, after the ones the board lost, in the order they were made, and
   step 4 cannot remove them; step 6's counts show them. To leave them out, recover from a copy
   taken before that server started, whose segment N ends before them, where there is one (the
   snapshot a build that takes them took before that start). When
   several segments lie below the log number, N is the lowest of them; when none does, nothing is
   skipped, and step 4 has nothing to do: a snapshot taken before a replay of a store that had not
   gone back reads so, its whole log still to replay, and step 5 replays it all.
4. **Point the clone at segment N, and remove the segments from N+1 up**, which must not be
   replayed on top of it: `printf '%016x' N | xxd -r -p | dd
   of=CLONE/manifest/00000000000000000000.manifest bs=1 seek=10 count=8 conv=notrunc` writes N
   there, and step 3's read then says N; then `rm -f CLONE/wal/NNNNNNNNNNNNNNNNNNNN.wal` for
   segment N+1 and each one after it.
5. **Replay it in a server of your own.** On a port of your own, never the board's, with the
   value-log threshold the board's own servers use, and a memtable that holds the largest segment,
   so its replay never splits it (a replay that splits a segment records the next one's number at
   its first flush, and an end before it flushes the rest loses that rest):
   `SURREAL_SURREALKV_VLOG_THRESHOLD=67108864 SURREAL_SURREALKV_MAX_MEMTABLE_SIZE=BYTES
   "$SYMPHONY_SURREAL" start --unauthenticated --log info --bind 127.0.0.1:PORT surrealkv://CLONE`,
   `BYTES` twice the largest segment's size in `ls -l CLONE/wal`, at most 4294967295 (SurrealKV's
   largest memtable): a memtable takes more of each write than the log holds of it, which is why
   the board's own replay takes twice its log, so one just above a segment's size can still split
   it. Record its pid, and wait until it answers (`curl -s http://127.0.0.1:PORT/health`); never
   end it while it replays.
6. **Verify by schema, counts and newest change.** `TASKS_SERVER=127.0.0.1:PORT just tasks
   schema` reads the clone's schema and writes nothing; the counts and the newest message by
   `"$SYMPHONY_SURREAL" sql --endpoint http://127.0.0.1:PORT --namespace sf --database tasks` with
   `RETURN [array::len(SELECT VALUE id FROM task), array::len(SELECT VALUE id FROM message),
   math::max(SELECT VALUE ts FROM message)];`. The schema is the one the record says, and the
   counts and the newest message are above what the store that went back holds, read the same way
   on a clone of it. Nothing else is run against it: a writer at its build's schema would move it.
7. **Stop it cleanly.** One SIGTERM to the pid you recorded, then wait for the process to end,
   never a second signal or a kill while it closes its store. At `--log info` it says `All
   memtables flushed successfully on shutdown` (the warning `No runtime available for closing the
   store correctly` is printed by every shutdown, clean ones too); its log is then in sstables, and
   the next start replays nothing.
8. **Swap, under the hold.** With the database still held, rename `symphony/tasks/data` aside
   (`data.rolled-back-DATE`) and the recovered clone to `symphony/tasks/data`, then `just tasks
   database release --as HANDLE`: the next board command starts the database on it.
   `just tasks schema` then says the board is at the schema it was seen at, and the record needs
   no acceptance. The copies kept in steps 1 and 8 and the snapshots that hold a log are the
   owner's to remove once the board is whole again ([Snapshots](#the-store-and-its-server)).

The first recovery done this way, with that day's numbers, is in
[history](history/board-went-back-2026-11-12.md).

## A board that was copied or restored

A task's number and version name it along one history of the board only. A copy of the whole
board from an older dump keeps the numbers the dump held, gives the ones made since to other
tasks, and starts every version again; so does putting a database's files back. What a program
outside the board keeps of a task to write it again later, a mapping to an issue elsewhere or the
base of an edit, is therefore bound to the board's **recovery generation**
(`crates/tasks/src/store/recovery.rs`): a text the database makes, in the record
`recovery:board`, replaced in the transaction of every copy of the whole board and never by an
ordinary write, and in no dump.

- **The token.** `Store::bound` reads a task's title and brief with its token, in one statement:
  `{"compat": 1, "generation": "…", "id": N, "version": N}`, the format's version, the board's
  generation, and the task's number and version.
- **The bound write.** `Store::amend_bound` retitles and links a task as `Store::amend` does, but
  only the task its token names: in the write's own transaction, before anything is written, it
  is refused unless the board holds the token's generation and the task is the one of that
  number at that version. A refusal writes nothing, no title, link, line on the thread or act of
  its session, and is never read again and tried on what the board holds now: the caller reads
  the task again. The task as the write left it, with its next token, is that transaction's
  answer. Every other write, `amend` among them, is as it was. `Store::edit_bound` is the same
  write for an edit that may rewrite the brief as well
  ([the api's calls](BOARD_API.md#what-a-program-outside-the-board-keeps)).
- **A copy.** `tasks-copy` and every `Store::copy` replace the generation in the transaction that
  replaces the rows, so a token read before a copy is refused after it, and one read before a
  copy that failed still holds. A build of a schema before 22 copies without replacing it, which
  is why the generation came with a schema step: such a build is refused the board.
  A copy that cannot finish leaves the board as it was and says why in one line. On PostgreSQL
  the copy is one request, which the server itself ends thirty seconds short of the wait of the
  program that sent it (270 s of the client's 300), saying the rows of each table it had copied
  and the table it was at: left to run past its caller's wait it would replace the board after
  the caller was told it failed, which only a copy whose commit and answer take those thirty
  seconds still can. On SurrealDB the copy stages its rows and swaps them in in one
  transaction, which must fit the memtable of the server it runs on: one the server does not
  commit is said with the rows and bytes it staged and the setting
  (`SURREAL_SURREALKV_MAX_MEMTABLE_SIZE`, 256 MiB on a server the board starts), and what it
  staged is removed a table at a time, or by the next copy before it stages. So a copy made
  while another stages takes the other's staging, and the other is refused at its swap, which
  first counts each staging table against the rows it sent: the later copy is the one kept. How large a board each takes is measured in
  [the store's baseline](analysis/board-store-baseline.md#a-board-ten-times-the-size-measured-2026-11-18).

The generation is a row of the database, so putting the database's files back puts it back too.
What tells that apart is the board's **recovery binding**, outside the database's data: the
generation the board is bound to, in `symphony/tasks/recovery/DATABASE.json` in the git common
dir, a file for each database of the board's server, published whole and removed to revoke. A
bound read or write is refused unless the binding names the generation the database holds, so a
board never bound, one revoked and one whose binding and database disagree all refuse. `just tasks
recovery` says which it is, `just tasks recovery revoke` binds the board to none, and `just tasks
recovery rebind` revokes, replaces the generation in one transaction and only then binds the
board to the new one. A rebind never adopts the generation it found, so one cut short anywhere
leaves the board refusing, and is run again. `revoke` and `rebind` change the project's own board
only from its main checkout; a scratch database (`--database NAME`) has a file of its own.

**Putting a database's files back**, from a snapshot or a recovered clone
([a board that went back](#a-board-that-went-back)), is in this order: `just tasks recovery revoke` first, before
the restored data is in place and served; the swap, under the hold; then, once the board is the
one to keep, `just tasks recovery rebind`. A copy made by a store that holds no binding (a
scratch program's, `tasks-copy`) leaves the binding naming the generation before it, which
refuses until a rebind. A PostgreSQL restore into a new board
([POSTGRESQL](POSTGRESQL.md#holds-snapshots-and-recovery)) revokes that board's bindings itself
before its data is in place.

The binding is this machine's file. A store is handed its authority and finds none by itself
(`Store::recovering`, the `recovery::Authority` trait): a board reached by `TASKS_SERVER` alone
has no folder here and so none, and a board several machines write needs an authority they all
read, which its deployer supplies. With none, every bound read and write is refused in words
that say so.

Two things are not told apart, and nothing here claims to: files put back without the revoke
where no copy came between, which hold the generation the binding still names; and a database
and its binding both put back to an older pair that agree.

## Board health

A board too slow to answer delivers no callback, and says nothing on the board itself, so the
Symphony server counts what it sees in memory, for each project it serves, over the last hour
(`crates/tasks/src/board_health.rs`): its clock's callback dispatch passes, ok and failed, with
the newest failure's error, kept on one line and cut to its first and last characters when it
is long (a tick that cannot reach the board at all is a failed pass); the idle
sweeps, the same way; every request any client in the process sent the board, the slowest with
the line of code that sent it (its tracked caller), the slow ones (a second or more) and the
unanswered ones; and the p50 and p90 latency of the requests that may write. Counting adds no
request. `/api/health` (`/<id>/api/health` on the one server) answers it from that memory,
without a board read. The machine's one server also writes it to `symphony/tasks/health.json` in the
git common dir (`health.<database>.json` for a scratch database; `TASKS_HEALTH` names another
file) every ten seconds, and at once when the dispatch or the sweep starts or stops failing; a
private server keeps it in memory alone. Its clock's thread and its sweep's write it one at a
time, each from its record to the rename of a file of that write's own, so a reader never meets
half a record and the last to land holds every pass. `just tasks status` prints that record
without asking the board, and `--json` prints it as `/api/health` answers it, beside the file
and the warnings. A warning stands from a failed dispatch pass, or a failed sweep, until its
passes have been good for a minute, so a board failing every other tick is warned of throughout,
not on and off; one good pass does not clear it, and the hour still counts the failure. A newest
pass that failed more than ten minutes ago, with none since, is warned of no more. While one stands,
every page of the project says so on the server's notice (beside a failed server update, when
there is one), told when it starts and when it ends, and the project picker says so under the
project's name; both read the server's memory, not the board (`apps/symphony-server/src/live/health.rs`).
A project with no board, its server answering that none is made yet or no store (`symphony/tasks/data`)
and no SurrealDB here to run one on (`symphony_board::board_health::absent`), is not unwell: no
pass of it is counted, and the picker says plainly that it has no task board. So is a project whose
folder is gone, or no longer holds the repository its server found there: the server says so once
in its log, then reads, maintains and records nothing more of it, answers its pages' reads and saves
with that refusal before any route runs, and never looks for its board in the folders above it,
another repository's (`Hub::holds_its_project`, `symphony_board::project::at`). Every connect to
its board the server's own process makes from the folder, through `symphony::taskview` (the hub's
tick, its refreshes and branch scan, its chat journal's flush, a route's read or save, the report
of a verdict's trouble and the app's session it names), is held to the repository it found when it
began to serve the folder (`symphony::taskview::serve`, `symphony_board::project::held`), and
every journal of the project's chat and asks it finds from the folder (the flush's, the asks read's,
a question's or a chat save's) is the one it found then (`symphony::asks::default_path`), so a
`.git` removed after a check leads none of them to another board or another checkout's journal.
That journal is found once, so a change of `[project] artifacts` while the server runs leaves its
routes on the old journal until it restarts, while an agent's `symphony reply` writes to the new
one. Not held, each finding its repository from the folder again, and so the checkout above a
folder left without its `.git`, starting and acting on that checkout's board:

- The cleanup a cancel starts: `tasks finish`, run in the folder, then the sweep, whose
  repository the server's own process finds from the folder by git
  (`symphony_board::sweep::trigger::trigger`), making its folder and log in that repository's
  common dir and starting the sweep from its main checkout. `tasks finish` starts milliseconds
  after the cancel's save, which is held; the sweep only once `tasks finish` has returned, so its
  window is that command's whole run.
- The idle-session reap, a `tasks reap` run in the folder milliseconds after its round's check
  that the folder holds its project.
- A verdict's commit, after its worktree's commit: the record of that commit (`common_dir` by git
  from the folder, a file written in that repository's record folder) and its `tasks` run in the
  folder for the app's session (`sessions`, then `register --kind app`), kept a run of the
  project's own build of `tasks` so that a board one schema ahead of the server still takes the
  app's registration. The commit's git in the folder walks up before either, so a held store
  would not hold the flow; once its worktree is made, their window is the gap from the worktree's
  `rev-parse HEAD`, as the worktree's git fails once the project's `.git`, which holds its git
  dir, is gone.

Only the machine's shared installation runs the cancel's cleanup and the reap. Each matters only
for a project the server serves whose folder sits inside another git checkout; one in no checkout
finds no repository above it, and no board. A board's
start, stop or hold makes no folder at or above the repository's common dir again; a start whose board folder goes while its server starts (SurrealDB makes every missing
folder of its store's path) ends that server and records none. A board whose store is there is never no board: when the SurrealDB
that serves it is missing (a version bump before the new binary is fetched, `SYMPHONY_SURREAL` naming a file that is not there), or its server answers with an
older schema or not at all, its passes fail and it is unwell and says so.
`tasks status` also warns when the record is more than a minute old, naming the server's pid and
whether it still runs: no dispatch pass has been recorded since.

**Open tasks nobody orchestrates.** A task with no orchestrator waits for one under its limit
that stays ([Tasks and roles](TASKS.md#tasks-and-roles)), a well state and no defect: the record counts the
unfinished tasks with no holding orchestrator on them or above them in the board the server's clock
read last (`unowned` in the record, `Recorder::unowned`), and `tasks status` says `open tasks no
orchestrator holds: N, as of the newest board read`, and warns of none of them. The tick gives what
it finds first ([Tasks and roles](TASKS.md#tasks-and-roles)), so the count it records is of tasks that wait
for an orchestrator that stays and holds fewer open tasks than the limit ([The limit on the tasks
an orchestrator holds](TASKS.md#the-limit-on-the-tasks-an-orchestrator-holds)). Counting reads the board
the tick already read and adds no request; a
record written before any board read, or by a build older than the count, has none and says none. The owner's word on each of those tasks reaches
every orchestrator ([Talking on a task](TASKS.md#talking-on-a-task)) until the board gives it one. The
server's own notice and the project picker do not warn of it; the picker says only that work
waiting to start has no live orchestrator (below).

**On the project picker.** The machine's project picker (`/`) draws a dot before each project's
name, its board's worst signal, and, only while the dot is not green, one line under its counts in
the dot's colour saying why; `/api/projects` sends that line as the project's `unwell`. One
function judges both (`Hub::standing`, `apps/symphony-server/src/live/health.rs`), from the
server's memory alone: drawing it reads no board. Red is every page's notice above, in its words
and alone: a dispatch or sweep failing, a board that went back, one replaying its log. Amber is a
board slow over the last hour, its writes at a p90 of a second or more (`SLOW_MS`, the record's
slow request) or any request unanswered in it, said with the figures that say so, `Board slow
(writes 3 s, 48 unanswered this hour)`, the p90 to the nearest whole second; or work waiting to
start (the listing's `unstarted`: ready tasks no executor that is there holds, nothing open before
them) while no orchestrator of an open task is live, by the board's one rule for a session that is
there: `orchestrator stale` while one is stale or its presence not yet swept, `no orchestrator`
when none is. Both are one line, joined by ` · `. Every other board is green, one with no task
board among them; a project refused, or whose server state is not up yet, has no dot. Its counts
say what they count: `N decision cards waiting on you`, the owner's cards, then in bold `N blocked,
M of them waiting on your decision`: the tasks, not their subtasks, blocked on the wire, a block
that names nothing never among them, and those of them that wait on a decision of the owner's
([What a task waits on](TASKS.md#what-a-task-waits-on)).

**Its hours.** The machine's one server also appends each hour of it, as the hour ends, as one
line (a `symphony_api::HealthHour`) to the day file of the hour's UTC day in `symphony/health/` in
the git common dir (`YYYY-MM-DD.jsonl`; `symphony/health.<database>/` for a scratch database,
`SYMPHONY_HEALTH` names another folder, and a board `TASKS_SERVER` names keeps none without it;
`crates/tasks/src/board_health/hours.rs`): the hour's dispatch passes and sweeps, ok and failed;
its requests, the slow and the unanswered ones, and the slowest with its caller; and its writes
with their p50 and p90. Its `start` is the hour's first second, as the agents' hour lines'
([AGENT_ACTIVITY](AGENT_ACTIVITY.md#the-day-files)) and the hourly roll-up's hours are keyed, so
the three join on it. The line is read from the minutes the server holds, the newest 120 that had a
pass or a request however long ago, so the append asks the board nothing. A line counts whole
minutes: a server that starts or stops mid-hour writes the part it recorded, from the first whole
minute after its start to its stop, marked `partial`; an hour no server ran in has no line, never
one of zeros, and nor has a project with no board. A machine asleep is told by its wall clock
running more than a minute ahead of the one that stops while it sleeps, not by how long a tick took
(a board slow to answer holds one for minutes, each request up to 300 s): its line ends with the
minute it fell asleep in, written once it wakes however long it slept, and goes on from the minute
it woke in. A span whose minutes the server let go before writing it (uncalled for two hours of
requests) has no line rather than one that counts short. Each line names its writer, its `pid` and
its `generation` (when it began recording), and no two lines of a project count the same minute:
the server generation's one gate, shared with the agents' recorder
([AGENT_ACTIVITY](AGENT_ACTIVITY.md#a-reloads-hand-off)), closes just before a reload's hand-off
commits, so the old generation's lines end where the minute it closed in begins, and the
replacement's begin at the first whole minute after the commit, its own warm-up counted in none.
Day files more than 90 days old go, once a day. A private server keeps none.

**A board that went back** below the schema it was seen at ([A board that went
back](#a-board-that-went-back)) is said in the same places, in its own words and in place of the
dispatch's. When the server's store for the board is refused as one that went back, every page's
notice and the picker say the board is unwell: that it went back from schema N to M, so what was
written since is lost, that nothing was written, not to run `just tasks migrate`, and that that
section says how to recover it (`crates/tasks/src/live/seen.rs`, `Rollback::line_in`). The record
holds that line as `rollback`, written at once when it starts and when it ends, and `tasks status`
warns of it first. The next store that opens the board, accepted or recovered, clears it.

**A board replaying its log** cannot be reached until the start replaying it is through, the
server's own, run apart from its reads, or another's it waited on ([the store and its
server](#the-store-and-its-server)). While the server's reach of the board is refused for one, every page's
notice and the picker say, in place of the dispatch's words, that the board is starting: its
store's log, of the size the start measured, is replayed in a private server before the board's
server starts (a log of gigabytes takes minutes), and the board answers once that is through
(`symphony_board::server::replaying_in`). Each such tick is a failed dispatch pass all the same,
which `tasks status` says with the refusal; a reach that finds no replay, the board's server up or
any other refusal, ends it.

**A disk running full** is warned of in the same two places: `tasks status` when the disk holding
the record has less free than `SYMPHONY_LOW_DISK_GB` gigabytes (20 when unset; `0` turns it off, and a
value that is no number is said and 20 taken), and every page of a project while the disk
holding its checkout has, read by the server each time it tells the pages of its health; the page
says under how many gigabytes, not how many, so it is told when the disk goes low and when it is
freed, not at every build's write. A full disk fails every build on the machine and can corrupt
the board's database; the warning names `just tasks sweep`, which
clears the build output of finished worktrees ([TASKS](TASKS.md#sessions)). `--json` adds `disk`: the folder read, its
free bytes and the threshold (`crates/tasks/src/board_health/disk.rs`).
