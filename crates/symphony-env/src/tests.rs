use super::*;
use std::collections::BTreeMap;

const FIXTURE: Var = Var::new("SYMPHONY_FIXTURE", "SF_FIXTURE");

/// A lookup over `pairs`, as a parsed environment answers: any value it holds, an empty one too.
fn holding<'a>(pairs: &'a [(&str, &'a str)]) -> impl FnMut(&'static str) -> Option<&'a str> {
    let vars: BTreeMap<&str, &str> = pairs.iter().copied().collect();
    move |name| vars.get(name).copied()
}

#[test]
fn the_new_name_wins_over_the_old_one_and_the_old_one_answers_alone() {
    assert_eq!(
        FIXTURE.pick(holding(&[("SYMPHONY_FIXTURE", "new")])),
        Some(("new", Named::New))
    );
    assert_eq!(
        FIXTURE.pick(holding(&[("SF_FIXTURE", "old")])),
        Some(("old", Named::Old))
    );
    assert_eq!(
        FIXTURE.pick(holding(&[
            ("SF_FIXTURE", "old"),
            ("SYMPHONY_FIXTURE", "new")
        ])),
        Some(("new", Named::New))
    );
    assert_eq!(FIXTURE.pick(holding(&[("SYMPHONY_OTHER", "x")])), None);
    assert_eq!(FIXTURE.pick(holding(&[])), None);
}

#[test]
fn what_counts_as_set_is_the_lookups_own() {
    let both = [("SYMPHONY_FIXTURE", ""), ("SF_FIXTURE", "old")];
    assert_eq!(FIXTURE.pick(holding(&both)), Some(("", Named::New)));
    let mut unset_when_empty = holding(&both);
    assert_eq!(
        FIXTURE.pick(|name| unset_when_empty(name).filter(|value| !value.is_empty())),
        Some(("old", Named::Old))
    );
}

#[test]
fn the_old_name_is_not_asked_once_the_new_one_answers() {
    let mut asked = Vec::new();
    let found = FIXTURE.pick(|name| {
        asked.push(name);
        Some(1)
    });
    assert_eq!(found, Some((1, Named::New)));
    assert_eq!(asked, ["SYMPHONY_FIXTURE"]);
    asked.clear();
    let found: Option<(u8, Named)> = FIXTURE.pick(|name| {
        asked.push(name);
        None
    });
    assert_eq!(found, None);
    assert_eq!(asked, ["SYMPHONY_FIXTURE", "SF_FIXTURE"]);
}

#[test]
fn a_value_found_under_the_old_name_is_said_once_per_variable() {
    let notices = Notices::new();
    let mut out = Vec::new();
    let old = [("SF_FIXTURE", "old")];
    for _ in 0..3 {
        assert_eq!(
            FIXTURE.read_noting(holding(&old), &notices, &mut out),
            Some("old")
        );
    }
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "SF_FIXTURE is the old name of SYMPHONY_FIXTURE and will stop being read: \
         set SYMPHONY_FIXTURE instead\n"
    );
    let other = Var::new("SYMPHONY_OTHER", "SF_OTHER");
    let mut out = Vec::new();
    assert_eq!(
        other.read_noting(holding(&[("SF_OTHER", "o")]), &notices, &mut out),
        Some("o")
    );
    assert_eq!(String::from_utf8(out).unwrap(), other.notice() + "\n");
}

#[test]
fn a_value_under_the_new_name_or_none_at_all_says_nothing() {
    let notices = Notices::new();
    let mut out = Vec::new();
    assert_eq!(
        FIXTURE.read_noting(holding(&[("SYMPHONY_FIXTURE", "new")]), &notices, &mut out),
        Some("new")
    );
    assert_eq!(FIXTURE.read_noting(holding(&[]), &notices, &mut out), None);
    assert_eq!(String::from_utf8_lossy(&out), "");
    // Nothing was said yet, so the old name's first use still is.
    assert!(notices.say(FIXTURE, &mut out));
    assert!(!notices.say(FIXTURE, &mut out));
}

#[test]
fn a_quiet_read_takes_the_same_value() {
    for pairs in [
        &[("SF_FIXTURE", "old")][..],
        &[("SYMPHONY_FIXTURE", "new"), ("SF_FIXTURE", "old")],
        &[],
    ] {
        assert_eq!(
            FIXTURE.quiet(holding(pairs)),
            FIXTURE.pick(holding(pairs)).map(|(value, _)| value)
        );
    }
}

#[test]
fn a_notice_that_cannot_be_written_is_not_tried_again() {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("closed"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let notices = Notices::default();
    assert!(notices.say(FIXTURE, &mut Broken));
    let mut out = Vec::new();
    assert!(!notices.say(FIXTURE, &mut out));
    assert_eq!(String::from_utf8_lossy(&out), "");
}

#[test]
fn the_process_environment_is_read_under_either_name() {
    let path = std::env::var_os("PATH").expect("a test runs with a PATH");
    assert_eq!(
        Var::new("PATH", "SF_NO_SUCH_VARIABLE").os(),
        Some(path.clone())
    );
    assert_eq!(
        Var::new("SYMPHONY_NO_SUCH_VARIABLE", "PATH").os(),
        Some(path.clone())
    );
    assert_eq!(
        Var::new("SYMPHONY_NO_SUCH_VARIABLE", "PATH").text(),
        path.into_string().ok()
    );
    let none = Var::new("SYMPHONY_NO_SUCH_VARIABLE", "SF_NO_SUCH_VARIABLE");
    assert_eq!(none.os(), None);
    assert_eq!(none.text(), None);
}

#[test]
fn every_new_name_is_the_old_one_under_symphonys_prefix() {
    let mut seen = BTreeSet::new();
    for var in ALL {
        assert!(seen.insert(var.name), "{} twice", var.name);
        assert!(seen.insert(var.old), "{} twice", var.old);
        let stem = var.old.strip_prefix("SF_").expect("an old name is SF_'s");
        // A doubled prefix is dropped; the hooks' plural folds into the one hook command name.
        let stem = stem.strip_prefix("SYMPHONY_").unwrap_or(stem);
        let stem = stem.replace("HOOKS_", "HOOK_");
        assert_eq!(var.name, format!("SYMPHONY_{stem}"), "{var:?}");
    }
    assert_eq!(seen.len(), 2 * ALL.len());
}

#[test]
fn either_name_of_a_variable_is_one_of_symphonys() {
    for var in ALL {
        assert!(named(var.name) && named(var.old), "{var:?}");
    }
    for other in [
        "",
        "SF_",
        "SYMPHONY_",
        "SYMPHONY_MACHINE_DIR",
        "SF_CONTENT_DIR",
        "PATH",
    ] {
        assert!(!named(other), "{other}");
    }
}

#[test]
fn both_names_are_listed_the_new_one_first() {
    assert_eq!(FIXTURE.names(), ["SYMPHONY_FIXTURE", "SF_FIXTURE"]);
    assert_eq!(
        HANDOVER.names(),
        ["SYMPHONY_HANDOVER", "SF_SYMPHONY_HANDOVER"]
    );
}
