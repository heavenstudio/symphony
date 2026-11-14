//! The default branch's hours from two `git log` reads of a scratch repository: commits by the hour
//! they reached the branch, merges and lines along the branch's first parents, a branch's own merge
//! of the default branch counted as neither, a commit that changed only what is left out counted as
//! none, the generator's outputs, lock files and binary files left out of the lines, and a read
//! that fails.

use super::*;
use crate::reports::{DAY, HOUR, day_start};
use crate::test_tree::Tree;
use symphony_api::reports::ReportHour;

/// A UTC midnight in January.
fn midnight() -> f64 {
    day_start(1_800_000_000.0)
}

/// Run git in `root` as of `hours` after midnight, with no user or system settings.
fn git(root: &Path, hours: f64, args: &[&str]) {
    #[allow(clippy::cast_possible_truncation)]
    let date = format!("@{} +0000", (midnight() + hours * HOUR) as i64);
    let mut command = symphony_board::sys::Command::new("git");
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
    ] {
        command.env_remove(name);
    }
    let done = symphony_board::sys::output(
        command
            .args([
                "-c",
                "user.name=Reports test",
                "-c",
                "user.email=r@example.invalid",
            ])
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date),
    )
    .unwrap();
    assert!(done.status.success(), "{args:?}: {done:?}");
}

/// `count` numbered lines.
fn lines(count: usize, word: &str) -> String {
    (0..count).map(|n| format!("{word} {n}\n")).collect()
}

/// Write `files` in `tree` and commit them as of `hours`, its subject `subject`.
fn commit(tree: &Tree, hours: f64, subject: &str, files: &[(&str, &str)]) {
    for (path, text) in files {
        tree.write(path, text);
    }
    git(&tree.root, hours, &["add", "-A"]);
    git(&tree.root, hours, &["commit", "-qm", subject]);
}

/// A project whose generator's outputs are `dist/`, its history: a commit the day before, one on
/// main at 00:30 with a lock file, a branch's work and revert merged at 02:15 with a generated file
/// and a picture, a branch that took main in at 02:48 before its merge at 03:12, a commit on main
/// between, a branch changing a lock file alone, merged at 03:30, and the generator's own commit
/// on main at 03:45.
fn repository() -> Tree {
    let tree = Tree::new();
    tree.write(
        "symphony.toml",
        "[land]\ngenerators = [{ name = \"dist\", outputs = \"dist/\", command = [\"true\"] }]\n",
    );
    git(&tree.root, -2.0, &["init", "-q", "-b", "main"]);
    commit(&tree, -2.0, "base", &[("a.txt", &lines(3, "a"))]);
    commit(
        &tree,
        0.5,
        "docs: owner verdicts",
        &[
            ("a.txt", &lines(5, "a")),
            ("Cargo.lock", &lines(10, "lock")),
        ],
    );
    git(&tree.root, 1.0, &["switch", "-qc", "feature"]);
    commit(
        &tree,
        1.0,
        "work",
        &[
            ("b.txt", &lines(5, "b")),
            ("dist/app.js", &lines(100, "js")),
            ("crates/x/Cargo.lock", &lines(7, "lock")),
        ],
    );
    std::fs::write(tree.path("img.png"), [0u8, 159, 146, 150, 0, 1]).unwrap();
    commit(
        &tree,
        1.5,
        "Revert \"docs: owner verdicts\"",
        &[("a.txt", &lines(3, "a"))],
    );
    git(&tree.root, 2.25, &["switch", "-q", "main"]);
    git(
        &tree.root,
        2.25,
        &["merge", "-q", "--no-ff", "-m", "Merge feature", "feature"],
    );
    git(&tree.root, 2.5, &["switch", "-qc", "other"]);
    commit(&tree, 2.5, "other work", &[("c.txt", "c\n")]);
    git(&tree.root, 2.6, &["switch", "-q", "main"]);
    commit(&tree, 2.6, "tweak", &[("d.txt", "d\n")]);
    git(&tree.root, 2.8, &["switch", "-q", "other"]);
    git(
        &tree.root,
        2.8,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "Merge main into other",
            "main",
        ],
    );
    git(&tree.root, 3.2, &["switch", "-q", "main"]);
    git(
        &tree.root,
        3.2,
        &["merge", "-q", "--no-ff", "-m", "Merge other", "other"],
    );
    git(&tree.root, 3.4, &["switch", "-qc", "locks"]);
    commit(
        &tree,
        3.4,
        "lock bump",
        &[("Cargo.lock", &lines(11, "lock"))],
    );
    git(&tree.root, 3.5, &["switch", "-q", "main"]);
    git(
        &tree.root,
        3.5,
        &["merge", "-q", "--no-ff", "-m", "Merge locks", "locks"],
    );
    commit(
        &tree,
        3.75,
        "client: rebuild the distribution",
        &[("dist/app.js", &lines(101, "js"))],
    );
    tree
}

/// The line of first parents' read alone, its pathspecs those of `excluded`: what git itself
/// leaves out.
fn line(tree: &Tree, excluded: &[String]) -> Vec<Commit> {
    read(&tree.root, "main", midnight(), excluded).unwrap().line
}

/// A day file with its hours up to `until` hours after midnight.
fn day(until: f64) -> ReportDay {
    let start = midnight();
    ReportDay {
        start,
        until: start + until * HOUR,
        hours: (0..24u32)
            .take_while(|hour| *hour == 0 || f64::from(*hour) < until)
            .map(|hour| ReportHour {
                start: start + f64::from(hour) * HOUR,
                ..ReportHour::default()
            })
            .collect(),
        ..ReportDay::default()
    }
}

fn counts(day: &ReportDay) -> Vec<(u32, u32, u64, u64, u32)> {
    day.hours
        .iter()
        .map(|hour| {
            let git = hour.git.unwrap();
            (git.commits, git.merges, git.added, git.removed, git.reverts)
        })
        .collect()
}

#[test]
fn two_reads_count_commits_by_the_hour_they_reached_the_branch_and_lines_along_the_first_parents() {
    let tree = repository();
    let project = Project::of(&tree.root);
    let excluded = excluded(&project);
    assert_eq!(
        excluded[0], "dist/",
        "the generator's outputs, then the lock files"
    );
    assert_eq!(&excluded[1..], LOCKS.map(str::to_string));
    let read = read(&tree.root, "main", midnight(), &excluded).unwrap();
    let subjects: Vec<&str> = read.line.iter().map(|c| c.subject.as_str()).collect();
    assert_eq!(
        subjects,
        [
            "client: rebuild the distribution",
            "Merge locks",
            "Merge other",
            "tweak",
            "Merge feature",
            "docs: owner verdicts"
        ],
        "the line of first parents from the tip, a commit and a merge whose lines are all left out \
         kept, and nothing made before the window"
    );
    assert_eq!(
        read.all.len(),
        11,
        "every commit since the one the line starts from"
    );
    assert!(read.all.iter().all(|c| c.added == 0 && c.removed == 0));
    let unchanged: Vec<&str> = read
        .all
        .iter()
        .filter(|c| c.parents.len() < 2 && !c.changed)
        .map(|c| c.subject.as_str())
        .collect();
    assert_eq!(
        unchanged,
        ["client: rebuild the distribution", "lock bump"],
        "the commits that changed only what is left out"
    );
    let mut day = day(4.0);
    let from = GitRead {
        branch: "main".into(),
        excluded,
    };
    fill(&mut day, &read, from.clone());
    assert_eq!(
        counts(&day),
        [
            (1, 0, 2, 0, 0),
            (0, 0, 0, 0, 0),
            (3, 1, 6, 2, 1),
            (1, 2, 1, 0, 0),
        ],
        "00:30's commit and its two lines, the lock file's left out; nothing reached the branch \
         in hour 1; the branch's two commits, its revert among them, with their merge at 02:15 and \
         its lines, less the generated file, the lock files and the picture, and the tweak beside \
         them; the branch's merge of main counted as nothing, its own merge bringing c.txt alone, \
         and the lock file's branch and the generator's own commit a landing of no lines and no \
         commit"
    );
    assert_eq!(day.git, Some(from));
}

#[test]
fn a_branch_s_commit_made_the_day_before_counts_on_the_day_its_merge_landed() {
    let tree = Tree::new();
    git(&tree.root, -30.0, &["init", "-q", "-b", "main"]);
    commit(&tree, -30.0, "base", &[("a.txt", "a\n")]);
    git(&tree.root, -2.0, &["switch", "-qc", "late"]);
    commit(&tree, -2.0, "made at 22:00", &[("b.txt", "b\n")]);
    commit(&tree, -1.0, "Revert \"made at 22:00\"", &[("b.txt", "")]);
    git(&tree.root, 0.5, &["switch", "-q", "main"]);
    git(
        &tree.root,
        0.5,
        &["merge", "-q", "--no-ff", "-m", "Merge late", "late"],
    );
    let today = read(&tree.root, "main", midnight(), &[]).unwrap();
    let mut day = day(1.0);
    fill(&mut day, &today, GitRead::default());
    assert_eq!(
        counts(&day),
        [(2, 1, 0, 0, 1)],
        "both commits, made the day before, reached main with its merge at 00:30"
    );
    let yesterday = read(&tree.root, "main", midnight() - DAY, &[]).unwrap();
    let mut before = ReportDay {
        start: midnight() - DAY,
        until: midnight(),
        hours: (0..24u32)
            .map(|hour| ReportHour {
                start: midnight() - DAY + f64::from(hour) * HOUR,
                ..ReportHour::default()
            })
            .collect(),
        ..ReportDay::default()
    };
    fill(&mut before, &yesterday, GitRead::default());
    assert!(
        before.hours.iter().all(|h| h.git.unwrap().commits == 0),
        "the day they were made reached nothing"
    );
}

#[test]
fn git_itself_leaves_out_the_outputs_and_the_lock_files_at_any_depth() {
    let tree = repository();
    let lines = |excluded: &[String], subject: &str| {
        let line = line(&tree, excluded);
        let commit = line.iter().find(|c| c.subject == subject).unwrap();
        (commit.added, commit.removed)
    };
    let merge = "Merge feature";
    assert_eq!(
        lines(&[], merge),
        (112, 2),
        "b.txt, dist/app.js and crates/x/Cargo.lock"
    );
    assert_eq!(
        lines(&[], "docs: owner verdicts"),
        (12, 0),
        "a.txt and Cargo.lock"
    );
    let excluded = ["dist/".to_string(), "Cargo.lock".to_string()];
    assert_eq!(
        lines(&excluded, merge),
        (5, 2),
        "b.txt alone, no line left out after the read"
    );
    assert_eq!(
        lines(&excluded, "docs: owner verdicts"),
        (2, 0),
        "the top lock file too"
    );
    assert_eq!(
        pathspecs(&excluded),
        [
            ".",
            ":(exclude,literal)dist/",
            ":(exclude,glob)**/Cargo.lock"
        ]
    );
}

#[test]
fn the_hour_under_way_counts_up_to_the_read() {
    let tree = repository();
    let read = read(&tree.root, "main", midnight(), &[]).unwrap();
    let mut day = day(2.5);
    fill(&mut day, &read, GitRead::default());
    assert_eq!(
        counts(&day)[2],
        (2, 1, 112, 2, 1),
        "up to 02:30: the merge and the branch's two commits it brought, no tweak yet, nothing \
         left out but the picture"
    );
}

#[test]
fn a_window_with_nothing_on_the_line_reads_nothing_more() {
    let tree = repository();
    let read = read(&tree.root, "main", midnight() + DAY, &[]).unwrap();
    assert_eq!(read, Read::default());
    let mut day = day(1.0);
    fill(&mut day, &read, GitRead::default());
    assert_eq!(counts(&day), [(0, 0, 0, 0, 0)]);
}

#[test]
fn a_merge_counts_only_what_no_commit_before_it_on_the_line_brought() {
    let commit = |hash: &str, parents: &[&str], at: f64, subject: &str| Commit {
        hash: hash.into(),
        parents: parents.iter().map(|p| (*p).to_string()).collect(),
        at,
        subject: subject.into(),
        changed: true,
        ..Commit::default()
    };
    // m1 lands b1; b2 was branched after it, from m1, and lands with m2, whose walk reaches b1
    // again through m1, the line's own; c1 is the base, from before the window.
    let line = vec![
        commit("m2", &["m1", "b2"], 20.0, "Merge b2"),
        commit("m1", &["c1", "b1"], 10.0, "Merge b1"),
    ];
    let all = vec![
        line[0].clone(),
        commit("b2", &["m1"], 15.0, "Revert \"x\""),
        line[1].clone(),
        commit("b1", &["c1"], 5.0, "b1"),
    ];
    let reached = reached(&Read { line, all });
    let counted: Vec<(f64, bool, u32, u32)> = reached
        .iter()
        .map(|r| (r.at, r.merge, r.commits, r.reverts))
        .collect();
    assert_eq!(
        counted,
        [(10.0, true, 1, 0), (20.0, true, 1, 1)],
        "oldest first, each commit once, by the merge that brought it"
    );
}

#[test]
fn the_settings_marks_are_the_commits_on_the_line_that_changed_the_project_file() {
    let tree = repository();
    // A branch's change of the file lands by its merge, at 04:30; one on main itself at 05:00.
    git(&tree.root, 4.0, &["switch", "-qc", "gates"]);
    commit(
        &tree,
        4.0,
        "gates: a slower check",
        &[("symphony.toml", "[land]\ncheck = false\n")],
    );
    git(&tree.root, 4.5, &["switch", "-q", "main"]);
    git(
        &tree.root,
        4.5,
        &["merge", "-q", "--no-ff", "-m", "Merge gates", "gates"],
    );
    commit(
        &tree,
        5.0,
        "settings: limits",
        &[("symphony.toml", "[land]\ncheck = true\n")],
    );
    commit(&tree, 5.5, "elsewhere", &[("e.txt", "e\n")]);
    let marks: Vec<(f64, String)> = settings_changes(&tree.root, "main", midnight())
        .unwrap()
        .into_iter()
        .map(|commit| ((commit.at - midnight()) / HOUR, commit.subject))
        .collect();
    assert_eq!(
        marks,
        [
            (5.0, "settings: limits".to_string()),
            (4.5, "Merge gates".to_string())
        ],
        "tip first, the merge for the branch's commit, the file's first commit before the window \
         and every commit that left it alone out"
    );
    assert!(
        settings_changes(&tree.root, "main", midnight() + DAY)
            .unwrap()
            .is_empty()
    );
    let error = settings_changes(&tree.root, "trunk", midnight()).unwrap_err();
    assert!(error.contains("trunk"), "{error}");
}

#[test]
fn a_read_of_no_repository_or_no_branch_fails_with_why() {
    let tree = Tree::new();
    let error = read(&tree.root, "main", midnight(), &[]).unwrap_err();
    assert!(error.starts_with("git log of main failed: "), "{error}");
    let tree = repository();
    let error = read(&tree.root, "trunk", midnight(), &[]).unwrap_err();
    assert!(error.contains("trunk"), "{error}");
}

#[test]
fn a_record_reads_its_fields_and_lines_and_skips_what_does_not_read() {
    let text = "\x1eaaa\x1fbbb ccc\x1f100\x1fMerge x\n\n3\t1\tsrc/a.rs\n-\t-\tpic.png\n2\t2\tdist/x.js\n\
                odd line\nx\t1\tbad.rs\n\
                \x1eddd\x1f\x1f200\x1fRoot\n\
                \x1enot a record\n\
                \x1eeee\x1fddd\x1fnot a time\x1fBad\n";
    let commits = parse(text);
    assert_eq!(
        commits,
        [
            Commit {
                hash: "aaa".into(),
                parents: vec!["bbb".into(), "ccc".into()],
                at: 100.0,
                subject: "Merge x".into(),
                added: 5,
                removed: 3,
                changed: true,
            },
            Commit {
                hash: "ddd".into(),
                parents: Vec::new(),
                at: 200.0,
                subject: "Root".into(),
                ..Commit::default()
            },
        ],
        "a numstat line's counts, a binary file's and a name's none but a change, and a record \
         with no change"
    );
}
