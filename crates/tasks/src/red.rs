//! A red task: work filed for a red that stops every landing, such as a project's verification of
//! its main line or its nightly run going red. Its filer marks it where it files it
//! ([`crate::NewTask::red`]), or later for a red filed before it marked reds
//! ([`crate::Store::mark_red`]), and the board keeps the mark among the task's own fields, beside
//! the plugins' ([`FIELDS`]`.`[`MARK`], the text naming what is red), so a reader judges a task by
//! its record, never its slug. Only a filer marks: a session of kind [`FILER`], as the tools that
//! file reds register; the board refuses any other session's mark, and keeps its own fields out of
//! `set_field`'s reach.
//!
//! The work that fixes a red waits for nothing: an open red task, and the work under it, takes no
//! orchestrator slot ([`crate::slots`]), and the PULL digest names a red task first while no one
//! executes it [`UNSTAFFED_SECONDS`] after it was filed ([`crate::cli::pull`]). A red done or
//! cancelled exempts nothing: the work under it, new work included, takes a slot as any does.

use serde_json::Value;
use std::collections::BTreeMap;

/// The board's own fields in a task's `fields`, beside each plugin's: a name no plugin takes.
pub const FIELDS: &str = "_board";
/// The board field marking a red task: what is red, as its filer names it.
pub const MARK: &str = "red";
/// The session kind of a red's filer, the one session a red mark is taken from.
pub const FILER: &str = "filer";
/// How long a red task may go without an executor before the PULL digest names it: ten minutes.
pub const UNSTAFFED_SECONDS: f64 = 600.0;

/// Whether a task's `fields` mark it red.
pub fn marked(fields: &BTreeMap<String, BTreeMap<String, Value>>) -> bool {
    (fields.get(FIELDS))
        .and_then(|board| board.get(MARK))
        .is_some_and(|what| !what.is_null())
}

/// Whether a task in `status` is open, so that its mark, when it has one, exempts the work under it.
pub fn open(status: &str) -> bool {
    !matches!(status, "done" | "cancelled")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fields(plugin: &str, name: &str, value: Value) -> BTreeMap<String, BTreeMap<String, Value>> {
        BTreeMap::from([(plugin.into(), BTreeMap::from([(name.into(), value)]))])
    }

    #[test]
    fn only_the_boards_own_mark_marks_a_task_red() {
        let cases = [
            (
                fields(FIELDS, MARK, json!("main")),
                true,
                "the board's mark",
            ),
            (BTreeMap::new(), false, "no fields"),
            (fields(FIELDS, MARK, Value::Null), false, "a cleared mark"),
            (
                fields("plugin", MARK, json!("main")),
                false,
                "a plugin's field of the same name",
            ),
            (
                fields(FIELDS, "other", json!("main")),
                false,
                "another board field",
            ),
        ];
        for (fields, red, why) in cases {
            assert_eq!(marked(&fields), red, "{why}: {fields:?}");
        }
    }
}
