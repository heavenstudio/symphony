//! What a release says it holds, in its tag's message: the board schema and the pin at its commit,
//! each said changed or not since the release before it, the checks it passed, the packages it
//! publishes on crates.io, what a project does to take it ([`super::consumer`]), and the changes on
//! main since that release. A project's update reads the
//! schema and the pin to know whether it must update (docs/workflow/RELEASES.md), so the message names both
//! by the words "Board schema" and "Pin".

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::git::{git, short};

/// The board's schema, `VERSION` in this file of the Symphony's source.
pub(crate) const SCHEMA_FILE: &str = "crates/tasks/src/schema.rs";
/// The pin, `PIN` in this file of the Symphony's source.
pub(crate) const PIN_FILE: &str = "crates/symphony/src/machine.rs";
/// The changes a message lists at most; the rest are counted.
const LISTED: usize = 100;

/// The board schema and the pin a commit holds, each none when its file does not say.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Facts {
    pub(crate) schema: Option<i64>,
    pub(crate) pin: Option<i64>,
}

/// The release before this one: its tag, its commit and what it held.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Previous {
    pub(crate) tag: String,
    pub(crate) commit: String,
    pub(crate) facts: Facts,
}

/// The value of the integer constant `name` that `text` defines on a line of its own
/// (`pub const VERSION: i64 = 16;`), or none.
pub(crate) fn constant(text: &str, name: &str) -> Option<i64> {
    text.lines().find_map(|line| {
        let line = line.trim();
        let rest = line
            .strip_prefix("pub const ")
            .or_else(|| line.strip_prefix("const "))?;
        let (named, rest) = rest.split_once(':')?;
        if named.trim() != name {
            return None;
        }
        let (_, value) = rest.split_once('=')?;
        value
            .trim()
            .strip_suffix(';')?
            .trim()
            .replace('_', "")
            .parse()
            .ok()
    })
}

/// What the commit `commit` of the repository at `root` holds.
pub(crate) fn facts(root: &Path, commit: &str) -> Facts {
    let read = |file: &str, name: &str| {
        git(root, &["show", &format!("{commit}:{file}")])
            .ok()
            .and_then(|text| constant(&text, name))
    };
    Facts {
        schema: read(SCHEMA_FILE, "VERSION"),
        pin: read(PIN_FILE, "PIN"),
    }
}

/// The line saying `what` is `now`, and whether it changed since `previous` held `before`.
fn fact(what: &str, now: Option<i64>, before: Option<(&str, Option<i64>)>) -> String {
    let Some(now) = now else {
        return format!("{what} unknown: its file names none.");
    };
    match before {
        None => format!("{what} {now}."),
        Some((tag, Some(old))) if old == now => format!("{what} {now}, unchanged since {tag}."),
        Some((tag, Some(old))) => format!("{what} {now}, changed since {tag} ({old} then)."),
        Some((tag, None)) => format!("{what} {now}; {tag} named none."),
    }
}

/// The tag `tag`'s message for the release of `commit`: what it holds against `previous` (none for
/// the first release), the checks it passed, what it publishes on crates.io (`crates`, its lines:
/// [`super::crates::notes`]), what a release candidate's record says of it (`candidate`, its lines:
/// [`super::candidate::Candidate::notes`]), the failures that passed when the run reran them alone
/// (`alone`, its lines: [`super::alone::notes`]), what a project does to take it (`taking`, its lines:
/// [`super::consumer::taking`]), and `changes`, the commits on main since `previous`, one `SHORT
/// SUBJECT` line each, newest first.
#[expect(
    clippy::too_many_arguments,
    reason = "each is a part of the message, in the order it says them"
)]
pub(crate) fn message(
    tag: &str,
    commit: &str,
    facts: Facts,
    previous: Option<&Previous>,
    checks: &[String],
    crates: &[String],
    candidate: &[String],
    alone: &[String],
    taking: &[String],
    changes: &[String],
) -> String {
    let before =
        |pick: fn(&Facts) -> Option<i64>| previous.map(|p| (p.tag.as_str(), pick(&p.facts)));
    let mut text = format!(
        "{tag}: Symphony main at {}\n\n{}\n{}\n",
        short(commit),
        fact("Board schema", facts.schema, before(|f| f.schema)),
        fact("Pin", facts.pin, before(|f| f.pin)),
    );
    if !checks.is_empty() {
        text.push_str(&format!("\nGreen: {}.\n", checks.join(", ")));
    }
    for lines in [crates, candidate, alone, taking] {
        if !lines.is_empty() {
            text.push('\n');
            for line in lines {
                text.push_str(line);
                text.push('\n');
            }
        }
    }
    match previous {
        None => text.push_str("\nThe first release.\n"),
        Some(previous) => {
            text.push_str(&format!(
                "\nChanges on main since {} ({}):\n",
                previous.tag,
                changes.len()
            ));
            for change in changes.iter().take(LISTED) {
                text.push_str(change);
                text.push('\n');
            }
            if changes.len() > LISTED {
                text.push_str(&format!("… and {} more\n", changes.len() - LISTED));
            }
        }
    }
    text
}

/// The commits on main's first-parent line from `since` (left out) to `commit`, newest first, one
/// `SHORT SUBJECT` line each: what a release adds to the one before.
pub(crate) fn changes(root: &Path, since: &str, commit: &str) -> Result<Vec<String>, String> {
    let listed = git(
        root,
        &[
            "log",
            "--first-parent",
            "--format=%h %s",
            &format!("{since}..{commit}"),
        ],
    )?;
    Ok(listed.lines().map(str::to_string).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_constant_is_read_from_its_own_line() {
        let text = "//! The schema.\n\n/// The version.\npub const VERSION: i64 = 16;\nconst OTHER: u32 = 2;\n";
        assert_eq!(constant(text, "VERSION"), Some(16));
        assert_eq!(constant(text, "OTHER"), Some(2));
        assert_eq!(
            constant("    pub const PIN: u32 = 1_000;", "PIN"),
            Some(1000)
        );
        for (text, why) in [
            (
                "pub const VERSIONS: i64 = 16;",
                "another name that starts the same",
            ),
            ("pub const VERSION: i64 = SIXTEEN;", "not a number"),
            (
                "pub const VERSION: i64 = 16",
                "no semicolon: not the whole value",
            ),
            ("// pub const VERSION: i64 = 16;", "a comment"),
            ("let VERSION = 16;", "not a constant"),
            ("", "nothing"),
        ] {
            assert_eq!(constant(text, "VERSION"), None, "{why}");
        }
    }

    /// The files a release reads its schema and pin from say them as the reading expects: a rename
    /// or a move here would leave every release's message saying "unknown".
    #[test]
    fn this_repositorys_schema_and_pin_read_as_the_values_they_are() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let read = |file: &str| std::fs::read_to_string(workspace.join(file)).unwrap();
        assert_eq!(
            constant(&read(SCHEMA_FILE), "VERSION"),
            Some(symphony_board::schema::VERSION)
        );
        assert_eq!(
            constant(&read(PIN_FILE), "PIN"),
            Some(i64::from(symphony::machine::PIN))
        );
    }

    #[test]
    fn each_fact_says_whether_it_changed_since_the_release_before() {
        assert_eq!(fact("Pin", Some(1), None), "Pin 1.");
        assert_eq!(
            fact("Pin", Some(1), Some(("release-2026-11-11", Some(1)))),
            "Pin 1, unchanged since release-2026-11-11."
        );
        assert_eq!(
            fact(
                "Board schema",
                Some(17),
                Some(("release-2026-11-11", Some(16)))
            ),
            "Board schema 17, changed since release-2026-11-11 (16 then)."
        );
        assert_eq!(
            fact("Board schema", Some(17), Some(("release-2026-11-11", None))),
            "Board schema 17; release-2026-11-11 named none."
        );
        assert_eq!(
            fact("Board schema", None, Some(("release-2026-11-11", Some(16)))),
            "Board schema unknown: its file names none."
        );
    }

    #[test]
    fn the_first_release_says_so_and_lists_no_changes() {
        let said = message(
            "release-2026-11-12",
            "0123456789abcdef0123",
            Facts {
                schema: Some(16),
                pin: Some(1),
            },
            None,
            &["verify".into()],
            &[],
            &[],
            &[],
            &[],
            &["abc first".into()],
        );
        assert_eq!(
            said,
            "release-2026-11-12: Symphony main at 0123456789ab\n\n\
             Board schema 16.\nPin 1.\n\nGreen: verify.\n\nThe first release.\n"
        );
    }

    #[test]
    fn a_later_release_names_what_changed_and_lists_the_changes() {
        let previous = Previous {
            tag: "release-2026-11-11".into(),
            commit: "f".repeat(40),
            facts: Facts {
                schema: Some(16),
                pin: Some(1),
            },
        };
        let said = message(
            "release-2026-11-12",
            &"0".repeat(40),
            Facts {
                schema: Some(17),
                pin: Some(1),
            },
            Some(&previous),
            &["verify".into(), "lint".into()],
            &[],
            &[],
            &[],
            &[],
            &["bbb second".into(), "aaa first".into()],
        );
        assert_eq!(
            said,
            "release-2026-11-12: Symphony main at 000000000000\n\n\
             Board schema 17, changed since release-2026-11-11 (16 then).\n\
             Pin 1, unchanged since release-2026-11-11.\n\n\
             Green: verify, lint.\n\n\
             Changes on main since release-2026-11-11 (2):\nbbb second\naaa first\n"
        );
    }

    #[test]
    fn a_candidates_release_says_its_record_between_the_checks_and_the_changes() {
        let said = message(
            "release-2026-11-12",
            &"0".repeat(40),
            Facts::default(),
            None,
            &["verify".into()],
            &[],
            &[
                "Release candidate: first judged aaa.".into(),
                "- tests/release-red-verify (verify): advanced it to bbb".into(),
            ],
            &[],
            &[],
            &[],
        );
        assert!(
            said.ends_with(
                "Green: verify.\n\nRelease candidate: first judged aaa.\n- \
                 tests/release-red-verify (verify): advanced it to bbb\n\nThe first release.\n"
            ),
            "{said}"
        );
    }

    #[test]
    fn what_a_release_publishes_is_said_after_its_checks_and_before_its_candidate() {
        let said = message(
            "release-2026-11-12",
            &"0".repeat(40),
            Facts::default(),
            None,
            &["verify".into()],
            &["Publishes to crates.io: a 1.0.0, b 0.1.0.".into()],
            &["Release candidate: first judged aaa.".into()],
            &[],
            &[],
            &[],
        );
        assert!(
            said.ends_with(
                "Green: verify.\n\nPublishes to crates.io: a 1.0.0, b 0.1.0.\n\nRelease candidate: \
                 first judged aaa.\n\nThe first release.\n"
            ),
            "{said}"
        );
    }

    #[test]
    fn a_long_list_of_changes_is_cut_and_counted() {
        let previous = Previous {
            tag: "release-2026-11-11".into(),
            commit: "f".repeat(40),
            facts: Facts::default(),
        };
        let changes: Vec<String> = (0..LISTED + 3).map(|n| format!("c{n} change")).collect();
        let said = message(
            "release-2026-11-12",
            &"0".repeat(40),
            Facts::default(),
            Some(&previous),
            &[],
            &[],
            &[],
            &[],
            &[],
            &changes,
        );
        assert!(said.contains(&format!("({})", LISTED + 3)), "{said}");
        assert!(
            said.contains(&format!("c{} change\n", LISTED - 1)),
            "{said}"
        );
        assert!(!said.contains(&format!("c{} change", LISTED)), "{said}");
        assert!(said.ends_with("… and 3 more\n"), "{said}");
        assert!(!said.contains("Green:"), "no checks named, none said");
    }

    #[test]
    fn what_a_project_does_to_take_it_is_said_after_the_candidate_and_before_the_changes() {
        let previous = Previous {
            tag: "release-2026-11-11".into(),
            commit: "f".repeat(40),
            facts: Facts::default(),
        };
        let said = message(
            "release-2026-11-12",
            &"0".repeat(40),
            Facts::default(),
            Some(&previous),
            &["verify".into()],
            &[],
            &["Release candidate: first judged aaa.".into()],
            &[],
            &["For a project taking this release:".into(), "Board:".into()],
            &["aaa first".into()],
        );
        assert!(
            said.ends_with(
                "Green: verify.\n\nRelease candidate: first judged aaa.\n\nFor a project taking \
                 this release:\nBoard:\n\nChanges on main since release-2026-11-11 (1):\naaa \
                 first\n"
            ),
            "{said}"
        );
    }
}
