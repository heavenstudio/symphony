//! How a store opens its board, by the schema rule ([`crate::schema`]): one read of the board's
//! version, then the board taken as it is, migrated forward once, or refused before any write.

use super::transaction::Run;
use super::{Clock, Error, Failure, Probe, STALE, Store, now, sessions};
use crate::live::{self, Live};
use crate::rpc::{Client, DbError};
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
    /// How a refusal names a scratch board, in the words the program opening it takes: its
    /// environment's [`live::SCRATCH_VARIABLE`] unless it says otherwise ([`Opening::scratch`]).
    pub scratch: &'static str,
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
            scratch: live::SCRATCH_VARIABLE,
        }
    }

    /// The board opened by a program that names a scratch board to its reader as `scratch` does.
    pub fn scratch(self, scratch: &'static str) -> Opening {
        Opening { scratch, ..self }
    }

    /// This build's schema, to read: a board one version ahead is read, never written, and one
    /// behind, or not made yet, is refused, never moved forward or made.
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
    pub fn opening(
        client: Client,
        clock: Clock,
        pid_alive: Probe,
        host: &str,
        how: &Opening,
    ) -> Result<Store, Error> {
        if let Some(why) =
            live::cargo_refusal_here(how.live.as_ref(), client.addr(), client.database())
        {
            return Err(Error::Refused(why));
        }
        let store = Store::unchecked_as(client, clock, pid_alive, host, how);
        let own = how.schema.version();
        store.retrying(|| {
            let (found, defined) = store.board_schema()?;
            if let Some(live) = &how.live {
                went_back(live, found)?;
            }
            let decided = schema::decide(found, own, how.access, how.project.as_deref());
            if let (Some(live), Some(found)) = (&how.live, found) {
                live::seen::record(live, found, store.now());
            }
            match decided {
                Opened::Current | Opened::Ahead => Ok(()),
                Opened::Refused(why) => Err(Failure::Stop(Error::Refused(why))),
                Opened::Migrate => {
                    let board = how
                        .live
                        .clone()
                        .or_else(|| live::find_here(store.client.addr(), store.client.database()));
                    if let Some(board) = &board {
                        if how.live.is_none() {
                            went_back(board, found)?;
                        }
                        store.may_move(how, board, found)?;
                    }
                    store.migrate(&how.schema, found, defined)?;
                    if let Some(board) = &board {
                        live::seen::record(board, own, store.now());
                    }
                    Ok(())
                }
            }
        })?;
        Ok(store)
    }

    /// Whether this store, opened by `how`, may move `board`, a project's live board found at
    /// `found` (none: not made yet), forward ([`live::may_move`]). A scratch board it always may.
    fn may_move(&self, how: &Opening, board: &Live, found: Option<i64>) -> Result<(), Failure> {
        live::may_move(
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
        Store {
            lookup: client.bounded(super::unanswered::LOOKUP_WAIT),
            client,
            clock,
            pid_alive,
            host: host.into(),
            schema: how.schema.version(),
            refusal: schema::Refusal::of(how.project.as_deref(), how.schema.version()),
            pick: Mutex::new(Box::new(sessions::Random::seeded())),
            callback_snapshot: Mutex::new(None),
            callback_idle: Mutex::new(None),
            clock_panes: Mutex::new(None),
            process_scan: super::kept::Scan::default(),
            hand_off: None,
            giving: false,
            inbox_pool: Mutex::new(None),
            limits: crate::limits::Held::default(),
            named_server: false,
        }
    }

    /// The board's schema version (none for a board not made yet), and whether its namespace and
    /// database are on the server: one read in them, which defines nothing.
    fn board_schema(&self) -> Result<(Option<i64>, bool), Failure> {
        match self.client.query(BOARD_SCHEMA, json!({})) {
            Ok(found) => Ok((found.last().and_then(Value::as_i64), true)),
            Err(DbError::Failed(said)) if undefined(&said, self.client.database()) => {
                Ok((None, false))
            }
            Err(other) => Err(other.into()),
        }
    }

    /// Bring the board, found at `found`, to `schema` once: its namespace and database defined
    /// first if they were not (`defined`), and the analyzer, committed as an index needs it; then
    /// the version's steps and its record in one transaction that first checks the board is still
    /// at `found`, so a migration that fails or loses to another's leaves the board as it was, and
    /// one another open finished first is read again rather than run twice.
    fn migrate(&self, schema: &Schema, found: Option<i64>, defined: bool) -> Result<(), Failure> {
        if !defined {
            self.client.define_then("RETURN NONE;", json!({}))?;
        }
        self.client.query(schema::ANALYZERS, json!({}))?;
        let steps = format!(
            "LET $found = {BOARD_SCHEMA_VALUE};\nIF $found != $was {{ THROW '{STALE}' }};\n{}\n\
             UPSERT meta:schema SET version = $version RETURN NONE;\nRETURN NONE;",
            schema.steps()
        );
        self.run_transaction::<Value>(
            &steps,
            json!({"version": schema.version(), "was": found}),
            Run::Unguarded,
        )?;
        Ok(())
    }
}

/// The refusal of `live`'s board, found at `found`, when it went back ([`live::seen::went_back`]).
fn went_back(live: &Live, found: Option<i64>) -> Result<(), Failure> {
    match live::seen::went_back(live, found) {
        Some(rollback) => Err(Failure::Stop(Error::Refused(rollback.refusal()))),
        None => Ok(()),
    }
}

/// Whether the server said `said` of a request in a namespace, or in the database `database`, it
/// does not hold yet: a board no one has opened. These are the pinned server's words.
fn undefined(said: &str, database: &str) -> bool {
    said == format!("The database '{database}' does not exist")
        || said
            .strip_prefix("The namespace '")
            .is_some_and(|name| name.ends_with("' does not exist"))
}

/// The board's schema version, or NONE before its tables are made.
macro_rules! board_schema {
    () => {
        "IF (INFO FOR DB).tables.meta = NONE { NONE } ELSE { (SELECT VALUE version FROM ONLY meta:schema) }"
    };
}
const BOARD_SCHEMA_VALUE: &str = board_schema!();
/// [`BOARD_SCHEMA_VALUE`] as a read of its own.
const BOARD_SCHEMA: &str = concat!("RETURN ", board_schema!(), ";");

#[cfg(test)]
mod tests {
    use super::{BOARD_SCHEMA, undefined};

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

    /// The open's check of the version is a read, so a late answer to it goes once more.
    #[test]
    fn the_schema_version_check_is_a_read() {
        assert_eq!(crate::rpc::Kind::of(BOARD_SCHEMA), crate::rpc::Kind::Read);
    }
}
