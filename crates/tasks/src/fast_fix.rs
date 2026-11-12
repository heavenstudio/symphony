//! A fast fix: a bug fix whose recorded risk calls for no reviewer (RULES.md
//! `workflow/fast-bugfixes`). Its filer records that risk where it files it
//! ([`crate::NewTask::fast_fix`], `just tasks create --fast-fix WHY`), and the board keeps it among
//! the task's own fields ([`crate::red::FIELDS`]`.`[`MARK`], the text saying why no reviewer is
//! needed), out of `set_field`'s reach, so a reader judges a task by its record, never its slug.
//!
//! Such a fix is judged on its posted evidence by its orchestrator, which did not execute it: on a
//! task carrying the mark, and only there, the orchestrator's verdict is the task's verdict
//! ([`crate::Store::verdict`]), and its `approve` is what the default landing lands
//! ([`crate::land`]). Every other task is approved by a fresh reviewer. On a red ([`crate::red`])
//! carrying the mark the land bounds that approve by what the fix changes
//! ([`crate::land::Scope::ChecksUnchanged`]): the board cannot read the repository, the land can.

use serde_json::Value;
use std::collections::BTreeMap;

/// The board field marking a fast fix: why its fix needs no reviewer, as its filer recorded it.
pub const MARK: &str = "fast_fix";

/// The risk a red's filer records for its fix when it files the red ([`crate::red`]) as a fast
/// fix, a red being fixed the way a bug fix is. A tool files a red before any fix exists, so what
/// it records is the bound the land holds the red's orchestrator's approve to
/// ([`crate::land::check_paths`]), said where the orchestrator reads it.
pub const RED: &str = "a red is fixed as a bug fix is (RULES.md `workflow/fast-bugfixes`): the \
code is fixed, never a test or check loosened. Its orchestrator's approve lands a fix that changes \
none of the project's tests and checks (`[land] check_paths`), on the posted evidence of the \
failing tests, unchanged, rerun alone and green at its tip; the land refuses it on a fix that \
changes a test, a check, a budget or a run step, which a fresh reviewer approves";

/// What a task's `fields` record of its fast fix, when they mark one.
pub fn marked(fields: &BTreeMap<String, BTreeMap<String, Value>>) -> Option<&str> {
    (fields.get(crate::red::FIELDS))
        .and_then(|board| board.get(MARK))
        .and_then(Value::as_str)
        .filter(|why| !why.trim().is_empty())
}

impl crate::Task {
    /// Why it is a fast fix, when it was filed as one ([`marked`]).
    pub fn fast_fix(&self) -> Option<&str> {
        marked(&self.fields)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fields(plugin: &str, name: &str, value: Value) -> BTreeMap<String, BTreeMap<String, Value>> {
        BTreeMap::from([(plugin.into(), BTreeMap::from([(name.into(), value)]))])
    }

    #[test]
    fn only_the_boards_own_field_with_a_reason_marks_a_fast_fix() {
        let why = "a one-line regex fix";
        assert_eq!(marked(&fields("_board", MARK, json!(why))), Some(why));
        for (fields, case) in [
            (BTreeMap::new(), "no fields"),
            (fields("_board", MARK, json!("")), "an empty reason"),
            (fields("_board", MARK, json!("  ")), "a blank reason"),
            (fields("_board", MARK, Value::Null), "a null"),
            (fields("_board", MARK, json!(true)), "not a reason"),
            (fields("_board", "red", json!(why)), "another board field"),
            (
                fields("game", MARK, json!(why)),
                "a plugin's field of that name",
            ),
        ] {
            assert_eq!(marked(&fields), None, "{case}");
        }
    }

    #[test]
    fn a_reds_recorded_risk_marks_it_a_fast_fix_and_bounds_it() {
        assert_eq!(marked(&fields("_board", MARK, json!(RED))), Some(RED));
        for said in [
            "RULES.md `workflow/fast-bugfixes`",
            "never a test or check loosened",
            "changes none of the project's tests and checks (`[land] check_paths`)",
            "unchanged, rerun alone and green at its tip",
            "changes a test, a check, a budget or a run step, which a fresh reviewer approves",
        ] {
            assert!(RED.contains(said), "{said:?} in {RED}");
        }
    }
}
