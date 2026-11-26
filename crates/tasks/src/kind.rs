//! A task's kind: what its work is, one word of a short list held here ([`KINDS`]), chosen when it
//! is filed (`tasks create --kind`, the api's `create`, [`crate::NewTask::kind`]) or given to an
//! open task after (`tasks kind SLUG KIND`, [`crate::Store::set_kind`]), as an orchestrator does
//! for a task the owner filed from a page. A task filed with a red mark and no kind is given
//! `red` by the board in the same write, so a red's filer names one whatever build it runs. Its
//! slug is never read as one: a slug's first part says a kind only by habit.
//!
//! The board keeps it among the task's own fields ([`crate::red::FIELDS`]`.`[`MARK`]), beside
//! the red mark and an experiment's arm ([`crate::arm`]), out of `set_field`'s reach: no table
//! and no field of the schema holds it, so it is no schema step. A task may have none, as every
//! task filed before kinds were kept has: nothing writes a task a kind it was not given. `tasks
//! show` prints it, and the hourly roll-up carries a landed task's into its day file
//! ([`crate::reports`]), where the Reports page groups the landed tasks by it.

use serde_json::Value;
use std::collections::BTreeMap;

/// The board field holding a task's kind.
pub const MARK: &str = "kind";

/// The kinds a task may be, in the order a list names them.
pub const KINDS: [&str; 5] = ["feature", "fix", "red", "tests", "docs"];

/// The kind a red's filing is given when it names none.
pub const RED: &str = "red";

/// `kind` as one of [`KINDS`], as the board keeps it; the refusal names the list.
pub fn named(kind: &str) -> Result<&'static str, String> {
    KINDS
        .iter()
        .copied()
        .find(|known| *known == kind)
        .ok_or_else(|| {
            let (last, rest) = KINDS.split_last().unwrap_or((&"", &[]));
            format!(
                "a task's kind is one of {} or {last}, not {}",
                rest.join(", "),
                crate::quote::repr(kind)
            )
        })
}

/// The kind a task's `fields` record, when they record one of [`KINDS`].
pub fn marked(fields: &BTreeMap<String, BTreeMap<String, Value>>) -> Option<&'static str> {
    (fields.get(crate::red::FIELDS))
        .and_then(|board| board.get(MARK))
        .and_then(Value::as_str)
        .and_then(|kind| named(kind).ok())
}

impl crate::Task {
    /// Its kind, when it was given one ([`marked`]).
    pub fn kind(&self) -> Option<&'static str> {
        marked(&self.fields)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fields(plugin: &str, name: &str, value: Value) -> BTreeMap<String, BTreeMap<String, Value>> {
        BTreeMap::from([(
            plugin.to_string(),
            BTreeMap::from([(name.to_string(), value)]),
        )])
    }

    #[test]
    fn a_kind_is_one_of_the_list_and_a_refusal_names_the_list() {
        for kind in KINDS {
            assert_eq!(named(kind), Ok(kind));
        }
        for kind in [
            "", "Feature", "feature ", " fix", "bug", "test", "doc", "red\n", "é",
        ] {
            let why = named(kind).expect_err(kind);
            assert_eq!(
                why,
                format!(
                    "a task's kind is one of feature, fix, red, tests or docs, not {}",
                    crate::quote::repr(kind)
                )
            );
        }
        assert!(KINDS.contains(&RED));
    }

    #[test]
    fn only_the_boards_own_field_naming_a_kind_is_a_kind() {
        assert_eq!(
            marked(&fields("_board", MARK, json!("tests"))),
            Some("tests")
        );
        for (fields, what) in [
            (fields("_board", MARK, json!("")), "an empty name"),
            (
                fields("_board", MARK, json!("chore")),
                "no kind of the list",
            ),
            (fields("_board", MARK, Value::Null), "a null"),
            (fields("_board", MARK, json!(["fix"])), "a list"),
            (fields("_board", "arm", json!("fix")), "another board field"),
            (fields("plugin", MARK, json!("fix")), "a plugin's field"),
            (BTreeMap::new(), "no fields"),
        ] {
            assert_eq!(marked(&fields), None, "{what}");
        }
    }
}
