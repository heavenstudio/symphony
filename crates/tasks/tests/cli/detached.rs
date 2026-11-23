//! A write run as the `tasks` binary (`cli::main_detaching`) queues what each recipient hears and
//! answers before delivering it, which the detached `tasks dispatch` it starts does once the
//! command has ended; one that cannot start it, or that `TASKS_DISPATCH=inline` asks to, leaves the
//! delivery to the command, as before.

use crate::common::NOW;
use crate::{Case, Ran};
use serde_json::{Value, json};
use std::io::Read;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;
use symphony_board::cli::{Env, main_detaching};
use symphony_board::sys::hostname;

/// A socket of the test's own that keeps every frame sent to it, and goes with it.
pub(crate) struct Listener {
    pub(crate) path: PathBuf,
    socket: UnixListener,
}

impl Listener {
    pub(crate) fn new(name: &str) -> Listener {
        let path = PathBuf::from(format!(
            "/tmp/symphony-detached-{}-{name}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let socket = UnixListener::bind(&path).unwrap();
        socket.set_nonblocking(true).unwrap();
        Listener { path, socket }
    }

    /// The event lines sent until one holds `marker`, waited for up to the backstop of
    /// `allowance`: a detached dispatcher delivers it once a loaded machine runs it.
    pub(crate) fn until(&self, marker: &str, allowance: Duration) -> Vec<String> {
        let mut lines = Vec::new();
        let held = test_time::until(
            format!("a delivered line holds {marker:?}"),
            test_time::backstop(allowance),
            || {
                lines.extend(self.queued());
                lines.iter().any(|line| line.contains(marker)).then_some(())
            },
        );
        held.unwrap_or_else(|stall| panic!("{stall}; lines: {lines:?}"));
        lines
    }

    /// That a line already sent holds `marker`, waited for not at all: what the command delivered
    /// itself is in the socket before the command answers.
    #[track_caller]
    fn holding(&self, marker: &str) {
        let lines = self.queued();
        assert!(
            lines.iter().any(|line| line.contains(marker)),
            "no line held {marker:?} once the command answered; lines: {lines:?}"
        );
    }

    /// The event lines of every connection already made to it, waiting for none.
    fn queued(&self) -> Vec<String> {
        let mut lines = Vec::new();
        loop {
            match self.socket.accept() {
                Ok((stream, _)) => {
                    for frame in frames(stream).lines() {
                        let frame: serde_json::Value = serde_json::from_str(frame).unwrap();
                        let content = frame["message"]["content"].as_str().unwrap();
                        lines.extend(content.lines().map(String::from));
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return lines,
                Err(e) => panic!("callback listener: {e}"),
            }
        }
    }
}

/// What one connection sent, read until the sender closes it (nothing, for a probe), waited for up
/// to the backstop of the 5 s it once had.
fn frames(mut stream: UnixStream) -> String {
    stream.set_nonblocking(true).unwrap();
    let (mut bytes, mut buffer) = (Vec::new(), [0; 4096]);
    let closed = test_time::until(
        "a callback frame closed",
        test_time::backstop(Duration::from_secs(5)),
        || {
            loop {
                match stream.read(&mut buffer) {
                    Ok(0) => return Some(()),
                    Ok(n) => bytes.extend_from_slice(&buffer[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return None,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) => panic!("callback frame: {e}"),
                }
            }
        },
    );
    closed.unwrap();
    String::from_utf8(bytes).unwrap()
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A worker executing `slug`, its callback `listener`, and another session to write to it.
fn board(case: &Case, slug: &str, listener: &Listener) -> (String, String) {
    let recipient = case.register("worker");
    let writer = case.register("worker");
    let store = case.store();
    store
        .create(&symphony_board::NewTask::new(slug, "Detached", &writer))
        .unwrap();
    store.claim(slug, &recipient).unwrap();
    store
        .callback(
            &recipient,
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    (recipient, writer)
}

/// `tasks ARGS` as the `tasks` binary at `program` runs them in `env`.
fn run_as(env: &Env, program: &Path, args: &[&str]) -> Ran {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = main_detaching(&args, env, &mut "".as_bytes(), &mut out, &mut err, program);
    Ran {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

fn say(case: &Case, program: &Path, args: &[&str]) -> Ran {
    run_as(&case.env, program, args)
}

/// `tasks api` as the `tasks` binary at `program` runs it in `env`, `call` on its stdin.
fn api(env: &Env, program: &Path, call: &Value) -> Ran {
    let args = ["api".to_string()];
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let input = call.to_string();
    let code = main_detaching(
        &args,
        env,
        &mut input.as_bytes(),
        &mut out,
        &mut err,
        program,
    );
    Ran {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

/// This host's dispatch lease as it stands.
pub(crate) fn lease(case: &Case) -> Value {
    case.store()
        .raw()
        .row("callback_lease", json!(["dispatch", hostname()]))
        .fields(&["until", "asked", "served"])
        .unwrap()
}

/// This host's dispatch lease held by a dispatcher that died: one a write starts asks it for a pass
/// and waits, until the lease lapses ([`lapse`]).
pub(crate) fn held_by_the_dead(case: &Case) {
    case.store()
        .raw()
        .row("callback_lease", json!(["dispatch", hostname()]))
        .put(json!({"until": NOW + 3600.0, "revision": 1, "asked": 0, "served": 0}))
        .unwrap();
}

/// The dead holder's lease on this host lapses ([`held_by_the_dead`]): a dispatcher waiting on it
/// takes the host over.
pub(crate) fn lapse(case: &Case) {
    case.store()
        .raw()
        .row("callback_lease", json!(["dispatch", hostname()]))
        .set(json!({"until": 0}))
        .unwrap();
}

/// Wait until `done`, which a detached dispatcher brings about, up to the backstop of the minute it
/// once had.
pub(crate) fn until(done: impl Fn() -> bool, what: &str) {
    test_time::until_true(what, test_time::backstop(Duration::from_secs(60)), done).unwrap();
}

#[test]
fn a_post_queues_its_line_answers_and_the_dispatcher_it_starts_outwaits_a_dead_holder() {
    let case = Case::new();
    let listener = Listener::new("handed");
    let (recipient, writer) = board(&case, "tools/handed", &listener);
    let tasks = Path::new(env!("CARGO_BIN_EXE_tasks"));
    // The one the post starts asks the dead holder for a pass and waits.
    held_by_the_dead(&case);
    let ran = say(
        &case,
        tasks,
        &[
            "say",
            "tools/handed",
            "after the answer",
            "--to",
            &recipient,
            "--as",
            &writer,
        ],
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    let id = ran
        .out
        .strip_prefix("sent #")
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap()
        .to_string();
    // The command queued the post for its recipient before it answered, and delivered nothing.
    let queued = case
        .store()
        .raw()
        .row(
            "callback_event",
            json!([recipient, format!("message-{id}")]),
        )
        .field("delivered")
        .unwrap();
    // A row that says nothing of its delivery was not delivered.
    let queued = if queued.is_null() {
        json!(false)
    } else {
        queued
    };
    assert_eq!(queued, json!(false));
    until(
        || lease(&case)["asked"] == json!(1),
        "the post's dispatcher's ask",
    );
    // The waiting dispatcher takes the host over and delivers.
    lapse(&case);
    let lines = listener.until(&format!("MSG #{id} "), Duration::from_secs(60));
    let delivered = lines
        .iter()
        .filter(|l| l.contains("after the answer"))
        .count();
    assert_eq!(delivered, 1, "{lines:?}");
    until(
        || lease(&case)["until"] == json!(0) && lease(&case)["served"] == json!(1),
        "the taken lease's release",
    );
}

#[test]
fn tasks_dispatch_inline_keeps_the_dispatch_in_the_command() {
    let case = Case::new();
    let listener = Listener::new("inline");
    let (recipient, writer) = board(&case, "tools/inline", &listener);
    let mut env = case.env.clone();
    env.vars.insert("TASKS_DISPATCH".into(), "inline".into());
    let ran = run_as(
        &env,
        Path::new(env!("CARGO_BIN_EXE_tasks")),
        &[
            "say",
            "tools/inline",
            "said and sent",
            "--to",
            &recipient,
            "--as",
            &writer,
        ],
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    // Delivered before the command answered: the socket already holds the frame.
    listener.holding("said and sent");
}

#[test]
fn a_post_answers_first_and_the_detached_dispatcher_it_starts_delivers_it() {
    let case = Case::new();
    let listener = Listener::new("started");
    let (recipient, writer) = board(&case, "tools/detached", &listener);
    let ran = say(
        &case,
        Path::new(env!("CARGO_BIN_EXE_tasks")),
        &[
            "say",
            "tools/detached",
            "over to you",
            "--to",
            &recipient,
            "--as",
            &writer,
        ],
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(ran.err, "", "the command itself dispatched nothing");
    let id = ran
        .out
        .strip_prefix("sent #")
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap()
        .to_string();
    let lines = listener.until(&format!("MSG #{id} "), Duration::from_secs(60));
    let delivered = lines.iter().filter(|l| l.contains("over to you")).count();
    assert_eq!(delivered, 1, "{lines:?}");
}

#[test]
fn a_dispatcher_that_cannot_start_leaves_the_dispatch_to_the_command() {
    let case = Case::new();
    let listener = Listener::new("unstarted");
    let (recipient, writer) = board(&case, "tools/undetached", &listener);
    let ran = say(
        &case,
        Path::new("/nonexistent/tasks"),
        &[
            "say",
            "tools/undetached",
            "here and now",
            "--to",
            &recipient,
            "--as",
            &writer,
        ],
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    // Delivered before the command answered: the socket already holds the frame.
    listener.holding("here and now");
}

/// An orchestrator with its callback on `listener`, a task of its in review whose executor has gone
/// quiet with none, and a reviewer of it: the reviewer, and the environment of a clock past the
/// executor's last beat.
fn quiet_executor(case: &Case, slug: &str, listener: &Listener) -> (String, Env) {
    let orchestrator = case.register("orchestrator");
    let executor = case.register("worker");
    let reviewer = case.register("reviewer");
    let store = case.store();
    store
        .create(&symphony_board::NewTask::new(slug, "Quiet", &orchestrator))
        .unwrap();
    store
        .take_role(slug, &orchestrator, "orchestrator")
        .unwrap();
    store.claim(slug, &executor).unwrap();
    store.move_to(slug, "review", &executor).unwrap();
    store.take_role(slug, &reviewer, "reviewer").unwrap();
    store
        .callback(
            &orchestrator,
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    let mut later = case.env.clone();
    let after = NOW + symphony_board::health::LIVE_SECONDS + 1.0;
    later.vars.insert("TASKS_NOW".into(), after.to_string());
    (reviewer, later)
}

#[test]
fn a_post_no_executor_reads_brings_the_orchestrators_callback_the_boards_notice() {
    let case = Case::new();
    let listener = Listener::new("unread-post");
    let (reviewer, later) = quiet_executor(&case, "tools/unread-post", &listener);
    let ran = run_as(
        &later,
        Path::new(env!("CARGO_BIN_EXE_tasks")),
        &[
            "say",
            "tools/unread-post",
            "the hash moved",
            "--to",
            "@executor",
            "--as",
            &reviewer,
        ],
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    let lines = listener.until("reaches no live executor", Duration::from_secs(60));
    let notice: Vec<&String> = (lines.iter())
        .filter(|line| line.contains("reaches no live executor"))
        .collect();
    assert_eq!(notice.len(), 1, "{lines:?}");
    assert!(
        notice[0].starts_with("MSG #") && notice[0].contains(" tools/unread-post from board — "),
        "{lines:?}"
    );
}

/// A changes verdict made through `tasks api`, as `land-approve` makes one, reaches the
/// orchestrator's callback as a verdict, and the board's notice that no executor reads it after it.
#[test]
fn an_api_changes_verdict_brings_the_orchestrators_callback_the_verdict_and_the_notice() {
    let case = Case::new();
    let listener = Listener::new("unread-verdict");
    let (reviewer, later) = quiet_executor(&case, "tools/unread-verdict", &listener);
    let call = json!({"method": "verdict", "args": {"slug": "tools/unread-verdict",
        "handle": reviewer, "verdict": "changes", "text": "the hit misses"}});
    let ran = api(&later, Path::new(env!("CARGO_BIN_EXE_tasks")), &call);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert!(ran.out.contains("\"in_progress\""), "{}", ran.out);
    let lines = listener.until("reaches no live executor", Duration::from_secs(60));
    assert!(
        lines.iter().any(|line| line.starts_with("VERDICT #")
            && line.contains(&format!(
                " tools/unread-verdict from {reviewer} — changes: the hit misses"
            ))),
        "{lines:?}"
    );
    assert_eq!(
        (lines.iter())
            .filter(|line| line.contains("reaches no live executor"))
            .count(),
        1,
        "{lines:?}"
    );
}

/// The project's cap holds in a detached dispatcher as in the write that started it. A PULL the
/// write queued is built again as the dispatcher delivers it, so a dispatcher that knew no cap
/// told an orchestrator its slots had gone (`slots=none (was 1/8)`) and the next capped writer
/// that they were back: a change said each time the two took turns, with none on the board.
#[test]
fn a_detached_dispatcher_counts_slots_against_the_cap_its_writer_holds_to() {
    let (case, _root) =
        crate::slots::in_project("detached-cap", "[limits]\norchestrator_slots = 8\n");
    let listener = Listener::new("capped");
    let orch = case.register("orchestrator");
    let worker = case.register("worker");
    let writer = case.register("worker");
    case.ok(&orch, &["create", "work/0", "Work"]);
    case.ok(&worker, &["claim", "work/0"]);
    case.store()
        .callback(
            &orch,
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    let first = listener.queued();
    assert!(
        first
            .iter()
            .any(|line| line.starts_with("PULL ") && line.ends_with(" slots=1/8")),
        "the first digest counts its slots against the cap: {first:?}"
    );
    // A program that starts and does nothing: the write queues its lines and leaves them there.
    let idle = Path::new("/usr/bin/true");
    let made = run_as(
        &case.env,
        idle,
        &["create", "work/1", "More", "--as", &writer],
    );
    assert_eq!(made.code, 0, "{}", made.err);
    assert!(listener.queued().is_empty(), "the write delivered nothing");
    let dispatched = run_as(&case.env, idle, &["dispatch"]);
    assert_eq!(dispatched.code, 0, "{}", dispatched.err);
    let lines = listener.queued();
    let pull = lines
        .iter()
        .find(|line| line.starts_with("PULL "))
        .unwrap_or_else(|| panic!("the new work's digest is delivered: {lines:?}"));
    // New work, and the slots it holds as they were: counted against the cap, never as none.
    assert!(pull.contains("ready_unstaffed=1 (was 0)"), "{lines:?}");
    assert!(!pull.contains("slots="), "{lines:?}");
    assert!(
        lines.iter().all(|line| !line.contains("cap=none")),
        "{lines:?}"
    );
}
