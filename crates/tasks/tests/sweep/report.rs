//! What a dry run says of every worktree (tools/sweep-removes-nothing): its size, the tasks tied to
//! it and their stage, whether its HEAD is in main, and what a sweep would do with it and why, then a
//! total of what it would free; a worktree with tracked edits or commits not in main is listed with
//! its size and never removed. And the branch a removed worktree had checked out, deleted once
//! merged.

use crate::common::Seed;
use crate::{Repo, git, text, write};
use std::path::Path;
use symphony_board::sweep::size;

/// The dry run's line for `wt`.
fn row(said: &str, wt: &Path) -> String {
    let start = format!("worktree {}: ", wt.display());
    let found: Vec<&str> = said.lines().filter(|l| l.starts_with(&start)).collect();
    assert_eq!(found.len(), 1, "{start}\n{said}");
    found[0].to_string()
}

#[test]
fn a_dry_run_says_every_worktree_and_what_a_sweep_would_free() {
    let r = Repo::with(
        Seed::default()
            .task("sim/gone", "done")
            .task("sim/edits", "done")
            .task("sim/ahead", "done")
            .task("sim/review", "review"),
    );
    let gone = r.merged("gone", "sim/gone");
    let edits = r.merged("edits", "sim/edits");
    write(
        &edits,
        "sim/world.gd",
        "extends RefCounted\n# not committed\n",
    );
    let ahead = r.add("ahead", Some("sim/ahead"), "main");
    r.commit(&ahead, "extends RefCounted\n# one\n");
    r.commit(&ahead, "extends RefCounted\n# two\n");
    let review = r.merged("review", "sim/review");

    let done = r.sweep_with(|o| o.dry_run = true);
    let said = text(&done);
    let held = [&gone, &edits, &ahead, &review].map(|wt| size::bytes(wt));
    let shown = held.map(size::shown);
    assert_eq!(
        row(&said, &r.root),
        format!(
            "worktree {}: no task; merged into main; kept: the main checkout",
            r.root.display()
        )
    );
    assert!(
        row(&said, &gone).starts_with(&format!(
            "worktree {}: {}; task sim/gone (done); merged into main; would remove: on sim/gone at ",
            gone.display(),
            shown[0]
        )),
        "{said}"
    );
    assert_eq!(
        row(&said, &edits),
        format!(
            "worktree {}: {}; task sim/edits (done); merged into main; kept for an orchestrator to \
             decide: holds files that did not land: sim/world.gd",
            edits.display(),
            shown[1]
        )
    );
    assert_eq!(
        row(&said, &ahead),
        format!(
            "worktree {}: {}; task sim/ahead (done); not merged: 2 commits not in main; kept for an \
             orchestrator to decide: its HEAD is not in main",
            ahead.display(),
            shown[2]
        )
    );
    assert_eq!(
        row(&said, &review),
        format!(
            "worktree {}: {}; task sim/review (review); merged into main; kept: task sim/review is \
             review",
            review.display(),
            shown[3]
        )
    );
    assert_eq!(done.freed, held[0], "{said}");
    assert!(
        said.ends_with(&format!(
            "; would free {} of the {} its 4 worktrees measured hold",
            shown[0],
            size::shown(held.iter().sum())
        )),
        "{said}"
    );
    for wt in [&gone, &edits, &ahead, &review] {
        assert!(wt.is_dir(), "a dry run removes nothing");
    }

    // The sweep itself removes the one, and neither the edits nor the commits not in main.
    let said = r.sweep();
    r.assert_removed(&gone, &said);
    for wt in [&edits, &ahead, &review] {
        r.assert_kept(wt, &said);
    }
    assert_eq!(
        git(&r.root, &["rev-parse", "sim/ahead"]),
        git(&ahead, &["rev-parse", "HEAD"])
    );
}

#[test]
fn a_clear_is_said_on_its_own_worktrees_line_inside_the_main_checkout_too() {
    // Claude Code makes its agents' worktrees inside the main checkout's folder.
    let r = Repo::with(Seed::default().task("sim/review", "review"));
    let wt = r.add(
        "repo/.claude/worktrees/agent-a5a6",
        Some("sim/review"),
        "main",
    );
    write(&wt, ".target/debug/deps/libwork.rlib", "compiled\n");
    let done = r.sweep_with(|o| {
        o.dry_run = true;
        o.now = Box::new(|| crate::common::NOW + 7200.0);
    });
    let said = text(&done);
    assert!(
        row(&said, &wt).ends_with(&format!(
            "kept: task sim/review is review; would clear its build output {} ({}): task \
             sim/review is review; nothing ran there for an hour",
            wt.join(".target").display(),
            size::shown(done.cleared[0].bytes)
        )),
        "{said}"
    );
    assert!(
        row(&said, &r.root).ends_with("kept: the main checkout"),
        "{said}"
    );
}

#[test]
fn a_removed_worktrees_merged_branch_goes_with_it() {
    let r = Repo::with(Seed::default().task("sim/a", "done"));
    let wt = r.merged("a", "sim/a");

    let dry = text(&r.sweep_with(|o| o.dry_run = true));
    assert!(
        dry.contains("would delete branch sim/a (merged into main, its worktree removed)")
            && dry.contains("; would delete 1 merged branch of removed worktrees"),
        "{dry}"
    );
    assert_eq!(r.branch("sim/a"), "+ sim/a", "a dry run deletes nothing");

    let said = r.sweep();
    r.assert_removed(&wt, &said);
    assert_eq!(r.branch("sim/a"), "", "{said}");
    assert!(
        said.contains("deleted branch sim/a (merged into main, its worktree removed)")
            && said.contains("; deleted 1 merged branch of removed worktrees"),
        "{said}"
    );
}

#[test]
fn a_branch_that_moved_after_its_worktree_went_stays() {
    // Between the removal and the deletion, the branch is moved to a commit not in main: what was
    // judged merged is no longer what it names.
    let r = Repo::with(Seed::default().task("sim/a", "done"));
    let wt = r.merged("a", "sim/a");
    let elsewhere = r.add("elsewhere", None, "main");
    r.commit(&elsewhere, "extends RefCounted\n# elsewhere\n");
    let moved_to = git(&elsewhere, &["rev-parse", "HEAD"]);
    let (root, to) = (r.root.clone(), moved_to.clone());
    let done = r.sweep_with(|o| {
        o.pause = Box::new(move |stage, what| {
            if stage == "delete" && what == "sim/a" {
                git(&root, &["branch", "-f", "sim/a", &to]);
            }
        });
    });
    let said = text(&done);
    r.assert_removed(&wt, &said);
    assert_eq!(git(&r.root, &["rev-parse", "sim/a"]), moved_to, "{said}");
    assert!(done.left_branches.is_empty(), "{said}");
}
