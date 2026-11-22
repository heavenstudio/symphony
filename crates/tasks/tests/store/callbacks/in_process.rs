//! The clock and the dispatcher on an in-process executor gone quiet while what it started works
//! in its worktree.
use crate::common::Backend;

use super::*;
use crate::children::Child as _;
use symphony_board::sweep::trigger::now as wall_clock;

/// A process working in a folder, as a subagent's background `cargo test` works in its worktree,
/// until it is dropped however the test ends: a `cat` on a pipe only this test holds, so it ends
/// with the test process too, with no deadline of its own.
struct Working(symphony_board::sys::Child);

impl Working {
    fn in_folder(folder: &std::path::Path) -> Self {
        use symphony_board::sys::{Command, Stdio};
        let mut cat = Command::of("cat");
        cat.current_dir(folder)
            .stdin(Stdio::piped())
            .stdout(Stdio::null());
        Self(cat.spawn().unwrap())
    }
}

impl Drop for Working {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

on_both! {
/// 2026-11-08: the shared server told an orchestrator LOST of an in-process executor that was waiting
/// on its own background test run, the only sign of it a process working in its worktree. While one
/// it started there works the clock calls it neither stale nor lost nor its task orphaned; once it
/// ends, the executor is lost as before.
fn an_in_process_child_waiting_on_a_process_in_its_worktree_is_told_lost_only_once_it_ends(backend: Backend) {
    use symphony_board::cli::agents::PROCESS_SCAN_SECONDS;
    use symphony_board::{Beat, Health, NewSession};

    let f = Fixture::on(backend, crate::common::Seed::default());
    // The process's start is the machine's, so the board's clock is too, before any session beats.
    f.advance(wall_clock() - f.now());
    let orchestrator = f.session("orchestrator");
    let child = f.session_with(NewSession {
        parent: Some(orchestrator.clone()),
        worktree: "/main".into(),
        ..NewSession::new("worker")
    });
    f.task("tools/waiting");
    f.store.claim("tools/waiting", &child).unwrap();
    let tree = std::env::temp_dir().join(format!(
        "symphony-in-process-{}-{}",
        std::process::id(),
        f.store.database().database()
    ));
    std::fs::create_dir_all(&tree).unwrap();
    std::fs::write(tree.join(".git"), "gitdir: /nowhere/worktrees/wt\n").unwrap();
    let tree = tree.canonicalize().unwrap();
    let place = Some((tree.to_str().unwrap(), "tools/waiting"));
    assert!(f.store.bind_agent("agent-c", &child, false).unwrap());
    // The background launch: its hook's beat before, the process, its hook's beat after, each
    // reading the machine's clocks as the hook does.
    let call = || {
        f.advance(wall_clock() - f.now());
        let boot = symphony_board::sys::boot_clock();
        let beat = f
            .store
            .agent_beat_in("agent-c", &Beat::default(), place, boot.as_ref());
        assert_eq!(beat.unwrap().as_deref(), Some(child.as_str()));
    };
    call();
    let waited = Working::in_folder(&tree);
    call();
    let listener = Listener::new(f.store.database().database());
    f.store
        .callback(
            &orchestrator,
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    for wait in [91.0, 510.0, 30.0] {
        f.advance(wait);
        f.store.dispatch_callbacks_snapshot().unwrap();
    }
    assert_eq!(f.health(&child), Health::Lost);
    let sender = f.session("worker");
    let said = listener.drain(&f, &sender, &orchestrator);
    drop(waited);
    let about = |lines: &[String], head: &str| {
        lines
            .iter()
            .filter(|line| line.starts_with(head))
            .cloned()
            .collect::<Vec<_>>()
    };
    for head in [
        format!("STALE {child}"),
        format!("LOST {child}"),
        "in_progress_orphaned p".to_string(),
    ] {
        assert_eq!(about(&said, &head), Vec::<String>::new(), "{said:?}");
    }
    f.advance(PROCESS_SCAN_SECONDS);
    f.store.dispatch_callbacks_snapshot().unwrap();
    let said = listener.drain(&f, &sender, &orchestrator);
    std::fs::remove_dir_all(&tree).unwrap();
    assert_eq!(
        about(&said, &format!("LOST {child}")),
        [format!(
            "LOST {child} — its work is claimable: tools/waiting"
        )],
        "{said:?}"
    );
    assert_eq!(
        about(&said, "in_progress_orphaned p"),
        ["in_progress_orphaned p2 tools/waiting — Jump departure"],
        "{said:?}"
    );
}
}
