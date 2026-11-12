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

/// Each snapshot a page or a list reads is a read, so a late answer to it goes once more: its
/// bounds are variables, so no `;` stands in it but between its statements.
#[test]
fn every_snapshot_read_is_a_read_and_binds_its_bounds() {
    let slugs = ["p/a".to_string()];
    for which in [
        Which::All,
        Which::SlotHoldings,
        Which::Status("ready"),
        Which::Slug("p/a"),
        Which::Starting("p/a"),
        Which::Slugs(&slugs),
        Which::Children("p/a"),
        Which::Linked("docs/x.md"),
        Which::Pullable("someone"),
    ] {
        let (read, vars) = Store::reading(&which);
        assert_eq!(
            crate::rpc::Kind::of(&read),
            crate::rpc::Kind::Read,
            "{read}"
        );
        assert_eq!(
            (vars["codex"].as_str(), vars["codex_after"].as_str()),
            (Some("codex:"), Some("codex;")),
            "{vars}"
        );
    }
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

#[test]
fn the_command_back_to_a_board_names_its_recipe_its_database_and_a_named_server() {
    for (server, database, want) in [
        (None, crate::DATABASE, "just tasks"),
        (None, "scratch1", "just tasks --database scratch1"),
        (
            Some("127.0.0.1:4321"),
            "scratch1",
            "TASKS_SERVER=127.0.0.1:4321 just tasks --database scratch1",
        ),
        (
            Some("127.0.0.1:4321"),
            crate::DATABASE,
            "TASKS_SERVER=127.0.0.1:4321 just tasks",
        ),
    ] {
        assert_eq!(board_command(server, database), want);
    }
}

/// A task's row as a reader that keeps a number's text holds it, its floats past their shortest
/// form: it reads beside the record's version in every build, handed only the fields it names (a
/// field the record holds beside them, a computed one say, is not its to refuse), and a record
/// without its version is refused.
#[test]
fn a_task_record_reads_its_row_beside_its_version_however_its_times_are_written() {
    let text = r#"{"sid":7,"slug":"sim/jump","title":"Jump","body":"","status":"ready",
        "priority":1,"created_by":"owner","created":1762912345.250,"updated":1.76291234525e9,
        "instruction":false,"fields":{"game":{"effort":3.0,"share":0.250}},"version":4}"#;
    let fields: BTreeMap<String, Value> =
        serde_json::from_str(r#"{"effort":3.0,"share":0.250}"#).unwrap();
    let want = TaskRow {
        id: 7,
        slug: "sim/jump".into(),
        title: "Jump".into(),
        status: "ready".into(),
        priority: 1,
        created_by: "owner".into(),
        created: 1_762_912_345.25,
        updated: 1_762_912_345.25,
        fields: BTreeMap::from([("game".into(), fields)]),
        ..TaskRow::default()
    };
    let value: Value = serde_json::from_str(text).unwrap();
    for record in [
        serde_json::from_str::<TaskRecord>(text).unwrap(),
        serde_json::from_value::<TaskRecord>(value).unwrap(),
    ] {
        assert_eq!(record.version, 4);
        assert_eq!(record.row, want);
    }
    let beside = text.replace(r#""version":4"#, r#""version":4,"stall":null,"extra":1.50"#);
    assert_eq!(
        serde_json::from_str::<TaskRecord>(&beside).unwrap().row,
        want
    );
    let unversioned = text.replace(r#","version":4"#, "");
    let error = serde_json::from_str::<TaskRecord>(&unversioned)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("missing field `version`"), "{error}");
}
