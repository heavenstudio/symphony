//! The roll-up's one read of a real board (`Store::report_records`): every task open now however
//! old, every one written since the window opened or with a line since then, and none closed before
//! it with nothing said since, top-level and subtasks, each with its whole thread (a post by its
//! sender and time alone) and the red mark; one request, and no write. Then the board's figures of
//! the day file from that read, on the scratch board the store itself wrote through its commands
//! ([`crate::common::report_board`]).

use crate::common::{Fixture, NOW, Seed, report_board};
use symphony_board::NewTask;
use symphony_board::reports::{DAY, HOUR, day, day_start};
use symphony_board::sweep::trigger::now;

/// The window's first second: the UTC midnight before the fixture's clock.
fn midnight() -> f64 {
    day_start(NOW)
}

#[test]
fn one_request_reads_every_open_task_and_every_one_written_since_and_writes_nothing() {
    // Exercise the fixture that the server's roll-up tests share too.
    let f = report_board();
    let log = f.store.database().queries();
    let (before, writes_before) = log.report(now());
    let read = f.store.report_records(midnight()).unwrap();
    let (after, writes_after) = log.report(now());
    assert_eq!(after.count - before.count, 1, "one round trip");
    assert_eq!(writes_after.count, writes_before.count, "no write");
    let slugs: Vec<&str> = read.tasks.iter().map(|t| t.slug.as_str()).collect();
    assert_eq!(
        slugs,
        [
            "sim/old",
            "sim/a",
            "sim/b",
            "tests/main-red-1",
            "sim/b/review"
        ],
        "the open task made before the window is read, the one closed before it is not"
    );
    let task = |slug: &str| read.tasks.iter().find(|t| t.slug == slug).unwrap();
    assert!(task("tests/main-red-1").marked_red);
    assert!(!task("sim/a").marked_red);
    let review = task("sim/b/review");
    assert_eq!(
        (review.parent.as_deref(), review.instruction),
        (Some("sim/b"), true)
    );
    let posts: Vec<_> = task("sim/b")
        .lines
        .iter()
        .filter(|line| line.kind == "msg")
        .map(|line| (line.sender.as_str(), line.text.as_str(), line.ts / HOUR))
        .collect();
    let executor = posts.iter().find(|(by, ..)| *by != "board").unwrap().0;
    assert_eq!(
        posts,
        [
            ("board", "", midnight() / HOUR + 4.0),
            (executor, "", midnight() / HOUR + 5.0),
            ("board", "", midnight() / HOUR + 7.0),
        ],
        "a post is read by its sender and time alone: the board's word that no executor reads \
         each changes verdict, and the executor's own"
    );
    let landed = task("sim/a")
        .lines
        .iter()
        .find(|line| line.kind == "verdict" && line.text.starts_with("landed "))
        .unwrap();
    assert_eq!(
        landed.text.chars().count(),
        16,
        "a verdict is cut to its first words"
    );
}

#[test]
fn a_task_approved_before_the_window_and_landed_in_it_is_read_and_its_landing_counted() {
    let f = Fixture::new(Seed::default());
    f.at_hour(-3.0);
    let orch = f.session("orchestrator");
    let exe = f.session("worker");
    let rev = f.session("reviewer");
    f.task_with(NewTask::new("sim/late", "Work", &orch));
    f.task_with(NewTask::new("sim/before", "Work", &orch));
    f.store
        .take_role("sim/late", &orch, "orchestrator")
        .unwrap();
    f.store.claim("sim/late", &exe).unwrap();
    f.store.claim("sim/before", &exe).unwrap();
    f.store.move_to("sim/before", "cancelled", "owner").unwrap();
    f.at_hour(-1.0);
    f.store.move_to("sim/late", "review", &exe).unwrap();
    f.store.take_role("sim/late", &rev, "reviewer").unwrap();
    f.at_hour(-0.25);
    f.store
        .verdict("sim/late", &rev, "approve", "fine")
        .unwrap();
    f.at_hour(0.5);
    f.store
        .landed(
            "sim/late",
            &orch,
            "landed sim/late on main at 3e0c661",
            false,
            &["done"],
        )
        .unwrap();
    f.at_hour(1.5);
    let read = f.store.report_records(midnight()).unwrap();
    let slugs: Vec<&str> = read.tasks.iter().map(|t| t.slug.as_str()).collect();
    assert_eq!(
        slugs,
        ["sim/late"],
        "approved at 23:45 and landed at 00:30, its task not written since: read by its line; the \
         task closed before midnight with nothing said since is not"
    );
    let late = &read.tasks[0];
    assert!(
        late.updated < midnight(),
        "the landing left the task as it was"
    );
    let day = day("sim", midnight(), f.now(), f.now(), &read);
    assert_eq!(day.hours[0].landed, 1);
    let waited: Vec<(&str, f64)> = day.hours[0]
        .waits
        .approved_to_landed
        .iter()
        .map(|w| (w.slug.as_str(), w.seconds / HOUR))
        .collect();
    assert_eq!(waited, [("sim/late", 0.75)]);
}

/// A red's filer names the commits it came between in its body, and the read takes them back to
/// the red's day file; a red that names none, and a task not marked red whose body names some,
/// read none.
#[test]
fn a_red_s_body_names_the_commits_it_came_between_and_the_read_takes_them_to_its_day_file() {
    let f = Fixture::new(Seed::default());
    f.at_hour(1.0);
    let filer = f.session("filer");
    let orch = f.session("orchestrator");
    let range =
        "1a5037fc9e2b4d6a8c0e1f3a5b7c9d0e2f4a6b8c..fd70821942b4c97d1e17b420217ada28988e27da";
    let body = format!("The release run is red.\n\nbetween: {range}\nkey: verify\ncheck: verify");
    let red = |slug: &str, body: &str| {
        f.task_with(NewTask {
            red: Some("release".into()),
            body: body.into(),
            ..NewTask::new(slug, "Release red", &filer)
        });
    };
    red("tests/release-red-verify", &body);
    red(
        "tests/release-red-lint",
        "The release run is red.\n\nkey: lint",
    );
    f.task_with(NewTask {
        body: body.clone(),
        ..NewTask::new("sim/quoting", "Work", &orch)
    });
    f.at_hour(2.0);
    let read = f.store.report_records(midnight()).unwrap();
    let between: Vec<(&str, Option<&str>)> = (read.tasks.iter())
        .map(|task| (task.slug.as_str(), task.between.as_deref()))
        .collect();
    assert_eq!(
        between,
        [
            ("tests/release-red-verify", Some(range)),
            ("tests/release-red-lint", None),
            ("sim/quoting", None),
        ],
        "a body is read for a red alone"
    );
    let day = day("sim", midnight(), f.now(), f.now(), &read);
    let reds: Vec<(&str, Option<&str>)> = (day.reds.iter())
        .map(|red| (red.slug.as_str(), red.between.as_deref()))
        .collect();
    assert_eq!(
        reds,
        [
            ("tests/release-red-verify", Some(range)),
            ("tests/release-red-lint", None),
        ]
    );
    assert!(
        day.reds.iter().all(|red| red.landed.is_none()),
        "the landings in a range are the roll-up's to read from git"
    );
}

#[test]
fn the_day_file_of_the_board_counts_its_bounces_reds_verdicts_landings_and_waits() {
    let f = report_board();
    let read = f.store.report_records(midnight()).unwrap();
    let start = midnight();
    let day = day("sim", start, f.now(), f.now(), &read);
    assert_eq!(day.hours.len(), 10);
    let total =
        |pick: fn(&symphony_api::ReportHour) -> u32| -> u32 { day.hours.iter().map(pick).sum() };
    assert_eq!(total(|h| h.top.bounces), 2, "sim/b went back twice");
    assert_eq!(total(|h| h.landed), 1);
    assert_eq!((total(|h| h.reds_filed), total(|h| h.reds_closed)), (1, 1));
    assert_eq!(
        day.hours[9].top.held,
        symphony_api::reports::ByStatus {
            ready: 1,
            review: 1,
            ..Default::default()
        },
        "sim/old, made the day before, and sim/b"
    );
    let changes: u32 = day.rounds.iter().map(|r| r.changes).sum();
    let approved: u32 = day.rounds.iter().map(|r| r.approved).sum();
    assert_eq!((changes, approved), (2, 2));
    assert_eq!(day.rounds[0].number, 1);
    assert_eq!(day.rounds.last().unwrap().number, 2);
    let red = &day.reds[0];
    assert_eq!(
        (red.series.as_str(), red.status.as_str(), red.closed),
        ("main", "done", Some(start + 4.0 * HOUR))
    );
    let waits = |pick: fn(&symphony_api::reports::Waits) -> &Vec<symphony_api::reports::Wait>| {
        day.hours
            .iter()
            .flat_map(|h| pick(&h.waits))
            .map(|w| (w.slug.clone(), w.seconds / HOUR))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        waits(|w| &w.approved_to_landed),
        [("sim/a".to_string(), 0.5)]
    );
    assert_eq!(
        waits(|w| &w.changes_to_resumed),
        [("sim/b".to_string(), 1.0), ("sim/b".to_string(), 1.0)],
        "its post an hour after the first, its move to review an hour after the second"
    );
    assert_eq!(
        waits(|w| &w.changes_to_review),
        [("sim/b".to_string(), 2.0), ("sim/b".to_string(), 1.0)]
    );
    assert_eq!(
        waits(|w| &w.review_to_reviewer),
        [
            ("sim/a".to_string(), 0.5),
            ("sim/b".to_string(), 0.5),
            ("tests/main-red-1".to_string(), 0.5),
        ],
        "sim/b's review subtask claimed half an hour in; back in review twice with its reviewer \
         keeping the role, no wait the board's lines can tell"
    );
    assert!(day.top.lead.is_some_and(|lead| lead.n == 2));
    assert!(!day.closed && day.until < start + DAY);
}
