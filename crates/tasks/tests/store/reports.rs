//! The roll-up's one read of a real board (`Store::report_records`): every task open now however
//! old, every one written since the window opened and none closed before it, top-level and
//! subtasks, each with its whole thread (a post by its sender and time alone), the red mark, and
//! the sessions that registered since; one request, and no write. Then the board's figures of the
//! day file from that read, on the scratch board the store itself wrote through its commands
//! ([`crate::common::report_board`]).

use crate::common::{NOW, report_board as board};
use symphony_board::board_health::queries_of;
use symphony_board::reports::{DAY, HOUR, day, day_start};
use symphony_board::sweep::trigger::now;

/// The window's first second: the UTC midnight before the fixture's clock.
fn midnight() -> f64 {
    day_start(NOW)
}

#[test]
fn one_request_reads_every_open_task_and_every_one_written_since_and_writes_nothing() {
    let f = board();
    let client = f.store.client();
    let log = queries_of(client.addr(), "sf", client.database());
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
    assert_eq!(
        read.sessions
            .iter()
            .map(|s| (s.kind.as_str(), s.model.as_str()))
            .collect::<Vec<_>>(),
        [("worker", "opus")],
        "only the session that registered since midnight"
    );
}

#[test]
fn the_day_file_of_the_board_counts_its_bounces_reds_verdicts_landings_and_waits() {
    let f = board();
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
        [("ready".to_string(), 1), ("review".to_string(), 1)].into(),
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
            ("sim/b".to_string(), 0.0),
            ("sim/b".to_string(), 0.0),
        ],
        "sim/b's review subtask claimed half an hour in, its reviewer on it since"
    );
    assert!(day.top.lead.is_some_and(|lead| lead.n == 2));
    assert!(!day.closed && day.until < start + DAY);
}
