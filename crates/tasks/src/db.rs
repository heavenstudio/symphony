//! What every database the board runs on shares: why a query gave no results ([`DbError`]) and
//! the names that need no quoting ([`plain`]). Each client answers in these, SurrealDB's
//! ([`crate::rpc`]) and PostgreSQL's ([`crate::pg`]) alike.

use std::fmt;

/// Why a query did not give its results.
#[derive(Debug, Clone, PartialEq)]
pub enum DbError {
    /// Another transaction committed a write to what this one wrote first, and the database says
    /// the transaction can be retried.
    Conflict,
    /// A `THROW` in the query, with its message.
    Thrown(String),
    /// Anything else, in the server's words (or why it could not be reached).
    Failed(String),
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbError::Conflict => f.write_str("the transaction met another's write"),
            DbError::Thrown(message) => f.write_str(message),
            DbError::Failed(message) => f.write_str(message),
        }
    }
}

/// Whether `name` may name a namespace, database, table or field: letters, digits and `_`,
/// nothing to quote.
pub fn plain(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_name_is_letters_digits_and_underscores_and_never_empty() {
        for name in ["tasks", "sf", "scratch_1", "A9", "_"] {
            assert!(plain(name), "{name:?}");
        }
        for name in ["", "a b", "a-b", "a.b", "a;b", "a'b", "tâche", "a\n"] {
            assert!(!plain(name), "{name:?}");
        }
    }

    #[test]
    fn an_error_says_its_own_words_and_a_conflict_names_no_database() {
        assert_eq!(
            DbError::Conflict.to_string(),
            "the transaction met another's write"
        );
        assert_eq!(
            DbError::Thrown("refused: no".into()).to_string(),
            "refused: no"
        );
        assert_eq!(
            DbError::Failed("cannot reach it".into()).to_string(),
            "cannot reach it"
        );
    }
}
