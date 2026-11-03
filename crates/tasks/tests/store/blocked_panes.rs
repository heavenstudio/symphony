//! The server clock's pane watch (`dispatch_callbacks_snapshot`), herdr the crate's `fake-herdr`
//! found on PATH as `herdr`, as the shared server finds herdr: each case runs again in a child
//! process of its own, in a repository of its own, its panes a file the case rewrites between ticks.

use super::common::Fixture;
use super::quiet_callbacks::listen;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tasks::NewLaunch;
use tasks::sys::Command;

const IN_CHILD: &str = "SF_BLOCKED_PANES_CHILD";
const BUSY_PULL: &str = "PULL executing=1 ready_unstaffed=0 in_progress_orphaned=0 review_unreviewed=0 landed_open=0 idle=0";

/// A folder removed when the case ends.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// In the parent, run the case `name` again in a child process whose PATH finds `fake-herdr` as
/// `herdr`, in a repository of its own so no slot or branch of this one is probed; false once it
/// passed. In that child, true: the case goes on.
fn in_child_with_herdr(name: &str) -> bool {
    if std::env::var_os(IN_CHILD).is_some() {
        return true;
    }
    let dir = std::env::temp_dir().join(format!(
        "sf-blocked-panes-{}-{}",
        std::process::id(),
        name.replace(':', "-")
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let scratch = Scratch(dir.clone());
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_fake-herdr"), bin.join("herdr")).unwrap();
    let state = dir.join("herdr.json");
    std::fs::write(&state, json!({"panes": []}).to_string()).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(&dir)
            .status()
            .unwrap()
            .success()
    );
    let output = tasks::sys::output(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .current_dir(&dir)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("HERDR_BIN")
            .env_remove("HERDR_WORKSPACE_ID")
            .env_remove("FAKE_HERDR_FAIL")
            .env(IN_CHILD, "1")
            .env("FAKE_HERDR_STATE", &state)
            .env(
                "PATH",
                format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
            ),
    )
    .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    drop(scratch);
    false
}

/// What herdr answers from now on: these panes, or, given None, a state it cannot read, which it
/// refuses as herdr does when it cannot be asked.
fn panes(panes: Option<Value>) {
    let state = std::env::var("FAKE_HERDR_STATE").unwrap();
    let text = match panes {
        Some(panes) => json!({"panes": panes}).to_string(),
        None => "not json".to_string(),
    };
    std::fs::write(Path::new(&state), text).unwrap();
}

fn pane(id: &str, status: &str) -> Value {
    json!({"pane_id": id, "agent": "codex", "agent_status": status, "cwd": "/wt"})
}

/// A tick of the server clock.
fn tick(f: &Fixture) {
    f.store.dispatch_callbacks_snapshot().unwrap();
}

/// Launched into `pane`, on `slug`.
fn launch(f: &Fixture, slug: &str, handle: &str, pane: &str) {
    f.store
        .record_launch(&NewLaunch {
            pane: pane.into(),
            ..NewLaunch::new(slug, handle, "executor")
        })
        .unwrap();
}

#[test]
fn a_blocked_pane_reaches_its_orchestrator_and_parent_once_an_episode() {
    if !in_child_with_herdr(
        "blocked_panes::a_blocked_pane_reaches_its_orchestrator_and_parent_once_an_episode",
    ) {
        return;
    }
    let f = Fixture::empty();
    let orch = f.session("orchestrator");
    let parent = f.session("orchestrator");
    let other = f.session("orchestrator");
    let mut child = tasks::NewSession::new("worker");
    child.parent = Some(parent.clone());
    let worker = f.session_with(child);
    f.task("tests/pane");
    f.store
        .take_role("tests/pane", &orch, "orchestrator")
        .unwrap();
    f.store.claim("tests/pane", &worker).unwrap();
    // The orchestrator's own pane and a pane no session holds wait too, and are never said.
    launch(&f, "tests/pane", &orch, "w1:p1");
    launch(&f, "tests/pane", &worker, "w1:p2");
    panes(Some(json!([
        pane("w1:p1", "blocked"),
        pane("w1:p2", "working"),
        pane("w1:p9", "blocked")
    ])));
    let (o, p, x) = (listen(&f, &orch), listen(&f, &parent), listen(&f, &other));
    let w = listen(&f, &worker);
    for listener in [&o, &p, &x] {
        assert_eq!(listener.text(), BUSY_PULL);
    }
    tick(&f);
    let quiet: [&[String]; 4] = [&[], &[], &[], &[]];
    // Each stream read to a barrier another session sends.
    let heard = || {
        [
            o.drain(&f, &worker, &orch),
            p.drain(&f, &worker, &parent),
            x.drain(&f, &worker, &other),
            w.drain(&f, &other, &worker),
        ]
    };
    assert_eq!(heard(), quiet, "nothing they oversee waits");

    panes(Some(json!([
        pane("w1:p1", "blocked"),
        pane("w1:p2", "blocked")
    ])));
    let blocked =
        format!("BLOCKED {worker} tests/pane w1:p2 — its pane waits on an approval or a question");
    tick(&f);
    tick(&f);
    let blocked = std::slice::from_ref(&blocked);
    let once: [&[String]; 4] = [blocked, blocked, &[], &[]];
    assert_eq!(heard(), once, "its orchestrator and its parent, once");

    panes(Some(json!([pane("w1:p2", "working")])));
    tick(&f);
    assert_eq!(heard(), quiet, "an answered prompt says nothing");
    panes(Some(json!([pane("w1:p2", "blocked")])));
    tick(&f);
    tick(&f);
    assert_eq!(heard(), once, "blocking again is a new episode");
}

#[test]
fn herdr_failing_is_said_once_and_a_known_block_is_not_repeated() {
    if !in_child_with_herdr(
        "blocked_panes::herdr_failing_is_said_once_and_a_known_block_is_not_repeated",
    ) {
        return;
    }
    let f = Fixture::empty();
    let orch = f.session("orchestrator");
    let worker = f.session("worker");
    f.task("tests/pane");
    f.store
        .take_role("tests/pane", &orch, "orchestrator")
        .unwrap();
    f.store.claim("tests/pane", &worker).unwrap();
    launch(&f, "tests/pane", &worker, "w1:p2");
    panes(Some(json!([pane("w1:p2", "blocked")])));
    let o = listen(&f, &orch);
    assert_eq!(o.text(), BUSY_PULL);
    tick(&f);
    assert_eq!(
        o.drain(&f, &worker, &orch),
        [format!(
            "BLOCKED {worker} tests/pane w1:p2 — its pane waits on an approval or a question"
        )]
    );

    panes(None);
    for _ in 0..3 {
        tick(&f);
    }
    let said = o.drain(&f, &worker, &orch);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with("WATCH herdr unanswered: herdr pane list failed: ")
            && said[0].ends_with(" — blocked panes go unreported until it answers"),
        "{said:?}"
    );

    // herdr answers again with the same prompt still waiting: nothing new to say.
    panes(Some(json!([pane("w1:p2", "blocked")])));
    tick(&f);
    assert_eq!(o.drain(&f, &worker, &orch), Vec::<String>::new());
    // A second outage is a second WATCH.
    panes(None);
    tick(&f);
    tick(&f);
    assert_eq!(o.drain(&f, &worker, &orch).len(), 1);
}

#[test]
fn with_no_session_launched_into_a_pane_there_is_nothing_to_watch() {
    if !in_child_with_herdr(
        "blocked_panes::with_no_session_launched_into_a_pane_there_is_nothing_to_watch",
    ) {
        return;
    }
    let f = Fixture::empty();
    let orch = f.session("orchestrator");
    let worker = f.session("worker");
    f.task("tests/pane");
    f.store
        .take_role("tests/pane", &orch, "orchestrator")
        .unwrap();
    f.store.claim("tests/pane", &worker).unwrap();
    let o = listen(&f, &orch);
    assert_eq!(o.text(), BUSY_PULL);
    // herdr cannot answer, but no session it oversees was launched into a pane: no WATCH.
    panes(None);
    tick(&f);
    tick(&f);
    assert_eq!(o.drain(&f, &worker, &orch), Vec::<String>::new());
}
