//! How a store opens its board, by the schema rule ([`crate::schema`]): one read of the board's
//! version, then the board taken as it is, migrated forward once, or refused before any write. A
//! writer keeps its line in the board's writers as it opens ([`super::writers`]).

use super::database::{Board, Database};
use super::postgres::{self, Pg};
use super::statement::both;
use super::surreal as sql;
use super::transaction::Run;
use super::writers::{self, Line, Program};
use super::{Clock, Error, Failure, Probe, Store, now, sessions};
use crate::db::DbError;
use crate::live::{self, Live};
use crate::rpc::Client;
use crate::schema::{self, Access, Opened, Schema};
use serde_json::{Value, json};
use std::sync::Mutex;

/// How a store opens its board: at which schema, for what, and the project the board is (none for
/// a board no project names, a scratch or a test one), which its refusals name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opening {
    pub schema: Schema,
    pub access: Access,
    pub project: Option<String>,
    /// The project's live board this is, when the caller reached it as one ([`crate::live`]); none
    /// asks the client's address ([`crate::live::find_here`]) where it matters.
    pub live: Option<Live>,
    /// Opened by `tasks migrate` in the live board's main checkout: what alone moves a live board
    /// whose default branch commits no board source ([`crate::live::may_move`]).
    pub migrating: bool,
    /// Whether a migrate moves the board though writers seen in the last week are behind the schema
    /// it moves it to ([`Store::writers_behind`]): `tasks migrate --leave-behind`.
    pub leave_behind: bool,
    /// How a refusal names a scratch board, in the words the program opening it takes: its
    /// environment's [`live::SCRATCH_VARIABLE`] unless it says otherwise ([`Opening::scratch`]).
    pub scratch: &'static str,
    /// The program opening it, which a writer's line and its refusals name: this process's unless
    /// it says otherwise ([`Opening::by`]).
    pub program: Program,
}

impl Opening {
    /// This build's schema, to read and write.
    pub fn write() -> Opening {
        Opening {
            schema: Schema::this_build(),
            access: Access::Write,
            project: None,
            live: None,
            migrating: false,
            leave_behind: false,
            scratch: live::SCRATCH_VARIABLE,
            program: Program::this(),
        }
    }

    /// The board opened by `program` instead of this process's.
    pub fn by(self, program: Program) -> Opening {
        Opening { program, ..self }
    }

    /// The board opened by a migrate that moves it though writers seen in the last week are behind.
    pub fn leave_behind(self) -> Opening {
        Opening {
            leave_behind: true,
            ..self
        }
    }

    /// The board opened by a program that names a scratch board to its reader as `scratch` does.
    pub fn scratch(self, scratch: &'static str) -> Opening {
        Opening { scratch, ..self }
    }

    /// This build's schema, to read: a board one version ahead is read, and written only where the
    /// record its migration wrote lets this build ([`crate::schema::COMPATIBLE`]), and one behind,
    /// or not made yet, is refused, never moved forward or made.
    pub fn read() -> Opening {
        Opening {
            access: Access::Read,
            ..Opening::write()
        }
    }

    /// The board as `project`'s.
    pub fn of(self, project: Option<&str>) -> Opening {
        Opening {
            project: project.map(str::to_string),
            ..self
        }
    }

    /// The board opened at `schema` instead.
    pub fn at(self, schema: Schema) -> Opening {
        Opening { schema, ..self }
    }

    /// The board as the project's live one, `live` (none: a scratch board, or not known).
    pub fn live(self, live: Option<Live>) -> Opening {
        Opening { live, ..self }
    }

    /// The board opened by `tasks migrate`, run in its main checkout.
    pub fn migrating(self) -> Opening {
        Opening {
            migrating: true,
            ..self
        }
    }
}

/// The board's schema as a store's writes meet it ([`Store::board_standing`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Standing {
    /// The board's version, none for a board not made yet.
    pub version: Option<i64>,
    /// Whether the store's writes pass the schema guard on it now.
    pub writes: bool,
}

/// What moves a board the rule says to migrate ([`Store::moved`]): the store, how it was opened,
/// the version and the additive steps found, whether its database is defined, and the opener's
/// line in its writers.
type Mover = fn(&Store, &Opening, Found, bool, &Line) -> Result<(), Failure>;

/// What an opening found of the board's schema: its version (none: not made yet) and how many
/// additive steps on that contract it has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Found {
    version: Option<i64>,
    additions: i64,
}

/// What a store opened with no mover says, after the board's name, of a board the rule would
/// migrate: no reader meets it, as the rule refuses a reader first.
const NO_MOVER: &str = " would be moved forward or made, and this opening only reads: nothing was \
                        read or written";

impl Store {
    /// The board in `client`'s database, to read and write, its tables made or brought to this
    /// build's version if they are older; read with this machine's clock, host name and process
    /// table.
    pub fn open(client: Client) -> Result<Store, Error> {
        Store::open_as(client, &Opening::write())
    }

    /// As `open`, by the schema rule for `how` ([`Store::opening`]).
    pub fn open_as(client: Client, how: &Opening) -> Result<Store, Error> {
        let host = crate::sys::hostname();
        Store::opening(
            client,
            Box::new(now),
            Box::new(crate::sys::pid_alive),
            &host,
            how,
        )
    }

    /// As `open`, with the clock, the process probe and the host name given (the tests' own).
    pub fn with(
        client: Client,
        clock: Clock,
        pid_alive: Probe,
        host: &str,
    ) -> Result<Store, Error> {
        Store::opening(client, clock, pid_alive, host, &Opening::write())
    }

    /// As `with`, by the schema rule for `how` ([`schema::decide`]): a board at the opening's
    /// version is taken as it is, after one read and no write; an older board, or none, is
    /// migrated forward once by a writer and refused to a reader, before anything is written; a
    /// newer one is refused before anything is written, but to a reader one version behind, which
    /// reads it. A project's live board is moved forward only by a build its main checkout says is
    /// its own ([`live::may_move`]), and never opened by a process Cargo runs from its repository
    /// ([`live::cargo_refusal`]), which is refused before the read.
    ///
    /// A live board that reads below the highest schema it was seen at went back
    /// ([`live::seen::went_back`]): it is refused, to a reader and a writer alike, in the rollback's
    /// words, after that one read and before anything is decided by it, written or read. Otherwise
    /// the version found, and the one a migration moved it to, go in that record. The live board is
    /// [`Opening::live`]; a store opened with none named is checked only when it would move the
    /// board, by the board its client's address names.
    ///
    /// A writer keeps its line in the board's writers ([`super::writers`]), read in that one read
    /// and written when it is due: as it opens a board at its version, once it migrated one, and
    /// when it is refused a board behind it, but never when the board is ahead of it. A migrate
    /// ([`Opening::migrating`]) that would leave behind a writer seen in the last week is refused
    /// before it moves anything, naming each, unless it is told to ([`Opening::leave_behind`]).
    pub fn opening(
        client: Client,
        clock: Clock,
        pid_alive: Probe,
        host: &str,
        how: &Opening,
    ) -> Result<Store, Error> {
        Store::opening_on(client.into(), clock, pid_alive, host, how)
    }

    /// As [`Store::opening`], the board in `db`, on SurrealDB or PostgreSQL.
    pub fn opening_on(
        db: Database,
        clock: Clock,
        pid_alive: Probe,
        host: &str,
        how: &Opening,
    ) -> Result<Store, Error> {
        Store::opening_by(db.into(), clock, pid_alive, host, how, Some(Store::moved))
    }

    /// As [`Store::open_as`], for a program that only reads boards: `how` is taken to read
    /// ([`Access::Read`]) whatever it says, so the board is never made or moved forward, and the
    /// program links none of the migration, the schemas' text among it (the Symphony's own
    /// binary, whose size that text would count against: docs/CHECKS.md, "Size budgets").
    pub fn read_as(client: Client, how: &Opening) -> Result<Store, Error> {
        let host = crate::sys::hostname();
        Store::reading(
            client,
            Box::new(now),
            Box::new(crate::sys::pid_alive),
            &host,
            how,
        )
    }

    /// As [`Store::read_as`], with the clock, the process probe and the host name given.
    pub fn reading(
        client: Client,
        clock: Clock,
        pid_alive: Probe,
        host: &str,
        how: &Opening,
    ) -> Result<Store, Error> {
        Store::reading_on(client.into(), clock, pid_alive, host, how)
    }

    /// As [`Store::reading`], the board in `db`, on SurrealDB or PostgreSQL.
    pub fn reading_on(
        db: Database,
        clock: Clock,
        pid_alive: Probe,
        host: &str,
        how: &Opening,
    ) -> Result<Store, Error> {
        let how = Opening {
            access: Access::Read,
            ..how.clone()
        };
        Store::opening_by(db.into(), clock, pid_alive, host, &how, None)
    }

    /// The hosted board `client` reaches ([`crate::pg::hosted`]), to read and write by `how`.
    /// Its database is the platform's: one that is not there is an error, and nothing here makes
    /// it. A board that is empty or behind `how`'s schema is moved only by an opening that says
    /// it is the deployment's migration ([`Opening::migrating`]); any other is refused before
    /// anything is written, and an empty database that holds anything of another's is never
    /// taken for a board to make.
    #[cfg(feature = "hosted-postgresql")]
    pub fn opening_hosted(
        client: crate::pg::hosted::Client,
        clock: Clock,
        pid_alive: Probe,
        host: &str,
        how: &Opening,
    ) -> Result<Store, Error> {
        let mover: Option<Mover> = match how.access {
            Access::Write => Some(Store::moved_hosted),
            Access::Read => None,
        };
        Store::opening_by(Board::Hosted(client), clock, pid_alive, host, how, mover)
    }

    /// [`Store::opening_on`], moving a board the rule says to migrate by `mover`. A reader's entry
    /// hands none, so nothing it links names the migration: the rule never asks a reader for one
    /// ([`schema::decide`]), and one asked for anyway is refused before anything is written.
    fn opening_by(
        db: Board,
        clock: Clock,
        pid_alive: Probe,
        host: &str,
        how: &Opening,
        mover: Option<Mover>,
    ) -> Result<Store, Error> {
        if let Some(why) = live::cargo_refusal_here(how.live.as_ref(), &db.addr(), db.database()) {
            return Err(Error::Refused(why));
        }
        let mut store = Store::unchecked_board(db, clock, pid_alive, host, how);
        let own = how.schema.version();
        store.retrying(|| {
            let (found, defined, line, additions) = store.board_opening(&how.program)?;
            if let Some(live) = &how.live {
                went_back(live, found)?;
            }
            let decided =
                schema::decide(found, own, how.access, how.project.as_deref(), &how.program);
            // A writer of the board's contract with additive steps it lacks adds them where it
            // may move the board ([`Store::moved`]), and uses it as it is where it may not.
            let decided = match decided {
                Opened::Current
                    if how.access == Access::Write && additions < how.schema.additions() =>
                {
                    Opened::Migrate
                }
                decided => decided,
            };
            let found = Found {
                version: found,
                additions,
            };
            if let (Some(live), Some(found)) = (&how.live, found.version) {
                live::seen::record(live, found, store.now());
            }
            match (decided, mover) {
                (Opened::Current, _) => {
                    store.record_writer(&how.program, &how.schema, how.access, &line);
                    Ok(())
                }
                (Opened::Ahead, _) => Ok(()),
                (Opened::Refused(why), _) => {
                    Err(Failure::Stop(Error::Refused(store.db.refusal(why))))
                }
                (Opened::Migrate, Some(mover)) => mover(&store, how, found, defined, &line),
                (Opened::Migrate, None) => Err(Failure::Stop(Error::Refused(format!(
                    "{}{NO_MOVER}",
                    schema::Named::of(how.project.as_deref()).board
                )))),
            }
        })?;
        // Opened to read, by the rule: the one opening whose writes take a board one schema ahead
        // that lets an older Symphony write it ([`schema::COMPATIBLE`]). The rule refuses a reader
        // every board but one at its schema or one ahead, so no reader got here by a migration.
        store.reader = how.access == Access::Read;
        Ok(store)
    }

    /// Move this board, found at `found` and opened by `how`, forward to its schema: what the rule's
    /// [`Opened::Migrate`] takes of a writer, `line` its line in the board's writers as read. A board
    /// of its own contract lacking additive steps it has takes them where this store may move it,
    /// holding no writer, as every build of the contract writes it after; where it may not, the
    /// board is used as it is, never refused.
    fn moved(
        &self,
        how: &Opening,
        found: Found,
        defined: bool,
        line: &Line,
    ) -> Result<(), Failure> {
        let record = |line: &Line| {
            self.record_writer(&how.program, &how.schema, how.access, line);
        };
        let adding = found.version == Some(how.schema.version());
        let board = how
            .live
            .clone()
            .or_else(|| live::find_here(&self.db.addr(), self.db.database()));
        if let Some(board) = &board {
            if how.live.is_none() {
                went_back(board, found.version)?;
            }
            if let Err(refused) = self.may_move(how, board, found.version) {
                record(line);
                return if adding { Ok(()) } else { Err(refused) };
            }
        }
        if how.migrating && !how.leave_behind && !adding {
            self.none_left_behind(how, found.version)?;
        }
        self.migrate(&how.schema, found, defined, None)?;
        if let Some(board) = &board {
            live::seen::record(board, how.schema.version(), self.now());
        }
        record(&Line::Held(None));
        Ok(())
    }

    /// A hosted board made or moved forward, by the deployment's migration alone: an opening
    /// that does not say it migrates is refused, its line kept so the migration can see who is
    /// behind; an empty database is made a board only when nothing of another's is in it; the
    /// writers still behind are held to as on any board.
    #[cfg(feature = "hosted-postgresql")]
    fn moved_hosted(
        &self,
        how: &Opening,
        found: Found,
        _defined: bool,
        line: &Line,
    ) -> Result<(), Failure> {
        let board = schema::Named::of(how.project.as_deref()).board;
        // Its additive steps too are the deployment's migration's to add: used as it is meanwhile.
        if !how.migrating && found.version == Some(how.schema.version()) {
            self.record_writer(&how.program, &how.schema, how.access, line);
            return Ok(());
        }
        if !how.migrating {
            self.record_writer(&how.program, &how.schema, how.access, line);
            return Err(Failure::Stop(Error::Refused(format!(
                "{board} is hosted and would be moved forward or made, which its deployment's \
                 migration alone does: its tables and rows are as they were, and this \
                 program's line among its writers says it is behind"
            ))));
        }
        if let Board::Hosted(client) = &self.db
            && client.class() != crate::pg::external::MAINTENANCE
        {
            return Err(Failure::Stop(Error::Refused(format!(
                "{board} is hosted, and its migration runs as the class {} of its budget, \
                 not {}: nothing was written",
                crate::pg::external::MAINTENANCE,
                client.class()
            ))));
        }
        if !how.leave_behind && found.version != Some(how.schema.version()) {
            self.none_left_behind(how, found.version)?;
        }
        // Made only where nothing of another's is, which the migration judges itself, under
        // its lock: a board another migration made a moment ago is a board, read again.
        let empty = format!(
            "{board} is hosted in a database that holds tables or functions and no board's \
             record of its schema: it is not taken for an empty one, and nothing was written"
        );
        self.migrate(&how.schema, found, true, Some(&empty))?;
        self.record_writer(&how.program, &how.schema, how.access, &Line::Held(None));
        Ok(())
    }

    /// Refused, naming each, when a writer seen in the last week is behind the schema `how` moves
    /// this board, found at `found`, to; a board not made yet has none.
    fn none_left_behind(&self, how: &Opening, found: Option<i64>) -> Result<(), Failure> {
        if found.is_none() {
            return Ok(());
        }
        let target = how.schema.version();
        let behind = self.writers_behind(target, &how.program)?;
        if behind.is_empty() {
            return Ok(());
        }
        let board = schema::Named::of(how.project.as_deref()).board;
        Err(Failure::Stop(Error::Refused(writers::left_behind(
            &board,
            target,
            &behind,
            writers::seen_now(),
        ))))
    }

    /// Whether this store, opened by `how`, may move `board`, a project's live board found at
    /// `found` (none: not made yet), forward ([`live::may_move`]). A scratch board it always may.
    fn may_move(&self, how: &Opening, board: &Live, found: Option<i64>) -> Result<(), Failure> {
        let may_move = if self.db.is_postgresql() {
            live::may_move_postgresql
        } else {
            live::may_move
        };
        may_move(
            board,
            how.schema == Schema::this_build(),
            how.migrating,
            found,
            how.schema.version(),
            how.scratch,
        )
        .map_err(|why| Failure::Stop(Error::Refused(why)))
    }

    /// The board's schema version, none for a board not made yet: one read, which defines and
    /// writes nothing.
    pub fn board_version(&self) -> Result<Option<i64>, Error> {
        self.retrying(|| self.board_schema().map(|(found, _)| found))
    }

    /// The board's schema as this store's writes meet it: its version (none for a board not made
    /// yet), and whether a write of this store passes the guard there ([`schema::writes`]): at its
    /// own schema, or, for a store opened to read, one ahead whose record lets a Symphony of its
    /// schema write it ([`schema::COMPATIBLE`]). One read, which defines and writes nothing; the
    /// guard still reads the record again in each write's own transaction.
    pub fn board_standing(&self) -> Result<Standing, Error> {
        let read = || {
            both(
                sql::opening::BOARD_STANDING,
                postgres::opening::BOARD_STANDING,
            )
        };
        let record = self.retrying(|| match self.request(read(), json!({})) {
            Ok(mut found) => Ok(found.pop().unwrap_or(Value::Null)),
            Err(DbError::Failed(said)) if self.db.undefined(&said) => Ok(Value::Null),
            Err(other) => Err(other.into()),
        })?;
        let version = record.get("version").and_then(Value::as_i64);
        let from = record.get("writable_from").cloned().unwrap_or(Value::Null);
        Ok(Standing {
            version,
            writes: schema::writes(version, &from, self.schema, self.reader),
        })
    }

    /// As `with`, taking the board's tables as they are, without the request that checks them: the
    /// heartbeat's, one request on every tool call, on a board any other command has opened. Its
    /// writes still meet the guard of this build's schema.
    pub fn unchecked(client: Client, clock: Clock, pid_alive: Probe, host: &str) -> Store {
        Store::unchecked_as(client, clock, pid_alive, host, &Opening::write())
    }

    /// As `unchecked`, writing at `how`'s schema and naming its project in a refusal.
    pub fn unchecked_as(
        client: Client,
        clock: Clock,
        pid_alive: Probe,
        host: &str,
        how: &Opening,
    ) -> Store {
        Store::unchecked_on(client.into(), clock, pid_alive, host, how)
    }

    /// As `unchecked_as`, the board in `db`, on SurrealDB or PostgreSQL.
    pub fn unchecked_on(
        db: Database,
        clock: Clock,
        pid_alive: Probe,
        host: &str,
        how: &Opening,
    ) -> Store {
        Store::unchecked_board(db.into(), clock, pid_alive, host, how)
    }

    fn unchecked_board(
        db: Board,
        clock: Clock,
        pid_alive: Probe,
        host: &str,
        how: &Opening,
    ) -> Store {
        Store {
            lookup: None,
            db,
            clock,
            pid_alive,
            host: host.into(),
            schema: how.schema.version(),
            additions: how.schema.additions(),
            added: how.schema.added_tables().into(),
            refusal: schema::Refusal::of(
                how.project.as_deref(),
                how.schema.version(),
                &how.program,
            ),
            pick: Mutex::new(Box::new(sessions::Random::seeded())),
            callback_snapshot: Mutex::new(None),
            callback_idle: Mutex::new(None),
            clock_panes: Mutex::new(None),
            process_scan: super::kept::Scan::default(),
            hand_off: None,
            giving: false,
            settling: None,
            inbox_pool: Mutex::new(None),
            limits: crate::limits::Held::default(),
            named_server: false,
            authority: None,
            reader: false,
        }
    }

    /// As [`Store::board_schema`], with `program`'s line in the board's writers on this host and the
    /// board's count of additive steps, in the same one read ([`sql::writers::OPENING`]).
    fn board_opening(&self, program: &Program) -> Result<(Option<i64>, bool, Line, i64), Failure> {
        let vars = writers::opening_vars(program, &self.host);
        match self.request(
            both(sql::writers::OPENING, postgres::writers::OPENING),
            vars,
        ) {
            Ok(answer) => {
                let (found, line, additions) = writers::opened(&answer);
                Ok((found, true, line, additions))
            }
            Err(DbError::Failed(said)) if self.db.undefined(&said) => {
                Ok((None, false, Line::NoTable, 0))
            }
            Err(other) => Err(other.into()),
        }
    }

    /// The additive steps on its contract this store may use on its board now: the fewer of its own
    /// and the board's ([`crate::schema::ADDED`]). Code of a step asks it before it reads or writes
    /// what the step defines, which a board without the step refuses. One read, which defines and
    /// writes nothing.
    pub fn additions(&self) -> Result<i64, Error> {
        Ok(self.board_additions()?.min(self.additions))
    }

    /// How many additive steps on its contract the board has: one read.
    fn board_additions(&self) -> Result<i64, Error> {
        let read = || {
            both(
                sql::opening::BOARD_ADDITIONS,
                postgres::opening::BOARD_ADDITIONS,
            )
        };
        self.retrying(|| match self.request(read(), json!({})) {
            Ok(found) => Ok(found.last().and_then(Value::as_i64).unwrap_or(0)),
            Err(DbError::Failed(said)) if self.db.undefined(&said) => Ok(0),
            Err(other) => Err(other.into()),
        })
    }

    /// Refused, in the guard's words, unless the board, where it is of this store's contract, has
    /// exactly this store's additive steps: what a whole-board read takes, which a copy, an export
    /// or a move to another engine is made from ([`schema::Steps::exactly`]), as it holds only the
    /// tables and fields this build knows. Read before and after `read`, so a move of steps while
    /// it read is refused too.
    pub(super) fn with_own_steps<T>(
        &self,
        read: impl FnOnce() -> Result<T, Error>,
    ) -> Result<T, Error> {
        let held = || -> Result<(), Error> {
            let found = self.board_additions()?;
            if self.board_version()? == Some(self.schema) && found != self.additions {
                let steps =
                    schema::Steps::exactly(self.additions, &self.refusal.board, self.schema);
                return Err(Error::Refused(format!(
                    "{}{found}{}",
                    steps.before, steps.after
                )));
            }
            Ok(())
        };
        held()?;
        let read = read()?;
        held()?;
        Ok(read)
    }

    /// The board's schema version (none for a board not made yet), and whether its namespace and
    /// database are on the server: one read in them, which defines nothing.
    fn board_schema(&self) -> Result<(Option<i64>, bool), Failure> {
        let read = both(sql::opening::BOARD_SCHEMA, postgres::opening::BOARD_SCHEMA);
        match self.request(read, json!({})) {
            Ok(found) => Ok((found.last().and_then(Value::as_i64), true)),
            Err(DbError::Failed(said)) if self.db.undefined(&said) => Ok((None, false)),
            Err(other) => Err(other.into()),
        }
    }

    /// Bring the board, found at `found`, to `schema` once: its namespace and database defined
    /// first if they were not (`defined`), and the analyzer, committed as an index needs it; then
    /// the steps and the records in one transaction that first checks the board is still at
    /// `found`, its version and its additive steps, so a migration that fails or loses to another's
    /// leaves the board as it was, and one another open finished first is read again rather than
    /// run twice. The records name the version and the oldest schema whose Symphony may write the
    /// board ([`Schema::writable_from`]), together, and the schema's additive steps. A SurrealDB
    /// board is made STRICT in the same transaction ([`sql::opening::strict`]).
    fn migrate(
        &self,
        schema: &Schema,
        found: Found,
        defined: bool,
        empty: Option<&str>,
    ) -> Result<(), Failure> {
        if !defined {
            self.define()?;
        }
        self.request(both(schema::ANALYZERS, Pg::Nothing), json!({}))?;
        let steps = sql::opening::migration(&schema.steps(), self.db.database());
        let postgres_steps = postgres::opening::migration(
            &format!("{}\n{}", self.defined()?, schema.postgres_steps()),
            found.version,
            schema.version(),
            schema.writable_from(),
            postgres::opening::Additions {
                was: found.additions,
                now: schema.additions(),
            },
            empty,
        );
        self.run_transaction::<Value>(
            both(steps, Pg::Simple(postgres_steps.into())),
            json!({
                "version": schema.version(),
                "writable_from": schema.writable_from(),
                "was": found.version,
                "additions": schema.additions(),
                "was_additions": found.additions,
            }),
            Run::Unguarded,
        )?;
        Ok(())
    }
}

/// The refusal of `live`'s board, found at `found`, when it went back ([`live::seen::went_back`]).
impl Store {
    /// This build's PostgreSQL definitions as the board's server takes them: PostgreSQL 16,
    /// which a hosted build alone takes, has its own analyzer
    /// ([`postgres::schema::define_on_16`]).
    fn defined(&self) -> Result<String, Failure> {
        #[cfg(feature = "hosted-postgresql")]
        if self.db.is_postgresql() {
            let version = self.request(
                both(
                    sql::opening::POSTGRESQL_ALONE,
                    postgres::opening::SERVER_VERSION,
                ),
                json!({}),
            )?;
            if version
                .last()
                .and_then(Value::as_i64)
                .map(|number| number / 10_000)
                == Some(16)
            {
                return Ok(postgres::schema::define_on_16());
            }
        }
        Ok(postgres::schema::define())
    }
}

fn went_back(live: &Live, found: Option<i64>) -> Result<(), Failure> {
    match live::seen::went_back(live, found) {
        Some(rollback) => Err(Failure::Stop(Error::Refused(rollback.refusal()))),
        None => Ok(()),
    }
}

/// Whether the server said `said` of a request in a namespace, or in the database `database`, it
/// does not hold yet: a board no one has opened. These are the pinned server's words.
pub(super) fn undefined(said: &str, database: &str) -> bool {
    said == format!("The database '{database}' does not exist")
        || said
            .strip_prefix("The namespace '")
            .is_some_and(|name| name.ends_with("' does not exist"))
}

#[cfg(test)]
mod tests {
    use super::sql::opening::BOARD_SCHEMA;
    use super::undefined;

    #[test]
    fn only_a_namespace_or_this_database_not_on_the_server_reads_as_a_board_not_made_yet() {
        assert!(undefined("The namespace 'sf' does not exist", "tasks"));
        assert!(undefined("The database 'tasks' does not exist", "tasks"));
        for other in [
            "The database 'other' does not exist",
            "The table 'meta' does not exist",
            "The namespace 'sf' already exists",
            "The namespace 'sf",
            "the server at 127.0.0.1:1 is slow: The database 'tasks' does not exist",
        ] {
            assert!(!undefined(other, "tasks"), "{other}");
        }
    }

    /// The open's check of the version, with its writer's line, is a read, so a late answer to it
    /// goes once more.
    #[test]
    fn the_schema_version_check_is_a_read() {
        assert_eq!(crate::rpc::Kind::of(BOARD_SCHEMA), crate::rpc::Kind::Read);
        assert_eq!(
            crate::rpc::Kind::of(super::sql::writers::OPENING),
            crate::rpc::Kind::Read
        );
    }
}
