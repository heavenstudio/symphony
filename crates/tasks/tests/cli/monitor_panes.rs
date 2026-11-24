//! The legacy monitor's terminal-watch controls; the parent owns the existing private harness.
use super::*;
use crate::children::Child as _;

/// `launched_child`'s terminal, `t1`, as hc keeps it.
fn child_pane(status: &str) -> Value {
    json!({"address": "t1", "agent": "claude", "status": status, "state": "live",
           "cwd": "/wt/child"})
}

/// The terminal watch's lines.
fn watched(m: &Monitor) -> Vec<String> {
    m.starting(&["BLOCKED ", "WATCH "])
}

#[test]
fn a_blocked_pane_is_said_once_until_it_is_answered_and_blocks_again() {
    let mut m = Monitor::new("blocked-once");
    let state = m.fake_hc(json!([child_pane("working")]));
    let (orch, child) = launched_child(&m);
    let set = |status: &str| {
        let terminals = json!({ "terminals": [child_pane(status)] });
        fs::write(&state, terminals.to_string()).unwrap()
    };
    m.watch(&orch, Watch::polls(5), &|_, n| match n {
        0 | 3 => set("blocked"),
        2 => set("working"),
        _ => {}
    });
    let blocked = format!(
        "BLOCKED {child} sim/a t1 — observed waiting on an approval or a question; inspect its terminal before acting"
    );
    assert_eq!(watched(&m), [blocked.clone(), blocked]);
}

#[test]
fn only_the_orchestrator_of_a_blocked_sessions_work_hears_it() {
    let mut m = Monitor::new("blocked-who");
    let (parent, child) = launched_child(&m);
    // Registered through the store, which gives it nothing: the command line's register would have
    // the board give sim/a, nobody's yet, to the live orchestrator holding fewest, a tie by handle.
    let session = symphony_board::NewSession::new("orchestrator");
    let orch = m.board().register(&session).unwrap().row.handle;
    m.ok(&orch, &["role", "sim/a", "orchestrator"]);
    let other = m.register("orchestrator");
    // The orchestrator's own terminal, and one no session holds, wait too.
    let own = NewLaunch {
        pane: "t2".into(),
        ..NewLaunch::new("sim/a", &orch, "reviewer")
    };
    m.board().record_launch(&own).unwrap();
    let pane =
        |id: &str| json!({"address": id, "agent": "codex", "status": "blocked", "state": "live"});
    m.fake_hc(json!([child_pane("blocked"), pane("t2"), pane("t9")]));
    let blocked = format!(
        "BLOCKED {child} sim/a t1 — observed waiting on an approval or a question; inspect its terminal before acting"
    );
    for (who, heard) in [
        (&other, vec![]),
        (&parent, vec![]), // its parent, another orchestrator: the work is orch's, as for LOST
        (&orch, vec![blocked]),
    ] {
        m.lines.borrow_mut().clear();
        m.watch(who, Watch::polls(0), &idle);
        assert_eq!(watched(&m), heard, "{who}");
    }
}

/// Looks a quarter minute apart: the third failing one ends hc's outage stretch.
fn quarter_minutes(polls: usize) -> Watch {
    Watch {
        interval: 15.0,
        ..Watch::polls(polls)
    }
}

#[test]
fn hc_absent_is_said_once() {
    let m = Monitor::new("hc-absent");
    let (orch, _) = launched_child(&m);
    m.watch(&orch, quarter_minutes(6), &idle);
    assert_eq!(
        watched(&m),
        [
            "WATCH hc unanswered at observation: hc ls: No such file or directory (os error 2) — blocked terminals may have gone unreported; inspect hc before acting"
        ]
    );
}

#[test]
fn hc_failing_is_said_once_and_the_block_it_hid_is_said_when_it_answers() {
    let mut m = Monitor::new("hc-failing");
    let state = m.fake_hc(json!([child_pane("blocked")]));
    let (orch, child) = launched_child(&m);
    let set = |text: String| fs::write(&state, text).unwrap();
    let blocked = json!({ "terminals": [child_pane("blocked")] }).to_string();
    set("not json".into());
    // Failing at the looks at 0, 15 and 30 s, answering at 45, failing at 60, 75 and 90, answering
    // at 105.
    m.watch(&orch, quarter_minutes(7), &|_, n| match n {
        2 | 6 => set(blocked.clone()),
        3 => set("not json".into()),
        _ => {}
    });
    let lines = watched(&m);
    let said: Vec<&str> = lines
        .iter()
        .map(|l| &l[..l.find(':').unwrap_or(l.len())])
        .collect();
    let block = format!(
        "BLOCKED {child} sim/a t1 — observed waiting on an approval or a question; inspect its terminal before acting"
    );
    // Two outages, each said once at the end of its stretch; the block once, not again after the
    // second.
    assert_eq!(
        said,
        [
            "WATCH hc unanswered at observation",
            block.as_str(),
            "WATCH hc unanswered at observation"
        ],
        "{lines:?}"
    );
    assert!(
        lines[0].starts_with("WATCH hc unanswered at observation: hc ls: fake_state: "),
        "{lines:?}"
    );
}

#[test]
fn a_poll_asks_hc_once_for_the_view_and_the_terminal_watch() {
    let mut m = Monitor::new("hc-once-watched");
    let (orch, _) = launched_child(&m);
    let calls = m.hc();
    let w = Watch {
        beat: 1000.0,
        ..Watch::past_the_lost_line()
    };
    m.watch(&orch, w, &idle);
    // Every poll watches the launched child's terminal; the polls it is quiet also need the view.
    assert_eq!(Monitor::asked(&calls), GONE_STEPS + 1);
}

/// A callback notice queued for `handle` under `key`, as an older build queued it: numbered by
/// `handle`'s callback counter, as every callback event is.
fn seed_inbox_notice(board: &Store, handle: &str, key: &str, text: &str) {
    let raw = board.raw();
    let counter = raw.row("counter", json!(["callback-event", handle]));
    let sequence = counter.field("last").unwrap().as_u64().unwrap_or(0) + 1;
    counter.put(json!({"last": sequence})).unwrap();
    (raw.row("callback_event", json!([handle, key])))
        .create(
            json!({"key": key, "handle": handle, "text": text, "attempts": 0,
                       "delivered": false, "sequence": sequence}),
        )
        .unwrap();
}

/// `handle`'s callback events, in their order: by sequence, then by key, its numbers by value.
fn inbox_rows(board: &Store, handle: &str) -> Vec<Value> {
    let fields = [
        "key",
        "text",
        "sequence",
        "attempts",
        "delivered",
        "message",
    ];
    let rows = (board.raw().table("callback_event"))
        .matching(json!({"handle": handle}))
        .fields(&fields)
        .unwrap();
    let mut rows = rows.as_array().unwrap().clone();
    rows.sort_by_key(|row| {
        let key = row["key"].as_str().unwrap_or_default();
        (row["sequence"].as_u64(), numbered(key))
    });
    rows
}

/// `key` as runs of digits, each by its value, and of the other characters between them.
fn numbered(key: &str) -> Vec<(String, u128)> {
    let mut runs: Vec<(String, u128)> = Vec::new();
    let mut text = String::new();
    let mut chars = key.chars().peekable();
    while let Some(c) = chars.next() {
        if !c.is_ascii_digit() {
            text.push(c);
            continue;
        }
        let mut digits = c.to_string();
        while let Some(d) = chars.next_if(char::is_ascii_digit) {
            digits.push(d);
        }
        runs.push((
            std::mem::take(&mut text),
            digits.parse().unwrap_or(u128::MAX),
        ));
    }
    runs.push((text, 0));
    runs
}

fn inbox_subprocess(monitor: &Monitor, actor: &str) -> String {
    let mut command = sys::Command::of(env!("CARGO_BIN_EXE_tasks"));
    command
        .args(["inbox", "--as", actor])
        .current_dir(&monitor.repo)
        .cleared()
        .envs(monitor.at().env.vars)
        .env("TASKS_SWEEP", "off")
        .env("TASKS_DISPATCH", "inline")
        .stdin(sys::Stdio::null());
    // The instrumented child contributes to this test run's owned coverage report.
    if let Some(profile) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", profile);
    }
    let output = sys::output(&mut command).unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

struct LegacyInbox {
    rows: Vec<Value>,
    keys: Vec<String>,
    printed: Vec<String>,
    message: i64,
}

fn legacy_inbox(board: &Store, orch: &str, child: &str) -> LegacyInbox {
    let legacy =
        format!("BLOCKED {child} sim/a t1 — its terminal waits on an approval or a question");
    let outage = "WATCH hc unanswered: down — blocked terminals go unreported until it answers";
    let arbitrary = format!("BLOCKED {child} sim/a t1 — user supplied prose");
    let notices = [
        ("pane-982-0", legacy.as_str()),
        ("pane-982-1", outage),
        ("event-982", legacy.as_str()),
        ("pane-user", legacy.as_str()),
        ("pane-982-2", arbitrary.as_str()),
        ("pane-982-3", "WATCH hc unanswered: user prose"),
    ];
    for (key, text) in notices {
        seed_inbox_notice(board, orch, key, text);
    }
    assert!(
        !board
            .send_agent(child, orch, "private fallback barrier")
            .unwrap(),
        "the owned native listener must really be unavailable"
    );
    let addressed = board.say("sim/a", child, &legacy, Some(orch)).unwrap();
    let before = inbox_rows(board, orch);
    let raw = board.callback_inbox(orch).unwrap();
    assert_eq!(raw.len(), notices.len() + 1, "{raw:?}");
    for (index, (key, text)) in notices.iter().enumerate() {
        assert_eq!(
            (&raw[index].0, &raw[index].1),
            (&key.to_string(), &text.to_string())
        );
        let row = before.iter().find(|row| row["key"] == *key).unwrap();
        assert_eq!(row["attempts"], 1, "real failed delivery: {row:?}");
    }
    assert_eq!(
        inbox_rows(board, orch),
        before,
        "raw reading changes nothing"
    );
    let expected = [
        format!("MSG sim/a from {child} to {orch} — {legacy}"),
        format!("BLOCKED {child} sim/a t1 — observed waiting on an approval or a question; inspect its terminal before acting"),
        "WATCH hc unanswered at observation: down — blocked terminals may have gone unreported; inspect hc before acting".into(),
        legacy.clone(), legacy, arbitrary, "WATCH hc unanswered: user prose".into(),
        format!("AGENT from {child} to {orch} — private fallback barrier"),
    ];
    LegacyInbox {
        rows: before,
        keys: raw.into_iter().map(|(key, _)| key).collect(),
        printed: expected.into(),
        message: addressed.id,
    }
}

#[test]
fn ordinary_inbox_renders_legacy_pane_observations_without_a_clock_tick() {
    let mut m = Monitor::new("inbox");
    let (orch, child) = launched_child(&m);
    m.fake_hc(json!([child_pane("working")]));
    let calls = m.dir.join("hc.calls");
    m.c.env
        .vars
        .insert("FAKE_HC_LOG".into(), calls.display().to_string());
    let observed = cli::probes::hc_terminals(&m.c.env).unwrap();
    assert_eq!(observed.len(), 1, "{observed:?}");
    assert_eq!(observed[0].status.as_deref(), Some("working"));
    let hc_calls = fs::read_to_string(&calls).unwrap();
    let board = m.board();
    let socket = m.dir.join("cb.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    board
        .callback(
            &orch,
            Some(&format!("uds:{}", socket.display())),
            false,
            false,
        )
        .unwrap();
    drop(listener);
    fs::remove_file(&socket).unwrap();
    let queued = legacy_inbox(&board, &orch, &child);
    let watch_before = (board.raw().row("callback_state", orch.as_str()))
        .field("pane_watch")
        .unwrap();
    assert_eq!(
        inbox_subprocess(&m, &orch),
        format!("{}\n", queued.printed.join("\n"))
    );
    let mut acknowledged = queued.rows;
    for row in &mut acknowledged {
        if queued.keys.iter().any(|key| row["key"] == *key) {
            row["delivered"] = true.into();
        }
    }
    assert_eq!(
        inbox_rows(&board, &orch),
        acknowledged,
        "only original callback keys are acknowledged"
    );
    let delivery = (board.raw().table("delivery"))
        .matching(json!({"handle":orch}))
        .fields(&["message", "handle"])
        .unwrap();
    assert_eq!(delivery, json!([{"message":queued.message, "handle":orch}]));
    assert_eq!(
        (board.raw().row("callback_state", orch.as_str()))
            .field("pane_watch")
            .unwrap(),
        watch_before
    );
    assert_eq!(
        fs::read_to_string(&calls).unwrap(),
        hc_calls,
        "inbox does not sample HC"
    );
    assert_eq!(
        inbox_subprocess(&m, &orch),
        "",
        "acknowledged output stays quiet"
    );
    assert_eq!(fs::read_to_string(&calls).unwrap(), hc_calls);
}
