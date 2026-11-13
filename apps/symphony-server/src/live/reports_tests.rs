//! The shared server's hourly roll-up on a fake clock, the scratch board of the store's tests
//! (`report_board`) and a repository with a card and commits in that day: its first tick writes
//! yesterday's final file and today's, with the board's, the cards' and git's figures, a tick later
//! in the same hour writes nothing, an hour later only today's file changes, a tick that cannot
//! reach the board leaves both byte for byte, after midnight the day that ended is written once
//! more, final, and never again, and no tick writes to the board. A private server keeps no day
//! files.

use super::Hub;
use super::reports::Rollup;
use crate::testing::board::{Fixture, NOW, report_board};
use crate::testing::{Tree, repository};
use std::path::Path;
use std::sync::{Arc, Mutex};
use symphony_api::{ReportDay, ReportHour};
use symphony_board::board_health::queries_of;
use symphony_board::reports::{DAY, HOUR, day_start};
use symphony_board::sweep::trigger::now;

/// The hub's clock, as the test moves it.
#[derive(Clone)]
struct Clock(Arc<Mutex<f64>>);

impl Clock {
    fn set(&self, time: f64) {
        *self.0.lock().unwrap() = time;
    }
}

/// A hub of the repository at `root` that rolls up into `folder` on `clock`, as the shared one does.
fn shared(root: &Path, folder: &Path, clock: &Clock) -> Hub {
    let mut hub = Hub::new(root, &root.join("build/client"));
    *hub.reports.get_mut().unwrap() = Some(Rollup::new(folder.to_path_buf()));
    let clock = clock.clone();
    hub.clock = Box::new(move || *clock.0.lock().unwrap());
    hub
}

/// The bytes of the day file of the day from `start`; none when there is none.
fn bytes(folder: &Path, start: f64) -> Option<Vec<u8>> {
    std::fs::read(folder.join(symphony_api::report_file(&symphony_api::run_day(start)))).ok()
}

fn day(folder: &Path, start: f64) -> ReportDay {
    symphony_board::reports::read(folder, &symphony_api::run_day(start))
        .unwrap()
        .unwrap()
}

/// The UTC midnight before the board fixture's clock: the scratch board's day.
fn midnight() -> f64 {
    day_start(NOW)
}

/// Run git in `root` as of `hours` after [`midnight`].
fn git_at(root: &Path, hours: f64, args: &[&str]) {
    let date = format!("@{} +0000", midnight() + hours * HOUR);
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(args)
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_COMMON_DIR")
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date),
    )
    .unwrap();
    assert!(done.status.success(), "{args:?}: {done:?}");
}

/// The repository's history in the scratch board's day: a card posted at 01:00 and approved by the
/// owner at 03:00, committed on main at 02:00 with its two lines, and a branch's commit of five
/// lines at 03:00 merged at 04:30. The repository's own card has no dated line.
fn history(tree: &Tree, root: &Path) {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a second since the epoch is far inside a time_t"
    )]
    let stamp = |hours: f64| symphony::clock::stamp_at((midnight() + hours * HOUR) as libc::time_t);
    tree.write(
        "main/docs/DECISIONS.md",
        &format!(
            "# Decisions\n\n- [x] **pick** — Is this it?\n  [{}] agent — Posted.\n  [{}] owner · approved\n",
            stamp(1.0),
            stamp(3.0)
        ),
    );
    git_at(root, 2.0, &["add", "."]);
    git_at(root, 2.0, &["commit", "-qm", "docs: owner verdicts"]);
    git_at(root, 3.0, &["switch", "-qc", "work"]);
    tree.write("main/src.txt", "1\n2\n3\n4\n5\n");
    git_at(root, 3.0, &["add", "."]);
    git_at(root, 3.0, &["commit", "-qm", "work"]);
    git_at(root, 4.5, &["switch", "-q", "main"]);
    git_at(
        root,
        4.5,
        &["merge", "-q", "--no-ff", "-m", "Merge work", "work"],
    );
}

#[test]
fn the_roll_up_writes_today_each_hour_makes_yesterday_final_once_and_survives_a_failed_read() {
    let tree = Tree::new();
    let root = repository(&tree);
    history(&tree, &root);
    let folder = tree.path("reports");
    let fixture = report_board();
    let today = midnight();
    let yesterday = today - DAY;
    let ten = today + 10.0 * HOUR;
    let clock = Clock(Arc::new(Mutex::new(ten)));
    let hub = shared(&root, &folder, &clock);
    let client = fixture.store.client();
    let log = queries_of(client.addr(), "sf", client.database());
    let (requests, writes) = log.report(now());

    hub.reports_tick(Some(&fixture.store));
    let first = day(&folder, today);
    assert!(!first.closed && first.until == ten && first.project == "main");
    assert_eq!(first.hours.len(), 10, "10:00: the day's first ten hours");
    let total =
        |day: &ReportDay, pick: fn(&ReportHour) -> u32| -> u32 { day.hours.iter().map(pick).sum() };
    assert_eq!(total(&first, |h| h.top.bounces), 2, "sim/b went back twice");
    assert_eq!(total(&first, |h| h.landed), 1);
    assert_eq!(
        first.hours[9].top.held,
        [("ready".to_string(), 1), ("review".to_string(), 1)].into()
    );
    let decisions = |hour: usize| {
        let counts = first.hours[hour].decisions.unwrap();
        (counts.open, counts.answered, counts.unasked)
    };
    assert_eq!(
        [decisions(0), decisions(1), decisions(3)],
        [(1, 0, 0), (2, 0, 0), (1, 1, 0)],
        "the repository's card at every hour, `pick` from 01:00 until the owner's line at 03:00"
    );
    let git = |hour: usize| {
        let counts = first.hours[hour].git.unwrap();
        (counts.commits, counts.merges, counts.added)
    };
    assert_eq!(
        [git(2), git(3), git(4)],
        [(1, 0, 5), (1, 0, 0), (0, 1, 5)],
        "the commit on main at 02:00, the branch's at 03:00, its merge at 04:30"
    );
    let ended = day(&folder, yesterday);
    assert!(ended.closed && ended.until == today && ended.hours.len() == 24);
    let (yesterday_bytes, today_bytes) = (bytes(&folder, yesterday), bytes(&folder, today));

    clock.set(ten + 59.0 * 60.0);
    hub.reports_tick(Some(&fixture.store));
    assert_eq!(
        bytes(&folder, today),
        today_bytes,
        "the same hour: nothing written"
    );

    clock.set(ten + HOUR + 5.0);
    hub.reports_tick(Some(&fixture.store));
    let later = day(&folder, today);
    assert_eq!((later.until, later.hours.len()), (ten + HOUR + 5.0, 12));
    assert_eq!(
        bytes(&folder, yesterday),
        yesterday_bytes,
        "an hour later only today's file changes"
    );
    let today_bytes = bytes(&folder, today);

    clock.set(ten + 2.0 * HOUR);
    hub.reports_tick(None);
    assert_eq!(
        (bytes(&folder, yesterday), bytes(&folder, today)),
        (yesterday_bytes.clone(), today_bytes),
        "a tick that could not reach the board leaves each file as it was"
    );

    let tomorrow = today + DAY;
    clock.set(tomorrow + 30.0);
    hub.reports_tick(Some(&fixture.store));
    let finished = day(&folder, today);
    assert!(finished.closed && finished.until == tomorrow && finished.hours.len() == 24);
    assert!(!day(&folder, tomorrow).closed);
    assert_eq!(
        bytes(&folder, yesterday),
        yesterday_bytes,
        "the day before stays final"
    );
    let finished_bytes = bytes(&folder, today);

    clock.set(tomorrow + HOUR);
    hub.reports_tick(Some(&fixture.store));
    assert_eq!(
        bytes(&folder, today),
        finished_bytes,
        "after midnight yesterday's file is final"
    );
    // The scratch board's final day file, for a reader of the run (`--nocapture`).
    println!("{}", String::from_utf8_lossy(&finished_bytes.unwrap()));
    let (requests_after, writes_after) = log.report(now());
    assert_eq!(
        writes_after.count, writes.count,
        "no roll-up writes to the board"
    );
    assert_eq!(
        requests_after.count - requests.count,
        4,
        "one board request for each of the four hours that reached it"
    );
}

#[test]
fn a_roll_up_git_cannot_read_still_writes_the_board_s_figures_and_leaves_git_s_out() {
    let tree = Tree::new();
    let root = tree.path("plain");
    std::fs::create_dir_all(&root).unwrap();
    let folder = tree.path("reports");
    let fixture = report_board();
    let clock = Clock(Arc::new(Mutex::new(midnight() + 10.0 * HOUR)));
    let hub = shared(&root, &folder, &clock);

    hub.reports_tick(Some(&fixture.store));
    let today = day(&folder, midnight());
    assert_eq!(
        today.git, None,
        "a folder with no repository has no history to read"
    );
    assert!(today.hours.iter().all(|hour| hour.git.is_none()));
    assert_eq!(
        today.hours.iter().map(|hour| hour.landed).sum::<u32>(),
        1,
        "the board's figures are written all the same"
    );
}

#[test]
fn a_private_server_keeps_no_day_files_and_the_shared_one_keeps_them_in_the_common_dir() {
    let tree = Tree::new();
    let root = repository(&tree);
    let fixture = Fixture::empty();
    let start = |drain_chat: bool| {
        Hub::start_watching::<notify::NullWatcher>(
            &root,
            &root.join("build/client"),
            false,
            drain_chat,
            crate::backstop::PERIOD,
            |_| Err("no board in this test".into()),
        )
        .unwrap()
    };
    let private = start(false);
    assert!(private.reports.lock().unwrap().is_none());
    private.reports_tick(Some(&fixture.store));
    private.stop();
    let common = root.join(".git/symphony");
    assert!(
        !common.join("reports").exists(),
        "a private server writes no day file"
    );
    let shared = start(true);
    let folder = shared
        .reports
        .lock()
        .unwrap()
        .as_ref()
        .map(|rollup| rollup.folder.clone());
    shared.stop();
    let folder = folder.unwrap();
    assert_eq!(
        folder,
        common.join("reports"),
        "in the one folder of Symphony's state in the git common dir"
    );
}
