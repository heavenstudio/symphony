//! A red task: work filed for a red that stops every landing, such as a project's verification of
//! its main line or its nightly run going red. Whoever files it marks it ([`crate::NewTask::red`]),
//! and the board keeps the mark among the task's own fields, beside the plugins' ([`FIELDS`]`.`
//! [`MARK`], the text naming what is red), so a reader judges a task by its record, never its slug.
//!
//! The work that fixes a red waits for nothing: a red task, and the work under it, takes no
//! orchestrator slot ([`crate::slots`]), and the PULL digest names a red task first while no one
//! executes it [`UNSTAFFED_SECONDS`] after it was filed ([`crate::cli::pull`]).

use serde_json::Value;
use std::collections::BTreeMap;

/// The board's own fields in a task's `fields`, beside each plugin's: a name no plugin takes.
pub const FIELDS: &str = "_board";
/// The board field marking a red task: what is red, as its filer names it.
pub const MARK: &str = "red";
/// How long a red task may go without an executor before the PULL digest names it: ten minutes.
pub const UNSTAFFED_SECONDS: f64 = 600.0;

/// Whether a task's `fields` mark it red.
pub fn marked(fields: &BTreeMap<String, BTreeMap<String, Value>>) -> bool {
    (fields.get(FIELDS))
        .and_then(|board| board.get(MARK))
        .is_some_and(|what| !what.is_null())
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
        assert!(marked(&fields(FIELDS, MARK, json!("main"))));
        assert!(!marked(&BTreeMap::new()), "no fields");
        assert!(
            !marked(&fields(FIELDS, MARK, Value::Null)),
            "a cleared mark"
        );
        assert!(
            !marked(&fields("plugin", MARK, json!("main"))),
            "a plugin's field of the same name"
        );
        assert!(!marked(&fields(FIELDS, "other", json!("main"))));
    }
}
