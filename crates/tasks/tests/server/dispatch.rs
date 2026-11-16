//! A write on the board in the live board's own shape (the repository's own, in its git dir,
//! reached from the repository with no `TASKS_*` variable naming it), run by the `tasks` executable
//! as `just tasks` runs it, every build of which is `cli::process_main`: it hands its delivery to a
//! detached `tasks dispatch` of that executable, which opens `symphony/tasks/dispatch.log` beside
//! the board for what it says, takes this host's dispatch lease and delivers to the recipient's
//! native callback. A post, a `tasks api` post shaped as `land-approve --verdict changes`' and a
//! changes verdict each do.
//!
//! The executable is whichever `tasks` the including test binary's package builds: this crate's in
//! the server tests, and the project's board-only one, the build the live board runs, in that
//! package's tests (`apps/tasks/tests/dispatch.rs`), which include this file as it is.

use crate::listeners::Listeners;
use crate::own_board::Repository;
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use symphony_board::cli::{Env, connect};
use symphony_board::server::FOLDER;
use symphony_board::sys::{self, Command, hostname};
use symphony_board::{NewSession, NewTask, Store};

const SLUG: &str = "tools/live-shape";

/// What a command in `repo` reads: its board, and no sweep or review command started by the work.
fn env_of(repo: &Repository) -> Env {
    let mut env = repo.env();
    for (name, value) in [("TASKS_SWEEP", "off"), ("TASKS_ON_REVIEW", "off")] {
        env.vars.insert(name.into(), value.into());
    }
    env
}

/// `tasks ARGS` run by the `tasks` executable from `repo`, `input` on its stdin, its environment
/// `env_of`'s and the `PATH` that finds `ps` and `git`: what it printed, once it has succeeded and
/// started its dispatcher (a write that could not says so and delivers itself).
fn tasks(repo: &Repository, args: &[&str], input: &str) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tasks"));
    command
        .args(args)
        .current_dir(&repo.dir)
        .env_clear()
        .envs(env_of(repo).vars)
        .env("PATH", std::env::var_os("PATH").unwrap_or_default());
    let mut child = sys::spawn_piped(&mut command).unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let done = sys::wait_output(child).unwrap();
    let (out, err) = (
        String::from_utf8(done.stdout).unwrap(),
        String::from_utf8(done.stderr).unwrap(),
    );
    assert!(done.status.success(), "tasks {args:?}: {out}{err}");
    assert!(!err.contains("CALLBACK"), "tasks {args:?}: {err}");
    out
}

/// This host's dispatch lease as it stands: who took it last (`revision`) and until when.
fn lease(store: &Store) -> Value {
    let lease = (store.raw()).row("callback_lease", json!(["dispatch", hostname()]));
    let lease = lease.fields(&["revision", "until", "asked"]).unwrap();
    // No lease yet reads as one that says nothing.
    if lease.is_null() { json!({}) } else { lease }
}

/// Waits until `recipient` has heard `text` and the dispatcher of write number `nth` has let this
/// host's lease go: the nth detached dispatcher to take it, none having found it held.
fn delivered(store: &Store, listeners: &Listeners, recipient: &str, text: &str, nth: u64) {
    let heard = || {
        listeners
            .heard(recipient)
            .iter()
            .any(|line| line.contains(text))
    };
    let done = test_time::until(
        format!("write {nth}: {text:?} delivered by its dispatcher, its lease let go"),
        test_time::backstop(Duration::from_secs(60)),
        || {
            let lease = lease(store);
            let released = lease["revision"] == json!(nth) && lease["until"] == json!(0);
            (released && heard()).then_some(lease)
        },
    );
    match done {
        Ok(lease) => assert_eq!(lease["asked"], json!(0), "{lease}"),
        Err(stall) => panic!(
            "{stall}; lease {}, heard {:?}",
            lease(store),
            listeners.heard(recipient)
        ),
    }
}

/// A folder for the recipient's socket, short enough for a socket's path, gone with the test.
struct Sockets(PathBuf);

impl Sockets {
    fn new() -> Sockets {
        let dir = std::env::temp_dir().join(format!("symphony-dispatch-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Sockets(dir)
    }
}

impl Drop for Sockets {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn each_write_of_the_tasks_executable_hands_its_delivery_to_a_detached_dispatcher() {
    let repo = Repository::new("dispatch");
    let store = connect(&env_of(&repo)).unwrap();
    let session = |kind| store.register(&NewSession::new(kind)).unwrap().row.handle;
    let (recipient, writer) = (session("worker"), session("reviewer"));
    // Filed by a third session: the writer reviews the task below, which its filer may not.
    let filer = session("orchestrator");
    store
        .create(&NewTask::new(SLUG, "Live shape", &filer))
        .unwrap();
    store.claim(SLUG, &recipient).unwrap();
    let sockets = Sockets::new();
    let listeners = Listeners::new(&sockets.0, std::slice::from_ref(&recipient));
    let address = format!("uds:{}", listeners.paths[&recipient].display());
    store
        .callback(&recipient, Some(&address), false, false)
        .unwrap();
    let log = repo.common().join(FOLDER).join("dispatch.log");
    assert!(!log.exists(), "no write has handed a dispatch off yet");

    let said = tasks(
        &repo,
        &[
            "say",
            SLUG,
            "a post to its executor",
            "--to",
            &recipient,
            "--as",
            &writer,
        ],
        "",
    );
    assert!(said.starts_with("sent #"), "{said}");
    delivered(&store, &listeners, &recipient, "a post to its executor", 1);
    assert!(log.exists(), "the dispatcher's log is beside the board");

    // `land-approve --verdict changes` posts its note to the executor through `tasks api`.
    let note = format!("{writer} asks for changes on {SLUG} at tip 0123abc: the landing's note");
    let call = json!({"method": "say", "args": {"slug": SLUG, "sender": writer, "text": note, "to": "@executor"}});
    let answered: Value = serde_json::from_str(&tasks(&repo, &["api"], &call.to_string())).unwrap();
    assert!(answered["ok"]["id"].is_u64(), "{answered}");
    delivered(&store, &listeners, &recipient, "the landing's note", 2);

    store.move_to(SLUG, "review", &recipient).unwrap();
    store.take_role(SLUG, &writer, "reviewer").unwrap();
    let ruled = tasks(
        &repo,
        &[
            "verdict",
            SLUG,
            "changes",
            "the reviewer's changes",
            "--as",
            &writer,
        ],
        "",
    );
    assert_eq!(ruled.trim(), format!("{SLUG} is in_progress"));
    delivered(&store, &listeners, &recipient, "the reviewer's changes", 3);

    assert_eq!(
        fs::read_to_string(&log).unwrap(),
        "",
        "every dispatcher delivered, with nothing to say"
    );
}
