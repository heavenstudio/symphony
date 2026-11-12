use super::*;
use std::collections::{BTreeMap, BTreeSet};

const FIXTURE: Var = Var::new("SYMPHONY_FIXTURE");

/// A lookup over `pairs`, as a parsed environment answers: any value it holds, an empty one too.
fn holding<'a>(pairs: &'a [(&str, &'a str)]) -> impl FnOnce(&'static str) -> Option<&'a str> {
    let vars: BTreeMap<&str, &str> = pairs.iter().copied().collect();
    move |name| vars.get(name).copied()
}

#[test]
fn a_variable_is_read_by_its_one_name() {
    assert_eq!(
        FIXTURE.get(holding(&[("SYMPHONY_FIXTURE", "value")])),
        Some("value")
    );
    assert_eq!(
        FIXTURE.get(holding(&[("OTHER_FIXTURE", "other"), ("FIXTURE", "bare")])),
        None,
        "no other name answers for it"
    );
    assert_eq!(FIXTURE.get(holding(&[])), None);
}

#[test]
fn an_empty_value_is_unset_and_a_blank_one_is_the_readers() {
    assert_eq!(FIXTURE.get(holding(&[("SYMPHONY_FIXTURE", "")])), None);
    assert_eq!(
        FIXTURE.get(holding(&[("SYMPHONY_FIXTURE", " ")])),
        Some(" "),
        "what a blank value means is the reader's to say"
    );
}

#[test]
fn the_lookup_is_asked_the_name_once() {
    let mut asked = Vec::new();
    let found = FIXTURE.get(|name| {
        asked.push(name);
        Some(OsString::from("x"))
    });
    assert_eq!(found, Some(OsString::from("x")));
    assert_eq!(asked, ["SYMPHONY_FIXTURE"]);
}

#[test]
fn a_variable_no_process_sets_reads_as_none() {
    let unset = Var::new("SYMPHONY_ENV_TEST_NEVER_SET_0d1f6a");
    assert_eq!(unset.os(), None);
    assert_eq!(unset.text(), None);
}

#[test]
fn every_variable_is_named_once_under_symphony() {
    let names: BTreeSet<&str> = ALL.iter().map(|var| var.name).collect();
    assert_eq!(names.len(), ALL.len(), "a name is listed twice");
    for name in names {
        let rest = name
            .strip_prefix("SYMPHONY_")
            .unwrap_or_else(|| panic!("{name} is not a SYMPHONY_ name"));
        assert!(
            !rest.is_empty()
                && rest
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
            "{name}"
        );
        assert!(named(name), "{name}");
    }
}

#[test]
fn only_symphonys_variables_are_named() {
    assert!(named(PORT.name));
    assert!(named(LOW_DISK_GB.name));
    for other in ["PORT", "SYMPHONY_FIXTURE", "PATH", "", "symphony_port"] {
        assert!(!named(other), "{other}");
    }
}
