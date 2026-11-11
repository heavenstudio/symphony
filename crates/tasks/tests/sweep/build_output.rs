//! The build output of finished work (tools/one-build-folder-for-worktrees): a worktree the
//! worktree sweep keeps loses its build folders once every task it is tied to is done, its HEAD is
//! merged work and no agent there beat within the hour; never a branch with no commits ahead of
//! main, a task in progress, an agent that beat within the hour, or a process working inside it.

use crate::common::{NOW, Seed};
use crate::{Repo, git, started, text, write};
use std::path::{Path, PathBuf};
use symphony_board::sweep::Sweep;

/// Two hours on the board's clock: past the hour the build output waits for.
const LATER: f64 = NOW + 7200.0;

/// `wt` built, its build folders holding a compiled file each, as Cargo leaves them.
fn built(wt: &Path) -> [PathBuf; 2] {
    write(wt, ".target/debug/deps/libwork.rlib", "compiled\n");
    write(wt, "symphony/.target/debug/deps/libwork.rlib", "compiled\n");
    [wt.join(".target"), wt.join("symphony/.target")]
}

/// A worktree on a new `branch` from main with a commit on it, merged into main by a merge commit,
/// as work lands here.
fn landed(r: &Repo, name: &str, branch: &str) -> PathBuf {
    let wt = r.add(name, Some(branch), "main");
    r.commit(&wt, &format!("extends RefCounted\n# {name}\n"));
    let message = format!("Merge branch '{branch}'");
    git(&r.root, &["merge", "-q", "--no-ff", "-m", &message, branch]);
    wt
}

/// Kept whole by the worktree sweep, as a Claude Code agent's worktree is while its orchestrator
/// runs: locked by a live process, this one.
fn held(r: &Repo, wt: &Path) {
    let pid = i64::from(std::process::id());
    r.claude_lock(wt, &pid.to_string(), &started(pid));
}

/// The sweep at `now` on the board's clock, every worktree idle and nobody inside.
fn sweep_at(r: &Repo, now: f64, dry_run: bool) -> Sweep {
    r.sweep_with(|o| {
        o.now = Box::new(move || now);
        o.dry_run = dry_run;
    })
}

/// That `wt` stayed, and its build folders with it.
fn assert_untouched(r: &Repo, wt: &Path, folders: &[PathBuf], done: &Sweep) {
    let said = text(done);
    r.assert_kept(wt, &said);
    assert!(done.cleared.is_empty(), "{said}");
    for folder in folders {
        assert!(folder.join("debug/deps/libwork.rlib").is_file(), "{said}");
    }
}

#[test]
fn finished_work_loses_its_build_output_and_keeps_its_worktree() {
    let r = Repo::with(Seed::default().task("sim/landed", "done"));
    let wt = landed(&r, "landed", "sim/landed");
    held(&r, &wt);
    let folders = built(&wt);
    let head = git(&wt, &["rev-parse", "HEAD"]);
    let why = format!(
        "task sim/landed is done; on sim/landed at {}, merged into main; no agent there for an hour",
        &head[..12]
    );

    let dry = sweep_at(&r, LATER, true);
    assert_eq!(
        dry.cleared,
        folders
            .iter()
            .map(|f| (f.clone(), why.clone()))
            .collect::<Vec<_>>()
    );
    let said = text(&dry);
    assert!(
        said.contains(&format!(
            "would clear build output {} ({why})",
            folders[0].display()
        )) && said.ends_with("; would clear 2 build folders of finished work"),
        "{said}"
    );
    assert!(
        folders.iter().all(|f| f.is_dir()),
        "a dry run removes nothing"
    );

    let done = sweep_at(&r, LATER, false);
    let said = text(&done);
    r.assert_kept(&wt, &said);
    assert!(
        said.contains(&format!(
            "cleared build output {} ({why})",
            folders[1].display()
        )) && said.ends_with("; cleared 2 build folders of finished work"),
        "{said}"
    );
    assert!(folders.iter().all(|f| !f.exists()), "{said}");
    assert!(
        wt.join("sim/world.gd").is_file(),
        "the worktree's files stay"
    );
    assert!(
        sweep_at(&r, LATER, false).cleared.is_empty(),
        "nothing is left to clear"
    );
}

#[test]
fn a_branch_with_no_commits_ahead_of_main_is_not_merged_work() {
    // Its task done or not, a branch that never committed is where an agent starts: its HEAD is a
    // commit of main's own line, an ancestor of main like every merged branch's.
    let r = Repo::with(
        Seed::default()
            .task("sim/fresh", "done")
            .task("sim/fast", "done")
            .task("sim/again", "done"),
    );
    let fresh = r.add("fresh", Some("sim/fresh"), "main");

    // One that caught up with main by a fast-forward, after other work landed.
    let fast = r.add("fast", Some("sim/fast"), "main");
    landed(&r, "other", "sim/other");
    git(&fast, &["merge", "-q", "--ff-only", "main"]);

    // One whose earlier work landed, reset to main for its next task, with nothing committed yet.
    let again = landed(&r, "again", "sim/again");
    git(&again, &["reset", "-q", "--hard", "main"]);

    for wt in [&fresh, &fast, &again] {
        held(&r, wt);
    }
    let folders: Vec<[PathBuf; 2]> = [&fresh, &fast, &again].map(|wt| built(wt)).into();
    let done = sweep_at(&r, LATER, false);
    for (wt, folders) in [&fresh, &fast, &again].iter().zip(&folders) {
        assert_untouched(&r, wt, folders, &done);
    }
}

#[test]
fn a_task_in_progress_keeps_the_build_output_of_merged_work() {
    for status in ["in_progress", "review", "blocked", "ready"] {
        let r = Repo::with(Seed::default().task("sim/landed", status));
        let wt = landed(&r, "landed", "sim/landed");
        held(&r, &wt);
        let folders = built(&wt);
        assert_untouched(&r, &wt, &folders, &sweep_at(&r, LATER, false));
    }
}

#[test]
fn one_task_in_progress_of_two_keeps_it() {
    // Its branch's task is done, but the agent bound there holds a task in progress.
    let r = Repo::with(
        Seed::default()
            .task("sim/landed", "done")
            .task("sim/next", "ready"),
    );
    let wt = landed(&r, "landed", "sim/landed");
    held(&r, &wt);
    let folders = built(&wt);
    let handle = r.bind(&wt);
    r.board.store.claim("sim/next", &handle).unwrap();
    assert_untouched(&r, &wt, &folders, &sweep_at(&r, LATER, false));
}

#[test]
fn an_agent_that_beat_within_the_hour_keeps_it() {
    let r = Repo::with(Seed::default().task("sim/landed", "done"));
    let wt = landed(&r, "landed", "sim/landed");
    held(&r, &wt);
    let folders = built(&wt);
    r.bind(&wt); // its session beat at NOW, whatever its health since

    let within = sweep_at(&r, NOW + 3599.0, false);
    assert_untouched(&r, &wt, &folders, &within);

    let past = sweep_at(&r, NOW + 3601.0, false);
    assert_eq!(past.cleared.len(), 2, "{}", text(&past));
    assert!(folders.iter().all(|f| !f.exists()));
}

#[test]
fn work_landed_by_a_fast_forward_counts_when_it_was_committed_here() {
    let r = Repo::with(Seed::default().task("sim/landed", "done"));
    let wt = r.merged("landed", "sim/landed");
    held(&r, &wt);
    let folders = built(&wt);
    let done = sweep_at(&r, LATER, false);
    assert_eq!(done.cleared.len(), 2, "{}", text(&done));
    assert!(folders.iter().all(|f| !f.exists()));
}

#[test]
fn a_worktree_tied_to_no_task_keeps_its_build_output() {
    let r = Repo::new();
    let wt = landed(&r, "landed", "sim/landed");
    held(&r, &wt);
    let folders = built(&wt);
    assert_untouched(&r, &wt, &folders, &sweep_at(&r, LATER, false));
}

#[test]
fn a_process_working_inside_keeps_it_and_so_does_not_knowing() {
    let r = Repo::with(Seed::default().task("sim/landed", "done"));
    let wt = landed(&r, "landed", "sim/landed");
    held(&r, &wt);
    let folders = built(&wt);
    let inside = wt.join("crates");
    for cwds in [Some(vec![inside]), None] {
        let scan = cwds.clone();
        let done = r.sweep_with(|o| {
            o.now = Box::new(|| LATER);
            o.cwds = Box::new(move || scan.clone());
        });
        assert_untouched(&r, &wt, &folders, &done);
    }
}
