# Task board

The standalone workspace provides the generic board and document commands, and
the default landing, `just land SLUG` ([the default landing](#the-default-landing)),
which `symphony init` installs in every project. Verification, certificates and the
landing/stamp recipes in the workflow examples are supplied by a consuming project,
whose own `land` recipe replaces the default everywhere for it; no project gate
executable or stamp backend is bundled here. See the workspace [command ownership](../AGENTS.md#command-ownership).

How several agent sessions share work, and how they, and the owner, talk
about it. Sessions may run in any worktree of this repository; the board is the
same for all of them.


Commands shown use the consuming project's recipes and board connection.

## Where it lives

A SurrealDB database (namespace `sf`, database `tasks`) whose server keeps its
data, its address and its log in the git common dir, `.git/symphony/tasks/`, so every
worktree reaches the same board, a claim is one transaction, and none of it
shows up in `git status`. Whatever needs the board starts the server when it is
not running: the first `just tasks`, `just symphony` as it starts, a page of the
Symphony. It starts detached, so it outlives whoever started it, and that is
also the restart: a server that was killed is started again on the same data.
The heartbeat hook never starts it. `TASKS_SERVER` (an address) and
`TASKS_DATABASE` reach another board, as the tests do. It is working state, not
history: `just tasks export` writes a readable snapshot to
`docs/history/tasks.md` when one is worth keeping.

Each project has a board of its own. A run's project is named by the nearest
`symphony.toml` from its folder up, never past the top of the checkout it is in;
with none, by the machine registry's entry for its repository's main checkout,
else by that checkout's folder name. Its board is that repository's, in its
common dir, under the same namespace and database. The Symphony serves each
registered project's board, and reconnects to one whose server restarted. A
project's heartbeat keys, sweep state and staffing lock stay in its common dir.

Each build of the board writes one schema version, which the board records. A
build opens a board at its own version with one read and no write. A newer
build migrates an older board forward once, its additive steps and the new
version in one transaction before any other write. An older build is refused
before it writes, naming the project and `just symphony`. A Symphony one version
behind still reads the board, and every write it would send is refused the same
way. A store opened before a newer build migrated is refused at its next write.
The Symphony never moves a board forward or makes one, so a branch's command line
or preview server with a newer schema leaves the live board as it is: a board
behind its build, or not made yet, it refuses before it reads, naming `just tasks
migrate` in the project's main checkout. A branch tries a newer schema on a
scratch board (`TASKS_DATABASE=NAME`), which its own `just tasks --database NAME`
moves.

A project's live board, the `tasks` database on the server of the data in its
common dir, however a run reaches it (by an address, it is known by the server
listening there, from any folder), moves forward only by a build its default
branch says is its own, by what that branch commits: never what a checkout
holds, nor the branch the main checkout has out. The default branch is
`[project] default_branch` in the main checkout's `symphony.toml`, else `main`.
Where it commits the board's schema source (`crates/tasks/src/schema.rs`, as this
repository does), that is any command of a build of that very file, byte for
byte, wherever it was built: main's build moves the board at its first command
once a schema lands, and only such a build makes the board when it is not made
yet. Where it commits none (a consuming project, which pins a build), it is only
`just tasks migrate` run in the main checkout, which says whether it made the
board, moved it from one version to the next, or found nothing to move; there
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
went back as any other), and a raw read (`search_bench`'s) opens no store. Otherwise the store
adds the version it found to the record when it is higher, and the one it migrated to. A scratch
board keeps no record, nor does a server an address names that is no project's board server.
Builds from before the record neither read nor write it: the protection holds for a project only
once each of its readers and writers runs a build that carries it, the main checkout's
`.target/bin/tasks`, the shared Symphony server and a consuming project's host.

The store takes about what it holds. Every server the board starts keeps each
value beside its key, where compaction drops the versions a rewrite leaves, and
logs at most 256 MiB of writes before it moves them to an sstable. SurrealKV's
default had sent every value over 4 KiB to a value log that only grows: 102 GiB
held a board of 0.5 GiB. The `tasks-reclaim` binary, the owner's to run from the main checkout,
rebuilds a store that grew that way. It stops the server and keeps the store as a
[snapshot](#where-it-lives) before any server of its own starts on it: a clone, a plain copy where
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
flush, as an older build's kill of a starting server does) holds a log segment
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

The command is the `tasks` binary (`crates/tasks`). In a project `symphony init`
set up, `just tasks` is the recipe it installs in `symphony.just`: the project's
build linked at `.target/bin/tasks`, else, from a linked worktree, the main
checkout's link, else `tasks` on `PATH`; it builds nothing, and says which link
to make when it finds none. The Symphony's own board clients find it the same
way. This repository's `justfile` has a `tasks` recipe of its own, which wins:
`tools/rs-bin.sh` builds the binary before every run, so the first command after
a change to its sources takes a minute or two, later ones only Cargo's check
that nothing changed. On the live board it is the main checkout's build, from
whichever worktree it runs; a branch's own build runs only on a scratch board
(`--database NAME`). The heartbeat hook finds the build as the recipe does, and
builds nothing. The sweeps work finishing starts, and the Symphony's reap and
finish, run the main checkout's build found that way, never a linked worktree's.
There is no Python board adapter.
`symphony init` allows the recipe unasked, so it refuses the recorder
([MACHINE_WORK](MACHINE_WORK.md#recording-builds-and-tests)) and, like this
repository's own, marks what it runs (`SYMPHONY_UNASKED`): under the mark
`just tasks export --out PATH` writes only into the checkout it runs in or, from a
linked worktree its repository lists, the main checkout's artifacts folder, never
into a git folder, with the path's links and `..` resolved, and replaces only an
earlier snapshot; it refuses any other place, or a file of the project's own,
with one line naming it, writing nothing (`crates/tasks/src/confine.rs`). The
reply recipes keep their writes to the same places (AGENTS.md, "Agent
integration"). The binary run by itself, which no rule allows, writes wherever it
is told. The mark is the one command's: no child the board starts keeps it
(`sys::MARKS`), so the review command, a land's `on_land` and the regenerator write
where the project says.
`launch`, `restore`, `cleanup`, `close` and `reap` are its own, hc's terminals and all
(`crates/tasks/src/launch/`); the board calls no `herdr`. The server is the pinned
SurrealDB at `symphony/surreal/VERSION/surreal` in the git common dir, or the binary
`SYMPHONY_SURREAL` names. In this repository `bash tools/surreal.sh` fetches it there,
checked against its pinned SHA-256 (the Rust tests fetch it the same way). A
project fetches the same tarball there, checked against the same SHA-256, by the
line `symphony init` lists among the steps left, which the board also says when
it is missing; a machine no build is pinned for puts SurrealDB's release binary
there.

| | |
|---|---|
| `just tasks …` | everything an agent does: `just tasks help` lists every command with a line on what it does, `just tasks help COMMAND` its arguments; a mistyped command, value or slug names the nearest, a mistyped flag the line to type, and a refused move or role the command that does it |
| `just tasks list [--all] [--mine] [--status S]` | the open tasks as a tree; `--all` adds the done and cancelled ones |
| `just tasks search QUERY [--status S] [--limit N] [--json]` | ranked full-text matches in task titles, briefs, slugs and threads; task rows with matching excerpts (default: 20); query capped at 1024 UTF-8 bytes and 32 terms (word/number runs and punctuation/symbols, after lowercasing) |
| `just tasks stats [--days N] [--json]` | where the pipeline's time goes: lead time, time in each status, bounces, tasks done per day and what sits longest ([Where the time goes](#where-the-time-goes)) |
| `just tasks status [--json]` | the board's health over the last hour, as its Symphony server recorded it: failed dispatches and sweeps, the slowest request, write latency ([Board health](#board-health)) |
| `just tasks asks [--all] [--json]` | what the owner waits on an agent for: the open asks, oldest first, each with its age and who holds it; `--all` adds those answered in the last day ([Asks](#asks)) |
| `just tasks asks close ID "WHY" --as HANDLE` | close an ask that needs no answer, saying why |
| `just tasks migrate` | move the board forward to this build's schema, or make it, and say which; a live board moves by it only from its main checkout (any checkout of a repository with none), and in a project whose main checkout holds no board source, or that has none, only by it |
| `just tasks database [hold WHY \| release [--of HOLDER]] [--as HANDLE]` | the board's database: whether it is held, what runs on its data, what a start or replay under way does, and the snapshots kept until the owner removes them; `hold` stops its server cleanly and keeps every board command and the Symphony from starting it, naming the hold, until `release` by its holder (or by another naming it with `--of`), which starts nothing |
| `just tasks schema [--accept VERSION]` | the board's schema and the highest its live board was seen at; exit 1, in the rollback's words, while it went back below it; `--accept VERSION` (`none` for a store with no board), the owner's, accepts an older board they meant ([A board that went back](#a-board-that-went-back)) |
| `just tasks snapshot [--list]` | a copy-on-write clone of the board's store, taken only while no server has it open; one whose write-ahead log holds writes is kept until the owner removes it, and the newest three of the others; `--list` names them ([Snapshots](#where-it-lives)) |
| `just tasks callback [ADDRESS] [--chat] [--if-free] [--off] --as HANDLE` | register a native channel; addressed events start a turn directly (`just tasks help callback`) |
| `just tasks callback --session-url URL --as HANDLE` | give the session the web address its Go to session control links to; empty clears it ([Going to a session](#going-to-a-session)) |
| `just tasks pull --as HANDLE` | the whole PULL digest as HANDLE hears it now; a callback carries only what changed since the last PULL it received |
| `just tasks wait --as HANDLE [--timeout S]` | for a session with no channel: exits printing what a callback would have pushed it since its last read, once there is any; run it as a background command and again when it exits |
| `just agent-send HANDLE "text" [--as SENDER]` | message an agent directly through its registered channel |
| `just symphony` → **Board**, **Agents** | the board, each task's page and thread, and who is connected |

### Stuck servers

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

### A board that went back

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
   ([the record](#where-it-lives) says which do). Never start anything on that copy: each step
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
   owner's to remove once the board is whole again ([Snapshots](#where-it-lives)).

The first recovery done this way, with that day's numbers, is in
[history](history/board-went-back-2026-11-12.md).

## Sessions

Every agent that works from the board has a **handle** (`amber-heron`), a
kind (`orchestrator`, `worker`, `reviewer`) and, for a subagent, a parent. The
handle is printed by `just tasks register`; later commands take it from
`--as`, else `TASKS_SESSION`. An agent writes it as `just tasks … --as HANDLE`:
no allow rule passes a command with an env prefix, so
`TASKS_SESSION=HANDLE just tasks …` stops at a prompt, while a session launched
with the variable already set needs neither. Then run `just tasks callback --as HANDLE`: it detects
the Claude inbox socket or Codex thread. An explicit `uds:/absolute/socket` or
`codex:THREAD-UUID` works too. A session with no channel is listed as such and
keeps the inbox fallback. hc terminals are for inspection, never delivery.

`just agent-send HANDLE "text"` addresses an agent without a task. The sender
comes from `TASKS_SESSION` or `--as`; delivery is one plain
`AGENT from SENDER to HANDLE — text` event line. Reply with
`just agent-send SENDER "text" --as HANDLE`. Whitespace is flattened as in board callbacks.
It uses the callback outbox, identity checks, retries and inbox fallback. Output
distinguishes sent from queued; sent confirms native transport only. An agent
without a channel is refused with the command to register one and the option
to send on a task with `just tasks say SLUG TEXT --to HANDLE` for its inbox.
The Agents page shows each native address and the command to reach it. Scratch
boards use `just agent-send --database NAME HANDLE "text" --as SENDER`.

`just tasks callback --chat` registers the app's chat recipient. Chat selects
the explicit seat, then an `--if-free` fallback, then a live orchestrator with
a native callback, choosing the lexicographically smallest handle among plain
orchestrators. A normal `just tasks callback --as HANDLE` is enough for that
last fallback; `--chat --if-free` opts into the higher-priority fallback without
displacing the seat. Sweep-confirmed native presence counts as live. A recipient
suspended after delivery failures waits for the sweep to restore it. While no
channel holds the seat, an orchestrator reading by `just tasks wait` holds it:
a session is waiting from its first `wait` until it registers a channel (or
turns one off) or leaves, and of the live waiting orchestrators the smallest
handle receives chat at its next read; one that stops waiting holds it while it
stays live, so what the owner says waits for that read. While nobody holds the
seat, every orchestrator's `just tasks pull` ends with what the owner said that
nobody received, and its `just tasks inbox` takes it, after which nobody reads
it again: an owner's word never goes to no one unseen.
The app names the recipient, and says when nobody receives chat. Saves queue
asks, follow-ups, card messages, verdict notes and callouts directly; an outbox
beside the asks preserves them across a board outage. No file watcher discovers
chat. `--off` or leaving the board unregisters delivery.

Board writes dispatch addressed messages and role/status/new-work changes. A
command line's write queues what each recipient hears, as the write left the
board, and answers without delivering it: a detached `tasks dispatch` delivers,
one a host at a time, which makes one more pass for each write that commits
while it runs. One that finds another running waits until that pass has ended,
and takes over if the other's lease lapses first.
The shared Symphony server schedules PULL digests and STALE/LOST transitions and
retries failed deliveries. Per-recipient leases prevent concurrent writers
from sending the same queued event. A successful native send acknowledges the
inbox; failure retries three times, is recorded on the task thread, and retains
the event in the inbox. As with any external queue, a process crash after the
native send but before acknowledgement can repeat delivery; messages carry
stable board ids. No normal agent event remains a stream; the browser's live
feed remains a stream because it updates the owner's page.

### Board health

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
pass of it is counted, and the picker says plainly that it has no task board. A board whose store
is there is never no board: when the SurrealDB that serves it is missing (a version bump before the
new binary is fetched, `SYMPHONY_SURREAL` naming a file that is not there), or its server answers with an
older schema or not at all, its passes fail and it is unwell and says so.
`tasks status` also warns when the record is more than a minute old, naming the server's pid and
whether it still runs: no dispatch pass has been recorded since.

**Open tasks nobody orchestrates.** Every open task has an orchestrator
([Tasks and roles](#tasks-and-roles)), so one with none is a defect: the record also counts the
unfinished tasks with no holding orchestrator on them or above them in the board the server's clock
read last (`unowned` in the record, `Recorder::unowned`), and `tasks status` says `open tasks no
orchestrator holds: N, as of the newest board read` and, while N is above zero, warns of it, in its
`--json` warnings too: `N open tasks no orchestrator holds, as of the newest board read: the next
read of the whole board gives each to an orchestrator that stays (live past its first live line, and
waiting, watching or holding a task), and it waits while none does`. The tick gives what it finds
first ([Tasks and roles](#tasks-and-roles)), so the count it records is of tasks that wait for an
orchestrator that stays. Counting reads the board the tick already read and adds no request; a
record written before any board read, or by a build older than the count, has none and says none. The owner's word on each of those tasks reaches
every orchestrator ([Talking on a task](#talking-on-a-task)) until the board gives it one. The
server's own notice and the project picker do not warn of it.

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
server's own, run apart from its reads, or another's it waited on ([Where it
lives](#where-it-lives)). While the server's reach of the board is refused for one, every page's
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
clears the build output of finished worktrees (below). `--json` adds `disk`: the folder read, its
free bytes and the threshold (`crates/tasks/src/board_health/disk.rs`).

The Claude wire envelope is newline-delimited
`{"type":"user","message":{"role":"user","content":"…"}}`, observed in the
installed Claude Code 2.1.308 socket example. Its body format is undocumented;
this is the compatibility assumption. On macOS the board sends it without
authentication and never reads the session's messaging token. Claude's inbound
controls can hold or refuse it; a successful socket write confirms transport,
not that Claude accepted the message. No project inbound setting is changed.
Codex delivery uses the installed `codex queue --thread UUID --message TEXT`.
For isolated runs use `tasks --database NAME ...` and
`symphony-server --scratch NAME --port N --root ROOT`; the latter keeps
its asks in `artifacts/callbacks-NAME/asks.db`.

**Agent sessions behind a task** (`task_sessions`): `just tasks launch SLUG` starts the
agent the project's `[agents] launch` in its main checkout's `symphony.toml` names (`default`,
`codex` or `claude`, with its `model` and `effort`), on the model `[agents.models]` names for
its role (`executor`, or `reviewer` under `--role reviewer`) when there is one
([Limits and models](#limits-and-models)), else a Codex agent (`--no-daemon`; model
`gpt-6.1-sol`, effort `xhigh`, workspace-write with network access), in an hc terminal of its
own, `hc run --bg --no-parent --outlive-parent --keep`: no child of the caller's terminal,
which the app would show under it rather than in a tab. `--kind` and `--model` win over the
project's; its models and effort go with its agent, so `--kind` naming the other agent takes the
board's model for that one (Codex `gpt-6.1-sol` at `xhigh`, Claude `sonnet` at its own effort).
A table that does not parse, or names another agent, refuses the launch, and so does a machine
running as many agents as `[limits] machine_agents` allows. The `launched` line
names the agent, model and effort it started, and `launch --help` ends with what a bare launch
starts where it runs (`crates/tasks/src/launch/defaults.rs`). The
terminal shows in its stream tab (`code` for an executor, `review` for a reviewer, or
`--tab` such as `3d` or `infra`) of the project's space in the Herdr app, found by name and
made when missing (the labels `herdr.space` and `herdr.tab`); the owner's tabs and other
projects' terminals are never a destination. Once hc calls the agent ready (`idle` or
`done`), its first message goes in by `hc send`, never forced, and the launch is confirmed
only by that send's receipt (`started`): an agent waiting on a person (a folder-trust
dialog), never ready, or not starting on the message fails the launch naming the terminal,
which is killed and removed. It writes its handle, role, agent session
id, worktree, branch, terminal and start time; `just tasks show` lists them under
`Sessions:`. No terminal the owner may be looking at is closed: one in the tab the Herdr
app shows, the agent it shows, or held over them (`herdr-app status`, read last, right
before the kill, after a last `hc get` that must still find it finished, running the program
cleanup judged: the same pid and foreground); when the window cannot say or the terminal
works again, it stays and cleanup names why. A shell closes only with the shell itself in
its foreground: a job it runs in front, or one it became by `exec`, is work. When a terminal is closed (`just
tasks cleanup`, or the task becoming done or cancelled: `hc kill`, then `hc rm`) the
record stays and says why and how to bring it back,
`just tasks restore SLUG [HANDLE]`, which resumes the conversation in the same
worktree, in a new terminal. A record from before hc names a herdr pane (`w1:p…`): that
terminal is gone, so cleanup records it closed with its restore line, and restore brings
the agent back in an hc terminal. `just tasks close SLUG HANDLE` closes one agent's terminal
whatever role it holds (a reviewer waiting for a new tip), records it the same way once the
terminal is killed, bound to that terminal, and retires the callback registered from it; it is
refused for the caller's own session or terminal, one the owner may be looking at, and one whose
program is not at rest (work under it as the idle sweep reads it, any test run included, or
another program in its foreground). A close and a restore of one session each hold it while they
run, so neither starts while the other runs; one whose hold lapsed and was taken over writes
nothing more, and a restore that lost it kills the terminal it started. A Codex record
takes the thread its callback registered whichever is written first; one closed with no thread
says on its thread that restore cannot resume it. See the [tasks skill](../crates/symphony/shipped/skills/tasks/SKILL.md#launch-restore-clean-up-herdr).

A consuming project chooses its agent models (`[agents.models]`), staffing policy and terminal
lifecycle. The board records those sessions and roles; it imposes no model preference.

**Optional automatic staffing:** `just tasks staff [--dry-run]` makes one pass, highest
priority first. Enable it in `tools/task_staff.json` in the consuming checkout: `enabled`
defaults to false, `max_sessions` to 8 and `min_free_memory_gib` to 4. `only` is an
optional list of task slugs whose subtrees may be staffed; an empty list allows all
orchestrated work. The config belongs to the checkout running the command (main for
the Symphony). `reap` runs staffing before its five-minute idle sweep. A shared
`symphony/tasks/staff.lock` keeps concurrent staffing passes from dispatching twice. A review
goes to an idle reviewer only when the board would take its verdict, by the rule the
verdict holds (`Held::unfresh`, [the default landing](#the-default-landing)), and that
reviewer takes the role before its job is reserved; a review job no agent was handed is
cancelled, and the reviewer's role a reuse took for it let go.

`reap` also recovers missing agents with addressed work left unanswered for thirty
minutes, even when staffing is disabled. It resumes a recorded conversation once
per handle/post, after the hc terminal inventory confirms its absence, and posts the
restore on the task thread. With no recorded conversation it only flags the work;
it never starts a fresh conversation on its own. A failed automatic attempt stays
flagged for manual action. `reap --dry-run` starts no recovery.

Only ready tasks with dependencies done and no executor or reservation qualify;
review tasks with no reviewer get one breakage-only reviewer through an instruction
child. A land-request task's opening `Review` line records the branch under review;
an explicit `Branch:` or `Worktree ..., branch ...` line does too. Otherwise reviews
use the executor's recorded branch, falling back to the task slug.
Dry runs resolve that branch and show its tip too. A task whose target or dispatch
fails prints its failure and leaves later tasks eligible; staffing errors never
prevent the idle sweep. Work already staffed through a child is left alone. A live orchestrator on
the task or an ancestor is required. Owner-only work is excluded by `owner-only`,
`owner only`, `owner tier`, `sign-off`, `signoff` or `bless` in its slug/title, or a body
line beginning `Owner-only:`, `Owner only:`, `Tier: owner` or `Review tier: owner`.
Use these explicit markers for owner decisions.

Before launching, the pass prompts an idle worker/reviewer holding no unfinished
work in a suitable open terminal: an executor already on the task's own branch, or an
instruction worker/reviewer in a detached checkout. A checkout with a live or stale
executor is never reused. Otherwise it launches what a bare `just tasks launch` of the role
would (the project's `[agents] launch` on the role's `[agents.models]` model, else Codex) in a
separate worktree: the task's own branch, or detached main for an instruction or review. Live
agent terminals and live board agent sessions without a terminal count toward the cap, the
lower of `max_sessions` and `[limits] machine_agents`; reuse adds no session. Memory below the
floor defers work. Every proposal, dispatch, deferral and failure prints one line. Only a successful ping or launch is posted on
the task; deferrals and dry runs post nothing. A dry run previews even while
disabled and creates no task, session or terminal.

**Idle agents are closed by the board, not by a reminder.** A task
that becomes done or cancelled closes its sessions at once; the rest is a sweep the
Symphony's process (`just symphony`) runs every five minutes for each project it serves that
has a board, each on a schedule of its own in its own checkout, started as the project is
registered and stopped as it is removed, so one project's failing or hung sweep holds up no
other's; each writes its own `reap.json` and its own board's health, which `just tasks status`
reads. A lock in `symphony/tasks/`, beside the board's server, keeps two sweeps (a second
app's, or `just tasks reap` by hand) from running at once, and nothing goes into any monitor.
It closes (`hc kill`,
then `hc rm`) one of this board's terminals (label `symphony.board`) whose named agent was
idle (or stuck on the folder-trust prompt) at two sweeps at least five minutes apart,
however many apps sweep, when neither its handle nor a subagent session under it holds
an executor role on an in-progress, blocked or review task, a reviewer role on a task in
review, an unanswered addressed post, or a reviewer awaiting re-review after changes
returned its task to in-progress. That last keep includes landing changes messages,
on the branch task or any numbered review task. A pending post keeps the terminal
immediately, before the thirty-minute recovery threshold. A post, status move or verdict
by its addressed handle answers earlier work; reading its inbox does not.
Nothing runs under it but MCP helpers (Codex's configured `node_repl` and the ChatGPT
app's `cua-repl.mjs` under its own `node` among them, each by its exact path, and
Codex's `codex-code-mode-host` in a standalone release's `bin` folder), caffeinate and
board commands: anything else is work, a `codex exec` job included. The roles above
keep it for an hour at most, since only a session doing work, or about to within the hour, stays
open: idle at its
prompt that long, from its last tool call or the last change of its role or task
status, it closes saying what brings it back; only an unanswered addressed post keeps
it longer. A terminal whose program ended is removed at the first sweep, the open
records naming it closed first with their restore line.
Status, roles and the window are read again right before the close, and the terminal and the window once
more after the close is recorded, right before the kill: work resumed by then (its agent
working, or work under it) or the window showing it reopens the record. Never an unnamed terminal or another board's (it may be the owner's), an
orchestrator or a chat, the caller's (`HC_TERMINAL_ID` or its session), or one on screen in the Herdr app, which
starts its idle wait again (as does a window that cannot say what it shows).
Each close is one line on the handle's task thread, with its restore line, written
before the terminal is closed. `just tasks reap --dry-run` names what the next sweep would close
and why each terminal stays; a sweep tells its log why it keeps an idle one, once per
reason. `just tasks clean` runs cleanup and one sweep at once, never closing the caller's.
Code: `crates/tasks/src/launch/reap.rs`.

**Finished worktrees go by themselves**, without asking, so they hold no disk once their tasks
are complete. The worktree sweep (`just tasks sweep`, the rules in
the consuming project's worktree policy) starts, detached, whenever work
finishes: a task going to done or cancelled (its closed sessions' worktrees then go without
the 2 h wait), `just tasks leave`, `just tasks reap`, and at most once an hour `just tasks
inbox`, by a stamp. The command that finished the work never waits on it and says nothing of
it; each sweep runs from the main checkout, by the main checkout's build of `tasks` (never
the caller's, whose sweep may not have landed; with none built it says so and sweeps nothing),
and appends what it did to `symphony/tasks/sweep.log` in the git common dir. Status writes through
`tasks api` (for example, a release run or reap caller writing status) do not start this CLI
trigger; the next hourly sweep covers them. The Symphony's explicit finish action starts its own
sweep.
A worktree goes, locked or not, when its HEAD is in main (detached or on a branch), nothing in
it did not land, no live work holds it and its lock holds nobody; the branch it had checked out
goes with it once merged, if no other worktree holds it and it still points where it did. Two
things keep that branch, each said as `kept branch NAME: WHY`: a land's record of its merge
naming it, as `just land SLUG` run again finishes an interrupted land from that record and the
branch, in a worktree of its own; and its task done by an approve no landing has closed, as what
keeps such a task from going back to review once it is on main is its branch's tip.
Nothing did not land when it has no tracked edit, no untracked file and no ignored file but
build output and caches, and holds no owner verdict no commit has. No live work holds it when no
live or stale session is bound to it (a subagent's session is bound to its own `agent-ID`
worktree as well), no live task is tied to it (its branch names the task's slug, or the slug
with its last `/` as `-`), no process works inside it and nothing touched it within the 2 h
window. Its lock holds nobody when it has none, or names a process that is gone: Claude Code's
(`claude agent agent-X (pid N start …)`) once N is not running or runs with another start, a
land's (`symphony land: SLUG (pid N, started S)`) once N is not the process that started at S
(or, with no start, once N is not running). Claude Code's lock names the
session that started the agent, its orchestrator, which outlives every agent it started, so a
lock whose process runs says nothing of the agent; the board says the agent finished when a
session is bound to the worktree (by its worktree, or as the agent the worktree's name and the
lock give), none of those sessions is live or stale, and every task tied to the worktree, at
least one, is done or cancelled. A live agent's worktree is never touched: its session, live or
stale, keeps it, and so does a lock the board cannot answer for (an agent it knows nothing of, a
land that runs, the release run's checkout, any other reason, a start it cannot read). Every
worktree git lists is the sweep's, the `../symphony-wt/*` trees the land and the release run make
included: a land's goes by the same rule once its process is gone; the release run's checkout,
which it resets and builds on each day, stays. A worktree with tracked edits or commits not
in main is never removed, whatever its task's stage: an orchestrator decides. What git ignores
in a finished worktree's artifacts folder (the project's `[project] artifacts`, `artifacts/`
when it names none: the pictures its thread names, a test's leftovers), less its build output, is
moved into the main checkout's `FOLDER/swept/NAME/` right before the worktree goes, whatever came
there up to then, and back if git refuses the removal; while a task tied to it is open, it keeps
the worktree. A move the sweep cannot make (the folder on another disk, a name there taken, even a
moment before: the rename itself refuses it) keeps the worktree and says why, and so does an entry
there the sweep cannot read, which would otherwise go with the worktree: evidence is moved, never
copied, replaced or removed. Nothing empties
`FOLDER/swept/` (below). The pass a land
runs for its task visits, without the window, the worktrees the task's closed launches ran in,
those its executors' and reviewers' sessions are bound to once none of those is live or stale (an
in-process agent has no launch record), and those on the task's branch (an agent that left has
released its roles). Before each removal the sweep refreshes
sessions, launches, locks and activity and searches that tree for process working directories;
a lock it lifts goes back if the removal is refused. A detached agent's merged
`worktree-agent-ID` branch stays while its live or stale session still binds its worktree.
In-process agents work only in their own `agent-ID` worktree; a different directory they create
is not bound by their agent id. Full sessions register from the worktree they use. Each removal
says what the worktree held on disk, and the summary what the sweep freed; a sweep started
detached (a land's, work finishing's) writes them to `symphony/tasks/sweep.log` under a line saying when
and why.
A board `TASKS_SERVER` names (a test's) starts no sweep unless
`TASKS_SWEEP=on`; `TASKS_SWEEP=off` starts none anywhere. Code: `crates/tasks/src/sweep.rs`.

**What the sweep sets aside stays, its build output never, and past a cap the sweep asks for a
check by hand.** A finished worktree's pictures and other artifacts are kept in the main checkout
and its build output is not; past a size cap, agents are asked to check by hand and remove what
nothing uses any more ([its card](DECISIONS.md#sweep-artifacts-set-aside)). What is set aside
stays in the main checkout's artifacts folder, which
git ignores, and nothing commits it; attaching it to the repository without a commit on main is a
design of its own. Build output in a worktree's artifacts folder (a folder tagged as a cache by a
`CACHEDIR.TAG` that begins with the convention's signature, `Signature:
8a477f597d28d172789f06886806bc55`, as Cargo writes it, a file of that name without it tagging
nothing; or a file of compiled code, known by its first bytes: ELF, Mach-O, wasm, an archive) is
never set aside and goes with the worktree. Nothing removes what
was set aside, ever: past its cap, `[board] swept_cap_gb` in `symphony.toml`, in gigabytes (5 when
unwritten; a value that is no number over 0 is said and 5 taken), the
sweep still sets aside and refuses nothing. Each sweep that may set aside measures `FOLDER/swept/`
and keeps the measure in `symphony/tasks/swept.json` in the git common dir, beside the tasks tied to each
folder's worktree when it was set aside. Past the cap, the sweep's output (the log's, for the
sweep a land starts) and a dry run's say so in one line, `set aside in PATH: SIZE, over its CAP GB
cap ([board] swept_cap_gb): …`,
and `tasks status` warns of it, once, from that record, standing while the last measure is past
the cap: `the artifacts set aside in PATH hold SIZE, over their CAP GB cap ([board] swept_cap_gb),
as the sweep measured them AGO ago, said until a sweep measures them again (…): …`, so a check by
hand clears it once a sweep measures again, a dry run recording nothing. Both ask for the check by
hand and name the five largest
folders there, each with the tasks tied to it and their stage and what still references it:
`in use: WHERE` for a task tied to it that is not done or cancelled (`task SLUG is open`), a post
on an open task's thread naming a path under it (`the thread of SLUG`) and a document of `docs/` on
main naming one (`docs/PATH`, cards included); `no reference found` for the rest, which alone are
suggested, and which the sweep does not remove either. A path is named under a folder there as
`FOLDER/swept/NAME/…`, or as the worktree's own artifacts folder held it, `FOLDER/…`, when the
folder holds that path now. A source that cannot be read is said, and then none is suggested. The
references are looked up once for each measure past the cap: one read of the open tasks' threads
and one `git grep` of `docs/` on main. Under the cap nothing is looked up, the sweep says nothing of
the folder and a dry run says `set aside in PATH: SIZE, under its CAP GB cap ([board]
swept_cap_gb), …`. Code: `crates/tasks/src/sweep/kept.rs`.

**Build output goes once the work is past its executor, the worktree staying.** A worktree the
sweep keeps (in review, its work not landed, holding files that did not land) loses its build
folders (`.target/`, `symphony/.target/`, each gigabytes, and what git ignores in `build/` and
`symphony/build/`) once every task tied to it is in review, done or cancelled, merged or not (an
in-process agent's worktree is tied to its task by its session), and nothing ran there for an
hour: no agent bound to it beat or made a tool call, the board's recorder recorded no build or
test there (a reviewer may build in its executor's worktree), nothing touched its git files or
its top folder, and no process works inside it. The worktree a land left when its process
ended loses it after the same hour, and the release run's checkout once nothing touched it for 7
days, as long as the release run itself keeps one. Any other worktree tied to no task keeps its
build: the owner's, or one made by hand. A task in concept, design, ready, in progress or
blocked keeps it, as do a land that runs and a lock the sweep does not know. Right before it
removes, the sweep reads the worktree listing, the board and the recorder's records again and
decides again, and asks again whether a process works inside; then it takes each Cargo build
lock in the folder without waiting (a build holding one keeps the folder), renames the folder
aside under them and removes it; of `build/`, what git tracks or holds untracked stays. A folder
reached through a link (`symphony` or `.target` linking elsewhere) is never touched. Each clear
says `cleared build output PATH (why; SIZE)`. Code: `crates/tasks/src/sweep/build_output.rs`.

**`just tasks sweep --dry-run` says what a sweep would do, and does none of it.** One line per
worktree git lists, `worktree PATH: SIZE; TASKS; MERGED; VERDICT`: what it holds on disk (the main
checkout's line has no size), the tasks tied to it with their status (`task SLUG (STATUS)`, or `no
task`), `merged into main` or how many commits it has that main has not, and what the sweep would
do with it and why (`would remove: …`, `kept: …`, or `kept for an orchestrator to decide: …` for
tracked edits, files that did not land and commits not in main). Then `set aside in PATH: SIZE`
says what the main checkout's `FOLDER/swept/` holds against its cap (above), and its summary ends
with what a sweep would free of what the worktrees it measured hold, which a sweep on the same
state frees.

**A linked worktree's first build starts from the main checkout's.** `tools/cargo.sh`, which
every build here goes through, makes a worktree's missing build folder as a copy-on-write clone
of the main checkout's `.target/debug` (`tools/seed-target.sh`: clonefile on APFS, a reflink on
Linux; nothing on a volume that cannot clone), taken under Cargo's own lock on that folder and
without the workspace's own packages, so the worktree compiles those from its own sources and
reuses every dependency. While the clone holds that lock, a build in the main checkout and every
board command (each builds the main checkout's launcher) waits: on macOS for one clonefile call
of the whole folder (`tools/seed-clone.rs`, compiled by rustc on the spot), else for `cp -c`
cloning each file. A clone takes disk only as a build replaces what it holds; `du` counts each
clone in full, `df` says what the disk holds. It is staged as `.target.seed.PID`, which a signal
removes with the clone and a later seed or sweep removes once its process is gone.
`SYMPHONY_SEED_TARGET=off` turns it off. Worktrees never share one build folder: Cargo names the same
package of two worktrees the same, so each would run the other's build of it. What it costs and
saves, measured: [the analysis](analysis/worktree-build-folders.md).

**Health** is derived from when the session was last seen. Every command
counts as a sign of life, and so does every tool call of a subagent (below):

| health | meaning |
|---|---|
| live | seen in the last 90 s, or inside one long tool call that could still be running |
| stale | quiet, or its monitor has stopped: its work is still held, nobody may take it |
| lost | gone for 10 minutes holding claimable work: that work can be claimed, on the record |
| left | it said `just tasks leave` (its roles were released, the open work it orchestrated given to another orchestrator, or waiting for one while none stays), or it has been gone 10 minutes holding nothing anyone could claim |

**Claimable work** is what a session executes in `concept`, `design`, `ready`
or `in_progress`, and what it reviews that is still in `review`. A session
whose part is finished (its task done, or waiting in review for someone else)
ends quietly as `left`; if its task comes back with `changes`, it is `lost`
again, and that task can be claimed. `just tasks who`, the Symphony's
**Agents** and the attach monitor read the same health.

Callback registration does not expire. The legacy attach stream remains only
for sessions with no native channel. Tool calls are still the heartbeat.

A **subagent** has no monitor; its own tool calls are its heartbeat. The
project's `PreToolUse` (Bash) and `PostToolUse` hooks
(`.target/bin/symphony hook tasks-beat`, `just tasks beat`) see the Claude Code agent id
on each of its tool calls. A `just tasks …` command the subagent runs binds it
to the handle that command acts as, and so does one naming a justfile or a
folder before `tasks` (`--justfile PATH`, `-f PATH`, `--working-directory DIR`
or `-d DIR`, as a worktree's `just --justfile MAIN/justfile tasks …`): its own
`--as <handle>` flag, else a leading `TASKS_SESSION=<handle>` on that same
command. The flag is the form to write: the allow rules pass no env prefix, and
the prefix binds nothing the flag does not. A handle anywhere else
in a command (a grep pattern, text inside a quoted argument, another recipe
such as `just land` run under someone's `TASKS_SESSION`) binds nothing. From
then on each of its tool calls beats for that handle, until a later board
command names another. A full session with no agent id (one started in an hc terminal, or a
`claude -p` run) is bound and beats the same way, keyed by its session id, but keeps the first handle
it names: naming another (registering or relaying for a child) moves nothing. A handle
with a monitor of its own (an orchestrator) is never bound: its monitor beats for it,
and a session that names such a handle is barred from binding at all, so the calls of
an orchestrator never keep a child it registered alive. The hook looks at the board
only for a board command or a key already bound. A foreground Bash call
reads live for as long as its timeout allows (at most 10 minutes), so a long
`verify-full` does not look like silence; of calls running side by side, the
one that may run longest keeps the mark, and only its own end clears it. Each
tool call of a bound subagent also records, on its binding (never its session
row, which keeps where its orchestrator ran `register`), the checkout that call
ran in: the top folder above the hook's `cwd` and the branch checked out there,
read from the `.git` files, and, when that checkout changed, the time it came
there; a call with no `cwd`, or one outside any checkout, moves neither (the
last checkout stands: such a call says nothing of where it works). A call there
that reads a time before any other there did (the clock went back) keeps that
earliest time too, never moving when it came. On Linux, whose kernel dates a
process's start on the clock since boot, the hook also reads that clock (its
boot's id and `/proc/uptime`): the binding keeps the reading of the call that
came to the checkout and of its latest call anywhere, a later boot's replacing
an earlier one's, and none when a call read none. Setting the wall clock moves
neither those readings nor a start dated on that clock. A
subagent waiting on a background command of its own (a `cargo test`, a build)
makes no calls; while a live process it started works in that recorded linked
worktree it reads `running_test` in the agents API, and is neither stale, lost
nor orphaning its task ([The agents API](#the-agents-api)).
A subagent that stops makes no more calls, leaves nothing working there, and
ages to stale and lost as before.

Orchestrators can share the board, each holding its own slug prefixes. A decision
one side makes that the other must act on becomes a short board task for that side,
and a side that approves documents commits them regularly, because worktree agents
see only what is committed.

## The agents API

The app is the source of truth for who is doing what: `GET /api/agents` of the Symphony and `just tasks agents
[--json] [--all]` return the same document (`crates/tasks/src/cli/agents.rs`; these keys are a contract, pinned by
`crates/tasks/tests/cli/agents.rs`). The board (sessions, roles, `task_sessions`) is the record; hc's
live terminals (`hc ls`) and the test-slot locks in the folder the project's `[board] slot_locks` names
(none when unset) are probes that say what each agent is doing now, and `probes` names
either as `unavailable` when it cannot be asked, the rest being built from the board alone. The hc probe keeps the
key `herdr` and an agent's terminal the key `pane`: names the wire keeps. The Symphony's `/api/agents` also names
that probe `panes`, with the same answer under both keys, and its client reads `panes`, else `herdr`
(`crates/symphony-api/src/agent_probes.rs`), so a client and a server one build apart still agree; the
rest of its document is the board's.

```
{"generated": 1790000000.0,
 "probes": {"herdr": "ok", "slots": "ok"},
 "agents": [{
   "handle": "amber-heron", "kind": "worker", "model": "sonnet", "parent": "cobalt-otter",
   "task": "sim/jump", "role": "executor",           # the task it works now (an open one first)
   "tasks": [{"slug": "sim/jump", "role": "executor", "status": "in_progress"}],
   "worktree": "/…/lamp-works-wt/sim-jump", "branch": "sim/jump",   # where it really works
   "agent": "claude", "session_id": "9aa0…", "pane": "t5",
   "callback": "uds:/tmp/cc-socks/1234.sock", "chat": false,
   "state": "running_test",
   "state_detail": {"recipe": "verify-full", "phase": "waiting", "since": 1789999000.0},
   "health": "live",
   "last_tool_call": 1789999900.0,
   "last_message": {"slug": "sim/jump", "ts": 1789999800.0, "text": "the ring is at 900"},
   "slots": null,                                     # an orchestrator's: {"open": 3, "cap": 8}
   "session_url": "https://claude.ai/code/session_01…",  # the web address its session gave, else null
   "in_process": false}]}                             # a subagent running inside its parent's session
```

`state` is `working`, `running_test` (a run of its own holds a test slot, `phase: holding`, or is queued for one,
`phase: waiting`, `state_detail` naming the recipe and since when; or a quiet subagent's own worktree holds a live
process, `{"recipe": "process", "phase": "running", "since": null}`, or the machine's processes cannot be listed to
tell, `"phase": "unknown"`), `waiting_on_input` (hc sees an approval or a
question), `idle`, or `gone` (no terminal and no sign of life). An agent running or queued for its own verify reads
`running_test` and `health: live`, never `lost`; so does a subagent whose worktree holds a live process, and one whose
processes cannot be listed reads `health: unknown`. `last_tool_call` is the time of the last tool call of the agent
(`null` before the first): the monitor's 20 s beat and board commands do not move it.
`callback` is the native address (`uds:` or `codex:`), or `null` for an inbox-only
session; `chat` says whether this session is the current chat recipient, including
an orchestrator fallback. `slots` is, under a project cap ([Slots](#slots)), the open
tasks an orchestrator (or any session holding open work's slot) holds against the
cap, and `null` otherwise. `session_url` is the web address the session gave
([Going to a session](#going-to-a-session)), else `null`; `in_process` says the agent runs inside
another session, a subagent bound to its parent's, with no pane of its own. No credentials appear in
this API.

The rules a port must copy, in this order:

- **Who is listed.** A session that ran `leave` (it has `left_at`) is omitted unless `--all`. So is one the board calls
  `left` (quiet, holding nothing another could claim; every session that never ran anything ends so) that has neither a
  terminal nor a run of its own nor a native registration: it is gone. A native
  registration stays visible while idle; its activity health is not fabricated.
  A quiet one with a terminal or a run stays (an executor whose task waits in review,
  its terminal idle). `--all` lists everything and changes nothing else.
- **worktree and branch.** From the agent's task session (`just tasks launch` recorded it; the one for its primary task,
  else its latest), else from its terminal's working directory (and the branch checked out there), else, for a
  subagent, the checkout its binding recorded at its last tool call, else, only with none of these, the directory
  `register` ran in: an agent that was never launched and has no terminal.
- **Which terminal.** The task session's terminal if hc still lists it live, else the terminal whose program runs the
  task session's conversation or the one bound by the heartbeat hook (when hc gives its argv), else the terminal titled
  or named with the handle. A terminal without an address is ignored.
- **Which run is the agent's.** First a run tagged with its handle (a slot records the `TASKS_SESSION` it ran for), wherever
  it runs: a verify in a trial merge, or by an agent that was never launched. Else, for an agent that is alone in its
  worktree, the run whose worktree is exactly its own (realpath; a slot records its git top level) and that carries no
  handle of any session on the board (left ones included, listed or not). Never a run in a directory beneath a worktree (the main checkout holds `.worktrees/` and
  exports), never to agents that share a worktree (all those registered in the main checkout); the count of agents sharing a
  worktree takes every session, left ones included, so `--all` never changes who a run belongs to. Several runs: holding
  before waiting, then the oldest.
- **What a quiet subagent waits on.** A subagent (an in-process binding) that is stale, lost or unknown, has no matched
  run and no terminal, and whose recorded checkout is a linked worktree (its `.git` a file; never the main checkout,
  which every agent registered there shares) waits on a process when one the machine lists is its own: it works in
  that worktree, its working directory the worktree's top folder or below it, with no other `.git` between (a scratch
  repository or a nested worktree is not its), compared component by component, never by a string prefix; and it
  started while the agent was there, from a second before its binding recorded that checkout to a second after its
  last tool call, on the clock its start is dated by: the board's for macOS, whose stamp is fixed when it started;
  the clock since boot for Linux, against the readings of that clock its calls took, never a wall time worked out
  from today's boot time, which setting the wall clock moves (a start in another boot than its last call's began
  after that call). A server or shell started there before the agent came, or after it went quiet, is not its, so a
  stray process never hides abandoned work. With none of its own there, the board cannot tell, and so never calls it
  lost, while one working there may be its: its start cannot be read (no permission, an unexpected record; one that
  ended since the listing is left out), or it began after the earliest time a call of the agent's there read and
  before it came, which only a clock that went back while it was there leaves, or it is dated since boot and the
  call that came, or its last call, read no clock since boot (a start after a last reading is still not its). The processes
  are listed with their starts (`lsof`, as the worktree sweep lists them, and the kernel's process table) only when
  such an agent is in the view, once a view, and a long-lived reader (the server's clock and dispatcher, the
  monitor) keeps the listing for 20 s by its store's clock, from when the listing ended.
- **state.** A matched run is `running_test`. Else a quiet subagent waiting on a process is `running_test` (`phase:
  running`), or, when the processes cannot be listed or the board cannot tell whether one there is its,
  `running_test` with `phase: unknown`. Else the terminal's status
  (`working`, `blocked` is `waiting_on_input`, `idle` and `done` are `idle`). Else the board's health: live is `working`,
  stale `idle`, lost or left `gone`. A matched run, and a process its subagent waits on, also lift `stale` and `lost` to
  `live`; processes that cannot be listed, or one whose ownership cannot be told, make them `unknown`.
- **A probe that fails** is `unavailable` in `probes`, and the answer is built from the board; a failing branch lookup is an
  empty branch.

Slot holders and waiters come from `test_slots.runs` (a held slot file whose writer is alive and whose lock is held; a
place in the pool's FIFO line, `<prefix>-<12-digit sequence>.place`, in the pool's own prefix: `slot-` and `render-`, that its waiter still holds); a slot file whose writer is dead is free and is not locked to find out.

An orchestrator's callback uses it: it does not call an agent stale or lost while `running_test`, and it receives a PULL
digest (`crates/tasks/src/cli/pull.rs`) once at its first dispatch, even when empty, then only when an actionable list gains an entry absent from the last delivered PULL after forgetting entries absent from any computed digest: red tasks ([Slots](#slots)) with no live executor 10 minutes after they were filed, in any status short of review, listed first as `red_unstaffed`, counted in the header only when there is one, and in no other list, `dependency_blocked` included, a cancelled dependency of its named in its note; ready tasks with no live executor
(one on a subtask still to be executed counts), in-progress with none, review with no reviewer, tasks that landed
(the branch is on main, or `landed <slug> on main` is on the task's thread) and are still open (a status change on the task after that line voids it, whether a move, a verdict such as `changes`, or a re-claim: the task is new work, not a landing left open), and idle agents, each scoped to the orchestrator's own tasks and those with no orchestrator yet ([Tasks and roles](#tasks-and-roles)) (an idle agent another orchestrator launched is left out). An executor
or reviewer that is `running_test` counts as present however quiet the board says it is.
A task in review whose tip a reviewer asked changes of is rework, counted in progress
([The pipeline alarm](#the-pipeline-alarm)). While the pipeline alarm stands, the whole board's
follows the header, whoever the digest is for, and its raising is an entry.
Counts and the clock do not trigger delivery. A departure alone pushes nothing;
an entry that stays present is never re-sent, and an ordinary entry that leaves and returns pushes once.
The first PULL a recipient receives is the whole digest: `PULL HH:MM` with every count, then each list's
first six rows and how many more it holds, but `unanswered`, whose every post is listed with the command that
answers it, since a post a PULL tells is never told again. Each later one says only what changed since the last PULL it
acknowledged: the counts that moved (`PULL ready_unstaffed=34 (was 33)`, or `PULL counts unchanged`), the
liveness and low-volume lines it was not told, each row a list gained, and `resolved LIST a, b` for the rows
that left it. A backlog of 33 that gains one row is that header and one row. What the last acknowledged PULL said
is kept in the recipient's callback state (`pull_said`, which older dispatchers ignore); after an older
dispatcher's acknowledgement, which does not record it, the next PULL is the whole digest again. A dispatcher
built before PULLs said only what changed (the shared server's, until it is rebuilt) records the entries of the
PULL it acknowledges and not what it said: a row it told that the recipient still hears shows it, and the next PULL
is the whole digest. Only when every row it told has left the recipient's digest by then is the next PULL measured
against the baseline before it: its header can repeat a count that moved before, with the older `(was N)`, its
liveness line can repeat or miss a change, and those rows are not said `resolved`. Rows added are exact either way,
never lost or said twice, and the next PULL this generation records is exact again.
A channel registered anew starts again from the whole digest: `--off`, a registration where none was (after
`--off` or a `tasks wait`) or one another process listens on (`Store::callback_in`) forgets which PULL the
session acknowledged, and a PULL waiting for it gives way to the whole digest; the posts it was told stay told.
Only the same listener moving to a new socket keeps its account, so what waited out an outage is said there as it
stands. An orchestrator reads the whole digest at its decision points with `just tasks pull` whatever it heard.
The scan reads only the open tasks' threads, and each of its reads waits at most ten seconds and is sent once, so a
board command never waits minutes on it. A pass whose unanswered-post scan fails says why on stderr and delivers the rest, and its PULL says nothing of
those posts, neither their count nor any of them resolved; their count is then unknown to the recipient, so the
next PULL whose scan answers says it (`unanswered=1 (was unknown)`) and resolves the posts answered meanwhile.
Unanswered work retains its task/post identity across departures and returns.
`unanswered` lists an executor on an in-progress, blocked or review task, or a reviewer
on a task in review, when it is lost or left (or its recorded terminal is closed) and an
addressed post has waited thirty minutes without a later message, status move or
verdict from that handle. A deliberate role release also answers earlier requests;
an automatic departure does not. Posts belong to their recipients at post time,
so a replacement never inherits its predecessor's role-addressed work. Each line
names the post and the recorded conversation's `just tasks restore SLUG HANDLE`
command, or the explicit launch command when no conversation was recorded. The
orchestrator receives each task/post once, even if it leaves and returns; a new
unanswered post can push a new line for the same task. Its count appears only when
nonzero. `reap` makes the sole automatic recovery attempt described above.

What is pushed is what the recipient cannot predict and must act on, said once: posts and verdicts, the
owner's moves (a `MOVE` line, [Talking on a task](#talking-on-a-task)), NEW, STATUS, ROLE, GIVEN (one line for every task the board gave the recipient since its last read, [Tasks and roles](#tasks-and-roles)), BLOCKED, the PULL above, and STALE, LOST or LIVENESS UNKNOWN once for each transition of a quiet
child, never once a pass (a child it already hears as stale is not also listed idle, in a pushed PULL, a wait
or `tasks pull` alike). A recipient is never told its own writes: not its own moves, not a role move it made (a hand-over, a take-over), and not what its own
children (the sessions it registered, `--parent`) take on or let go, their reservations, claims and releases
and the moves and readiness those make; a child's own move, to review say, is still said, and so is one it made
before a claim or release between two looks (a block, then a release, says the block). What is read is the
rest: `just tasks pull --as HANDLE` prints the whole digest as HANDLE hears it now, whatever was pushed, and
changes nothing; `just tasks show`, `list` and `agents` read the board as it is.

A push through a channel arrives wrapped in the receiving client's notice and costs a turn. A session that can
hold a background command reads the same lines instead: `just tasks wait --as HANDLE [--timeout S]` exits
printing what a channel would have carried since its last read, once there is any, and the session runs it
again. It is the dispatch's own pass for HANDLE (`crates/tasks/src/cli/commands/wait.rs`,
`Store::collect_callbacks`), its lines printed rather than delivered, each text flushed before it is recorded
as delivered (one that does not print is retried five seconds on, so a failure repeats a line and never loses
one), so it says the same lines by the same diff and account, from the same callback state: the first read is the whole digest
and any posts waiting, each later one only what changed. It looks when the board's marker moved, every two
seconds at most, and at least every fifteen, since a child going quiet or a PULL falling due changes with time
alone; with `--timeout` it gives up after S seconds with `WAIT nothing changed in S s` (`0` looks once). It
is refused while HANDLE has a channel, which takes those lines (`just tasks callback --off --as HANDLE` gives
them to `wait`), and nothing else dispatches to a session without one, so between reads nothing is said and
nothing lost. Waiting is the session's sign of life, as a monitor's is: it beats for HANDLE every twenty seconds
or so, at a look, so an orchestrator waiting on its board is not called stale for it. Each look also watches
the panes HANDLE oversees, one hc sample a look, as the server's clock does for a channel, so a pane waiting on
a prompt is said as `BLOCKED` by the second look that sees it; and while HANDLE holds the chat seat (see the
callback section above) the owner's chat events come with the rest.
What waiting costs, measured on 2026-11-11: once HANDLE has its callback state and is on the chat seat's waiting
list (its first look puts it there), a look at a board that stood still is nine reads and no write; a poll between
looks is one read of the board's marker; a beat is one write. While the board stands still that is about 30
marker reads, four looks and two beats a minute: some 66 reads and 2 writes. Every write to the board moves the
marker, a session's heartbeat too, so on a board where other sessions beat a look comes at nearly every poll, up to
30 a minute (about 300 reads). A look took 7 ms on a small board and 0.8 to 0.9 s as a command of its own on
the 2.2k-task board of the perf suite (its process's start and connection included, a debug build at load 35 to
80), and the first look there, which says the whole digest, 18 s.

A legacy native observer starts from the current board on upgrade rather than replaying its history.
Its upgrade marker and message cursor are separate callback-state fields that older writers leave intact.
The fields writers older than that scoping read (the observation, the open-status map and the delivered PULL entries)
keep their whole-board meaning; the scoped observation and the entries it withheld are kept beside them, in fields
those writers ignore, and read over them: a line either writer said is not said again by the other.
A line still waiting for delivery to an orchestrator (a NEW, STATUS, ROLE, BLOCKED, STALE, LIVENESS UNKNOWN or LOST)
is said only if a dispatch would tell it that line now, by the same test, so one a hand-over or a released claim took
from it meanwhile is dropped, or a LOST said again of the work it still hears; the hand-over's own ROLE line is kept,
and a quiet child holding no work is still its parent's to hear of. A PULL still waiting is never delivered as it was
written: the digest is built again for its recipient, counts included, and said in its place, by one rule over what
the recipient hears now, what the PULLs it acknowledged told it and what the waiting PULL tells. With nothing heard
beyond what it acknowledged, the waiting PULL is taken back; an entry beyond both, shown or only counted, makes a new
PULL with a retry budget of its own; otherwise the waiting PULL keeps its key and attempts and says the digest as it
is now. What it was told changes only when it acknowledges a PULL, whichever writer records that, so work it gains
while one waits is said and work that only departs renews no budget. Work it hears is told once a PULL naming it is
acknowledged, whatever older dispatchers write, queue or acknowledge after. Only what was withheld from it is
ambiguous, since an older dispatcher's own PULL told it the whole board and reads the same as the waiting one:
withheld work counts as untold, so when a hand-over makes it the recipient's, one PULL counts it, once more at most
for that hand-over and never left out, and nothing repeats on its own. Posts, verdicts and the owner's `MOVE`
lines are kept: they were said to whoever held the role then.
Codex callbacks use the queue by default; `git config symphony.codexSteer true` opts in to
active-turn steering. The owner decides its first live use.

### Going to a session

Every agent on the agents page, and every role chip on a task's page, carries a **Go to session**
control when the board knows a way into that agent's session. The server works out each agent's
way in at each tick of its clock (`symphony_api::resolve_sessions`; the Symphony's wire carries it
as the agent's `go`, `{"handle", "url", "pane"}`, and a task page's `sessions` for its role
holders), and what the control does depends on the session and on where the page was asked:

- **A web session.** A session that gave a web address (a cloud agent, a Claude Code session with a
  remote link) is an ordinary link to it, opened in a new tab, from any device. The session gives
  it with `just tasks register --session-url URL`, or later `just tasks callback --session-url URL`
  (alone, or beside a channel; an empty one clears it). It is `https://` or `http://` with a host,
  with no space or control character, at most 2048 bytes (`crates/tasks/src/session_url.rs`),
  checked when it is set and again when the board reads it, so a row written past the check (a
  copied dump, a hand write) is left out; a page links only what `symphony_api::web_address`
  passes, so no other scheme ever becomes a link. `callback --session-url` keeps the address only
  once its registration is accepted. The board keeps it in `callback_state` beside the session's
  row, never on it, moving no board marker, and `leave` deletes it. It is the agents API's
  `session_url`.
- **A pane, from the server's machine.** Where the project addresses its panes (`[agents] panes =
  "hc"`) and the agent has one (`pane`), a page asked from this machine (loopback, or one of its
  own tailnet addresses as `tailscale status` gives them) shows a button. The server has the Herdr
  app show that terminal, `herdr-app do agent.focus --terminal T` (`pane.focus --terminal T` for
  one that runs no agent), and brings the app's window forward with `open -a` on the app bundle
  its program lies in (`crates/tasks/src/launch/go.rs`, `POST /api/agent-focus`); the page says
  what was done, or why not. Herdr has no URL scheme a page could link to (neither `herdr-app`'s
  help nor its commands name one), so the server asking the app on its own machine is the only
  way, and the button is offered only to a page there; the route refuses any other.
- **A pane, from another machine.** A phone or another computer on the tailnet cannot focus
  anything on the Mac: its button shows the pane's visible screen, read only (`hc read T --lines
  0`, `GET /api/agent-screen?handle=H`), and reads it again on the next click. With both a web
  address and a pane, this machine focuses the pane and any other follows the web address.
- **A subagent.** An in-process agent (`in_process`: its binding says it runs inside another
  session) has no pane of its own, so its control goes to its parent's session (the first parent
  in turn that has a way in, at most eight up) and says "in PARENT's session". A launched agent
  with a parent is no subagent: its parent's pane is not its session.
- **Nothing known.** No web address, no pane the project addresses, no parent with either: no
  control.

Decided with it:

- **One call shows a pane.** The terminal's address alone is enough: the app finds the space, tab
  and pane that hold it, so no workspace or tab is named in turn.
- **What names a pane across a Herdr restart.** The control names the agent by its handle, never a
  terminal. Each tick finds the agent's pane anew, by its task session's terminal, the
  conversation its program runs or its handle as the terminal's name or label ("Which terminal",
  above), so once Herdr restarts and its terminals take new addresses, the next tick finds the
  pane again; a terminal's address is never kept as the pane's name.
- **What the screen may show.** A pane's screen shows whatever its agent printed, a secret among
  it, and so does a session's web address to whoever can open it. Both reach only a request the
  gate admits, this machine or a tailnet peer `tailscale whois` confirms (the gate refuses every
  other address before any route), and the screen is read when asked, never kept, and answered
  `no-store`.
- **Nothing is typed.** Neither a focus nor a screen sends a key, a message or a paste to a pane:
  the only calls are `herdr-app do agent.focus` or `pane.focus`, `open -a` and `hc read`.
- **One deadline, one at a time.** A focus (up to three calls: `agent.focus`, `pane.focus` and
  `open -a`) and a screen read each have 10 s for all their calls together
  (`symphony_board::launch::go::DEADLINE`), under the 15 s a page waits for its answer: a call
  still running when that is spent is killed and reaped, and one not yet started is not run. The
  kill reaches that call's own process, which stays in its caller's process group, so a signal to
  the caller's group (the server ending a `tasks reap`, Ctrl-C in a terminal) ends it too. hc and
  `herdr-app` are single binaries. A process one started that still held its output would keep
  the call's reader, and the killed process unreaped, until it ended, though the answer has
  already been given. While
  a focus, or a screen read, of a session is under way, another of the same kind for that session
  is refused (409), so pressing the button again starts no second chain of calls.

## Tasks and roles

A task is a slug (`sim/jump-departure`, the same slug the branch and commits
use), a title, a markdown body, a priority from `0` (most urgent) to `4`, and
optional links: `--link docs/game/SPEC.md#Heading` to the spec it implements,
`--depends` for tasks that must be done first, `--parent` for a larger task
or an instruction to an agent ([Subtasks](#subtasks)).

A cancelled dependency does not satisfy that requirement. Cancellation moves its
unfinished dependents to `blocked` and records the cancelled prerequisite on their
threads, with the status each held (`(was ready)`). Creating or moving a task with
a cancelled dependency blocks it too.
`next` excludes that work; PULL lists it as `dependency_blocked` with the reason,
including older tasks still in their previous stage. Done and cancelled dependents
keep their status. An orchestrator must replan the dependent work before it resumes.

`just tasks depends SLUG --replace OLD NEW [--as HANDLE]` points a task at a
successor; `--add DEP` and `--remove DEP` add or drop one, and each flag repeats.
It refuses a task the board does not hold, a finished task, a cancelled
prerequisite, a dependency already there or not there, and a cycle, and records
the change on the task's thread. A task a cancellation blocked that then waits on
nothing cancelled goes back to the status its block line recorded, or to `ready`
from `in_progress` when nobody executes it; a block line written before the
status was recorded sends it to `ready` (`in_progress` with an executor). A task
blocked by hand stays blocked.

`just tasks priority SLUG N [--as HANDLE]` changes an existing task's priority,
from `0` to `4`, and records the actor and both values on its thread. `next`,
`list` and the PULL digest use the new priority immediately; the change sends
no callback. A value outside that range or a task the board does not hold exits 2;
an actor it does not know, or one that left, exits 1, as every command refuses one.

The pipeline is `backlog → concept → design → ready → in_progress → review →
done`. The board shows the moving stages in one row, and where work ends
up (`done`, `blocked`, `cancelled`) in a row below it. A task
starts wherever it belongs (`--stage concept`, `--stage design`, or `ready` by
default). Work with no concept or design step goes straight to `ready`.
Claiming concept or design work keeps the task in its stage; the executor
moves it on when the concept or design is done. The card shows what waits in
each document the task links ("N waiting" undecided cards), for information
only: nothing waits on it.

Between `in_progress` and `review` the board draws **Build** and **Test**: an
in-progress task whose build or test run is going sits there until it ends, then
returns to its column. They are display stages, never statuses: no command moves a
task there, and a task in any other status (a task in review whose reviewer runs
its tests) keeps its column and shows the run as a chip. A run for a subtask (a
slice, or the `/work` instruction its executor holds) moves the card of the task it
is under. Runs come from the board's
recorder (`tasks run`, which this repository's `tools/cargo.sh` and recipes use) or
the project's own reader; until a machine reports either, the two stages are one
narrow rail that says so once, and the board's Status filter offers neither of them
([Machine build and test work](MACHINE_WORK.md#recording-builds-and-tests)).

**Review waiting on nobody is blocked.** A task that only waits on an approval,
with no work happening, says so by its status. A task
in review whose branch has every stamp it needs to land but ones nobody is
producing (the owner's approval, a dependency only that approval collects, a
hold its orchestrator names: the consuming project's stamp policy)
goes to `blocked`, with a line on its thread naming them. `just land`, `just
land-approve` and `just land-stamps` make that move when they find it, and move
the task back to `review` once one arrives: the owner's approval (the
Symphony's approve-to-land button, or relayed), a waiver, or a stamp someone
produces going missing. A role holder may move blocked work back to review, and
so may the owner; a command run by a session with no role on the task says the
move it could not make, and leaves it to the next run by a role holder. A
landing finishes work still held blocked as it does work in review. It
refuses work in progress, but finishes a task taken back to `in_progress`
after its last read of it, as main moved: that branch is on main, so the
landing tells its executor that further changes go in a new task. No other
command closes work in progress but a subtask's `done` and a close as covered by another
task's landing ([above](#a-task-another-landing-covers)). `landed` takes the statuses this landing
closes (`from`; review and blocked when it names none) and checks them against
the status its write reads; its answer names the one it found, and a close from
`in_progress` addresses the verdict to `@executor` in that write.

A task that has posted a card for the owner is lifted out of its column into
a **Waiting for your sign-off** lane above the board. That needs all of:

- the task is not `done`, `blocked` or `cancelled`, and someone executes it;
- its body or thread names a card slug whole (a card named `x` is not
  named by a mention of `x-2`);
- that card is still undecided in a document the task links: the cards a
  document page counts as "N decisions waiting" (`docs_tree.pending`).

Deciding the card puts the task back in its column. Work nobody holds, or that
posted no card, stays in its own column however its documents stand.

**`done` is reached only by a reviewer's `approve` (its orchestrator's, on a fast
fix), a landing, for a subtask its executor's `done`, or a close as covered by another
task's landing ([above](#a-task-another-landing-covers)).** A session
that ever executed a task, or a work subtask of it at any depth, gives it no verdict, whatever
role it holds since. Nor, but on a fast fix, does a session that requested the work: one that
orchestrates the task or ever did, or that filed it. The board refuses it the reviewer's role
and the verdict at once, naming the command a fresh reviewer takes the role with, by the rule
the land goes by ([below](#the-default-landing)), so no approve leaves a task done that the land
then refuses. An instruction subtask (`--instruction`: a review, a question, an errand)
puts nothing on the task's branch, so its executor is not counted: the reviewer that claimed
`SLUG/review` gives `SLUG` its verdict, unless it executed `SLUG` or a work subtask of it too.
Work on the branch is a slice, filed without `--instruction`. `done` is left only by an approved task no landing has closed and whose tip the
default branch does not hold, back to `review` by its executor, reviewer or orchestrator,
for a fresh approve of a tip the approve does not cover
([below](#the-default-landing)). A move of a done task the board refuses names that move
back to review only while the approve stands; once a landing closed the task, it says what
the task still needs goes in a new one. Several sessions
can hold one task at once, one role each:

| role | how | may |
|---|---|---|
| orchestrator | `just tasks role SLUG orchestrator`, or `just tasks create` by an orchestrator session | coordinate: spawn the others, follow the thread; close it as covered or cancel it; one a task |
| executor | `just tasks claim SLUG` | do the work, move it to `review`; close it as covered or cancel it |
| reviewer | `just tasks role SLUG reviewer` | give the verdict: `approve` (done) or `changes` (back to the executor) |
| reserved | `just tasks reserve SLUG` | hold it for later, for an hour, so nobody else claims it |

A branch reaches `main` only through `just land`: the Symphony's default
([below](#the-default-landing)), or the consuming project's own, whose recipe replaces it.
Under a consuming project's landing policy its
reviewer gives one verdict, not two: `just land-approve BRANCH --tip SHA` with
the full commit it checked, from its own
session (`TASKS_SESSION`), holding the reviewer role on the branch's task,
recorded on the branch's patch (`--adversarial` for the second reviewer of a
tier 3 change). The landing then finishes the task in review for it: no board
`approve` is needed. A branch with no task gets `review/BRANCH` from
`just land-request BRANCH`, and the session that ran that (or the task's
executor) is the one that lands it.

### A task another landing covers

A task whose fix landed with another task's branch has no branch of its own to land, and can
still be closed. Its executor or its orchestrator, the session holding that role on it, closes it
in any open status with no reviewer's verdict:

```sh
just tasks done SLUG "what came of it" --covered-by OTHER --as HANDLE
```

- **Covered by a landing.** OTHER is done and landed: its last verdict starts `landed `, its first
  line names a commit after its first ` at `, and the default branch holds that commit. The
  default land writes `landed SLUG on DEFAULT at SHA`; a project's own land, through
  `Store::landed`, may say more after the commit (`landed BRANCH on main at SHA; stamps counted:
  …`). A task in review, done by an approve no landing closed, or landed by a line naming no
  commit covers nothing, and the refusal says which.
- **No work of its own.** It refuses, naming what it found (the first five and how many more),
  while the task has work the default branch lacks where the board records its executors working:
  commits on the branch named as its slug, on the branch a launch of its executor recorded
  (`tasks launch`, `restore`), or on the branch an in-process executor's binding places it on (the
  checkout its last tool call ran in, as `just tasks agents` shows it); and, in such a linked
  worktree still there, commits at its HEAD, a detached one too, or changes no commit has. That
  work needs its own review and land; a branch the default branch holds whole is no work of its
  own. It does not read the main checkout's changes, which are every session's, an in-process
  executor's earlier checkouts, the binding of a session that has left the board, stashes, or
  branches under other names: the closer checks those.
- **Said on the thread.** Its line, the task's verdict, names the covering task, the commit and who
  closed it: `covered by OTHER, whose landing at SHA holds its work; closed by HANDLE: NOTE`. The
  task is done, and the orchestrator hears it as it hears a verdict. The task page's review gate
  shows it waived, `Covered by OTHER, landed at SHA`, its rounds before it as its past: never as
  approved, nor as landed with no review verdict.
- **No approve.** A covered close is not a verdict a landing goes by. Being the task's last verdict,
  it leaves no approve standing, so `just land SLUG` refuses it (`SLUG is done with no approving
  verdict from a reviewer`, or `SLUG was closed after its approval: covered by …` where an approve
  came before it and the task went back to review since), and it never goes back to review. Who may
  approve work is unchanged: a fresh reviewer, by the rule above.
- **Read in the repository.** As a move back to review does, the command line and the API read the
  repository first (`crates/tasks/src/covering.rs`), refusing from outside a checkout of it; the
  board itself checks the roles, the statuses and the landing's line (`Store::close_covered`). A
  refused `done` of a top-level task, and a refused move to done, name the command.

Work that is no longer wanted is cancelled instead, by its executor or its orchestrator, in any
open status: `just tasks move SLUG cancelled --reason "why" --as HANDLE`. Once work has started
(in progress or past it, or with an executor), only a session holding a role on the task moves it;
the owner cancels any open task. The reason is a post of the mover's right after the move's own
line, `cancelled: why`, in the move's one write; any move takes one (`blocked: waits on …`), and a
reason with no words is refused.

### The default landing

`just land SLUG --as HANDLE` (`tasks land`, `crates/tasks/src/land.rs`) lands the
task's branch, named as its slug, on the default branch. It is the convention a project gets without configuring anything, and a lower level
replaces a higher one: a project's own `land` recipe replaces it, everywhere. It runs only as `just land`, whose recipe sets `SYMPHONY_LAND`: `tasks
land` without it refuses, so `just tasks land` run by itself never lands. The marker is a
guard against accident, not the gate: anyone can set an environment variable, and what
lands is decided by the approval check below, a fresh reviewer's approve standing on the
tip. So `symphony init` allows `just land` with no permission prompt, and writes no ask
rule; one an older init wrote, for the marker or for `just tasks land`, a run removes. A
project's own `land` recipe gets the same allow rule, and its own landing policy decides
what it lands. An orchestrator lands an approved task without asking the owner. In a project
whose justfile runs a `land` of its own (`just --show land` is not the Symphony's),
`tasks land` refuses by any route, naming that justfile. A land is its merge, its move,
its push and its close: it runs no generator and no sweep, and nothing slow is on its
path or behind its lock, since every merge could make a slow run start again, and a lock
or queue around one would hold the whole pipeline back
([its card](DECISIONS.md#land-tests-changed-packages)). One land of a
repository runs at a time (`land.lock` in `symphony/tasks/`), the lock held for those
seconds and its check; a second waits its turn and says whose, with no timeout,
since the lock is freed when its holder's process ends however it ends, and a signal
stops the wait. The settings are read from the default branch as the main checkout
has it committed, never from the branch landing, so a branch cannot change its own
check, nor what counts as one (`check_paths`, below). A land of this repository runs no check either (`[land] check = false`) and
takes seconds; what the client distribution needs, the regenerator it starts makes
afterwards, and runs the check, the docs audit, on what it made ([below](#the-regenerator)).

- **Approved by someone else.** The task's last verdict is an `approve`, which left
  it done, given by a fresh reviewer: never a session that ever executed the task or a
  work subtask of it (claimed it, whatever role it holds now), nor one that orchestrates or
  ever orchestrated it (joined as its orchestrator, or was handed the role or took it
  over), nor its filer, which requested it (RULES.md `workflow/reviewers-not-the-owner`).
  The board refuses each of those verdicts itself, by the same rule (`Held::unfresh`): a
  former executor's, a work subtask's included, and, but on a fast fix, its orchestrator's,
  a former orchestrator's and its filer's, which it refuses the reviewer's role as well, so an
  approve the land refuses for whose it is is one an older board took; the
  executor of an instruction subtask, the review its reviewer claimed, is none. The one
  exception is a **fast fix** (RULES.md `workflow/fast-bugfixes`): a task filed with
  `just tasks create SLUG TITLE --fast-fix WHY`, the risk that calls for no reviewer in
  words, which the board keeps among its own fields where no session sets or clears it
  afterwards. On a fast fix its orchestrator, which did not execute it, gives the
  verdict on the executor's posted evidence, and the land goes by that approve; on any
  other task it refuses an orchestrator's. A fast fix its orchestrator executed itself
  is landed by a fresh reviewer's approve, as any other task is.
- **A red's fix changes no check.** A tool that files a red before any fix exists may
  file it as a fast fix, its risk the bound `symphony_board::fast_fix::RED` states
  (`crates/tasks/src/fast_fix.rs`), and the land enforces that bound, since the board
  cannot read the repository: on a fast fix whose recorded risk is that bound, or one
  marked red, its orchestrator's approve (or its filer's) lands only a fix whose diff from
  where it left the default branch changes none of the project's tests and checks, and a
  fresh reviewer's lands any fix. It goes by the recorded risk, which no session clears, so
  a red whose mark its filer lifted is held to the bound still, and knows it by its opening,
  `a red is fixed as a bug fix is` (`fast_fix::RED_OPENING`), which every wording of it keeps,
  so a red filed before a rewording is held as well. A loosened test or check is
  how a red turns green unfixed, so a fix that changes one needs a reader that did not ask
  for it; when the code is fixed and the failing test, unchanged, passes, the test is that
  reader. The refusal names what the fix changes and sends the task back to review for
  `@orchestrator` to staff a fresh reviewer. What counts is `[land] check_paths`, a list of
  entries read as `.gitignore` lines (a final `/` a folder, a `/` at the start or in the
  middle anchoring it at the root, `*` and `?` within one part of a path), `PATH [TABLE]`
  naming a table of a TOML file, dotted for one inside another, which counts only where its
  value differs. With none written it is `["tests/", "fixtures/", "testdata/",
  "snapshots/", "*_tests.rs", "tests.rs", "*.snap", "Cargo.toml [lib.test]", "Cargo.toml
  [lib.doctest]", "Cargo.toml [lib.harness]", "Cargo.toml [test]", "Cargo.toml [bench]",
  "Cargo.toml [bin]", "Cargo.toml [example]", "Cargo.toml [package.autotests]",
  "Cargo.toml [package.autobenches]", "Cargo.toml [features]", "Cargo.toml
  [dev-dependencies]", "/justfile", "/symphony.just", "/tools/", "clippy.toml",
  ".clippy.toml", "rustfmt.toml", ".rustfmt.toml", "/.cargo/", "/Cargo.toml
  [workspace.lints]", "Cargo.toml [lints]", "/symphony.toml [verdicts]", "/symphony.toml [land]", "/symphony.toml [releases]",
  "/crates/symphony/src/cli/commands/size_budget.rs", "/crates/symphony/src/docs_audit.rs",
  "/crates/symphony/src/docs_audit/", "/crates/symphony/src/release.rs",
  "/crates/symphony/src/release/checks.rs",
  "/crates/symphony/src/release/steps.rs", "/crates/tasks/src/land.rs",
  "/crates/tasks/src/land/", "/crates/tasks/src/fast_fix.rs", "/crates/tasks/src/red.rs",
  "/crates/tasks/src/store/work/red.rs"]`: the tests and what they read, the Cargo tables
  that say which tests run and how (a `[[test]]`, `[[bench]]`, `[[bin]]` or `[[example]]`
  target whole, its `test`, `harness` and `required-features` with it, and the features, whose
  defaults decide what `#[cfg(feature = …)]` and `required-features` leave out of a test run)
  and what tests alone build with (`[dev-dependencies]`), the recipes and the scripts and budget under
  `tools/` they run, the lint and format settings, the project file's tables that say what
  checks, lands and releases, and the checks' own code: the size budgets, the docs audit,
  the release run's checks and their steps, this land with its list, and the red and
  fast-fix marks. A project's own list replaces it. Beside the list a Rust file counts where
  the diff reaches a test-only module (its file saying `#![cfg(test)]`, declared under a
  test's attribute, by `#[path]` too, or declared by a test-only module), an item a test's
  attribute marks inline in a source file (`#[test]`, `#[bench]`, one whose last name says
  `test`, or a `cfg` naming `test` but as `not(test)`, `#[cfg(any(test, …))]` among them),
  from the first attribute of its run to what ends the item in the file's code, its
  comments, strings and characters left out, or a line added or removed that sets a lint's
  level. What it cannot tell counts as a check: a file whose code does not lex, or where a
  marked item runs unclosed to the end, counts whole, as does a module such a file declares,
  and a path git fails to read or a branch with more than one merge base refuses the
  orchestrator's approve for a fresh reviewer's. What it does not see is product: a module
  declared inside an inline module is looked for beside its file, and a test-only file named
  only by a macro or a build script, or a lint set in a file no entry names, is product. The
  release run files every red so (RULES.md `workflow/release-reds-first`,
  [RELEASES](RELEASES.md#the-release-run)), but a stop the owner clears, for which nothing
  lands; the regenerator files its reds (`tests/main-red-regenerate`,
  `tests/main-red-check`) with no fast-fix mark, as it does on every project's board.
- **Covering the tip.** The approve names the branch's tip: a reviewer approves as
  `just tasks verdict SLUG approve "FULL_SHA: why"`, and the land goes by the named
  commit over any commit time. An approve naming none of the branch's commits covers
  each commit made before the verdict and before the task last went to review; the
  landing's line on the thread then says it judged by time. A commit after the named
  one, after the verdict, or during the review without a named tip refuses, naming it.
- **Back to review.** An approve that does not do (someone else's to give, or not
  covering the tip) sends the task back to review, said to `@orchestrator` or
  `@reviewer`. A conflict or a red check on the merge of the tip as approved is said to
  `@executor`, which merges the default branch into its branch and fixes it there, then
  moves the task back to review itself (`just tasks move SLUG review`) for a fresh
  approve naming the new tip, and the land runs again. The one done task that may go
  back is an approved one no landing has closed whose tip the default branch does not
  hold (one landed or merged by hand stays done, and what it still needs is a new
  task), moved by its executor, reviewer or orchestrator; the command line and the API
  read the repository for that, and refuse from outside one. The board then says so to
  `@executor` and `@reviewer`, in their inbox and PULL digest though their panes are
  closed, and with no live executor `@orchestrator` hears it, which restaffs the
  executor or reviewer the task needs. Either move, the land's or the command line's,
  starts `[board] on_review` as a move to review does.
- **A release candidate's red.** While the release run holds a candidate
  ([RELEASES](RELEASES.md#the-release-candidate)), a red of it is fixed on a branch started
  at the local branch `release-candidate`, which merges that branch again before review and
  never merges main before its approve, its approve naming the full tip, and it lands by this
  same land. Nothing in the land changes for it: its `[board] on_land`, the release run's
  `--due`, reads the land afterwards and moves the candidate to the newest commit of the
  branch an approve named that holds the candidate and the fix alone. A conflict goes back to
  `@executor` as above, who merges the default branch then and asks for a fresh approve; the
  candidate keeps the commit approved before that merge. A red whose fix landed with another
  red's task is closed as covered by that landing ([above](#a-task-another-landing-covers)),
  which answers it once that land advanced the candidate; one covered by any other landing has
  its fix on main and not in the candidate, which is dropped. A red fixed outside this
  repository is answered by its task cancelled, saying where; one done otherwise waits for its
  land.
- **Merged and checked.** In a worktree of its own (`land` in the project's `[agents]
  worktrees`, else `symphony/tasks/land/worktree`), never the main checkout: one `--no-ff`
  merge commit, "Merge branch 'SLUG'"; then `[verdicts] check` on it, unless `[land]
  check = false`, its output in `symphony/tasks/land/SLUG.log` (the slug's `/` written `%2F`,
  its `%` `%25`, so no two slugs share a file). This repository turns it off: its check,
  the docs audit, is evidence at a document change's tip, run on main's tip by the
  regenerator a land starts ([below](#the-regenerator)), and part of the release run's full
  verification ([CHECKS](CHECKS.md#checks-with-no-reviewer)).
- **Generated paths.** A generator's `outputs` are the paths it owns: the path itself
  and, for a folder, what is inside it (`gen/` and `gen` own `gen/a`, never `gen-x/a` or
  `gen.txt`). What they hold is the regenerator's, made of main's own sources, so the
  merge keeps main's side of every one of them the branch changed, in a conflict or
  not, and commits, its second parent still the approved tip, so no new approve is
  needed and no branch's own build of them (its client distribution) ever lands. The
  thread's line before the move, the record and the landed line name those paths (`its
  generated paths (PATHS) taken from main, for the regenerator to make`). A conflict
  anywhere else, alone or beside generated ones, refuses as any conflict does. What
  `origin` gained there is main's own (a regenerator's commit pushed from elsewhere),
  kept but where it conflicts.
- **Moved.** The main checkout, on the default branch with no staged or tracked edit,
  fast-forwards to what passed, so the default branch's new tree is the tree checked.
  Before it moves, the land records the landing (the task, the tip, the merge, the
  commit checked and the generated paths it took) in `symphony/tasks/land/SLUG.merged` and on
  the thread (`land SLUG: merged TIP as MERGE, checked at SHA; moving main there`). The
  fast-forward runs in a process group of its own, which no terminal's signal reaches
  and the land forwards none to, within the land's timeout, so it is never stopped part
  way but by that; `symphony/tasks/land/moving` names the move while it runs. Where the
  default branch is once git ends says what happened, whatever git answered: at what
  passed, the move happened and the record stays; still where it was, the move did not
  happen, the record is dropped and the land restores what git half-wrote in the main
  checkout, saying each path: one the two commits differ in whose index entry and file
  are each as before or as after the move is put back as main has it (`restored PATH as
  main at SHA has it`), one main lacks is removed (`removed PATH, which main at SHA
  lacks`), and one changed otherwise is left as found (`left PATH as it found it`).
  When the default branch moved meanwhile, the merge and the check run again on its new
  tip; a red then is said on the thread to `@orchestrator`.
- **Pushed.** Unless `[land] push = false` or the repository has no `origin`, the
  default branch alone is pushed, no other branch and no tag, never forced: what
  `origin` gained since is fetched first, merged and checked the same way, and the
  push names the commit checked (`SHA:refs/heads/main`), never what the branch holds
  by then. A push that fails leaves the landing done here and says so in its line,
  with the command to retry; a merge with `origin` that conflicts is said to
  `@orchestrator` too.
- **Closed and swept.** `landed` writes the line `landed SLUG on main at SHA`, with the
  generated paths it took from main and whether it pushed, and closes the approved
  task, which stays done; a task sent back to review since stays there. The land then
  starts the sweep detached, which removes the task's finished worktrees after it, its
  output in `symphony/tasks/sweep.log` (`land SLUG: its finished worktrees are swept in the
  background`), and a project with `[land] generators` has the regenerator started
  ([below](#the-regenerator)). The land waits for neither, and neither runs under its
  lock; while a regenerator runs, the land starts none and signals none.
- **Then started.** Last, after the regenerator, the land starts the command `[board]
  on_land` names in the main checkout's `symphony.toml` (none unless the project says),
  with the task's slug and its merge commit appended (`crates/tasks/src/on_land.rs`), so
  what it starts finds the regenerator started already. It runs detached from
  the main checkout in a session of its own, outside the land's recorded run, with
  neither the lander's session nor the marks of how the land was asked for, `SYMPHONY_LAND`
  and the `tasks` recipe's `SYMPHONY_UNASKED`: what it starts, a release run among them, is
  no command an agent ran unasked, and lands nothing. The land listens to it for 3 seconds at most
  and shows what it said by then, then goes on, never waiting on it longer. Its output goes to a
  file of its own, `symphony/tasks/on-land/STAMP-SLUG.log` (the newest 40 kept), which the land's output
  names, and a line naming that file to `symphony/tasks/on-land.log` (set aside as `on-land.log.1` past
  256 KiB). A command that cannot be read or started is said there and on the land's output, and
  the land still stands.
  A board that `TASKS_SERVER` names (a test's) starts it only under `TASKS_ON_LAND=on`;
  `TASKS_ON_LAND=off` starts none anywhere. The Symphony's own repository names its
  release cadence there, whose run waits for that regenerator
  ([RELEASES](RELEASES.md#the-cadence)).

The merge of the task's tip is one step of the land, the one a hosted pull request's
merge would take the place of: the approval before it and the record, the move, the
push and the close after it stay as they are.

**Bounded.** The check runs in a process group of its own, which the land ends whole
past `[land] timeout_seconds` (an hour unless the project says;
`0` is refused, as is any `[land]` key the land does not know); a fetch or a push has
five minutes at most, never asking for a password. A land's worktree carries the pid
of the land that made it and when that process started (`symphony land: SLUG (pid N,
started S)`), and a later land replaces one left behind once that pid no longer names
the process of that start (a land killed outright, its number since another's) and
`lsof` says no process works in it; when `lsof` cannot say, the refusal names
`lsof +D PATH` to look and the command that removes it once nothing works there. The
group a land runs is named in `symphony/tasks/land/running` while it runs, with when its
first process started. While that group has any process alive, a later land refuses
and rebuilds nothing; it says to end the group (`kill -TERM -N`) only while its first
process is the one the land started, by its start, and for the fast-forward it says
to wait for it instead. A first process of another start under the number means the
land's group is gone (no pid is reused while a group of that number lives), and the
name is forgotten; with the first process gone, or never dated (a land before this
one), nothing proves the processes in the group the land's, and the refusal says how
to list them (`ps -A -o pid,pgid,lstart,command`) and to remove the file once those
of the land's are ended, never to end the group.

**Interrupted.** Ctrl-C, a hang-up or `kill` (SIGINT, SIGHUP, SIGTERM) does not reach
the check or the fast-forward, which run in groups of their own: the land takes the
signal, ends the running group (SIGTERM, then SIGKILL five seconds on) but the
fast-forward, which it waits for, removes its worktree, says one line and ends by that
signal; a land waiting for another's lock ends at once, nothing landed. Before the default branch moved, nothing landed. After it moved (while it
pushed, say), the approve still stands and the task is open, and running the land
again finishes it, by the record it wrote before the move alone: its tip the branch's
still, its merge holding that tip, its checked commit on the default branch. The run
checks that the approve covers that tip and finishes the push of the recorded checked
commit, never of a newer tip no check passed on, the landed line, marked as finishing
an interrupted land, and the sweep, with nothing merged or checked again. A sweep
meanwhile keeps the task's branch while that record names it; the worktrees on it may
go, as the run makes a worktree of its own. A land killed outright while the main checkout moved leaves the move running and named: the next
land waits for it, and once it ended without moving the default branch, restores what
it half-wrote before it reads the main checkout, saying each path here and on that
task's thread, and drops its record. A tip on the default branch with no such record
(merged by hand, or a branch that never left it) refuses, `merged outside land:
nothing to finish`, with nothing pushed, closed or said; the line says how to finish by
hand a land whose record was lost: push the default branch once a check passed on it,
say the landing on the thread (`landed SLUG on main at MERGE`, the merge that brought
the tip named) and sweep.

Each refusal is one line, on the land's stderr and on the task's thread, and leaves
the default branch where it was: no approval standing, one that is not a fresh
reviewer's, a commit it does not cover, a main checkout with edits or another branch
checked out, a conflict outside the generators' outputs (never resolved), a
fast-forward that did not move the default branch (what it half-wrote restored), and a
red check.

**A task has one orchestrator.** Taking the role on a task another session
holds while it is live, stale or of unknown liveness is refused, naming the
holder and the command that moves it: `just tasks handover SLUG HANDLE`, run by
the holder, drops its role and gives the role to HANDLE in one write. The
thread says `A handed orchestrator over to B; address B or @orchestrator`,
and B hears one `ROLE SLUG B orchestrator, STATUS (from A)`, whatever other
role B holds there; A made the move and is not told it. HANDLE must still be there (live, stale or
of unknown liveness); a hand-over to a session gone quiet, or leaving as it is
written, is refused, naming its health; under a cap, so is one that would raise
HANDLE's open tasks past it ([Slots](#slots)). Taking the role over a holder that is
lost or left drops that one in the same write, and the thread says `B took
orchestrator over from A (lost)`, which A hears and B, who made it, does not; so
does taking it again where a left holder still sits beside you. A task is its orchestrator's when that session holds the
role on it, or on the nearest task above it that has a holder still there, and
**every open task has an orchestrator**: one without is a bug. The board gives the role wherever
a task would come to have none (`crates/tasks/src/orchestration.rs`,
`crates/tasks/src/store/work/given.rs`):

- **Filed.** An orchestrator session that files a task with `just tasks create` holds its
  orchestrator role from that write (`created SLUG (ready); HANDLE orchestrates it`), unless the
  task is already another's under a parent it holds; a subagent's claim of it leaves that role
  where it is. `--no-role --for HANDLE` files it for HANDLE, a live orchestrator, which takes the
  role instead; `--no-role` without `--for`, or with one naming no live orchestrator, is refused.
  Any other filer's task (a worker's or a reviewer's follow-up, a filer's red, staffing's review
  job, `tasks api`'s `create`) goes to the orchestrator of a task that session holds another role
  on, else to its parent session (`register --parent`) while that is a live orchestrator, else by
  the rule below; the owner's task by the rule alone. One filed under a task an orchestrator holds
  is that one's by the tree, and nothing is written. `create` names who took it.
- **Left.** A `leave` gives each open task the leaver orchestrates, and no other holder keeps, to
  another orchestrator by the rule, in the leave's own write, one thread line each:
  `B was given orchestrator from A (left)`. Only a leaver holding the orchestrator role reads the
  board for it.
- **Moved.** A `just tasks parent` move that takes a task out from under the task its orchestrator
  holds keeps it with that orchestrator: the move gives it the role on the moved task in the same
  write (`B was given orchestrator (SLUG moved out from under PARENT)`), so an orchestrator
  restructuring its own tree keeps its work, and the move's cap read counts the moved work as
  B's, in a slot of its own ([Slots](#slots)). Only such a move reads more than the tree: the two
  lines of parents and the moved tree, once.
- **Lost, or nobody's.** Every read of the whole board that shows an open task nobody orchestrates
  gives it, as the board (`Store::give_if_due`): each look of an orchestrator's `just tasks wait`
  and of its `attach` monitor, `just tasks pull`, each pass of the detached dispatcher a
  command-line write starts while a channel is registered, the machine server's clock for every
  project it serves, and the idle sweep (`just tasks reap`, from the server's maintenance of each
  project it serves that has a board). So a lost or left holder's task (`B was given
  orchestrator from A (lost)`), one filed by a build or a tool that gives none, or one on a board
  copied in (`B was given orchestrator (nobody orchestrated it)`) is given at the next of them, on
  every board, with no sweep run. Each read is bounded: one that shows every open task held reads
  nothing more, one that shows only tasks waiting with none that stays reads one record of who
  listens, and a give is made once (fenced on what it read, one thread line per give).
  `reap --dry-run` says what it would give.
- **Released.** `release SLUG --role orchestrator`, and `release` of every role, is refused while
  it would leave open work with none and no orchestrator holds a task above it, naming
  `just tasks handover SLUG HANDLE` to the orchestrator the rule picks, or saying no other that
  stays is there (a `leave` then leaves the work waiting).
- **Reopened.** A done task moved back to open work after its orchestrator went is given by the
  rule in that move.

A task's own story (`--for`, the filer's work, its parent session, the task above it, the one it
had) names its orchestrator while that one is live. Otherwise the rule: the orchestrator holding the
nearest task above it, else, among the orchestrators that stay, the one holding the fewest open
tasks (ties by handle), one task at a time. An orchestrator **stays** when it is live, has been
since past the first live line (registered 90 seconds or more before), and either listens (a native
channel, a `just tasks wait` in progress, an `attach` monitor) or already holds the orchestrator role
on an unfinished task. A session that only registered is given nothing by the board's own choice,
so a throwaway that registers and goes moves nothing. With none that stays, the task is marked
waiting (`just tasks show` says `waits for an orchestrator that stays, for N min`), and the first
orchestrator to come to stay is given it at the next whole-board read, told in one line however
many it was given: `GIVEN 2 open tasks are yours to orchestrate: sim/a, sim/b` (the first ten
named), in its wait, its monitor or its channel. The board's gives reach their receiver as that one
line, never a `ROLE` line per task, and a given orchestrator has orchestrated the task, for the
fresh reviewer rule. So a task with no orchestrator is only the moment before the board gives it
one, or the wait while none stays, and their count is a defect on the board's health ([Board
health](#board-health)). The cap ([Slots](#slots)) refuses no give but a move's: a given task in
progress counts for its new orchestrator as it did for the one it had, and a ready one counts for
none.

Each orchestrator hears its own tasks, and every orchestrator hears a task with none, for that
interval (`crates/tasks/src/orchestration.rs`): its NEW lines, PULL digest rows and the STATUS of
its move to `blocked`, which no digest row shows; only its orchestrator hears an orchestrated
one's. A post to `@orchestrator`, and a verdict, go to the nearest holder still there, passing one
gone quiet. The owner's word, a post, a verdict or a move, asks more: on a task whose nearest
holder is not live (stale, or of unknown liveness, as well as a task with none) it reaches every
orchestrator, marked, since none is there to relay it ([Talking on a task](#talking-on-a-task)). A
session gone stale or lost, or waiting in its terminal, is said to the
orchestrator of the work it holds, its LOST line naming only that
orchestrator's tasks and its BLOCKED line only a launch for one of them; a
session whose work has no orchestrator yet, or who holds none, is
said to its parent, as before. The digest's idle sessions leave out the
children of another orchestrator still there. Another orchestrator's task
reaches it only in a post to its own handle. `just tasks list --doubled` lists the tasks
that still hold two orchestrators from before the rule; one of them settles it
with `handover` or `release SLUG --role orchestrator`. No board data is
rewritten for them.

The executor can never review its own task, and the reviewer cannot claim it; nor, but on a
fast fix, does the task's orchestrator, a former one or its filer take the reviewer's role.
Of any number of simultaneous claims exactly one wins. `just tasks next` picks
work for you: your own reservation first, then priority, then a red task
([Slots](#slots)) before other work of its priority, then the areas you
already work in (`--affinity`, and the slug prefix of tasks you hold).

### The regenerator

What a project's `[land] generators` make (this repository's client distribution) is
made after a land, never on its path. One build for main runs at a time, and once it is
built the next is main's newest tip: a build in progress is never cancelled but let
finish, and there is no queue, so some merges are never built on their own. A land that
closed its task starts
`tasks regenerate` detached (`crates/tasks/src/land/regenerate.rs`), by the land's own
build, its output appended to `symphony/tasks/land/regenerate.log`, and waits for nothing it
does.

- **One build of main at a time.** It holds `symphony/tasks/land/regenerate.lock`, taken
  without waiting and naming its process and that process's start, as the land's lock
  names its land. A land that finds it held starts nothing and signals nothing: main's
  ref is all it leaves, and no list of builds to make is kept anywhere. One started
  anyway (two lands' starts at once) finds it held and ends at once, and one that finds
  main's tip already made, current or red, makes nothing.
- **Never cut short.** A build is never stopped or begun again because main moved: it
  finishes, and then lands what it made of its commit or discards it (below).
- **Then the newest tip, once.** Once it lets go of its lock it reads main once: on
  another tip than the one it finished with, it builds that tip, the newest, and every
  merge that came in between is never built on its own; else it ends. Three lands
  during one build cost one more build, of the last.
- **A builder that died.** Its lock is free once its process is gone, the file still
  naming it, and the next land's start takes it over and builds main's tip.
- **A worktree it keeps.** Each round checks main's tip out in a worktree of its own
  that stays between rounds (`regenerate` in `[agents] worktrees`, else
  `symphony/tasks/land/regenerate-worktree`, locked from the sweep), so what its builds leave
  there stays warm, and runs the generators there in order, a `when_changed` one only
  after an earlier one changed something.
- **A key it passed on before.** A generator with a key (`[land] keys`, by its name, beside
  the generators so a Symphony older than keys still reads the file), a command whose last
  line names everything it reads, runs it first: a key and committed outputs it passed on
  before (`symphony/tasks/land/regenerated.json`) need nothing made. This repository's key
  is the release snapshot's name (`tools/client-key.sh`), so a land that changed no
  client input builds no client. A key names one generator, so the file is refused when
  two `[land] generators` share a name, keyed or not.
- **One commit, landed in seconds, or discarded as stale.** What the generators change,
  inside their outputs alone, is one commit, `Regenerate NAMES for main at TIP`. When
  main moved during the build, it is made on main's tip as it is then, with those
  outputs replaced, only when what they are made of is the same there as at the tip it
  built: no path outside the generators' outputs changed, or each one's key, read again
  on main's tip, is the one it built on. Otherwise what it made is stale: discarded,
  said in its log, never committed, and the newest tip is built next. The commit lands
  under the land's lock: the fast-forward of the main checkout and the push, as a land
  makes them, its interrupt safety included. It needs no reviewer's approve: it changes
  the generators' outputs alone, made by them of what main holds.
- **A red, one task.** A round whose generator fails, runs past `[land]
  timeout_seconds` or writes outside its outputs lands nothing and files
  `tests/main-red-regenerate` at p0, marked red, or adds a line to the one open while
  it stays red; the next round that passes closes it, and a later red files
  `tests/main-red-regenerate-2`, and on.
- **The check no land runs.** Where `[land] check = false` keeps `[verdicts] check` out
  of the land, as this repository keeps the docs audit, each round runs it on what it left
  main holding: the commit it landed, which holds whatever main gained during the build
  beside the outputs, so a land that came in then is checked too; else the tip it took (a
  red round's as committed, or one whose outputs were already current). It runs in its
  worktree, within `[land] timeout_seconds`, its output in the regenerator's log, once
  what the round made has landed, so it never holds back the generators' commit or a land.
  It is part of the round, under its lock, and never cut short because main moved: only
  its time limit or a signal ends it, and a signal ends the regenerator with it, nothing
  judged. A round discarded as stale, or one that stopped, is not checked: the round after
  it checks the newer tip. A check that fails, runs past its time or cannot run files
  `tests/main-red-check` at p0, marked red, a task of its own beside the generators' reds,
  or adds a line to the one open while it stays red; its text says no more than is known
  (one past its time or one that could not run neither passed nor failed). A later round
  it passes on closes it, and a later red files `tests/main-red-check-2`, and on. So a red
  a land let through is on the board within a round of that land, not first found by a
  release run, while whatever runs that check on main refuses (the Symphony's commit of the
  owner's verdicts). Where the land runs the check, every merge passed it, and the
  regenerator runs none.

`just tasks regenerate --status` says where it stands, from
`symphony/tasks/land/regenerate.json`: current on main at SHA; making them of main at SHA
since when, by which process, how far main is past it, and how far behind the outputs
main holds are (the commit they were last found current at, and how many commits ago);
behind, how far main is past what they were last made of; red; or stopped. Where the
regenerator runs the check, the line ends with what it last came to, from
`symphony/tasks/land/regenerate-check.json` (`the check CMD passed on main at SHA (HH:MM)`, or
`is red on main at SHA (HH:MM, tests/main-red-check)`), or that it runs (`checking main at
SHA since HH:MM (pid N)`), or that one begun ended unjudged. It exits nonzero but when current,
by the outputs alone: a red check holds back no pin or release. With `--wait` it first
waits while the regenerator makes them, and then says the same, so a release or a pin
waits on it. `tasks regenerate`
itself runs only as a land starts it (`SYMPHONY_LAND`, which the land hands it alone of its
children, and never `SYMPHONY_UNASKED`), since it moves and pushes main.
A test's board (`TASKS_SERVER`) starts none unless `TASKS_REGENERATE=on`, and `off`
starts none anywhere.

Between a land and the regenerator's commit, main's committed client is older than its
sources, and a server built from main then embeds the older client. The release run waits,
within its time, while the regenerator makes main's outputs, and releases nothing of a
main still behind, filing nothing for it: the regenerator's red is the task then
([RELEASES](RELEASES.md)). A consuming project's pin moved to a commit of main waits the
same way ([its card](DECISIONS.md#pin-waits-for-current-client)): it moves only once `just tasks
regenerate --status --wait`, run in the Symphony checkout, exits 0, and then to the
commit it names. The pin is the project's own tooling, which runs it first; a pin to a
release needs no wait, as no release is cut on a main whose client is behind. A branch
commits no distribution of its own: the land keeps main's. The one that does is a release
candidate's fix that changes what the client is built from, since no regenerator builds the
candidate: it commits the distribution its own sources make (`just client-update`), which the
candidate's rerun checks, and the land keeps main's side of it all the same
([RELEASES](RELEASES.md#the-release-candidate)).

## Gates

A task's gates are every stamp or check it needs to land, answered for its recorded
branch at its tip: `GET /api/task-gates?slug=SLUG`, the wire type `TaskGates` of
`symphony_api::gates`, built by `crates/symphony/src/gates.rs`. Each gate has a
name, a state (`missing`, `held`, `unknown`, `waived` or `met`), who and when
(server time, seconds), the commit it holds for, why for a waiver or a hold, a
one-line detail, a link to its evidence, and the states it took before, oldest
first, for the timeline. They come in that order of states: what the task waits
on first. The task's page draws them in one section under its head, with what
the task waits on and how many gates are in each state; on a phone a gate is
three lines, wide one row of bounded columns, a part whose words need more room
wrapping within its own, so every word shows and none waits on a hover
(`apps/symphony-ui/src/task_gates.rs`). A link is kept only for a path of the
app or an `http(s)` address (`symphony_api::linkable`): the server drops any
other where it puts the answer together, and the page draws any other as words,
so a plugin cannot hand a page a `javascript:` link in the origin every project
shares. A path of the app is written under the project's base, as every link the
client draws is. Until it is read, the section is its own outline, a shadow row
for each gate it expects: the shape of the last answer the server gave for the
task (how many gates, how many waited on, how many with no who and when, whose
rows draw none), which the task page carries (`TaskPage::gates`) and the server
forgets once the task closes, else what this browser last drew, at the height it
drew it, else four, and nothing when it knows there are none. It is read when it
nears the screen and again when the task's record changes, one of its builds or
tests starts or ends, the branch it was answered for has a new tip (the feed's
`branches` region, the branches whose refs moved), a verdict lands on its thread
or a card in a document it links comes or goes; never for a plain post. The page's
feed counts those events for the task it shows, and the section reads again when
the count grows. A newer read aborts the one before.

The Symphony answers three of its own:

- **review**: the reviewer verdicts on the task's thread, one round each
  (`gates::review_rounds`, the Symphony's one reader of review rounds; the board's
  own commands in `crates/tasks` read the verdict words apart): met by an `approve`
  in the last round, missing until then, the earlier rounds its past. A task done
  with no round landed without review, as a bug fix may
  (RULES.md `workflow/fast-bugfixes`): waived. A subtask
  has none: only its top-level task goes to review.
- **check**: the project's `[verdicts] check`, its words joined by spaces, as the
  board's recorder labels a run of it ([MACHINE_WORK](MACHINE_WORK.md#recording-builds-and-tests)),
  at the branch's tip: met when the newest such run there was green, missing when it
  was red or is still going, unknown when none ran at the tip or the repository has
  no such branch. A project with no check has no check gate.
- **owner**: held while a card the task posted waits on the owner, by the
  sign-off lane's rule above; absent otherwise.

A project's plugin answers its own through the gates seam (`symphony_plugin::Gates`,
`Plugin::gates`) as the same typed data, so every project's gates are drawn the
same way and can be counted. A plugin's gate with the same name as one of the
Symphony's takes its place. The server asks all of a project's answerers at once
and keeps each answer by the task's status, its branch's tip and what the answerer
says it reads besides (`Gates::kept_while`), so an answer that starts a landing
binary runs once a tip. A read waits 10 s for an answer. Past that, or when the
answerer errs or panics, each gate it answered before shows `unknown`, the error as
its detail, in place of the Symphony's of that name (before it ever answered, one
gate named after the answerer), and the rest of the answer still arrives. A late
answer is kept for the next read, and an error is asked again after 30 s. A call
still running after 60 s is abandoned and asked again; while four of one
answerer's calls run, abandoned ones among them, none starts, so an answerer bounds
its own work.

## Subtasks

Every instruction an orchestrator gives an agent is a **subtask**: a task made
with `--parent`, whose description is the instruction, word for word. The agent's
whole prompt is then `/work SLUG HANDLE` (`.agents/skills/work/SKILL.md`), never a brief
in a file. A parent's subtasks nest to any depth; a task is never its own
ancestor (`just tasks parent SLUG PARENT` moves one, and refuses a cycle).

```
just tasks create perf/x/review "Review perf/x" --parent perf/x --for amber-heron --instruction --body-file - <<'EOF'
(the instruction, verbatim)
EOF
```

`--body-file -` reads the body from stdin, so a long instruction needs no file.
A consuming project's optional brief guard refuses writing a brief to a file: to
`artifacts/orchestrator/` (which must not exist), or to a brief in a session scratchpad (a file named
`brief*` or `*-brief.*`, or under `briefs/` or `followups/`), and says to
create a subtask instead.
A consuming project's optional wait guard refuses a loop waiting on machine-wide process state
(`pgrep`/`ps` for verify, test-sim, godot): wait on your own run's log or PID.

- `--for HANDLE` reserves it for that handle with no lapse of the hour (the
  handle is one the orchestrator registered for the agent). `next` offers it to no
  one else, and only that handle can claim it. The reservation still drops when
  that handle reads `lost` or `left` (ten minutes without a sign of life, holding
  nothing claimable yet), after which anyone may claim the task: start the agent
  promptly, or register the handle again and re-reserve.
- Every subtask closes with its executor's `just tasks done SLUG "integration commit and focused result"`, including ordinary implementation slices. Only the top-level parent enters review, gathers stamps and lands. `--instruction` still marks an agent instruction, but is not required for lightweight completion. It does say whose work is on the parent's branch: a slice's executor wrote it and gives the parent no verdict, an instruction's (a review) did not ([Tasks and roles](#tasks-and-roles)).
- Source slices start from their parent feature branch, commit there or return commits for integration, and run only their focused checks. A separate agent is justified only by useful parallelism. No independent slice review or landing. A parent's gate is evidence too, the tests closest to its change, which its reviewer judges; the full verification is no task's, and is the release run's ([CHECKS](CHECKS.md)).
- `just tasks show` lists a task's subtasks with each one's status and holder;
  `just tasks list` and the Symphony's **tasks** page, under its **Task
  groups** heading, show the same tree, at any depth (`list` leaves finished
  tasks out unless `--all`).
- Events climb: a message to `@orchestrator`, any verdict and any
  status change on a subtask reaches the orchestrator of the nearest task, at
  or above it, that has one, so the orchestrator takes the role on the root
  only. A subtask handed to another orchestrator is that one's, with what is
  below it. A message to a handle or to another role stays with them.

## Limits and models

Every work-in-progress limit, and the model each role is delegated to, is a setting, so a change
of conditions or of mind is one edit of one file. Each is a key of the project's `symphony.toml`,
read from the repository's main
checkout as `on_review` is, so a branch cannot change its own (`crates/tasks/src/limits.rs`,
`crates/tasks/src/launch/defaults.rs`):

```toml
[limits]
orchestrator_slots = 8  # the open tasks an orchestrator holds (Slots, below)
low_volume = 4          # PULL's low_volume line below this many open slots; 0 says it to none
machine_agents = 12     # the agents this machine runs

[agents.models]
orchestrator = "fable"
executor = "opus"
reviewer = "opus"
```

- **Defaults.** A key not written takes its default: `orchestrator_slots` none (no cap on an
  orchestrator's open tasks), `low_volume` 4, `machine_agents` none (no limit on the agents a
  machine runs), and `orchestrator`, `executor` and `reviewer` none (a launch takes `[agents]
  launch`'s `model`, else the board's own, as **Models** below says). `orchestrator_slots` is
  also read from `[board]` where `[limits]` names none, where a build that does not know
  `[limits]` reads it; written in both with two values, `[limits]`'s holds and the other is
  said on stderr.
- **`machine_agents`** counts every agent the machine runs, whoever started it, as the staffing
  pass counts: each hc terminal running an agent, and each live worker, reviewer or
  orchestrator session of this board with no terminal of its own
  (`crates/tasks/src/launch/running.rs`). `just tasks launch` refuses at the limit (`no
  launch: this machine runs 12 agents, at its limit of 12 ([limits] machine_agents in PATH)`),
  and when hc cannot be asked to count them. `just tasks next` warns past it on stderr and
  still names the task: whoever asks runs already, and refusing it work frees nothing. The
  staffing pass defers at the lower of it and its own `max_sessions`.
- **Models.** A launch of an executor or a reviewer, by hand or by the staffing pass, starts
  `[agents] launch`'s agent on its role's model, else on `[agents] launch`'s `model`, else on
  the board's own; `--model` and `--kind` win, and a role's model goes with the project's
  agent. No board launch starts an orchestrator: with no model written for it, whoever starts
  one chooses. An agent starting a subagent passes the model `just tasks limits` names for the
  subagent's role.
- **`just tasks limits`** prints each limit, the pipeline alarm's thresholds
  ([The pipeline alarm](#the-pipeline-alarm)) and each role's model, each with where it came
  from (`[limits] low_volume, line 8`, `[agents] launch model, line 12`, or `default`), the
  machine's count of agents under `machine_agents`, and each key of `[limits]` this build does
  not know, left out.
- **A change holds at once.** Every command reads the file as it stands, and a store kept open
  (the Symphony's server keeps its reader and its data worker's) reads it again at its next
  read whenever the file's length or time changed: no restart.
- **A value the board cannot read** is said on stderr with its line and takes its default
  (`⚠ PATH: [limits] low_volume (line 3) is -1; it is at least 0; it takes its default, 4`): a
  broken project file must not stop the board, nor the landing that mends it. A load of the
  project and `just symphony-status --lint` refuse it in the same words, as they refuse a key
  the Symphony does not know, by name and line (`[limits] wip (line 4) is not a setting the
  Symphony knows`).
- **Staging a new key.** A build older than a key does not know it: its `just symphony-status
  --lint` and its load of the project refuse the file, naming the key, its server reads the rest
  and says the key is left out, and its board never reads it. So a project writes a new key only
  once every build that reads its file knows it: the machine's shared server, and the `symphony`
  and `tasks` its recipes and lands run (its main checkout's `.target/bin/` links), which a
  project taking the Symphony as a Git dependency builds at the Symphony commit its
  `Cargo.lock` pins, so that pin must be one that knows the key. Until then the setting stays
  where those builds read it (a cap in `[board] orchestrator_slots` holds for all of them); once
  they all know the key, the value moves, and while builds are mixed it can be written in both
  tables with one value, the older key dropped after.

## Slots

An orchestrator holds at most a set number of open tasks, its slots, so work is
pushed to done before new work starts (`crates/tasks/src/slots.rs`).
The number is the project's: `orchestrator_slots` in the `[limits]` table of
`symphony.toml` ([Limits and models](#limits-and-models)), else in its `[board]` table,
where builds before `[limits]` read it, from the repository's main checkout, as
`on_review` is, so a branch cannot raise its own cap. With no key, or no file, nothing is
capped. A value that does not parse, or one below 1, is said on stderr with its line
(`⚠ …; no cap on an orchestrator's open tasks applies`) and caps nothing.

- **Open** is `in_progress` or `review`, or `blocked` under a hold that keeps
  the slot (below). Other `blocked` work, `backlog`, `concept`, `design` and
  `ready` hold no slot; a slot frees when its task is `done` or `cancelled`,
  or leaves the open statuses another way.
- **Each open task, one slot.** A subtask holds a slot of its own, so
  gathering open work under one task frees none. Two kinds of task share the
  slot of another: a review mirror (`review/BRANCH`, `review-N/BRANCH`,
  `SLUG/review`) counts with the task it reviews when the board holds that
  task, and an instruction subtask (`instruction: true`) with the nearest
  task above it that is not one.
- **Whose slot.** The orchestrator of the task, or of the nearest task above it
  that has one whose session still holds ([Subtasks](#subtasks)); a task two
  orchestrators hold counts against both, and a start is refused if either is
  full. A task no orchestrator holds yet, until the board gives it one
  ([Tasks and roles](#tasks-and-roles)), counts against the parent session
  (`register --parent`) of the session starting it, or of its executors once
  open. No cap refuses the board's give: the task then counts against the orchestrator given it.
  The one give a cap decides is a move's, which keeps the moved work with the orchestrator it had
  (below), decided by that move's own cap read.
- **Where it bites.** A claim into `in_progress`; a move into `in_progress`
  or `review` from a status that holds no slot, blocked work going back to
  review included; a dependency change that sends blocked work back to
  `in_progress` or `review`; taking the `orchestrator` role on a task with
  open work at or below it, or being handed it (`handover`, refused as the
  receiver's take would be); and a change of parent that brings open work at
  or below the task under an orchestrator's tree, or takes it out from under one, which keeps it
  that orchestrator's in a slot of its own. Each is refused when it would raise an
  orchestrator's open tasks past the cap, naming them and their status, a
  shared slot's tasks joined by `with`:
  `sim/x cannot start: cobalt-otter holds 8 open tasks, the cap of 8 an
  orchestrator holds: sim/a (review) with review/sim/a (review), …; a slot
  frees when one of them is done or cancelled`. The count is the number of
  entries listed. `tasks api` holds the same cap. A start inside a slot
  already open (a mirror of a task in review, an instruction for open work)
  is never refused.
- **A landing's hold.** A landing that holds a task in review blocked while
  every stamp its branch lacks is one nobody is producing does it with a hold
  (`Store::hold_blocked`, `tasks api`'s `hold`, api version 4): the task
  stays blocked, as any blocked task does to everyone on it, and keeps its
  slot, shown as `(blocked, held)`, so its move
  back to review starts nothing and is never refused. The hold is the record
  `counter:['hold', SLUG]`, stamped with the task's `updated` as the hold left
  it, and ends with the task's next move (a landing's close and the owner's
  moves included: every status write deletes it), change of parent or of
  dependencies.
  Work blocked by any other move holds no slot, and takes one to come back.
  The PULL header and the agents API do not read holds: they count a held task
  as the blocked task it is.
- **Red work takes no slot.** A task filed for a red that stops every
  landing is marked so where it is filed (`create`'s `red`, naming what is
  red, `tasks api` version 7; kept as the board field `fields._board.red`,
  `crates/tasks/src/red.rs`). Only a red's filer marks: a session of
  kind `filer`, which `tasks api`'s `register` takes and the command line's
  `register` does not, as the project's tools that file reds register. A
  create naming `red` from any other session is refused, and `set_field`
  refuses the board's own `_board` fields. The mark goes only on a task of a
  series reds are filed in (`red::SERIES`: `tests/main-red-*`, a red verify
  of main, `tests/release-red-*`, a red release run, and `tests/nightly-red-*`,
  its name before the rename and a consumer's nightly's), whoever marks it, so
  no feature takes it; never on a task named under one (`red::filed`: no `/`
  past the series' prefix), which the release run's reading of its reds and
  land's of the standing reds take for ordinary work too. `mark_red` marks an open red its
  filer filed before it marked reds, and makes it p0, once: what it judges is
  read in the transaction that writes, so two at once post one line. While the
  marked task is open, it and the work under it take no slot: subtasks,
  review mirrors and instructions are judged by the task whose work they are
  and the tasks above it. Red work starts at any count and frees none. A red
  done or cancelled exempts nothing: the work under it counts again, new
  work there included. Its close is never refused, though it raises its
  owners' counts: whether or not the closing store caps starts (the
  release run's does not), it holds their slot counters and the red's own,
  `counter:['red', SLUG]`, which every start of work under the open red holds
  too, so a start decided on a board read before the close is decided again,
  counted. A close decides what it holds on the task as it read it, and its
  write goes on only from that version, so a red marked between the two
  (an earlier red the release run or main's verify marks) is read and decided
  again, and a task it read as not on the board is refused there, never
  closed unfenced as a red filed since. `unmark_red`, from a filer too, lifts
  the mark of an open red that stays open as ordinary work (main's green
  reached it once work on it started): it and the work under it count from
  then, decided and held as a close is, never refused; a task unmarked or
  closed is left as it is. A refusal names the owner's open red work after its
  counted slots: `…; a slot frees when one of them is done or cancelled; not
  counted, as red work takes no slot: tests/main-red-abc (in_progress)`. The
  mark alone tells; a slug never does. The PULL header's `slots=` and the
  agents API count no red work. As with review, nothing authenticates a
  session: a filer is honour-based, guarding against a mistake, not malice.
- **What it leaves alone.** The owner's moves, changes of parent and
  hand-overs are not capped.
- **Not retroactive.** An orchestrator above the cap keeps every task and
  works them; it starts nothing new until it is below the cap.

Two starts at once cannot both take the last slot: every start that changes an
orchestrator's open tasks, one into a slot already open included, holds
`counter:['slots', HANDLE]`, read in the same transaction as the task it
starts, and the write that loses is decided again on the new board. The cap
reads the open tasks through the `task_status` index, the holds as a range of
record ids and their tasks by id, the tasks above them and the tasks they
review by id, a level a round trip, and their roles through `role_slug`; it
reads no table whole (`tests/store/slots/cost.rs`). Each
orchestrator's PULL header shows `slots=OPEN/CAP`, and the agents API its
`slots` ([The agents API](#the-agents-api)).

## The pipeline alarm

Work piling up in review is said, not left for someone to notice
(`crates/tasks/src/alarm.rs`). Review holding more than twice the work
in progress for 4 hours raises the alarm; it clears once review has held no
more than twice in progress for an hour, so a short dip does not lower it and
a short rise does not raise it. Exactly twice is not over; review with nothing
in progress is.

- **Thresholds.** The project's, in the `[board]` table of `symphony.toml`, read
  from the main checkout as the cap is and printed by `just tasks limits`
  ([Limits and models](#limits-and-models)):
  `review_alarm_ratio` (2), `review_alarm_hours` (4) and
  `review_alarm_clear_hours` (1), whole numbers of at least 1. A key not
  written takes its default; a value that does not parse, or one below 1, is
  said on stderr with its line (`⚠ …; the pipeline alarm takes its default
  thresholds`) and the defaults apply.
- **What counts.** Work only: a review mirror counts with the task it reviews
  and an instruction subtask with the task it serves, as for
  [Slots](#slots), neither on its own, as the tree stood at each moment: a
  mirror counts on its own until the task it reviews is made, and an
  instruction subtask from when it leaves its parent (a move's line names the
  parent it left, `HANDLE put it under PARENT (was PARENT)`), so a task made or
  moved changes the counts from then on, never the hours before. A task in
  review whose branch's tip a reviewer asked changes of is rework (below),
  counted in progress.
- **From the record.** The board keeps no history of counts. Every change of a
  task's status writes a line on its thread (a claim, a move, a verdict, a
  landing, a block by a cancelled dependency and its end), so each reader walks
  the last day's lines (or twice the raise and clear spans, if longer) back
  from each task's status now and replays the alarm over the counts. What held
  when that window opened is read, never assumed: a task's status then, where
  its oldest line in the window is a claim or a landing, and the record of
  what its landing waits on then, for one in review some time in the window,
  come from its newest such lines before the window, read back past any field
  value whose text only reads like one; and when the window holds neither a
  raise span over nor a clear span under, so the alarm's state at its start
  decides, a window twice as long is replayed, until one does or it opens
  before the board's first task, when the alarm was lowered. Every reader at
  one moment finds the same alarm, and
  nothing writes it. The window's lines are read through the message time
  index and those before it from each task's own thread; no table is read
  whole.
- **What it says.** While it stands, every orchestrator's PULL header ends
  `review_alarm=REVIEW/IN_PROGRESS`, the whole board's whoever the digest is
  for, and the lines after the header say the counts and since when, then the
  5 tasks longest in review, longest first, each with when it went in and
  what its landing waits on:

  ```text
  PULL 14:05 executing=4 … idle=0 review_alarm=10/4
    review_alarm: review 10, in progress 4, more than 2× since 09:40
    review_alarm p1 tools/x — X   (in review since 11-07 21:12; missing certificate, review)
  ```

  In the hour that clears it, the line says `at most 2× since HH:MM; it clears
  after 1 h of that`. Its raising is an entry of its own, so the monitor
  delivers it once, as it delivers any new entry.
- **What a landing waits on.** The board holds no stamps; the tool that lands
  a task records what its landing lacks in the board field
  `fields._board.waits` (`Store::record_waits`; `set_field` is a plugin's): the tip it judged, `: `, then
  each name, `, ` between them (`0123abc…: certificate, review`), none after
  the colon when it lacks nothing. The alarm shows the names as written
  (`missing certificate, review`), `missing nothing` for none, and `its tip is
  newer than its landing's last look` when the branch has moved past the tip
  the record judged. A task with no record shows no note. Each record names
  the assessment it states, kept with when it was written
  (`fields._board.assessed`): the same assessment retried writes nothing,
  and a fresh one writes its record again however like the last it reads.
  Stamp and approval reassessment, refused landing attempts and completed tip
  verification submit these assessments through a detached, owned delivery.
  It checks the current branch and task state, evaluates stamps and merge
  conflicts, and records `changes` only for an effective reviewer veto. A
  superseded interrupted verification supplies no completed assessment.
  Local submissions and receipts live in the folder the project's `[board]
  assessments` names (a `.git/` prefix names the common directory; none when
  unset, and an assessment is then refused); the delivery child is stopped and
  reaped after 30 seconds or checkout removal. A delayed board, an older API or an unconfirmed reply
  leaves pending evidence; `just land-stamps BRANCH` retries the latest
  submission, preserving a committed occurrence's identity, time and line.
  Pending evidence alone does not confirm a board write.
- **Rework.** The name `changes` in a record says a reviewer asked changes of
  that tip. While the branch's tip is still the one judged and the branch has
  not moved past it since the record, the task is rework: PULL counts it in progress (its live executor as executing, and one
  nobody executes as `in_progress_orphaned`, noted `in review; changes asked
  of its tip`), not as `review_unreviewed`, and the alarm counts it in progress
  from the record to the next; a change of status ends nothing, since the
  record judged a tip, which a move into progress and back keeps. The first
  commit the branch moved to from the tip judged, after the record, ends it,
  from the moment that commit was made or the branch moved to it, however late
  its record comes and whatever follows (more commits, the task taken back
  into progress): the task waits on review again. The moves come from the
  repository the reader runs in, as the branch's reference log tells them: the
  next commit made on the tip judged, or what replaced it, amended or rebased
  away. Without that log, the first commit after the tip judged on the
  branch's line of first parents ends it, never one that reaches it through a
  merge's other parent; a reader that cannot see a task's branch takes each
  record as judging the tip of its time, and one that cannot place the tip
  judged takes the tip now as the first after it. The board keeps the first
  end it learns of each record (`fields._board.fixed`), and nothing later
  moves it: not another commit, an amend or a rebase, nor what the repository
  tells after. A new record of changes on a tip the branch came back to, from
  a fresh assessment, is a request of its own, whatever its text: it waits on
  the branch's next move from that tip. The record answered, retried, is no
  new request: PULL and the alarm count the task in review alike.

## Where the time goes

The board keeps no table of status changes. Where a task's time went is read
back from what it already stores: the task's `created`, `updated` and `status`,
and the `system` and `verdict` lines its changes left on its thread
(`crates/tasks/src/stages.rs`). The store builds its move and claim lines
with that module's functions and the release, block and unblock lines from its
constants; the verdict words and the `(was STATUS)` ending are written out on
both sides.

| line | what it changes |
|---|---|
| `HANDLE moved it FROM → TO` | the move it names, the owner's too |
| `HANDLE claimed it[, handed over from …]` | ready work is in progress; a claim of work in another status changes none |
| `HANDLE released executor (released)`, `HANDLE released every role (left)` | work in progress whose last executor let go is ready again |
| `blocked: cancelled dependencies: … (was STATUS)` | blocked |
| `unblocked: no cancelled dependency left; back to STATUS` | the status it names |
| a `changes` verdict | review back to in progress: a bounce |
| any other verdict (`approve`, a subtask's `done`, a landing) | done |

The lines are replayed in the order of their message ids, the order they were
written in: a line's time is its writer's clock, so one dated before the line
written ahead of it is taken at that line's time. A line naming the status
the task already holds changes nothing, and nothing after the line that closed
the task is read. The status a task was made in is the one the first line that
names a status says it left; with no such line it is the task's own status
where a task can be made in it, else `ready`. Where the replay ends in another
status than the task holds (a change the thread does not record), that change
is dated at the task's `updated`. An open task is counted up to now; a done or
cancelled one stops at the line that closed it, and the closing status itself
counts no time. The statuses' times add up to the time from creation to close
(or to now).

`just tasks show SLUG` prints it in one line under the task's roles and links:

```
  Stages: ready 0.4 h · in_progress 1.3 h (2 visits) · review 0.8 h · 1 bounce; first claimed after 0.4 h
```

Each status is in the order the task first held it, with its visits when more
than one; a bounce is review sending the task back to in progress, by a
`changes` verdict or a move. After the `;` comes the wait from the task's
creation to its first claim (`never claimed in 20.0 h` when nobody claimed it:
to its close, else to now). That wait is a part of the statuses before it,
usually the time in `ready`, and is never added to them.
`just tasks show SLUG --json` carries the same as `stages`, every duration in
seconds and every time in epoch seconds:

```json
"stages": {
  "statuses": {
    "ready": {"first_entered": 1800000000.0, "seconds": 1440.0, "visits": 1},
    "in_progress": {"first_entered": 1800001440.0, "seconds": 4680.0, "visits": 2}
  },
  "segments": [
    {"status": "ready", "from": 1800000000.0, "to": 1800001440.0, "by": "glacial-merlin"},
    {"status": "in_progress", "from": 1800001440.0, "to": 1800003600.0, "by": "quiet-wren"},
    {"status": "review", "from": 1800003600.0, "to": 1800006000.0, "by": "quiet-wren"},
    {"status": "in_progress", "from": 1800006000.0, "to": null, "by": "lucid-heron"}
  ],
  "unclaimed_seconds": 1440.0,
  "first_claimed": 1800001440.0,
  "bounces": 1,
  "current": {"status": "in_progress", "since": 1800006000.0},
  "closed": null
}
```

`segments` are the replay's stretches in the order the task held them, each
with who moved it there (`null` where its line names nobody, as a block's or
an unblock's): they follow one another from the task's creation, the one it
holds has no `to`, and their times add up to the statuses'. No table is added
for them; they are the same read.

`just tasks list --json` carries, on each task it lists, `in_status_since`:
when the task took the status it holds, the replay's `current.since` (its close
for a done or cancelled task; the task's `updated` where its thread does not
record the change). `just tasks search --json` carries it too, its rows being
list's, and `tasks api`'s `in_status_since` (api version 14) answers it for the
tasks its `slugs` name, by slug, refusing a slug no task has. Each reads the
tasks it names and their status and verdict lines in one request however many
there are (`Store::in_status_since`; `tests/cli/stats.rs` counts them over 300
tasks); `list` and `search` read nothing more without `--json`.

`just tasks stats [--days N] [--json]` (14 days unless `--days` says, from 1
to 3650; no session needed) is the same reading over every top-level task
made in the window, subtasks left out. Every figure is of those tasks but the
tasks done per day, which its header says:

- how many hold each status;
- for the done ones, the lead time (creation to close) and the time in each
  open status, each as median, p75 and p90 (a value between two tasks is
  taken proportionally between them); a status is spread over the done tasks
  that held it, and says how many did;
- on a line of its own, the done ones' wait before their first claim: a part
  of the statuses above it, not a row to add to them;
- how many tasks review sent back how often (`0 × 41 · 1 × 6 · 2 × 1`);
- top-level tasks done on each day of the window, whenever they were made (a
  task made a month ago and closed yesterday counts yesterday), by this
  machine's time zone, every date of the window named once: each task done
  now, once, on the day of its last close (`Stages::done_at`), so one approved,
  sent back to review by its landing and approved again counts once, at the
  second approve, and one cancelled or reopened since counts on no day. The
  Reports page's Tasks done counts the same tasks by the same rule, by UTC
  day (its day files' `done`, beside `entered.done`, which counts every entry
  into done);
- the ten open tasks that have held their status longest, with that status
  and how long;
- how many tasks are blocked and how many are ready with no executor, each
  with the median time they have been there.

`--json` carries the same figures as `by_status`, `lead_seconds`,
`stage_seconds` (by status; each spread is `tasks`, `median`, `p75`, `p90`),
`waiting_before_first_claim_seconds` (beside `stage_seconds`, never in it),
`bounces`, `done_per_day`, `slowest`, `blocked` and
`ready_unstaffed` (`tasks`, `median_seconds`), every duration in seconds.

`stats` reads the window in one round trip (`Store::stage_records`): the
top-level tasks made since the window's start and the done ones written since
(a task closed in the window is one of them), who executes them, and each one's thread
by its own range of keys with the posts left out, never a thread at a time.
With the read that opens the board that is two requests whatever the board
holds (`tests/cli/stats.rs` counts them over 300 tasks).

## The timeline

A task's page draws what the task went through under its [gates](#gates) and
subtasks, above the brief:
`GET /api/task-timeline?slug=SLUG` (400 with no slug, 404 for a task not on the
board) answers the wire type `TaskTimeline` of `symphony_api::timeline`, built by
`crates/symphony/src/timeline.rs` from the task alone, never the whole board, from
one read of its thread and of the recorder's history that the gates answer shares.
The page reads it once its box nears the screen, again at once when the task
changes, and each `every` seconds (a minute) while the task is open and the page is
shown, since a build or a gate moves without the task changing; a hidden page reads
nothing, and once shown again reads once. Each read names the last answer's
validator, which leaves out the answer's `now`, so a minute in which nothing moved is
a `304` and the page counts `now` on by its own clock. A read that fails leaves the
last answer shown; with none, the box says why in a sentence, grows to show its Try
again, and reads again a minute on. Only the box of the task the page shows draws
its timeline: an answer that arrives after the page moved to another task, or after
its box closed, is dropped. Every time is server time in seconds; `zone` gives the server's
offset from UTC at the task's making and at each change of it after (summer time),
so each time reads on the server's local clock as it then stood, the thread's.

- **stages**: the segments of the replay above, which goes on past a close a move
  out of `done` or `cancelled` reopens, so a reopened task keeps its history and
  the bounces after it. A stretch in review is a round (`round`, from 1). Where the
  round's verdict did not close the task (a project's review gate turning met while
  the task waits on its landing), the stretch after the approval is a stage of its
  own, `approved`: the wait to land.
- **claims**: who held the executor role while the task was in progress, from the
  claims, hand-overs and releases on its thread (`stages::holds`).
- **rounds**: each review round, from the task going to review: `taken` when a
  reviewer took the role up (its start when one held it already), the time before
  it the wait for a reviewer; `to` and `verdict` the round's end, by the board's
  `approve` or `changes` verdict or the review gate's turning met; `by` the reviewer.
- **runs**: the builds and tests the board's recorder kept for the task
  ([MACHINE_WORK](MACHINE_WORK.md#recording-builds-and-tests)), in its history or
  running now, a step inside one of them left to that one: phase, label, who,
  commit, host, when queued, started and ended, and the result (`queued`,
  `running`, `green`, `red`, or `unknown` when its recorder ended before it could
  say). None until the recorder keeps one for the task: the page then draws no
  builds or tests at all, and no machine time. Where the recorder's history no
  longer reaches back to the task's start, `runs_from` says since when it keeps
  every run, and the page says so.
- **gates**: each gate's states from the [gates](#gates) answer, its past and its
  current, a lane each by name; a state taken before the task was made counts from
  its creation, and the review gate's from the task's first move to review. A gates
  answer that cannot be read leaves the lanes out, never the timeline.

The page keeps a box for it (`apps/symphony-ui/src/timeline_slot.rs`) as tall as
this browser last drew the task's timeline, at most 240 px on a phone and wide (the
owner's choice, so the brief and the thread start near the page's top), with
placeholder rows in it, so nothing on the page moves when the timeline comes or a read
adds a step: the steps fit the box, what does not fit fading out (the latest steps in
view on a phone, the first lanes wide), and "Show all" lets it grow.
The timeline itself is a module of its own (`apps/symphony-ui/src/task_timeline.rs`,
the page kind `timeline`), which the shell loads once the page has drawn and is idle,
so the page's first visit does not wait for it. On a phone it is a list: a row per
stage with its start, its label, its length and a bar as long, the wait in it drawn
lighter; under each stage the builds, tests and gate changes that began in it (the
review gate's are its rounds), a build or test named by its recipe, the machine's by
what it ran and its commit; the verdict after a review's bar, and a striped bar for a
run going on now; a stage left within half a minute has no row. From 900 px wide it
draws lanes over the same hours: the stage, the executor, the reviewer, builds and
tests (runs that overlap a row each), and a thin row per gate, each state by its
pattern as well as its colour (met solid, missing dotted, waived dashed, held
double), with a line at now. A word in the lanes is drawn only where it fits; every
part of them is said to assistive technology and reached by the arrow keys, and the
list stays for assistive technology beside them. The pictures it follows are
`docs/analysis/reports-timeline-phone.svg` and
`docs/analysis/reports-timeline-wide.svg`.

## Talking on a task

Every task has a thread, and the thread is the way to reach whoever is on the
task, whatever worktree or machine it runs in:

```
just tasks say sim/jump-departure "the dock ring is at 900" --to @reviewer
```

`--to` takes a handle, a role (`@executor`, `@reviewer`, `@orchestrator`,
`@reserved`), or `owner`. Addressed messages reach the matching native callback
as plain `MSG` event lines. Without an address, an agent's post stays on the
thread; the orchestrator reads it in the app or with `just tasks show SLUG`.
Verdicts reach the executor and orchestrator. The owner's word, its posts, its
verdicts and its moves (the thread's `owner moved it FROM → TO`, which a channel,
`just tasks wait` and the inbox carry as a `MOVE` line, never again as a
`STATUS`), reaches everyone on the task and the task's orchestrator, with no
orchestrator relaying it. On a task no live orchestrator holds it reaches every
orchestrator too: one nobody holds (none on it or above it, or only a lost or left
one, in the interval before the board gives it one, [Tasks and roles](#tasks-and-roles)), its copy marked `[unowned: no orchestrator holds it]`, and one whose nearest
holder is stale or of unknown liveness, marked with that holder,
`[unowned: no live orchestrator holds it: HANDLE (stale)]`; that holder hears it too,
unmarked (`Store::inbox`, `crates/tasks/src/store/messages.rs`). Only a live holder
keeps it to itself. The owner's cancel adds `; it moves no further: stop work on it`
to its `MOVE` line, so the executor and reviewer stop at once (the owner's `done` is
an approve verdict, which says no more). The thread keeps every line as written. Post milestones only, address what someone must act on, never
acknowledge a message, and never repeat a post on a subtask and its parent.
A session without a channel keeps `just tasks inbox`.

The owner reads the board and every thread in the Symphony, sends messages
to anyone on a task, changes its priority from p0 to p4 and cancels open tasks.
Cancellation asks for confirmation; done and cancelled tasks cannot be cancelled
again. The board records these writes as `owner`. In the shared app, cancellation
also closes the task's sessions and starts the worktree sweep, as the CLI does.
An agent message addressed to `owner` rings the app's 🔔. Creating, claiming,
other moves and verdicts go through agents. The app files no task: what the owner
says that waits on an agent is an [ask](#asks).

## Asks

An **ask** is one thing the owner waits on an agent for: a verdict with a note or
a rejection, a message on a card, a comment in a document, a chat, a message on a
task's thread. It is a record of its own (`ask:N`, schema 17), never a task: it has
no stage, no claim and no review, and no task count includes it. Each one says
what it is about (the card on its document, the commented block, the chat's
thread or the task, and the worktree's branch when the owner said it on one), the
owner's first words (cut at 200 characters), when they were said, how many lines
followed, who holds it now and since when, and each earlier holder with when it
had it and why it moved on.

While an ask is open, another line from the owner about the same thing joins it:
the line is counted, and the ask keeps its first words and its age. A verdict and
a message on one card are one thing. A line after the ask was answered opens a new
one. The first answer closes an ask, whoever holds it, and a later answer changes
nothing. An agent closes an ask that needs no answer, such as a thank-you, with
`just tasks asks close ID "WHY"`. An open ask shows as late from ten minutes
(`LATE_SECONDS`, the same for every kind), and an answered one never does: its
age is how long it waited for its answer. The
owner's words and the answers stay where they were written: the ask records only
what it is about, who has it and whether it is answered.

`just tasks asks` lists the open asks, the oldest first:

```
ask 3  verdict  card on docs/DECISIONS.md @tools/x  14 min, late  held by umber-stoat for 12 min
  "why not B?"
```

`tasks api` opens, holds and answers them: `open_ask` (the kind, what it is about,
the words and, if not now, `at`), `ask`, `asks` (`since`: answered ones too),
`hold_ask` (`holder`, none for nobody, and `why`), `answer_ask` and `answer_about`
(`by`, an agent's handle, and `answer`, where the answer is), and `close_ask`.
The [design](analysis/owner-asks.md) has who holds an ask and when it moves.

Who opens and closes them:

- **A save in the Symphony.** A verdict that leaves its card waiting on an agent
  (a rejection, a note, an approval its card kind says an agent applies), a
  message on a card, a comment and a chat question or follow-up each open their
  ask, or join the open one, in the same board write that queues the save's chat
  event: the save's journal record carries both, and the shared server hands
  them over once (`Store::owner_save`). A rejection with no note queues no chat
  and opens its ask alone.
- **A message on a task's thread.** The owner's message from the app opens the
  task's ask in the message's own write. A line on the thread by a session
  holding a role on the task, or by the one the owner's last line there was
  sent to, answers it. A line the app posts as the owner for a board action with
  no note opens none.
- **The shared server's clock.** It closes an ask once its answer can be read:
  one on a card once main's page holds the card, on the branch's page the owner
  saw it on or as main's, waiting on no agent; one on a comment once the comment
  is resolved, in its document (the worktree's for a branch's) or in the review
  ledger the tidy archived it to; a chat once its thread in the ask store is
  answered. Main's copy of a card answers, or keeps waiting, an ask said on a
  branch only when that copy carries the owner's line, one of the owner's lines
  dated from the minute before the ask on: the branch merged, or its line laid
  on main's copy for a finished branch or a card the branch did not post.
  Without it, main's copy says nothing of the branch's agent. It names who
  answered: a chat's answering session, and on a card or a comment the session
  that wrote the answer with `just symphony-reply PATH
  SLUG TEXT --as HANDLE` (a comment's slug is `L` and its callout's line). The
  register's lines name only `agent`, so an answer written into the file by hand
  is credited to no one. It looks when the documents were read again or the ask
  store changed, and a minute after it last looked
  (`owner_asks::RESCAN_SECONDS`). An ask the owner said more to after the
  documents were read waits for the next read. A private server opens and
  closes none.
- **Gone is no answer.** An ask whose card, comment, document, worktree or chat
  thread is gone (the task cancelled, its worktree removed, the document
  deleted) stays open, and `just tasks asks` says what is gone and for how long
  (`gone` and `gone_since` in `--json`). A branch's card whose worktree was
  removed unmerged is gone even where main holds the card: main's copy, without
  the owner's line, marks it `the worktree of BRANCH is gone, and main's copy of
  card SLUG does not carry the owner's line`. The mark comes off when the card or
  comment is found again; the ask closes when its answer can be read, or by
  hand with `just tasks asks close`. A gone ask is open, so it counts with the
  open ones and never in the answered figures. A read that says nothing changes
  nothing: a `git worktree list` that failed leaves every ask on a branch as it
  was; a listed worktree git could not read (`Scan::failed`) leaves every ask on
  a card of its branch as it was; and so does a document that could not be read.

The `verdicts/…` tasks the app filed before asks are history: no save files one
any more, and no server start touches them. `just symphony-cli verdict-asks`,
run once in a project's main checkout, turns each one still open into asks:
every card it lists that still waits on an agent becomes an ask keeping the
owner's time, and the task is cancelled, as the app's own `app` session, then
gets a line naming the asks and saying so. One an executor holds is not
cancelled: it stays with its holder, and its line says that and why. Run again,
it turns nothing more, as each turned task's line says so. On
a project's live board it is refused outside the main checkout before it reads,
as `just tasks migrate` is; a scratch board it turns from anywhere. Until it
runs, an open verdict task stays the task it was, and an agent can still finish
it. Comments left open before asks are asks only once the owner says more on
them, unless `owner_asks::COMMENTS_OPEN_BEFORE` is set: then the same command
opens one for each, aged from the commit that wrote it (the design's second open
question, left to the owner).

**In this repository the landing is step 1.** Main's first build that writes
the board moves the live board to schema 17, and from then until the shared
server restarts on main's build, the server's board writes are refused, the
owner's messages on tasks among them; that restart needs the owner's own word.

A consuming project whose pin moves to a build with asks takes three steps in
its main checkout, in this order:

1. Relink the project's builds at `.target/bin/` (`tasks` and `symphony`) to
   the new pin, then run `just tasks migrate`, which moves its board to schema 17
   (its checkout holds no board source, so no other command does). Before the
   migration the new build is refused the board, naming `just tasks migrate`, and
   the old one still runs. From the migration the old `tasks` build is refused
   the board, as it writes only a board at its own schema, 16: a link, a session
   or a tool still on it fails until it is relinked to the new pin.
2. Restart the shared server on the new host build, a separate act that needs its
   own word.
3. Run `just symphony-cli verdict-asks` once.

Between step 1 and the restart (in this repository, from the landing), the old
server reads the board and its writes are refused:

- **The owner's messages on tasks are refused**, as every other write of the
  server is; the owner sends them again after the restart.
- **The owner's saves open no ask.** They are written into the documents and
  wait in the server's journal, and the new server delivers them on its start
  to the chat recipient as chat only: the old build's journal records carry no
  ask, and the new server opens none for them. Their asks are lost, not the
  saves: a card or comment saved then waits on an agent in the app's agent queue
  and in `just symphony-status --open`, and a chat question in `just symphony-ask
  list --open`. Each gets an ask once the owner says more on it.

From the restart, saves open asks, and the open verdict tasks wait as they were
until the last step turns them.

## Links with the documents

A task's `--link` lists it in the **Related tasks** panel of that document's page in the
Symphony, beside the document (below it on a narrow screen): open work first, each row its
title, slug and status, and finished work folded under its count. The `verdicts/…` tasks answer
the owner, not the document, and are left out. In the other direction, write
`[[task:sim/jump-departure]]` in any document: it renders as a link to the
task, with its live status beside it.

## Limits

- **One machine.** The board's server listens on this machine alone
  (`127.0.0.1`) and keeps its data in `.git/`; a session on another machine or
  in the cloud cannot reach it.
- **The owner's review of a task is the reviewer's verdict**, not a web button.
  The owner steers through the thread.
- **Nothing is reassigned automatically.** A lost session's work becomes
  claimable, and the claim that takes it over says so in the thread.
- **A subagent is seen through its tool calls and what it left running.** One
  waiting on a background job without calling a tool reads `running_test` while
  a process it started works in the linked worktree its last tool call ran in;
  one with nothing of its own working there (or in the main checkout, which says
  nothing of whose a process is) goes stale after 90 s, and lost after 10 minutes if it holds
  claimable work. The board's own health (`just tasks show`, a claim) still ages
  by its beats alone.
