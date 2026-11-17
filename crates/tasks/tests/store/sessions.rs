//! Sessions: registering, being seen, a Claude Code agent's tool calls beating for a handle, leaving and
//! coming back.
use crate::common::Backend;

use crate::common::{Fixture, GONE, HOST, NOW, Seed, refused};
use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Barrier};
use symphony_board::health::LIVE_SECONDS;
use symphony_board::{Beat, Health, NewSession, Pick, Store};

fn executing(backend: Backend) -> (Fixture, String) {
    let f = Fixture::on(backend, Seed::default().task("sim/jump", "ready"));
    let h = f.session("worker");
    f.store.claim("sim/jump", &h).unwrap();
    (f, h)
}

fn monitored(kind: &str, host: &str) -> NewSession {
    NewSession {
        pid: Some(41),
        host: host.into(),
        ..NewSession::new(kind)
    }
}

fn beat() -> Beat {
    Beat::default()
}

fn busy(seconds: f64, call: &str) -> Beat {
    Beat {
        busy_for: Some(seconds),
        call: Some(call.into()),
        ..Beat::default()
    }
}

fn ended(call: &str) -> Beat {
    Beat {
        call: Some(call.into()),
        ended: true,
        ..Beat::default()
    }
}

/// `^[a-z]+-[a-z]+(-\d+)?$`
fn readable(handle: &str) -> bool {
    let word = |w: &&str| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase());
    let number = |n: &&str| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit());
    match handle.split('-').collect::<Vec<_>>().as_slice() {
        [a, b] => word(a) && word(b),
        [a, b, n] => word(a) && word(b) && number(n),
        _ => false,
    }
}

/// `random.Random` stuck on the first word of each list, counting up for the suffix from where it may
/// start; or, `forever`, giving the same suffix every time.
struct Stuck {
    n: Option<i64>,
    forever: bool,
}

impl Stuck {
    fn new(forever: bool) -> Box<Stuck> {
        Box::new(Stuck { n: None, forever })
    }
}

impl Pick for Stuck {
    fn index(&mut self, _len: usize) -> usize {
        0
    }

    fn number(&mut self, from: i64, _to: i64) -> i64 {
        let n = match self.n {
            Some(n) if !self.forever => n + 1,
            _ => from,
        };
        self.n = Some(n);
        n
    }
}

on_both! {
fn register_gives_a_readable_unique_handle(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let handles: HashSet<String> = (0..40).map(|_| f.session("worker")).collect();
    assert_eq!(handles.len(), 40);
    for h in &handles {
        assert!(readable(h), "{h}");
    }
}
}

on_both! {
fn a_session_is_registered_with_what_it_says_of_itself(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let parent = f.session("orchestrator");
    let session = f
        .store
        .register(&NewSession {
            parent: Some(parent.clone()),
            worktree: "/wt/a".into(),
            branch: "sim/a".into(),
            model: "opus".into(),
            pid: Some(41),
            host: "elsewhere".into(),
            affinity: vec![" sim".into(), String::new(), "view ".into(), "  ".into()],
            ..NewSession::new("worker")
        })
        .unwrap();
    let row = &session.row;
    assert_eq!(
        (
            row.kind.as_str(),
            row.parent.as_deref(),
            row.worktree.as_str(),
            row.branch.as_str(),
            row.model.as_str()
        ),
        ("worker", Some(parent.as_str()), "/wt/a", "sim/a", "opus")
    );
    assert_eq!(
        (row.pid, row.host.as_str(), row.affinity.as_str()),
        (Some(41), "elsewhere", "sim,view")
    );
    assert_eq!(
        (row.connected, row.last_seen, row.left_at, row.last_tool),
        (NOW, NOW, None, None)
    );
    assert_eq!((row.agent.as_deref(), session.health), (None, Health::Live));
    assert_eq!(f.store.session(&row.handle).unwrap(), session);
}
}

on_both! {
fn every_kind_is_a_session_and_no_other(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    for kind in ["orchestrator", "worker", "reviewer", "chat", "app", "filer"] {
        let h = f.session(kind);
        assert_eq!(f.store.session(&h).unwrap().row.kind, kind);
    }
    assert_eq!(
        refused(f.store.register(&NewSession::new("boss"))),
        "kind must be one of orchestrator, worker, reviewer, chat, app, filer"
    );
}
}

on_both! {
fn an_unknown_parent_is_refused(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let orphan = NewSession {
        parent: Some("nobody-here".into()),
        ..NewSession::new("worker")
    };
    assert_eq!(
        refused(f.store.register(&orphan)),
        "no parent session 'nobody-here'"
    );
    assert!(f.store.sessions().unwrap().is_empty());
}
}

on_both! {
fn handles_stay_unique_when_the_word_lists_run_out(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let stuck = f.other().picking(Stuck::new(false));
    let handles: Vec<String> = (0..3)
        .map(|_| {
            stuck
                .register(&NewSession::new("worker"))
                .unwrap()
                .row
                .handle
        })
        .collect();
    assert_eq!(handles, ["amber-heron", "amber-heron-2", "amber-heron-3"]);
}
}

on_both! {
fn a_handle_not_found_in_two_hundred_tries_is_refused(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let stuck = f.other().picking(Stuck::new(true));
    stuck.register(&NewSession::new("worker")).unwrap();
    stuck.register(&NewSession::new("worker")).unwrap();
    assert_eq!(
        refused(stuck.register(&NewSession::new("worker"))),
        "could not find a free session handle"
    );
}
}

on_both! {
fn concurrent_registers_never_share_a_handle(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let barrier = Arc::new(Barrier::new(6));
    let threads: Vec<_> = (0..6)
        .map(|_| {
            let (store, barrier) = (f.other().picking(Stuck::new(false)), Arc::clone(&barrier));
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .register(&NewSession::new("worker"))
                    .unwrap()
                    .row
                    .handle
            })
        })
        .collect();
    let handles: HashSet<String> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(handles.len(), 6, "{handles:?}");
    assert_eq!(f.store.sessions().unwrap().len(), 6);
}
}

on_both! {
fn sessions_come_by_when_they_connected_with_their_health(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let early = f.session("worker");
    f.advance(1.0);
    let late = f.session("reviewer");
    f.advance(GONE - 1.0);
    let read: Vec<(String, Health)> = f
        .store
        .sessions()
        .unwrap()
        .into_iter()
        .map(|s| (s.row.handle, s.health))
        .collect();
    assert_eq!(read, [(early, Health::Left), (late, Health::Stale)]);
}
}

on_both! {
fn touch_keeps_a_session_live(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let h = f.session("worker");
    f.advance(LIVE_SECONDS + 1.0);
    f.store.touch(&h).unwrap();
    assert_eq!(f.health(&h), Health::Live);
    assert_eq!(f.store.session(&h).unwrap().row.last_seen, f.now());
}
}

on_both! {
fn touch_of_an_unknown_or_left_session_is_refused(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    assert_eq!(
        refused(f.store.touch("ghost-owl")),
        f.no_session("ghost-owl")
    );
    let h = f.session("worker");
    f.store.leave(&h).unwrap();
    assert_eq!(
        refused(f.store.touch(&h)),
        format!("session {h} has left; attach again")
    );
}
}

on_both! {
fn reattach_takes_the_handle_back_with_its_roles(backend: Backend) {
    let f = Fixture::on(backend, Seed::default().task("sim/jump", "ready"));
    let h = f.session_with(monitored("worker", HOST));
    f.store.claim("sim/jump", &h).unwrap();
    f.advance(LIVE_SECONDS + 1.0);
    let back = f.store.reattach(&h, Some(42)).unwrap();
    assert_eq!(
        (back.row.pid, back.row.host.as_str(), back.row.last_seen),
        (Some(42), HOST, f.now())
    );
    f.alive(&[42]);
    assert_eq!(f.health(&h), Health::Live);
    assert_eq!(
        f.store.get("sim/jump").unwrap().holders("executor"),
        [h.as_str()]
    );
}
}

on_both! {
fn reattach_without_a_monitor_forgets_the_old_one(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let h = f.session_with(monitored("worker", HOST));
    let back = f.store.reattach(&h, None).unwrap();
    assert_eq!(
        (back.row.pid, back.row.host.as_str(), back.health),
        (None, "", Health::Live)
    );
}
}

on_both! {
fn reattach_of_a_left_session_is_refused(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let h = f.session("worker");
    f.store.leave(&h).unwrap();
    assert_eq!(
        refused(f.store.reattach(&h, Some(1))),
        format!("session {h} has left; attach again")
    );
}
}

on_both! {
fn leave_releases_every_role_on_the_record_and_ends_the_session(backend: Backend) {
    let f = Fixture::on(backend,
        Seed::default()
            .session("amber-heron")
            .session("brisk-finch")
            .task("sim/jump", "in_progress")
            .role("sim/jump", "amber-heron", "executor")
            .task("sim/dock", "in_progress")
            .role("sim/dock", "brisk-finch", "executor")
            .role("sim/dock", "amber-heron", "orchestrator")
            .role("sim/dock", "amber-heron", "reviewer")
            .task("sim/lamp", "design")
            .role("sim/lamp", "amber-heron", "executor"),
    );
    f.advance(5.0);
    f.store.leave("amber-heron").unwrap();
    let gone = f.store.session("amber-heron").unwrap();
    assert_eq!(
        (gone.health, gone.row.left_at, gone.row.last_seen),
        (Health::Left, Some(f.now()), f.now())
    );
    let jump = f.store.get("sim/jump").unwrap();
    assert_eq!(
        (jump.status.as_str(), jump.roles.len(), jump.updated),
        ("ready", 0, f.now())
    );
    let dock = f.store.get("sim/dock").unwrap();
    assert_eq!(
        (dock.status.as_str(), dock.roles.len(), dock.updated),
        ("in_progress", 1, NOW)
    );
    assert_eq!(dock.holders("executor"), ["brisk-finch"]);
    let lamp = f.store.get("sim/lamp").unwrap();
    assert_eq!((lamp.status.as_str(), lamp.roles.len()), ("design", 0));
    for slug in ["sim/dock", "sim/lamp", "sim/jump"] {
        let last = f.store.thread(slug).unwrap().pop().unwrap();
        assert_eq!(
            (
                last.sender.as_str(),
                last.kind.as_str(),
                last.text.as_str(),
                last.ts
            ),
            (
                "amber-heron",
                "system",
                "amber-heron released every role (left)",
                f.now()
            )
        );
    }
    assert_eq!(
        refused(f.store.leave("amber-heron")),
        "session amber-heron has left; attach again"
    );
    assert_eq!(
        refused(f.store.leave("ghost-owl")),
        f.no_session("ghost-owl")
    );
}
}

on_both! {
fn a_session_that_left_comes_back_holding_nothing(backend: Backend) {
    let f = Fixture::on(backend,
        Seed::default()
            .session("amber-heron")
            .task("sim/jump", "in_progress")
            .role("sim/jump", "amber-heron", "executor"),
    );
    f.store.leave("amber-heron").unwrap();
    f.advance(30.0);
    f.store.revive("amber-heron").unwrap();
    let back = f.store.session("amber-heron").unwrap();
    assert_eq!(
        (back.health, back.row.left_at, back.row.last_seen),
        (Health::Live, None, f.now())
    );
    assert!(f.store.get("sim/jump").unwrap().roles.is_empty());
    f.store.touch("amber-heron").unwrap();
    assert_eq!(
        refused(f.store.revive("ghost-owl")),
        "no session 'ghost-owl'"
    );
}
}

on_both! {
fn a_bound_agent_beating_stays_live_past_the_lost_line(backend: Backend) {
    let (f, h) = executing(backend);
    assert!(f.store.bind_agent("agent-1", &h, false).unwrap());
    for _ in 0..(GONE as i64 / 30 + 2) {
        f.advance(30.0);
        assert_eq!(
            f.store.agent_beat("agent-1", &beat()).unwrap(),
            Some(h.clone())
        );
    }
    assert_eq!(f.health(&h), Health::Live);
}
}

on_both! {
/// A beat that read the clock, then lost the race to one that read it later, never takes `last_seen`
/// back: the SQLite store read the clock inside its write lock.
fn a_beat_that_read_the_clock_first_and_wrote_last_never_takes_last_seen_back(backend: Backend) {
    let (f, h) = executing(backend);
    assert!(f.store.bind_agent("agent-1", &h, false).unwrap());
    let client = f.other_database();
    let slow = f.slow(move || {
        let later = Store::opening_on(
            client.another(),
            Box::new(|| NOW + 30.0),
            Box::new(|_| false),
            HOST,
            &symphony_board::Opening::write(),
        )
        .unwrap();
        later.agent_beat("agent-1", &Beat::default()).unwrap();
    });
    assert_eq!(
        slow.agent_beat("agent-1", &beat()).unwrap(),
        Some(h.clone())
    );
    assert_eq!(f.store.session(&h).unwrap().row.last_seen, NOW + 30.0);
}
}

on_both! {
fn an_unbound_agent_beats_nobody(backend: Backend) {
    let (f, h) = executing(backend);
    f.advance(LIVE_SECONDS + 1.0);
    assert_eq!(f.store.agent_beat("agent-9", &beat()).unwrap(), None);
    assert_eq!(f.health(&h), Health::Stale);
}
}

on_both! {
fn the_binding_follows_the_latest_handle_named(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let (a, b) = (f.session("worker"), f.session("worker"));
    f.store.bind_agent("agent-1", &a, false).unwrap();
    f.store.agent_beat("agent-1", &busy(600.0, "long")).unwrap();
    f.store.bind_agent("agent-1", &b, false).unwrap();
    let left = f.store.session(&a).unwrap().row;
    assert_eq!(
        (left.agent, left.busy_until, left.busy_call),
        (None, None, None)
    );
    f.advance(LIVE_SECONDS + 1.0);
    assert_eq!(
        f.store.agent_beat("agent-1", &beat()).unwrap(),
        Some(b.clone())
    );
    assert_eq!((f.health(&a), f.health(&b)), (Health::Stale, Health::Live));
    assert_eq!(
        f.store.handle_for_agent("agent-1").unwrap(),
        Some(b.clone())
    );
    assert_eq!(
        f.store.bound_agents().unwrap(),
        BTreeMap::from([(b, "agent-1".to_string())])
    );
}
}

on_both! {
fn binding_the_handle_already_bound_writes_nothing(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let h = f.session("worker");
    f.advance(10.0);
    assert!(f.store.bind_agent("agent-1", &h, false).unwrap());
    f.advance(10.0);
    assert!(f.store.bind_agent("agent-1", &h, false).unwrap());
    assert_eq!(f.store.session(&h).unwrap().row.last_seen, NOW + 10.0);
}
}

on_both! {
fn concurrent_binds_of_one_agent_leave_it_one_handle(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let handles: Vec<String> = (0..6).map(|_| f.session("worker")).collect();
    let barrier = Arc::new(Barrier::new(handles.len()));
    let threads: Vec<_> = handles
        .iter()
        .map(|h| {
            let (store, barrier, h) = (f.other(), Arc::clone(&barrier), h.clone());
            std::thread::spawn(move || {
                barrier.wait();
                store.bind_agent("agent-1", &h, false).unwrap()
            })
        })
        .collect();
    for t in threads {
        assert!(t.join().unwrap());
    }
    let bound: Vec<_> = f
        .store
        .sessions()
        .unwrap()
        .into_iter()
        .filter(|s| s.row.agent.is_some())
        .collect();
    assert_eq!(bound.len(), 1, "{bound:?}");
}
}

on_both! {
fn an_unknown_or_left_handle_is_not_bound(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let h = f.session("worker");
    f.store.leave(&h).unwrap();
    assert!(!f.store.bind_agent("agent-1", &h, false).unwrap());
    assert!(!f.store.bind_agent("agent-1", "ghost-owl", false).unwrap());
    assert_eq!(f.store.agent_beat("agent-1", &beat()).unwrap(), None);
}
}

on_both! {
fn a_session_that_left_is_not_revived_by_its_agent(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let h = f.session("worker");
    f.store.bind_agent("agent-1", &h, false).unwrap();
    f.store.leave(&h).unwrap();
    assert_eq!(f.store.agent_beat("agent-1", &beat()).unwrap(), None);
    assert_eq!(f.health(&h), Health::Left);
    assert_eq!(f.store.handle_for_agent("agent-1").unwrap(), None);
    assert!(f.store.bound_agents().unwrap().is_empty());
}
}

on_both! {
fn one_long_call_reads_live_until_it_could_have_ended(backend: Backend) {
    let (f, h) = executing(backend);
    f.store.bind_agent("agent-1", &h, false).unwrap();
    f.store
        .agent_beat(
            "agent-1",
            &Beat {
                busy_for: Some(600.0),
                ..Beat::default()
            },
        )
        .unwrap();
    f.advance(400.0);
    assert_eq!(f.health(&h), Health::Live);
    f.store.agent_beat("agent-1", &beat()).unwrap(); // another tool call meanwhile: the long one still runs
    f.advance(LIVE_SECONDS + 1.0);
    assert_eq!(f.health(&h), Health::Live);
    let returned = Beat {
        ended: true,
        ..Beat::default()
    };
    f.store.agent_beat("agent-1", &returned).unwrap(); // the call returned: busy no more
    f.advance(LIVE_SECONDS + 1.0);
    assert_eq!(f.health(&h), Health::Stale);
}
}

on_both! {
fn a_call_ending_clears_only_its_own_busy_mark(backend: Backend) {
    let (f, h) = executing(backend);
    f.store.bind_agent("agent-1", &h, false).unwrap();
    f.store
        .agent_beat("agent-1", &busy(120.0, "short"))
        .unwrap();
    f.store.agent_beat("agent-1", &busy(600.0, "long")).unwrap();
    f.store.agent_beat("agent-1", &ended("short")).unwrap();
    let row = f.store.session(&h).unwrap().row;
    assert_eq!(
        (row.busy_until, row.busy_call.as_deref()),
        (Some(NOW + 600.0), Some("long"))
    );
    f.advance(500.0);
    assert_eq!(f.health(&h), Health::Live);
    f.store.agent_beat("agent-1", &ended("long")).unwrap();
    f.advance(LIVE_SECONDS + 1.0);
    assert_eq!(f.health(&h), Health::Stale);
}
}

on_both! {
fn a_shorter_call_does_not_cut_a_longer_mark(backend: Backend) {
    let (f, h) = executing(backend);
    f.store.bind_agent("agent-1", &h, false).unwrap();
    f.store.agent_beat("agent-1", &busy(600.0, "long")).unwrap();
    f.store.agent_beat("agent-1", &busy(10.0, "short")).unwrap();
    f.advance(500.0);
    assert_eq!(f.health(&h), Health::Live);
}
}

on_both! {
fn a_busy_mark_a_beat_set_and_nobody_cleared_lapses_to_lost(backend: Backend) {
    let (f, h) = executing(backend);
    f.store.bind_agent("agent-1", &h, false).unwrap();
    f.store.agent_beat("agent-1", &busy(600.0, "long")).unwrap();
    f.advance(600.0 + GONE);
    assert_eq!(f.health(&h), Health::Lost);
}
}

on_both! {
fn a_child_that_stops_beating_while_it_holds_a_claim_is_lost(backend: Backend) {
    let (f, h) = executing(backend);
    f.store.bind_agent("agent-1", &h, false).unwrap();
    f.store.agent_beat("agent-1", &beat()).unwrap();
    f.advance(GONE);
    assert_eq!(f.health(&h), Health::Lost);
    assert_eq!(f.store.claimable(&h).unwrap(), ["sim/jump"]);
}
}

on_both! {
fn a_session_with_a_live_monitor_is_never_bound(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    f.alive(&[41]);
    let h = f.session_with(monitored("orchestrator", HOST));
    assert!(!f.store.bind_agent("agent-1", &h, false).unwrap());
    let remote = f.session_with(monitored("worker", "some-other-machine")); // a monitor we cannot probe counts as live
    assert!(!f.store.bind_agent("agent-1", &remote, false).unwrap());
    f.alive(&[]);
    assert!(f.store.bind_agent("agent-1", &h, false).unwrap()); // its monitor is gone
}
}

on_both! {
fn a_full_session_keeps_the_first_handle_it_names(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let (mine, theirs) = (f.session("worker"), f.session("worker"));
    assert!(f.store.bind_agent("s-pane", &mine, true).unwrap());
    assert!(!f.store.bind_agent("s-pane", &theirs, true).unwrap()); // registering or relaying for a child
    assert_eq!(f.store.handle_for_agent("s-pane").unwrap(), Some(mine));
}
}

on_both! {
fn a_session_naming_a_monitored_handle_lets_go_and_is_barred(backend: Backend) {
    let f = Fixture::on(backend, crate::common::Seed::default());
    f.alive(&[41]);
    let orch = f.session_with(monitored("orchestrator", HOST));
    let child = f.session("worker");
    assert!(f.store.bind_agent("s-orch", &child, true).unwrap());
    f.store.agent_beat("s-orch", &busy(600.0, "c")).unwrap();
    assert!(!f.store.bind_agent("s-orch", &orch, true).unwrap()); // it proves itself monitored
    let row = f.store.session(&child).unwrap().row;
    assert_eq!(
        (row.agent, row.busy_until, row.busy_call),
        (None, None, None)
    );
    assert!(!f.store.bind_agent("s-orch", &child, true).unwrap());
    assert!(!f.store.bind_agent("s-orch", &orch, true).unwrap());
    assert_eq!(f.store.agent_beat("s-orch", &beat()).unwrap(), None);
    assert_eq!(f.store.raw().table("barred_key").count().unwrap(), 1);
    assert_eq!(f.store.raw().row("barred_key", "s-orch").field("key").unwrap(), serde_json::json!("s-orch"));
}
}

on_both! {
fn only_a_tool_call_beat_moves_last_tool(backend: Backend) {
    let f = Fixture::on(backend, Seed::default().task("sim/jump", "ready"));
    let h = f.session("worker");
    assert_eq!(f.store.session(&h).unwrap().row.last_tool, None);
    f.advance(20.0);
    f.store.touch(&h).unwrap(); // the monitor's beat
    f.store.claim("sim/jump", &h).unwrap(); // a board command
    let row = f.store.session(&h).unwrap().row;
    assert_eq!((row.last_tool, row.last_seen), (None, f.now()));
    f.store.bind_agent("agent-1", &h, false).unwrap();
    f.advance(30.0);
    f.store.agent_beat("agent-1", &beat()).unwrap();
    assert_eq!(f.store.session(&h).unwrap().row.last_tool, Some(f.now()));
    f.advance(20.0);
    f.store.touch(&h).unwrap();
    assert_eq!(
        f.store.session(&h).unwrap().row.last_tool,
        Some(f.now() - 20.0)
    );
}
}
