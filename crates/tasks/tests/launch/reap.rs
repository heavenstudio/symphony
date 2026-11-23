//! Idle terminal sweep with deterministic board-clock intervals.

#[path = "reap_codex.rs"]
mod codex;
#[path = "reap_ended.rs"]
mod ended;
#[path = "reap_hour.rs"]
mod hour;
#[path = "reap_pending.rs"]
mod pending;
use codex::sweep_stopped_before_its_kill;

use crate::{Folder, Hooked, World, git};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use symphony_board::cli::Env;
use symphony_board::health::LIVE_SECONDS;
use symphony_board::launch::hc::Hc;
use symphony_board::launch::reap::{
    self, Closed, IDLE_ONCE, Kept, REAP_SECONDS, Reap, Sweep, lock_path, state_path,
};
use symphony_board::launch::{Ask, LaunchError, Processes, restore as restore_session};
use symphony_board::{Error, NewLaunch, NewSession, NewTask, Raw, Store};
use symphony_board::{Health, cli};

/// Why a sweep closes the terminal of an agent idle with its work finished.
const FINISHED: &str = "idle for two sweeps, its work finished";

/// How an agent's terminal names its handle.
#[derive(Clone, Copy)]
enum By {
    /// Claude's `-n HANDLE`.
    Flag,
    /// Its `symphony.handle` label.
    Label,
    /// Its terminal's name.
    Name,
}

/// A claude agent's terminal of this board, as [`World::agent`] makes it.
struct Agent<'a> {
    handle: Option<&'a str>,
    status: &'a str,
    /// What runs under its agent.
    procs: &'a [&'a str],
    by: By,
    screen: &'a str,
    extra: Value,
}

impl<'a> Agent<'a> {
    fn named(handle: &'a str) -> Agent<'a> {
        Agent {
            handle: Some(handle),
            status: "idle",
            procs: &[],
            by: By::Flag,
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
    caller: Option<&'a str>,
    processes: Option<&'a Processes>,
}

impl World {
    /// A claude agent's terminal of this board (its program `claude --session-id sess-ADDRESS`),
    /// under which runs what `a` says: its address.
    fn agent(&self, a: Agent<'_>) -> String {
        let address = self.add_terminal(true, None, json!({}));
        self.edit_terminal(&address, |t| {
            let mut argv = vec![
                json!("env"),
                json!("TASKS_SESSION=x"),
                json!("claude"),
                json!("--session-id"),
                json!(format!("sess-{address}")),
            ];
            match (a.handle, a.by) {
                (Some(handle), By::Flag) => argv.extend([json!("-n"), json!(handle)]),
                (Some(handle), By::Label) => t["labels"]["symphony.handle"] = json!(handle),
                (Some(handle), By::Name) => {
                    t.insert("name".into(), json!(handle));
                }
                (None, _) => {}
            }
            argv.push(json!("/work sim/jump x"));
            t.insert("argv".into(), Value::Array(argv));
            t.insert("status".into(), json!(a.status));
            if !a.screen.is_empty() {
                t.insert("screen".into(), json!(a.screen));
            }
            for (key, value) in a.extra.as_object().into_iter().flatten() {
                t.insert(key.clone(), value.clone());
            }
        });
        self.run_under(self.pid_of(&address), a.procs);
        address
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

    fn reap<'a>(&self, how: How<'a>, processes: &'a Processes) -> Reap<'a> {
        Reap {
            state: self.reap_state(),
            apply: !how.dry,
            processes: how.processes.unwrap_or(processes),
            caller: how.caller,
            min_idle: REAP_SECONDS,
        }
    }

    /// One sweep through `hc` as `how` says, then, if it swept, the timer's interval: what it
    /// found (None: the lock was held), and what it warned of.
    fn once(&self, hc: &dyn Hc, how: How<'_>) -> (Option<Sweep>, String) {
        let m = self.machine();
        let reap = self.reap(how, &*m.processes);
        let mut warn = Vec::new();
        let swept = reap::sweep(self.store(), hc, &reap, &mut warn).expect("the sweep runs");
        if swept.is_some() {
            self.f.advance(REAP_SECONDS);
        }
        (swept, String::from_utf8(warn).unwrap())
    }

    /// Wait for the previous sweep's lock to be released, then complete exactly one round.
    fn sweep_with(&self, hc: &dyn Hc, how: How<'_>) -> (Sweep, String) {
        wait_for_sweep(&self.reap_state());
        let (swept, warned) = self.once(hc, how);
        (swept.expect("the sweep records its round"), warned)
    }

    fn sweep(&self) -> Sweep {
        self.sweep_as(How::default())
    }

    fn sweep_as(&self, how: How<'_>) -> Sweep {
        let (swept, warned) = self.sweep_with(&self.hc(), how);
        assert_eq!(warned, "");
        swept
    }

    /// Two sweeps in a row: a terminal idle in both is closed by the second.
    fn twice(&self, how: How<'_>) -> Sweep {
        self.sweep_as(how);
        self.sweep_as(how)
    }

    /// The terminals removed.
    fn closes(&self) -> Vec<Vec<String>> {
        self.calls_of(&["rm"])
    }

    /// Kept through two sweeps, with nothing closed.
    fn kept(&self, terminal: &str, how: How<'_>) {
        self.twice(how);
        assert!(self.has_terminal(terminal));
        assert_eq!(self.closes(), Vec::<Vec<String>>::new());
        assert!(self.calls_of(&["kill"]).is_empty());
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
    hc: &dyn Hc,
    how: &Reap<'_>,
    warn: &mut dyn Write,
) -> Result<Sweep, Error> {
    wait_for_sweep(&how.state);
    reap::sweep(store, hc, how, warn).map(|swept| swept.expect("the sweep records its round"))
}

fn terminals(swept: &Sweep) -> Vec<&str> {
    swept.closed.iter().map(|c| c.pane.as_str()).collect()
}

fn restore(slug: &str, handle: &str) -> String {
    format!("just tasks restore {slug} {handle}")
}

/// Seed a Codex registration on the board, without invoking the real Codex queue transport: from
/// `terminal`, if one is given.
fn register_codex(w: &World, handle: &str, thread: &str, terminal: Option<&str>) {
    (w.store().raw().row("callback_state", handle))
        .put(
            json!({"handle": handle, "address": format!("codex:{thread}"), "host": "this-host",
                    "confirmed_until": w.f.now() + 390.0, "lease": w.f.now() + 1_000_000.0,
                    "terminal": terminal}),
        )
        .unwrap();
}

/// A Codex agent's terminal of this board for `handle`, at work.
fn codex_terminal(w: &World, handle: &str) -> String {
    w.agent(Agent {
        status: "working",
        extra: json!({"agent": "codex", "argv": ["codex", "--no-daemon", "hi"]}),
        by: By::Label,
        ..Agent::named(handle)
    })
}

// -- native presence ----------------------------------------------------------------------

#[test]
fn failed_or_unanswered_presence_rounds_hold_work_until_the_cap_and_confirmation_resets_it() {
    let w = World::new();
    let mut workers = Vec::new();
    for i in 0..2 {
        let slug = format!("tools/presence-{i}");
        w.f.task(&slug);
        let handle = w.sub_session("worker", &w.orch);
        w.store().claim(&slug, &handle).unwrap();
        let terminal = codex_terminal(&w, &handle);
        register_codex(&w, &handle, &format!("abcdef0{i}"), Some(&terminal));
        workers.push((slug, handle));
    }
    w.f.task("tools/presence-finished");
    let finished = w.executor_on("done", "tools/presence-finished");
    register_codex(&w, &finished, "abcdef0f", None);
    let before = cli::attach::observe(w.store(), &w.orch, "orchestrator").unwrap();
    // A PostgreSQL socket whose server takes the connection and never answers it.
    let silent = Folder::new();
    let listener = std::os::unix::net::UnixListener::bind(
        silent.0.join(symphony_board::server::postgres::SOCKET_FILE),
    )
    .unwrap();
    // The server says nothing until the round is over, or until the backstop of the 5 s the round
    // once had has passed, when it closes the connection: a round that waited for it says the
    // server closed, not that it does not answer.
    let (release, held) = std::sync::mpsc::channel::<()>();
    let server = std::thread::spawn(move || {
        let (_stream, _) = listener.accept().unwrap();
        let _ = held.recv_timeout(test_time::backstop(Duration::from_secs(5)));
    });
    let now = w.f.now();
    let failed = Store::unchecked_on(
        symphony_board::pg::Client::new(&silent.0, w.store().database().database())
            .waiting(Duration::from_millis(30))
            .into(),
        Box::new(move || now),
        Box::new(|_| true),
        "this-host",
        &symphony_board::Opening::write(),
    );
    let m = w.machine();
    let how = w.reap(How::default(), &*m.processes);
    let error = presence_round(&failed, &w.hc(), &how, &mut Vec::new()).unwrap_err();
    let _ = release.send(());
    server.join().unwrap();
    let unanswered = format!(
        "is not answering on {}",
        silent
            .0
            .join(symphony_board::server::postgres::SOCKET_FILE)
            .display()
    );
    assert!(error.to_string().contains(&unanswered), "{error}");

    w.f.advance(601.0);
    for (slug, handle) in &workers {
        assert_eq!(w.store().session(handle).unwrap().health, Health::Unknown);
        assert_eq!(
            w.store().get(slug).unwrap().roles[0].health,
            Health::Unknown
        );
        let other = w.f.session("worker");
        assert!(
            w.store().claim(slug, &other).is_err(),
            "unknown signal must retain ownership"
        );
    }
    let digest = cli::pull::digest(w.store(), &[], None, w.f.now(), &[]).unwrap();
    assert_eq!(digest.executing, 2);
    assert_eq!(digest.count("in_progress_orphaned"), 0);
    assert_eq!(
        w.store().session(&finished).unwrap().health,
        Health::Unknown
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
    let after = cli::attach::observe(w.store(), &w.orch, "orchestrator").unwrap();
    let mut events = Vec::new();
    cli::attach::diff(w.store(), &before, &after, &w.orch, &mut |line| {
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
    let view = cli::agents::describe_board(
        w.store(),
        &cli::probes::machine(&w.env),
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
        assert_eq!(agent.health, Health::Unknown);
        assert_ne!(agent.state, "gone");
    }

    w.f.advance(389.0); // exactly ten minutes since confirmation expired
    for (_, handle) in &workers {
        assert_eq!(w.f.health(handle), Health::Unknown);
    }
    w.f.advance(1.0);
    for (slug, handle) in &workers {
        assert_eq!(w.f.health(handle), Health::Lost);
        assert_eq!(w.store().get(slug).unwrap().roles[0].health, Health::Lost);
    }
    assert_eq!(w.f.health(&finished), Health::Left);
    let lost = cli::attach::observe(w.store(), &w.orch, "orchestrator").unwrap();
    events.clear();
    cli::attach::diff(w.store(), &after, &lost, &w.orch, &mut |line| {
        events.push(line.to_string())
    })
    .unwrap();
    assert_eq!(events.len(), 2, "{events:?}");
    assert!(
        events.iter().all(|line| line.starts_with("LOST ")),
        "{events:?}"
    );
    let again = cli::attach::observe(w.store(), &w.orch, "orchestrator").unwrap();
    events.clear();
    cli::attach::diff(w.store(), &lost, &again, &w.orch, &mut |line| {
        events.push(line.to_string())
    })
    .unwrap();
    assert!(events.is_empty(), "LOST is announced once: {events:?}");
    let digest = cli::pull::digest(w.store(), &[], None, w.f.now(), &[]).unwrap();
    assert_eq!(digest.count("in_progress_orphaned"), 2);
    assert!(digest.liveness_unknown.is_empty());

    // A real confirmation after expiry revives the workers and starts a fresh grace interval.
    presence_round(w.store(), &w.hc(), &how, &mut Vec::new()).unwrap();
    for (_, handle) in &workers {
        assert_eq!(w.f.health(handle), Health::Live);
    }
    // A registration from no terminal that no launch records: hc cannot say where it runs.
    assert!(w.store().callbacks().unwrap().contains_key(&finished));
    w.f.advance(601.0);
    for reply in [
        json!({}),
        json!({"terminals": null}),
        json!({"terminals": {}}),
        json!({"terminals": [null]}),
    ] {
        let reply = reply.to_string();
        let mut unanswered = Hooked::new(&w);
        unanswered.instead = Box::new(move |args| (args[0] == "ls").then(|| Ok(reply.clone())));
        let mut warnings = Vec::new();
        presence_round(w.store(), &unanswered, &how, &mut warnings).unwrap();
        let warned = String::from_utf8(warnings).unwrap();
        assert!(
            warned.starts_with("terminal sweep unavailable: hc ls ")
                && warned.ends_with("; native sockets checked\n"),
            "{warned}"
        );
        for (_, handle) in &workers {
            assert_eq!(w.store().session(handle).unwrap().health, Health::Unknown);
            assert!(w.store().callbacks().unwrap().contains_key(handle));
        }
    }
    presence_round(w.store(), &w.hc(), &how, &mut Vec::new()).unwrap();
    for (_, handle) in &workers {
        assert_eq!(w.store().session(handle).unwrap().health, Health::Live);
        assert!(w.store().callbacks().unwrap().contains_key(handle));
    }
    w.edit_terminals(|all| all.clear());
    w.f.advance(601.0);
    presence_round(w.store(), &w.hc(), &how, &mut Vec::new()).unwrap();
    for (_, handle) in &workers {
        assert!(!w.store().callbacks().unwrap().contains_key(handle));
        assert_eq!(w.store().session(handle).unwrap().health, Health::Lost);
    }
    assert!(w.store().callbacks().unwrap().contains_key(&finished));
}

#[test]
fn a_codex_thread_is_present_in_the_terminal_its_launch_records_or_it_registered_from() {
    let w = World::new();
    let m = w.machine();
    let how = w.reap(How::default(), &*m.processes);
    w.task("sim/dock", None);
    // Launched: the record puts its thread in its terminal, whatever terminal it registered from.
    let s = w.launch("sim/jump").row;
    w.store()
        .callback_in(&s.handle, Some("codex:0abc"), false, false, None)
        .unwrap();
    // Registered from its terminal, with no launch record.
    let found = w.f.session("worker");
    w.store().claim("sim/dock", &found).unwrap();
    let terminal = codex_terminal(&w, &found);
    register_codex(&w, &found, "0def", Some(&terminal));
    let present = |w: &World| -> Vec<bool> {
        let callbacks = w.store().callbacks().unwrap();
        [&s.handle, &found]
            .map(|h| callbacks.contains_key(h.as_str()))
            .to_vec()
    };
    w.f.advance(601.0);
    presence_round(w.store(), &w.hc(), &how, &mut Vec::new()).unwrap();
    assert_eq!(present(&w), [true, true]);
    assert_eq!(w.f.health(&s.handle), Health::Live);
    assert_eq!(w.f.health(&found), Health::Live);

    // A terminal that runs something else, or none: the thread no longer occupies it.
    w.edit_terminal(&terminal, |t| {
        t.insert("agent".into(), json!("zsh"));
        t.insert("argv".into(), json!(["zsh"]));
    });
    presence_round(w.store(), &w.hc(), &how, &mut Vec::new()).unwrap();
    assert_eq!(present(&w), [true, false]);
    // Registered from no terminal, as Codex registers: kept while its launch's terminal is live,
    // though it runs something else; retired once that terminal is gone.
    w.edit_terminal(&s.pane, |t| {
        t.insert("agent".into(), json!("zsh"));
        t.insert("argv".into(), json!(["zsh"]));
    });
    presence_round(w.store(), &w.hc(), &how, &mut Vec::new()).unwrap();
    assert_eq!(present(&w), [true, false], "its launch's terminal is live");
    w.edit_terminal(&s.pane, |t| {
        t.insert("state".into(), json!("finished"));
    });
    presence_round(w.store(), &w.hc(), &how, &mut Vec::new()).unwrap();
    assert_eq!(
        present(&w),
        [false, false],
        "every terminal it was launched in is gone: retired"
    );
    w.store()
        .callback_in(&s.handle, Some("codex:0abc"), false, false, Some(&s.pane))
        .unwrap();
    presence_round(w.store(), &w.hc(), &how, &mut Vec::new()).unwrap();
    assert_eq!(present(&w), [false, false]);
}

#[test]
fn a_dry_run_reports_presence_and_changes_no_registration() {
    let w = World::new();
    let m = w.machine();
    let handle = w.f.session("worker");
    register_codex(&w, &handle, "0abc", Some("t-gone"));
    let how = w.reap(
        How {
            dry: true,
            ..How::default()
        },
        &*m.processes,
    );
    presence_round(w.store(), &w.hc(), &how, &mut Vec::new()).unwrap();
    assert!(w.store().callbacks().unwrap().contains_key(&handle));
}

// -- what closes ------------------------------------------------------------------------

#[test]
fn a_finished_idle_agent_is_killed_and_removed_on_the_second_idle_sweep() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let terminal = w.agent(Agent::named(&handle));
    let first = w.sweep();
    assert_eq!(
        first,
        Sweep {
            closed: vec![],
            idle: vec![handle.clone()],
            kept: vec![Kept {
                pane: terminal.clone(),
                handle: handle.clone(),
                why: IDLE_ONCE.into(),
                idle: true,
                told: true,
            }],
        }
    );
    assert!(w.has_terminal(&terminal));
    let second = w.sweep();
    assert_eq!(
        second,
        Sweep {
            closed: vec![Closed {
                pane: terminal.clone(),
                handle: handle.clone(),
                why: FINISHED.into(),
                restore: restore("sim/jump", &handle),
            }],
            ..Sweep::default()
        }
    );
    assert!(!w.has_terminal(&terminal));
    assert_eq!(w.calls_of(&["kill", &terminal]).len(), 1);
    assert_eq!(w.closes().len(), 1);
}

#[test]
fn an_agent_that_worked_between_two_idle_sweeps_starts_its_count_again() {
    let w = World::new();
    let terminal = w.agent(Agent::named(&w.executor_on("done", "sim/jump")));
    w.sweep();
    w.edit_terminal(&terminal, |t| {
        t.insert("status".into(), "working".into());
    });
    w.sweep();
    w.edit_terminal(&terminal, |t| {
        t.insert("status".into(), "idle".into());
    });
    w.sweep();
    assert!(w.has_terminal(&terminal));
    w.sweep();
    assert!(!w.has_terminal(&terminal));
}

#[test]
fn a_done_status_counts_as_idle() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let terminal = w.agent(Agent {
        status: "done",
        ..Agent::named(&handle)
    });
    w.twice(How::default());
    assert!(!w.has_terminal(&terminal));
}

#[test]
fn an_agent_stuck_on_the_folder_trust_prompt_is_closed() {
    let w = World::new();
    let handle = w.f.session("worker");
    let terminal = w.agent(Agent {
        status: "blocked",
        screen: "Do you trust this folder?\n 1. Yes, proceed",
        ..Agent::named(&handle)
    });
    w.twice(How::default());
    assert!(!w.has_terminal(&terminal));
    assert!(
        w.calls().contains(
            &["read", &terminal, "--lines", "30"]
                .map(String::from)
                .to_vec()
        )
    );
}

#[test]
fn an_agent_blocked_on_anything_else_is_kept() {
    let w = World::new();
    let handle = w.f.session("worker");
    let terminal = w.agent(Agent {
        status: "blocked",
        screen: "Allow this command?",
        ..Agent::named(&handle)
    });
    w.kept(&terminal, How::default());
}

#[test]
fn the_executor_of_a_task_in_review_waits_for_its_reviewers_and_closes_once_it_is_done() {
    let w = World::new();
    let handle = w.executor_on("review", "sim/jump");
    let terminal = w.agent(Agent::named(&handle));
    w.kept(&terminal, How::default());
    let reviewer = w.f.session("reviewer");
    w.store()
        .take_role("sim/jump", &reviewer, "reviewer")
        .unwrap();
    w.store()
        .verdict("sim/jump", &reviewer, "approve", "ok")
        .unwrap();
    assert_eq!(terminals(&w.twice(How::default())), [terminal.as_str()]);
}

#[test]
fn a_post_to_an_executor_whose_terminal_was_closed_tells_the_orchestrator_until_it_is_restored() {
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
    // As the sweep closed it before an executor in review was kept: the terminal goes, then its
    // beat ages.
    w.hc().close(&s.pane).unwrap();
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
    restore_session(w.store(), &w.hc(), &w.machine(), "sim/jump", None, None).unwrap();
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
fn a_handle_named_by_the_terminals_label_or_name_is_matched_too() {
    for by in [By::Label, By::Name] {
        let w = World::new();
        let handle = w.executor_on("done", "sim/jump");
        let terminal = w.agent(Agent {
            by,
            ..Agent::named(&handle)
        });
        w.twice(How::default());
        assert!(!w.has_terminal(&terminal));
    }
}

#[test]
fn a_monitor_and_helpers_under_claude_are_not_work() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let terminal = w.agent(Agent {
        procs: &[
            "caffeinate -i -t 300",
            "node /x/mcp-server.js",
            "codex --profile e",
            "codex resume THREAD 'a prompt containing exec'",
            "codex -- exec",
            "codex -- e",
            "/bin/zsh -c just tasks-attach --resume a",
            "just tasks-attach --resume a",
            "/repo/.git/builds/tasks-0123456789abcdef attach --resume a",
            "/repo/.git/builds/tasks-0123456789abcdef api --lines",
            "/x/.target/rs-bin/tasks-0123456789abcdef api",
            "/repo/.target/bin/tasks beat",
            "just tasks inbox",
        ],
        ..Agent::named(&handle)
    });
    w.twice(How::default());
    assert!(!w.has_terminal(&terminal));
}

// -- what is kept -----------------------------------------------------------------------

#[test]
fn a_working_agent_is_kept() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let terminal = w.agent(Agent {
        status: "working",
        ..Agent::named(&handle)
    });
    w.kept(&terminal, How::default());
}

#[test]
fn an_executor_on_an_in_progress_or_blocked_task_is_kept() {
    for status in ["in_progress", "blocked"] {
        let w = World::new();
        let terminal = w.agent(Agent::named(&w.executor_on(status, "sim/jump")));
        w.kept(&terminal, How::default());
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
    let terminal = w.agent(Agent::named(&reviewer));
    w.kept(&terminal, How::default());
}

#[test]
fn a_busy_descendant_keeps_the_terminal() {
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
        let terminal = w.agent(Agent {
            procs: &["/bin/zsh -c x", command],
            ..Agent::named(&handle)
        });
        w.twice(How::default());
        assert!(w.has_terminal(&terminal), "{command}");
    }
    assert_eq!(w.closes(), Vec::<Vec<String>>::new());
}

#[test]
fn an_unnamed_terminal_is_kept_it_may_be_the_owners() {
    let w = World::new();
    let terminal = w.agent(Agent::unnamed());
    w.kept(&terminal, How::default());
}

#[test]
fn a_name_that_is_no_board_session_is_kept() {
    let w = World::new();
    let terminal = w.agent(Agent::named("someone-else"));
    w.kept(&terminal, How::default());
}

#[test]
fn an_orchestrator_or_a_chat_is_kept() {
    for kind in ["orchestrator", "chat"] {
        let w = World::new();
        let terminal = w.agent(Agent::named(&w.f.session(kind)));
        w.kept(&terminal, How::default());
    }
}

#[test]
fn the_callers_own_terminal_and_handle_are_kept() {
    let mut w = World::new();
    let mine = w.executor_on("done", "sim/jump");
    let terminal = w.agent(Agent::named(&mine));
    w.kept(
        &terminal,
        How {
            caller: Some(&mine),
            ..How::default()
        },
    );
    w.set("HC_TERMINAL_ID", &terminal);
    w.kept(&terminal, How::default());
}

#[test]
fn a_terminal_whose_processes_cannot_be_read_is_kept() {
    let w = World::new();
    let broken: &Processes = &|_| Err(LaunchError("ps printed nothing".into()));
    let terminal = w.agent(Agent::named(&w.executor_on("done", "sim/jump")));
    w.kept(
        &terminal,
        How {
            processes: Some(broken),
            ..How::default()
        },
    );
}

#[test]
fn a_terminal_hc_cannot_describe_or_with_no_pid_is_kept() {
    let mut w = World::new();
    let terminal = w.agent(Agent::named(&w.executor_on("done", "sim/jump")));
    w.set("FAKE_HC_FAIL", "get");
    w.kept(&terminal, How::default());
    w.set("FAKE_HC_FAIL", "");
    w.edit_terminal(&terminal, |t| {
        t.insert("pid".into(), json!(0));
    });
    w.kept(&terminal, How::default());
}

#[test]
fn a_dry_run_closes_nothing_and_leaves_the_count_alone() {
    let w = World::new();
    let terminal = w.agent(Agent::named(&w.executor_on("done", "sim/jump")));
    w.sweep();
    let dry = w.sweep_as(How {
        dry: true,
        ..How::default()
    });
    assert_eq!(terminals(&dry), [terminal.as_str()]);
    assert_eq!(dry.closed[0].restore, "");
    assert!(w.has_terminal(&terminal));
    assert_eq!(w.closes(), Vec::<Vec<String>>::new());
    assert!(w.store().all_task_sessions().unwrap().is_empty());
    w.sweep();
    assert!(!w.has_terminal(&terminal));
}

#[test]
fn a_first_dry_run_does_not_count_as_a_sweep() {
    let w = World::new();
    let terminal = w.agent(Agent::named(&w.executor_on("done", "sim/jump")));
    w.sweep_as(How {
        dry: true,
        ..How::default()
    });
    assert!(!w.reap_state().exists());
    w.sweep();
    assert!(w.has_terminal(&terminal));
}

// -- what the board records -------------------------------------------------------------

#[test]
fn a_close_closes_the_launched_session_with_one_line_on_its_thread() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let terminal = w.agent(Agent::named(&handle));
    w.store()
        .record_launch(&NewLaunch {
            slug: "sim/jump".into(),
            handle: handle.clone(),
            role: "executor".into(),
            claude_session: format!("sess-{terminal}"),
            worktree: "/wt".into(),
            branch: "sim/jump".into(),
            pane: terminal.clone(),
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
    let terminal = w.agent(Agent::named(&handle));
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
        ("sim/dock", terminal.as_str(), format!("sess-{terminal}"))
    );
    assert_eq!(
        (s.role.as_str(), s.worktree.as_str()),
        ("executor", "/somewhere")
    );
}

#[test]
fn an_agent_with_no_task_is_closed_with_nothing_to_record() {
    let w = World::new();
    let handle = w.f.session("worker");
    let terminal = w.agent(Agent::named(&handle));
    let swept = w.twice(How::default());
    assert_eq!(
        swept.closed,
        [Closed {
            pane: terminal.clone(),
            handle,
            why: FINISHED.into(),
            restore: String::new(),
        }]
    );
    assert!(!w.has_terminal(&terminal));
    assert!(w.store().all_task_sessions().unwrap().is_empty());
}

#[test]
fn a_close_hc_refuses_leaves_its_session_open() {
    let mut w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let terminal = w.agent(Agent::named(&handle));
    w.sweep();
    w.set("FAKE_HC_FAIL", "kill");
    let swept = w.sweep();
    assert_eq!(swept.closed, []);
    let [kept] = <[_; 1]>::try_from(swept.kept).unwrap();
    assert!(kept.why.starts_with("hc refused to close it: "), "{kept:?}");
    assert!(w.has_terminal(&terminal));
    let sessions = w.store().sessions_of(&handle).unwrap();
    assert!(!sessions.is_empty());
    assert!(sessions.iter().all(|s| s.closed.is_none()), "{sessions:?}");
    assert!(sessions.iter().all(|s| s.pane == terminal), "{sessions:?}");
}

#[test]
fn a_closed_codex_agents_registration_is_retired_with_it() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let terminal = w.agent(Agent {
        extra: json!({"agent": "codex", "argv": ["codex", "--no-daemon", "hi"]}),
        by: By::Label,
        ..Agent::named(&handle)
    });
    register_codex(&w, &handle, "0abc", Some(&terminal));
    assert_eq!(terminals(&w.twice(How::default())), [terminal.as_str()]);
    assert!(!w.store().callbacks().unwrap().contains_key(&handle));
    assert_eq!(
        w.store().sessions_of(&handle).unwrap()[0].claude_session,
        "codex:0abc"
    );
}

#[test]
fn an_interrupted_close_is_retried_without_repeating_its_thread_line() {
    for launched in [false, true] {
        let w = World::new();
        let handle = w.executor_on("done", "sim/jump");
        let terminal = w.agent(Agent::named(&handle));
        if launched {
            w.store()
                .record_launch(&NewLaunch {
                    slug: "sim/jump".into(),
                    handle: handle.clone(),
                    role: "executor".into(),
                    claude_session: format!("sess-{terminal}"),
                    pane: terminal.clone(),
                    log: false,
                    ..NewLaunch::default()
                })
                .unwrap();
        }
        w.sweep();
        let before = w.f.said("sim/jump").len();
        sweep_stopped_before_its_kill(&w);
        let recorded = w.store().sessions_of(&handle).unwrap();
        assert_eq!(recorded.len(), 1);
        assert!(recorded[0].closed.is_some());
        assert!(w.has_terminal(&terminal));
        assert!(w.closes().is_empty());
        assert_eq!(w.f.said("sim/jump").len(), before + 1);
        let original = w.state();

        assert_eq!(
            w.sweep().closed,
            [Closed {
                pane: terminal.clone(),
                handle: handle.clone(),
                why: FINISHED.into(),
                restore: restore("sim/jump", &handle),
            }]
        );
        assert!(!w.has_terminal(&terminal));
        assert_eq!(w.store().sessions_of(&handle).unwrap(), recorded);
        assert_eq!(w.f.said("sim/jump").len(), before + 1);

        // A terminal of the same address running a new conversation writes that one's own close.
        w.set_state(original);
        w.edit_terminal(&terminal, |t| {
            t["argv"][4] = json!("new-conversation");
        });
        assert_eq!(terminals(&w.twice(How::default())), [terminal.as_str()]);
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
    let terminal = w.agent(Agent::named(&w.executor_on("done", "sim/jump")));
    w.sweep();
    let held = hold_lock(&w);
    let calls = w.calls();
    for _ in 0..2 {
        assert_eq!(w.once(&w.hc(), How::default()), (None, String::new()));
    }
    assert!(w.has_terminal(&terminal));
    assert_eq!(
        w.calls(),
        calls,
        "a sweep that found the lock held asks hc nothing"
    );
    drop(held);
    w.sweep();
    assert!(!w.has_terminal(&terminal));
}

#[test]
fn two_sweeps_at_once_close_a_terminal_once() {
    let w = World::new();
    let terminal = w.agent(Agent::named(&w.executor_on("done", "sim/jump")));
    w.sweep();
    let second: RefCell<Option<Option<Sweep>>> = RefCell::new(None);
    let mut hooked = Hooked::new(&w);
    hooked.instead = Box::new(|args| {
        if args[0] == "ls" && second.borrow().is_none() {
            // the first sweep holds the lock, inside its sweep: a second comes at the same time
            let (swept, _) = w.once(&w.hc(), How::default());
            *second.borrow_mut() = Some(swept);
        }
        None
    });
    let (first, warned) = w.sweep_with(&hooked, How::default());
    assert_eq!((second.borrow().clone(), warned.as_str()), (Some(None), ""));
    assert_eq!(terminals(&first), [terminal.as_str()]);
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
fn the_boards_own_is_in_symphony_tasks_of_the_git_common_dir() {
    let dir = Folder::new();
    let repo = dir.0.join("repo");
    std::fs::create_dir_all(repo.join("deep")).unwrap();
    git(&repo, &["init", "-q"]);
    let env = env_of(&[], dir.0.clone());
    assert_eq!(
        state_path(&env, &repo.join("deep")),
        Ok(repo.join(".git").join("symphony/tasks").join("reap.json"))
    );
}

#[test]
fn tasks_reap_state_names_it() {
    let env = env_of(
        &[
            ("TASKS_REAP_STATE", "/x/reap.json"),
            ("TASKS_SERVER", "postgresql:/scratch/socket"),
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
            &[
                ("TASKS_SERVER", "postgresql:/scratch/socket"),
                ("TASKS_DATABASE", db),
            ],
            std::env::temp_dir(),
        );
        state_path(&env, &std::env::temp_dir()).unwrap()
    };
    let (one, two) = (at("t1"), at("t2"));
    assert_ne!(one, two);
    assert_eq!(at("t1"), one, "the same board, the same file");
    assert_eq!(
        one.parent(),
        Some(std::env::temp_dir().join("symphony-tasks-reap").as_path())
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
fn a_scratch_database_alone_keeps_it_apart_from_the_live_boards_too() {
    let scratch = state_path(
        &env_of(&[("TASKS_DATABASE", "scratch")], std::env::temp_dir()),
        &std::env::temp_dir(),
    )
    .unwrap();
    assert_eq!(
        scratch.parent(),
        Some(std::env::temp_dir().join("symphony-tasks-reap").as_path())
    );
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
        "codex exec 'draw a lamp'",
        "codex e 'draw a lamp'",
        "codex --profile e exec 'draw a lamp'",
        "codex exec 'draw an mcp-lamp'",
        "CODEX e 'run just tasks inbox'",
        "ffmpeg -i a.wav b.ogg",
        "node x.js",
        "python3 /tmp/scratchpad/x.py",
        "/bin/zsh -c sleep 600",
        "sleep 600",
    ] {
        let handle = w.f.session("worker");
        let terminal = w.agent(Agent {
            procs: &["caffeinate -i -t 300", command],
            ..Agent::named(&handle)
        });
        w.twice(How::default());
        assert!(w.has_terminal(&terminal), "{command}");
    }
    assert_eq!(w.closes(), Vec::<Vec<String>>::new());
}

#[test]
fn two_sweeps_with_no_time_between_them_close_nothing() {
    let w = World::new();
    let terminal = w.agent(Agent::named(&w.executor_on("done", "sim/jump")));
    w.sweep();
    w.f.advance(-(REAP_SECONDS - 1.0)); // a second sweeper, 1 s after the first
    w.sweep();
    assert!(w.has_terminal(&terminal));
    w.f.advance(-2.0); // a third, 299 s after the terminal was first seen idle
    w.sweep();
    assert!(w.has_terminal(&terminal));
    w.sweep(); // an interval and more since it was first seen idle
    assert!(!w.has_terminal(&terminal));
}

#[test]
fn invalid_idle_state_starts_a_fresh_idle_wait() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let terminal = w.agent(Agent::named(&handle));
    w.sweep();
    let initial = w.reap_data();
    let marks = initial["idle"].as_object().unwrap();
    assert_eq!(marks.len(), 1);
    let key = marks.keys().next().unwrap();
    assert_eq!(key, &format!("{terminal} {handle} {}", w.pid_of(&terminal)));
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
        assert!(w.has_terminal(&terminal), "{invalid}");
        assert_eq!(w.reap_data()["idle"][key], json!(now), "{invalid}");
    }
    assert!(w.closes().is_empty());
    assert_eq!(terminals(&w.sweep()), [terminal.as_str()]);
}

#[test]
fn a_terminal_whose_subagent_holds_live_work_is_kept() {
    let w = World::new();
    w.executor_on("review", "sim/jump");
    let parent = w.f.session("worker");
    let child = w.sub_session("reviewer", &parent);
    w.store().take_role("sim/jump", &child, "reviewer").unwrap();
    let terminal = w.agent(Agent::named(&parent));
    w.twice(How::default());
    assert!(w.has_terminal(&terminal));
}

#[test]
fn a_grandchild_holding_live_work_keeps_the_terminal_too() {
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
            let terminal = w.agent(Agent::named(&parent));
            w.kept(&terminal, How::default());
            w.store().leave(&grandchild).unwrap();
            assert_eq!(terminals(&w.twice(How::default())), [terminal.as_str()]);
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
    let terminal = w.agent(Agent::named(&parent));
    w.twice(How::default());
    assert!(!w.has_terminal(&terminal));
}

// -- what changes during the sweep ------------------------------------------------------

/// A sweep whose `n`th look at a terminal (`hc get`) is followed by `change`.
fn sweep_changing(w: &World, n: usize, change: impl Fn()) -> (Sweep, String) {
    let looks = Cell::new(0);
    let mut hooked = Hooked::new(w);
    hooked.after = Box::new(|args| {
        if args[0] == "get" {
            looks.set(looks.get() + 1);
            if looks.get() == n {
                change();
            }
        }
    });
    let swept = w.sweep_with(&hooked, How::default());
    assert!(looks.get() >= n, "the sweep looked {} times", looks.get());
    swept
}

#[test]
fn an_agent_prompted_during_the_sweep_is_kept() {
    let w = World::new();
    let terminal = w.agent(Agent::named(&w.executor_on("done", "sim/jump")));
    w.sweep();
    let (swept, _) = sweep_changing(&w, 1, || {
        w.edit_terminal(&terminal, |t| {
            t.insert("status".into(), "working".into());
        });
    });
    assert_eq!(swept.closed, []);
    assert!(w.has_terminal(&terminal));
}

#[test]
fn a_live_reviewer_replacing_the_candidate_keeps_its_terminal() {
    for new_conversation in [false, true] {
        let w = World::new();
        let finished = w.executor_on("done", "sim/jump");
        let terminal = w.agent(Agent::named(&finished));
        w.task("sim/dock", None);
        w.executor_on("review", "sim/dock");
        let reviewer = w.f.session("reviewer");
        w.store()
            .take_role("sim/dock", &reviewer, "reviewer")
            .unwrap();
        w.sweep();
        let (swept, warned) = sweep_changing(&w, 1, || {
            w.edit_terminal(&terminal, |t| {
                if new_conversation {
                    t["argv"][4] = json!("replacement-reviewer");
                }
                t["argv"][6] = json!(reviewer);
            });
        });
        assert_eq!(w.store().get("sim/dock").unwrap().status, "review");
        assert!(warned.is_empty(), "{warned}");
        assert!(swept.closed.is_empty(), "{swept:?}");
        assert!(w.has_terminal(&terminal));
        assert_eq!(w.reap_data()["idle"], json!({}));
        assert!(w.store().sessions_of(&finished).unwrap().is_empty());
        w.kept(&terminal, How::default());
        w.store().leave(&reviewer).unwrap();
        assert_eq!(terminals(&w.twice(How::default())), [terminal.as_str()]);
    }
}

#[test]
fn a_new_program_in_the_terminal_during_the_sweep_restarts_the_idle_wait() {
    let w = World::new();
    let handle = w.executor_on("done", "sim/jump");
    let terminal = w.agent(Agent::named(&handle));
    w.sweep();
    let pid = w.new_pid();
    let (swept, warned) = sweep_changing(&w, 1, || {
        w.edit_terminal(&terminal, |t| {
            t.insert("pid".into(), json!(pid));
            t.insert("agent".into(), json!("codex"));
            t.insert("argv".into(), json!(["codex", "-n", handle, "hi"]));
        });
    });
    assert!(warned.is_empty(), "{warned}");
    assert!(swept.closed.is_empty(), "{swept:?}");
    assert!(w.has_terminal(&terminal));
    assert!(w.store().sessions_of(&handle).unwrap().is_empty());
    assert_eq!(w.reap_data()["idle"], json!({}));
    assert_eq!(w.sweep().idle.as_slice(), std::slice::from_ref(&handle));
    assert_eq!(terminals(&w.sweep()), [terminal.as_str()]);
    assert_eq!(
        w.store().sessions_of(&handle).unwrap()[0].claude_session,
        "codex:"
    );
}

#[test]
fn a_worker_becoming_a_protected_session_during_the_sweep_is_kept() {
    for kind in ["orchestrator", "chat"] {
        let w = World::new();
        let handle = w.executor_on("done", "sim/jump");
        let terminal = w.agent(Agent::named(&handle));
        w.sweep();
        let (swept, warned) = sweep_changing(&w, 1, || {
            Raw::of(&w.f.other_database())
                .row("session", json!(handle))
                .set(json!({"kind": kind}))
                .unwrap();
        });
        assert_eq!(w.store().session(&handle).unwrap().row.kind, kind);
        assert!(warned.is_empty(), "{warned}");
        assert!(swept.closed.is_empty());
        assert!(w.has_terminal(&terminal));
        assert!(w.store().sessions_of(&handle).unwrap().is_empty());
        assert_eq!(w.reap_data()["idle"], json!({}));
        w.kept(&terminal, How::default());
    }
}

#[test]
fn an_unconfirmed_final_look_restarts_the_idle_wait() {
    for case in 0..6 {
        let w = World::new();
        let handle = w.executor_on("done", "sim/jump");
        let terminal = w.agent(Agent::named(&handle));
        let pid = w.pid_of(&terminal);
        let answer = match case {
            0 => Err(LaunchError("hc get: no answer in 120 s".into())),
            1 => Ok("{}".to_string()),
            // another handle, or the same one with no pid, another pid, or no agent
            2 => Ok(
                json!({"state": "live", "pid": pid, "agent": "claude", "status": "idle",
                           "argv": ["claude", "-n", "x", "hi"]})
                .to_string(),
            ),
            3 | 4 => Ok(
                json!({"state": "live", "agent": "claude", "status": "idle", "name": handle,
                           "pid": if case == 3 { Value::Null } else { json!(pid + 1) }})
                .to_string(),
            ),
            _ => Ok(
                json!({"state": "live", "pid": pid, "status": "idle", "name": handle,
                           "argv": ["zsh"]})
                .to_string(),
            ),
        };
        w.sweep();
        let looks = Cell::new(0);
        let mut hooked = Hooked::new(&w);
        hooked.instead = Box::new(|args| {
            if args[0] == "get" {
                looks.set(looks.get() + 1);
                if looks.get() == 2 {
                    return Some(answer.clone());
                }
            }
            None
        });
        let (swept, warned) = w.sweep_with(&hooked, How::default());
        assert_eq!(looks.get(), 2);
        assert!(warned.is_empty(), "{warned}");
        assert!(swept.closed.is_empty(), "{answer:?}");
        assert!(w.has_terminal(&terminal));
        assert!(w.store().sessions_of(&handle).unwrap().is_empty());
        assert_eq!(w.reap_data()["idle"], json!({}));
        assert_eq!(w.sweep().idle.as_slice(), std::slice::from_ref(&handle));
        assert_eq!(terminals(&w.sweep()), [terminal.as_str()]);
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
        let terminal = w.agent(Agent::named(&handle));
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
        assert!(w.has_terminal(&terminal));
        assert!(w.store().sessions_of(&handle).unwrap().is_empty());
        assert_eq!(w.reap_data()["idle"], json!({}));
        assert_eq!(w.sweep().idle.as_slice(), std::slice::from_ref(&handle));
        assert_eq!(terminals(&w.sweep()), [terminal.as_str()]);
    }
}

#[test]
fn an_agent_that_claims_work_during_the_sweep_is_kept() {
    for claim_after in [1, 2] {
        let w = World::new();
        let handle = w.executor_on("done", "sim/jump");
        let terminal = w.agent(Agent::named(&handle));
        w.task("sim/dock", None);
        w.sweep();
        let (swept, _) = sweep_changing(&w, claim_after, || {
            w.store().claim("sim/dock", &handle).unwrap();
        });
        assert_eq!(w.store().get("sim/dock").unwrap().status, "in_progress");
        assert_eq!(swept.closed, []);
        assert!(w.has_terminal(&terminal));
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
    let db = w.f.other_database();
    let removed = gone.clone();
    let machine = w.machine();
    let processes = move |pid| {
        reads.set(reads.get() + 1);
        if reads.get() == 2 {
            // Final eligibility was confirmed; the board then refuses its close record.
            Raw::of(&db)
                .row("session", json!(removed))
                .delete()
                .unwrap();
        }
        (machine.processes)(pid)
    };
    let (swept, warned) = w.sweep_with(
        &w.hc(),
        How {
            processes: Some(&processes),
            ..How::default()
        },
    );
    assert_eq!(terminals(&swept), [second.as_str()]);
    assert!(
        warned.starts_with(&format!(
            "· idle sweep kept {first} ({gone}): the board took no record: "
        )) && warned.ends_with('\n')
            && warned.lines().count() == 1,
        "{warned}"
    );
    assert!(w.has_terminal(&first));
    assert!(!w.has_terminal(&second));
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
