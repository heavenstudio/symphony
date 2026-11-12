//! Why the sweep removed nothing (tools/sweep-removes-nothing), each cause reproduced on a scratch
//! repository as the board's worktrees were, and the rule that answers it:
//!
//! * the land's own pass (`--task SLUG`) visited only the worktrees of the task's closed launch
//!   records, and an in-process subagent has none: it now visits those its sessions worked in;
//! * Claude Code's lock names the orchestrator's live process, which outlives its subagent: a lock
//!   of a live process lets the worktree go once the board shows its agent finished;
//! * this repository ignores `/artifacts/`, so git lists the whole folder as one entry, and anything
//!   in it kept a finished worktree: it is now set aside in the main checkout first;
//! * build output went only once the task was done and its HEAD merged: it now goes once every task
//!   tied to the worktree is in review or later and nothing ran there for an hour;
//! * a land's lock kept its worktree after the land's process was gone.

use crate::common::{GONE, NOW, Seed, session};
use crate::{Repo, dead_pid, git, started, text, write};
use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use symphony_board::rows::SessionRow;
use symphony_board::sweep::holder::NIGHTLY_LOCK;
use symphony_board::sweep::{self, Request, SWEPT, size};

const AGENT: &str = "a5a6262f496f70d23";
const SLUG: &str = "tools/landed";

/// A board where `SLUG` is `status`, its executor an in-process subagent (its session tied to its
/// own `agent-ID` worktree by its agent id, no launch record), gone since.
fn subagent_board(status: &str) -> Repo {
    let r = Repo::with(
        Seed::default()
            .session_row(SessionRow {
                agent: Some(AGENT.into()),
                ..session("amber-heron")
            })
            .task(SLUG, status)
            .role(SLUG, "amber-heron", "executor"),
    );
    r.board.advance(GONE);
    r
}

/// The subagent's worktree, on the task's branch with a commit, merged into main by a merge
/// commit, as `just land` merges.
fn landed(r: &Repo) -> PathBuf {
    let wt = r.add(&format!("agent-{AGENT}"), Some(SLUG), "main");
    r.commit(&wt, "extends RefCounted\n# landed\n");
    let message = format!("Merge branch '{SLUG}'");
    git(&r.root, &["merge", "-q", "--no-ff", "-m", &message, SLUG]);
    wt
}

/// This repository's ignore file, committed on main before any worktree is made.
fn ignore_as_here(r: &Repo) {
    write(&r.root, ".gitignore", "/.target/\n/build/\n/artifacts/\n");
    git(
        &r.root,
        &["commit", "-qam", "this repository's ignore file"],
    );
}

/// The line of `said` saying why `wt` was removed.
fn removed(said: &str, wt: &Path) -> String {
    starting(said, &format!("removed {} (", wt.display()))
}

/// The line of a dry run's `said` for `wt`: its size, tasks, merge and verdict.
fn row(said: &str, wt: &Path) -> String {
    starting(said, &format!("worktree {}: ", wt.display()))
}

/// The one line of `said` that starts with `start`.
fn starting(said: &str, start: &str) -> String {
    let found: Vec<&str> = said.lines().filter(|l| l.starts_with(start)).collect();
    assert_eq!(found.len(), 1, "{start}\n{said}");
    found[0].to_string()
}

/// A dry run's lines, every worktree idle and nobody inside.
fn dry(r: &Repo) -> String {
    text(&r.sweep_with(|o| o.dry_run = true))
}

/// The pass a land runs for `SLUG`, from the main checkout.
fn lands_pass(r: &Repo) -> String {
    let request = Request {
        tasks: vec![SLUG],
        ..Request::default()
    };
    sweep::command(&r.board.store, &r.env(&r.root), &request)
        .unwrap()
        .join("\n")
}

#[test]
fn the_lands_pass_removes_its_subagents_worktree_and_names_the_bytes() {
    let r = subagent_board("done");
    let wt = landed(&r);
    write(&wt, ".target/debug/deps/libwork.rlib", "compiled\n");
    let bytes = size::bytes(&wt);
    // Just landed: no window for its own worktree.
    let said = lands_pass(&r);
    r.assert_removed(&wt, &said);
    let shown = size::shown(bytes);
    assert!(
        removed(&said, &wt).ends_with(&format!(", merged into main, clean, no lock; {shown})")),
        "{said}"
    );
    assert!(
        said.contains("removed 1 of 2 worktrees") && said.contains(&format!("; freed {shown}")),
        "{said}"
    );
    // Its branch, merged and held by no worktree now, goes with it.
    assert_eq!(r.branch(SLUG), "", "{said}");
    assert!(
        said.contains(&format!(
            "deleted branch {SLUG} (merged into main, its worktree removed)"
        )),
        "{said}"
    );
}

#[test]
fn the_lands_pass_leaves_a_worktree_its_subagent_still_works_in() {
    // The same subagent, its session live: the land's pass neither visits nor removes it.
    let r = Repo::with(
        Seed::default()
            .session_row(SessionRow {
                agent: Some(AGENT.into()),
                ..session("amber-heron")
            })
            .task(SLUG, "done")
            .role(SLUG, "amber-heron", "executor"),
    );
    let wt = landed(&r);
    let said = lands_pass(&r);
    r.assert_kept(&wt, &said);
    assert!(said.contains("removed 0 of 2 worktrees"), "{said}");
}

#[test]
fn a_finished_subagents_worktree_locked_by_its_live_orchestrator_goes() {
    let r = subagent_board("done");
    let wt = landed(&r);
    let orchestrator = i64::from(std::process::id());
    r.claude_lock(&wt, &orchestrator.to_string(), &started(orchestrator));
    let said = r.sweep();
    r.assert_removed(&wt, &said);
    assert!(
        removed(&said, &wt).contains(&format!(
            "Claude Code's lock of pid {orchestrator}, the session that started agent-{AGENT} and \
             outlives it: its session amber-heron is neither live nor stale, and task {SLUG} is done"
        )),
        "{said}"
    );
}

#[test]
fn a_live_orchestrators_lock_keeps_an_agent_the_board_does_not_show_finished() {
    let orchestrator = i64::from(std::process::id());
    // Its session live (bound, so kept before its lock is read), its task in review, or no
    // session the board knows of: the lock is all there is, and it holds.
    for case in ["live", "review", "unknown agent", "cancelled but live"] {
        let r = match case {
            "live" => Repo::with(
                Seed::default()
                    .session_row(SessionRow {
                        agent: Some(AGENT.into()),
                        ..session("amber-heron")
                    })
                    .task(SLUG, "done")
                    .role(SLUG, "amber-heron", "executor"),
            ),
            "review" => subagent_board("review"),
            "unknown agent" => Repo::with(Seed::default().task(SLUG, "done")),
            _ => Repo::with(
                Seed::default()
                    .session_row(SessionRow {
                        agent: Some(AGENT.into()),
                        ..session("amber-heron")
                    })
                    .task(SLUG, "cancelled")
                    .role(SLUG, "amber-heron", "executor"),
            ),
        };
        let wt = landed(&r);
        r.claude_lock(&wt, &orchestrator.to_string(), &started(orchestrator));
        let said = dry(&r);
        let line = row(&said, &wt);
        let expected = match case {
            "live" | "cancelled but live" => "kept: session amber-heron is bound there",
            "review" => "kept: task tools/landed is review",
            _ => "and the board does not show that agent finished",
        };
        assert!(line.contains(expected), "{case}: {said}");
        let said = r.sweep();
        r.assert_kept(&wt, &said);
    }
}

#[test]
fn a_live_claude_lock_naming_another_agent_keeps_it() {
    // A lock the board cannot tie to this worktree: its agent's own worktree is another.
    let r = subagent_board("done");
    let wt = landed(&r);
    let orchestrator = i64::from(std::process::id());
    r.lock(
        &wt,
        &format!(
            "claude agent agent-ac953cb051c3dd046 (pid {orchestrator} start {})",
            started(orchestrator)
        ),
    );
    let said = r.sweep();
    r.assert_kept(&wt, &said);
    assert!(
        starting(&said, &format!("kept {}: ", wt.display()))
            .contains("which is not this worktree's"),
        "{said}"
    );
}

#[test]
fn a_finished_worktrees_artifacts_are_set_aside_and_the_worktree_goes() {
    let r = subagent_board("done");
    ignore_as_here(&r);
    let wt = landed(&r);
    write(&wt, "artifacts/docs-app.port", "4321\n");
    write(&wt, "artifacts/tools-landed/shot.png", "a picture\n");

    // A dry run says where they would go, and moves nothing.
    let dry = r.sweep_with(|o| o.dry_run = true);
    let kept_in = r.root.join(SWEPT).join(format!("agent-{AGENT}"));
    assert!(
        text(&dry).contains(&format!(
            "its artifacts/ would move to {}",
            kept_in.display()
        )),
        "{}",
        text(&dry)
    );
    assert!(wt.join("artifacts/docs-app.port").is_file());
    assert!(!kept_in.exists());

    let said = r.sweep();
    r.assert_removed(&wt, &said);
    assert!(
        said.contains(&format!("its artifacts/ moved to {}", kept_in.display())),
        "{said}"
    );
    assert_eq!(
        fs::read_to_string(kept_in.join("docs-app.port")).unwrap(),
        "4321\n"
    );
    assert_eq!(
        fs::read_to_string(kept_in.join("tools-landed/shot.png")).unwrap(),
        "a picture\n"
    );
}

#[test]
fn artifacts_set_aside_go_back_when_git_refuses_the_removal() {
    let r = subagent_board("done");
    ignore_as_here(&r);
    let wt = landed(&r);
    write(&wt, "artifacts/tools-landed/shot.png", "a picture\n");
    let kept_in = r.root.join(SWEPT).join(format!("agent-{AGENT}"));
    let (there, watched) = (wt.clone(), wt.canonicalize().unwrap());
    let reached = Rc::new(Cell::new(false));
    let seen = Rc::clone(&reached);
    // Set aside, then an untracked file appears: git refuses the removal.
    let done = r.sweep_with(|o| {
        o.pause = Box::new(move |stage, what| {
            if stage == "aside" && Path::new(what).canonicalize().ok() == Some(watched.clone()) {
                seen.set(true);
                write(&there, "notes.txt", "written meanwhile\n");
            }
        });
    });
    assert!(reached.get(), "{}", text(&done));
    r.assert_kept(&wt, &text(&done));
    assert_eq!(
        fs::read_to_string(wt.join("artifacts/tools-landed/shot.png")).unwrap(),
        "a picture\n"
    );
    assert!(!kept_in.exists(), "{}", text(&done));
}

#[test]
fn artifacts_keep_a_worktree_whose_work_is_not_finished() {
    // Its task parked, neither live nor finished: what its artifacts hold may still be named on
    // its thread, so they keep the worktree, named.
    let r = subagent_board("backlog");
    ignore_as_here(&r);
    let wt = landed(&r);
    write(&wt, "artifacts/tools-landed/shot.png", "a picture\n");
    let said = r.sweep();
    r.assert_kept(&wt, &said);
    assert_eq!(
        starting(&said, &format!("kept {}: ", wt.display())),
        format!(
            "kept {}: merged, but holds files that did not land: artifacts/",
            wt.display()
        )
    );
    assert!(!r.root.join(SWEPT).exists(), "{said}");
}

#[test]
fn build_output_does_not_keep_a_merged_worktree() {
    let r = subagent_board("done");
    ignore_as_here(&r);
    let wt = landed(&r);
    write(&wt, ".target/debug/deps/libwork.rlib", "compiled\n");
    write(&wt, "build/client/index.html", "built\n");
    let said = r.sweep();
    r.assert_removed(&wt, &said);
}

#[test]
fn build_output_of_work_in_review_or_awaiting_its_land_goes_after_an_hour() {
    for status in ["review", "done"] {
        let r = subagent_board(status);
        let wt = r.add(&format!("agent-{AGENT}"), Some(SLUG), "main");
        r.commit(&wt, "extends RefCounted\n# in review\n");
        write(&wt, ".target/debug/deps/libwork.rlib", "compiled\n");
        let done = r.sweep_with(|o| o.now = Box::new(|| NOW + 7200.0));
        let said = text(&done);
        r.assert_kept(&wt, &said);
        assert_eq!(done.cleared.len(), 1, "{status}: {said}");
        assert_eq!(
            done.cleared[0].why,
            format!("task {SLUG} is {status}; nothing ran there for an hour")
        );
        assert!(!wt.join(".target").exists(), "{status}: {said}");
    }
}

#[test]
fn a_lands_worktree_goes_once_its_land_is_gone_and_the_nightlys_stays() {
    // Both where the land and the nightly make them, beside the main checkout.
    let r = Repo::new();
    let land = r.detached("symphony-wt/land-tools-x");
    let gone = dead_pid();
    r.lock(&land, &format!("symphony land: tools/x (pid {gone})"));
    write(&land, ".target/debug/deps/libwork.rlib", "compiled\n");
    let running = r.detached("symphony-wt/land-tools-y");
    r.lock(
        &running,
        &format!("symphony land: tools/y (pid {})", std::process::id()),
    );
    let nightly = r.detached("symphony-wt/nightly");
    r.lock(&nightly, NIGHTLY_LOCK);
    write(&nightly, "artifacts/nightly/report.json", "{}\n");
    let said = r.sweep();
    r.assert_removed(&land, &said);
    assert!(
        removed(&said, &land).contains(&format!("the land's lock, pid {gone} is not running")),
        "{said}"
    );
    for (wt, why) in [
        (&running, "a land runs there"),
        (&nightly, "the nightly's checkout"),
    ] {
        r.assert_kept(wt, &said);
        assert!(
            starting(&said, &format!("kept {}: ", wt.display())).contains(why),
            "{said}"
        );
    }
    assert!(nightly.join("artifacts/nightly/report.json").is_file());
}
