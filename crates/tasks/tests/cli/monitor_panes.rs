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
