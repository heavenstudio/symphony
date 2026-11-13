//! `tasks wait --as HANDLE [--timeout S]`: what a callback would push the session, printed on its
//! own output instead, once there is any; a session with no channel runs it as a background command
//! and runs it again when it exits. The board's clock is pinned: each wait between looks moves it
//! on, says `STEP` and reads a line, which is where a test acts.

use super::command::case;
use crate::{Case, Ran};
use serde_json::json;
use std::io::Read;

/// What it says when it times out after `seconds` with nothing to say.
fn nothing(seconds: u32) -> String {
    format!("WAIT nothing changed in {seconds} s\n")
}

/// An orchestrator with no channel, past its first read, which says the whole digest.
fn orchestrator(c: &Case) -> String {
    let orch = c.register("orchestrator");
    let first = c.ok(&orch, &["wait", "--timeout", "0"]);
    assert!(first.starts_with("PULL "), "{first}");
    assert!(first.contains(" executing=0 "), "{first}");
    orch
}

/// A reader of the lines a pinned clock reads between looks: `act` runs at the first of them.
struct Between<F: FnOnce()> {
    act: Option<F>,
}

impl<F: FnOnce()> Read for Between<F> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.act.take() {
            Some(act) => {
                act();
                buf[0] = b'\n';
                Ok(1)
            }
            None => Ok(0),
        }
    }
}

/// `tasks wait ARGS` as `who`, `act` run at its first wait between looks.
fn wait_with(c: &Case, who: &str, args: &[&str], act: impl FnOnce()) -> Ran {
    let mut env = c.env.clone();
    env.vars.insert("TASKS_SESSION".into(), who.into());
    let args: Vec<String> = ["wait"].iter().chain(args).map(|a| a.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut input = Between { act: Some(act) };
    let code = symphony_board::cli::main(&args, &env, &mut input, &mut out, &mut err);
    Ran {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

#[test]
fn wait_says_what_changed_since_its_last_read_once() {
    let c = case("wait-changed");
    let orch = orchestrator(&c);
    let other = c.register("orchestrator");
    // Nothing changed: nothing to say.
    assert_eq!(c.ok(&orch, &["wait", "--timeout", "0"]), nothing(0));
    c.ok(&other, &["create", "sim/free", "Free", "--no-role"]);
    assert_eq!(
        c.ok(&orch, &["wait", "--timeout", "0"]),
        "NEW sim/free ready — Free\nPULL ready_unstaffed=1 (was 0)\nready_unstaffed p2 sim/free — Free\n"
    );
    // Said once: the next read has nothing.
    assert_eq!(c.ok(&orch, &["wait", "--timeout", "0"]), nothing(0));
    // A post to it is said, once.
    c.ok(&other, &["say", "sim/free", "take it", "--to", &orch]);
    let said = c.ok(&orch, &["wait", "--timeout", "0"]);
    assert!(said.starts_with("MSG #"), "{said}");
    let from = format!(" sim/free from {other} — take it\n");
    assert!(said.ends_with(&from), "{said}");
    assert_eq!(c.ok(&orch, &["wait", "--timeout", "0"]), nothing(0));
}

#[test]
fn wait_never_says_its_own_writes() {
    let c = case("wait-own");
    let orch = orchestrator(&c);
    c.ok(&orch, &["create", "sim/mine", "Mine"]);
    c.ok(&orch, &["role", "sim/mine", "orchestrator"]);
    c.ok(&orch, &["wait", "--timeout", "0"]);
    c.ok(&orch, &["move", "sim/mine", "blocked"]);
    assert_eq!(c.ok(&orch, &["wait", "--timeout", "0"]), nothing(0));
}

/// An output a reader closed: it takes nothing.
struct Closed;

impl std::io::Write for Closed {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::ErrorKind::BrokenPipe.into())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_line_a_wait_could_not_print_is_printed_by_the_next_one() {
    let c = case("wait-closed");
    let orch = orchestrator(&c);
    let other = c.register("orchestrator");
    c.ok(&other, &["create", "sim/free", "Free", "--no-role"]);
    let mut env = c.env.clone();
    env.vars.insert("TASKS_SESSION".into(), orch.clone());
    let args: Vec<String> = ["wait", "--timeout", "0"].map(String::from).to_vec();
    let mut err = Vec::new();
    symphony_board::cli::main(&args, &env, &mut std::io::empty(), &mut Closed, &mut err);
    // Not printed, so not delivered: the next wait prints it once its retry falls due, five
    // seconds on.
    let again = c.ok(&orch, &["wait", "--timeout", "20"]);
    assert!(again.starts_with("NEW sim/free ready — Free\n"), "{again}");
}

#[test]
fn wait_blocks_until_something_changes() {
    let c = case("wait-blocks");
    let orch = orchestrator(&c);
    let other = c.register("orchestrator");
    let ran = wait_with(&c, &orch, &["--timeout", "30"], || {
        c.ok(&other, &["create", "sim/late", "Late", "--no-role"]);
    });
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert!(
        ran.out.starts_with("NEW sim/late ready — Late\n"),
        "{}",
        ran.out
    );
    // It looked, waited once, and looked again.
    assert_eq!(ran.err.matches("STEP ").count(), 1, "{}", ran.err);
}

#[test]
fn wait_times_out_saying_nothing_changed() {
    let c = case("wait-timeout");
    let orch = orchestrator(&c);
    let ran = wait_with(&c, &orch, &["--timeout", "3"], || {});
    assert_eq!((ran.code, ran.out), (0, nothing(3)), "{}", ran.err);
    // It waited two seconds between looks, then the one second left.
    let steps: Vec<&str> = ran.err.lines().filter(|l| l.starts_with("STEP ")).collect();
    assert_eq!(steps.len(), 2, "{}", ran.err);
}

#[test]
fn waiting_beats_for_the_session_as_a_monitor_does() {
    let c = case("wait-beats");
    let orch = orchestrator(&c);
    let ran = wait_with(&c, &orch, &["--timeout", "40"], || {});
    assert_eq!((ran.code, ran.out), (0, nothing(40)), "{}", ran.err);
    // It looked at 0 s, every 16 s while the board stood still (its marker read every 2 s), and
    // beat at the first look and at the first 20 s or more after: at 32 s.
    let seen = c.store().client().query(
        "RETURN type::record('session', $handle).last_seen;",
        json!({"handle": orch}),
    );
    let seen = seen.unwrap().first().and_then(|seen| seen.as_f64());
    assert_eq!(seen, Some(crate::common::NOW + 32.0));
}

#[test]
fn wait_is_refused_while_a_channel_takes_the_pushes() {
    let c = case("wait-channel");
    let orch = c.register("orchestrator");
    // A registered Codex thread, as `tasks callback` records it.
    let registered = c.store().client().query(
        "UPSERT type::record('callback_state', $handle) SET handle = $handle, address = 'codex:abcdef01' RETURN NONE;",
        json!({"handle": orch}),
    );
    registered.unwrap();
    let ran = c.run_as(&orch, &["wait", "--timeout", "0"], "");
    assert_eq!(ran.code, 1, "{}", ran.out);
    assert!(ran.err.contains("callback --off"), "{}", ran.err);
}

/// What the app publishes when the owner decides a card.
pub(super) const OWNER: &str = "OWNER accepted card 7 in docs/DECISIONS.md — land it";

#[test]
fn wait_says_what_the_owner_says_while_no_channel_holds_the_seat() {
    let c = case("wait-owner");
    let orch = orchestrator(&c);
    c.store().owner_callback("asks-1", OWNER).unwrap();
    assert_eq!(
        c.ok(&orch, &["wait", "--timeout", "0"]),
        format!("{OWNER}\n")
    );
    assert_eq!(c.ok(&orch, &["wait", "--timeout", "0"]), nothing(0));
}

#[test]
fn wait_says_a_pane_blocked_on_the_work_it_orchestrates() {
    let c = case("wait-blocked");
    let orch = orchestrator(&c);
    let worker = c.register("worker");
    c.ok(&orch, &["create", "sim/pane", "Pane"]);
    c.ok(&orch, &["role", "sim/pane", "orchestrator"]);
    c.ok(&worker, &["claim", "sim/pane"]);
    let launch = symphony_board::NewLaunch {
        pane: "t2".into(),
        ..symphony_board::NewLaunch::new("sim/pane", &worker, "executor")
    };
    c.store().record_launch(&launch).unwrap();
    let terminal = json!({"address": "t2", "agent": "codex", "status": "blocked", "cwd": "/wt", "state": "live"});
    let state = &c.env.vars["FAKE_HC_STATE"];
    std::fs::write(state, json!({"terminals": [terminal]}).to_string()).unwrap();
    // Two looks that see it waiting say it, once.
    let said: String = (0..3)
        .map(|_| c.ok(&orch, &["wait", "--timeout", "0"]))
        .collect();
    let blocked = format!(
        "BLOCKED {worker} sim/pane t2 — observed waiting on an approval or a question; inspect its terminal before acting\n"
    );
    assert_eq!(said.matches(&blocked).count(), 1, "{said}");
}

#[test]
fn wait_refuses_a_timeout_that_is_no_number_of_seconds() {
    let c = case("wait-bad-timeout");
    let orch = c.register("orchestrator");
    for timeout in ["--timeout=-1", "--timeout=inf", "--timeout=NaN"] {
        let ran = c.run_as(&orch, &["wait", timeout], "");
        assert_eq!(ran.code, 1, "{timeout}: {}", ran.out);
        assert!(
            ran.err
                .contains("--timeout is a number of seconds, 0 or more"),
            "{timeout}: {}",
            ran.err
        );
    }
}

#[test]
fn wait_needs_a_session_still_here() {
    let c = case("wait-gone");
    let orch = c.register("orchestrator");
    c.ok(&orch, &["leave"]);
    let ran = c.run_as(&orch, &["wait", "--timeout", "0"], "");
    assert_eq!(ran.code, 1, "{}", ran.out);
}
