//! The roll-up's days and what it fills them with: yesterday's file while it is not final, then
//! today's; the board's read, the documents with a live branch's posted cards, and git; no days
//! write nothing; the documents and git failing leave only their own figures out, and say so.

use super::*;
use crate::branches::Tree as Worktree;
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

/// Roll the repository at `tree` up into `folder` at `now`, for the days due then, from an empty
/// board read and the live branches' `cards`.
fn roll(tree: &Tree, folder: &Path, now: f64, cards: &[BranchCard]) -> Result<Rolled, String> {
    let days = due(folder, now);
    roll_up(
        &tree.root,
        "sim",
        now,
        folder,
        &days,
        &ReportRead::default(),
        cards,
    )
}

#[test]
fn one_read_from_yesterday_fills_both_files_with_the_board_documents_and_git() {
    let tree = Tree::new();
    repository(&tree);
    let folder = tree.path("reports");
    let rolled = roll(&tree, &folder, NOW, &[]).unwrap();
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
fn a_live_branch_s_posted_card_waits_on_the_owner_beside_main_s_by_its_page() {
    let tree = Tree::new();
    repository(&tree);
    let folder = tree.path("reports");
    #[allow(clippy::cast_possible_truncation)]
    let stamp = |hours: f64| crate::clock::stamp_at((today() + hours * HOUR) as libc::time_t);
    let text = format!(
        "- [x] **two** — And this?\n  [{}] agent — Posted on the branch.\n  [{}] owner · \
         approved\n",
        stamp(6.0),
        stamp(7.75)
    );
    let lines: Vec<&str> = text.lines().collect();
    let card = BranchCard {
        tree: Worktree {
            branch: "tools/x".into(),
            path: tree.path("x"),
        },
        doc: "docs/DECISIONS.md".into(),
        card: crate::register::parse_decisions(&lines).remove(0),
        on_main: false,
    };
    roll(&tree, &folder, NOW, &[card]).unwrap();
    let day = written(&folder, today());
    let open = |hour: usize| day.hours[hour].decisions.unwrap().open;
    assert_eq!(
        [open(5), open(6), open(7)],
        [0, 1, 1],
        "the branch's card from 06:00, main's from 07:00, the branch's answered at 07:45"
    );
    assert_eq!(day.hours[7].decisions.unwrap().answered, 1);
    let decisions = day.decisions.unwrap();
    let answered: Vec<(&str, &str)> = decisions
        .answered
        .iter()
        .map(|a| (a.doc.as_str(), a.slug.as_str()))
        .collect();
    assert_eq!(answered, [("@tools~x/docs/DECISIONS.md", "two")]);
    assert_eq!(decisions.open[0].slug, "one");
}

#[test]
fn no_day_writes_nothing() {
    let tree = Tree::new();
    repository(&tree);
    let folder = tree.path("reports");
    let rolled = roll_up(
        &tree.root,
        "sim",
        NOW,
        &folder,
        &[],
        &ReportRead::default(),
        &[],
    )
    .unwrap();
    assert_eq!(rolled, Rolled::default());
    assert!(!folder.exists());
}

#[test]
fn without_git_the_files_are_written_with_no_git_figures_and_say_why() {
    let tree = Tree::new();
    tree.write("docs/README.md", "# Readme\n");
    let folder = tree.path("reports");
    let rolled = roll(&tree, &folder, NOW, &[]).unwrap();
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
    let error = roll(&tree, &folder, NOW, &[]).unwrap_err();
    assert!(error.contains("reports"), "{error}");
}
