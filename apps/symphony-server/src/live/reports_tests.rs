//! The shared server's hourly roll-up on a fake clock, the scratch board of the store's tests
//! (`report_board`) and a repository with a card and commits in that day: its first tick writes
//! yesterday's final file and today's, with the board's, the cards' and git's figures, a tick later
//! in the same hour writes nothing, an hour later only today's file changes, a tick that cannot
//! reach the board leaves both byte for byte, after midnight the day that ended is written once
//! more, final, and never again, and no tick writes to the board. The tick hands the roll-up to a
//! thread of its own: an hour that comes while one runs waits for it, and a stop waits on none. A
//! private server keeps no day files. A failure is said once while it lasts.

use super::Hub;
use super::reports::{Rollup, say};
use crate::testing::board::{Fixture, NOW, report_board};
use crate::testing::{Tree, repository};
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use symphony_api::{ReportDay, ReportHour};
use symphony_board::recording::Gate;
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

/// A hub of the repository at `root` that rolls up into `folder` on `clock`, as the shared one does,
/// its server generation's gate not given yet.
fn ungated(root: &Path, folder: &Path, clock: &Clock) -> Hub {
    let mut hub = Hub::new(root, &root.join("build/client"));
    *hub.reports.get_mut().unwrap() = Some(Rollup::new(folder.to_path_buf(), None));
    let clock = clock.clone();
    hub.clock = Box::new(move || *clock.0.lock().unwrap());
    hub
}

/// [`ungated`] with the open gate of a generation no reload is handing over.
fn shared(root: &Path, folder: &Path, clock: &Clock) -> Hub {
    let hub = ungated(root, folder, clock);
    hub.gate_recorders(Arc::new(Gate::default()));
    hub
}

/// The hub's tick with the board `board`, then the roll-up it started, if any, waited for.
fn tick(hub: &Hub, board: Option<&symphony_board::Store>) {
    hub.reports_tick(board);
    if let Some(rollup) = hub.reports.lock().unwrap().as_mut() {
        rollup.settle();
    }
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
    let log = fixture.store.database().queries();
    let (requests, writes) = log.report(now());

    tick(&hub, Some(&fixture.store));
    let first = day(&folder, today);
    assert!(!first.closed && first.until == ten && first.project == "main");
    assert_eq!(first.hours.len(), 10, "10:00: the day's first ten hours");
    let total =
        |day: &ReportDay, pick: fn(&ReportHour) -> u32| -> u32 { day.hours.iter().map(pick).sum() };
    assert_eq!(total(&first, |h| h.top.bounces), 2, "sim/b went back twice");
    assert_eq!(total(&first, |h| h.landed), 1);
    assert_eq!(
        first.hours[9].top.held,
        symphony_api::reports::ByStatus {
            ready: 1,
            review: 1,
            ..Default::default()
        }
    );
    let decisions = |hour: usize| {
        let counts = first.hours[hour].decisions.unwrap();
        (
            counts.open,
            counts.answered,
            counts.auto_approved,
            counts.withdrawn,
        )
    };
    assert_eq!(
        [decisions(0), decisions(1), decisions(3)],
        [(1, 0, 0, 0), (2, 0, 0, 0), (1, 1, 0, 0)],
        "the repository's card at every hour, `pick` from 01:00 until the owner's line at 03:00"
    );
    let git = |hour: usize| {
        let counts = first.hours[hour].git.unwrap();
        (counts.commits, counts.merges, counts.added)
    };
    assert_eq!(
        [git(2), git(3), git(4)],
        [(1, 0, 5), (0, 0, 0), (1, 1, 5)],
        "the commit on main at 02:00; the branch's, made at 03:00, with its merge at 04:30"
    );
    let ended = day(&folder, yesterday);
    assert!(ended.closed && ended.until == today && ended.hours.len() == 24);
    let (yesterday_bytes, today_bytes) = (bytes(&folder, yesterday), bytes(&folder, today));

    clock.set(ten + 59.0 * 60.0);
    tick(&hub, Some(&fixture.store));
    assert_eq!(
        bytes(&folder, today),
        today_bytes,
        "the same hour: nothing written"
    );

    clock.set(ten + HOUR + 5.0);
    tick(&hub, Some(&fixture.store));
    let later = day(&folder, today);
    assert_eq!((later.until, later.hours.len()), (ten + HOUR + 5.0, 12));
    assert_eq!(
        bytes(&folder, yesterday),
        yesterday_bytes,
        "an hour later only today's file changes"
    );
    let today_bytes = bytes(&folder, today);

    clock.set(ten + 2.0 * HOUR);
    tick(&hub, None);
    assert_eq!(
        (bytes(&folder, yesterday), bytes(&folder, today)),
        (yesterday_bytes.clone(), today_bytes),
        "a tick that could not reach the board leaves each file as it was"
    );

    let tomorrow = today + DAY;
    clock.set(tomorrow + 30.0);
    tick(&hub, Some(&fixture.store));
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
    tick(&hub, Some(&fixture.store));
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

    tick(&hub, Some(&fixture.store));
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
    let start = |one_server: bool| {
        Hub::start_watching::<notify::NullWatcher>(
            &root,
            &root.join("build/client"),
            false,
            one_server,
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
        !common.join("reports").exists() && !common.join("runs/rollup").exists(),
        "a private server writes no day file, and rolls no runs up"
    );
    let shared = start(true);
    let (folder, runs) = shared
        .reports
        .lock()
        .unwrap()
        .as_ref()
        .map(|rollup| (rollup.folder.clone(), rollup.runs.clone()))
        .unzip();
    assert_eq!(
        runs.flatten(),
        Some(common.join("runs")),
        "the runs it rolls up are the recorder's, in the same folder"
    );
    // A roll-up that has not ended, as one stuck in git would be: the stop waits on none of it.
    let (release, held) = mpsc::channel::<()>();
    let stuck = std::thread::spawn(move || {
        let _ = held.recv();
    });
    shared.reports.lock().unwrap().as_mut().unwrap().hold(stuck);
    shared.stop();
    release.send(()).unwrap();
    shared.reports.lock().unwrap().as_mut().unwrap().settle();
    let folder = folder.unwrap();
    assert_eq!(
        folder,
        common.join("reports"),
        "in the one folder of Symphony's state in the git common dir"
    );
}

#[test]
fn an_hour_that_comes_while_a_roll_up_runs_waits_for_it_and_then_rolls_up() {
    let tree = Tree::new();
    let root = repository(&tree);
    let folder = tree.path("reports");
    let fixture = report_board();
    let clock = Clock(Arc::new(Mutex::new(midnight() + 10.0 * HOUR)));
    let hub = shared(&root, &folder, &clock);
    let log = fixture.store.database().queries();
    let (requests, _) = log.report(now());
    let (release, held) = mpsc::channel::<()>();
    let running = std::thread::spawn(move || {
        let _ = held.recv();
    });
    hub.reports.lock().unwrap().as_mut().unwrap().hold(running);

    hub.reports_tick(Some(&fixture.store));
    hub.reports_tick(Some(&fixture.store));
    assert!(
        !folder.exists(),
        "no roll-up starts while the last one runs"
    );
    let (waiting, _) = log.report(now());
    assert_eq!(waiting.count, requests.count, "nor reads the board");

    release.send(()).unwrap();
    hub.reports.lock().unwrap().as_mut().unwrap().settle();
    tick(&hub, Some(&fixture.store));
    assert!(
        bytes(&folder, midnight()).is_some(),
        "the hour's roll-up, once the last one ended"
    );
}

#[test]
fn a_day_file_that_cannot_be_written_is_said_and_the_next_hour_writes_it() {
    let tree = Tree::new();
    let root = repository(&tree);
    let folder = tree.path("reports");
    std::fs::write(&folder, "a file where the folder goes").unwrap();
    let fixture = report_board();
    let clock = Clock(Arc::new(Mutex::new(midnight() + 10.0 * HOUR)));
    let hub = shared(&root, &folder, &clock);
    let said = || hub.reports.lock().unwrap().as_ref().unwrap().said();

    tick(&hub, Some(&fixture.store));
    let why = said().unwrap();
    assert!(
        why.starts_with("a day file could not be written (")
            && why.ends_with("); the next hour tries again"),
        "{why}"
    );
    std::fs::remove_file(&folder).unwrap();
    tick(&hub, Some(&fixture.store));
    assert_eq!(
        bytes(&folder, midnight()),
        None,
        "not again in the same hour"
    );
    clock.set(midnight() + 11.0 * HOUR);
    tick(&hub, Some(&fixture.store));
    assert!(
        bytes(&folder, midnight()).is_some(),
        "the next hour writes it"
    );
    assert_eq!(said(), None, "right again");
}

#[test]
fn the_roll_up_keeps_to_its_generation_s_gate_as_a_reload_moves_it() {
    let tree = Tree::new();
    let root = repository(&tree);
    let folder = tree.path("reports");
    let fixture = report_board();
    let clock = Clock(Arc::new(Mutex::new(midnight() + 10.0 * HOUR)));
    let hub = ungated(&root, &folder, &clock);
    tick(&hub, Some(&fixture.store));
    assert!(!folder.exists(), "no gate given yet: nothing is recorded");
    let gate = Arc::new(Gate::default());
    gate.hold();
    hub.gate_recorders(Arc::clone(&gate));
    tick(&hub, Some(&fixture.store));
    assert!(
        !folder.exists(),
        "a replacement's, before its hand-off commits"
    );
    gate.open(midnight() + 10.5 * HOUR);
    tick(&hub, Some(&fixture.store));
    let today = bytes(&folder, midnight());
    assert!(
        today.is_some(),
        "the hour rolled up once the hand-off committed, in the same hour"
    );
    clock.set(midnight() + 11.0 * HOUR);
    gate.close(midnight() + 11.0 * HOUR - 1.0);
    tick(&hub, Some(&fixture.store));
    assert_eq!(
        bytes(&folder, midnight()),
        today,
        "the generation a reload replaced writes nothing more"
    );
}

/// A day file of runs in `runs` for the day from `start`, holding one green run of the task `task`
/// that ended at `ended` after 60 s and `cpu` CPU seconds.
fn run_day_file(runs: &Path, start: f64, task: &str, ended: f64, cpu: f64) {
    let line = serde_json::json!({
        "schema": 1, "run": format!("{task}-run"), "pipeline": "test", "label": "just test-rs",
        "trigger": "owner", "task": task, "queued": ended - 60.0, "started": ended - 60.0,
        "ended": ended, "end": "green", "exit": 0, "cpu_s": cpu,
    });
    std::fs::create_dir_all(runs).unwrap();
    std::fs::write(
        runs.join(format!("{}.jsonl", symphony_api::run_day(start))),
        format!("{line}\n"),
    )
    .unwrap();
}

#[test]
fn the_runs_roll_up_each_hour_beside_their_day_files_whether_the_board_is_reached_or_not() {
    let tree = Tree::new();
    let root = repository(&tree);
    let folder = tree.path("reports");
    let runs = tree.path("runs");
    let today = midnight();
    run_day_file(&runs, today - DAY, "old", today - HOUR, 5.0);
    run_day_file(&runs, today, "new", today + HOUR, 3.0);
    let clock = Clock(Arc::new(Mutex::new(today + 10.0 * HOUR)));
    let hub = shared(&root, &folder, &clock);
    hub.reports.lock().unwrap().as_mut().unwrap().runs = Some(runs.clone());
    let said = || hub.reports.lock().unwrap().as_ref().unwrap().said_runs();
    let rolled_up = |start: f64| {
        std::fs::read(
            runs.join("rollup")
                .join(symphony_api::report_file(&symphony_api::run_day(start))),
        )
        .ok()
    };
    let day = |start: f64| -> symphony_api::RunsDay {
        serde_json::from_slice(&rolled_up(start).unwrap()).unwrap()
    };

    tick(&hub, None);
    assert_eq!(bytes(&folder, today), None, "no board: no board day file");
    let (ended, under_way) = (day(today - DAY), day(today));
    assert!(ended.closed && !under_way.closed && under_way.project == "main");
    assert_eq!(
        (ended.cost.cpu_s, under_way.cost.cpu_s),
        (5.0, 3.0),
        "each day's runs, yesterday's made final"
    );
    assert_eq!(under_way.tasks[0].task, "new");
    assert_eq!(said(), None);
    let before = (rolled_up(today - DAY), rolled_up(today));

    // Today's day file is a folder: no roll-up of runs written, said once; the board's go on.
    let file = runs.join(format!("{}.jsonl", symphony_api::run_day(today)));
    let aside = runs.join("aside");
    std::fs::rename(&file, &aside).unwrap();
    std::fs::create_dir(&file).unwrap();
    clock.set(today + 11.0 * HOUR);
    let fixture = report_board();
    tick(&hub, Some(&fixture.store));
    let why = said().unwrap();
    assert!(
        why.starts_with("runs: no roll-up written (")
            && why.ends_with("); the last ones stay, and the next hour tries again"),
        "{why}"
    );
    assert_eq!((rolled_up(today - DAY), rolled_up(today)), before);
    assert!(
        bytes(&folder, today).is_some(),
        "the board's figures are written all the same"
    );

    std::fs::remove_dir(&file).unwrap();
    std::fs::rename(&aside, &file).unwrap();
    clock.set(today + 12.0 * HOUR);
    tick(&hub, None);
    assert_eq!(said(), None, "right again");
    assert_eq!(
        day(today).until,
        today + 12.0 * HOUR,
        "today's written again"
    );
    assert_eq!(rolled_up(today - DAY), before.0, "yesterday's stays final");
}

#[test]
fn a_failure_is_said_once_while_it_lasts_and_again_once_it_comes_back() {
    let said = Mutex::new(None);
    assert!(say(&said, Some("the board could not be reached".into())));
    assert!(!say(&said, Some("the board could not be reached".into())));
    assert!(say(&said, Some("git: no repository".into())), "another");
    assert!(!say(&said, None), "right again: nothing to say");
    assert!(say(&said, Some("git: no repository".into())), "back");
}
