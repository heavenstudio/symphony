//! The kept answers, by counts of what a read did ([`Kept::passes`], [`Kept::gits`]), never by
//! time: a second read with nothing moved reads no day file and starts no git; a day file written,
//! changed or appended to, an older roll-up file, a commit on the default branch and the hour each
//! have the answer read again, git started for a moved tip or a window reaching further back
//! alone; an answer whose marks or tip could not be read is not kept; a kept hit does not wait on
//! another window's read; and a hub keeps a few windows, the one read longest ago going first.

use super::super::tests::{MONDAY, folders, hour, report, wednesday, week};
use super::super::{DAY, HOUR, ReportsPage};
use super::*;
use crate::testing::{Tree, git, repository};
use std::path::PathBuf;
use test_time::wait::{FLOOR, until};

/// The fixture week's folders beside a repository whose default branch holds one commit.
fn project(tree: &Tree) -> (Folders, PathBuf) {
    week(tree);
    (folders(tree), repository(tree))
}

/// What `kept` has read so far: its passes over the day files and the gits it started.
fn read(kept: &Kept) -> (u64, u64) {
    (kept.passes(), kept.gits())
}

fn page_of(answer: &Answer) -> ReportsPage {
    serde_json::from_slice(&answer.body).unwrap()
}

/// An hour past this machine's clock: a commit a test makes falls in a day of hours that ends
/// after it, whichever minute of the hour the test runs in.
fn clock() -> f64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    now.as_secs_f64() + HOUR
}

#[test]
fn a_second_read_with_nothing_moved_reads_no_day_file_and_starts_no_git() {
    let tree = Tree::new();
    let (folders, root) = project(&tree);
    let kept = Kept::default();
    let first = kept.answer(&folders, &root, Grain::Day, 3, wednesday());
    assert_eq!(read(&kept), (1, 1));
    assert_eq!(
        page_of(&first),
        page(&folders, Grain::Day, 3, wednesday(), |_| Ok(Vec::new())),
        "the kept answer is the page"
    );
    assert_eq!(first.tag, checksum(&first.body), "tagged by its bytes");
    // Later in the same hour, as often as it is asked.
    for minutes in [1.0, 5.0, 29.0] {
        let again = kept.answer(&folders, &root, Grain::Day, 3, wednesday() + minutes * 60.0);
        assert_eq!((&again.body, &again.tag), (&first.body, &first.tag));
    }
    assert_eq!(read(&kept), (1, 1), "nothing was read again");
    // Another window is its own answer, its marks the kept ones: the first reached further back.
    let hours = kept.answer(&folders, &root, Grain::Hour, 1, wednesday());
    assert_eq!(page_of(&hours).buckets.len(), 24);
    assert_eq!(read(&kept), (2, 1));
    kept.answer(&folders, &root, Grain::Hour, 1, wednesday());
    kept.answer(&folders, &root, Grain::Day, 3, wednesday());
    assert_eq!(read(&kept), (2, 1), "both windows are kept");
}

#[test]
fn a_day_file_written_changed_or_appended_to_is_read_again_with_no_git() {
    let tree = Tree::new();
    let (folders, root) = project(&tree);
    let kept = Kept::default();
    let ask = || kept.answer(&folders, &root, Grain::Day, 3, wednesday());
    let first = ask();
    assert!(
        page_of(&first).buckets[1].flow.is_none(),
        "Tuesday: no file"
    );
    // A day's file that was not there.
    let tuesday = MONDAY + DAY;
    report(&tree, tuesday, vec![hour(tuesday + HOUR, 5, 0)], |_| {});
    let second = ask();
    assert_eq!(page_of(&second).buckets[1].flow.unwrap().landed, 5);
    assert_ne!(second.tag, first.tag);
    assert_eq!(read(&kept), (2, 1));
    // The same file written again, as the roll-up writes it each hour.
    report(
        &tree,
        tuesday,
        vec![hour(tuesday + HOUR, 5, 0), hour(tuesday + 2.0 * HOUR, 2, 0)],
        |_| {},
    );
    let third = ask();
    assert_eq!(page_of(&third).buckets[1].flow.unwrap().landed, 7);
    assert_eq!(read(&kept), (3, 1));
    // The agents' file appended to with a line that is no hour's: read again, the same bytes, so
    // the page's check is still a 304.
    let agents = tree.path(&format!("agents/{}.jsonl", run_day(MONDAY)));
    let mut text = std::fs::read_to_string(&agents).unwrap();
    text.push_str("\n{\"type\":\"change\",\"schema\":1,\"at\":2}\n");
    std::fs::write(&agents, text).unwrap();
    let fourth = ask();
    assert_eq!((&fourth.body, &fourth.tag), (&third.body, &third.tag));
    assert_eq!(read(&kept), (4, 1));
    // A file that goes.
    std::fs::remove_file(tree.path(&format!("health/{}.jsonl", run_day(MONDAY + 2.0 * DAY))))
        .unwrap();
    let fifth = ask();
    assert!(page_of(&fifth).buckets[2].health.is_none());
    assert_eq!(read(&kept), (5, 1));
    ask();
    assert_eq!(read(&kept), (5, 1), "and kept again once read");
}

#[test]
fn an_older_roll_up_file_moves_the_first_hour_of_an_answer_no_file_of_whose_window_moved() {
    let tree = Tree::new();
    let (folders, root) = project(&tree);
    let kept = Kept::default();
    let ask = || kept.answer(&folders, &root, Grain::Day, 3, wednesday());
    assert_eq!(page_of(&ask()).first, Some(MONDAY - 39.0 * DAY));
    report(&tree, MONDAY - 50.0 * DAY, vec![], |_| {});
    assert_eq!(page_of(&ask()).first, Some(MONDAY - 50.0 * DAY));
    assert_eq!(read(&kept), (2, 1));
    // A file whose name is no day's is none of the answer's.
    std::fs::write(tree.path("reports/more-notes.json"), "{}").unwrap();
    ask();
    assert_eq!(read(&kept), (2, 1));
}

#[test]
fn a_commit_on_the_default_branch_has_the_marks_read_again() {
    let tree = Tree::new();
    let (folders, root) = project(&tree);
    let kept = Kept::default();
    let now = clock();
    let ask = || kept.answer(&folders, &root, Grain::Hour, 1, now);
    let first = ask();
    assert_eq!(page_of(&first).marks, []);
    let commit = |file: &str, text: &str, subject: &str| {
        std::fs::write(root.join(file), text).unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-m", subject]);
    };
    // One that changes the settings file is a mark.
    commit(symphony::project::FILE, "# settings\n", "Settings begin");
    let second = ask();
    let marks = page_of(&second).marks;
    assert_eq!(marks.len(), 1, "{marks:?}");
    assert_eq!(marks[0].subject, "Settings begin");
    assert_eq!(marks[0].changed, ["settings"]);
    assert_ne!(second.tag, first.tag);
    assert_eq!(read(&kept), (2, 2));
    // One that changes another file moves the tip, so git is asked, and leaves the answer as it
    // was.
    commit("notes.txt", "notes\n", "Notes");
    let third = ask();
    assert_eq!((&third.body, &third.tag), (&second.body, &second.tag));
    assert_eq!(read(&kept), (3, 3));
    // One that changes a skill and the rules is a mark that names them, after the settings'.
    std::fs::create_dir_all(root.join(".agents/skills/work")).unwrap();
    std::fs::write(root.join("RULES.md"), "# Rules\n").unwrap();
    commit(".agents/skills/work/SKILL.md", "# work\n", "The work skill");
    let fourth = page_of(&ask()).marks;
    assert_eq!(
        fourth
            .iter()
            .map(|mark| (mark.subject.as_str(), mark.changed.join(", ")))
            .collect::<Vec<_>>(),
        [
            ("Settings begin", "settings".to_string()),
            ("The work skill", "rules, skill work".to_string())
        ]
    );
    assert_eq!(read(&kept), (4, 4));
    // A branch that is not the default one is none of it.
    git(&root, &["checkout", "-b", "side"]);
    commit(symphony::project::FILE, "# side\n", "Side settings");
    ask();
    assert_eq!(read(&kept), (4, 4));
}

#[test]
fn the_hour_reads_the_files_again_and_git_only_for_a_window_reaching_further_back() {
    let tree = Tree::new();
    let (folders, root) = project(&tree);
    let kept = Kept::default();
    let first = kept.answer(&folders, &root, Grain::Hour, 1, wednesday());
    // The next hour: the window moved on, its marks are among the kept ones.
    let next = kept.answer(&folders, &root, Grain::Hour, 1, wednesday() + HOUR);
    assert_eq!(page_of(&next).to, page_of(&first).to + HOUR);
    assert_eq!(read(&kept), (2, 1));
    // A window reaching further back than the marks kept asks git, and its marks then serve both.
    kept.answer(&folders, &root, Grain::Day, 30, wednesday() + HOUR);
    assert_eq!(read(&kept), (3, 2));
    kept.answer(&folders, &root, Grain::Hour, 1, wednesday() + 2.0 * HOUR);
    kept.answer(&folders, &root, Grain::Day, 30, wednesday() + 2.0 * HOUR);
    assert_eq!(read(&kept), (5, 2));
}

#[test]
fn an_answer_whose_marks_could_not_be_read_is_not_kept() {
    let tree = Tree::new();
    let (folders, root) = project(&tree);
    // The repository has no branch of the default one's name: git has nothing to read.
    git(&root, &["branch", "-m", "main", "trunk"]);
    let kept = Kept::default();
    let ask = || kept.answer(&folders, &root, Grain::Day, 3, wednesday());
    let first = page_of(&ask());
    assert!(
        first
            .unread
            .iter()
            .any(|line| line.starts_with("settings and instructions marks: ")),
        "{:?}",
        first.unread
    );
    assert!(first.buckets[0].flow.is_some(), "the files still read");
    ask();
    assert_eq!(read(&kept), (2, 2), "the next read tries again");
}

#[test]
fn an_answer_with_marks_but_no_tip_is_not_kept() {
    let tree = Tree::new();
    let (folders, root) = project(&tree);
    std::fs::write(root.join("RULES.md"), "# Rules\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "Rules begin"]);
    let kept = Kept::default();
    let now = clock();
    let ask = || kept.answer_with_tip(&folders, &root, Grain::Day, 3, now, |_, _| None);
    for reads in 1..=2 {
        let page = page_of(&ask());
        assert!(page.unread.is_empty(), "{:?}", page.unread);
        assert_eq!(page.marks.len(), 1);
        assert_eq!(page.marks[0].subject, "Rules begin");
        assert_eq!(read(&kept), (reads, reads), "no tip: try again");
    }
    // Once the tip can be observed, both the marks and the answer can be kept again.
    let found = kept.answer(&folders, &root, Grain::Day, 3, now);
    let again = kept.answer(&folders, &root, Grain::Day, 3, now);
    assert_eq!((&again.body, &again.tag), (&found.body, &found.tag));
    assert_eq!(read(&kept), (3, 3));
    // Losing the observation again must neither reuse the answer nor keep its replacement.
    for reads in 4..=5 {
        assert_eq!(ask().body, found.body);
        assert_eq!(read(&kept), (reads, reads));
    }
}

#[test]
fn a_kept_hit_is_answered_while_another_windows_read_is_held() {
    let tree = Tree::new();
    let (folders, root) = project(&tree);
    let kept = Kept::default();
    let first = kept.answer(&folders, &root, Grain::Day, 3, wednesday());
    std::thread::scope(|scope| {
        // Hold the marks read at its lock until the kept hit has answered. The miss has already
        // entered its read when the hit starts, so holding the windows lock across a read would
        // prevent the hit from answering until the backstop.
        let held = lock(&kept.marks);
        let miss = scope.spawn(|| kept.answer(&folders, &root, Grain::Hour, 1, wednesday()));
        until("the other window's read has begun", FLOOR, || {
            (kept.passes() == 2).then_some(())
        })
        .unwrap();
        let (sent, received) = crate::test_channel::channel();
        {
            let (kept, folders, root) = (&kept, &folders, &root);
            scope.spawn(move || {
                sent.send(kept.answer(folders, root, Grain::Day, 3, wednesday()))
                    .unwrap();
            });
        }
        let hit = until("the kept hit answers while the miss is held", FLOOR, || {
            received.try_recv().ok()
        });
        // Release and reap the miss even if the hit stalled.
        drop(held);
        let hit = hit.unwrap();
        assert_eq!((&hit.body, &hit.tag), (&first.body, &first.tag));
        assert_eq!(page_of(&miss.join().unwrap()).buckets.len(), 24);
    });
    assert_eq!(read(&kept), (2, 1), "the hit did no extra read");
}

#[test]
fn concurrent_readers_of_one_window_share_its_read() {
    let tree = Tree::new();
    let (folders, root) = project(&tree);
    let kept = Kept::default();
    let together = std::sync::Barrier::new(4);
    std::thread::scope(|scope| {
        let readers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    together.wait();
                    kept.answer(&folders, &root, Grain::Day, 3, wednesday())
                })
            })
            .collect();
        let answers: Vec<_> = readers
            .into_iter()
            .map(|reader| reader.join().unwrap())
            .collect();
        for answer in &answers[1..] {
            assert_eq!(
                (&answer.body, &answer.tag),
                (&answers[0].body, &answers[0].tag)
            );
        }
    });
    assert_eq!(read(&kept), (1, 1));
}

#[test]
fn a_hub_keeps_a_few_windows_the_one_read_longest_ago_going_first() {
    let tree = Tree::new();
    let (folders, root) = project(&tree);
    let kept = Kept::default();
    let ask = |days: u32| kept.answer(&folders, &root, Grain::Day, days, wednesday());
    let most = u32::try_from(WINDOWS).unwrap();
    for days in 1..=most {
        ask(days);
    }
    assert_eq!(kept.passes(), u64::from(most));
    // The first asked is asked again, so the second is the one read longest ago.
    ask(1);
    ask(most + 1);
    assert_eq!(kept.passes(), u64::from(most) + 1);
    ask(1);
    assert_eq!(
        kept.passes(),
        u64::from(most) + 1,
        "kept: it was read since"
    );
    ask(2);
    assert_eq!(kept.passes(), u64::from(most) + 2, "gone: read again");
}
