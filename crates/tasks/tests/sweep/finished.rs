//! The sweep's rules for finished work (tools/worktree-sweep-on-done): a task still live keeps a
//! worktree its session or its launch is tied to; a finished worktree's output goes with it; and a
//! task just done or cancelled (`--task`) has its closed sessions' worktrees go without the window.

use crate::common::{GONE, Seed};
use crate::{Repo, git, text, write};
use std::path::Path;
use tasks::sweep::Finished;
use tasks::{NewLaunch, NewTask};

/// `handle` launched on `slug` in `wt` as `role`, its pane closed if `closed`.
fn launched(r: &Repo, slug: &str, handle: &str, role: &str, wt: &Path, closed: bool) {
    let store = &r.board.store;
    store
        .record_launch(&NewLaunch {
            worktree: wt.to_string_lossy().into_owned(),
            pane: "w1:p1".into(),
            ..NewLaunch::new(slug, handle, role)
        })
        .unwrap();
    if closed {
        store
            .close_task_session(slug, handle, "idle", "just tasks restore")
            .unwrap();
    }
}

/// A session bound to no worktree.
fn unbound(r: &Repo) -> String {
    r.board.session("worker")
}

// -- a task still live keeps the worktrees tied to it ------------------------------------------

#[test]
fn a_lost_session_bound_here_holding_live_work_keeps_it() {
    // Its binding no longer counts (lost), but its task can still be restored there.
    let r = Repo::new();
    let wt = r.detached("lost");
    let handle = r.bind(&wt);
    r.task("sim/jump");
    r.board.store.claim("sim/jump", &handle).unwrap();
    r.board.advance(GONE);
    assert_eq!(r.board.health(&handle).as_str(), "lost");
    let said = r.sweep();
    r.assert_kept(&wt, &said);
    assert!(
        said.contains(&format!(
            "task sim/jump is in_progress, held by {handle}, whose session is bound here"
        )),
        "{said}"
    );
}

#[test]
fn a_session_that_left_holds_nothing_so_its_worktree_goes() {
    let r = Repo::new();
    let wt = r.detached("left-work");
    let handle = r.bind(&wt);
    r.task("sim/jump");
    r.board.store.claim("sim/jump", &handle).unwrap();
    r.board.store.leave(&handle).unwrap(); // its roles are released: the task is ready for another
    r.assert_removed(&wt, &r.sweep());
}

#[test]
fn a_live_task_launched_here_keeps_it_whoever_holds_it() {
    let r = Repo::with(Seed::default().task("sim/jump", "review"));
    let wt = r.detached("reviewer");
    let handle = unbound(&r);
    launched(&r, "sim/jump", &handle, "reviewer", &wt, true);
    let said = r.sweep();
    r.assert_kept(&wt, &said);
    assert!(
        said.contains(&format!(
            "task sim/jump is review, launched here for {handle}"
        )),
        "{said}"
    );
}

#[test]
fn once_the_task_launched_here_is_done_the_worktree_goes() {
    let r = Repo::with(Seed::default().task("sim/jump", "done"));
    let wt = r.detached("reviewer");
    launched(&r, "sim/jump", &unbound(&r), "reviewer", &wt, true);
    r.assert_removed(&wt, &r.sweep());
}

// -- a finished worktree's output goes with it ------------------------------------------------

const OUTPUT: [&str; 5] = [
    "artifacts/render-check/frame.png",
    "artifacts/solar-swift/probe.json",
    ".godot/editor/cache",
    ".venv/lib/x.py",
    ".target/debug/tasks",
];

#[test]
fn a_finished_worktrees_output_goes_with_it() {
    for status in ["done", "cancelled"] {
        let r = Repo::with(Seed::default().task("sim/landed", status));
        let wt = r.merged("landed", "sim/landed");
        for path in OUTPUT {
            write(&wt, path, "x\n");
        }
        r.assert_removed(&wt, &r.sweep());
    }
}

#[test]
fn a_worktree_with_no_task_lets_its_output_go() {
    let r = Repo::new();
    let wt = r.detached("reviewer");
    for path in OUTPUT {
        write(&wt, path, "x\n");
    }
    r.assert_removed(&wt, &r.sweep());
}

#[test]
fn a_worktree_whose_task_is_not_finished_keeps_its_output_and_names_it() {
    let r = Repo::with(Seed::default().task("sim/parked", "backlog"));
    // Unlike explicit landing cleanup, a sweep does not infer that a parked task's test output
    // can go. Only done/cancelled (or no task) releases all artifacts.
    write(
        &r.root,
        "tests/probe_test.gd",
        "const OUT = \"res://artifacts/solar-swift\"\n",
    );
    git(&r.root, &["add", "tests/probe_test.gd"]);
    git(&r.root, &["commit", "-qm", "test output contract"]);
    let wt = r.merged("parked", "sim/parked");
    write(&wt, "artifacts/solar-swift/probe.json", "x\n");
    write(&wt, "artifacts/slynx-verify.log", "x\n"); // a log goes whatever the task
    let said = r.sweep();
    r.assert_kept(&wt, &said);
    let kept = said
        .lines()
        .find(|l| l.starts_with(&format!("kept {}: merged, but", wt.display())))
        .unwrap_or_else(|| panic!("{said}"));
    assert!(kept.contains("artifacts/solar-swift/"), "{said}");
    assert!(!kept.contains("slynx-verify.log"), "{said}");
}

#[test]
fn a_finished_worktree_still_keeps_a_change_or_an_untracked_file() {
    for (path, tracked) in [("notes.txt", false), ("sim/world.gd", true)] {
        let r = Repo::with(Seed::default().task("sim/landed", "done"));
        let wt = r.merged("landed", "sim/landed");
        write(&wt, path, "not committed\n");
        write(&wt, "artifacts/solar-swift/probe.json", "x\n");
        let said = r.sweep();
        r.assert_kept(&wt, &said);
        assert!(said.contains(path), "{tracked}: {said}");
    }
}

// -- a task just done or cancelled: its closed sessions' worktrees go at once (`--task`) -------

/// sim/jump done, `handle` live and bound to its merged worktree, launched there and closed (or not).
fn finished_jump(closed: bool) -> (Repo, std::path::PathBuf, String) {
    let r = Repo::with(Seed::default().task("sim/jump", "done"));
    let wt = r.merged("sim-jump", "sim/jump");
    let handle = r.bind(&wt);
    launched(&r, "sim/jump", &handle, "executor", &wt, closed);
    (r, wt, handle)
}

#[test]
fn the_closed_sessions_worktree_of_a_finished_task_goes_without_the_window() {
    let (r, wt, handle) = finished_jump(true);
    let finished = Finished::of(&r.board.store, "sim/jump").unwrap();
    assert!(finished.handles.contains(&handle));
    // Touched just now and bound to a live session: only the finish lets it go.
    r.assert_kept(&wt, &text(&r.sweep_now(|_| {})));
    let result = r.sweep_now(|o| o.finished = finished);
    r.assert_removed(&wt, &text(&result));
}

#[test]
fn an_open_session_keeps_its_worktree_when_its_task_finishes() {
    // A busy pane is not closed, so its live session still counts, past the window too.
    let (r, wt, _) = finished_jump(false);
    let finished = Finished::of(&r.board.store, "sim/jump").unwrap();
    assert_eq!(finished, Finished::default());
    r.assert_kept(&wt, &text(&r.sweep_with(|o| o.finished = finished)));
}

#[test]
fn an_unmerged_or_dirty_worktree_of_a_finished_task_stays() {
    for dirty in [false, true] {
        let r = Repo::with(Seed::default().task("sim/jump", "done"));
        let wt = r.add("sim-jump", Some("sim/jump"), "main");
        r.commit(&wt, "extends RefCounted\n# work\n");
        if dirty {
            git(&r.root, &["merge", "-q", "--ff-only", "sim/jump"]);
            write(&wt, "left.txt", "not committed");
        }
        launched(&r, "sim/jump", &r.bind(&wt), "executor", &wt, true);
        let finished = Finished::of(&r.board.store, "sim/jump").unwrap();
        r.assert_kept(&wt, &text(&r.sweep_now(|o| o.finished = finished)));
    }
}

#[test]
fn a_worktree_of_another_task_keeps_its_window() {
    let (r, _, _) = finished_jump(true);
    r.board
        .store
        .create(&NewTask::new("sim/dock", "Dock", "owner"))
        .unwrap();
    let dock = r.merged("sim-dock", "sim/dock");
    launched(&r, "sim/dock", &unbound(&r), "executor", &dock, true);
    let finished = Finished::of(&r.board.store, "sim/jump").unwrap();
    r.assert_kept(&dock, &text(&r.sweep_now(|o| o.finished = finished)));
}

#[test]
fn a_task_not_finished_hurries_no_worktree() {
    let r = Repo::with(Seed::default().task("sim/jump", "in_progress"));
    let wt = r.merged("sim-jump", "sim/jump");
    launched(&r, "sim/jump", &r.bind(&wt), "executor", &wt, true);
    assert_eq!(
        Finished::of(&r.board.store, "sim/jump").unwrap(),
        Finished::default()
    );
}

#[test]
fn a_closed_session_of_a_task_under_it_counts_for_the_one_that_finishes() {
    let r = Repo::with(Seed::default().task("sim/jump", "done"));
    r.board
        .store
        .create(&NewTask {
            parent: Some("sim/jump".into()),
            instruction: true,
            ..NewTask::new("sim/jump/do", "Do", "owner")
        })
        .unwrap();
    let wt = r.detached("do");
    let handle = r.bind(&wt);
    launched(&r, "sim/jump/do", &handle, "executor", &wt, true);
    let finished = Finished::of(&r.board.store, "sim/jump").unwrap();
    assert!(finished.worktrees.contains(&wt.canonicalize().unwrap()));
    assert!(finished.handles.contains(&handle));
}
