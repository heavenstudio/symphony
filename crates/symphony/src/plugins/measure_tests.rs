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

fn new_meter(path: &Path) -> Meter {
    Meter {
        plugin: "fixture",
        record: Record::at(path),
    }
}

/// The lines of the record at `path` whose seam is `seam`.
fn of_seam(path: &Path, seam: &str) -> Vec<serde_json::Value> {
    lines(path)
        .into_iter()
        .filter(|line| line["seam"] == seam)
        .collect()
}

#[test]
fn a_duration_is_written_in_microseconds_to_the_nanosecond() {
    assert!((micros(Duration::from_nanos(1_234)) - 1.234).abs() < 1e-9);
    assert!(micros(Duration::ZERO).abs() < f64::EPSILON);
    assert!((micros(Duration::from_secs(2)) - 2e6).abs() < 1e-6);
    assert!((micros(Duration::from_nanos(999)) - 0.999).abs() < 1e-9);
}

#[test]
fn a_timed_call_holds_the_calls_own_time() {
    const SLEPT: Duration = Duration::from_millis(40);
    let tree = Tree::new();
    let path = tree.path("artifacts/moves.jsonl");
    let timed = new_meter(&path);
    #[expect(
        clippy::disallowed_methods,
        reason = "no-timed-waits baseline: a meter's test of a call made slow by a length of time"
    )]
    timed.time::<Validate, _>("slow", || std::thread::sleep(SLEPT));
    let lines = lines(&path);
    // Times on the wall are bounded from below alone: a loaded machine only adds to them.
    assert!(
        lines[0]["us"].as_f64().unwrap() >= micros(SLEPT),
        "{}",
        lines[0]
    );
}

#[test]
fn a_timed_call_writes_its_seam_call_name_and_time_and_nothing_of_a_round_trip() {
    let tree = Tree::new();
    let path = tree.path("artifacts/plugin-calls.jsonl");
    let meter = new_meter(&path);
    let answer = meter.time::<Validate, _>("effort", || Ok::<_, String>(FieldValue::Number(1.0)));
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

/// One timed line written into `record`.
fn timed_into(record: &'static Record) {
    let meter = Meter {
        plugin: "fixture",
        record,
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
    let meter = new_meter(&path);
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
    let blocked = new_meter(&tree.path("blocked/calls.jsonl"));
    blocked.time::<Validate, _>("effort", || ());
    blocked.time::<Validate, _>("effort", || ());
    assert!(blocked.record.warned.load(Ordering::SeqCst));
    assert!(!tree.path("blocked/calls.jsonl").exists());
}

#[test]
fn a_record_is_one_per_path_and_a_wrapper_one_per_plugin_and_record() {
    struct Bare;
    impl Plugin for Bare {
        fn name(&self) -> &str {
            "bare"
        }
    }
    static BARE: Bare = Bare;
    static OTHER: Bare = Bare;
    let tree = Tree::new();
    let (one, two) = (tree.path("one.jsonl"), tree.path("two.jsonl"));
    assert!(std::ptr::eq(Record::at(&one), Record::at(&one)));
    assert!(!std::ptr::eq(Record::at(&one), Record::at(&two)));
    let timed = measured(&BARE, &one);
    assert!(std::ptr::eq(timed, measured(&BARE, &one)));
    assert!(!std::ptr::eq(timed, measured(&BARE, &two)));
    assert!(!std::ptr::eq(timed, measured(&OTHER, &one)));
    assert_eq!(timed.name(), "bare");
    // Making a wrapper is no call: what a plugin declares is read from it, untimed.
    assert!(lines(&one).is_empty());
}

#[test]
fn an_authority_measured_answers_as_linked_and_its_call_is_recorded() {
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
    let path = tree.path("timed.jsonl");
    let wrapped = measured(&LEDGERED, &path);
    let authorities = wrapped.authorities();
    let [authority] = authorities.as_slice() else {
        panic!("{} authorities", authorities.len());
    };
    assert_eq!(authority.adapter(), "approvals");
    assert_eq!(authority.attest(root, &operation, within), linked);
    let attests = of_seam(&path, "authorities");
    assert_eq!(attests.len(), 1, "{attests:?}");
    assert_eq!(
        (
            &attests[0]["call"],
            &attests[0]["name"],
            &attests[0]["path"]
        ),
        (&"attest".into(), &"approvals".into(), &"linked".into())
    );
}

#[test]
fn a_command_measured_writes_to_the_streams_it_is_handed_and_exits_as_its_own() {
    struct Echo;
    impl Command for Echo {
        fn name(&self) -> &str {
            "echo"
        }
        fn summary(&self) -> &str {
            "says its arguments"
        }
        fn run(
            &self,
            args: &[String],
            out: &mut dyn std::io::Write,
            err: &mut dyn std::io::Write,
        ) -> i32 {
            let _ = out.write_all(args.join(" ").as_bytes());
            let _ = err.write_all(format!("{} said", args.len()).as_bytes());
            3
        }
    }
    struct Echoing;
    impl Plugin for Echoing {
        fn name(&self) -> &str {
            "echoing"
        }
        fn commands(&self) -> Vec<&dyn Command> {
            vec![&Echo]
        }
    }
    static ECHOING: Echoing = Echoing;
    let tree = Tree::new();
    let path = tree.path("commands.jsonl");
    let commands = measured(&ECHOING, &path).commands();
    let [command] = commands.as_slice() else {
        panic!("{} commands", commands.len());
    };
    assert_eq!(
        (command.name(), command.summary()),
        ("echo", "says its arguments")
    );
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args = ["a".to_string(), "b".to_string()];
    assert_eq!(command.run(&args, &mut out, &mut err), 3);
    assert_eq!(
        (out.as_slice(), err.as_slice()),
        (&b"a b"[..], &b"2 said"[..])
    );
    let runs = of_seam(&path, "commands");
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(
        (&runs[0]["call"], &runs[0]["name"]),
        (&"run".into(), &"echo".into())
    );
}

#[test]
fn a_watch_is_named_by_its_paths_and_each_of_its_calls_is_recorded() {
    struct Paths;
    impl Watch for Paths {
        fn paths(&self) -> Vec<String> {
            vec!["content/".into(), "data/ships.toml".into()]
        }
        fn changed(&self, root: &Path) -> Option<String> {
            Some(root.display().to_string())
        }
    }
    struct Watching;
    impl Plugin for Watching {
        fn name(&self) -> &str {
            "watching"
        }
        fn watches(&self) -> Vec<&dyn Watch> {
            vec![&Paths]
        }
    }
    static WATCHING: Watching = Watching;
    let tree = Tree::new();
    let path = tree.path("watches.jsonl");
    let watches = measured(&WATCHING, &path).watches();
    let [watch] = watches.as_slice() else {
        panic!("{} watches", watches.len());
    };
    assert_eq!(watch.paths(), ["content/", "data/ships.toml"]);
    assert_eq!(watch.changed(Path::new("/r")), Some("/r".into()));
    watch.forget(Path::new("/r"));
    let calls: Vec<_> = of_seam(&path, "watches")
        .iter()
        .map(|line| (line["call"].clone(), line["name"].clone()))
        .collect();
    let named = serde_json::Value::from("content/, data/ships.toml");
    assert_eq!(
        calls,
        [("changed".into(), named.clone()), ("forget".into(), named)]
    );
}
