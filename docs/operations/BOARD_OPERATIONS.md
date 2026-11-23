# Operating the board

What whoever runs a board needs, and what a program that opens one must keep: the store and the
schema contract between the builds that read and write it, which build moves a live board, who
writes it, its server, snapshots and holds, what the command may write, a server that will not
end, a board that went back or was copied, and the health the server records of it. Using the
board, its tasks, roles, landing and commands, is [TASKS](../workflow/TASKS.md); a program reads and writes
it through [its JSON APIs](../engineering/BOARD_API.md); the board's PostgreSQL, its engine identity and
a board still on SurrealDB are [POSTGRESQL](POSTGRESQL.md).

## The schema contract

Each build of the board writes one contract, its schema version, which the board records with the
additive steps on it the board has ([CHECKS](../workflow/CHECKS.md#the-board-is-a-contract)). The version
counts breaking steps alone: a step that only adds (a table of its own, an index that is not
unique, an optional field on a table every build reads and writes past what it does not know)
leaves it as it is. A build opens a board of its own contract with one read and no write, whatever
additive steps either has. One with fewer steps than the board reads and writes it, keeping on each
row the fields it does not know. One with more adds them where it may move the board (below), in
one transaction, as any move, holding no writer; where it may not, it uses the board as it is: a
write naming a table of a step the board lacks is refused before anything is written, and the
board refuses a field or a table it does not define. A whole-board copy or export runs only on a
board with the build's own steps. A newer contract migrates an older board
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
For the board's engine identity, holds, snapshots and recovery, see
[PostgreSQL board lifecycle](POSTGRESQL.md).

Where it commits the board's schema source (`crates/tasks/src/schema.rs`, as this
repository does), that is any command of a build of that very file, byte for
byte, wherever it was built: main's build moves the board at its first command
once a schema lands, and only such a build makes the board when it is not made
yet. The same holds of the board's definitions, `crates/tasks/src/store/postgres/schema.rs`, at
the same default-branch commit. Where it commits none (a consuming project, which pins a build), it is only
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
  refuses its one write.

Nothing else reads the record: `just tasks schema` reports what it says (exit 1 while the board
went back), `just tasks snapshot` opens no board, nor do `just tasks database`, its hold and its
release (so the hold stops a board that went back as any other), nor `tasks-reclaim`. Otherwise the store adds the version it found to the record when it is
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
after it ([CHECKS](../workflow/CHECKS.md#the-board-is-a-contract)).

The shared server is not counted. The Symphony opens a board to read
(`crates/symphony/src/taskview/connect.rs`), though it writes through that store (the owner's
saves and chat, asks, a task's priority, fields and cancels), and only an opening to write keeps a
line. Counting it would stop every move: the server takes a build of the new schema only by its
reload after the move ([RELEASES](../workflow/RELEASES.md#the-refresh)), so its line would always be behind
when the migrate reads it, and every refresh's migrate would refuse. Meanwhile a server one schema
behind its board reads on, and every write it sends is refused: the owner's saves and chat, with
the asks they open, wait in its journal, which it tries again each tick until a server that can
write hands them over, while its other writes (the owner's message on a task, a priority, a field,
a cancel, an ask it would close) are refused until it has reloaded. Every page of the project
says so in the server's notice, and that a verdict or a message saved meanwhile is kept and
reaches no agent until then. A server two schemas behind
reads nothing of its board.

The exception is a step declared to keep the Symphony writing
([CHECKS](../workflow/CHECKS.md#a-schema-step-that-keeps-the-symphony-writing)). Its migration records beside
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
there to judge ([RELEASES](../workflow/RELEASES.md#the-refresh)): that move names no one. From then on every
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
([a writer gone from its path](../workflow/RELEASES.md#the-refresh)); so is one whose program is no build the
project runs now (none its main checkout's `.target/bin` links name) and that has not run for
longer than the project's `[board] stale_writer_hours`, a day by default.
Any other writer behind holds that board alone: the refresh names each to rebuild and leaves that
board where it was, until each is rebuilt and run once and a later refresh, or the machine's
server's next round, moves it. No board holds another or `current`: each board is snapshotted
just before its own move, and a migrate that fails is that board's red alone
([RELEASES](../workflow/RELEASES.md#the-refresh)). A release that keeps the schema `current` names moves only
a board left behind it.

## The store and its server

The board's store is a PostgreSQL cluster of its own, `symphony/tasks/postgresql/` in the git
common dir, served by the machine's PostgreSQL on a Unix socket alone: its identity, its programs
and its socket are [POSTGRESQL](POSTGRESQL.md)'s. Whatever needs the board starts its server when
it is not running, and the server outlives its starter; a scratch board's ends with the process
that started it. Every start, stop, hold, release, snapshot, restore and reclaim takes the
starters' lock (`symphony/tasks/lock`), one at a time. A command that waits on another's says on
stderr, after a second and then every 30 s, what it waits on: the holder's pid, since when, and
what it does (`symphony/tasks/lock.json`, while it holds the lock). The Symphony's reads wait at
most 5 s, then fail naming the same.

The server is never killed. A stop is PostgreSQL's fast shutdown (its clients cut off, what they
had not committed rolled back, the rest written), waited for by `pg_ctl` for the board's patience
at most: one still stopping by then is left to finish, the stop says so, and it is never sent a
second signal ([Stuck servers](#stuck-servers)). A start waits for the server's word that it is
ready, or that it ended, for the same patience at most; one that says neither is left running,
never signalled, and the caller told so: it is replaying its log, or stuck. The server's log is
`symphony/tasks/postgresql.log`. Nobody else signals, starts or restarts the board's server by
hand.

Work on the data itself, a snapshot, a restore or a swap, holds the database stopped:
`just tasks database hold "WHY" --as HANDLE` writes `symphony/tasks/hold`, then stops the server
cleanly, and until its release every board command and the Symphony are refused, naming who holds
it, since when and why. A stop that fails leaves the hold standing, and its holder runs the same
command again to finish it. Only its holder releases it (`just tasks database release --as
HANDLE`), or another handle naming the holder (`--of HOLDER`), which it says: a release by mistake
in the middle of a swap is what the hold is there to stop. No role decides it, as the board, which
holds the roles, is not read while it is held. Each hold and release is a line of
`symphony/tasks/hold.log`. The hold reads no record of the schema the board was seen at, so it
stops a board that went back as any other.

**Snapshots.** `just tasks snapshot` copies the complete stopped cluster, its WAL included, into
`symphony/tasks/postgresql-snapshots/YYYY-MM-DDTHH-MM-SSZ` (the time it was taken, in UTC; `.N`
after another taken in the same second); `--list` names those kept, the newest first
([POSTGRESQL](POSTGRESQL.md#holds-snapshots-and-recovery)). It is refused while a server runs on
the cluster or its status cannot be read, so a running board is snapshotted under the hold. The
copy is copy-on-write, `clonefile` on macOS and `cp --reflink=always` on Linux, and a plain copy
where the volume cannot clone. It is made under `postgresql-snapshots/.taking` and renamed once
whole, so a snapshot listed is whole; one cut short is replaced by the next. Each is kept until
the owner removes it (`rm -rf symphony/tasks/postgresql-snapshots/NAME`; nothing runs on a
snapshot, so that is safe at any time). A board `TASKS_SERVER` names has no store here to
snapshot, and the command opens no board.

**Reclaiming space.** The `tasks-reclaim` binary, the owner's to run from the main checkout, runs
`VACUUM (FULL, ANALYZE)` on the board's database while its server runs, and is refused while the
board is held ([POSTGRESQL](POSTGRESQL.md#free-space-and-versions)).

**Putting a snapshot back.** The `tasks-move` binary, the owner's to run the same way, puts a
snapshot back over the board it was taken from (`tasks-move --restore SNAPSHOT --as HANDLE`):
held, copied, then switched under a journal (`symphony/tasks/move.json`) with a commit point, the
cluster it replaced kept in `symphony/tasks/moved/`. A failure before the commit point leaves the
board on the cluster it had, nothing is deleted, and a switch cut short is settled by its holder's
next `tasks-move`. `tasks-move postgresql --as HANDLE` (or `surrealdb`) only settles a journal an
earlier release's move left, and moves no board between the engines
([POSTGRESQL](POSTGRESQL.md#holds-snapshots-and-recovery)).

## What the command may write

`symphony init` allows the `just tasks` recipe ([TASKS](../workflow/TASKS.md#where-it-lives)) unasked, so it
refuses the recorder
([MACHINE_WORK](../workflow/MACHINE_WORK.md#recording-builds-and-tests)) and, like this
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

The board asks its server once to stop and kills none, so a server can outlast its commands, and
that is the owner's. A server still running when its stop's wait runs out (the board's patience,
`pg_ctl`'s own bound) is left to finish: the stop says so, naming its pid, the machine's load and
its log (`symphony/tasks/postgresql.log`), and it is never signalled again. A hold whose stop
ended so stays standing, so nothing starts another server meanwhile, and its holder runs the hold
again to finish it once that server has ended. A server that does not say it is ready within the
patience of its start is left running, never signalled, and the start refused saying so: it is
replaying its log, or stuck.

## A board that went back

A board refused as one that went back holds an older store than the one it was seen at: what was
written since is not in it. Run nothing that writes, `just tasks migrate` above all, which would
write on top of the older state. `just tasks schema` says what the store holds and what the
record says, and exits 1 while the board went back; every page of the project says it on the
server's notice, the project picker under its name, and `just tasks status` first among its
warnings ([Board health](#board-health)).

When the older board is meant, a fresh store or a restore the owner chose, the owner accepts it:
`just tasks schema --accept VERSION`, `none` for a store with no board, is refused unless the
board holds exactly that version, and adds an `accepted` line to the record, which counts from
there. The board is then read, and moved forward, as any board behind its build is.

**Recovering what was lost** is the owner's, or done on the owner's word. A board's store goes
back when an older one takes its place: a snapshot put back over it, its cluster's files put back
by hand, or a copy of an older dump. A snapshot that holds the board at the schema it was seen at
is put back over it with `tasks-move --restore SNAPSHOT --as HANDLE`
([POSTGRESQL](POSTGRESQL.md#holds-snapshots-and-recovery)), then `just tasks recovery rebind`;
`just tasks schema` then says the board is at the schema it was seen at, and the record needs no
acceptance. A restore keeps the cluster it replaced in `symphony/tasks/moved/STAMP-postgresql/`,
so a restore that sent the board back leaves what it lost there, to be put back as any
database's files are ([below](#a-board-that-was-copied-or-restored)): under the hold, with
`symphony/tasks/postgresql` renamed aside first, never removed.

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
  ([the api's calls](../engineering/BOARD_API.md#what-a-program-outside-the-board-keeps)).
- **A copy.** `tasks-copy` and every `Store::copy` replace the generation in the transaction that
  replaces the rows, so a token read before a copy is refused after it, and one read before a
  copy that failed still holds. A build of a schema before 22 copies without replacing it, which
  is why the generation came with a schema step: such a build is refused the board.
  A copy that cannot finish leaves the board as it was and says why in one line. The copy is one
  request, which the server itself ends thirty seconds short of the wait of the
  program that sent it (270 s of the client's 300), saying the rows of each table it had copied
  and the table it was at: left to run past its caller's wait it would replace the board after
  the caller was told it failed, which only a copy whose commit and answer take those thirty
  seconds still can. How large a board it takes is measured in
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

**Putting a database's files back** by hand, a cluster a restore kept aside say
([a board that went back](#a-board-that-went-back)), is in this order: `just tasks recovery revoke` first, before
the restored data is in place and served; the swap, under the hold; then, once the board is the
one to keep, `just tasks recovery rebind`. A copy made by a store that holds no binding (a
scratch program's, `tasks-copy`) leaves the binding naming the generation before it, which
refuses until a rebind. A restore, over the board or into a new one
([POSTGRESQL](POSTGRESQL.md#holds-snapshots-and-recovery)), revokes the bindings of the board it
restores itself, before that board is served.

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
A project with no board, its server answering that none is made yet
(`symphony_board::board_health::absent`), is not unwell: no
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
start, stop or hold makes no folder at or above the repository's common dir again. A board whose
store is there is never no board: when its server cannot be started, or answers with an
older schema or not at all, its passes fail and it is unwell and says so.
`tasks status` also warns when the record is more than a minute old, naming the server's pid and
whether it still runs: no dispatch pass has been recorded since.

**Open tasks nobody orchestrates.** A task with no orchestrator waits for one under its limit
that stays ([Tasks and roles](../workflow/TASKS.md#tasks-and-roles)), a well state and no defect: the record counts the
unfinished tasks with no holding orchestrator on them or above them in the board the server's clock
read last (`unowned` in the record, `Recorder::unowned`), and `tasks status` says `open tasks no
orchestrator holds: N, as of the newest board read`, and warns of none of them. The tick gives what
it finds first ([Tasks and roles](../workflow/TASKS.md#tasks-and-roles)), so the count it records is of tasks that wait
for an orchestrator that stays and holds fewer open tasks than the limit ([The limit on the tasks
an orchestrator holds](../workflow/TASKS.md#the-limit-on-the-tasks-an-orchestrator-holds)). Counting reads the board
the tick already read and adds no request; a
record written before any board read, or by a build older than the count, has none and says none. The owner's word on each of those tasks reaches
every orchestrator ([Talking on a task](../workflow/TASKS.md#talking-on-a-task)) until the board gives it one. The
server's own notice and the project picker do not warn of it; the picker says only that work
waiting to start has no live orchestrator (below).

**On the project picker.** The machine's project picker (`/`) draws a dot before each project's
name, its board's worst signal, and, only while the dot is not green, one line under its counts in
the dot's colour saying why; `/api/projects` sends that line as the project's `unwell`. One
function judges both (`Hub::standing`, `apps/symphony-server/src/live/health.rs`), from the
server's memory alone: drawing it reads no board. Red is every page's notice above, in its words
and alone: a dispatch or sweep failing, a board that went back. Amber is a
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
([What a task waits on](../workflow/TASKS.md#what-a-task-waits-on)).

**Its hours.** The machine's one server also appends each hour of it, as the hour ends, as one
line (a `symphony_api::HealthHour`) to the day file of the hour's UTC day in `symphony/health/` in
the git common dir (`YYYY-MM-DD.jsonl`; `symphony/health.<database>/` for a scratch database,
`SYMPHONY_HEALTH` names another folder, and a board `TASKS_SERVER` names keeps none without it;
`crates/tasks/src/board_health/hours.rs`): the hour's dispatch passes and sweeps, ok and failed;
its requests, the slow and the unanswered ones, and the slowest with its caller; and its writes
with their p50 and p90. Its `start` is the hour's first second, as the agents' hour lines'
([AGENT_ACTIVITY](../workflow/AGENT_ACTIVITY.md#the-day-files)) and the hourly roll-up's hours are keyed, so
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
([AGENT_ACTIVITY](../workflow/AGENT_ACTIVITY.md#a-reloads-hand-off)), closes just before a reload's hand-off
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

**A disk running full** is warned of in the same two places: `tasks status` when the disk holding
the record has less free than `SYMPHONY_LOW_DISK_GB` gigabytes (20 when unset; `0` turns it off, and a
value that is no number is said and 20 taken), and every page of a project while the disk
holding its checkout has, read by the server each time it tells the pages of its health; the page
says under how many gigabytes, not how many, so it is told when the disk goes low and when it is
freed, not at every build's write. A full disk fails every build on the machine and can corrupt
the board's database; the warning names `just tasks sweep`, which
clears the build output of finished worktrees ([TASKS](../workflow/TASKS.md#sessions)). `--json` adds `disk`: the folder read, its
free bytes and the threshold (`crates/tasks/src/board_health/disk.rs`).
