//! A statement as the store sends it: its text in SurrealQL ([`super::surreal`]) and its twin in
//! PostgreSQL ([`super::postgres`]), with what it does said beside them. A store call names both
//! ([`both`]). One that names its SurrealQL alone has no twin yet, and on a PostgreSQL board it is
//! refused before anything is sent, naming the line it was sent from ([`Statement::postgres_at`]);
//! one written for PostgreSQL alone ([`Statement::postgres_alone`]) is refused on SurrealDB the
//! same way ([`Statement::surreal_at`]).
//!
//! What a statement does is said by the store, the same for both databases, and read from neither
//! text. The call that sends it says whether it writes: [`super::Store::transact`] sends a write,
//! behind the schema's guard, and a call that reads says so ([`super::Store::read`],
//! [`super::Store::read_together`]), so a write that forgets to is guarded all the same. The
//! statement says whether it puts a line on a thread, which its callbacks then hear
//! ([`Statement::posting`]), and which tables of an additive step it names, which the guard
//! refuses on a board without that step ([`Statement::naming`]).
//!
//! A debug build, every test's, holds each statement it sends that has SurrealQL to what that text
//! does ([`Statement::disagreement`]), so every store test walks every statement it sends: one
//! whose declaration disagrees is refused before anything is sent, naming its line.

use super::Error;
use super::postgres::Pg;
use std::borrow::Cow;
use std::panic::Location;

/// A statement's SurrealQL and its PostgreSQL twin, each where it has one, and what it does.
pub(super) struct Statement<'a> {
    /// Its SurrealQL, none for a statement written for PostgreSQL alone.
    pub surreal: Option<Cow<'a, str>>,
    pub postgres: Option<Pg<'a>>,
    /// Whether it puts a line on a thread ([`super::surreal::logged`], [`super::postgres::logged`]),
    /// which a write's callbacks then hear.
    pub posts: bool,
    /// The tables of the build's additive steps it names ([`crate::schema::ADDED`]): a write naming
    /// one is refused on a board without its step.
    pub names: &'static [&'static str],
}

/// The statement `surreal` in SurrealQL and `postgres` in PostgreSQL, which posts no line and
/// names no additive step's table until it says so.
pub(super) fn both<'a>(surreal: impl Into<Cow<'a, str>>, postgres: Pg<'a>) -> Statement<'a> {
    Statement {
        surreal: Some(surreal.into()),
        postgres: Some(postgres),
        posts: false,
        names: &[],
    }
}

impl<'a> From<&'a str> for Statement<'a> {
    fn from(surreal: &'a str) -> Statement<'a> {
        Statement::from(Cow::Borrowed(surreal))
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
        Statement::from(Cow::Owned(surreal))
    }
}

impl<'a> From<Cow<'a, str>> for Statement<'a> {
    fn from(surreal: Cow<'a, str>) -> Self {
        Statement {
            surreal: Some(surreal),
            postgres: None,
            posts: false,
            names: &[],
        }
    }
}

impl<'a> Statement<'a> {
    /// The statement `postgres`, written for PostgreSQL alone.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "every statement the store sends has its SurrealQL until SurrealDB's are removed"
        )
    )]
    pub fn postgres_alone(postgres: Pg<'a>) -> Statement<'a> {
        Statement {
            surreal: None,
            postgres: Some(postgres),
            posts: false,
            names: &[],
        }
    }

    /// It puts a line on a thread.
    pub fn posting(self) -> Self {
        self.posting_when(true)
    }

    /// It puts a line on a thread when `posts`: a statement whose parts are chosen by the call.
    pub fn posting_when(mut self, posts: bool) -> Self {
        self.posts = posts;
        self
    }

    /// It names `tables`, each defined by an additive step ([`crate::schema::ADDED`]).
    pub fn naming(mut self, tables: &'static [&'static str]) -> Self {
        self.names = tables;
        self
    }

    /// The text of its PostgreSQL twin, each part as it is sent, none where it has no twin. A read
    /// of lists gives none.
    pub fn postgres_text(&self) -> Vec<&str> {
        match &self.postgres {
            Some(Pg::Read(text) | Pg::Simple(text) | Pg::Within(_, text)) => vec![text.as_ref()],
            Some(Pg::Script(block, answer)) => std::iter::once(block.as_ref())
                .chain(answer.as_deref())
                .collect(),
            Some(Pg::Listed(_) | Pg::Nothing) | None => Vec::new(),
        }
    }

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

    /// Its SurrealQL, or the refusal of a statement written for PostgreSQL alone, sent from `at`.
    pub fn surreal_at(&self, at: &Location<'_>) -> Result<&str, Error> {
        self.surreal.as_deref().ok_or_else(|| {
            Error::Db(format!(
                "this board is on SurrealDB, and the statement sent from {}:{} was written for \
                 PostgreSQL alone: nothing was sent",
                at.file(),
                at.line()
            ))
        })
    }

    /// How what it says it does disagrees with what its SurrealQL does, sent as a write when
    /// `writes` by a store whose additive steps define `added` (each table with its step): none
    /// where they agree, or where it has no SurrealQL. Its SurrealQL writes where it names a
    /// statement that changes the database ([`crate::schema::mutates`]), posts where it makes a
    /// line ([`super::surreal::posts`]), and names a table of `added` where its text or its twin's
    /// has the table's name as a word ([`crate::schema::needs`]): the store's readings of each
    /// before a statement said them. A table it says it names that is none of `added` is no step
    /// this store would refuse it for, and is not compared.
    pub fn disagreement(&self, writes: bool, added: &[(String, i64)]) -> Option<String> {
        let surreal = self.surreal.as_deref()?;
        let mut said = Vec::new();
        match (writes, crate::schema::mutates(surreal)) {
            (true, false) => {
                said.push("is sent as a write, and its SurrealQL changes nothing".into())
            }
            (false, true) => said.push("is sent as a read, and its SurrealQL writes".into()),
            _ => {}
        }
        match (self.posts, super::surreal::posts(surreal)) {
            (true, false) => said.push("says it posts a line, and its SurrealQL posts none".into()),
            (false, true) => said.push("says it posts no line, and its SurrealQL posts one".into()),
            _ => {}
        }
        let texts: Vec<&str> = std::iter::once(surreal)
            .chain(self.postgres_text())
            .collect();
        let (mut declared, mut named) = (Vec::new(), Vec::new());
        for (table, _) in added {
            if self.names.contains(&table.as_str()) {
                declared.push(table.as_str());
            }
            let one = [(table.clone(), 1)];
            if texts
                .iter()
                .any(|text| crate::schema::needs(text, &one).is_some())
            {
                named.push(table.as_str());
            }
        }
        if declared != named {
            said.push(format!(
                "names the added tables {declared:?}, and its text names {named:?}"
            ));
        }
        (!said.is_empty()).then(|| said.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store's additive steps: `fixture_tag` defined by its first, `fixture_mark` by its second.
    fn added() -> Vec<(String, i64)> {
        vec![("fixture_tag".into(), 1), ("fixture_mark".into(), 2)]
    }

    #[test]
    fn a_statements_postgresql_text_is_each_part_it_sends_and_a_read_of_lists_has_none() {
        let texts = |postgres: Option<Pg<'static>>| -> Vec<String> {
            let statement = Statement {
                postgres,
                ..Statement::from("RETURN NONE;")
            };
            statement
                .postgres_text()
                .into_iter()
                .map(str::to_owned)
                .collect()
        };
        assert_eq!(texts(Some(Pg::read("v->'slug'"))), ["v->'slug'"]);
        assert_eq!(texts(Some(Pg::Simple("SELECT 1;".into()))), ["SELECT 1;"]);
        assert_eq!(
            texts(Some(Pg::Within(5, "BEGIN NULL; END".into()))),
            ["BEGIN NULL; END"]
        );
        assert_eq!(
            texts(Some(Pg::script("BEGIN NULL; END"))),
            ["BEGIN NULL; END"]
        );
        assert_eq!(
            texts(Some(Pg::script_then("BEGIN NULL; END", "v->'slug'"))),
            ["BEGIN NULL; END", "v->'slug'"]
        );
        let none: [&str; 0] = [];
        assert_eq!(texts(Some(Pg::Nothing)), none);
        assert_eq!(
            texts(Some(super::super::postgres::listed("t", "task", "slug"))),
            none
        );
        assert_eq!(texts(None), none);
    }

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

    #[test]
    fn a_statement_written_for_postgresql_alone_is_refused_on_surrealdb_at_its_call_site() {
        let alone = Statement::postgres_alone(Pg::read("NULL"));
        let at = Location::caller();
        let why = alone.surreal_at(at).unwrap_err().to_string();
        assert!(
            why.contains("was written for PostgreSQL alone: nothing was sent"),
            "{why}"
        );
        assert!(
            why.contains(&format!("{}:{}", at.file(), at.line())),
            "{why}"
        );
        assert_eq!(alone.postgres_at(at).unwrap(), &Pg::read("NULL"));
        assert_eq!(
            both("RETURN NONE;", Pg::read("NULL"))
                .surreal_at(at)
                .unwrap(),
            "RETURN NONE;"
        );
        for surreal in [
            Statement::from("RETURN 1;"),
            Statement::from(&"RETURN 1;".to_string()),
            Statement::from("RETURN 1;".to_string()),
            Statement::from(Cow::Borrowed("RETURN 1;")),
        ] {
            assert_eq!(surreal.surreal_at(at).unwrap(), "RETURN 1;");
            assert!(surreal.postgres.is_none() && !surreal.posts && surreal.names.is_empty());
        }
    }

    #[test]
    fn a_statement_posts_and_names_only_what_it_says() {
        let plain = both("RETURN NONE;", Pg::read("NULL"));
        assert!(!plain.posts && plain.names.is_empty());
        assert!(both("RETURN NONE;", Pg::read("NULL")).posting().posts);
        assert!(
            !both("RETURN NONE;", Pg::read("NULL"))
                .posting()
                .posting_when(false)
                .posts
        );
        assert!(
            both("RETURN NONE;", Pg::read("NULL"))
                .posting_when(true)
                .posts
        );
        let alone = Statement::postgres_alone(Pg::read("NULL"))
            .posting()
            .naming(&["setting"]);
        assert!(alone.posts && alone.surreal.is_none());
        assert_eq!(alone.names, ["setting"]);
    }

    #[test]
    fn a_statement_that_says_what_its_surrealql_does_agrees_with_it() {
        let added = added();
        let agrees = |statement: Statement, writes: bool| statement.disagreement(writes, &added);
        let line = super::super::surreal::logged("$slug", "$actor", "'msg'", "$text", "NONE");
        let read = || both("RETURN (SELECT VALUE slug FROM task);", Pg::read("NULL"));
        let write = || {
            both(
                "UPSERT counter:walk SET last = 1 RETURN NONE;",
                Pg::script(""),
            )
        };
        assert_eq!(agrees(read(), false), None);
        assert_eq!(agrees(write(), true), None);
        assert_eq!(
            agrees(
                both(format!("{line}\nRETURN NONE;"), Pg::script("")).posting(),
                true
            ),
            None
        );
        // A word in any case is the old reading's, as every value is a variable beside the text.
        assert_eq!(
            agrees(
                both("upsert counter:walk SET last = 1;", Pg::script("")),
                true
            ),
            None
        );
        // A table of a step, named in its SurrealQL, quoted, or in its twin's text alone.
        assert_eq!(
            agrees(
                both("CREATE `fixture_tag`:a SET v = 1;", Pg::script("")).naming(&["fixture_tag"]),
                true
            ),
            None
        );
        assert_eq!(
            agrees(
                both(
                    "UPSERT counter:walk SET last = 1;",
                    Pg::script("UPDATE FIXTURE_MARK SET v = 1;")
                )
                .naming(&["fixture_mark"]),
                true
            ),
            None
        );
        // Both of a store's steps, said in either order.
        let both_tables = || {
            both(
                "UPSERT fixture_tag:a SET v = 1; UPSERT fixture_mark:a SET v = 1;",
                Pg::script(""),
            )
        };
        assert_eq!(
            agrees(both_tables().naming(&["fixture_mark", "fixture_tag"]), true),
            None
        );
        assert_eq!(
            agrees(both_tables().naming(&["fixture_tag", "fixture_mark"]), true),
            None
        );
        // A table no step of this store defines is not compared: a store of an older build's
        // steps, which would not refuse the write for it.
        assert_eq!(
            write().naming(&["fixture_tag"]).disagreement(true, &[]),
            None
        );
        assert_eq!(agrees(write().naming(&["setting"]), true), None);
        // A statement written for PostgreSQL alone holds to no SurrealQL.
        assert_eq!(
            agrees(Statement::postgres_alone(Pg::read("NULL")), true),
            None
        );
    }

    #[test]
    fn each_way_a_statement_says_other_than_its_surrealql_does_is_said() {
        let added = added();
        let line = super::super::surreal::logged("$slug", "$actor", "'msg'", "$text", "NONE");
        let says =
            |statement: Statement, writes: bool| statement.disagreement(writes, &added).unwrap();
        assert_eq!(
            says(both("RETURN 1;", Pg::read("1")), true),
            "is sent as a write, and its SurrealQL changes nothing"
        );
        assert_eq!(
            says(both("DELETE counter:walk;", Pg::script("")), false),
            "is sent as a read, and its SurrealQL writes"
        );
        assert_eq!(
            says(both(line.clone(), Pg::script("")), true),
            "says it posts no line, and its SurrealQL posts one"
        );
        assert_eq!(
            says(
                both("UPSERT counter:walk SET last = 1;", Pg::script("")).posting(),
                true
            ),
            "says it posts a line, and its SurrealQL posts none"
        );
        assert_eq!(
            says(
                both("UPSERT fixture_tag:a SET v = 1;", Pg::script("")),
                true
            ),
            "names the added tables [], and its text names [\"fixture_tag\"]"
        );
        assert_eq!(
            says(
                both("UPSERT counter:walk SET last = 1;", Pg::script("")).naming(&["fixture_mark"]),
                true
            ),
            "names the added tables [\"fixture_mark\"], and its text names []"
        );
        // Its twin's text is read as its SurrealQL is.
        assert_eq!(
            says(
                both(
                    "UPSERT fixture_tag:a SET v = 1;",
                    Pg::script("UPDATE fixture_mark SET v = 1;")
                )
                .naming(&["fixture_tag"]),
                true
            ),
            "names the added tables [\"fixture_tag\"], and its text names [\"fixture_tag\", \
             \"fixture_mark\"]"
        );
        assert_eq!(
            says(both(line, Pg::script("")), false),
            "is sent as a read, and its SurrealQL writes; says it posts no line, and its \
             SurrealQL posts one"
        );
    }
}
