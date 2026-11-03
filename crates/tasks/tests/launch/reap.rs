//! Idle pane sweep with deterministic board-clock intervals.

use crate::common::RawWrite;
use crate::{Folder, Hooked, World, git};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tasks::cli::Env;
use tasks::health::LIVE_SECONDS;
use tasks::launch::herdr::Herdr;
use tasks::launch::reap::{self, Closed, REAP_SECONDS, Reap, Sweep, lock_path, state_path};
use tasks::launch::{Ask, LaunchError, Processes, restore as restore_session};
use tasks::{Error, NewLaunch, NewSession, NewTask, Store};

#[test]
fn failed_or_incomplete_presence_rounds_hold_work_until_the_cap_and_confirmation_resets_it() {
    let w = World::new();
    let mut workers = Vec::new();
    for i in 0..2 {
        let slug = format!("tools/presence-{i}");
        w.f.task(&slug);
        let handle = w.sub_session("worker", &w.orch);
        w.store().claim(&slug, &handle).unwrap();
        let thread = format!("abcdef0{i}");
        w.agent(Agent {
            status: "working",
            extra: json!({"agent":"codex", "agent_session":{"agent":"codex", "kind":"id", "value":thread}}),
            ..Agent::named(&handle)
        });
        // Seed the persisted registration, without invoking the real Codex queue transport.
        w.store().client().query(
            "UPSERT type::record('callback_state', $handle) SET handle = $handle, address = $address, host = $host, confirmed_until = $until, lease = $lease RETURN NONE;",
            json!({"handle":handle, "address":format!("codex:{thread}"), "host":"this-host", "until":w.f.now() + 390.0, "lease":w.f.now()+1_000_000.0}),
        ).unwrap();
        workers.push((slug, handle));
    }
    w.f.task("tools/presence-finished");
    let finished = w.executor_on("done", "tools/presence-finished");
    w.store().client().query(
        "UPSERT type::record('callback_state', $handle) SET handle = $handle, address = 'codex:abcdef0f', host = 'this-host', confirmed_until = $until, lease = $lease RETURN NONE;",
        json!({"handle":finished, "until":w.f.now()+390.0, "lease":w.f.now()+1_000_000.0}),
    ).unwrap();
    let before = tasks::cli::attach::observe(w.store(), &w.orch, "orchestrator").unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let (release, held) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let (_stream, _) = listener.accept().unwrap();
        held.recv().unwrap();
    });
    let now = w.f.now();
    let failed = tasks::Store::unchecked(
        tasks::rpc::Client::new(&addr, "sf", w.store().client().database())
            .waiting(Duration::from_millis(30)),
        Box::new(move || now),
        Box::new(|_| true),
        "this-host",
    );
    let m = w.machine();
    let how = Reap {
        state: w.reap_state(),
        apply: true,
        processes: &*m.processes,
        current_pane: None,
        caller: None,
        min_idle: REAP_SECONDS,
    };
    let began = Instant::now();
    let error = presence_round(&failed, &w.herdr(), &how, &mut Vec::new()).unwrap_err();
    release.send(()).unwrap();
    server.join().unwrap();
    assert!(began.elapsed() < Duration::from_secs(5));
    assert!(error.to_string().contains("did not answer"), "{error}");

    w.f.advance(601.0);
    for (slug, handle) in &workers {
        assert_eq!(
            w.store().session(handle).unwrap().health,
            tasks::Health::Unknown
        );
        assert_eq!(
            w.store().get(slug).unwrap().roles[0].health,
            tasks::Health::Unknown
        );
        let other = w.f.session("worker");
        assert!(
            w.store().claim(slug, &other).is_err(),
            "unknown signal must retain ownership"
        );
    }
    let digest = tasks::cli::pull::digest(w.store(), &[], None, w.f.now(), &[]).unwrap();
    assert_eq!(digest.executing, 2);
    assert_eq!(digest.count("in_progress_orphaned"), 0);
    assert_eq!(
        w.store().session(&finished).unwrap().health,
        tasks::Health::Unknown
    );
    let mut holding = workers
        .iter()
        .map(|(_, handle)| handle.clone())
        .collect::<Vec<_>>();
    holding.sort();
    assert_eq!(
        digest.liveness_unknown, holding,
        "finished roles are not actionable work"
    );
    assert!(
        digest
            .lines()
            .iter()
            .any(|line| line.contains("native presence sweep overdue or failed"))
    );
    let after = tasks::cli::attach::observe(w.store(), &w.orch, "orchestrator").unwrap();
    let mut events = Vec::new();
    tasks::cli::attach::diff(w.store(), &before, &after, &w.orch, &mut |line| {
        events.push(line.to_string())
    })
    .unwrap();
    assert_eq!(events.len(), 2, "{events:?}");
    assert!(
        events
            .iter()
            .all(|line| line.starts_with("LIVENESS UNKNOWN ")),
        "{events:?}"
    );
    let board = w.store().board().unwrap();
    let view = tasks::cli::agents::describe_board(
        w.store(),
        &tasks::cli::probes::machine(&w.env),
        w.f.now(),
        false,
        &board,
    )
    .unwrap();
    for (_, handle) in &workers {
        let agent = view
            .agents
            .iter()
            .find(|a| &a.handle == handle)
            .expect("unknown agents remain visible");
        assert_eq!(agent.health, tasks::Health::Unknown);
        assert_ne!(agent.state, "gone");
    }

    w.f.advance(389.0); // exactly ten minutes since confirmation expired
    for (_, handle) in &workers {
        assert_eq!(w.f.health(handle), tasks::Health::Unknown);
    }
    w.f.advance(1.0);
    for (slug, handle) in &workers {
        assert_eq!(w.f.health(handle), tasks::Health::Lost);
        assert_eq!(
            w.store().get(slug).unwrap().roles[0].health,
            tasks::Health::Lost
        );
    }
    assert_eq!(w.f.health(&finished), tasks::Health::Left);
    let lost = tasks::cli::attach::observe(w.store(), &w.orch, "orchestrator").unwrap();
    events.clear();
    tasks::cli::attach::diff(w.store(), &after, &lost, &w.orch, &mut |line| {
        events.push(line.to_string())
    })
    .unwrap();
    assert_eq!(events.len(), 2, "{events:?}");
    assert!(
        events.iter().all(|line| line.starts_with("LOST ")),
        "{events:?}"
    );
    let again = tasks::cli::attach::observe(w.store(), &w.orch, "orchestrator").unwrap();
    events.clear();
    tasks::cli::attach::diff(w.store(), &lost, &again, &w.orch, &mut |line| {
        events.push(line.to_string())
    })
    .unwrap();
    assert!(events.is_empty(), "LOST is announced once: {events:?}");
    let digest = tasks::cli::pull::digest(w.store(), &[], None, w.f.now(), &[]).unwrap();
    assert_eq!(digest.count("in_progress_orphaned"), 2);
    assert!(digest.liveness_unknown.is_empty());

    // A real confirmation after expiry revives the workers and starts a fresh grace interval.
    presence_round(w.store(), &w.herdr(), &how, &mut Vec::new()).unwrap();
    for (_, handle) in &workers {
        assert_eq!(w.f.health(handle), tasks::Health::Live);
    }
    w.f.advance(601.0);
    for reply in [
        json!({}),
        json!({"agents":null}),
        json!({"agents":{}}),
        json!({"agents":[null]}),
    ] {
        let reply = json!({"result":reply}).to_string();
        let mut incomplete = Hooked::new(&w);
        incomplete.instead =
            Box::new(move |args| (args == ["agent", "list"]).then(|| Ok(reply.clone())));
        let mut warnings = Vec::new();
        presence_round(w.store(), &incomplete, &how, &mut warnings).unwrap();
        assert!(
            String::from_utf8(warnings)
                .unwrap()
                .contains("presence is unknown")
        );
        for (_, handle) in &workers {
            assert_eq!(
                w.store().session(handle).unwrap().health,
                tasks::Health::Unknown
            );
            assert!(w.store().callbacks().unwrap().contains_key(handle));
        }
    }
    let inventory = w.state();
    for missing in ["agent_session", "pane_id"] {
        for replace_known in [true, false] {
            w.set_state(inventory.clone());
            presence_round(w.store(), &w.herdr(), &how, &mut Vec::new()).unwrap();
            w.edit_panes(|panes| {
                let mut anonymous = panes[0].clone();
                if !replace_known {
                    anonymous["pane_id"] = json!("w1:anonymous");
                    anonymous["agent_session"]["value"] = json!("unidentified-session");
                }
                anonymous.as_object_mut().unwrap().remove(missing);
                if replace_known {
                    panes[0] = anonymous;
                } else {
                    panes.push(anonymous);
                }
            });
            w.f.advance(601.0);
            let mut warnings = Vec::new();
            presence_round(w.store(), &w.herdr(), &how, &mut warnings).unwrap();
            assert!(
                String::from_utf8(warnings)
                    .unwrap()
                    .contains("native presence incomplete")
            );
            for (i, (_, handle)) in workers.iter().enumerate() {
                let expected = if replace_known && i == 0 {
                    tasks::Health::Unknown
                } else {
                    tasks::Health::Live
                };
                assert_eq!(
                    w.store().session(handle).unwrap().health,
                    expected,
                    "a partial inventory confirms every identified worker but never retires the missing one"
                );
                assert!(w.store().callbacks().unwrap().contains_key(handle));
            }
        }
    }
    w.set_state(inventory);
    presence_round(w.store(), &w.herdr(), &how, &mut Vec::new()).unwrap();
    for (_, handle) in &workers {
        assert_eq!(
            w.store().session(handle).unwrap().health,
            tasks::Health::Live
        );
        assert!(w.store().callbacks().unwrap().contains_key(handle));
    }
    w.edit_panes(|panes| panes.clear());
    w.f.advance(601.0);
    presence_round(w.store(), &w.herdr(), &how, &mut Vec::new()).unwrap();
    for (_, handle) in &workers {
        assert!(!w.store().callbacks().unwrap().contains_key(handle));
        assert_eq!(
            w.store().session(handle).unwrap().health,
            tasks::Health::Lost
        );
    }
}

/// A claude agent's pane, as [`World::agent`] makes it.
struct Agent<'a> {
    handle: Option<&'a str>,
    status: &'a str,
    /// What runs under its claude process.
    procs: &'a [&'a str],
    /// Named by claude's `-n` flag, else by herdr's agent name.
    by_flag: bool,
    screen: &'a str,
    extra: Value,
}

impl<'a> Agent<'a> {
    fn named(handle: &'a str) -> Agent<'a> {
        Agent {
            handle: Some(handle),
            status: "idle",
            procs: &[],
            by_flag: true,
            screen: "",
            extra: json!({}),
        }
    }

    fn unnamed() -> Agent<'a> {
        Agent {
            handle: None,
            ..Agent::named("")
        }
    }
}

/// How one sweep runs, besides the world's state file and `ps` table.
#[derive(Clone, Copy, Default)]
struct How<'a> {
    dry: bool,
    current_pane: Option<&'a str>,
    caller: Option<&'a str>,
    processes: Option<&'a Processes>,
}

impl World {
    /// A claude pane (with caffeinate beside claude, as a launched one has), whose claude process
    /// runs what `a` says: its id.
    fn agent(&self, a: Agent<'_>) -> String {
        let pid = self.new_pid();
        let pane = format!("w1:p{pid}");
        let mut argv = vec!["claude", "--model", "opus"];
        if let (Some(handle), true) = (a.handle, a.by_flag) {
            argv.extend(["-n", handle]);
        }
        let mut row = json!({
            "pane_id": pane, "tab_id": "w1:t1", "agent": "claude", "agent_status": a.status,
            "focused": false, "screen": a.screen, "shell_pid": pid + 50_000,
            "agent_session": {"agent": "claude", "kind": "id", "value": format!("sess-{pane}")},
            "foreground": [
                {"argv": ["caffeinate", "-i", "-t", "300"], "name": "caffeinate", "pid": 1},
                {"argv": argv, "cmdline": argv.join(" "), "name": "claude", "pid": pid},
            ],
        });
        if let (Some(handle), false) = (a.handle, a.by_flag) {
            row["name"] = json!(handle);
        }
        for (key, value) in a.extra.as_object().into_iter().flatten() {
            row[key] = value.clone();
        }
        self.edit_panes(|panes| panes.push(row));
        self.run_under(pid, a.procs);
        pane
    }

    /// A worker that executed `slug`, which is now `status`: its handle.
    fn executor_on(&self, status: &str, slug: &str) -> String {
        let handle = self.f.session("worker");
        let store = self.store();
        store.claim(slug, &handle).unwrap();
        if status == "blocked" {
            store.move_to(slug, "blocked", &handle).unwrap();
        } else if status == "review" || status == "done" {
            store.move_to(slug, "review", &handle).unwrap();
            if status == "done" {
                let reviewer = self.f.session("reviewer");
                store.take_role(slug, &reviewer, "reviewer").unwrap();
                store.verdict(slug, &reviewer, "approve", "ok").unwrap();
            }
        }
        handle
    }

    fn sub_session(&self, kind: &str, parent: &str) -> String {
        self.f.session_with(NewSession {
            parent: Some(parent.into()),
            ..NewSession::new(kind)
        })
    }

    fn reap_state(&self) -> PathBuf {
        self.dir().join("reap.json")
    }

    fn reap_data(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.reap_state()).unwrap()).unwrap()
    }

    /// One sweep through `herdr` as `how` says, then, if it swept, the timer's interval: what it
    /// found (None: the lock was held), and what it warned of.
    fn once(&self, herdr: &dyn Herdr, how: How<'_>) -> (Option<Sweep>, String) {
        let m = self.machine();
        let reap = Reap {
            state: self.reap_state(),
            apply: !how.dry,
            processes: how.processes.unwrap_or(&*m.processes),
            current_pane: how.current_pane,
            caller: how.caller,
            min_idle: REAP_SECONDS,
        };
        let mut warn = Vec::new();
        let swept = reap::sweep(self.store(), herdr, &reap, &mut warn).expect("the sweep runs");
        if swept.is_some() {
            self.f.advance(REAP_SECONDS);
        }
        (swept, String::from_utf8(warn).unwrap())
    }

    /// Wait for the previous sweep's lock to be released, then complete exactly one round.
    fn sweep_with(&self, herdr: &dyn Herdr, how: How<'_>) -> (Sweep, String) {
        wait_for_sweep(&self.reap_state());
        let (swept, warned) = self.once(herdr, how);
        (swept.expect("the sweep records its round"), warned)
    }

    fn sweep(&self) -> Sweep {
        self.sweep_as(How::default())
    }

    fn sweep_as(&self, how: How<'_>) -> Sweep {
        let (swept, warned) = self.sweep_with(&self.herdr(), how);
        assert_eq!(warned, "");
        swept
    }

    /// Two sweeps in a row: a pane idle in both is closed by the second.
    fn twice(&self, how: How<'_>) -> Sweep {
        self.sweep_as(how);
        self.sweep_as(how)
    }

    fn closes(&self) -> Vec<Vec<String>> {
        self.calls_of(&["pane", "close"])
    }

    /// Kept through two sweeps, with nothing closed.
    fn kept(&self, pane: &str, how: How<'_>) {
        self.twice(how);
        assert!(self.has_pane(pane));
        assert_eq!(self.closes(), Vec::<Vec<String>>::new());
    }
}

/// The sweep's lock, taken by the test as another sweep would hold it.
fn sweep_lock(state: &Path) -> File {
    let held = File::options()
        .append(true)
        .create(true)
        .open(lock_path(state))
        .unwrap();
    held.lock().unwrap();
    held
}

fn hold_lock(w: &World) -> File {
    sweep_lock(&w.reap_state())
}

/// Another test thread can fork while this test holds the lock itself ([`hold_lock`], let go by
/// closing): the child retains that lock until exec, even after the test drops it. Wait for that
/// release in the kernel, without retrying a skipped round. Explicitly unlock our barrier so
/// another fork cannot prolong it in turn. No other sweep runs on this world's state, so the
/// following sweep can take the released lock.
fn wait_for_sweep(state: &Path) {
    sweep_lock(state).unlock().unwrap();
}

/// A presence assertion needs a completed round, not the legitimate no-op of a contended sweep.
fn presence_round(
    store: &Store,
    herdr: &dyn Herdr,
    how: &Reap<'_>,
    warn: &mut dyn Write,
) -> Result<Sweep, Error> {
    wait_for_sweep(&how.state);
    reap::sweep(store, herdr, how, warn).map(|swept| swept.expect("the sweep records its round"))
}

fn panes(swept: &Sweep) -> Vec<&str> {
    swept.closed.iter().map(|c| c.pane.as_str()).collect()
}

fn restore(slug: &str, handle: &str) -> String {
    format!("just tasks restore {slug} {handle}")
}

// -- what closes ------------------------------------------------------------------------

#[test]
fn a_finished_idle_agent_is_closed_on_the_second_idle_sweep() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let pane = w.agent(Agent::named(&handle));
    let first = w.sweep();
    assert_eq!(
        first,
        Sweep {
            closed: vec![],
            idle: vec![handle.clone()]
        }
    );
    assert!(w.has_pane(&pane));
    let second = w.sweep();
    assert_eq!(
        second.closed,
        [Closed {
            pane: pane.clone(),
            handle: handle.clone(),
            restore: restore("sim/jump", &handle),
        }]
    );
    assert!(!w.has_pane(&pane));
}

#[test]
fn an_agent_that_worked_between_two_idle_sweeps_starts_its_count_again() {
    let w = World::new();
    let pane = w.agent(Agent::named(&w.executor_on("done", "sim/jump")));
    w.sweep();
    w.edit_pane(&pane, |p| {
        p.insert("agent_status".into(), "working".into());
    });
    w.sweep();
    w.edit_pane(&pane, |p| {
        p.insert("agent_status".into(), "idle".into());
    });
    w.sweep();
    assert!(w.has_pane(&pane));
    w.sweep();
    assert!(!w.has_pane(&pane));
}

#[test]
fn a_done_status_counts_as_idle() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let pane = w.agent(Agent {
        status: "done",
        ..Agent::named(&handle)
    });
    w.twice(How::default());
    assert!(!w.has_pane(&pane));
}

#[test]
fn an_agent_stuck_on_the_folder_trust_prompt_is_closed() {
    let w = World::new();
    let handle = w.f.session("worker");
    let pane = w.agent(Agent {
        status: "blocked",
        screen: "Do you trust this folder?\n 1. Yes, proceed",
        ..Agent::named(&handle)
    });
    w.twice(How::default());
    assert!(!w.has_pane(&pane));
    assert!(
        w.calls().contains(
            &["pane", "read", &pane, "--lines", "30"]
                .map(String::from)
                .to_vec()
        )
    );
}

#[test]
fn an_agent_blocked_on_anything_else_is_kept() {
    let w = World::new();
    let handle = w.f.session("worker");
    let pane = w.agent(Agent {
        status: "blocked",
        screen: "Allow this command?",
        ..Agent::named(&handle)
    });
    w.kept(&pane, How::default());
}

#[test]
fn the_executor_of_a_task_in_review_waits_for_its_reviewers_and_closes_once_it_is_done() {
    let w = World::new();
    let handle = w.executor_on("review", "sim/jump");
    let pane = w.agent(Agent::named(&handle));
    w.kept(&pane, How::default());
    let reviewer = w.f.session("reviewer");
    w.store()
        .take_role("sim/jump", &reviewer, "reviewer")
        .unwrap();
    w.store()
        .verdict("sim/jump", &reviewer, "approve", "ok")
        .unwrap();
    assert_eq!(panes(&w.twice(How::default())), [pane.as_str()]);
}

#[test]
fn a_post_to_an_executor_whose_pane_was_closed_tells_the_orchestrator_until_it_is_restored() {
    let w = World::new();
    w.store()
        .take_role("sim/jump", &w.orch, "orchestrator")
        .unwrap();
    let s = w
        .try_launch(&Ask {
            kind: Some("claude".into()),
            ..w.ask("sim/jump")
        })
        .unwrap()
        .row;
    w.store().claim("sim/jump", &s.handle).unwrap();
    w.store().move_to("sim/jump", "review", &s.handle).unwrap();
    // As the sweep closed it before an executor in review was kept: the pane goes, then its beat ages.
    w.edit_panes(|panes| panes.retain(|p| p["pane_id"] != s.pane.as_str()));
    let why = "idle for two sweeps, its work finished";
    w.store()
        .close_task_session("sim/jump", &s.handle, why, &restore("sim/jump", &s.handle))
        .unwrap();
    w.f.advance(LIVE_SECONDS + 1.0);
    let reviewer = w.f.session("reviewer");
    w.store()
        .take_role("sim/jump", &reviewer, "reviewer")
        .unwrap();
    let told = || -> Vec<String> {
        let inbox = w.store().inbox(&w.orch).unwrap();
        inbox
            .into_iter()
            .filter(|m| m.sender == "board")
            .map(|m| m.text)
            .collect()
    };
    let first = w
        .store()
        .say("sim/jump", &reviewer, "the hash moved", Some("@executor"))
        .unwrap();
    assert_eq!(
        told(),
        [format!(
            "#{} from {reviewer} to @executor reaches no live executor: {}'s pane was closed ({why}); {}",
            first.id,
            s.handle,
            restore("sim/jump", &s.handle)
        )]
    );
    restore_session(w.store(), &w.herdr(), &w.machine(), "sim/jump", None, None).unwrap();
    let second = w
        .store()
        .say(
            "sim/jump",
            &reviewer,
            "and the trace line",
            Some("@executor"),
        )
        .unwrap();
    assert_eq!(
        told().len(),
        1,
        "the restored executor reads it: nobody is told"
    );
    let read: Vec<i64> = w
        .store()
        .inbox(&s.handle)
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(read, [first.id, second.id]);
}

#[test]
fn a_handle_named_only_by_herdr_is_matched_too() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let pane = w.agent(Agent {
        by_flag: false,
        ..Agent::named(&handle)
    });
    w.twice(How::default());
    assert!(!w.has_pane(&pane));
}

#[test]
fn a_monitor_and_helpers_under_claude_are_not_work() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let pane = w.agent(Agent {
        procs: &[
            "caffeinate -i -t 300",
            "node /x/mcp-server.js",
            "codex --profile e",
            "codex resume THREAD 'a prompt containing exec'",
            "codex -- exec",
            "codex -- e",
            "/bin/zsh -c just tasks-attach --resume a",
            "just tasks-attach --resume a",
            "/repo/.git/sf-cargo/bin/tasks-0123456789abcdef attach --resume a",
            "/repo/.git/sf-cargo/bin/tasks-0123456789abcdef api --lines",
            "/x/.target/sf-bin/tasks-0123456789abcdef api",
            "/repo/.target/bin/tasks beat",
            "/usr/bin/Python -u -B tools/tasks.py attach --resume a",
            "just tasks inbox",
        ],
        ..Agent::named(&handle)
    });
    w.twice(How::default());
    assert!(!w.has_pane(&pane));
}

// -- what is kept -----------------------------------------------------------------------

#[test]
fn a_working_agent_is_kept() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let pane = w.agent(Agent {
        status: "working",
        ..Agent::named(&handle)
    });
    w.kept(&pane, How::default());
}

#[test]
fn an_executor_on_an_in_progress_or_blocked_task_is_kept() {
    for status in ["in_progress", "blocked"] {
        let w = World::new();
        let pane = w.agent(Agent::named(&w.executor_on(status, "sim/jump")));
        w.kept(&pane, How::default());
    }
}

#[test]
fn a_reviewer_of_a_task_in_review_is_kept() {
    let w = World::new();
    w.executor_on("review", "sim/jump");
    let reviewer = w.f.session("worker");
    w.store()
        .take_role("sim/jump", &reviewer, "reviewer")
        .unwrap();
    let pane = w.agent(Agent::named(&reviewer));
    w.kept(&pane, How::default());
}

#[test]
fn a_busy_descendant_keeps_the_pane() {
    let w = World::new();
    let first = w.executor_on("done", "sim/jump");
    for (n, command) in [
        "just verify",
        "/opt/homebrew/bin/godot --headless -s x.gd",
        "cargo build",
        "python3 -B tools/run_tool_suites.py",
        "just test-sim SUITE=jump",
        "rustc --edition 2021 x.rs",
    ]
    .into_iter()
    .enumerate()
    {
        let handle = if n == 0 {
            first.clone()
        } else {
            w.f.session("worker")
        };
        let pane = w.agent(Agent {
            procs: &["/bin/zsh -c x", command],
            ..Agent::named(&handle)
        });
        w.twice(How::default());
        assert!(w.has_pane(&pane), "{command}");
    }
    assert_eq!(w.closes(), Vec::<Vec<String>>::new());
}

#[test]
fn an_unnamed_pane_is_kept_it_may_be_the_owners() {
    let w = World::new();
    let pane = w.agent(Agent::unnamed());
    w.kept(&pane, How::default());
}

#[test]
fn a_name_that_is_no_board_session_is_kept() {
    let w = World::new();
    let pane = w.agent(Agent::named("someone-else"));
    w.kept(&pane, How::default());
}

#[test]
fn an_orchestrator_or_a_chat_is_kept() {
    for kind in ["orchestrator", "chat"] {
        let w = World::new();
        let pane = w.agent(Agent::named(&w.f.session(kind)));
        w.kept(&pane, How::default());
    }
}

#[test]
fn the_callers_own_pane_and_handle_are_kept() {
    let w = World::new();
    let mine = w.executor_on("done", "sim/jump");
    let pane = w.agent(Agent::named(&mine));
    w.kept(
        &pane,
        How {
            current_pane: Some(&pane),
            ..How::default()
        },
    );
    w.kept(
        &pane,
        How {
            caller: Some(&mine),
            ..How::default()
        },
    );
}

#[test]
fn the_focused_pane_is_kept() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let pane = w.agent(Agent {
        extra: json!({"focused": true}),
        ..Agent::named(&handle)
    });
    w.kept(&pane, How::default());
}

#[test]
fn an_agent_in_another_workspace_is_kept_and_agent_list_takes_no_workspace_flag() {
    let mut w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let pane = w.agent(Agent {
        extra: json!({"workspace_id": "w1"}),
        ..Agent::named(&handle)
    });
    w.set("HERDR_WORKSPACE_ID", "w9"); // the real `agent list` refuses --workspace
    w.kept(&pane, How::default());
    assert!(
        w.calls()
            .contains(&vec!["agent".to_string(), "list".to_string()])
    );
}

#[test]
fn alternating_workspace_sweeps_preserve_each_others_first_seen_marks() {
    let mut w = World::new();
    let one = w.f.session("worker");
    let two = w.f.session("worker");
    let first = w.agent(Agent {
        extra: json!({"workspace_id": "w1"}),
        ..Agent::named(&one)
    });
    let second = w.agent(Agent {
        extra: json!({"workspace_id": "w2"}),
        ..Agent::named(&two)
    });
    w.set("HERDR_WORKSPACE_ID", "w1");
    assert_eq!(w.sweep().idle, [one]);
    let first_marks = w.reap_data()["idle"].as_object().unwrap().clone();
    assert_eq!(first_marks.len(), 1);

    w.set("HERDR_WORKSPACE_ID", "w2");
    assert_eq!(w.sweep().idle, [two]);
    let both = w.reap_data()["idle"].as_object().unwrap().clone();
    assert_eq!(both.len(), 2);
    for (key, since) in &first_marks {
        assert_eq!(both.get(key), Some(since));
    }
    let second_marks: serde_json::Map<String, Value> = both
        .into_iter()
        .filter(|(key, _)| !first_marks.contains_key(key))
        .collect();
    assert_eq!(second_marks.len(), 1);

    w.set("HERDR_WORKSPACE_ID", "w1");
    assert_eq!(panes(&w.sweep()), [first.as_str()]);
    assert!(w.has_pane(&second));
    assert_eq!(w.reap_data()["idle"], json!(second_marks));

    w.set("HERDR_WORKSPACE_ID", "w2");
    assert_eq!(panes(&w.sweep()), [second.as_str()]);
    assert_eq!(w.reap_data()["idle"], json!({}));
}

#[test]
fn a_pane_whose_processes_cannot_be_read_is_kept() {
    let w = World::new();
    let broken: &Processes = &|_| Err(LaunchError("ps printed nothing".into()));
    let pane = w.agent(Agent::named(&w.executor_on("done", "sim/jump")));
    w.kept(
        &pane,
        How {
            processes: Some(broken),
            ..How::default()
        },
    );
}

#[test]
fn a_dry_run_closes_nothing_and_leaves_the_count_alone() {
    let w = World::new();
    let pane = w.agent(Agent::named(&w.executor_on("done", "sim/jump")));
    w.sweep();
    let dry = w.sweep_as(How {
        dry: true,
        ..How::default()
    });
    assert_eq!(panes(&dry), [pane.as_str()]);
    assert_eq!(dry.closed[0].restore, "");
    assert!(w.has_pane(&pane));
    assert_eq!(w.closes(), Vec::<Vec<String>>::new());
    assert!(w.store().all_task_sessions().unwrap().is_empty());
    w.sweep();
    assert!(!w.has_pane(&pane));
}

#[test]
fn a_first_dry_run_does_not_count_as_a_sweep() {
    let w = World::new();
    let pane = w.agent(Agent::named(&w.executor_on("done", "sim/jump")));
    w.sweep_as(How {
        dry: true,
        ..How::default()
    });
    assert!(!w.reap_state().exists());
    w.sweep();
    assert!(w.has_pane(&pane));
}

// -- what the board records -------------------------------------------------------------

#[test]
fn a_close_closes_the_launched_session_with_one_line_on_its_thread() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let pane = w.agent(Agent::named(&handle));
    w.store()
        .record_launch(&NewLaunch {
            slug: "sim/jump".into(),
            handle: handle.clone(),
            role: "executor".into(),
            claude_session: format!("sess-{pane}"),
            worktree: "/wt".into(),
            branch: "sim/jump".into(),
            pane: pane.clone(),
            ..NewLaunch::default()
        })
        .unwrap();
    let before = w.f.said("sim/jump").len();
    w.twice(How::default());
    let [s] = <[_; 1]>::try_from(w.store().sessions_of(&handle).unwrap()).unwrap();
    assert!(s.closed.is_some());
    assert_eq!(s.restore, restore("sim/jump", &handle));
    let said = w.f.said("sim/jump");
    let [line] = <&[String; 1]>::try_from(&said[before..]).unwrap();
    assert!(
        line.contains(&format!("closed the pane of {handle}")),
        "{line}"
    );
}

#[test]
fn an_unlaunched_agent_is_recorded_on_its_last_task_in_one_line() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    w.task("sim/dock", None);
    w.f.advance(60.0);
    w.store().claim("sim/dock", &handle).unwrap();
    w.store().move_to("sim/dock", "review", &handle).unwrap();
    let reviewer = w.f.session("reviewer");
    w.store()
        .take_role("sim/dock", &reviewer, "reviewer")
        .unwrap();
    w.store()
        .verdict("sim/dock", &reviewer, "approve", "ok")
        .unwrap();
    let pane = w.agent(Agent::named(&handle));
    let (jump, dock) = (w.f.said("sim/jump").len(), w.f.said("sim/dock").len());
    w.twice(How::default());
    assert_eq!(w.f.said("sim/jump").len(), jump);
    let said = w.f.said("sim/dock");
    let [line] = <&[String; 1]>::try_from(&said[dock..]).unwrap();
    assert!(
        line.contains(&format!("closed the pane of {handle}")),
        "{line}"
    );
    let [s] = <[_; 1]>::try_from(w.store().sessions_of(&handle).unwrap()).unwrap();
    assert_eq!(
        (s.slug.as_str(), s.pane.as_str(), s.claude_session),
        ("sim/dock", pane.as_str(), format!("sess-{pane}"))
    );
    assert_eq!(s.role, "executor");
}

#[test]
fn an_agent_with_no_task_is_closed_with_nothing_to_record() {
    let w = World::new();
    let handle = w.f.session("worker");
    let pane = w.agent(Agent::named(&handle));
    let swept = w.twice(How::default());
    assert_eq!(
        swept.closed,
        [Closed {
            pane: pane.clone(),
            handle,
            restore: String::new(),
        }]
    );
    assert!(!w.has_pane(&pane));
    assert!(w.store().all_task_sessions().unwrap().is_empty());
}

#[test]
fn a_close_herdr_refuses_leaves_its_session_open() {
    let mut w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let pane = w.agent(Agent::named(&handle));
    w.sweep();
    w.set("FAKE_HERDR_FAIL", "pane close");
    let swept = w.sweep();
    assert_eq!(swept.closed, []);
    assert!(w.has_pane(&pane));
    let sessions = w.store().sessions_of(&handle).unwrap();
    assert!(!sessions.is_empty());
    assert!(sessions.iter().all(|s| s.closed.is_none()), "{sessions:?}");
}

#[test]
fn an_interrupted_close_is_retried_without_repeating_its_thread_line() {
    for launched in [false, true] {
        let w = World::new();
        let handle = w.executor_on("done", "sim/jump");
        let pane = w.agent(Agent::named(&handle));
        if launched {
            w.store()
                .record_launch(&NewLaunch {
                    slug: "sim/jump".into(),
                    handle: handle.clone(),
                    role: "executor".into(),
                    claude_session: format!("sess-{pane}"),
                    pane: pane.clone(),
                    log: false,
                    ..NewLaunch::default()
                })
                .unwrap();
        }
        w.sweep();
        let before = w.f.said("sim/jump").len();
        let mut interrupted = Hooked::new(&w);
        interrupted.instead = Box::new(|args| {
            if args[..2] == ["pane", "close"] {
                panic!("sweep stopped before the pane close");
            }
            None
        });
        let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            w.sweep_with(&interrupted, How::default())
        }));
        assert_eq!(
            stopped.unwrap_err().downcast_ref::<&str>(),
            Some(&"sweep stopped before the pane close")
        );
        let recorded = w.store().sessions_of(&handle).unwrap();
        assert_eq!(recorded.len(), 1);
        assert!(recorded[0].closed.is_some());
        assert!(w.has_pane(&pane));
        assert!(w.closes().is_empty());
        assert_eq!(w.f.said("sim/jump").len(), before + 1);
        let original = w.state();

        assert_eq!(
            w.sweep().closed,
            [Closed {
                pane: pane.clone(),
                handle: handle.clone(),
                restore: restore("sim/jump", &handle),
            }]
        );
        assert!(!w.has_pane(&pane));
        assert_eq!(w.store().sessions_of(&handle).unwrap(), recorded);
        assert_eq!(w.f.said("sim/jump").len(), before + 1);

        // Reusing the pane for a new conversation must write that conversation's own close.
        w.set_state(original);
        w.edit_pane(&pane, |p| {
            p["agent_session"]["value"] = json!("new-conversation");
        });
        assert_eq!(panes(&w.twice(How::default())), [pane.as_str()]);
        assert_eq!(w.f.said("sim/jump").len(), before + 2);
        assert_eq!(
            w.store().sessions_of(&handle).unwrap()[0].claude_session,
            "new-conversation"
        );
    }
}

// -- one sweeper at a time --------------------------------------------------------------

#[test]
fn a_sweep_while_another_holds_the_lock_does_nothing() {
    let w = World::new();
    let pane = w.agent(Agent::named(&w.executor_on("done", "sim/jump")));
    w.sweep();
    let held = hold_lock(&w);
    let calls = w.calls();
    for _ in 0..2 {
        assert_eq!(w.once(&w.herdr(), How::default()), (None, String::new()));
    }
    assert!(w.has_pane(&pane));
    assert_eq!(
        w.calls(),
        calls,
        "a sweep that found the lock held asks herdr nothing"
    );
    drop(held);
    w.sweep();
    assert!(!w.has_pane(&pane));
}

#[test]
fn two_sweeps_at_once_close_a_pane_once() {
    let w = World::new();
    let pane = w.agent(Agent::named(&w.executor_on("done", "sim/jump")));
    w.sweep();
    let second: RefCell<Option<Option<Sweep>>> = RefCell::new(None);
    let mut hooked = Hooked::new(&w);
    hooked.instead = Box::new(|args| {
        if args[..2] == ["agent", "list"] && second.borrow().is_none() {
            // the first sweep holds the lock, inside its sweep: a second comes at the same time
            let (swept, _) = w.once(&w.herdr(), How::default());
            *second.borrow_mut() = Some(swept);
        }
        None
    });
    let (first, warned) = w.sweep_with(&hooked, How::default());
    assert_eq!((second.borrow().clone(), warned.as_str()), (Some(None), ""));
    assert_eq!(panes(&first), [pane.as_str()]);
    assert_eq!(w.closes().len(), 1);
    let closed: Vec<_> = w
        .store()
        .all_task_sessions()
        .unwrap()
        .into_iter()
        .filter(|s| s.closed.is_some())
        .collect();
    assert_eq!(closed.len(), 1);
}

// -- where the state is -----------------------------------------------------------------

fn env_of(vars: &[(&str, &str)], cwd: PathBuf) -> Env {
    Env {
        vars: vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<BTreeMap<_, _>>(),
        cwd,
    }
}

#[test]
fn the_boards_own_is_in_sf_tasks_of_the_git_common_dir() {
    let dir = Folder::new();
    let repo = dir.0.join("repo");
    std::fs::create_dir_all(repo.join("deep")).unwrap();
    git(&repo, &["init", "-q"]);
    let env = env_of(&[], dir.0.clone());
    assert_eq!(
        state_path(&env, &repo.join("deep")),
        Ok(repo.join(".git").join("sf-tasks").join("reap.json"))
    );
}

#[test]
fn tasks_reap_state_names_it() {
    let env = env_of(
        &[
            ("TASKS_REAP_STATE", "/x/reap.json"),
            ("TASKS_SERVER", "127.0.0.1:9"),
        ],
        std::env::temp_dir(),
    );
    assert_eq!(
        state_path(&env, &std::env::temp_dir()),
        Ok(PathBuf::from("/x/reap.json"))
    );
}

#[test]
fn a_board_tasks_server_names_keeps_it_apart_from_the_live_boards() {
    let at = |db: &str| {
        let env = env_of(
            &[("TASKS_SERVER", "127.0.0.1:9"), ("TASKS_DATABASE", db)],
            std::env::temp_dir(),
        );
        state_path(&env, &std::env::temp_dir()).unwrap()
    };
    let (one, two) = (at("t1"), at("t2"));
    assert_ne!(one, two);
    assert_eq!(at("t1"), one, "the same board, the same file");
    assert_eq!(
        one.parent(),
        Some(std::env::temp_dir().join("sf-tasks-reap").as_path())
    );
    let name = one.file_name().unwrap().to_str().unwrap();
    assert!(
        name.len() == "reap-.json".len() + 16
            && name.starts_with("reap-")
            && name.ends_with(".json"),
        "{name}"
    );
    assert_eq!(lock_path(&one), one.with_file_name(format!("{name}.lock")));
}

#[test]
fn outside_a_git_repository_with_no_server_named_it_is_an_error() {
    let dir = Folder::new();
    let said = state_path(&env_of(&[], dir.0.clone()), &dir.0);
    assert!(
        matches!(&said, Err(why) if why.starts_with("not in a git repository (")
            && why.ends_with("): no task board to sweep")),
        "{said:?}"
    );
}

// -- what the review of b49c2081 found (lunar-egret, nimble-crane) ----------------------

#[test]
fn anything_not_known_harmless_under_claude_is_work() {
    let w = World::new();
    for command in [
        "blender -b -noaudio scene.blend -P build.py",
        "codex exec 'draw a ship'",
        "codex e 'draw a ship'",
        "codex --profile e exec 'draw a ship'",
        "codex exec 'draw an mcp-ship'",
        "CODEX e 'run just tasks inbox'",
        "ffmpeg -i a.wav b.ogg",
        "node x.js",
        "python3 /tmp/scratchpad/x.py",
        "/bin/zsh -c sleep 600",
        "sleep 600",
    ] {
        let handle = w.f.session("worker");
        let pane = w.agent(Agent {
            procs: &["caffeinate -i -t 300", command],
            ..Agent::named(&handle)
        });
        w.twice(How::default());
        assert!(w.has_pane(&pane), "{command}");
    }
    assert_eq!(w.closes(), Vec::<Vec<String>>::new());
}

#[test]
fn two_sweeps_with_no_time_between_them_close_nothing() {
    let w = World::new();
    let pane = w.agent(Agent::named(&w.executor_on("done", "sim/jump")));
    w.sweep();
    w.f.advance(-(REAP_SECONDS - 1.0)); // a second sweeper, 1 s after the first
    w.sweep();
    assert!(w.has_pane(&pane));
    w.f.advance(-2.0); // a third, 299 s after the pane was first seen idle
    w.sweep();
    assert!(w.has_pane(&pane));
    w.sweep(); // an interval and more since it was first seen idle
    assert!(!w.has_pane(&pane));
}

#[test]
fn invalid_idle_state_starts_a_fresh_idle_wait() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let pane = w.agent(Agent::named(&handle));
    w.sweep();
    let initial = w.reap_data();
    let marks = initial["idle"].as_object().unwrap();
    assert_eq!(marks.len(), 1);
    let key = marks.keys().next().unwrap();
    let mut invalid_states: Vec<String> = [
        json!(0),
        json!(-1),
        json!(w.f.now() + 100_000.0),
        json!("yesterday"),
        Value::Null,
        json!(true),
        json!([]),
        json!({}),
    ]
    .into_iter()
    .map(|invalid| {
        let mut data = initial.clone();
        data["idle"][key] = invalid;
        data.to_string()
    })
    .collect();
    invalid_states.extend([
        "{".into(),
        "[]".into(),
        "{}".into(),
        r#"{"idle":[]}"#.into(),
        format!(r#"{{"idle":{{{key:?}:1e400}}}}"#),
    ]);
    for invalid in invalid_states {
        let now = w.f.now();
        std::fs::write(w.reap_state(), &invalid).unwrap();
        let swept = w.sweep();
        assert_eq!(
            swept.idle.as_slice(),
            std::slice::from_ref(&handle),
            "{invalid}"
        );
        assert!(swept.closed.is_empty(), "{invalid}");
        assert!(w.has_pane(&pane), "{invalid}");
        assert_eq!(w.reap_data()["idle"][key], json!(now), "{invalid}");
    }
    assert!(w.closes().is_empty());
    assert_eq!(panes(&w.sweep()), [pane.as_str()]);
}

#[test]
fn a_pane_whose_subagent_holds_live_work_is_kept() {
    let w = World::new();
    w.executor_on("review", "sim/jump");
    let parent = w.f.session("worker");
    let child = w.sub_session("reviewer", &parent);
    w.store().take_role("sim/jump", &child, "reviewer").unwrap();
    let pane = w.agent(Agent::named(&parent));
    w.twice(How::default());
    assert!(w.has_pane(&pane));
}

#[test]
fn a_grandchild_holding_live_work_keeps_the_pane_too() {
    for middle_left in [false, true] {
        for status in ["in_progress", "blocked", "review"] {
            let w = World::new();
            let parent = w.f.session("worker");
            let child = w.sub_session("worker", &parent);
            let grandchild = w.sub_session("worker", &child);
            if status == "review" {
                w.executor_on("review", "sim/jump");
                w.store()
                    .take_role("sim/jump", &grandchild, "reviewer")
                    .unwrap();
            } else {
                w.store().claim("sim/jump", &grandchild).unwrap();
                if status == "blocked" {
                    w.store()
                        .move_to("sim/jump", "blocked", &grandchild)
                        .unwrap();
                }
            }
            if middle_left {
                w.store().leave(&child).unwrap();
                assert!(w.store().session(&child).unwrap().row.left_at.is_some());
            }
            let pane = w.agent(Agent::named(&parent));
            w.kept(&pane, How::default());
            w.store().leave(&grandchild).unwrap();
            assert_eq!(panes(&w.twice(How::default())), [pane.as_str()]);
        }
    }
}

#[test]
fn a_subagent_that_left_holds_nothing() {
    let w = World::new();
    let parent = w.f.session("worker");
    let child = w.sub_session("worker", &parent);
    w.store().claim("sim/jump", &child).unwrap();
    w.store().move_to("sim/jump", "review", &child).unwrap();
    w.store().leave(&child).unwrap();
    let pane = w.agent(Agent::named(&parent));
    w.twice(How::default());
    assert!(!w.has_pane(&pane));
}

#[test]
fn an_agent_prompted_during_the_sweep_is_kept() {
    let w = World::new();
    let pane = w.agent(Agent::named(&w.executor_on("done", "sim/jump")));
    w.sweep();
    let mut hooked = Hooked::new(&w);
    hooked.after = Box::new(|args| {
        if args[..2] == ["pane", "process-info"] {
            w.edit_pane(&pane, |p| {
                p.insert("agent_status".into(), "working".into());
            });
        }
    });
    let (swept, _) = w.sweep_with(&hooked, How::default());
    assert_eq!(swept.closed, []);
    assert!(w.has_pane(&pane));
}

#[test]
fn a_pane_focused_during_the_sweep_restarts_its_idle_wait() {
    for focus_after in [1, 2] {
        let w = World::new();
        let handle = w.executor_on("done", "sim/jump");
        let pane = w.agent(Agent::named(&handle));
        w.sweep();
        let reads = Cell::new(0);
        let mut hooked = Hooked::new(&w);
        hooked.after = Box::new(|args| {
            if args[..2] == ["pane", "process-info"] {
                reads.set(reads.get() + 1);
                if reads.get() == focus_after {
                    w.edit_pane(&pane, |p| {
                        p.insert("focused".into(), json!(true));
                    });
                }
            }
        });
        let (swept, warned) = w.sweep_with(&hooked, How::default());
        assert_eq!((swept.closed, warned), (vec![], String::new()));
        assert!(w.has_pane(&pane));
        assert!(w.closes().is_empty());
        assert!(w.store().sessions_of(&handle).unwrap().is_empty());
        w.edit_pane(&pane, |p| {
            p.insert("focused".into(), json!(false));
        });
        assert_eq!(w.sweep().idle, [handle]);
        assert!(w.has_pane(&pane));
        assert_eq!(panes(&w.sweep()), [pane.as_str()]);
    }
}

#[test]
fn a_live_reviewer_replacing_the_candidate_keeps_its_pane() {
    for new_conversation in [false, true] {
        let w = World::new();
        let finished = w.executor_on("done", "sim/jump");
        let pane = w.agent(Agent::named(&finished));
        w.task("sim/dock", None);
        w.executor_on("review", "sim/dock");
        let reviewer = w.f.session("reviewer");
        w.store()
            .take_role("sim/dock", &reviewer, "reviewer")
            .unwrap();
        w.sweep();
        let replaced = Cell::new(false);
        let mut hooked = Hooked::new(&w);
        hooked.after = Box::new(|args| {
            if args[..2] == ["pane", "process-info"] && !replaced.replace(true) {
                w.edit_pane(&pane, |p| {
                    if new_conversation {
                        p["agent_session"]["value"] = json!("replacement-reviewer");
                    }
                    p["foreground"][1]["argv"] = json!(["claude", "-n", reviewer]);
                    p["foreground"][1]["cmdline"] = json!(format!("claude -n {reviewer}"));
                });
            }
        });
        let (swept, warned) = w.sweep_with(&hooked, How::default());
        assert!(replaced.get());
        assert_eq!(w.store().get("sim/dock").unwrap().status, "review");
        assert!(
            w.store()
                .get("sim/dock")
                .unwrap()
                .roles
                .iter()
                .any(|role| { role.handle == reviewer && role.role == "reviewer" })
        );
        assert!(warned.is_empty(), "{warned}");
        assert!(swept.closed.is_empty(), "{swept:?}");
        assert!(w.has_pane(&pane));
        assert_eq!(w.reap_data()["idle"], json!({}));
        assert!(w.store().sessions_of(&finished).unwrap().is_empty());
        w.kept(&pane, How::default());
        w.store().leave(&reviewer).unwrap();
        assert_eq!(panes(&w.twice(How::default())), [pane.as_str()]);
    }
}

#[test]
fn a_new_conversation_during_the_sweep_restarts_the_idle_wait() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let pane = w.agent(Agent::named(&handle));
    w.sweep();
    let changed = Cell::new(false);
    let mut hooked = Hooked::new(&w);
    hooked.after = Box::new(|args| {
        if args[..2] == ["pane", "process-info"] && !changed.replace(true) {
            w.edit_pane(&pane, |p| {
                p["agent_session"]["value"] = json!("a-new-conversation");
            });
        }
    });
    let (swept, warned) = w.sweep_with(&hooked, How::default());
    assert!(changed.get());
    assert!(warned.is_empty(), "{warned}");
    assert!(swept.closed.is_empty(), "{swept:?}");
    assert!(w.has_pane(&pane));
    assert!(w.store().sessions_of(&handle).unwrap().is_empty());
    assert_eq!(w.reap_data()["idle"], json!({}));
    assert_eq!(w.sweep().idle.as_slice(), std::slice::from_ref(&handle));
    assert_eq!(panes(&w.sweep()), [pane.as_str()]);
    assert_eq!(
        w.store().sessions_of(&handle).unwrap()[0].claude_session,
        "a-new-conversation"
    );
}

#[test]
fn a_different_agent_kind_starts_its_own_idle_wait() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let pane = w.agent(Agent {
        by_flag: false,
        ..Agent::named(&handle)
    });
    w.sweep();
    w.edit_pane(&pane, |p| {
        p["agent"] = json!("codex");
        p["agent_session"]["agent"] = json!("codex");
        p["foreground"][1]["name"] = json!("codex");
        p["foreground"][1]["argv"] = json!(["codex"]);
        p["foreground"][1]["cmdline"] = json!("codex");
    });
    assert_eq!(w.sweep().idle.as_slice(), std::slice::from_ref(&handle));
    assert!(w.has_pane(&pane));
    assert!(w.store().sessions_of(&handle).unwrap().is_empty());
    assert_eq!(panes(&w.sweep()), [pane.as_str()]);
    assert_eq!(
        w.store().sessions_of(&handle).unwrap()[0].claude_session,
        format!("codex:sess-{pane}")
    );
}

#[test]
fn a_worker_becoming_a_protected_session_during_the_sweep_is_kept() {
    for kind in ["orchestrator", "chat"] {
        let w = World::new();
        let handle = w.executor_on("done", "sim/jump");
        let pane = w.agent(Agent::named(&handle));
        w.sweep();
        let changed = Cell::new(false);
        let mut hooked = Hooked::new(&w);
        hooked.after = Box::new(|args| {
            if args[..2] == ["pane", "process-info"] && !changed.replace(true) {
                w.f.other_client()
                    .write(
                        "UPDATE type::record('session', $handle) SET kind = $kind RETURN NONE;",
                        json!({"handle":handle, "kind":kind}),
                    )
                    .unwrap();
            }
        });
        let (swept, warned) = w.sweep_with(&hooked, How::default());
        assert!(changed.get());
        assert_eq!(w.store().session(&handle).unwrap().row.kind, kind);
        assert!(warned.is_empty(), "{warned}");
        assert!(swept.closed.is_empty());
        assert!(w.has_pane(&pane));
        assert!(w.store().sessions_of(&handle).unwrap().is_empty());
        assert_eq!(w.reap_data()["idle"], json!({}));
        w.kept(&pane, How::default());
    }
}

#[test]
fn an_unconfirmed_conversation_clears_the_idle_wait() {
    for unknown in [
        None,
        Some(Value::Null),
        Some(json!("")),
        Some(json!(12)),
        Some(json!({})),
        Some(json!([])),
    ] {
        let w = World::new();
        let handle = w.executor_on("done", "sim/jump");
        let pane = w.agent(Agent::named(&handle));
        let original = w.state();
        w.sweep();
        let changed = Cell::new(false);
        let mut hooked = Hooked::new(&w);
        hooked.after = Box::new(|args| {
            if args[..2] == ["pane", "process-info"] && !changed.replace(true) {
                w.edit_pane(&pane, |p| match &unknown {
                    Some(value) => p["agent_session"]["value"] = value.clone(),
                    None => {
                        p.remove("agent_session");
                    }
                });
            }
        });
        let (swept, warned) = w.sweep_with(&hooked, How::default());
        assert!(changed.get());
        assert!(warned.is_empty(), "{warned}");
        assert!(swept.closed.is_empty(), "{unknown:?}");
        assert!(w.has_pane(&pane));
        assert!(w.store().sessions_of(&handle).unwrap().is_empty());
        assert_eq!(w.reap_data()["idle"], json!({}));
        assert!(w.sweep().closed.is_empty());
        assert_eq!(w.reap_data()["idle"], json!({}));
        w.set_state(original);
        assert_eq!(w.sweep().idle.as_slice(), std::slice::from_ref(&handle));
        assert_eq!(panes(&w.sweep()), [pane.as_str()]);
    }
}

#[test]
fn an_unconfirmed_final_process_owner_restarts_the_idle_wait() {
    for answer in [
        Err(LaunchError("process information unavailable".into())),
        Ok(json!({"result":{"process_info":{"foreground_processes":[]}}}).to_string()),
        Ok(json!({"result":{"process_info":{"foreground_processes":[{"name":"claude", "argv":["claude"]}]}}}).to_string()),
        Ok(json!({"result":{"process_info":{"foreground_processes":[{"name":"claude", "argv":["claude"], "pid":12345}]}}}).to_string()),
    ] {
        let w = World::new();
        let handle = w.executor_on("done", "sim/jump");
        let pane = w.agent(Agent::named(&handle));
        w.sweep();
        let reads = Cell::new(0);
        let mut hooked = Hooked::new(&w);
        hooked.instead = Box::new(|args| {
            if args[..2] == ["pane", "process-info"] {
                reads.set(reads.get() + 1);
                if reads.get() == 2 {
                    return Some(answer.clone());
                }
            }
            None
        });
        let (swept, warned) = w.sweep_with(&hooked, How::default());
        assert_eq!(reads.get(), 2);
        assert!(warned.is_empty(), "{warned}");
        assert!(swept.closed.is_empty(), "{answer:?}");
        assert!(w.has_pane(&pane));
        assert!(w.store().sessions_of(&handle).unwrap().is_empty());
        assert_eq!(w.reap_data()["idle"], json!({}));
        assert_eq!(w.sweep().idle.as_slice(), std::slice::from_ref(&handle));
        assert_eq!(panes(&w.sweep()), [pane.as_str()]);
    }
}

#[test]
fn work_or_unreadable_processes_on_the_final_check_restart_the_idle_wait() {
    for commands in [
        Ok(vec!["just verify".into()]),
        Err(LaunchError("process tree unavailable".into())),
    ] {
        let w = World::new();
        let handle = w.executor_on("done", "sim/jump");
        let pane = w.agent(Agent::named(&handle));
        w.sweep();
        let reads = Cell::new(0);
        let processes = move |_| {
            reads.set(reads.get() + 1);
            if reads.get() == 1 {
                Ok(vec![])
            } else {
                commands.clone()
            }
        };
        let swept = w.sweep_as(How {
            processes: Some(&processes),
            ..How::default()
        });
        assert!(swept.closed.is_empty());
        assert!(w.has_pane(&pane));
        assert!(w.store().sessions_of(&handle).unwrap().is_empty());
        assert_eq!(w.reap_data()["idle"], json!({}));
        assert_eq!(w.sweep().idle.as_slice(), std::slice::from_ref(&handle));
        assert_eq!(panes(&w.sweep()), [pane.as_str()]);
    }
}

#[test]
fn an_agent_that_claims_work_during_the_sweep_is_kept() {
    for claim_after in [1, 2] {
        let w = World::new();
        let handle = w.executor_on("done", "sim/jump");
        let pane = w.agent(Agent::named(&handle));
        w.task("sim/dock", None);
        w.sweep();
        let reads = Cell::new(0);
        let mut hooked = Hooked::new(&w);
        hooked.after = Box::new(|args| {
            if args[..2] == ["pane", "process-info"] {
                reads.set(reads.get() + 1);
                if reads.get() == claim_after {
                    w.store().claim("sim/dock", &handle).unwrap();
                }
            }
        });
        let (swept, _) = w.sweep_with(&hooked, How::default());
        assert!(reads.get() >= claim_after);
        assert_eq!(w.store().get("sim/dock").unwrap().status, "in_progress");
        assert_eq!(swept.closed, []);
        assert!(w.has_pane(&pane));
    }
}

#[test]
fn a_close_whose_record_fails_is_not_closed_and_the_sweep_goes_on() {
    let w = World::new();
    let gone = w.executor_on("done", "sim/jump");
    let first = w.agent(Agent::named(&gone));
    w.f.task_with(NewTask::new("sim/dock", "Dock", "owner"));
    let second = w.agent(Agent::named(&w.executor_on("done", "sim/dock")));
    w.sweep();
    let reads = Cell::new(0);
    let client = w.f.other_client();
    let removed = gone.clone();
    let machine = w.machine();
    let processes = move |pid| {
        reads.set(reads.get() + 1);
        if reads.get() == 2 {
            // Final eligibility was confirmed; the board then refuses its close record.
            client
                .write(
                    "DELETE type::record('session', $handle);",
                    json!({"handle": removed}),
                )
                .unwrap();
        }
        (machine.processes)(pid)
    };
    let (swept, warned) = w.sweep_with(
        &w.herdr(),
        How {
            processes: Some(&processes),
            ..How::default()
        },
    );
    assert_eq!(panes(&swept), [second.as_str()]);
    assert!(
        warned.starts_with(&format!(
            "· idle sweep kept {first} ({gone}): the board took no record: "
        )) && warned.ends_with('\n')
            && warned.lines().count() == 1,
        "{warned}"
    );
    assert!(w.has_pane(&first));
    assert!(!w.has_pane(&second));
    let [s] = <[_; 1]>::try_from(
        w.store()
            .all_task_sessions()
            .unwrap()
            .into_iter()
            .filter(|s| s.slug == "sim/dock")
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert!(s.closed.is_some());
}
