//! A board's rows as they are, past every rule: what a test sets up that no command would write,
//! and reads to see what a write left ([`Store::raw`]).
//!
//! Nothing here is a statement a caller wrote: a row is named by its table and its id, a table's
//! rows by the fields they must equal, and what is set by its fields, so the same test asks the
//! same of whichever database holds the board. A write goes behind no schema guard and decides on
//! nothing; one that meets another's is sent again, as the store's own are. The board's commands
//! never come here.

use super::database::Database;
use super::postgres::{Pg, raw as pg};
use super::surreal::raw as sql;
use super::transaction::on_postgres;
use super::{Error, Failure, PATIENCE, Store, retried};
use crate::rpc::Client;
use serde_json::{Map, Value, json};

/// The board a [`Raw`] reaches, on either database.
#[derive(Clone, Copy)]
enum On<'a> {
    SurrealDb(&'a Client),
    PostgreSql(&'a crate::pg::Client),
}

/// A board's rows, read and set as they are ([`Store::raw`], [`Raw::on`]).
pub struct Raw<'a> {
    client: On<'a>,
}

/// The rows of one table a [`Raw`] reads or changes: one by its id, or every one, or those whose
/// fields equal given values.
pub struct Rows<'a> {
    client: On<'a>,
    table: String,
    which: Which,
}

enum Which {
    One(Value),
    Where(Map<String, Value>),
}

impl Store {
    /// This board's rows as they are, for a test's fixture and its look at what was written.
    pub fn raw(&self) -> Raw<'_> {
        Raw::of(&self.db)
    }
}

impl<'a> Raw<'a> {
    /// The rows of the board `client` reaches.
    pub fn on(client: &'a Client) -> Raw<'a> {
        Raw {
            client: On::SurrealDb(client),
        }
    }

    /// The rows of the board in `db`, on either database.
    pub fn of(db: &'a Database) -> Raw<'a> {
        Raw {
            client: match db {
                Database::SurrealDb(client) => On::SurrealDb(client),
                Database::PostgreSql(client) => On::PostgreSql(client),
            },
        }
    }

    /// The row of `table` whose id is `id`: a text, a number, or the parts of a compound id.
    pub fn row(&self, table: &str, id: impl Into<Value>) -> Rows<'a> {
        Rows {
            client: self.client,
            table: named(table),
            which: Which::One(id.into()),
        }
    }

    /// Every row of `table`.
    pub fn table(&self, table: &str) -> Rows<'a> {
        Rows {
            client: self.client,
            table: named(table),
            which: Which::Where(Map::new()),
        }
    }
}

impl Rows<'_> {
    /// Of a table's rows, those whose fields equal `equal`'s, a null one being a field not set.
    /// One row by its id is that row whatever it holds.
    #[must_use]
    pub fn matching(mut self, equal: Value) -> Self {
        if let Which::Where(wanted) = &mut self.which {
            wanted.extend(object(equal));
        }
        self
    }

    /// The one row whole, its id left out, or null when it is not there; a table's rows as a
    /// list, each whole with its id, in the order of their ids.
    pub fn get(&self) -> Result<Value, Error> {
        let rows = self.rows();
        self.ask(&sql::get(&rows), pg::get(&rows), Map::new())
    }

    /// As [`Rows::get`], each row only its `fields`.
    pub fn fields(&self, fields: &[&str]) -> Result<Value, Error> {
        fields.iter().copied().for_each(plain);
        let rows = self.rows();
        self.ask(
            &sql::fields(&rows, fields),
            pg::fields(&rows, fields),
            Map::new(),
        )
    }

    /// One field of the one row, null when the field or the row is not there.
    pub fn field(&self, field: &str) -> Result<Value, Error> {
        plain(field);
        let one = self.one();
        self.ask(&sql::field(&one, field), pg::field(&one, field), Map::new())
    }

    /// Whether the one row is there.
    pub fn exists(&self) -> Result<bool, Error> {
        let one = self.one();
        Ok(self.ask(&sql::exists(&one), pg::exists(&one), Map::new())? == json!(true))
    }

    /// How many rows there are.
    pub fn count(&self) -> Result<u64, Error> {
        let rows = self.rows();
        let counted = self.ask(&sql::count(&rows), pg::count(&rows), Map::new())?;
        Ok(counted.as_u64().unwrap_or(0))
    }

    /// `fields` set on the rows there are, a null unsetting its field: a row that is not there
    /// is not made.
    pub fn set(&self, fields: Value) -> Result<(), Error> {
        let fields = object(fields);
        let vars = values("s", &fields);
        let rows = self.rows();
        self.ask(&sql::set(&rows, &fields), pg::set(&rows, &fields), vars)
            .map(drop)
    }

    /// `field` of the rows there are, moved by `by`.
    pub fn add(&self, field: &str, by: impl Into<Value>) -> Result<(), Error> {
        plain(field);
        let vars = Map::from_iter([("by".to_string(), by.into())]);
        let rows = self.rows();
        self.ask(&sql::add(&rows, field), pg::add(&rows, field), vars)
            .map(drop)
    }

    /// The one row made with `fields`, or changed where it is there.
    pub fn put(&self, fields: Value) -> Result<(), Error> {
        let fields = object(fields);
        let vars = values("s", &fields);
        let one = self.one();
        self.ask(&sql::put(&one, &fields), pg::put(&one, &fields), vars)
            .map(drop)
    }

    /// The one row made with `fields`: an error where it is there already.
    pub fn create(&self, fields: Value) -> Result<(), Error> {
        let fields = object(fields);
        let vars = values("s", &fields);
        let one = self.one();
        self.ask(&sql::create(&one, &fields), pg::create(&one, &fields), vars)
            .map(drop)
    }

    /// The rows removed.
    pub fn delete(&self) -> Result<(), Error> {
        let rows = self.rows();
        self.ask(&sql::delete(&rows), pg::delete(&rows), Map::new())
            .map(drop)
    }

    fn rows(&self) -> sql::Rows<'_> {
        match &self.which {
            Which::One(_) => sql::Rows::One(&self.table),
            Which::Where(equal) => sql::Rows::Where(&self.table, equal),
        }
    }

    /// The one row a statement about a single row names.
    fn one(&self) -> sql::Rows<'_> {
        assert!(
            matches!(self.which, Which::One(_)),
            "this is asked of one row of {}, by its id",
            self.table
        );
        self.rows()
    }

    /// `statement`'s one answer, the SurrealQL `surreal` or its twin `postgres` as the board's
    /// database takes, with this target's variables and `more`; sent again when it met another's
    /// write.
    fn ask(&self, surreal: &str, postgres: Pg, more: Map<String, Value>) -> Result<Value, Error> {
        let mut vars = match &self.which {
            Which::One(id) => Map::from_iter([("id".to_string(), id.clone())]),
            Which::Where(equal) => values("w", equal),
        };
        vars.extend(more);
        let vars = Value::Object(vars);
        retried(PATIENCE, &mut || match self.client {
            On::SurrealDb(client) => {
                let mut answers = client.query(surreal, vars.clone()).map_err(Failure::from)?;
                Ok(answers.pop().unwrap_or(Value::Null))
            }
            On::PostgreSql(client) => {
                on_postgres(client, &postgres, &vars, false).map_err(Failure::from)
            }
        })
    }
}

/// Each of `fields`' values that is not null, as the variable its place gives it (`s0`, `w1`).
fn values(prefix: &str, fields: &Map<String, Value>) -> Map<String, Value> {
    (fields.iter().enumerate())
        .filter(|(_, (_, value))| !value.is_null())
        .map(|(at, (_, value))| (format!("{prefix}{at}"), value.clone()))
        .collect()
}

/// `fields` as the object it must be, each name plain.
fn object(fields: Value) -> Map<String, Value> {
    let Value::Object(fields) = fields else {
        panic!("fields are given as an object, not {fields}");
    };
    fields.keys().map(String::as_str).for_each(plain);
    fields
}

/// `table`, which must be a plain name: it goes into a statement as it is.
fn named(table: &str) -> String {
    assert!(
        crate::rpc::plain(table),
        "{table:?} is not a plain name for a table"
    );
    table.to_string()
}

/// A field's name, or a path of them (`fields._board.red`), must be plain: it goes into a
/// statement as it is.
fn plain(field: &str) {
    assert!(
        !field.is_empty() && field.split('.').all(crate::rpc::plain),
        "{field:?} is not a plain name for a field"
    );
}
