//! The database a store's board is in: SurrealDB's, reached by its RPC ([`crate::rpc`]), or
//! PostgreSQL's, reached on its socket ([`crate::pg`]). The store's modules never ask which: a
//! statement names its text in each database's language ([`super::statement`]), and the store's one
//! sender ([`super::transaction`]) hands it to the database the board is in.

use crate::db::DbError;
use crate::pg;
use crate::rpc;
use serde_json::Value;
use std::time::Duration;

/// Where a board is.
#[derive(Debug)]
pub enum Database {
    SurrealDb(rpc::Client),
    PostgreSql(pg::Client),
}

impl From<rpc::Client> for Database {
    fn from(client: rpc::Client) -> Database {
        Database::SurrealDb(client)
    }
}

impl From<pg::Client> for Database {
    fn from(client: pg::Client) -> Database {
        Database::PostgreSql(client)
    }
}

impl Database {
    /// The server a command names to reach the board again: SurrealDB's address, or `postgresql:`
    /// and its socket's folder.
    pub fn addr(&self) -> String {
        match self {
            Database::SurrealDb(client) => client.addr().to_string(),
            Database::PostgreSql(client) => client.addr(),
        }
    }

    /// The board's database on its server.
    pub fn database(&self) -> &str {
        match self {
            Database::SurrealDb(client) => client.database(),
            Database::PostgreSql(client) => client.database(),
        }
    }

    /// How many requests its client has made: a read's round trips, for the tests that count them.
    pub fn requests(&self) -> usize {
        match self {
            Database::SurrealDb(client) => client.requests(),
            Database::PostgreSql(client) => client.requests(),
        }
    }

    /// Request and write telemetry shared by clients of this board.
    pub fn queries(&self) -> std::sync::Arc<crate::board_health::QueryLog> {
        match self {
            Database::SurrealDb(client) => client.queries(),
            Database::PostgreSql(client) => client.queries(),
        }
    }

    /// A client of its own of the same board, which has made no request yet.
    pub fn another(&self) -> Database {
        match self {
            Database::SurrealDb(client) => Database::SurrealDb(rpc::Client::new(
                client.addr(),
                client.namespace(),
                client.database(),
            )),
            Database::PostgreSql(client) => Database::PostgreSql(client.another()),
        }
    }

    /// Whether this is a PostgreSQL board.
    pub fn is_postgresql(&self) -> bool {
        matches!(self, Database::PostgreSql(_))
    }

    /// The same board, each answer waited for at most `most` and a read that met it sent no
    /// second time ([`rpc::Client::bounded`]; PostgreSQL's client sends nothing twice).
    pub(super) fn bounded(&self, most: Duration) -> Database {
        match self {
            Database::SurrealDb(client) => Database::SurrealDb(client.bounded(most)),
            Database::PostgreSql(client) => Database::PostgreSql(client.bounded(most)),
        }
    }

    /// Whether the server said `said` of a board no one has opened: SurrealDB's namespace or
    /// database not there ([`super::opening::undefined`]), or PostgreSQL's database.
    pub(super) fn undefined(&self, said: &str) -> bool {
        match self {
            Database::SurrealDb(client) => super::opening::undefined(said, client.database()),
            Database::PostgreSql(_) => pg::no_database(said),
        }
    }

    /// The board's database made on its server, if it is not there yet.
    #[track_caller]
    pub(super) fn define(&self, nothing: &str) -> Result<(), DbError> {
        match self {
            Database::SurrealDb(client) => {
                client.define_then(nothing, Value::Object(serde_json::Map::new()))?;
                Ok(())
            }
            Database::PostgreSql(client) => client.make_database(),
        }
    }
}

/// A PostgreSQL board's client: the socket's ([`pg::Client`]), or a hosted board's handle on its
/// pool ([`pg::hosted::Client`]). Both take the same requests and give the same answers.
#[derive(Clone, Copy, Debug)]
pub(super) enum Postgres<'a> {
    Socket(&'a pg::Client),
    #[cfg(feature = "hosted-postgresql")]
    Hosted(&'a pg::hosted::Client),
}

impl Postgres<'_> {
    /// Each of `steps`' answers, all of them in one transaction.
    #[track_caller]
    pub(super) fn request(self, steps: &[pg::Step]) -> Result<Vec<pg::Answer>, DbError> {
        match self {
            Postgres::Socket(client) => client.request(steps),
            #[cfg(feature = "hosted-postgresql")]
            Postgres::Hosted(client) => client.request(steps),
        }
    }

    /// `sql`'s statements as one simple query, which says its own transactions.
    #[track_caller]
    pub(super) fn simple(self, sql: &str) -> Result<Vec<pg::Answer>, DbError> {
        match self {
            Postgres::Socket(client) => client.simple(sql),
            #[cfg(feature = "hosted-postgresql")]
            Postgres::Hosted(client) => client.simple(sql),
        }
    }
}

/// The client a statement goes to, by the language it is sent in.
pub(super) enum Reach<'a> {
    SurrealDb(&'a rpc::Client),
    PostgreSql(Postgres<'a>),
}

/// Where a store's board is: one of the [`Database`]s a caller names, or a hosted board, which
/// no caller's [`Database`] names and the store alone dispatches to.
#[derive(Debug)]
pub(super) enum Board {
    Local(Database),
    #[cfg(feature = "hosted-postgresql")]
    Hosted(pg::hosted::Client),
}

impl From<Database> for Board {
    fn from(db: Database) -> Board {
        Board::Local(db)
    }
}

impl Board {
    pub(super) fn reach(&self) -> Reach<'_> {
        match self {
            Board::Local(Database::SurrealDb(client)) => Reach::SurrealDb(client),
            Board::Local(Database::PostgreSql(client)) => {
                Reach::PostgreSql(Postgres::Socket(client))
            }
            #[cfg(feature = "hosted-postgresql")]
            Board::Hosted(client) => Reach::PostgreSql(Postgres::Hosted(client)),
        }
    }

    /// How long a PostgreSQL board's client waits for a request's answer; none on SurrealDB.
    pub(super) fn postgresql_wait(&self) -> Option<std::time::Duration> {
        match self {
            Board::Local(Database::SurrealDb(_)) => None,
            Board::Local(Database::PostgreSql(client)) => Some(client.wait()),
            #[cfg(feature = "hosted-postgresql")]
            Board::Hosted(client) => Some(client.wait()),
        }
    }

    /// The [`Database`] of a board that is in one. A hosted board has none: it is reached by its
    /// pool, and no socket or address of it is ever given out.
    pub(super) fn local(&self) -> &Database {
        match self {
            Board::Local(db) => db,
            #[cfg(feature = "hosted-postgresql")]
            Board::Hosted(_) => {
                panic!("this board is hosted, and is reached by no socket or address of its own")
            }
        }
    }

    /// As [`Board::local`], the database itself.
    pub(super) fn into_local(self) -> Database {
        match self {
            Board::Local(db) => db,
            #[cfg(feature = "hosted-postgresql")]
            Board::Hosted(_) => {
                panic!("this board is hosted, and is reached by no socket or address of its own")
            }
        }
    }

    /// What a command or a log names the board's server by: a hosted board's is its label,
    /// which says nothing of where it is.
    pub(super) fn addr(&self) -> String {
        match self {
            Board::Local(db) => db.addr(),
            #[cfg(feature = "hosted-postgresql")]
            Board::Hosted(client) => client.label().to_string(),
        }
    }

    pub(super) fn database(&self) -> &str {
        match self {
            Board::Local(db) => db.database(),
            #[cfg(feature = "hosted-postgresql")]
            Board::Hosted(client) => client.label(),
        }
    }

    pub(super) fn is_postgresql(&self) -> bool {
        matches!(self.reach(), Reach::PostgreSql(_))
    }

    /// A refusal of this board's schema, `why`, as its reader takes it: the commands it names
    /// make and move a local board, and a hosted one is its deployment's migration's.
    pub(super) fn refusal(&self, why: String) -> String {
        match self {
            Board::Local(_) => why,
            #[cfg(feature = "hosted-postgresql")]
            Board::Hosted(_) => format!(
                "{why}; this board is hosted, where the commands that refusal names make and \
                 move nothing: its deployment's migration does"
            ),
        }
    }

    pub(super) fn bounded(&self, most: Duration) -> Board {
        match self {
            Board::Local(db) => Board::Local(db.bounded(most)),
            #[cfg(feature = "hosted-postgresql")]
            Board::Hosted(client) => Board::Hosted(client.bounded(most)),
        }
    }

    /// Whether the server said `said` of a board no one has opened. A hosted board's database
    /// is the platform's to make: one that is not there is an error, never a board to make.
    pub(super) fn undefined(&self, said: &str) -> bool {
        match self {
            Board::Local(db) => db.undefined(said),
            #[cfg(feature = "hosted-postgresql")]
            Board::Hosted(_) => false,
        }
    }

    #[track_caller]
    pub(super) fn define(&self, nothing: &str) -> Result<(), DbError> {
        match self {
            Board::Local(db) => db.define(nothing),
            #[cfg(feature = "hosted-postgresql")]
            Board::Hosted(_) => Err(DbError::Failed(
                "a hosted board's database is made by its platform, and nothing here makes one"
                    .into(),
            )),
        }
    }
}
