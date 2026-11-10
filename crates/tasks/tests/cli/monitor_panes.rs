//! The legacy monitor's terminal-watch controls; the parent owns the existing private harness.
use super::*;

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
    let orch = m.register("orchestrator");
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

#[test]
fn hc_absent_is_said_once() {
    let m = Monitor::new("hc-absent");
    let (orch, _) = launched_child(&m);
    m.watch(&orch, Watch::polls(5), &idle);
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
    m.watch(&orch, Watch::polls(5), &|_, n| match n {
        2 => set(blocked.clone()),
        3 => set("not json".into()),
        4 => set(blocked.clone()),
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
    // Two outages, each said once; the block once, not again after the second.
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

fn seed_inbox_notice(board: &Store, handle: &str, key: &str, text: &str) {
    board.client().query(
        "LET $counted = (UPSERT ONLY type::record('counter', ['callback-event', $handle]) SET last = (last ?? 0) + 1);
         CREATE type::record('callback_event', [$handle, $key]) CONTENT {
             key:$key, handle:$handle, text:$text, slug:NONE, message:NONE, attempts:0,
             delivered:false, sequence:$counted.last
         } RETURN NONE;",
        json!({"handle":handle, "key":key, "text":text})).unwrap();
}

fn inbox_rows(board: &Store, handle: &str) -> Vec<Value> {
    board.client().query(
        "RETURN SELECT key, text, sequence, attempts, delivered, message FROM callback_event WHERE handle = $handle ORDER BY sequence, key NUMERIC;",
        json!({"handle":handle})).unwrap()[0].as_array().unwrap().clone()
}

fn inbox_subprocess(monitor: &Monitor, actor: &str) -> String {
    let mut command = sys::Command::new(env!("CARGO_BIN_EXE_tasks"));
    command
        .args(["inbox", "--as", actor])
        .current_dir(&monitor.repo)
        .env_clear()
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
    let watch_before = board
        .client()
        .query(
            "RETURN type::record('callback_state', $handle).pane_watch;",
            json!({"handle":orch}),
        )
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
    let delivery = board
        .client()
        .query(
            "RETURN SELECT message, handle FROM delivery WHERE handle = $handle;",
            json!({"handle":orch}),
        )
        .unwrap();
    assert_eq!(
        delivery[0],
        json!([{"message":queued.message, "handle":orch}])
    );
    assert_eq!(
        board
            .client()
            .query(
                "RETURN type::record('callback_state', $handle).pane_watch;",
                json!({"handle":orch})
            )
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
