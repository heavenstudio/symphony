//! The catalogue as a project file writes it, and a task's needs as the board keeps and says them.

use super::*;
use serde_json::json;

fn names(catalogue: &Catalogue) -> Vec<&'static str> {
    catalogue.names()
}

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
}

#[test]
fn with_no_stamps_written_the_catalogue_is_the_symphonys_four_review_needed_by_default() {
    for text in ["", "[project]\nname = \"x\"\n", "[land]\ncheck = false\n"] {
        let catalogue = Catalogue::parse(text).unwrap();
        assert_eq!(catalogue, Catalogue::symphony(), "{text:?}");
        assert!(!catalogue.written);
        assert_eq!(names(&catalogue), READERS);
        assert_eq!(catalogue.defaults(), ["review"]);
    }
    let own = Catalogue::symphony();
    for stamp in &own.stamps {
        for text in [stamp.met, &stamp.applies, &stamp.skips] {
            assert!(!text.trim().is_empty(), "{}", stamp.name);
        }
    }
    // The `review` guideline names the paths the board computes for the assessor.
    assert!(own.stamps[1].applies.contains("[land] check_paths"));
}

/// A change of settings' values is held to the settings check and no reviewer: `review` skips it,
/// `tests` names the check that proves it, and `review` still applies to a setting that is code
/// or a command.
#[test]
fn a_change_of_settings_values_skips_review_for_the_settings_check_as_tests() {
    let own = Catalogue::symphony();
    let (tests, review) = (&own.stamps[0], &own.stamps[1]);
    assert_eq!((tests.name, review.name), ("tests", "review"));
    assert!(
        tests.applies.contains("`symphony settings-check`"),
        "{}",
        tests.applies
    );
    assert!(
        tests
            .applies
            .contains("no model or recipe judged against a list"),
        "{}",
        tests.applies
    );
    assert!(
        (review.skips).contains("the values the project file gives settings the Symphony already")
            && review.skips.contains("the settings check green at the tip"),
        "{}",
        review.skips
    );
    for kept in [
        "a setting the Symphony does not read yet",
        "a command or a program the Symphony runs or that decides what lands",
        "[project] cargo and default_branch",
        "[plugins] crates and process",
        "[verdicts], [land], [releases], [stamps]",
        "the workflow's [workflow], [[workflow_mappings]] and [[workflow_authorities]]",
        "[board] on_review, on_land, machine_work_reader, postgresql, external_postgresql and \
         assessments",
    ] {
        assert!(review.applies.contains(kept), "{kept}: {}", review.applies);
    }
}

#[test]
fn a_project_writes_its_whole_list_each_key_left_out_the_symphonys_own() {
    let text = "\
[stamps.review]
default = true

[stamps.tests]
default = true
applies = \"every change\"
skips = \"  \"
";
    let catalogue = Catalogue::parse(text).unwrap();
    assert!(catalogue.written);
    // Listed in the readers' order, whatever order the file writes them in.
    assert_eq!(names(&catalogue), ["tests", "review"]);
    assert_eq!(catalogue.defaults(), ["tests", "review"]);
    let own = Catalogue::symphony();
    let (tests, review) = (&catalogue.stamps[0], &catalogue.stamps[1]);
    assert_eq!(tests.applies, "every change");
    assert_eq!(tests.skips, own.stamps[0].skips, "a blank key is left out");
    assert_eq!(tests.met, own.stamps[0].met);
    assert_eq!(review, &own.stamps[1]);
    // A stamp written with no keys at all is the Symphony's own, its default too.
    let owner = Catalogue::parse("[stamps.owner]\n[stamps.review]\ndefault = false\n").unwrap();
    assert_eq!(names(&owner), ["review", "owner"]);
    assert!(owner.defaults().is_empty(), "nothing needed unassessed");
    // Inline, as a table of tables.
    let inline = Catalogue::parse("stamps = { pictures = { default = true } }\n").unwrap();
    assert_eq!(names(&inline), ["pictures"]);
    assert_eq!(inline.defaults(), ["pictures"]);
}

#[test]
fn only_pictures_is_waivable_by_a_gate_decision_and_a_project_may_mark_it_not() {
    let own = Catalogue::symphony();
    let waivable: Vec<&str> = (own.stamps.iter())
        .filter(|stamp| stamp.waivable)
        .map(|stamp| stamp.name)
        .collect();
    assert_eq!(waivable, ["pictures"]);
    assert_eq!(own.unwaivable("pictures"), None);
    for never in crate::gates::NEVER {
        let why = own.unwaivable(never).unwrap();
        assert!(
            why.starts_with(&format!("{never} is never waived: tests (a red")),
            "{why}"
        );
        assert!(
            why.ends_with("; the catalogue's waivable stamps are pictures"),
            "{why}"
        );
    }
    assert_eq!(
        own.unwaivable("lint").unwrap(),
        "the catalogue has no stamp \"lint\": its stamps are tests, review, pictures, owner \
         (`just tasks stamps` lists them); a raise names a number in a file, PATH:NAME"
    );
    // A project's own catalogue keeps the Symphony's mark unless it writes one.
    let kept = Catalogue::parse("[stamps.review]\n[stamps.pictures]\n").unwrap();
    assert_eq!(kept.unwaivable("pictures"), None);
    let strict =
        Catalogue::parse("[stamps.review]\n[stamps.pictures]\nwaivable = false\n").unwrap();
    assert_eq!(
        strict.unwaivable("pictures").unwrap(),
        "the catalogue does not mark pictures waivable ([stamps.pictures] waivable); the \
         catalogue marks none waivable"
    );
    // tests, review and owner cannot be marked waivable, named with their lines.
    assert_eq!(
        Catalogue::parse("[stamps.tests]\nwaivable = true\n[stamps.owner]\nwaivable = true\n")
            .unwrap_err(),
        format!(
            "symphony.toml: [stamps] marks waivable \"tests\" (line 2), \"owner\" (line 4), \
             which no gate decision waives: {}",
            crate::gates::NEVER_WHY
        )
    );
    // Marked not waivable, they read as they are.
    assert!(Catalogue::parse("[stamps.review]\nwaivable = false\n").is_ok());
}

#[test]
fn a_stamp_the_symphony_has_no_reader_for_is_refused_by_name_and_line() {
    assert_eq!(
        Catalogue::parse("[stamps.review]\n\n[stamps.lint]\ndefault = true\n").unwrap_err(),
        "symphony.toml: [stamps] names a stamp the Symphony has no reader for, \"lint\" (line \
         3): it has no way to stand on a tip; a stamp is one of tests, review, pictures, owner"
    );
    assert_eq!(
        Catalogue::parse("[stamps]\nsecurity = {}\nReview = {}\ntests = {}\n").unwrap_err(),
        "symphony.toml: [stamps] names stamps the Symphony has no reader for, \"Review\" (line \
         3), \"security\" (line 2): they have no way to stand on a tip; a stamp is one of tests, \
         review, pictures, owner"
    );
}

#[test]
fn a_catalogue_naming_no_stamp_a_key_no_stamp_has_or_a_value_of_another_kind_is_refused() {
    assert_eq!(
        Catalogue::parse("[stamps]\n").unwrap_err(),
        "symphony.toml: [stamps] names no stamp: leave the table out for the Symphony's own \
         four, or write each stamp the project's land can require"
    );
    for (text, said) in [
        ("[stamps.review]\nneeded = true\n", "unknown field `needed`"),
        ("[stamps.review]\ndefault = \"yes\"\n", "invalid type"),
        ("[stamps]\nreview = true\n", "invalid type"),
        ("stamps = [\"review\"]\n", "invalid type"),
        ("[stamps\n", "symphony.toml: "),
    ] {
        let problem = Catalogue::parse(text).unwrap_err();
        assert!(
            problem.starts_with("symphony.toml: "),
            "{text:?}: {problem}"
        );
        assert!(problem.contains(said), "{text:?}: {problem}");
    }
}

#[test]
fn an_assessment_names_stamps_of_the_catalogue_or_none_alone() {
    let own = Catalogue::symphony();
    assert_eq!(
        own.needs(&strings(&["review", "tests", "review", " pictures "]))
            .unwrap(),
        ["tests", "review", "pictures"],
        "in the catalogue's order, each once"
    );
    assert!(own.needs(&strings(&["none"])).unwrap().is_empty());
    assert_eq!(
        own.needs(&strings(&["tests", "none"])).unwrap_err(),
        "none is no stamp: name it alone for a task that needs none, or name the stamps it needs"
    );
    assert_eq!(
        own.needs(&[]).unwrap_err(),
        "name the stamps the task needs, of tests, review, pictures, owner, or none for none"
    );
    // A stamp the catalogue lacks, though the Symphony has a reader for it.
    let project = Catalogue::parse("[stamps.tests]\n[stamps.review]\n").unwrap();
    for lacked in ["owner", "lint", "Review"] {
        assert_eq!(
            project.needs(&strings(&["tests", lacked])).unwrap_err(),
            format!(
                "the catalogue has no stamp {lacked:?}: its stamps are tests, review (`just tasks \
                 stamps` lists them)"
            )
        );
    }
}

#[test]
fn the_needs_are_read_from_the_boards_own_field_alone() {
    let needs = Needs {
        stamps: strings(&["tests", "review"]),
        tip: Some("1a2b3c4d5e6f".into()),
        by: "able-otter".into(),
        why: "a store query and its test, no page or wire".into(),
        import: None,
    };
    let fields = |plugin: &str, value: Value| {
        BTreeMap::from([(
            plugin.to_string(),
            BTreeMap::from([(NEEDS.to_string(), value)]),
        )])
    };
    let value = serde_json::to_value(&needs).unwrap();
    assert_eq!(
        value,
        json!({"stamps": ["tests", "review"], "tip": "1a2b3c4d5e6f", "by": "able-otter",
               "why": "a store query and its test, no page or wire"})
    );
    assert_eq!(Needs::of(&fields("_board", value.clone())), Some(needs));
    assert_eq!(Needs::of(&fields("game", value)), None, "a plugin's field");
    assert_eq!(Needs::of(&BTreeMap::new()), None);
    assert_eq!(Needs::of(&fields("_board", json!("review"))), None);
    // None waived everything, read from the brief: no tip.
    let none = Needs::of(&fields(
        "_board",
        json!({"stamps": [], "tip": null, "by": "b", "why": "docs only"}),
    ))
    .unwrap();
    assert!(none.stamps.is_empty());
    assert_eq!(none.tip, None);
}

#[test]
fn the_threads_line_says_the_stamps_the_tip_read_what_it_needed_before_and_who() {
    let mut needs = Needs {
        stamps: strings(&["tests", "review"]),
        tip: Some("1a2b3c4d5e6f7a8b".into()),
        by: "able-otter".into(),
        why: "a store query and its test, no page or wire".into(),
        import: None,
    };
    assert_eq!(
        needs.line(&strings(&["review"])),
        "needs tests, review at 1a2b3c4 (was review): a store query and its test, no page or \
         wire; by able-otter"
    );
    needs.stamps.clear();
    needs.tip = None;
    needs.why = "documents only".into();
    assert_eq!(
        needs.line(&strings(&["tests", "review"])),
        "needs none from the brief (was tests, review): documents only; by able-otter"
    );
    assert_eq!(
        needs.line(&[]),
        "needs none from the brief (was none): documents only; by able-otter"
    );
    assert_eq!(short("abc"), "abc");
}

#[test]
fn a_commit_is_named_by_its_first_seven_characters_and_other_text_is_never_cut_inside_one() {
    assert_eq!(short("0123456789abcdef"), "0123456");
    assert_eq!(short("0123456"), "0123456");
    assert_eq!(short(""), "");
    // Byte 7 falls inside the `é`: the text whole, where a slice by index would panic.
    assert_eq!(short("abcdefé"), "abcdefé");
    assert_eq!(short("abcdeéf"), "abcdeé");
}
