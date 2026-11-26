//! A board's rows as they are, past every rule: what a test sets up that no command would write,
//! and reads to see what a write left ([`Store::raw`]).
//!
//! Nothing here is a statement a caller wrote: a row is named by its table and its id, a table's
//! rows by the fields they must equal, and what is set by its fields, so the same test asks the
//! same of whichever database holds the board. A write goes behind no schema guard and decides on
//! nothing; one that meets another's is sent again, as the store's own are. The board's commands
//! never come here.

use super::database::{Database, Postgres};
use super::postgres::{Pg, raw as pg};
use super::transaction::on_postgres;
use super::{Error, Failure, PATIENCE, Store, retried};
use serde_json::{Map, Value, json};

/// The board a [`Raw`] reaches.
type On<'a> = Postgres<'a>;

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
        Raw {
            client: self.db.reach(),
        }
    }
}

impl<'a> Raw<'a> {
    /// The rows of the board in `db`.
    pub fn of(db: &'a Database) -> Raw<'a> {
        Raw {
            client: match db {
                Database::PostgreSql(client) => Postgres::Socket(client),
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
        self.ask(pg::get(&rows), Map::new())
    }

    /// As [`Rows::get`], each row only its `fields`.
    pub fn fields(&self, fields: &[&str]) -> Result<Value, Error> {
        fields.iter().copied().for_each(plain);
        let rows = self.rows();
        self.ask(pg::fields(&rows, fields), Map::new())
    }

    /// One field of the one row, null when the field or the row is not there.
    pub fn field(&self, field: &str) -> Result<Value, Error> {
        plain(field);
        let one = self.one();
        self.ask(pg::field(&one, field), Map::new())
    }

    /// Whether the one row is there.
    pub fn exists(&self) -> Result<bool, Error> {
        let one = self.one();
        Ok(self.ask(pg::exists(&one), Map::new())? == json!(true))
    }

    /// How many rows there are.
    pub fn count(&self) -> Result<u64, Error> {
        let rows = self.rows();
        let counted = self.ask(pg::count(&rows), Map::new())?;
        Ok(counted.as_u64().unwrap_or(0))
    }

    /// `fields` set on the rows there are, a null unsetting its field: a row that is not there
    /// is not made.
    pub fn set(&self, fields: Value) -> Result<(), Error> {
        self.writable()?;
        let fields = object(fields);
        let vars = values("s", &fields);
        let rows = self.rows();
        self.ask(pg::set(&rows, &fields), vars).map(drop)
    }

    /// `field` of the rows there are, moved by `by`.
    pub fn add(&self, field: &str, by: impl Into<Value>) -> Result<(), Error> {
        self.writable()?;
        plain(field);
        let vars = Map::from_iter([("by".to_string(), by.into())]);
        let rows = self.rows();
        self.ask(pg::add(&rows, field), vars).map(drop)
    }

    /// The one row made with `fields`, or changed where it is there.
    pub fn put(&self, fields: Value) -> Result<(), Error> {
        self.writable()?;
        let fields = object(fields);
        let vars = values("s", &fields);
        let one = self.one();
        self.ask(pg::put(&one, &fields), vars).map(drop)
    }

    /// The one row made with `fields`: an error where it is there already.
    pub fn create(&self, fields: Value) -> Result<(), Error> {
        self.writable()?;
        let fields = object(fields);
        let vars = values("s", &fields);
        let one = self.one();
        self.ask(pg::create(&one, &fields), vars).map(drop)
    }

    /// The rows removed.
    pub fn delete(&self) -> Result<(), Error> {
        self.writable()?;
        let rows = self.rows();
        self.ask(pg::delete(&rows), Map::new()).map(drop)
    }

    /// Refused for a workflow's records, which its activation and its operations alone write
    /// (`super::workflow`): a row set here past every rule would be a transition nobody admitted,
    /// and the task fence would take it for one. A task's position needs its record, so it is
    /// not written here either.
    fn writable(&self) -> Result<(), Error> {
        if self.table == "workflow" {
            return Err(Error::Refused(
                "workflow: a workflow's records are written by its activation and its operations \
                 alone, never past them"
                    .into(),
            ));
        }
        Ok(())
    }

    fn rows(&self) -> pg::Rows<'_> {
        match &self.which {
            Which::One(_) => pg::Rows::One(&self.table),
            Which::Where(equal) => pg::Rows::Where(&self.table, equal),
        }
    }

    /// The one row a statement about a single row names.
    fn one(&self) -> pg::Rows<'_> {
        assert!(
            matches!(self.which, Which::One(_)),
            "this is asked of one row of {}, by its id",
            self.table
        );
        self.rows()
    }

    /// `postgres`'s one answer, with this target's variables and `more`; sent again when it met
    /// another's write.
    fn ask(&self, postgres: Pg, more: Map<String, Value>) -> Result<Value, Error> {
        let mut vars = match &self.which {
            Which::One(id) => Map::from_iter([("id".to_string(), id.clone())]),
            Which::Where(equal) => values("w", equal),
        };
        vars.extend(more);
        let vars = Value::Object(vars);
        retried(PATIENCE, &mut || {
            let mut written = vars.clone();
            // The protocol binds jsonb, so carry raw assignments as text inside it before
            // PostgreSQL can normalize numeric tokens. Only the field setter reads this.
            if matches!(postgres, Pg::Script(..)) {
                written["board_written"] = Value::String(vars.to_string());
            }
            on_postgres(self.client, &postgres, &written, false).map_err(Failure::from)
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
        crate::db::plain(table),
        "{table:?} is not a plain name for a table"
    );
    table.to_string()
}

/// A field's name, or a path of them (`fields._board.red`), must be plain: it goes into a
/// statement as it is.
fn plain(field: &str) {
    assert!(
        !field.is_empty() && field.split('.').all(crate::db::plain),
        "{field:?} is not a plain name for a field"
    );
}
