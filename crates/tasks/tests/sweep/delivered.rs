//! The sweep's second proof of a landing (tools/github-sweep-by-receipt): a head a pull request
//! squashed into main is in no history, and goes, with its branch, only while the receipt its land
//! kept is borne out by git: of that exact head, the head still retained, the squash the commit it
//! recorded, on the remote and in main here. Every other guard of the sweep holds over it. Each
//! repository here is a scratch one with a scratch bare remote; nothing asks a host.

use crate::{HeldCwd, Repo, git, text, write};
use std::fs;
use std::path::{Path, PathBuf};
use symphony_board::land::delivery::{Journal, RETAINED, Receipt, Stage};
use symphony_board::sweep::Sweep;

const SLUG: &str = "tools/squashed";

/// The lands' folder of `r`'s repository.
fn lands(r: &Repo) -> PathBuf {
    let common = symphony_board::git::common_dir(&r.root).unwrap();
    symphony_board::sweep::trigger::folder(&common).join(symphony_board::land::FOLDER)
}

/// A scratch bare remote `origin` of `r`, holding its main.
fn remote(r: &Repo) -> PathBuf {
    let bare = r.dir.join("origin.git");
    git(
        &r.dir,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    git(
        &r.root,
        &["remote", "add", "origin", bare.to_str().unwrap()],
    );
    git(&r.root, &["push", "-q", "origin", "main"]);
    bare
}

/// A worktree `name` on a new branch `slug` from main with one commit of its own file, pushed as
/// a land's publication pushes it: the worktree and its head.
fn worked(r: &Repo, name: &str, slug: &str) -> (PathBuf, String) {
    let wt = r.add(name, Some(slug), "main");
    write(&wt, &format!("sim/{name}.gd"), "extends Node\n");
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "work"]);
    git(&r.root, &["push", "-q", "origin", slug]);
    let head = git(&wt, &["rev-parse", "HEAD"]);
    (wt, head)
}

/// `head` squashed onto main as a host does, in one commit of one parent, which the remote and
/// the main checkout both hold: the receipt of it, not kept.
fn squashed(r: &Repo, slug: &str, head: &str) -> Receipt {
    let parent = git(&r.root, &["rev-parse", "main"]);
    git(&r.root, &["merge", "-q", "--squash", head]);
    git(&r.root, &["commit", "-qm", &format!("{slug}: its title")]);
    git(&r.root, &["push", "-q", "origin", "main"]);
    Receipt {
        slug: slug.into(),
        task: 7,
        operation: "task".into(),
        repository: 11,
        name: "owner/name".into(),
        pull: 5,
        head: head.into(),
        base_observed: parent.clone(),
        parent,
        squash: git(&r.root, &["rev-parse", "main"]),
        tree: git(&r.root, &["rev-parse", "main^{tree}"]),
        revision: "r1".into(),
        by: "lander".into(),
        import: None,
    }
}

/// `receipt` kept as the land keeps it, under its own task's name.
fn keep(r: &Repo, receipt: &Receipt) {
    let path = Receipt::path(&lands(r), &receipt.slug);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_string_pretty(receipt).unwrap()).unwrap();
}

/// `head` kept reachable as the land keeps a delivered one.
fn retain(r: &Repo, slug: &str, head: &str) {
    git(&r.root, &["update-ref", &format!("{RETAINED}{slug}"), head]);
}

/// A repository whose task [`SLUG`] was worked in a worktree and delivered by a squash, the
/// receipt kept and the head retained: the worktree and the receipt.
fn delivered() -> (Repo, PathBuf, Receipt) {
    let r = Repo::new();
    remote(&r);
    let (wt, head) = worked(&r, "squashed", SLUG);
    let receipt = squashed(&r, SLUG, &head);
    keep(&r, &receipt);
    retain(&r, SLUG, &head);
    (r, wt, receipt)
}

/// How a line says `receipt`'s squash.
fn how(receipt: &Receipt) -> String {
    format!(
        "squashed into main as {} by pull request #5",
        &receipt.squash[..12]
    )
}

/// What a sweep decided of `wt`, as a dry run would say it.
fn verdict(result: &Sweep, wt: &Path) -> String {
    let row = (result.rows.iter()).find(|row| row.path == wt);
    row.map(|row| row.verdict.clone()).unwrap_or_default()
}

/// A sweep kept `wt` and its branch as work that never landed.
fn assert_unlanded(r: &Repo, wt: &Path, result: &Sweep) {
    let said = text(result);
    r.assert_kept(wt, &said);
    assert!(result.removed.is_empty(), "{said}");
    assert!(!r.branch(SLUG).is_empty(), "the branch stays\n{said}");
    assert!(result.left_branches.is_empty(), "{said}");
}

#[test]
fn a_squashed_head_goes_with_its_branch_and_its_head_stays_retained() {
    let (r, wt, receipt) = delivered();
    assert!(
        !symphony_board::sweep::git::ancestor(&r.root, &receipt.head, "main"),
        "a squash leaves the head in no history"
    );
    let result = r.sweep_with(|_| {});
    let said = text(&result);
    r.assert_removed(&wt, &said);
    assert!(
        said.contains(&format!(
            "on {SLUG} at {}, {}, clean",
            &receipt.head[..12],
            how(&receipt)
        )),
        "{said}"
    );
    assert!(r.branch(SLUG).is_empty(), "{said}");
    assert!(
        said.contains(&format!(
            "deleted branch {SLUG} ({}, its worktree removed)",
            how(&receipt)
        )),
        "{said}"
    );
    assert_eq!(result.left_branches, [SLUG]);
    assert_eq!(result.squashed.get(SLUG), Some(&how(&receipt)));
    // What proves it is all still there: the head, its receipt, and the remote's branch.
    assert_eq!(
        git(&r.root, &["rev-parse", &format!("{RETAINED}{SLUG}")]),
        receipt.head
    );
    assert_eq!(Receipt::read(&lands(&r), SLUG), Some(receipt.clone()));
    let bare = r.dir.join("origin.git");
    assert_eq!(git(&bare, &["rev-parse", SLUG]), receipt.head);
}

#[test]
fn a_dry_run_says_a_squashed_head_landed_and_removes_nothing() {
    let (r, wt, receipt) = delivered();
    let result = r.sweep_with(|o| o.dry_run = true);
    let said = text(&result);
    assert!(wt.is_dir() && !r.branch(SLUG).is_empty(), "{said}");
    assert!(
        said.contains(&format!("would remove {}", wt.display())),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "would delete branch {SLUG} ({}, its worktree removed)",
            how(&receipt)
        )),
        "{said}"
    );
    let row = (result.rows.iter()).find(|row| row.path == wt).unwrap();
    assert_eq!(row.merged, Some(how(&receipt)));
}

#[test]
fn a_detached_worktree_and_an_agent_branch_at_the_squashed_head_go_too() {
    let (r, wt, receipt) = delivered();
    let reviewer = r.add("reviewer", None, &receipt.head);
    git(
        &r.root,
        &["branch", "worktree-agent-abc", receipt.head.as_str()],
    );
    // An agent branch one commit on is of another head.
    let on = r.add("agent-def", Some("worktree-agent-def"), &receipt.head);
    r.commit(&on, "extends RefCounted\n# on\n");
    git(&on, &["checkout", "-q", "--detach", "main"]);
    let result = r.sweep_with(|_| {});
    let said = text(&result);
    r.assert_removed(&wt, &said);
    r.assert_removed(&reviewer, &said);
    assert!(
        said.contains(&format!(
            "detached at {}, {}",
            &receipt.head[..12],
            how(&receipt)
        )),
        "{said}"
    );
    assert_eq!(result.branches, ["worktree-agent-abc"], "{said}");
    assert!(
        said.contains(&format!(
            "deleted branch worktree-agent-abc ({}, no worktree holds it)",
            how(&receipt)
        )),
        "{said}"
    );
    assert!(!r.branch("worktree-agent-def").is_empty(), "{said}");
}

#[test]
fn a_head_that_moved_on_after_its_delivery_is_kept() {
    let (r, wt, _) = delivered();
    write(&wt, "sim/more.gd", "extends Node\n");
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "more"]);
    let result = r.sweep_with(|_| {});
    assert_unlanded(&r, &wt, &result);
    assert_eq!(
        verdict(&result, &wt),
        "kept for an orchestrator to decide: its HEAD is not in main"
    );
    // Nor is a worktree behind the delivered head, at a commit of the branch's own.
    let (r, wt, receipt) = delivered();
    write(&wt, "sim/more.gd", "extends Node\n");
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "more"]);
    let later = git(&wt, &["rev-parse", "HEAD"]);
    let receipt = Receipt {
        head: later.clone(),
        ..receipt
    };
    keep(&r, &receipt);
    retain(&r, SLUG, &later);
    git(&wt, &["reset", "-q", "--hard", "HEAD~1"]);
    assert_unlanded(&r, &wt, &r.sweep_with(|_| {}));
}

#[test]
fn a_dry_run_counts_the_commits_of_a_head_no_receipt_proves() {
    let (r, wt, _) = delivered();
    fs::remove_file(Receipt::path(&lands(&r), SLUG)).unwrap();
    let result = r.sweep_with(|o| o.dry_run = true);
    let row = (result.rows.iter()).find(|row| row.path == wt).unwrap();
    assert_eq!(
        row.merged.as_deref(),
        Some("not merged: 1 commit not in main")
    );
}

#[test]
fn a_receipt_that_is_missing_torn_or_another_tasks_proves_nothing() {
    // Missing.
    let (r, wt, receipt) = delivered();
    let path = Receipt::path(&lands(&r), SLUG);
    fs::remove_file(&path).unwrap();
    assert_unlanded(&r, &wt, &r.sweep_with(|_| {}));
    // Torn.
    fs::write(&path, "{\"slug\": ").unwrap();
    assert_unlanded(&r, &wt, &r.sweep_with(|_| {}));
    // A receipt kept under another task's name, whatever it says inside.
    fs::remove_file(&path).unwrap();
    let other = Receipt::path(&lands(&r), "tools/other");
    fs::write(&other, serde_json::to_string(&receipt).unwrap()).unwrap();
    assert_unlanded(&r, &wt, &r.sweep_with(|_| {}));
    // And one of another head: the branch's own first commit was never what was delivered.
    fs::remove_file(&other).unwrap();
    let of_main = Receipt {
        head: receipt.parent.clone(),
        ..receipt.clone()
    };
    keep(&r, &of_main);
    assert_unlanded(&r, &wt, &r.sweep_with(|_| {}));
    // No folder at all, as in a project that never delivered by pull request.
    fs::remove_dir_all(lands(&r)).unwrap();
    assert_unlanded(&r, &wt, &r.sweep_with(|_| {}));
}

#[test]
fn a_forged_receipt_git_does_not_bear_out_proves_nothing() {
    let (r, wt, receipt) = delivered();
    // A later commit of main, real, with its true parent and tree: not this head's squash.
    write(&r.root, "sim/later.gd", "extends Node\n");
    git(&r.root, &["add", "-A"]);
    git(&r.root, &["commit", "-qm", "later"]);
    git(&r.root, &["push", "-q", "origin", "main"]);
    let later = git(&r.root, &["rev-parse", "main"]);
    let later_tree = git(&r.root, &["rev-parse", "main^{tree}"]);
    // A merge commit of the head, as a host that did not squash makes.
    let merge = git(
        &r.root,
        &[
            "commit-tree",
            "-p",
            &later,
            "-p",
            &receipt.head,
            "-m",
            "merged",
            &later_tree,
        ],
    );
    let written = r.dir.join("written");
    let option = format!("--output={}", written.display());
    let forged = [
        (
            "its squash named by a branch",
            Receipt {
                squash: "main".into(),
                ..receipt.clone()
            },
        ),
        (
            "its squash a short id",
            Receipt {
                squash: receipt.squash[..12].into(),
                ..receipt.clone()
            },
        ),
        (
            "a commit git does not hold",
            Receipt {
                squash: "f".repeat(40),
                ..receipt.clone()
            },
        ),
        (
            "another parent than the commit's",
            Receipt {
                parent: later.clone(),
                ..receipt.clone()
            },
        ),
        (
            "no parent",
            Receipt {
                parent: String::new(),
                ..receipt.clone()
            },
        ),
        (
            "another tree than the commit's",
            Receipt {
                tree: later_tree.clone(),
                ..receipt.clone()
            },
        ),
        (
            "a real commit of main that is not the head's squash",
            Receipt {
                squash: later.clone(),
                parent: receipt.squash.clone(),
                tree: later_tree.clone(),
                ..receipt.clone()
            },
        ),
        (
            "a merge commit",
            Receipt {
                squash: merge.clone(),
                parent: later.clone(),
                tree: later_tree.clone(),
                ..receipt.clone()
            },
        ),
        (
            "an option in its squash's place",
            Receipt {
                squash: option.clone(),
                ..receipt.clone()
            },
        ),
        (
            "an option in its parent's place",
            Receipt {
                parent: option.clone(),
                ..receipt.clone()
            },
        ),
        (
            "an option in its tree's place",
            Receipt {
                tree: option.clone(),
                ..receipt.clone()
            },
        ),
    ];
    for (what, forged) in forged {
        keep(&r, &forged);
        let result = r.sweep_with(|_| {});
        assert_unlanded(&r, &wt, &result);
        assert_eq!(
            verdict(&result, &wt),
            "kept for an orchestrator to decide: its HEAD is not in main",
            "{what}"
        );
    }
    // No git read an option a receipt put where an id goes: `git log --output=` writes a file.
    assert!(!written.exists(), "{}", written.display());
    // The true receipt put back proves it again: the cases above failed on what they forged.
    keep(&r, &receipt);
    r.assert_removed(&wt, &r.sweep());
}

#[test]
fn a_retained_head_lost_or_moved_keeps_the_worktree_and_the_branch() {
    let (r, wt, receipt) = delivered();
    let retained = format!("{RETAINED}{SLUG}");
    git(&r.root, &["update-ref", "-d", &retained]);
    assert_unlanded(&r, &wt, &r.sweep_with(|_| {}));
    git(&r.root, &["update-ref", &retained, &receipt.parent]);
    assert_unlanded(&r, &wt, &r.sweep_with(|_| {}));
    retain(&r, SLUG, &receipt.head);
    r.assert_removed(&wt, &r.sweep());
}

#[test]
fn a_squash_the_remote_is_not_known_to_hold_proves_nothing() {
    // The remote's branch as last fetched is before the squash.
    let (r, wt, receipt) = delivered();
    let tracking = "refs/remotes/origin/main";
    git(&r.root, &["update-ref", tracking, &receipt.parent]);
    assert_unlanded(&r, &wt, &r.sweep_with(|_| {}));
    // Never fetched.
    git(&r.root, &["update-ref", "-d", tracking]);
    assert_unlanded(&r, &wt, &r.sweep_with(|_| {}));
    // No remote at all.
    git(&r.root, &["remote", "remove", "origin"]);
    assert_unlanded(&r, &wt, &r.sweep_with(|_| {}));
}

#[test]
fn a_delivery_the_main_checkout_has_not_followed_yet_is_not_landed_here() {
    let (r, wt, receipt) = delivered();
    git(&r.root, &["reset", "-q", "--hard", &receipt.parent]);
    assert_unlanded(&r, &wt, &r.sweep_with(|_| {}));
    git(&r.root, &["merge", "-q", "--ff-only", "origin/main"]);
    r.assert_removed(&wt, &r.sweep());
}

#[test]
fn every_guard_of_the_sweep_holds_over_a_squashed_head() {
    // Files that did not land.
    let (r, wt, _) = delivered();
    write(&wt, "sim/unsaved.gd", "extends Node\n");
    let said = r.sweep();
    r.assert_kept(&wt, &said);
    assert!(
        said.contains("holds files that did not land: sim/unsaved.gd"),
        "{said}"
    );
    assert!(!r.branch(SLUG).is_empty(), "{said}");
    // A tracked edit.
    let (r, wt, _) = delivered();
    write(&wt, "sim/world.gd", "extends RefCounted\n# edited\n");
    r.assert_kept(&wt, &r.sweep());
    // A lock the sweep does not know.
    let (r, wt, _) = delivered();
    r.lock(&wt, "mine");
    let said = r.sweep();
    r.assert_kept(&wt, &said);
    assert!(
        said.contains("locked by something the sweep does not know"),
        "{said}"
    );
    // Another worktree inside it.
    let (r, wt, _) = delivered();
    let inner = wt.join("nested");
    git(
        &r.root,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            inner.to_str().unwrap(),
            "main",
        ],
    );
    let result = r.sweep_with(|_| {});
    assert!(wt.is_dir(), "{}", text(&result));
    assert_eq!(verdict(&result, &wt), "kept: another worktree is inside it");
    // A live session bound there.
    let (r, wt, _) = delivered();
    let handle = r.bind(&wt);
    let result = r.sweep_with(|_| {});
    r.assert_kept(&wt, &text(&result));
    assert_eq!(
        verdict(&result, &wt),
        format!("kept: session {handle} is bound there, live or stale")
    );
    // A task still open on the branch.
    let (r, wt, _) = delivered();
    r.task(SLUG);
    let said = r.sweep();
    r.assert_kept(&wt, &said);
    assert!(said.contains(&format!("task {SLUG} is ready")), "{said}");
    // A process working inside it.
    let (r, wt, _) = delivered();
    let _held = HeldCwd::at(&wt);
    let inside = wt.clone();
    let said = text(&r.sweep_with(move |o| {
        o.cwds = Box::new(move || Some(vec![inside.clone()]));
    }));
    r.assert_kept(&wt, &said);
    assert!(said.contains("a process is working inside it"), "{said}");
    // Touched within the window.
    let (r, wt, _) = delivered();
    let said = text(&r.sweep_now(|_| {}));
    r.assert_kept(&wt, &said);
    // A head that changed between the judgement and the removal.
    let (r, wt, _) = delivered();
    let moving = wt.clone();
    let result = r.sweep_with(move |o| {
        o.pause = Box::new(move |stage, _| {
            if stage == "remove" {
                git(
                    &moving,
                    &["commit", "-q", "--allow-empty", "-m", "one more"],
                );
            }
        });
    });
    assert_unlanded(&r, &wt, &result);
}

/// The journal of a delivery of [`SLUG`] at `receipt`'s head that went as far as `stage`, kept
/// in `r`'s lands' folder as the land keeps it.
fn journal(r: &Repo, receipt: &Receipt, stage: Stage) {
    let merged = stage >= Stage::RemoteMerged;
    Journal {
        slug: SLUG.into(),
        operation: "task".into(),
        stage,
        head: receipt.head.clone(),
        base_observed: receipt.parent.clone(),
        revision: "r1".into(),
        title: "its title".into(),
        body: String::new(),
        by: "lander".into(),
        closes: Vec::new(),
        stood: String::new(),
        binding: None,
        squash: merged.then(|| receipt.squash.clone()),
        held: None,
        import: None,
        green: None,
        armed: false,
    }
    .write(&lands(r))
    .unwrap();
}

#[test]
fn a_delivery_asked_to_merge_keeps_no_branch_as_its_land_reads_its_journal_alone() {
    // From the merge's request on, a land run again goes on from its journal and never reads the
    // branch: the land's own sweep, started while its journal says Dispatching, takes it.
    for stage in [
        Stage::MergeRequested,
        Stage::RemoteMerged,
        Stage::Reconciled,
        Stage::Closed,
        Stage::Dispatching,
    ] {
        let (r, wt, receipt) = delivered();
        journal(&r, &receipt, stage);
        let result = r.sweep_with(|_| {});
        let said = text(&result);
        r.assert_removed(&wt, &said);
        assert!(r.branch(SLUG).is_empty(), "{stage:?}\n{said}");
        assert_eq!(result.left_branches, [SLUG], "{stage:?}\n{said}");
        assert!(result.kept_branches.is_empty(), "{stage:?}\n{said}");
    }
}

#[test]
fn a_delivery_not_yet_asked_to_merge_keeps_the_branch_its_land_reads() {
    for stage in [Stage::Prepared, Stage::Eligible] {
        let (r, wt, receipt) = delivered();
        journal(&r, &receipt, stage);
        let result = r.sweep_with(|_| {});
        let said = text(&result);
        r.assert_removed(&wt, &said);
        assert!(!r.branch(SLUG).is_empty(), "{stage:?}\n{said}");
        assert!(
            said.contains(&format!(
                "kept branch {SLUG}: a delivery of it by pull request is not yet asked to merge, \
                 and `just land {SLUG}` run again reads its tip"
            )),
            "{stage:?}\n{said}"
        );
        assert!(
            result.squashed.is_empty(),
            "nothing deleted is said squashed"
        );
    }
}

#[test]
fn a_branch_in_mains_history_is_merged_as_before_whatever_receipts_say() {
    let (r, _, receipt) = delivered();
    let wt = r.merged("merged", "tools/merged");
    // A receipt of its head changes nothing of how it is said: ancestry is read first.
    let head = git(&wt, &["rev-parse", "HEAD"]);
    keep(
        &r,
        &Receipt {
            slug: "tools/merged".into(),
            head,
            ..receipt
        },
    );
    let result = r.sweep_with(|_| {});
    let said = text(&result);
    r.assert_removed(&wt, &said);
    assert!(
        said.contains("on tools/merged at ") && said.contains(", merged into main, clean"),
        "{said}"
    );
    assert!(
        said.contains("deleted branch tools/merged (merged into main, its worktree removed)"),
        "{said}"
    );
    assert!(!result.squashed.contains_key("tools/merged"), "{said}");
}
