//! A statement as the store sends it: its text in SurrealQL ([`super::surreal`]) and, once it is
//! written, its twin in PostgreSQL ([`super::postgres`]). A store call names both
//! ([`both`]); one that names its SurrealQL alone has no twin yet, and on a PostgreSQL board it is
//! refused before anything is sent, naming the line it was sent from
//! ([`Statement::postgres_at`]).
//!
//! What a statement is, a write or a read, and whether it puts a line on a thread, is read from its
//! SurrealQL ([`crate::schema::mutates`], [`super::surreal::posts`]), the one text every statement
//! has: its twin says the same in the other language.

use super::Error;
use super::postgres::Pg;
use std::borrow::Cow;
use std::panic::Location;

/// A statement's SurrealQL, and its PostgreSQL twin when it has one.
pub(super) struct Statement<'a> {
    pub surreal: Cow<'a, str>,
    pub postgres: Option<Pg<'a>>,
}

/// The statement `surreal` in SurrealQL and `postgres` in PostgreSQL.
pub(super) fn both<'a>(surreal: impl Into<Cow<'a, str>>, postgres: Pg<'a>) -> Statement<'a> {
    Statement {
        surreal: surreal.into(),
        postgres: Some(postgres),
    }
}

impl<'a> From<&'a str> for Statement<'a> {
    fn from(surreal: &'a str) -> Statement<'a> {
        Statement {
            surreal: Cow::Borrowed(surreal),
            postgres: None,
        }
    }
}

impl<'a> From<&'a String> for Statement<'a> {
    fn from(surreal: &'a String) -> Statement<'a> {
        Statement::from(surreal.as_str())
    }
}

impl<'a> From<&'a std::sync::LazyLock<String>> for Statement<'a> {
    fn from(surreal: &'a std::sync::LazyLock<String>) -> Statement<'a> {
        Statement::from(surreal.as_str())
    }
}

impl From<String> for Statement<'_> {
    fn from(surreal: String) -> Self {
        Statement {
            surreal: Cow::Owned(surreal),
            postgres: None,
        }
    }
}

impl<'a> Statement<'a> {
    /// Its PostgreSQL twin, or the refusal of a statement that has none yet, sent from `at`.
    pub fn postgres_at(&self, at: &Location<'_>) -> Result<&Pg<'a>, Error> {
        self.postgres.as_ref().ok_or_else(|| {
            Error::Db(format!(
                "this board is on PostgreSQL, and the statement sent from {}:{} has no PostgreSQL \
                 twin yet: nothing was sent",
                at.file(),
                at.line()
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unpaired_statement_is_refused_at_the_call_site_before_it_can_be_sent() {
        let statement = Statement::from("RETURN NONE;");
        let at = Location::caller();
        let why = statement.postgres_at(at).unwrap_err().to_string();
        assert!(
            why.contains("has no PostgreSQL twin yet: nothing was sent"),
            "{why}"
        );
        assert!(
            why.contains(&format!("{}:{}", at.file(), at.line())),
            "{why}"
        );
        assert!(
            both("RETURN NONE;", Pg::read("NULL"))
                .postgres_at(at)
                .is_ok()
        );
    }
}
