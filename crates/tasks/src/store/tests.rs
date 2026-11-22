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
        for sessions in [Sessions::Whole, Sessions::Shown] {
            let (read, vars) = (reading(&which, sessions), super::reading::vars(&which));
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
}

/// A read's variables are the store's own, the same for each database: what it picks its tasks
/// by, the cap's only where it reads an orchestrator's slots, and always the executing statuses
/// and the presence bounds.
#[test]
fn a_reads_variables_are_what_it_picks_by_and_the_caps_only_where_slots_are_read() {
    let slugs = ["p/a".to_string(), "p/b".to_string()];
    let own = |which: Which| {
        let vars = super::reading::vars(&which);
        assert_eq!(vars["executing"], json!(EXECUTING), "{vars}");
        assert_eq!(vars["codex"], "codex:", "{vars}");
        assert_eq!(vars["codex_after"], "codex;", "{vars}");
        let shared: Vec<String> = ["executing", "codex", "codex_after"]
            .into_iter()
            .chain(cap::vars().as_object().unwrap().keys().map(String::as_str))
            .map(str::to_string)
            .collect();
        let mut own = vars.as_object().unwrap().clone();
        own.retain(|name, _| !shared.contains(name));
        Value::Object(own)
    };
    assert_eq!(own(Which::All), json!({}));
    assert_eq!(own(Which::SlotHoldings), json!({}));
    assert_eq!(own(Which::Status("ready")), json!({"status": "ready"}));
    assert_eq!(own(Which::Slug("p/a")), json!({"slug": "p/a"}));
    assert_eq!(own(Which::Starting("p/a")), json!({"slug": "p/a"}));
    assert_eq!(own(Which::Children("p/a")), json!({"slug": "p/a"}));
    assert_eq!(own(Which::Slugs(&slugs)), json!({"wanted": ["p/a", "p/b"]}));
    assert_eq!(own(Which::Slugs(&[])), json!({"wanted": []}));
    assert_eq!(own(Which::Linked("docs/x.md")), json!({"doc": "docs/x.md"}));
    assert_eq!(
        own(Which::Pullable("someone")),
        json!({"handle": "someone", "pullable": PULLABLE})
    );
    // The cap's variables ride only the reads that count an orchestrator's slots.
    let cap_vars = cap::vars();
    let caps: Vec<&String> = cap_vars.as_object().unwrap().keys().collect();
    assert!(!caps.is_empty());
    for (which, slots) in [
        (Which::Starting("p/a"), true),
        (Which::SlotHoldings, true),
        (Which::All, false),
        (Which::Slug("p/a"), false),
        (Which::Pullable("someone"), false),
    ] {
        let vars = super::reading::vars(&which);
        for name in &caps {
            assert_eq!(vars.get(name.as_str()).is_some(), slots, "{name}: {vars}");
            if slots {
                assert_eq!(vars[name.as_str()], cap_vars[name.as_str()], "{name}");
            }
        }
    }
}

/// The session table is read whole only where every session is asked for: a read of the sessions
/// its answer shows names each by its id, the holders of the roles it read and, for `next`, the
/// session asking.
#[test]
fn a_read_of_the_sessions_shown_reads_them_by_id_and_never_the_table() {
    const TABLE: &str = "FROM session ORDER BY";
    const BY_ID: &str =
        "FROM $shown_sessions.map(|$h| type::record('session', $h)) ORDER BY connected, handle";
    let slugs = ["p/a".to_string()];
    for which in [
        Which::All,
        Which::Status("ready"),
        Which::Slug("p/a"),
        Which::Slugs(&slugs),
        Which::Children("p/a"),
        Which::Linked("docs/x.md"),
        Which::Pullable("someone"),
    ] {
        let whole = reading(&which, Sessions::Whole);
        assert!(whole.contains(TABLE) && !whole.contains(BY_ID), "{whole}");
        let shown = reading(&which, Sessions::Shown);
        assert!(!shown.contains(TABLE) && shown.contains(BY_ID), "{shown}");
        let asker = "array::concat($view_roles.handle, [$handle])";
        assert_eq!(
            shown.contains(asker),
            matches!(which, Which::Pullable(_)),
            "{shown}"
        );
        assert!(
            shown.contains("LET $shown_sessions = array::distinct("),
            "{shown}"
        );
    }
}

/// Each try a moving board costs a call is counted on the call's thread, and on no other: what a
/// test reads to say a call never met a conflict. A refusal stops the call untried again, and one
/// out of patience gives up, saying how often it tried, without one more counted.
#[test]
fn a_call_tried_again_is_counted_on_its_own_thread_and_one_out_of_patience_gives_up() {
    let before = Store::tried_again_here();
    let mut tries = 0;
    let done = retried(PATIENCE, &mut || {
        tries += 1;
        if tries < 3 {
            Err(Failure::Again)
        } else {
            Ok(tries)
        }
    });
    assert_eq!(done, Ok(3));
    assert_eq!(Store::tried_again_here() - before, 2);
    let elsewhere = std::thread::spawn(Store::tried_again_here).join().unwrap();
    assert_eq!(elsewhere, 0, "another thread's count is its own");
    let before = Store::tried_again_here();
    let refused = retried::<()>(PATIENCE, &mut || {
        Err(Failure::Stop(Error::Refused("no task 'x'".into())))
    });
    assert_eq!(refused, Err(Error::Refused("no task 'x'".into())));
    let gave_up = retried::<()>(Duration::ZERO, &mut || Err(Failure::Again));
    assert_eq!(
        gave_up,
        Err(Error::Db(
            "gave up after 0 s and 1 tries: the board kept moving under the write".into()
        ))
    );
    assert_eq!(Store::tried_again_here(), before);
}

/// A call on a board that keeps moving under it goes on while its patience lasts, however many tries
/// that takes, never cut off by a count of them, resting the backoff's wait before each; once its
/// clock has passed its patience it gives up, naming its tries. Neither clock nor rest is waited on.
#[test]
fn a_call_goes_on_while_patience_lasts_however_many_tries_and_gives_up_once_it_ran_out() {
    let before = Store::tried_again_here();
    let rests = std::cell::RefCell::new(Vec::new());
    let mut tries = 0;
    let done = retried_on(
        PATIENCE,
        &|| Duration::ZERO,
        &|wait| rests.borrow_mut().push(wait),
        &mut || {
            tries += 1;
            if tries <= 10_000 {
                Err(Failure::Again)
            } else {
                Ok(tries)
            }
        },
    );
    assert_eq!(done, Ok(10_001));
    assert_eq!(Store::tried_again_here() - before, 10_000);
    let rests = rests.into_inner();
    assert_eq!(rests.len(), 10_000, "a rest before each try again");
    assert!(
        rests.iter().all(|wait| *wait < Duration::from_millis(64)),
        "each rest within the backoff's ceiling"
    );

    let clock = std::cell::Cell::new(Duration::ZERO);
    let mut tries = 0;
    let gave_up = retried_on::<()>(
        PATIENCE,
        &|| clock.get(),
        &|_| clock.set(clock.get() + Duration::from_secs(1)),
        &mut || {
            tries += 1;
            Err(Failure::Again)
        },
    );
    assert_eq!(
        gave_up,
        Err(Error::Db(
            "gave up after 30 s and 31 tries: the board kept moving under the write".into()
        ))
    );
    assert_eq!(
        tries, 31,
        "tried while its clock was short of 30 s, and once more at 30"
    );
}

#[test]
fn a_refusal_is_its_words_and_a_database_failure_says_so() {
    assert_eq!(
        Error::Refused("no task 'x'".into()).to_string(),
        "no task 'x'"
    );
    assert_eq!(
        Error::Db("cannot reach it".into()).to_string(),
        "the board's database: cannot reach it"
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

#[test]
fn retrying_keeps_callback_capture_and_result_drop_order_through_success_refusal_and_panic() {
    use std::sync::{Arc, Mutex};
    struct Dropped(&'static str, Arc<Mutex<Vec<&'static str>>>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.1.lock().unwrap().push(self.0);
        }
    }
    let store = Store::unchecked(
        crate::rpc::Client::new("127.0.0.1:1", "sf", "unused"),
        Box::new(|| 0.0),
        Box::new(|_| false),
        "fixture-host",
    );
    for case in ["success", "refusal", "panic"] {
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = Dropped("capture dropped", Arc::clone(&events));
        let called = Arc::clone(&events);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            store.retrying(move || {
                let _keep = &captured;
                called.lock().unwrap().push("called");
                match case {
                    "refusal" => Err(Failure::Stop(Error::Refused("exact refusal 猫".into()))),
                    "panic" => std::panic::panic_any("exact callback panic"),
                    _ => Ok(Dropped("result dropped", Arc::clone(&called))),
                }
            })
        }));
        assert_eq!(
            *events.lock().unwrap(),
            ["called", "capture dropped"],
            "{case}"
        );
        match (case, result) {
            ("success", Ok(Ok(value))) => {
                drop(value);
                assert_eq!(
                    *events.lock().unwrap(),
                    ["called", "capture dropped", "result dropped"]
                );
            }
            ("refusal", Ok(Err(error))) => {
                assert_eq!(error, Error::Refused("exact refusal 猫".into()))
            }
            ("panic", Err(payload)) => assert_eq!(
                payload.downcast_ref::<&str>(),
                Some(&"exact callback panic")
            ),
            _ => panic!("unexpected result for {case}"),
        }
    }
}
