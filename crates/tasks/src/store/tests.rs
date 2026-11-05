//! Store value helpers: native presence, quoting and error messages.

use super::*;

#[test]
fn an_expired_native_signal_never_overrules_a_heartbeat_or_an_explicit_departure() {
    let row = SessionRow::default();
    for (health, until, want) in [
        (Health::Live, Some(0.0), Health::Live),
        (Health::Lost, Some(100.0), Health::Lost),
        (Health::Stale, Some(99.0), Health::Unknown),
        (Health::Lost, Some(99.0), Health::Unknown),
        (Health::Left, Some(99.0), Health::Unknown),
        (Health::Lost, None, Health::Lost),
    ] {
        assert_eq!(native_health(&row, health, 100.0, until), want);
    }
    let left = SessionRow {
        left_at: Some(1.0),
        ..row
    };
    assert_eq!(
        native_health(&left, Health::Left, 100.0, Some(0.0)),
        Health::Left
    );
    assert!(Health::Unknown.holds());
    assert_eq!(Health::Unknown.as_str(), "unknown");
}

#[test]
fn native_uncertainty_expires_without_overruling_the_heartbeat_health() {
    let row = SessionRow::default();
    for health in [Health::Live, Health::Stale, Health::Lost, Health::Left] {
        let protected = if health == Health::Live {
            health
        } else {
            Health::Unknown
        };
        for (now, expected) in [(699.0, protected), (700.0, protected), (701.0, health)] {
            assert_eq!(native_health(&row, health, now, Some(100.0)), expected);
        }
    }
}

#[test]
fn a_name_is_quoted_as_python_quotes_it() {
    assert_eq!(repr("nobody"), "'nobody'");
    assert_eq!(repr(""), "''");
    assert_eq!(repr("it's"), "\"it's\"");
    assert_eq!(repr("it's \"x\""), "'it\\'s \"x\"'");
    assert_eq!(repr("a\\b\nc\td\r\u{1}"), "'a\\\\b\\nc\\td\\r\\x01'");
    assert_eq!(repr("sim/jump — ✓"), "'sim/jump — ✓'");
}

#[test]
fn a_refusal_is_its_words_and_a_database_failure_says_so() {
    assert_eq!(
        Error::Refused("no task 'x'".into()).to_string(),
        "no task 'x'"
    );
    assert_eq!(
        Error::Db("cannot reach it".into()).to_string(),
        "SurrealDB: cannot reach it"
    );
}
