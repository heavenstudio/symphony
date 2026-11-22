use super::*;

const PATH: &str = "/r/symphony.toml";

fn parsed(text: &str) -> Read {
    parse(PATH, text)
}

fn written(at: &str, line: usize) -> Written {
    Written {
        at: at.into(),
        line: Some(line),
    }
}

#[test]
fn a_setting_is_named_with_its_line_and_without_one_it_cannot_find() {
    let found = written("[limits] low_volume", 3);
    assert_eq!(found.said(), "[limits] low_volume, line 3");
    assert_eq!(found.named(), "[limits] low_volume (line 3)");
    let lost = Written {
        at: "[limits] low_volume".into(),
        line: None,
    };
    assert_eq!(lost.said(), "[limits] low_volume");
    assert_eq!(lost.named(), "[limits] low_volume");
}

#[test]
fn a_file_naming_no_limit_is_every_default_as_today() {
    for text in [
        "",
        "[project]\nname = \"x\"\n",
        "[board]\non_review = [\"x\"]\nexport = \"tasks.md\"\n",
        "[limits]\n",
        "[agents]\nlaunch = { default = \"claude\", model = \"opus\" }\n",
    ] {
        let read = parsed(text);
        assert_eq!(read, Read::default(), "{text}");
        assert_eq!(read.limits.orchestrator_slots, None, "caps nothing");
        assert_eq!(read.limits.orchestrator_tasks, 16);
        assert_eq!(read.limits.low_volume, 4);
        assert_eq!(read.limits.machine_agents, None, "limits no machine");
        assert_eq!(read.limits.alarm, Thresholds::default());
    }
}

#[test]
fn each_limit_is_read_from_the_table_with_where_it_stands() {
    let read = parsed(
        "[project]\nname = \"x\"\n\n[limits]\norchestrator_slots = 6\nlow_volume = 2\nmachine_agents = 12\norchestrator_tasks = 9\n",
    );
    assert_eq!(read.problems, Vec::<String>::new());
    assert_eq!(
        read.limits,
        Limits {
            orchestrator_slots: Some(6),
            orchestrator_tasks: 9,
            low_volume: 2,
            machine_agents: Some(12),
            alarm: Thresholds::default(),
        }
    );
    assert_eq!(
        read.written,
        BTreeMap::from([
            (SLOTS, written("[limits] orchestrator_slots", 5)),
            (LOW, written("[limits] low_volume", 6)),
            (MACHINE_AGENTS, written("[limits] machine_agents", 7)),
            (TASKS, written("[limits] orchestrator_tasks", 8)),
        ])
    );
    assert_eq!(
        written("[limits] low_volume", 6).said(),
        "[limits] low_volume, line 6"
    );
}

#[test]
fn a_low_volume_of_naught_tells_none_and_the_smallest_cap_and_machine_are_one() {
    let read = parsed("[limits]\nlow_volume = 0\norchestrator_slots = 1\nmachine_agents = 1\n");
    assert_eq!(read.problems, Vec::<String>::new());
    assert_eq!(read.limits.low_volume, 0);
    assert_eq!(read.limits.orchestrator_slots, Some(1));
    assert_eq!(read.limits.machine_agents, Some(1));
}

#[test]
fn the_boards_cap_holds_where_the_limits_name_none_as_an_older_build_reads_it() {
    let read = parsed("[board]\nguide = \"docs/TASKS.md\"\norchestrator_slots = 8\n");
    assert_eq!(read.limits.orchestrator_slots, Some(8));
    assert_eq!(
        read.written[SLOTS],
        written("[board] orchestrator_slots", 3)
    );
    assert_eq!(read.problems, Vec::<String>::new());
}

#[test]
fn a_cap_written_in_both_tables_is_the_limits_one_and_two_values_are_said() {
    let same = parsed("[board]\norchestrator_slots = 8\n[limits]\norchestrator_slots = 8\n");
    assert_eq!(same.limits.orchestrator_slots, Some(8));
    assert_eq!(
        same.written[SLOTS],
        written("[limits] orchestrator_slots", 4)
    );
    assert_eq!(
        same.problems,
        Vec::<String>::new(),
        "the same value is a staging, never said"
    );
    let two = parsed("[board]\norchestrator_slots = 8\n[limits]\norchestrator_slots = 5\n");
    assert_eq!(two.limits.orchestrator_slots, Some(5));
    assert_eq!(
        two.problems,
        vec![format!(
            "{PATH}: [board] orchestrator_slots (line 2) is 8, left out: [limits] \
             orchestrator_slots (line 4) is 5, which holds"
        )]
    );
}

#[test]
fn every_limit_is_a_whole_number_from_its_least_and_the_cap_at_most_u32() {
    let most = i64::from(u32::MAX);
    let size = |n: i64| usize::try_from(n).unwrap();
    for (key, least, below) in LEAST {
        assert_eq!(
            judged(key, &Value::Integer(least)),
            Ok(size(least)),
            "{key}"
        );
        assert_eq!(judged(key, &Value::Integer(least - 1)), Err(below), "{key}");
        assert_eq!(judged(key, &Value::Integer(most)), Ok(size(most)), "{key}");
        for value in [
            Value::Float(2.0),
            Value::String("8".into()),
            Value::Boolean(true),
        ] {
            assert_eq!(judged(key, &value), Err("not a whole number"), "{key}");
        }
    }
    // The cap alone is held to what the Symphony's project holds it in; the others are as before.
    assert_eq!(
        judged(SLOTS, &Value::Integer(most + 1)),
        Err("past what it can be")
    );
    for key in [TASKS, LOW, MACHINE_AGENTS] {
        assert_eq!(
            judged(key, &Value::Integer(most + 1)),
            Ok(size(most + 1)),
            "{key}"
        );
    }
}

#[test]
fn a_cap_past_u32_is_said_in_either_table_and_caps_nothing() {
    for (table, line) in [("limits", 2), ("board", 2)] {
        let read = parsed(&format!("[{table}]\norchestrator_slots = 4294967296\n"));
        assert_eq!(read.limits.orchestrator_slots, None, "{table}");
        assert_eq!(
            read.problems,
            [format!(
                "{PATH}: [{table}] orchestrator_slots (line {line}) is 4294967296; past what it \
                 can be; no cap on an orchestrator's open tasks applies"
            )]
        );
        let read = parsed(&format!("[{table}]\norchestrator_slots = 4294967295\n"));
        assert_eq!(
            read.limits.orchestrator_slots,
            Some(4_294_967_295),
            "{table}"
        );
        assert_eq!(read.problems, Vec::<String>::new(), "{table}");
    }
}

#[test]
fn a_bad_value_is_said_with_its_line_and_takes_its_default_alone() {
    for (text, problem, kept) in [
        (
            "[limits]\nlow_volume = 3\norchestrator_slots = 0\n",
            format!(
                "{PATH}: [limits] orchestrator_slots (line 3) is 0; an orchestrator needs at \
                 least one slot; no cap on an orchestrator's open tasks applies"
            ),
            Limits {
                low_volume: 3,
                ..Limits::default()
            },
        ),
        (
            "[board]\norchestrator_slots = -2\n",
            format!(
                "{PATH}: [board] orchestrator_slots (line 2) is -2; an orchestrator needs at \
                 least one slot; no cap on an orchestrator's open tasks applies"
            ),
            Limits::default(),
        ),
        (
            "[board]\norchestrator_slots = \"eight\"\n",
            format!(
                "{PATH}: [board] orchestrator_slots (line 2) is \"eight\"; not a whole number; \
                 no cap on an orchestrator's open tasks applies"
            ),
            Limits::default(),
        ),
        (
            "[limits]\nmachine_agents = 9\n\nlow_volume = -1\n",
            format!(
                "{PATH}: [limits] low_volume (line 4) is -1; it is at least 0; it takes its \
                 default, 4"
            ),
            Limits {
                machine_agents: Some(9),
                ..Limits::default()
            },
        ),
        (
            "[limits]\nlow_volume = 2.5\n",
            format!(
                "{PATH}: [limits] low_volume (line 2) is 2.5; not a whole number; it takes its \
                 default, 4"
            ),
            Limits::default(),
        ),
        (
            "[limits]\nmachine_agents = 0\n",
            format!(
                "{PATH}: [limits] machine_agents (line 2) is 0; a machine runs at least one \
                 agent; no limit on the agents this machine runs applies"
            ),
            Limits::default(),
        ),
        (
            "[limits]\nmachine_agents = \"many\"\n",
            format!(
                "{PATH}: [limits] machine_agents (line 2) is \"many\"; not a whole number; no \
                 limit on the agents this machine runs applies"
            ),
            Limits::default(),
        ),
        (
            "[limits]\nmachine_agents = true\n",
            format!(
                "{PATH}: [limits] machine_agents (line 2) is true; not a whole number; no limit \
                 on the agents this machine runs applies"
            ),
            Limits::default(),
        ),
        (
            "[limits]\nlow_volume = [4]\n",
            format!(
                "{PATH}: [limits] low_volume (line 2) is array; not a whole number; it takes its \
                 default, 4"
            ),
            Limits::default(),
        ),
        (
            "[limits]\nlow_volume = 2\norchestrator_tasks = 0\n",
            format!(
                "{PATH}: [limits] orchestrator_tasks (line 3) is 0; an orchestrator holds at \
                 least one task; it takes its default, 16"
            ),
            Limits {
                low_volume: 2,
                ..Limits::default()
            },
        ),
        (
            "[limits]\norchestrator_tasks = \"sixteen\"\n",
            format!(
                "{PATH}: [limits] orchestrator_tasks (line 2) is \"sixteen\"; not a whole number; \
                 it takes its default, 16"
            ),
            Limits::default(),
        ),
        (
            "[limits]\norchestrator_tasks = 12.5\n",
            format!(
                "{PATH}: [limits] orchestrator_tasks (line 2) is 12.5; not a whole number; it \
                 takes its default, 16"
            ),
            Limits::default(),
        ),
    ] {
        let read = parsed(text);
        assert_eq!(read.problems, vec![problem], "{text}");
        assert_eq!(read.limits, kept, "{text}");
    }
}

#[test]
fn the_most_tasks_an_orchestrator_holds_is_read_from_limits_alone_down_to_one() {
    let read = parsed("[limits]\norchestrator_tasks = 1\n");
    assert_eq!(read.problems, Vec::<String>::new());
    assert_eq!(read.limits.orchestrator_tasks, 1);
    assert_eq!(
        read.written[TASKS],
        written("[limits] orchestrator_tasks", 2)
    );
    assert!(read.unknown.is_empty());
    // Never read from `[board]`, where no build that knows the key looks: the default holds.
    let board = parsed("[board]\norchestrator_tasks = 3\n");
    assert_eq!(board.limits.orchestrator_tasks, ORCHESTRATOR_TASKS);
    assert!(!board.written.contains_key(TASKS));
}

#[test]
fn a_broken_limit_in_the_table_leaves_the_boards_cap_out_too() {
    // The `[limits]` cap is the one that counts once written: broken, nothing caps, and it is said.
    let read = parsed("[board]\norchestrator_slots = 8\n[limits]\norchestrator_slots = 0\n");
    assert_eq!(read.limits.orchestrator_slots, None);
    assert_eq!(read.problems.len(), 1, "{:?}", read.problems);
    assert!(
        read.problems[0].contains("[limits] orchestrator_slots (line 4) is 0"),
        "{:?}",
        read.problems
    );
}

#[test]
fn the_alarm_thresholds_are_read_beside_them_or_all_take_the_defaults() {
    let read = parsed("[board]\nreview_alarm_ratio = 3\n");
    assert_eq!(
        read.limits.alarm,
        Thresholds {
            ratio: 3,
            ..Thresholds::default()
        }
    );
    assert_eq!(
        read.written[alarm::KEYS[0]],
        written("[board] review_alarm_ratio", 2)
    );
    let read = parsed(
        "[board]\nreview_alarm_ratio = 2\nreview_alarm_hours = 6\nreview_alarm_clear_hours = 2\n",
    );
    assert_eq!(
        read.limits.alarm,
        Thresholds {
            ratio: 2,
            raise_hours: 6,
            clear_hours: 2,
        }
    );
    for (text, said) in [
        (
            "[board]\nreview_alarm_ratio = 0\n",
            "[board] review_alarm_ratio (line 2) is 0; it is at least 1",
        ),
        (
            "[board]\nreview_alarm_hours = 5\nreview_alarm_clear_hours = -1\n",
            "[board] review_alarm_clear_hours (line 3) is -1; it is at least 1",
        ),
        (
            "[board]\nreview_alarm_clear_hours = 1.5\n",
            "review_alarm_clear_hours (line 2) is 1.5; not a whole number",
        ),
        (
            "[board]\nreview_alarm_ratio = \"two\"\n",
            "review_alarm_ratio (line 2) is \"two\"",
        ),
        (
            "[board]\nreview_alarm_ratio = 5000000000\n",
            "review_alarm_ratio (line 2) is 5000000000; past what it can be",
        ),
    ] {
        let read = parsed(text);
        assert_eq!(read.limits.alarm, Thresholds::default(), "{text}");
        assert_eq!(read.problems.len(), 1, "{text}: {:?}", read.problems);
        let problem = &read.problems[0];
        assert!(problem.starts_with(PATH), "{text}: {problem}");
        assert!(problem.contains(said), "{text}: {problem}");
        assert!(
            problem.ends_with("; the pipeline alarm takes its default thresholds"),
            "{text}: {problem}"
        );
        assert!(
            !read.written.contains_key(alarm::KEYS[1]),
            "a threshold read beside a broken one is not in force: {text}"
        );
    }
}

#[test]
fn a_key_the_table_does_not_know_is_left_out_by_name_and_line() {
    let read = parsed("[limits]\nlow_volume = 2\nexecutors = 3\nmachne_agents = 4\n");
    assert_eq!(
        read.unknown,
        vec![
            "[limits] executors (line 3)".to_string(),
            "[limits] machne_agents (line 4)".to_string()
        ]
    );
    assert_eq!(read.limits.low_volume, 2);
    assert_eq!(read.limits.machine_agents, None);
    assert_eq!(read.problems, Vec::<String>::new());
}

#[test]
fn a_file_that_does_not_parse_or_a_table_that_is_none_takes_every_default() {
    let read = parsed("[limits\nlow_volume = 2\n");
    assert_eq!(read.limits, Limits::default());
    assert_eq!(read.problems.len(), 1);
    assert!(
        read.problems[0].starts_with(&format!("{PATH}: ")),
        "{:?}",
        read.problems
    );
    assert!(
        read.problems[0].ends_with("; every limit takes its default"),
        "{:?}",
        read.problems
    );
    let read = parsed("limits = 4\n[board]\norchestrator_slots = 3\n");
    assert_eq!(
        read.limits.orchestrator_slots,
        Some(3),
        "the board's cap still holds"
    );
    assert_eq!(
        read.problems,
        vec![format!(
            "{PATH}: limits (line 1) is integer, not a table; every limit takes its default"
        )]
    );
}

#[test]
fn lines_are_found_in_tables_inline_and_dotted_and_not_for_what_is_missing() {
    let text = "[board]\nguide = \"x\"\n\n[agents]\nlaunch = { default = \"claude\", model = \"opus\" }\n[agents.models]\nexecutor = \"opus\"\n";
    assert_eq!(line(text, &["board"], "guide"), Some(2));
    assert_eq!(line(text, &["agents"], "launch"), Some(5));
    assert_eq!(line(text, &["agents", "launch"], "model"), Some(5));
    assert_eq!(line(text, &["agents", "models"], "executor"), Some(7));
    assert_eq!(line(text, &["agents", "models"], "reviewer"), None);
    assert_eq!(line(text, &["limits"], "low_volume"), None);
    assert_eq!(line("[broken", &["board"], "guide"), None);
}

/// A scratch folder of this test's own, made empty.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("limits-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn no_file_is_every_default_and_one_that_cannot_be_read_is_said() {
    let dir = folder("files");
    assert_eq!(read_at(&dir.join(MANIFEST)), Read::default());
    // A folder where the file would be is no file missing.
    std::fs::create_dir_all(dir.join("unreadable").join(MANIFEST)).unwrap();
    let read = read_at(&dir.join("unreadable").join(MANIFEST));
    assert_eq!(read.limits, Limits::default());
    assert_eq!(read.problems.len(), 1);
    assert!(
        read.problems[0].starts_with(&format!(
            "cannot read {}: ",
            dir.join("unreadable").join(MANIFEST).display()
        )),
        "{:?}",
        read.problems
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_watched_file_is_read_again_once_it_changes_and_not_before() {
    let dir = folder("watched");
    let path = dir.join(MANIFEST);
    std::fs::write(&path, "[limits]\nlow_volume = 2\n").unwrap();
    let (watched, read) = Watched::new(path.clone());
    assert_eq!(watched.path(), path.as_path());
    assert_eq!(read.limits.low_volume, 2);
    assert_eq!(watched.now().low_volume, 2);
    // Its length changes with the value: what tells one version from the next.
    std::fs::write(&path, "[limits]\nlow_volume = 3\norchestrator_slots = 5\n").unwrap();
    let now = watched.now();
    assert_eq!((now.low_volume, now.orchestrator_slots), (3, Some(5)));
    std::fs::remove_file(&path).unwrap();
    assert_eq!(watched.now(), Limits::default(), "no file: every default");
    std::fs::write(&path, "[limits]\nmachine_agents = 7\n").unwrap();
    assert_eq!(watched.now().machine_agents, Some(7));
    let held = Held::Watched(watched.clone());
    assert_eq!(held.now(Stored::default).machine_agents, Some(7));
    assert_eq!(Held::default().now(Stored::default), Limits::default());
    // A version it cannot read is said (on stderr, once) and its value takes the default.
    std::fs::write(&path, "[limits]\nmachine_agents = 0\nlow_volume = 1\n").unwrap();
    let read = watched.read();
    assert_eq!(
        read.problems,
        [format!(
            "{}: [limits] machine_agents (line 2) is 0; a machine runs at least one agent; no \
             limit on the agents this machine runs applies",
            path.display()
        )]
    );
    assert_eq!(
        (read.limits.machine_agents, read.limits.low_volume),
        (None, 1)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_file_of_a_run_is_its_main_checkouts_and_none_outside_a_repository() {
    let dir = folder("file-of");
    let mut env = Env {
        cwd: dir.clone(),
        ..Env::default()
    };
    assert_eq!(file_of(&env), None, "no repository");
    let root = dir.join("repo");
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
    env.cwd = root.join("docs");
    let found = file_of(&env).expect("the main checkout's file, from a folder in it");
    assert_eq!(found, root.canonicalize().unwrap().join(MANIFEST));
    let _ = std::fs::remove_dir_all(&dir);
}
