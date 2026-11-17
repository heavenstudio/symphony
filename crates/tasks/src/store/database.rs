//! The database a store's board is in: SurrealDB's, reached by its RPC ([`crate::rpc`]), or
//! PostgreSQL's, reached on its socket ([`crate::pg`]). The store's modules never ask which: a
//! statement names its text in each database's language ([`super::statement`]), and the store's one
//! sender ([`super::transaction`]) hands it to the database the board is in.

use crate::pg;
use crate::rpc::{self, DbError};
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
