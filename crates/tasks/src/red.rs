//! A red task: work filed for a red that stops every landing, such as a project's verification of
//! its main line or its release run going red. Its filer marks it where it files it
//! ([`crate::NewTask::red`]), or later for a red filed before it marked reds
//! ([`crate::Store::mark_red`]), and the board keeps the mark among the task's own fields, beside
//! the plugins' ([`FIELDS`]`.`[`MARK`], the text naming what is red), so a reader judges a task by
//! its record, never its slug. Only a filer marks: a session of kind [`FILER`], as the tools that
//! file reds register; the board refuses any other session's mark, and keeps its own fields out of
//! `set_field`'s reach. A filer is a role a session takes as it registers, on its word as every
//! role is, so the mark goes only on a task of the series reds are filed in ([`SERIES`]), never
//! on one named under it ([`filed`]): no feature takes it, whoever marks it.
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
/// The series reds are filed in, by slug: a red verify of the main line, which a landing files, and
/// a red release run's findings; last, the series a release run filed them in before the rename to
/// `just release`, whose reds still open are its own until they close. Only their tasks take a red
/// mark ([`filed`]).
pub const SERIES: [&str; 3] = [
    "tests/main-red-",
    "tests/release-red-",
    "tests/nightly-red-",
];
/// How long a red task may go without an executor before the PULL digest names it: ten minutes.
pub const UNSTAFFED_SECONDS: f64 = 600.0;
/// How a line of a red's body opens that names the commits it came between, `between: GOOD..BAD`:
/// the newest commit its filer knew green and the first it found red, each by its object name
/// (the release run's last green run and its candidate). The roll-up lists the landings in that
/// range on the red's day files; a red whose body names none is filed and read as any other.
pub const BETWEEN: &str = "between: ";

/// The range of commits `body` names a red came between ([`BETWEEN`]), as `GOOD..BAD`: its last
/// line that names one, so a quoted log above it cannot. None when no line does, or when an end is
/// no object name (7 to 64 hex digits), which git would read as something else.
pub fn between(body: &str) -> Option<&str> {
    let commit =
        |name: &str| (7..=64).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_hexdigit());
    body.lines().rev().find_map(|line| {
        let range = line.trim_end().strip_prefix(BETWEEN)?;
        let (good, bad) = range.split_once("..")?;
        (commit(good) && commit(bad)).then_some(range)
    })
}

/// Whether a task's `fields` mark it red.
pub fn marked(fields: &BTreeMap<String, BTreeMap<String, Value>>) -> bool {
    (fields.get(FIELDS))
        .and_then(|board| board.get(MARK))
        .is_some_and(|what| !what.is_null())
}

impl crate::Task {
    /// Whether it was filed for a red ([`marked`]).
    pub fn red(&self) -> bool {
        marked(&self.fields)
    }
}

/// Whether a task in `status` is open, so that its mark, when it has one, exempts the work under it.
pub fn open(status: &str) -> bool {
    !matches!(status, "done" | "cancelled")
}

/// Whether `slug` is a task of a series reds are filed in ([`SERIES`]), the one kind a red mark
/// goes on ([`filed_in`]): never a task named under one, which is ordinary work.
pub fn filed(slug: &str) -> bool {
    SERIES.iter().any(|series| filed_in(series, slug))
}

/// Whether `slug` is a task the series of slugs `series` begins files itself: a name past the
/// prefix with no `/`, so `tests/main-red-3e0c` and not `tests/main-red-3e0c/fix`, a task named
/// under it. What marks a red, reads a series' reds again to mark them, or stops a landing for
/// one, judges a slug by this.
pub fn filed_in(series: &str, slug: &str) -> bool {
    (slug.strip_prefix(series)).is_some_and(|name| !name.is_empty() && !name.contains('/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fields(plugin: &str, name: &str, value: Value) -> BTreeMap<String, BTreeMap<String, Value>> {
        BTreeMap::from([(plugin.into(), BTreeMap::from([(name.into(), value)]))])
    }

    /// A task's fields, whether they mark it red, and why.
    struct Mark {
        fields: BTreeMap<String, BTreeMap<String, Value>>,
        red: bool,
        why: &'static str,
    }

    #[test]
    fn only_the_boards_own_mark_marks_a_task_red() {
        let cases = [
            Mark {
                fields: fields(FIELDS, MARK, json!("main")),
                red: true,
                why: "the board's mark",
            },
            Mark {
                fields: BTreeMap::new(),
                red: false,
                why: "no fields",
            },
            Mark {
                fields: fields(FIELDS, MARK, Value::Null),
                red: false,
                why: "a cleared mark",
            },
            Mark {
                fields: fields("plugin", MARK, json!("main")),
                red: false,
                why: "a plugin's field of the same name",
            },
            Mark {
                fields: fields(FIELDS, "other", json!("main")),
                red: false,
                why: "another board field",
            },
        ];
        for Mark { fields, red, why } in cases {
            assert_eq!(marked(&fields), red, "{why}: {fields:?}");
        }
    }

    #[test]
    fn only_a_task_of_a_red_series_is_filed_for_a_red() {
        let cases = [
            ("tests/main-red-3e0c", true),
            ("tests/nightly-red-sim-determinism", true),
            ("tests/nightly-red-sim-2", true),
            ("tests/release-red-verify-tests", true),
            ("tests/release-red-run-2", true),
            ("tests/release-red-run/fix", false),
            ("tests/release-red", false),
            ("tests/main-red-", false),
            ("tests/main-red-old/feature", false),
            ("tests/nightly-red-sim/fix", false),
            ("work/feature", false),
            ("tests/main-verify", false),
            ("work/tests/main-red-3e0c", false),
            ("tests/nightly-red", false),
        ];
        for (slug, red) in cases {
            assert_eq!(filed(slug), red, "{slug}");
        }
    }

    #[test]
    fn a_red_names_the_commits_it_came_between_on_its_last_line_that_names_two() {
        let good = "0123456789abcdef0123456789abcdef01234567";
        let bad = "fedcba9876543210fedcba9876543210fedcba98";
        let range = format!("{good}..{bad}");
        let footer = |line: &str| format!("The release run is red.\n\n{line}\nkey: verify\n");
        assert_eq!(
            between(&footer(&format!("between: {range}"))),
            Some(range.as_str())
        );
        assert_eq!(
            between(&format!("between: {range}  \r")),
            Some(range.as_str()),
            "trailing blanks are no part of it"
        );
        assert_eq!(
            between("between: abc1234..DEF5678"),
            Some("abc1234..DEF5678")
        );
        let quoted = format!("between: 1111111..2222222\n\nbetween: {range}\nkey: verify");
        assert_eq!(
            between(&quoted),
            Some(range.as_str()),
            "the last line that names one: a quoted log above it names nothing"
        );
        let sha256 = format!("{}..{}", "a".repeat(64), "b".repeat(64));
        assert_eq!(
            between(&format!("between: {sha256}")),
            Some(sha256.as_str())
        );
        for (body, why) in [
            ("", "no body"),
            ("The release run is red.\nkey: verify", "no line names one"),
            ("between: 0123456", "one end"),
            ("between: 0123456..", "no second end"),
            ("between: ..0123456", "no first end"),
            (
                "between: 012345..0123456",
                "an end too short for git to take",
            ),
            (
                "between: 0123456...0123456",
                "three dots, a symmetric difference",
            ),
            ("between: main..0123456", "a branch, which moves"),
            ("between: --output=x..0123456", "an option"),
            ("between: 0123456..0123456 extra", "more after it"),
            (" between: 0123456..1234567", "not at the line's start"),
            ("Between: 0123456..1234567", "another word"),
        ] {
            assert_eq!(between(body), None, "{why}: {body:?}");
        }
        let long = format!("between: {}..0123456", "a".repeat(65));
        assert_eq!(between(&long), None, "an end longer than any object name");
    }
}
