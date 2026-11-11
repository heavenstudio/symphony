//! A task's tip as the repository tells it ([`tip`]): its branch's newest commit, and the commit
//! the branch first moved to from one a record judged, after the record was written, as its reflog
//! tells or, without one, along its own line of first parents.

use super::*;
use symphony_board::alarm::{Tip, Written};
use symphony_board::cli::probes::tip;

const HOUR: f64 = 3600.0;

/// The commit git's `args` make in `dir` at `hour` on 2026-11-08 (UTC), as a tip.
fn made_at(dir: &Path, args: &[&str], hour: u32) -> Tip {
    let made = format!("2026-11-08T{hour:02}:00:00+00:00");
    let done = symphony_board::sys::output(
        Command::new("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-C"])
            .arg(dir)
            .args(args)
            .env("GIT_COMMITTER_DATE", made),
    )
    .unwrap();
    assert!(done.status.success(), "git {args:?}");
    let head = symphony_board::sys::output(
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["rev-parse", "HEAD"]),
    )
    .unwrap();
    Tip {
        commit: String::from_utf8_lossy(&head.stdout).trim().to_string(),
        at: 1_794_096_000.0 + f64::from(hour) * HOUR,
    }
}

/// A commit of nothing on the branch checked out in `dir` at `hour`.
fn work_at(dir: &Path, hour: u32) -> Tip {
    made_at(dir, &["commit", "-q", "--allow-empty", "-m", "work"], hour)
}

/// What `slug`'s branch in `dir` first moved to from `judged` after a record judged it then.
fn after(dir: &Path, slug: &str, judged: &Tip) -> Option<Tip> {
    let record = Written {
        sid: 1,
        tip: &judged.commit,
        at: judged.at,
    };
    tip(dir, slug, Some(record))
}

#[test]
fn a_task_s_tip_is_its_branch_s_newest_commit_and_when_it_was_made() {
    let dir = repo("tip");
    git(&dir, &["checkout", "-q", "-b", "sim/a"]);
    let expected = work_at(&dir, 10);
    assert_eq!(expected.at, 1_794_132_000.0);
    git(&dir, &["checkout", "-q", "main"]);
    assert_eq!(tip(&dir, "sim/a", None), Some(expected.clone()));
    assert_eq!(
        tip(&dir, "review/sim/a", None),
        Some(expected),
        "a review mirror's is its branch's"
    );
    assert_eq!(tip(&dir, "sim/none", None), None);
    let nowhere = outside("no-repo-tip");
    assert_eq!(tip(&nowhere, "sim/a", None), None);
    fs::remove_dir_all(nowhere).unwrap();
}

#[test]
fn the_commit_after_one_judged_is_the_next_the_branch_moved_to_however_many_follow() {
    let dir = repo("after");
    git(&dir, &["checkout", "-q", "-b", "sim/gone"]);
    let gone = work_at(&dir, 9);
    git(&dir, &["checkout", "-q", "main"]);
    git(&dir, &["checkout", "-q", "-b", "sim/a"]);
    let judged = work_at(&dir, 10);
    let fixed = work_at(&dir, 11);
    git(&dir, &["checkout", "-q", "main"]);
    git(&dir, &["commit", "-q", "--allow-empty", "-m", "main moves"]);
    git(&dir, &["checkout", "-q", "sim/a"]);
    let merged = made_at(&dir, &["merge", "-q", "--no-edit", "main"], 12);
    let now = work_at(&dir, 13);
    assert_eq!(after(&dir, "sim/a", &judged), Some(fixed.clone()));
    assert_eq!(
        after(&dir, "sim/a", &fixed),
        Some(merged),
        "a merge of main is the branch's"
    );
    assert_eq!(after(&dir, "sim/a", &now), None, "none after the tip now");
    // A commit the branch never held, an id of none, and text git would read as an option: the
    // tip now stands in.
    let unplaced = [gone.commit.as_str(), "0123abc", "--output=x"];
    for commit in unplaced {
        let record = Written {
            sid: 1,
            tip: commit,
            at: now.at,
        };
        let asked = tip(&dir, "sim/a", Some(record));
        assert_eq!(asked, Some(now.clone()), "{commit}");
    }
}

#[test]
fn a_commit_amended_or_reset_away_is_replaced_by_what_the_branch_first_moved_to_then() {
    let dir = repo("amended");
    git(&dir, &["checkout", "-q", "-b", "sim/a"]);
    let judged = work_at(&dir, 10);
    let amend = ["commit", "-q", "--amend", "--allow-empty", "-m", "fixed"];
    let amended = made_at(&dir, &amend, 11);
    assert_eq!(after(&dir, "sim/a", &judged), Some(amended.clone()));
    let later = work_at(&dir, 13);
    assert_eq!(
        after(&dir, "sim/a", &judged),
        Some(amended),
        "a later commit moves it never"
    );
    // Reset to main, whose commit is older: from when the branch moved there.
    let reset = made_at(&dir, &["reset", "-q", "--hard", "main"], 14);
    work_at(&dir, 15);
    assert_eq!(after(&dir, "sim/a", &later), Some(reset));
}

#[test]
fn adversarial_an_amended_tip_a_merge_brings_back_keeps_its_first_replacement() {
    let dir = repo("amended-merged");
    git(&dir, &["checkout", "-q", "-b", "sim/a"]);
    let judged = work_at(&dir, 10);
    git(&dir, &["branch", "held-judged"]);
    let amend = ["commit", "-q", "--amend", "--allow-empty", "-m", "fixed"];
    let amended = made_at(&dir, &amend, 11);
    work_at(&dir, 13);
    let merge = ["merge", "-q", "--no-ff", "--no-edit", "held-judged"];
    made_at(&dir, &merge, 14);
    assert_eq!(
        after(&dir, "sim/a", &judged),
        Some(amended),
        "the tip judged, on the merge's second parent, left the branch's line at its amend"
    );
}

#[test]
fn adversarial_a_record_of_a_tip_the_branch_came_back_to_waits_on_the_move_after_it() {
    let dir = repo("returned");
    git(&dir, &["checkout", "-q", "-b", "sim/a"]);
    let judged = work_at(&dir, 10);
    let fixed = work_at(&dir, 11);
    made_at(&dir, &["reset", "-q", "--hard", &judged.commit], 12);
    // Changes asked of it again at half past twelve.
    let again = Tip {
        at: judged.at + 2.5 * HOUR,
        ..judged.clone()
    };
    assert_eq!(
        after(&dir, "sim/a", &judged),
        Some(fixed.clone()),
        "the first record's end"
    );
    assert_eq!(after(&dir, "sim/a", &again), None, "the new record waits");
    let next = work_at(&dir, 13);
    assert_eq!(
        (after(&dir, "sim/a", &judged), after(&dir, "sim/a", &again)),
        (Some(fixed), Some(next))
    );
}

#[test]
fn without_a_reflog_the_commit_after_one_judged_is_on_the_branch_s_own_line_alone() {
    let dir = repo("no-reflog");
    git(&dir, &["checkout", "-q", "-b", "sim/a"]);
    let judged = work_at(&dir, 10);
    let fixed = work_at(&dir, 11);
    // On sim/b the tip judged is amended away, then merged back as a merge's second parent.
    git(&dir, &["checkout", "-q", "-b", "sim/b", "main"]);
    let held = work_at(&dir, 12);
    git(&dir, &["branch", "held"]);
    made_at(
        &dir,
        &["commit", "-q", "--amend", "--allow-empty", "-m", "fixed"],
        13,
    );
    work_at(&dir, 14);
    made_at(&dir, &["merge", "-q", "--no-ff", "--no-edit", "held"], 15);
    let now = work_at(&dir, 16);
    git(&dir, &["reflog", "expire", "--expire=all", "--all"]);
    assert_eq!(
        after(&dir, "sim/a", &judged),
        Some(fixed),
        "the next on its line, when it was made"
    );
    assert_eq!(
        after(&dir, "sim/b", &held),
        Some(now),
        "a merge reaching it through its other parent follows it on no line: the tip now"
    );
}
