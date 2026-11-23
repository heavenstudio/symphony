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

/// A read's variables are the store's own: what it picks its tasks by, the cap's only where it
/// reads an orchestrator's slots, and always the executing statuses and the presence bounds.
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
        (Which::Status("ready"), false),
        (Which::Slug("p/a"), false),
        (Which::Slugs(&slugs), false),
        (Which::Children("p/a"), false),
        (Which::Linked("docs/x.md"), false),
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
    let store = Store::unchecked_on(
        crate::pg::Client::new(std::path::Path::new("/nonexistent"), "unused").into(),
        Box::new(|| 0.0),
        Box::new(|_| false),
        "fixture-host",
        &Opening::write(),
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

#[test]
fn a_reservation_expires_only_once_its_time_is_past_and_one_with_none_never_does() {
    assert!(!expired(None, 1e12));
    assert!(!expired(Some(100.0), 99.0));
    assert!(!expired(Some(100.0), 100.0));
    assert!(expired(Some(100.0), 100.5));
}

#[test]
fn a_reservation_lapses_past_its_time_or_with_its_holder_gone_fenced_only_by_a_session_read_gone() {
    let (live, time, past) = (Some(200.0), 100.0, 300.0);
    // Kept: in its time, or with no expiry, while its holder may still act.
    for health in [Health::Live, Health::Stale, Health::Unknown] {
        assert_eq!(
            lapsed(live, Some(health), time),
            (false, false),
            "{health:?}"
        );
        assert_eq!(
            lapsed(None, Some(health), time),
            (false, false),
            "{health:?}"
        );
        // Past its time it lapses on the clock alone: no beat fences that.
        assert_eq!(
            lapsed(live, Some(health), past),
            (true, false),
            "{health:?}"
        );
    }
    // A holder lost or left lets it go before its time, fenced at the beat it was read at, and
    // after its time on the clock alone.
    for health in [Health::Lost, Health::Left] {
        assert_eq!(lapsed(live, Some(health), time), (true, true), "{health:?}");
        assert_eq!(lapsed(None, Some(health), time), (true, true), "{health:?}");
        assert_eq!(
            lapsed(live, Some(health), past),
            (true, false),
            "{health:?}"
        );
    }
    // No session holds the handle: dropped, with no beat to fence.
    assert_eq!(lapsed(live, None, time), (true, false));
    assert_eq!(lapsed(None, None, time), (true, false));
}

#[test]
fn a_start_counts_against_its_executors_and_only_with_none_against_its_mover() {
    let handles = |names: &[&str]| names.iter().map(|n| (*n).to_string()).collect::<Vec<_>>();
    assert_eq!(
        executors_or(handles(&["amber-heron", "teal-owl"]), Some("mover")),
        handles(&["amber-heron", "teal-owl"])
    );
    assert_eq!(
        executors_or(handles(&["amber-heron"]), None),
        handles(&["amber-heron"])
    );
    assert_eq!(executors_or(Vec::new(), Some("mover")), handles(&["mover"]));
    assert_eq!(executors_or(Vec::new(), None), Vec::<String>::new());
}

#[test]
fn a_rules_write_is_each_part_its_change_holds_and_a_move_says_whether_it_cancels() {
    assert_eq!(Change::default().parts(&[], None), RuleParts::default());
    let some = || Some(json!({}));
    let change = Change {
        reason: Some("why".into()),
        needs: some(),
        gates: some(),
        blocked: some(),
        unlanded: some(),
        status: Some("cancelled".into()),
        ..Change::default()
    };
    let placing = workflow::Transition {
        id: workflow::Transition::id_of("sim/a", 2),
        slug: "sim/a".into(),
        revision: 2,
        generation: 1,
        digest: "d1".into(),
        edge: None,
        operation: None,
        activation: None,
        from: None,
        to: symphony_api::workflow::Pair {
            phase: "build".into(),
            state: "working".into(),
        },
        resume: None,
        status: "in_progress".into(),
        principal: symphony_plugin::execution::Principal::new(
            symphony_plugin::execution::PrincipalKind::Human,
            "local".into(),
            "owner".into(),
        ),
        effects: vec![],
        key: None,
        recovery: None,
        accepted: None,
        at: 1.0,
    };
    assert_eq!(
        change.parts(&[json!({"slot": 1})], Some(&placing)),
        RuleParts {
            placed: true,
            reason: true,
            blocked: true,
            needs: true,
            gates: true,
            unlanded: true,
            moved: Some(true),
            holds: true,
        }
    );
    // Each field alone sets its own part and no other.
    let only = |change: Change, part: RuleParts| assert_eq!(change.parts(&[], None), part);
    let none = RuleParts::default();
    only(
        Change {
            reason: Some("why".into()),
            ..Change::default()
        },
        RuleParts {
            reason: true,
            ..none
        },
    );
    only(
        Change {
            needs: some(),
            ..Change::default()
        },
        RuleParts {
            needs: true,
            ..none
        },
    );
    only(
        Change {
            gates: some(),
            ..Change::default()
        },
        RuleParts {
            gates: true,
            ..none
        },
    );
    only(
        Change {
            blocked: some(),
            ..Change::default()
        },
        RuleParts {
            blocked: true,
            ..none
        },
    );
    // A mark lifted (null) is written as one set.
    only(
        Change {
            unlanded: Some(Value::Null),
            ..Change::default()
        },
        RuleParts {
            unlanded: true,
            ..none
        },
    );
    only(
        Change {
            status: Some("ready".into()),
            ..Change::default()
        },
        RuleParts {
            moved: Some(false),
            ..none
        },
    );
}
