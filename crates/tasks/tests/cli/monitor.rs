//! Monitor/attach harness with TASKS_NOW pinned: time advances only at waits.
//! A wait writes STEP <now> to stderr and reads a line before the next look;
//! between(n) changes state between look n and its successor.
//! Each case owns its repository, slot locks and landed branches. A stand-in hc
//! returns a file of terminals and counts each query. The in-process command uses
//! the case's stdin/stderr, keeping steps under the test's control.

use crate::beat::{bash, tool};
use crate::common::NOW;
use crate::{Case, git, repository, run, scratch};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::rc::Rc;
use symphony_board::health::{LIVE_SECONDS, STALE_SECONDS};
use symphony_board::{NewLaunch, NewTask, Store, cli, sys};
mod fixtures;
use fixtures::launched_child;

/// Three-second polls past the lost line.
const GONE_POLLS: usize = ((LIVE_SECONDS + STALE_SECONDS) / 3.0) as usize + 3;
/// Thirty-second waits past the lost line.
const GONE_STEPS: usize = ((LIVE_SECONDS + STALE_SECONDS) / 30.0) as usize + 3;

/// How a monitor runs: `--polls`, `--beat`, `--interval` and `TASKS_PULL_SECONDS`.
#[derive(Clone, Copy)]
struct Watch {
    polls: usize,
    beat: f64,
    pull_every: f64,
    interval: f64,
}

impl Watch {
    fn polls(polls: usize) -> Watch {
        Watch {
            polls,
            beat: 20.0,
            pull_every: 600.0,
            interval: 3.0,
        }
    }

    /// Thirty-second waits past the lost line.
    fn past_the_lost_line() -> Watch {
        Watch {
            interval: 30.0,
            ..Watch::polls(GONE_STEPS)
        }
    }
}

/// Nothing between the looks.
fn idle(_: &Monitor, _: usize) {}

struct Monitor {
    c: Case,
    dir: PathBuf,
    repo: PathBuf,
    now: Cell<f64>,
    lines: RefCell<Vec<String>>,
    /// What hc answers, once `hc()` is called.
    panes: RefCell<Value>,
    slots: Cell<usize>,
}

/// What the monitor says on stderr, kept where its stdin reads the last step from.
struct Said(Rc<RefCell<Vec<u8>>>);

impl Write for Said {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The monitor's stdin: each read is a wait's step, taken after its `STEP <now>` line.
struct Steps<'a> {
    said: Rc<RefCell<Vec<u8>>>,
    monitor: &'a Monitor,
    between: &'a dyn Fn(&Monitor, usize),
    taken: usize,
}

impl Read for Steps<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let said = String::from_utf8_lossy(&self.said.borrow()).into_owned();
        let at = said
            .lines()
            .rev()
            .find_map(|line| line.strip_prefix("STEP "))
            .expect("a wait says its step before it reads");
        self.monitor.now.set(at.parse().unwrap());
        (self.between)(self.monitor, self.taken);
        self.taken += 1;
        buf[0] = b'\n';
        Ok(1)
    }
}

impl Monitor {
    fn new(name: &str) -> Monitor {
        let dir = scratch(&format!("monitor-{name}"));
        let repo = repository(&dir.join("repo"));
        let mut c = Case::new();
        c.env.cwd = repo.clone();
        // hc cannot be asked until `hc()`.
        let none = dir.join("no-hc").display().to_string();
        c.env.vars.insert("HC_BIN".into(), none);
        Monitor {
            c,
            dir,
            repo,
            now: Cell::new(NOW),
            lines: RefCell::new(Vec::new()),
            panes: RefCell::new(json!([])),
            slots: Cell::new(0),
        }
    }

    /// The case at the monitor's clock.
    fn at(&self) -> Case {
        let mut env = self.c.env.clone();
        env.vars
            .insert("TASKS_NOW".into(), format!("{:?}", self.now.get()));
        Case { env }
    }

    fn ok(&self, who: &str, args: &[&str]) -> String {
        self.at().ok(who, args)
    }

    fn register(&self, kind: &str) -> String {
        self.at().register(kind)
    }

    fn child(&self, kind: &str, parent: &str) -> String {
        self.at().register_with(kind, &["--parent", parent])
    }

    fn board(&self) -> Store {
        self.at().store()
    }

    fn health(&self, handle: &str) -> &'static str {
        self.board().session(handle).unwrap().health.as_str()
    }

    fn last_seen(&self, handle: &str) -> f64 {
        self.board().session(handle).unwrap().row.last_seen
    }

    /// A tool call's hook at the monitor's clock.
    fn beat(&self, event: &Value) {
        let ran = run(&self.at().env, &["beat"], &event.to_string());
        assert_eq!((ran.code, ran.out.as_str(), ran.err.as_str()), (0, "", ""));
    }

    /// hc answers the terminals as they are when the monitor starts; the file returned counts its calls.
    fn hc(&mut self) -> PathBuf {
        let (state, calls) = (self.dir.join("hc.json"), self.dir.join("hc.calls"));
        fs::write(&calls, "").unwrap();
        let bin = format!(
            "sh -c 'echo >>\"$1\"; cat \"$0\"' '{}' '{}'",
            state.display(),
            calls.display()
        );
        self.c.env.vars.insert("HC_BIN".into(), bin);
        calls
    }

    /// hc as the crate's `fake-hc`, found on PATH as `hc`; its terminals `terminals` in the file
    /// returned, which a case rewrites between looks.
    fn fake_hc(&mut self, terminals: Value) -> PathBuf {
        let bin = self.dir.join("bin");
        fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_fake-hc"), bin.join("hc")).unwrap();
        let state = self.dir.join("fake-hc.json");
        fs::write(&state, json!({ "terminals": terminals }).to_string()).unwrap();
        let vars = &mut self.c.env.vars;
        vars.remove("HC_BIN");
        vars.insert("PATH".into(), format!("{}:/usr/bin:/bin", bin.display()));
        vars.insert("FAKE_HC_STATE".into(), state.display().to_string());
        state
    }

    fn asked(calls: &PathBuf) -> usize {
        fs::read_to_string(calls).unwrap().lines().count()
    }

    /// `attach --resume HANDLE` until its polls are done, `between(n)` after its n-th wait; what it
    /// printed after its first line goes to `lines`.
    fn watch(&self, handle: &str, w: Watch, between: &dyn Fn(&Monitor, usize)) {
        let state = json!({"terminals": *self.panes.borrow()});
        fs::write(self.dir.join("hc.json"), state.to_string()).unwrap();
        let mut env = self.at().env;
        env.vars
            .insert("TASKS_PULL_SECONDS".into(), format!("{:?}", w.pull_every));
        let (polls, interval, beat) = (
            w.polls.to_string(),
            format!("{:?}", w.interval),
            format!("{:?}", w.beat),
        );
        let args: Vec<String> = [
            "attach",
            "--resume",
            handle,
            "--polls",
            &polls,
            "--interval",
            &interval,
            "--beat",
            &beat,
        ]
        .iter()
        .map(|a| a.to_string())
        .collect();
        let said = Rc::new(RefCell::new(Vec::new()));
        let mut steps = Steps {
            said: said.clone(),
            monitor: self,
            between,
            taken: 0,
        };
        let mut out = Vec::new();
        let code = cli::main(&args, &env, &mut steps, &mut out, &mut Said(said.clone()));
        let said = String::from_utf8_lossy(&said.borrow()).into_owned();
        let errors: Vec<&str> = said.lines().filter(|l| !l.starts_with("STEP ")).collect();
        assert_eq!(code, 0, "{errors:?}");
        let printed = String::from_utf8(out).unwrap();
        let mut printed = printed.lines();
        let first = printed.next().unwrap_or_default();
        assert!(
            first.starts_with(&format!("ATTACHED {handle} ")),
            "{first:?}"
        );
        self.lines.borrow_mut().extend(printed.map(str::to_string));
    }

    fn lines(&self) -> Vec<String> {
        self.lines.borrow().clone()
    }

    fn starting(&self, prefixes: &[&str]) -> Vec<String> {
        self.lines()
            .into_iter()
            .filter(|l| prefixes.iter().any(|p| l.starts_with(p)))
            .collect()
    }

    fn has(&self, line: &str) -> bool {
        self.lines.borrow().iter().any(|l| l == line)
    }

    fn pulls(&self) -> Vec<String> {
        self.starting(&["PULL "])
    }

    /// A test run in `worktree` this process holds, as a project's slot tool writes one in the
    /// folder its `[board] slot_locks` names: a locked slot file (`holding`) or a place in the line
    /// (`waiting`). Dropping what is returned ends it.
    fn slot(
        &self,
        worktree: &str,
        holding: bool,
        since: f64,
        handle: Option<&str>,
        recipe: &str,
    ) -> File {
        fs::write(
            self.repo.join("symphony.toml"),
            "[board]\nslot_locks = \".git/test-slots\"\n",
        )
        .unwrap();
        let slots = self.repo.join(".git").join("test-slots");
        fs::create_dir_all(&slots).unwrap();
        let n = self.slots.get() + 1;
        self.slots.set(n);
        let path = if holding {
            slots.join(format!("slot-{n}.lock"))
        } else {
            slots.join(format!("slot-{n:012}.place"))
        };
        let record = json!({"pid": std::process::id(), "worktree": worktree, "recipe": recipe,
                            "since": since, "handle": handle});
        fs::write(&path, record.to_string()).unwrap();
        let held = OpenOptions::new()
            .read(true)
            .append(true)
            .open(&path)
            .unwrap();
        held.lock().unwrap();
        held
    }

    /// `branch` with a commit of its own, merged into main.
    fn land(&self, branch: &str) {
        for args in [
            &["checkout", "-q", "-b", branch][..],
            &["commit", "-q", "--allow-empty", "-m", "work"],
            &["checkout", "-q", "main"],
            &["merge", "-q", "--no-ff", "-m", "land", branch],
        ] {
            git(&self.repo, args);
        }
    }
}

#[test]
fn messages_arrive_once_each() {
    let m = Monitor::new("once");
    let (a, b) = (m.register("worker"), m.register("worker"));
    m.ok(&a, &["create", "sim/a", "A"]);
    m.watch(&b, Watch::polls(4), &|m, n| {
        if n == 0 {
            m.ok(&a, &["say", "sim/a", "first", "--to", &b]);
        }
        if n == 2 {
            m.ok(&a, &["say", "sim/a", "second", "--to", &b]);
        }
    });
    assert_eq!(
        m.starting(&["MSG "]),
        [
            format!("MSG sim/a from {a} to {b} — first"),
            format!("MSG sim/a from {a} to {b} — second")
        ]
    );
}

#[test]
fn unread_messages_print_at_the_first_look() {
    let m = Monitor::new("unread");
    let (a, b) = (m.register("worker"), m.register("worker"));
    m.ok(&a, &["create", "sim/a", "A"]);
    m.ok(&a, &["say", "sim/a", "two\nlines", "--to", &b]);
    m.watch(&b, Watch::polls(0), &idle);
    assert_eq!(
        m.lines(),
        [format!("MSG sim/a from {a} to {b} — two lines")]
    );
    m.lines.borrow_mut().clear();
    m.watch(&b, Watch::polls(0), &idle);
    assert_eq!(m.lines(), Vec::<String>::new());
}

#[test]
fn role_changes_are_reported() {
    let m = Monitor::new("roles");
    let orch = m.register("orchestrator");
    let exe = m.register("worker");
    m.ok(&orch, &["create", "sim/a", "A"]);
    m.ok(&orch, &["role", "sim/a", "orchestrator"]);
    m.watch(&orch, Watch::polls(3), &|m, n| {
        if n == 0 {
            m.ok("", &["claim", "sim/a", "--as", &exe]);
        }
        if n == 1 {
            m.ok("", &["move", "sim/a", "review", "--as", &exe]);
        }
    });
    assert_eq!(
        m.starting(&["ROLE ", "STATUS "]),
        [
            format!("ROLE sim/a {exe} executor, in_progress"),
            "STATUS sim/a in_progress → review".to_string(),
        ]
    );
}

#[test]
fn its_own_roles_are_not_echoed() {
    let m = Monitor::new("own-roles");
    let a = m.register("worker");
    m.ok(&a, &["create", "sim/a", "A"]);
    m.watch(&a, Watch::polls(2), &|m, n| {
        if n == 0 {
            m.ok(&a, &["claim", "sim/a"]);
        }
        if n == 1 {
            m.ok(&a, &["release", "sim/a"]);
        }
    });
    assert_eq!(m.starting(&["ROLE "]), Vec::<String>::new());
}

#[path = "monitor_grandchild.rs"]
mod grandchild;

#[test]
fn a_released_role_is_reported() {
    let m = Monitor::new("released");
    let (orch, exe) = (m.register("orchestrator"), m.register("worker"));
    m.ok(&orch, &["create", "sim/a", "A"]);
    m.ok(&orch, &["role", "sim/a", "orchestrator"]);
    m.ok(&exe, &["claim", "sim/a"]);
    m.watch(&orch, Watch::polls(1), &|m, n| {
        if n == 0 {
            m.ok(&exe, &["release", "sim/a"]);
        }
    });
    assert!(
        m.has(&format!("ROLE sim/a {exe} none, ready")),
        "{:?}",
        m.lines()
    );
    assert!(m.starting(&["STATUS "]).is_empty(), "{:?}", m.lines());
}

#[test]
fn the_monitor_of_a_session_that_left_stops() {
    let m = Monitor::new("left");
    let a = m.register("worker");
    let w = Watch {
        beat: 3.0,
        ..Watch::polls(1000) // would run 1000 polls if it did not stop
    };
    m.watch(&a, w, &|m, n| {
        if n == 0 {
            m.ok(&a, &["leave"]);
        }
    });
    let lines = m.lines();
    assert!(
        lines
            .last()
            .is_some_and(|l| l.starts_with("WATCH heartbeat refused")),
        "{lines:?}"
    );
    assert!(lines.len() < 3, "{lines:?}");
}

#[test]
fn verdicts_are_their_own_event() {
    let m = Monitor::new("verdicts");
    let (exe, rev) = (m.register("worker"), m.register("reviewer"));
    m.ok(&exe, &["create", "sim/a", "A"]);
    m.ok(&exe, &["claim", "sim/a"]);
    m.ok(&exe, &["move", "sim/a", "review"]);
    m.ok(&rev, &["role", "sim/a", "reviewer"]);
    m.watch(&exe, Watch::polls(2), &|m, n| {
        if n == 0 {
            m.ok(&rev, &["verdict", "sim/a", "changes", "the hash moved"]);
        }
    });
    let verdict = format!("VERDICT sim/a from {rev} — changes: the hash moved");
    assert!(m.has(&verdict), "{:?}", m.lines());
}

#[test]
fn the_heartbeat_keeps_the_session_live() {
    let m = Monitor::new("heartbeat");
    let a = m.register("worker");
    m.now.set(m.last_seen(&a));
    let seen = RefCell::new(Vec::new());
    m.watch(&a, Watch::polls(60), &|m, n| {
        if n == 59 {
            // 3 minutes of polling, its monitor still running
            seen.borrow_mut().push(m.health(&a));
        }
    });
    assert_eq!(seen.into_inner(), ["live"]);
}

/// An orchestrator, a worker under it and `sim/a`, which the worker claimed; the clock at the
/// worker's last sign of life: both handles.
fn quiet_child(m: &Monitor) -> (String, String) {
    let orch = m.register("orchestrator");
    let child = m.child("worker", &orch);
    // Nobody's work: the parent files it but does not orchestrate it.
    m.ok(&orch, &["create", "sim/a", "A", "--no-role"]);
    m.ok(&child, &["claim", "sim/a"]);
    m.now.set(m.last_seen(&child));
    (orch, child)
}

#[test]
fn a_child_going_lost_is_reported_to_its_parent() {
    let m = Monitor::new("lost");
    let (orch, child) = quiet_child(&m);
    m.watch(&orch, Watch::past_the_lost_line(), &idle);
    assert_eq!(
        m.starting(&["STALE "]),
        [format!("STALE {child} — no sign of it for 90 s")]
    );
    assert_eq!(
        m.starting(&["LOST "]),
        [format!("LOST {child} — its work is claimable: sim/a")]
    );
}

/// `slug`, made by `orch`, claimed by `child`, and approved by `rev`.
fn finished(m: &Monitor, orch: &str, child: &str, rev: &str, slug: &str) {
    m.ok(orch, &["create", slug, "Old"]);
    m.ok(child, &["claim", slug]);
    m.ok(child, &["move", slug, "review"]);
    m.ok(rev, &["role", slug, "reviewer"]);
    m.ok(rev, &["verdict", slug, "approve", "good"]);
}

#[test]
fn a_child_that_stops_is_lost_naming_only_its_claimable_work() {
    let m = Monitor::new("claimable");
    let (orch, rev) = (m.register("orchestrator"), m.register("reviewer"));
    let child = m.child("worker", &orch);
    finished(&m, &orch, &child, &rev, "sim/old");
    m.ok(&orch, &["create", "sim/new", "New"]);
    m.ok(&child, &["claim", "sim/new"]);
    m.now.set(m.last_seen(&child));
    m.watch(&orch, Watch::past_the_lost_line(), &idle);
    assert_eq!(
        m.starting(&["LOST "]),
        [format!("LOST {child} — its work is claimable: sim/new")]
    );
}

#[test]
fn a_child_holding_only_finished_work_ends_quietly() {
    let m = Monitor::new("quietly");
    let (orch, rev) = (m.register("orchestrator"), m.register("reviewer"));
    let child = m.child("worker", &orch);
    finished(&m, &orch, &child, &rev, "sim/a");
    m.now.set(m.last_seen(&child));
    m.watch(&orch, Watch::past_the_lost_line(), &idle);
    let about: Vec<String> = m
        .starting(&["STALE", "LOST"])
        .into_iter()
        .filter(|l| l.contains(&child))
        .collect();
    assert_eq!(about, Vec::<String>::new(), "{:?}", m.lines());
    assert_eq!(m.health(&child), "left");
}

#[test]
fn a_child_beating_through_its_tool_calls_stays_live() {
    let m = Monitor::new("beating");
    let (orch, child) = quiet_child(&m);
    let bind = format!("TASKS_SESSION={child} just tasks inbox");
    m.beat(&bash(&bind).agent("agent-1").0);
    // A tool call every 30 s, and never a board command.
    m.watch(&orch, Watch::past_the_lost_line(), &|m, _| {
        m.beat(&tool("Edit", "agent-1"))
    });
    let about: Vec<String> = m
        .starting(&["STALE", "LOST"])
        .into_iter()
        .filter(|l| l.contains(&child))
        .collect();
    assert_eq!(about, Vec::<String>::new(), "{:?}", m.lines());
    assert_eq!(m.health(&child), "live");
}

#[test]
fn who_agrees_with_the_monitor_on_who_is_connected() {
    let m = Monitor::new("who-agrees");
    let (orch, rev) = (m.register("orchestrator"), m.register("reviewer"));
    let (done, working) = (m.child("worker", &orch), m.child("worker", &orch));
    finished(&m, &orch, &done, &rev, "sim/a");
    m.ok(&orch, &["create", "sim/b", "B"]);
    m.ok(&working, &["claim", "sim/b"]);
    m.now.set(NOW + (GONE_POLLS * 3) as f64);
    m.beat(
        &bash(&format!("just tasks --as {working} inbox"))
            .agent("agent-2")
            .0,
    );
    let sessions = m.board().sessions().unwrap();
    let health = |h: &str| {
        sessions
            .iter()
            .find(|s| s.row.handle == h)
            .unwrap()
            .health
            .as_str()
    };
    assert_eq!((health(&done), health(&working)), ("left", "live"));
}

#[test]
fn new_ready_tasks_are_announced_to_orchestrators() {
    let m = Monitor::new("announced");
    let orch = m.register("orchestrator");
    m.watch(&orch, Watch::polls(2), &|m, n| {
        if n == 0 {
            m.board()
                .create(&NewTask::new("sim/new", "New", "owner"))
                .unwrap();
        }
    });
    assert!(m.has("NEW sim/new ready — New"), "{:?}", m.lines());
}

#[test]
fn a_task_created_by_the_recipient_is_not_echoed_as_new() {
    let m = Monitor::new("self-created");
    let orch = m.register("orchestrator");
    m.watch(&orch, Watch::polls(1), &|m, n| {
        if n == 0 {
            m.ok(&orch, &["create", "sim/new", "New"]);
        }
    });
    assert_eq!(m.board().get("sim/new").unwrap().created_by, orch);
    assert!(m.starting(&["NEW "]).is_empty(), "{:?}", m.lines());
}

#[test]
fn attach_registers_and_prints_the_handle_first() {
    let m = Monitor::new("registers");
    let ran = m
        .at()
        .run_as("", &["attach", "--kind", "worker", "--polls", "0"], "");
    assert_eq!(ran.code, 0, "{}", ran.err);
    let first = ran.out.lines().next().unwrap_or_default();
    let handle = first.split_whitespace().nth(1).unwrap_or_default();
    // The handle goes on each command as its own flag: no allow rule passes an env prefix.
    assert_eq!(
        first,
        format!("ATTACHED {handle} worker — run commands with --as {handle}")
    );
    let s = m.board().session(handle).unwrap().row;
    assert_eq!(
        (s.kind.as_str(), s.worktree.as_str(), s.host),
        ("worker", m.repo.to_str().unwrap(), sys::hostname())
    );
    assert!(s.pid.is_some());
}

#[test]
fn attach_resume_keeps_the_handle() {
    let m = Monitor::new("resume");
    let a = m.register("worker");
    let ran = m
        .at()
        .run_as("", &["attach", "--resume", &a, "--polls", "0"], "");
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert!(
        ran.out.starts_with(&format!("ATTACHED {a} worker")),
        "{}",
        ran.out
    );
}

#[test]
fn attach_resume_of_a_session_that_is_not_there_is_refused() {
    let m = Monitor::new("resume-nobody");
    let ran = m.at().run_as(
        "",
        &["attach", "--resume", "nobody-here", "--polls", "0"],
        "",
    );
    assert_eq!((ran.code, ran.out.as_str()), (1, ""));
    assert!(ran.err.contains("nobody-here"), "{}", ran.err);
}

// The orchestrator's monitor pulls work from the board, and does not call a waiting agent lost
// (`PullAttachTest`).

#[path = "monitor_pull.rs"]
mod pull_observations;

#[test]
fn a_child_waiting_on_its_verify_is_not_called_lost() {
    let m = Monitor::new("waiting-verify");
    let (orch, child) = launched_child(&m);
    let _run = m.slot("/wt/child", false, m.now.get(), Some(&child), "verify");
    m.watch(&orch, Watch::past_the_lost_line(), &idle);
    assert_eq!(
        m.starting(&["LOST ", "STALE "]),
        Vec::<String>::new(),
        "{:?}",
        m.lines()
    );
}

#[test]
fn the_same_child_is_lost_once_its_run_is_gone() {
    let m = Monitor::new("run-gone");
    let (orch, child) = launched_child(&m);
    let run = RefCell::new(Some(m.slot(
        "/wt/child",
        true,
        m.now.get(),
        Some(&child),
        "verify",
    )));
    m.watch(&orch, Watch::past_the_lost_line(), &|_, n| {
        if n == GONE_STEPS / 2 {
            drop(run.borrow_mut().take());
        }
    });
    assert_eq!(m.starting(&["LOST "]).len(), 1, "{:?}", m.lines());
}

#[test]
fn a_poll_asks_hc_once_however_many_children_are_quiet() {
    let mut m = Monitor::new("hc-once");
    let orch = m.register("orchestrator");
    let children: Vec<String> = (0..5).map(|_| m.child("worker", &orch)).collect();
    for (i, child) in children.iter().enumerate() {
        let slug = format!("sim/c{i}");
        m.ok(&orch, &["create", &slug, "C"]);
        m.ok(child, &["claim", &slug]);
    }
    let latest = children
        .iter()
        .map(|c| m.last_seen(c))
        .fold(f64::MIN, f64::max);
    m.now.set(latest);
    let calls = m.hc();
    let w = Watch {
        beat: 1000.0,
        ..Watch::past_the_lost_line()
    };
    m.watch(&orch, w, &idle);
    let asked = Monitor::asked(&calls);
    // At most once a poll, not once a child, and each digest's view.
    assert!(asked <= GONE_STEPS + 1 + 3, "{asked}");
    assert!(asked > GONE_STEPS / 2, "{asked}");
}

#[test]
fn a_poll_with_nobody_quiet_asks_nothing_but_the_digest_at_attach() {
    let mut m = Monitor::new("hc-quiet");
    let orch = m.register("orchestrator");
    let child = m.child("worker", &orch);
    m.ok(&orch, &["create", "sim/a", "A"]);
    m.ok(&child, &["claim", "sim/a"]);
    let calls = m.hc();
    m.watch(&orch, Watch::polls(3), &idle);
    assert_eq!(Monitor::asked(&calls), 1);
}

#[path = "monitor_panes.rs"]
mod panes;
