//! The roll-up's days and its one read: yesterday's file while it is not final, then today's; the
//! board read once from the first of them; a failing read writes nothing; the documents and git
//! failing leave only their own figures out, and say so.

use super::*;
use crate::test_tree::Tree;
use symphony_api::ReportDay;

/// 08:00 UTC on a January day.
const NOW: f64 = 1_800_000_000.0;

fn today() -> f64 {
    day_start(NOW)
}

/// A repository at `tree`'s root with one card in its documents and one commit, made at 07:30.
fn repository(tree: &Tree) {
    #[allow(clippy::cast_possible_truncation)]
    let posted = crate::clock::stamp_at((today() + 7.0 * HOUR) as libc::time_t);
    tree.write(
        "docs/DECISIONS.md",
        &format!("# Decisions\n\n- [ ] **one** — Is this it?\n  [{posted}] agent — Posted.\n"),
    );
    let date = format!("@{} +0000", today() + 7.5 * HOUR);
    for args in [
        &["init", "-q", "-b", "main"][..],
        &["add", "-A"],
        &["commit", "-qm", "the first"],
    ] {
        let done = symphony_board::sys::output(
            symphony_board::sys::Command::new("git")
                .args([
                    "-c",
                    "user.name=Reports test",
                    "-c",
                    "user.email=r@example.invalid",
                ])
                .args(args)
                .current_dir(&tree.root)
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_AUTHOR_DATE", &date)
                .env("GIT_COMMITTER_DATE", &date),
        )
        .unwrap();
        assert!(done.status.success(), "{args:?}: {done:?}");
    }
}

fn written(folder: &Path, start: f64) -> ReportDay {
    symphony_board::reports::read(folder, &symphony_api::run_day(start))
        .unwrap()
        .unwrap()
}

#[test]
fn yesterday_is_due_until_its_file_is_final_and_today_always() {
    let tree = Tree::new();
    let folder = tree.path("reports");
    let yesterday = today() - DAY;
    assert_eq!(due(&folder, NOW), [yesterday, today()], "no file yet");
    let mut day = ReportDay {
        day: symphony_api::run_day(yesterday),
        ..ReportDay::default()
    };
    symphony_board::reports::write(&folder, &day).unwrap();
    assert_eq!(due(&folder, NOW), [yesterday, today()], "not final yet");
    day.closed = true;
    symphony_board::reports::write(&folder, &day).unwrap();
    assert_eq!(due(&folder, NOW), [today()]);
    tree.write(
        &format!("reports/{}", symphony_api::report_file(&day.day)),
        "not json",
    );
    assert_eq!(
        due(&folder, NOW),
        [yesterday, today()],
        "a file that does not read is written again"
    );
}

#[test]
fn one_read_from_yesterday_fills_both_files_with_the_board_documents_and_git() {
    let tree = Tree::new();
    repository(&tree);
    let folder = tree.path("reports");
    let mut asked = Vec::new();
    let rolled = roll_up(&tree.root, "sim", NOW, &folder, |since| {
        asked.push(since);
        Ok(ReportRead::default())
    })
    .unwrap();
    assert_eq!(asked, [today() - DAY], "one read, from yesterday's start");
    let names = [today() - DAY, today()].map(symphony_api::run_day);
    assert_eq!(
        rolled,
        Rolled {
            written: names.to_vec(),
            problems: Vec::new()
        }
    );
    let yesterday = written(&folder, today() - DAY);
    assert!(yesterday.closed && yesterday.project == "sim");
    let day = written(&folder, today());
    assert!(!day.closed && day.until == NOW);
    let git = day.hours[7].git.unwrap();
    assert_eq!(
        (git.commits, git.added),
        (1, 4),
        "07:30's commit and its lines"
    );
    assert_eq!(day.git.unwrap().branch, "main");
    let counts = day.hours[7].decisions.unwrap();
    assert_eq!(counts.open, 1, "the card posted at 07:00");
    // Yesterday ended before the card or the commit.
    assert_eq!(yesterday.hours[23].decisions.unwrap().open, 0);
    assert_eq!(yesterday.hours[23].git.unwrap().commits, 0);
}

#[test]
fn a_failing_board_read_writes_nothing_and_leaves_each_file_as_it_was() {
    let tree = Tree::new();
    repository(&tree);
    let folder = tree.path("reports");
    roll_up(&tree.root, "sim", NOW, &folder, |_| {
        Ok(ReportRead::default())
    })
    .unwrap();
    let file = folder.join(symphony_api::report_file(&symphony_api::run_day(today())));
    let before = std::fs::read(&file).unwrap();
    let error = roll_up(&tree.root, "sim", NOW + HOUR, &folder, |_| {
        Err("the board is down".into())
    })
    .unwrap_err();
    assert_eq!(error, "the board is down");
    assert_eq!(std::fs::read(&file).unwrap(), before);
    assert_eq!(
        std::fs::read_dir(&folder).unwrap().count(),
        2,
        "nothing left beside the two files"
    );
}

#[test]
fn without_git_the_files_are_written_with_no_git_figures_and_say_why() {
    let tree = Tree::new();
    tree.write("docs/README.md", "# Readme\n");
    let folder = tree.path("reports");
    let rolled = roll_up(&tree.root, "sim", NOW, &folder, |_| {
        Ok(ReportRead::default())
    })
    .unwrap();
    assert_eq!(rolled.written.len(), 2);
    assert_eq!(rolled.problems.len(), 1, "{:?}", rolled.problems);
    assert!(
        rolled.problems[0].starts_with("git: "),
        "{:?}",
        rolled.problems
    );
    let day = written(&folder, today());
    assert!(day.git.is_none() && day.hours.iter().all(|h| h.git.is_none()));
    assert!(day.decisions.is_some(), "the documents still count");
}

#[test]
fn a_folder_that_cannot_be_written_is_the_error() {
    let tree = Tree::new();
    repository(&tree);
    let folder = tree.write("reports", "a file where the folder goes");
    let error = roll_up(&tree.root, "sim", NOW, &folder, |_| {
        Ok(ReportRead::default())
    })
    .unwrap_err();
    assert!(error.contains("reports"), "{error}");
}
