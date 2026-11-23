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
        &Gate::default(),
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

/// The register's line stamp `hours` after today's midnight.
fn stamp(hours: f64) -> String {
    #[allow(clippy::cast_possible_truncation)]
    crate::clock::stamp_at((today() + hours * HOUR) as libc::time_t)
}

/// The card `text` as the live branch `branch` of the repository at `tree` posted it in
/// `docs/DECISIONS.md`.
fn posted(tree: &Tree, branch: &str, text: &str) -> BranchCard {
    let lines: Vec<&str> = text.lines().collect();
    BranchCard {
        tree: Worktree {
            branch: branch.into(),
            path: tree.path(&branch.replace('/', "~")),
        },
        doc: "docs/DECISIONS.md".into(),
        card: crate::register::parse_decisions(&lines).remove(0),
        on_main: false,
    }
}

#[test]
fn a_live_branch_s_posted_card_waits_on_the_owner_beside_main_s_by_its_page() {
    let tree = Tree::new();
    repository(&tree);
    let folder = tree.path("reports");
    let text = format!(
        "- [x] **two** — And this?\n  [{}] agent — Posted on the branch.\n  [{}] owner · \
         approved\n",
        stamp(6.0),
        stamp(7.75)
    );
    roll(&tree, &folder, NOW, &[posted(&tree, "tools/x", &text)]).unwrap();
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
fn a_live_branch_s_copy_of_one_of_main_s_cards_is_counted_once_by_main_s_page() {
    let tree = Tree::new();
    repository(&tree);
    let folder = tree.path("reports");
    let main = format!(
        "- [ ] **one** — Is this it?\n  [{}] agent — Posted.\n",
        stamp(7.0)
    );
    let edited = format!(
        "- [x] **one** — Is this it, now?\n  [{}] agent — Posted.\n  [{}] owner · approved\n",
        stamp(7.0),
        stamp(7.75)
    );
    let new = format!(
        "- [x] **two** — And this?\n  [{}] agent — Posted on the branch.\n  [{}] owner · \
         approved\n",
        stamp(6.0),
        stamp(7.5)
    );
    let cards = [
        posted(&tree, "tools/x", &edited),
        posted(&tree, "tools/x", &new),
        posted(&tree, "tools/y", &main),
    ];
    roll(&tree, &folder, NOW, &cards).unwrap();
    let day = written(&folder, today());
    assert_eq!(
        day.hours[7].decisions.unwrap(),
        symphony_api::reports::DecisionCounts {
            open: 0,
            answered: 2,
            ..Default::default()
        },
        "each card once: tools/y's copy of main's says nothing main's does not"
    );
    let decisions = day.decisions.unwrap();
    let answered: Vec<(&str, &str)> = decisions
        .answered
        .iter()
        .map(|a| (a.doc.as_str(), a.slug.as_str()))
        .collect();
    assert_eq!(
        answered,
        [
            ("@tools~x/docs/DECISIONS.md", "two"),
            ("docs/DECISIONS.md", "one")
        ],
        "a card only the branch has is the branch's page's; its edit of main's card is main's"
    );
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
        &Gate::default(),
    )
    .unwrap();
    assert_eq!(rolled, Rolled::default());
    assert!(!folder.exists());
}

#[test]
fn a_gate_a_reload_holds_or_closed_writes_nothing_and_an_open_one_writes_each_day() {
    let tree = Tree::new();
    repository(&tree);
    let folder = tree.path("reports");
    let days = due(&folder, NOW);
    let roll = |gate: &Gate| {
        roll_up(
            &tree.root,
            "sim",
            NOW,
            &folder,
            &days,
            &ReportRead::default(),
            &[],
            gate,
        )
        .unwrap()
    };
    let held = Gate::default();
    held.hold();
    let closed = Gate::default();
    closed.close(NOW - 1.0);
    for (gate, why) in [
        (&held, "a replacement's before its hand-off"),
        (&closed, "closed"),
    ] {
        assert_eq!(roll(gate).written, Vec::<String>::new(), "{why}");
        assert!(!folder.exists(), "{why}");
    }
    held.open(NOW);
    assert_eq!(
        roll(&held).written.len(),
        2,
        "open once its hand-off commits"
    );
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

/// A board read whose one task is the red `slug`, filed at 06:00 and open, its body naming
/// `between`.
fn red_read(slug: &str, between: &str) -> ReportRead {
    ReportRead {
        workflow: false,
        places: None,
        tasks: vec![symphony_board::ReportTask {
            slug: slug.into(),
            status: "ready".into(),
            created: today() + 6.0 * HOUR,
            updated: today() + 6.0 * HOUR,
            parent: None,
            instruction: false,
            marked_red: true,
            between: Some(between.into()),
            kind: None,
            lines: Vec::new(),
        }],
        ..ReportRead::default()
    }
}

#[test]
fn a_red_that_names_its_commits_has_the_landings_between_them_and_one_git_cannot_read_says_why() {
    let tree = Tree::new();
    repository(&tree);
    let folder = tree.path("reports");
    let roll = |read: &ReportRead| {
        roll_up(
            &tree.root,
            "sim",
            NOW,
            &folder,
            &[today()],
            read,
            &[],
            &Gate::default(),
        )
        .unwrap()
    };
    let tip = git::read(&tree.root, "main", today(), &[]).unwrap().line[0]
        .hash
        .clone();
    let rolled = roll(&red_read(
        "tests/release-red-verify",
        &format!("{tip}..{tip}"),
    ));
    assert_eq!(rolled.problems, Vec::<String>::new());
    let red = &written(&folder, today()).reds[0];
    assert_eq!(
        (red.between.as_deref(), red.landed.as_deref()),
        (Some(format!("{tip}..{tip}").as_str()), Some(&[][..])),
        "red where it was green: no landing to trace it to"
    );
    let unknown = format!("{0}..{0}", "0".repeat(40));
    let rolled = roll(&red_read("tests/release-red-lint", &unknown));
    assert_eq!(rolled.written.len(), 1, "the file is written all the same");
    assert!(
        rolled.problems.len() == 1
            && rolled.problems[0].starts_with("git: tests/release-red-lint: "),
        "{:?}",
        rolled.problems
    );
    let day = written(&folder, today());
    assert_eq!(day.reds[0].landed, None);
    assert!(day.git.is_some(), "the branch's own figures still count");
}

#[test]
fn a_folder_that_cannot_be_written_is_the_error() {
    let tree = Tree::new();
    repository(&tree);
    let folder = tree.write("reports", "a file where the folder goes");
    let error = roll(&tree, &folder, NOW, &[]).unwrap_err();
    assert!(error.contains("reports"), "{error}");
}

/// A read whose `usage` table holds `n` input tokens of one executor.
fn usage_read(n: u64) -> ReportRead {
    ReportRead {
        usage: vec![symphony_api::reports::TokenSum {
            role: "executor".into(),
            model: "m".into(),
            speed: "standard".into(),
            input: n,
            calls: 1,
            ..Default::default()
        }],
        ..ReportRead::default()
    }
}

/// Each hour of the file of the day from `start`, by its hour of the day, with the input tokens
/// it tells; none for an hour that tells none.
fn inputs(folder: &Path, start: f64) -> Vec<(u32, Option<u64>)> {
    (written(folder, start).hours.iter())
        .zip(0..)
        .map(|(hour, at)| {
            let sum = (hour.tokens.as_ref()).map(|sums| sums.iter().map(|s| s.input).sum());
            (at, sum)
        })
        .collect()
}

#[test]
fn the_next_roll_up_shares_what_the_table_gained_among_the_hours_since_the_last() {
    let tree = Tree::new();
    repository(&tree);
    let folder = tree.path("reports");
    let roll_at = |now: f64, read: &ReportRead| {
        roll_up(
            &tree.root,
            "sim",
            now,
            &folder,
            &due(&folder, now),
            read,
            &[],
            &Gate::default(),
        )
        .unwrap()
    };
    // The first roll-up, at 22:00 the day before: it keeps the sums and tells no hour.
    let first = today() - 2.0 * HOUR;
    roll_at(first, &usage_read(1_000));
    let before = written(&folder, today() - DAY);
    assert_eq!(before.usage.as_ref().unwrap()[0].input, 1_000);
    assert!(before.hours.iter().all(|hour| hour.tokens.is_none()));
    // The next, at 01:00: 300 gained over three hours, a hundred an hour, across midnight.
    roll_at(today() + HOUR, &usage_read(1_300));
    let yesterday = inputs(&folder, today() - DAY);
    assert_eq!(
        &yesterday[20..],
        [(20, None), (21, None), (22, Some(100)), (23, Some(100))]
    );
    // A roll-up at 01:00 sharp has no hour from 01:00 yet.
    assert_eq!(inputs(&folder, today()), [(0, Some(100))]);
    // An hour later: what it gained goes to the hour from 01:00, added to what that hour held.
    roll_at(today() + 2.0 * HOUR, &usage_read(1_360));
    assert_eq!(inputs(&folder, today()), [(0, Some(100)), (1, Some(60))]);
    assert_eq!(
        written(&folder, today()).usage.unwrap()[0].input,
        1_360,
        "today's file keeps the sums for the next"
    );
}
