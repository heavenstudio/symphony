//! A task's arm: the label of the process experiment's side it was filed on, one word its filer
//! gives with `tasks create --arm NAME` ([`crate::Store::create_in_arm`]) and nothing changes
//! after, since an arm moved once the work is under way spoils the comparison. The board keeps it
//! among the task's own fields ([`crate::red::FIELDS`]`.`[`MARK`]), beside the red and fast-fix
//! marks and out of `set_field`'s reach: no table and no field of the schema holds it. `tasks show`
//! prints it, and `tasks stats` gives each arm's figures ([`crate::stages::stats::Arm`]).
//!
//! The board assigns no arm, applies no rule for one and picks no winner: whoever files the work
//! says which side it is on, and reads the figures.

use serde_json::Value;
use std::collections::BTreeMap;

/// The board field holding a task's arm.
pub const MARK: &str = "arm";
/// The longest name an arm takes.
pub const LONGEST: usize = 32;

/// `name` as an arm's name: lowercase letters, digits and dashes, starting with a letter or a
/// digit, at most [`LONGEST`]. The refusal says what one is.
pub fn named(name: &str) -> Result<&str, String> {
    let plain = (name.bytes()).all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if name.is_empty() || name.len() > LONGEST || !plain || name.starts_with('-') {
        return Err(format!(
            "an arm is one word of lowercase letters, digits and dashes, at most {LONGEST} long, \
             such as control, not {}",
            crate::quote::repr(name)
        ));
    }
    Ok(name)
}

/// The arm a task's `fields` record, when they record one.
pub fn marked(fields: &BTreeMap<String, BTreeMap<String, Value>>) -> Option<&str> {
    (fields.get(crate::red::FIELDS))
        .and_then(|board| board.get(MARK))
        .and_then(Value::as_str)
        .filter(|arm| named(arm).is_ok())
}

impl crate::Task {
    /// The arm it was filed on, when it was filed on one ([`marked`]).
    pub fn arm(&self) -> Option<&str> {
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
    fn an_arm_is_one_plain_word() {
        for name in ["a", "control", "no-review", "b2", "2", &"x".repeat(LONGEST)] {
            assert_eq!(named(name), Ok(name));
        }
        for name in [
            "",
            " ",
            "Control",
            "two words",
            "-a",
            "a_b",
            "a/b",
            "é",
            " a",
            "a\n",
            &"x".repeat(LONGEST + 1),
        ] {
            let why = named(name).expect_err(name);
            assert!(why.starts_with("an arm is one word"), "{why}");
            assert!(why.ends_with(&crate::quote::repr(name)), "{why}");
        }
    }

    #[test]
    fn only_the_boards_own_field_with_a_name_is_an_arm() {
        assert_eq!(
            marked(&fields("_board", MARK, json!("control"))),
            Some("control")
        );
        for (fields, what) in [
            (fields("_board", MARK, json!("")), "an empty name"),
            (fields("_board", MARK, json!("Two Words")), "no arm's name"),
            (fields("_board", MARK, Value::Null), "a null"),
            (fields("_board", MARK, json!(1)), "a number"),
            (
                fields("_board", "red", json!("control")),
                "another board field",
            ),
            (fields("plugin", MARK, json!("control")), "a plugin's field"),
            (BTreeMap::new(), "no fields"),
        ] {
            assert_eq!(marked(&fields), None, "{what}");
        }
    }
}
