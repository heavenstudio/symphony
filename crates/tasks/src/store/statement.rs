//! A statement as the store sends it: its PostgreSQL text ([`super::postgres`]), with what it does
//! said beside it. A store call names it ([`Statement::from`] a [`Pg`]).
//!
//! What a statement does is said by the store and read from no text. The call that sends it says
//! whether it writes: [`super::Store::transact`] sends a write, behind the schema's guard, and a
//! call that reads says so ([`super::Store::read`], [`super::Store::read_together`]), so a write
//! that forgets to is guarded all the same. The statement says whether it puts a line on a thread,
//! which its callbacks then hear ([`Statement::posting`]), and which tables of an additive step it
//! names, which the guard refuses on a board without that step ([`Statement::naming`]).
//!
//! A debug build, every test's, holds each statement it sends to the additive steps' tables its
//! text names ([`Statement::disagreement`]), so every store test walks every statement it sends:
//! one whose declaration disagrees is refused before anything is sent, naming its line.

use super::postgres::Pg;

/// A statement's PostgreSQL text and what it does.
pub(super) struct Statement<'a> {
    pub postgres: Pg<'a>,
    /// Whether it puts a line on a thread ([`super::postgres::logged`]), which a write's callbacks
    /// then hear.
    pub posts: bool,
    /// The tables of the build's additive steps it names ([`crate::schema::ADDED`]): a write naming
    /// one is refused on a board without its step.
    pub names: &'static [&'static str],
}

/// The statement `postgres`, which posts no line and names no additive step's table until it says
/// so.
impl<'a> From<Pg<'a>> for Statement<'a> {
    fn from(postgres: Pg<'a>) -> Statement<'a> {
        Statement {
            postgres,
            posts: false,
            names: &[],
        }
    }
}

impl<'a> Statement<'a> {
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

    /// Its text, each part as it is sent. A read of lists gives none.
    pub fn postgres_text(&self) -> Vec<&str> {
        match &self.postgres {
            Pg::Read(text) | Pg::Simple(text) | Pg::Within(_, text) => vec![text.as_ref()],
            Pg::Script(block, answer) => std::iter::once(block.as_ref())
                .chain(answer.as_deref())
                .collect(),
            Pg::Listed(_) => Vec::new(),
        }
    }

    /// How the additive steps' tables it says it names disagree with those its text names, by a
    /// store whose additive steps define `added` (each table with its step): none where they
    /// agree. It names a table of `added` where its text has the table's name as a word
    /// ([`crate::schema::needs`]). A table it says it names that is none of `added` is no step this
    /// store would refuse it for, and is not compared.
    pub fn disagreement(&self, added: &[(String, i64)]) -> Option<String> {
        let texts = self.postgres_text();
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
        (declared != named)
            .then(|| format!("names the added tables {declared:?}, and its text names {named:?}"))
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
    fn a_statements_text_is_each_part_it_sends_and_a_read_of_lists_has_none() {
        let texts = |postgres: Pg<'static>| -> Vec<String> {
            let statement = Statement::from(postgres);
            (statement.postgres_text().into_iter())
                .map(str::to_owned)
                .collect()
        };
        assert_eq!(texts(Pg::read("v->'slug'")), ["v->'slug'"]);
        assert_eq!(texts(Pg::Simple("SELECT 1;".into())), ["SELECT 1;"]);
        assert_eq!(
            texts(Pg::Within(5, "BEGIN NULL; END".into())),
            ["BEGIN NULL; END"]
        );
        assert_eq!(texts(Pg::script("BEGIN NULL; END")), ["BEGIN NULL; END"]);
        assert_eq!(
            texts(Pg::script_then("BEGIN NULL; END", "v->'slug'")),
            ["BEGIN NULL; END", "v->'slug'"]
        );
        let none: [&str; 0] = [];
        assert_eq!(
            texts(super::super::postgres::listed("t", "task", "slug")),
            none
        );
    }

    #[test]
    fn a_statement_posts_and_names_only_what_it_says() {
        let plain = || Statement::from(Pg::read("NULL"));
        assert!(!plain().posts && plain().names.is_empty());
        assert!(plain().posting().posts);
        assert!(!plain().posting().posting_when(false).posts);
        assert!(plain().posting_when(true).posts);
        let named = plain().posting().naming(&["setting"]);
        assert!(named.posts);
        assert_eq!(named.names, ["setting"]);
    }

    #[test]
    fn a_statement_that_says_the_added_tables_its_text_names_agrees_with_it() {
        let added = added();
        let agrees = |statement: Statement| statement.disagreement(&added);
        let write = || Statement::from(Pg::script("UPDATE counter SET last = 1;"));
        assert_eq!(agrees(write()), None);
        // A table of a step, in any case, quoted or not, in any part of its text.
        assert_eq!(
            agrees(
                Statement::from(Pg::script("INSERT INTO \"fixture_tag\" VALUES (1);"))
                    .naming(&["fixture_tag"])
            ),
            None
        );
        assert_eq!(
            agrees(
                Statement::from(Pg::script_then(
                    "UPDATE counter SET last = 1;",
                    "(SELECT 1 FROM FIXTURE_MARK)"
                ))
                .naming(&["fixture_mark"])
            ),
            None
        );
        // Both of a store's steps, said in either order.
        let both_tables = || {
            Statement::from(Pg::script(
                "UPDATE fixture_tag SET v = 1; UPDATE fixture_mark SET v = 1;",
            ))
        };
        assert_eq!(
            agrees(both_tables().naming(&["fixture_mark", "fixture_tag"])),
            None
        );
        assert_eq!(
            agrees(both_tables().naming(&["fixture_tag", "fixture_mark"])),
            None
        );
        // A table no step of this store defines is not compared: a store of an older build's
        // steps, which would not refuse the write for it.
        assert_eq!(write().naming(&["fixture_tag"]).disagreement(&[]), None);
        assert_eq!(agrees(write().naming(&["setting"])), None);
        // A read of lists has no text to read a table from.
        assert_eq!(
            agrees(Statement::from(super::super::postgres::listed(
                "t", "task", "slug"
            ))),
            None
        );
    }

    #[test]
    fn each_way_a_statement_says_other_tables_than_its_text_names_is_said() {
        let added = added();
        let says = |statement: Statement| statement.disagreement(&added).unwrap();
        assert_eq!(
            says(Statement::from(Pg::script("UPDATE fixture_tag SET v = 1;"))),
            "names the added tables [], and its text names [\"fixture_tag\"]"
        );
        assert_eq!(
            says(
                Statement::from(Pg::script("UPDATE counter SET v = 1;")).naming(&["fixture_mark"])
            ),
            "names the added tables [\"fixture_mark\"], and its text names []"
        );
        assert_eq!(
            says(
                Statement::from(Pg::script_then(
                    "UPDATE fixture_tag SET v = 1;",
                    "(SELECT 1 FROM fixture_mark)"
                ))
                .naming(&["fixture_tag"])
            ),
            "names the added tables [\"fixture_tag\"], and its text names [\"fixture_tag\", \
             \"fixture_mark\"]"
        );
    }
}
