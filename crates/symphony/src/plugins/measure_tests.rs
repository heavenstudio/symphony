use symphony_plugin::vocabulary::GateState;

use super::*;
use crate::test_tree::Tree;

/// The record at `path`, each line read as JSON.
fn lines(path: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn new_meter(path: &Path, loopback: bool) -> Meter {
    Meter {
        plugin: "fixture",
        loopback,
        record: Record::at(path),
        said: Mutex::new(BTreeSet::new()),
    }
}

fn validate(value: FieldValue) -> Validate {
    Validate::new("effort".into(), value)
}

#[test]
fn a_duration_is_written_in_microseconds_to_the_nanosecond() {
    assert!((micros(Duration::from_nanos(1_234)) - 1.234).abs() < 1e-9);
    assert!(micros(Duration::ZERO).abs() < f64::EPSILON);
    assert!((micros(Duration::from_secs(2)) - 2e6).abs() < 1e-6);
    assert!((micros(Duration::from_nanos(999)) - 0.999).abs() < 1e-9);
}

#[test]
fn a_call_that_survives_json_answers_what_its_far_side_answered_and_costs_both_ways() {
    let trip = round_trip(
        || panic!("the linked call is not made when the request reads back"),
        || validate(FieldValue::Text(" 3 ".into())),
        |asked: Validate| {
            assert_eq!(asked.value, FieldValue::Text(" 3 ".into()));
            Ok(FieldValue::Number(3.0))
        },
    );
    assert_eq!(trip.answer, Ok(FieldValue::Number(3.0)));
    assert_eq!(trip.lost, None);
    let request: Request = validate(FieldValue::Text(" 3 ".into())).into();
    assert_eq!(
        trip.request_bytes,
        Some(serde_json::to_string(&request).unwrap().len())
    );
    // The answer goes as the wire carries it: an answer, not a refusal.
    assert_eq!(trip.answer_bytes, Some(r#"{"answer":{"Ok":3.0}}"#.len()));
    assert!(trip.round_trip.unwrap() >= trip.call);
}

#[test]
fn what_did_not_survive_is_said_after_what_already_had() {
    let mut lost = None;
    lose(&mut lost, "the request read back otherwise".into());
    assert_eq!(lost.as_deref(), Some("the request read back otherwise"));
    lose(&mut lost, "the answer: no".into());
    assert_eq!(
        lost.as_deref(),
        Some("the request read back otherwise; the answer: no")
    );
}

#[test]
fn an_answer_that_does_not_read_back_is_named_and_the_plugins_own_is_kept() {
    let trip = round_trip(
        || panic!("the request reads back"),
        || validate(FieldValue::Text("x".into())),
        |_: Validate| Ok(FieldValue::Number(f64::NAN)),
    );
    let lost = trip.lost.unwrap();
    assert!(lost.starts_with("the answer: "), "{lost}");
    assert!(matches!(trip.answer, Ok(FieldValue::Number(n)) if n.is_nan()));
    assert!(trip.round_trip.is_some());
}

#[test]
fn an_answer_that_reads_back_otherwise_is_named_and_what_came_back_is_the_answer() {
    let at = |at| Gate {
        name: "certificate".into(),
        state: GateState::default(),
        at,
        ..Gate::default()
    };
    let trip = round_trip(
        || panic!("the request reads back"),
        || {
            let task = TaskRef {
                slug: "a/b",
                status: "review",
                branch: "a/b",
            };
            AnswerGates::new("stamps".into(), "/r".into(), TaskData::of(&task), None)
        },
        |_: AnswerGates| Ok(vec![at(Some(f64::NAN))]),
    );
    assert_eq!(trip.lost.as_deref(), Some("the answer read back otherwise"));
    assert_eq!(trip.answer, Ok(vec![at(None)]));
}

#[test]
fn a_request_that_cannot_be_written_is_named_and_the_call_is_made_linked() {
    use std::os::unix::ffi::OsStrExt as _;
    let trip = round_trip(
        || Some(Reply::Json("linked".into())),
        || {
            Serve::new(
                "widgets".into(),
                PathBuf::from(std::ffi::OsStr::from_bytes(b"/r/\xff")),
                String::new(),
            )
        },
        |_: Serve| panic!("the far side is not asked a request that was never sent"),
    );
    assert!(
        trip.lost.as_deref().unwrap().starts_with("the request: "),
        "{:?}",
        trip.lost
    );
    assert_eq!(trip.answer, Some(Reply::Json("linked".into())));
    assert_eq!(
        (trip.round_trip, trip.request_bytes, trip.answer_bytes),
        (None, None, None)
    );
}

#[test]
fn the_times_hold_the_calls_own_time_and_the_round_trip_the_jsons_besides() {
    const SLEPT: Duration = Duration::from_millis(40);
    let tree = Tree::new();
    let path = tree.path("artifacts/moves.jsonl");
    let timed = new_meter(&path, false);
    #[expect(
        clippy::disallowed_methods,
        reason = "no-timed-waits baseline: a meter's test of a call made slow by a length of time"
    )]
    timed.time::<Validate, _>("slow", || std::thread::sleep(SLEPT));
    let looped = new_meter(&path, true);
    let slow_answer = || {
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: a meter's test of a call made slow by a length of time"
        )]
        std::thread::sleep(SLEPT);
        Ok(FieldValue::Text("x".into()))
    };
    let slow = looped.call(
        "slow",
        || panic!("the request reads back"),
        || {
            #[expect(clippy::disallowed_methods, reason = "no-timed-waits baseline: a meter's test of a call made slow by a length of time")]
            std::thread::sleep(SLEPT);
            validate(FieldValue::Text("x".into()))
        },
        |_| slow_answer(),
    );
    assert_eq!(slow, Ok(FieldValue::Text("x".into())));
    let big = "y".repeat(1_000_000);
    let big_answer = looped.call(
        "big",
        || panic!("the request reads back"),
        || validate(FieldValue::Text(big.clone())),
        |_| Ok(FieldValue::Text(big.clone())),
    );
    assert_eq!(big_answer, Ok(FieldValue::Text(big)));
    let lines = lines(&path);
    let us = |n: usize| lines[n]["us"].as_f64().unwrap();
    let slept = micros(SLEPT);
    // Times on the wall are bounded from below alone: a loaded machine only adds to them.
    assert!(us(0) >= slept, "{}", lines[0]);
    assert!(us(1) >= slept, "{}", lines[1]);
    // The round trip holds the request's making and its JSON besides the call's own time.
    let json = |n: usize| lines[n]["round_trip_us"].as_f64().unwrap() - us(n);
    assert!(json(1) >= slept, "{}", lines[1]);
    // A million bytes each way are counted.
    assert!(lines[2]["request_bytes"].as_u64().unwrap() > 1_000_000);
    assert!(lines[2]["answer_bytes"].as_u64().unwrap() > 1_000_000);
}

#[test]
fn a_timed_call_writes_its_seam_call_name_and_time_and_nothing_of_a_round_trip() {
    let tree = Tree::new();
    let path = tree.path("artifacts/plugin-calls.jsonl");
    let meter = new_meter(&path, false);
    let answer = meter.call(
        "effort",
        || Ok(FieldValue::Number(1.0)),
        || panic!("no request is built for a linked call"),
        |_: Validate| panic!("no far side for a linked call"),
    );
    assert_eq!(answer, Ok(FieldValue::Number(1.0)));
    let lines = lines(&path);
    assert_eq!(lines.len(), 1);
    let line = lines[0].as_object().unwrap();
    let mut keys: Vec<&str> = line.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "at_ms", "call", "name", "path", "pid", "plugin", "seam", "us"
        ]
    );
    assert_eq!(line["path"], "linked");
    assert_eq!(line["plugin"], "fixture");
    assert_eq!(line["seam"], "board_fields");
    assert_eq!(line["call"], "validate");
    assert_eq!(line["name"], "effort");
    assert_eq!(line["pid"], std::process::id());
    assert!(line["us"].as_f64().unwrap() >= 0.0);
    assert!(line["at_ms"].as_u64().unwrap() > 1_700_000_000_000);
}

#[test]
fn a_looped_call_writes_both_times_its_bytes_and_what_it_lost() {
    let tree = Tree::new();
    let path = tree.path("out/calls.jsonl");
    let meter = new_meter(&path, true);
    let ask = || validate(FieldValue::Text("2".into()));
    let kept = meter.call(
        "effort",
        || panic!("the request reads back"),
        ask,
        |_| Ok(FieldValue::Number(2.0)),
    );
    assert_eq!(kept, Ok(FieldValue::Number(2.0)));
    let lost = meter.call(
        "effort",
        || panic!("the request reads back"),
        ask,
        |_| Ok(FieldValue::Number(f64::INFINITY)),
    );
    assert!(matches!(lost, Ok(FieldValue::Number(n)) if n.is_infinite()));
    let lines = lines(&path);
    assert_eq!(lines.len(), 2);
    for line in &lines {
        assert_eq!(line["path"], "loopback");
        assert!(line.get("failed").is_none());
        assert!(line["round_trip_us"].as_f64().unwrap() >= line["us"].as_f64().unwrap());
        assert!(line["request_bytes"].as_u64().unwrap() > 0);
        assert!(line["answer_bytes"].as_u64().unwrap() > 0);
    }
    assert!(lines[0].get("lost").is_none());
    assert!(
        lines[1]["lost"]
            .as_str()
            .unwrap()
            .starts_with("the answer: "),
        "{}",
        lines[1]
    );
    // Stderr names a call's loss once; the record names every one.
    assert!(meter.said.lock().unwrap().contains("board_fields.validate"));
}

/// One timed line written into `record`.
fn timed_into(record: &'static Record) {
    let meter = Meter {
        plugin: "fixture",
        loopback: false,
        record,
        said: Mutex::new(BTreeSet::new()),
    };
    meter.time::<Validate, _>("effort", || ());
}

#[test]
fn a_record_past_its_cap_is_rolled_to_its_one_and_begun_again() {
    let tree = Tree::new();
    let path = tree.path("artifacts/plugin-calls.jsonl");
    let roll = tree.path("artifacts/plugin-calls.1.jsonl");
    assert_eq!(rolled(&path), roll);
    assert_eq!(rolled(Path::new("/a/calls")), Path::new("/a/calls.1"));
    // A cap of two lines and a half: three fill it, and the fourth begins the next.
    let sized = tree.path("sized.jsonl");
    timed_into(Box::leak(Box::new(Record::new(&sized, CAP))));
    let cap = std::fs::metadata(&sized).unwrap().len() * 5 / 2;
    let record: &'static Record = Box::leak(Box::new(Record::new(&path, cap)));
    for _ in 0..3 {
        timed_into(record);
    }
    assert_eq!(lines(&path).len(), 3);
    assert!(!roll.exists());
    timed_into(record);
    assert_eq!((lines(&roll).len(), lines(&path).len()), (3, 1));
    for _ in 0..3 {
        timed_into(record);
    }
    // The roll before is replaced: the record and its roll hold twice the cap at most.
    assert_eq!((lines(&roll).len(), lines(&path).len()), (3, 1));
    assert!(std::fs::metadata(&path).unwrap().len() < cap);
    assert_eq!(CAP, 32 * 1024 * 1024);
}

#[test]
fn a_record_removed_and_made_again_by_another_is_the_one_written_to() {
    let tree = Tree::new();
    let path = tree.path("artifacts/plugin-calls.jsonl");
    let record: &'static Record = Box::leak(Box::new(Record::new(&path, CAP)));
    timed_into(record);
    timed_into(record);
    assert_eq!(lines(&path).len(), 2);
    // Another process removes it and begins its own: this one's next line goes there, where the
    // path names, not to the file it held.
    std::fs::remove_file(&path).unwrap();
    std::fs::write(&path, "").unwrap();
    timed_into(record);
    assert_eq!(lines(&path).len(), 1);
    // And one moved aside (as a roll by another process does) leaves the path to a new file.
    std::fs::rename(&path, tree.path("aside.jsonl")).unwrap();
    timed_into(record);
    assert_eq!(lines(&path).len(), 1);
    assert_eq!(lines(&tree.path("aside.jsonl")).len(), 1);
}

#[test]
fn a_record_removed_starts_over_and_one_that_cannot_be_written_is_said_once() {
    let tree = Tree::new();
    let path = tree.path("artifacts/again.jsonl");
    let meter = new_meter(&path, false);
    let call = || {
        meter.time::<Validate, _>("effort", || ());
    };
    call();
    call();
    assert_eq!(lines(&path).len(), 2);
    std::fs::remove_file(&path).unwrap();
    call();
    assert_eq!(lines(&path).len(), 1);
    // A record under a file, not a folder, cannot be opened.
    tree.write("blocked", "");
    let blocked = new_meter(&tree.path("blocked/calls.jsonl"), false);
    blocked.time::<Validate, _>("effort", || ());
    blocked.time::<Validate, _>("effort", || ());
    assert!(blocked.record.warned.load(Ordering::SeqCst));
    assert!(!tree.path("blocked/calls.jsonl").exists());
}

#[test]
fn a_record_is_one_per_path_and_a_wrapper_one_per_plugin_record_and_setting() {
    struct Bare;
    impl Plugin for Bare {
        fn name(&self) -> &str {
            "bare"
        }
    }
    static BARE: Bare = Bare;
    let tree = Tree::new();
    let (one, two) = (tree.path("one.jsonl"), tree.path("two.jsonl"));
    assert!(std::ptr::eq(Record::at(&one), Record::at(&one)));
    assert!(!std::ptr::eq(Record::at(&one), Record::at(&two)));
    let timed = measured(&BARE, &one, false);
    assert!(std::ptr::eq(timed, measured(&BARE, &one, false)));
    assert!(!std::ptr::eq(timed, measured(&BARE, &two, false)));
    let looped = measured(&BARE, &one, true);
    assert!(!std::ptr::eq(timed, looped));
    assert_eq!(looped.name(), "bare");
    // The loopback sends the plugin's declaration once, when its wrapper is made.
    let declared: Vec<_> = lines(&one)
        .into_iter()
        .filter(|line| line["seam"] == "plugin")
        .collect();
    assert_eq!(declared.len(), 1, "{declared:?}");
    assert_eq!(declared[0]["call"], "declare");
    assert_eq!(declared[0]["name"], "bare");
    assert!(declared[0].get("lost").is_none());
}

#[test]
fn an_authority_measured_timed_or_looped_answers_as_linked_and_each_call_is_recorded() {
    use symphony_plugin::authority::{Operation, Signed, Snapshot};
    use symphony_plugin::execution::{Principal, PrincipalKind};
    struct Ledger;
    impl Authority for Ledger {
        fn adapter(&self) -> &str {
            "approvals"
        }
        fn attest(
            &self,
            root: &Path,
            operation: &Operation,
            within: Duration,
        ) -> Result<Signed, String> {
            Ok(Signed::new(
                format!(
                    "{} in {} within {} ms",
                    operation.guard,
                    root.display(),
                    within.as_millis()
                ),
                "ab".repeat(64),
            ))
        }
    }
    struct Ledgered;
    impl Plugin for Ledgered {
        fn name(&self) -> &str {
            "ledgered"
        }
        fn authorities(&self) -> Vec<&dyn Authority> {
            vec![&Ledger]
        }
    }
    static LEDGERED: Ledgered = Ledgered;
    let operation = Operation::new(
        "sim".into(),
        "sim/a".into(),
        "accept".into(),
        "approved".into(),
        Snapshot::new("d1".into(), 2, 3, "verify".into(), "awaiting".into()),
        std::collections::BTreeMap::new(),
        Principal::new(PrincipalKind::Human, "local".into(), "owner".into()),
    );
    let (root, within) = (Path::new("/project"), Duration::from_millis(2500));
    let linked = Ledger.attest(root, &operation, within);
    assert_eq!(
        linked,
        Ok(Signed::new(
            "approved in /project within 2500 ms".into(),
            "ab".repeat(64)
        ))
    );
    let tree = Tree::new();
    for (file, loopback) in [("timed.jsonl", false), ("looped.jsonl", true)] {
        let path = tree.path(file);
        let wrapped = measured(&LEDGERED, &path, loopback);
        let authorities = wrapped.authorities();
        let [authority] = authorities.as_slice() else {
            panic!("{} authorities", authorities.len());
        };
        assert_eq!(authority.adapter(), "approvals");
        assert_eq!(authority.attest(root, &operation, within), linked, "{file}");
        let attests: Vec<_> = lines(&path)
            .into_iter()
            .filter(|line| line["seam"] == "authorities")
            .collect();
        assert_eq!(attests.len(), 1, "{file}: {attests:?}");
        assert_eq!(
            (&attests[0]["call"], &attests[0]["name"]),
            (&"attest".into(), &"approvals".into()),
            "{file}"
        );
        assert_eq!(attests[0]["path"] == "loopback", loopback, "{file}");
        assert!(attests[0].get("lost").is_none(), "{file}");
    }
}
