//! The database a store's board is in: PostgreSQL's, reached on its socket ([`crate::pg`]), or a
//! hosted board's pool. The store's one sender ([`super::transaction`]) hands each statement
//! ([`super::statement`]) to it.

use crate::db::DbError;
use crate::pg;
use crate::rpc;
use std::time::Duration;

/// Where a board is.
///
/// Non-exhaustive: an earlier release had a SurrealDB variant too, and a consumer's code reads the
/// client by a refutable pattern (`let Database::PostgreSql(client) = store.database() else …`),
/// which a single exhaustive variant would make irrefutable, refused by its lint
/// (`irrefutable_let_patterns` under `-D warnings`). Outside this crate the client is
/// [`Database::postgresql`]'s or [`Database::into_postgresql`]'s.
#[derive(Debug)]
#[non_exhaustive]
pub enum Database {
    PostgreSql(pg::Client),
}

impl From<pg::Client> for Database {
    fn from(client: pg::Client) -> Database {
        Database::PostgreSql(client)
    }
}

/// The refusal of a client that names a SurrealDB server: a board is reached on PostgreSQL alone,
/// and one still on SurrealDB is moved first by an earlier release ([`crate::server::GONE`]).
pub fn surrealdb_refused(addr: &str) -> String {
    format!(
        "{addr} names a SurrealDB server, and this build reaches a board on PostgreSQL alone \
         (`postgresql:SOCKET`): {}",
        crate::server::GONE
    )
}

impl Database {
    /// The board a board's address names ([`rpc::Client`], as `cli::board_client` and a
    /// consumer's code hand it on): `postgresql:` and its socket's folder, waiting for each answer
    /// as the client does. Any other address is a SurrealDB server's, refused
    /// ([`surrealdb_refused`]).
    pub fn named(client: &rpc::Client) -> Result<Database, String> {
        match client.addr().strip_prefix("postgresql:") {
            Some(socket) => Ok(
                pg::Client::new(std::path::Path::new(socket), client.database())
                    .waiting(client.wait_limit())
                    .into(),
            ),
            None => Err(surrealdb_refused(client.addr())),
        }
    }

    /// The server a command names to reach the board again: `postgresql:` and its socket's
    /// folder.
    pub fn addr(&self) -> String {
        match self {
            Database::PostgreSql(client) => client.addr(),
        }
    }

    /// The board's database on its server.
    pub fn database(&self) -> &str {
        match self {
            Database::PostgreSql(client) => client.database(),
        }
    }

    /// How many requests its client has made: a read's round trips, for the tests that count them.
    pub fn requests(&self) -> usize {
        match self {
            Database::PostgreSql(client) => client.requests(),
        }
    }

    /// Request and write telemetry shared by clients of this board.
    pub fn queries(&self) -> std::sync::Arc<crate::board_health::QueryLog> {
        match self {
            Database::PostgreSql(client) => client.queries(),
        }
    }

    /// A client of its own of the same board, which has made no request yet and waits for each
    /// answer as this one does.
    pub fn another(&self) -> Database {
        match self {
            Database::PostgreSql(client) => Database::PostgreSql(client.another()),
        }
    }

    /// Whether this is a PostgreSQL board: every board is.
    pub fn is_postgresql(&self) -> bool {
        matches!(self, Database::PostgreSql(_))
    }

    /// Its PostgreSQL client: every board this build reaches is on PostgreSQL.
    pub fn postgresql(&self) -> &pg::Client {
        match self {
            Database::PostgreSql(client) => client,
        }
    }

    /// Its PostgreSQL client, taken whole ([`Database::postgresql`]).
    pub fn into_postgresql(self) -> pg::Client {
        match self {
            Database::PostgreSql(client) => client,
        }
    }

    /// The same board, each answer waited for at most `most` (PostgreSQL's client sends nothing
    /// twice).
    pub(super) fn bounded(&self, most: Duration) -> Database {
        match self {
            Database::PostgreSql(client) => Database::PostgreSql(client.bounded(most)),
        }
    }

    /// Whether the server said `said` of a board no one has opened: its database not there.
    pub(super) fn undefined(&self, said: &str) -> bool {
        match self {
            Database::PostgreSql(_) => pg::no_database(said),
        }
    }

    /// The board's database made on its server, if it is not there yet.
    #[track_caller]
    pub(super) fn define(&self) -> Result<(), DbError> {
        match self {
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
    /// The client a statement goes to.
    pub(super) fn reach(&self) -> Postgres<'_> {
        match self {
            Board::Local(Database::PostgreSql(client)) => Postgres::Socket(client),
            #[cfg(feature = "hosted-postgresql")]
            Board::Hosted(client) => Postgres::Hosted(client),
        }
    }

    /// How long the board's client waits for a request's answer.
    pub(super) fn postgresql_wait(&self) -> std::time::Duration {
        match self {
            Board::Local(Database::PostgreSql(client)) => client.wait(),
            #[cfg(feature = "hosted-postgresql")]
            Board::Hosted(client) => client.wait(),
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

    /// The board's request and write telemetry, shared by every client of it.
    pub(super) fn queries(&self) -> std::sync::Arc<crate::board_health::QueryLog> {
        match self {
            Board::Local(db) => db.queries(),
            #[cfg(feature = "hosted-postgresql")]
            Board::Hosted(client) => client.queries(),
        }
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
    pub(super) fn define(&self) -> Result<(), DbError> {
        match self {
            Board::Local(db) => db.define(),
            #[cfg(feature = "hosted-postgresql")]
            Board::Hosted(_) => Err(DbError::Failed(
                "a hosted board's database is made by its platform, and nothing here makes one"
                    .into(),
            )),
        }
    }
}

#[cfg(test)]
#[path = "database_tests.rs"]
mod tests;
