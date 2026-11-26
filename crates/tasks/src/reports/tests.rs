//! The board's figures of a day file, over threads written out by hand: each expected figure is
//! counted here from the lines, hour by hour.

use super::*;
use std::collections::BTreeMap;

/// A UTC midnight: the day every case reads.
const D: f64 = 1_799_971_200.0;
const H: f64 = 3_600.0;

fn at(hours: f64) -> f64 {
    D + hours * H
}

/// A thread's lines, numbered as written.
#[derive(Default)]
struct Thread(Vec<Line>);

impl Thread {
    fn line(mut self, kind: &str, hours: f64, by: &str, text: &str) -> Thread {
        let id = i64::try_from(self.0.len()).unwrap() + 1;
        self.0.push(Line {
            id,
            kind: kind.into(),
            text: text.into(),
            ts: at(hours),
            sender: by.into(),
        });
        self
    }

    fn made(self, hours: f64, by: &str) -> Thread {
        self.line("system", hours, by, &format!("created by {by}"))
    }

    fn claim(self, hours: f64, by: &str) -> Thread {
        self.line("system", hours, by, &format!("{by} claimed it"))
    }

    fn moved(self, hours: f64, by: &str, from: &str, to: &str) -> Thread {
        self.line("system", hours, by, &stages::moved_line(by, from, to))
    }

    fn joined(self, hours: f64, by: &str) -> Thread {
        self.line("system", hours, by, &format!("{by} joined as reviewer"))
    }

    fn left(self, hours: f64, by: &str) -> Thread {
        self.line(
            "system",
            hours,
            by,
            &format!("{by} released every role (left)"),
        )
    }

    fn verdict(self, hours: f64, by: &str, text: &str) -> Thread {
        self.line("verdict", hours, by, text)
    }

    fn post(self, hours: f64, by: &str) -> Thread {
        self.line("msg", hours, by, "")
    }
}

/// A task made `created` hours from midnight, `status` now and last written `updated` hours in.
fn task(slug: &str, created: f64, status: &str, updated: f64, thread: Thread) -> ReportTask {
    ReportTask {
        slug: slug.into(),
        status: status.into(),
        created: at(created),
        updated: at(updated),
        parent: None,
        instruction: false,
        marked_red: false,
        between: None,
        kind: None,
        lines: thread.0,
    }
}

/// A figure for each status `pairs` names, zero for the rest.
fn by_status<T: Copy + Default>(pairs: &[(&str, T)]) -> ByStatus<T> {
    let mut figures = ByStatus::default();
    for (status, figure) in pairs {
        *figures.get_mut(status).unwrap() = *figure;
    }
    figures
}

fn counts(pairs: &[(&str, u32)]) -> ByStatus<u32> {
    by_status(pairs)
}

fn waits(all: &[Wait]) -> Vec<(&str, f64)> {
    all.iter()
        .map(|w| (w.slug.as_str(), w.seconds / H))
        .collect()
}

/// The fixture the brief asks for: tasks across statuses, one bounced twice, a red filed and
/// closed, two `changes` verdicts, open tasks made before the window and a review subtask, read
/// 13.5 hours after midnight.
fn board() -> ReportRead {
    let (orch, exe, rev) = ("orch", "exe", "rev");
    let old = task(
        "sim/old",
        -48.0,
        "ready",
        -48.0,
        Thread::default().made(-48.0, orch),
    );
    let mut release = task(
        "tests/release-red-2",
        -24.0,
        "ready",
        -24.0,
        Thread::default().made(-24.0, "filer"),
    );
    release.marked_red = true;
    release.between = Some("0123456..89abcde".into());
    let a = task(
        "sim/a",
        1.0,
        "done",
        4.5,
        Thread::default()
            .made(1.0, orch)
            .claim(2.0, exe)
            .moved(3.0, exe, "in_progress", "review")
            .joined(3.5, rev)
            .verdict(4.0, rev, "approve: fine")
            .verdict(4.5, orch, "landed sim/a on m"),
    );
    let b = task(
        "sim/b",
        1.0,
        "done",
        10.0,
        Thread::default()
            .made(1.0, orch)
            .claim(2.0, exe)
            .moved(3.0, exe, "in_progress", "review")
            .joined(3.0, rev)
            .verdict(5.0, rev, "changes: a test i")
            .post(6.0, exe)
            .moved(7.0, exe, "in_progress", "review")
            .verdict(8.0, rev, "changes")
            .post(8.25, exe)
            .moved(9.0, exe, "in_progress", "review")
            .verdict(10.0, rev, "approve"),
    );
    let c = task(
        "sim/c",
        1.0,
        "review",
        11.0,
        Thread::default()
            .made(1.0, orch)
            .claim(1.5, exe)
            .moved(11.0, exe, "in_progress", "review"),
    );
    let mut review = task(
        "sim/c/review",
        11.0,
        "in_progress",
        12.0,
        Thread::default().made(11.0, orch).claim(12.0, "rev2"),
    );
    review.parent = Some("sim/c".into());
    review.instruction = true;
    let main_red = task(
        "tests/main-red-1",
        2.0,
        "done",
        6.0,
        Thread::default().made(2.0, "filer").claim(2.5, exe).moved(
            6.0,
            "owner",
            "in_progress",
            "done",
        ),
    );
    ReportRead {
        workflow: false,
        places: None,
        tasks: vec![old, release, a, b, c, main_red, review],
        ..ReportRead::default()
    }
}

#[test]
fn a_day_under_way_counts_each_figure_of_the_fixture_hour_by_hour() {
    let day = day("sim", D, at(13.5), at(13.5), &board());
    assert_eq!(day.day, symphony_api::run_day(D));
    assert_eq!((day.start, day.until, day.read), (D, at(13.5), at(13.5)));
    assert!(!day.closed);
    assert_eq!(day.hours.len(), 14, "midnight to the hour under way");
    assert_eq!(
        day.hours.iter().map(|h| h.start).collect::<Vec<_>>(),
        (0..14).map(|n| at(f64::from(n))).collect::<Vec<_>>(),
        "each hour by its first second"
    );
    let hour = |n: usize| &day.hours[n];
    assert_eq!(hour(0).top.held, counts(&[("ready", 2)]));
    assert_eq!(hour(0).top.task_hours, by_status(&[("ready", 2.0)]));
    assert_eq!(hour(0).top.unclaimed_hours, 2.0);
    assert_eq!(
        hour(1).top.entered,
        counts(&[("ready", 3), ("in_progress", 1)])
    );
    assert_eq!(
        hour(1).top.held,
        counts(&[("ready", 4), ("in_progress", 1)]),
        "a claim at the hour's end is the next hour's"
    );
    assert_eq!(
        hour(2).top.entered,
        counts(&[("ready", 1), ("in_progress", 3)])
    );
    assert_eq!(hour(2).reds_filed, 1);
    assert_eq!(
        hour(3).top.held,
        counts(&[("ready", 2), ("in_progress", 2), ("review", 2)])
    );
    assert_eq!(
        waits(&hour(3).waits.review_to_reviewer),
        [("sim/a", 0.5), ("sim/b", 0.0)],
        "a reviewer joining in the move's second staffs it at once"
    );
    for hour in 7..=10 {
        assert!(
            day.hours[hour].waits.review_to_reviewer.is_empty(),
            "sim/b's re-reviews: its reviewer kept the role, so nothing tells when it came back, \
             neither the move nor its verdict: hour {hour}"
        );
    }
    assert_eq!(hour(4).top.entered, counts(&[("done", 1)]));
    assert_eq!(hour(4).landed, 1);
    assert_eq!(waits(&hour(4).waits.approved_to_landed), [("sim/a", 0.5)]);
    assert_eq!(hour(5).top.bounces, 1);
    assert_eq!(hour(5).top.entered, counts(&[("in_progress", 1)]));
    assert_eq!(waits(&hour(6).waits.changes_to_resumed), [("sim/b", 1.0)]);
    assert_eq!(hour(6).reds_closed, 1);
    assert_eq!(waits(&hour(7).waits.changes_to_review), [("sim/b", 2.0)]);
    assert_eq!(hour(8).top.bounces, 1);
    assert_eq!(waits(&hour(8).waits.changes_to_resumed), [("sim/b", 0.25)]);
    assert_eq!(waits(&hour(9).waits.changes_to_review), [("sim/b", 1.0)]);
    assert_eq!(hour(11).subtasks.entered, counts(&[("ready", 1)]));
    assert_eq!(waits(&hour(12).waits.review_to_reviewer), [("sim/c", 1.0)]);
    assert_eq!(hour(12).subtasks.entered, counts(&[("in_progress", 1)]));
    assert_eq!(hour(13).top.held, counts(&[("ready", 2), ("review", 1)]));
    assert_eq!(hour(13).subtasks.held, counts(&[("in_progress", 1)]));
    assert_eq!(
        hour(13).top.task_hours,
        by_status(&[("ready", 1.0), ("review", 0.5)]),
        "the hour under way counts up to the read"
    );
    let bounces: u32 = day.hours.iter().map(|h| h.top.bounces).sum();
    assert_eq!(bounces, 2, "sim/b bounced twice");
    let landed: u32 = day.hours.iter().map(|h| h.landed).sum();
    assert_eq!(landed, 1);

    let lead = day.top.lead.unwrap();
    assert_eq!((lead.n, lead.median / H, lead.p90 / H), (3, 4.0, 8.0));
    let done: Vec<(usize, u32)> = (day.hours.iter().enumerate())
        .filter_map(|(n, h)| h.top.done.filter(|done| *done > 0).map(|done| (n, done)))
        .collect();
    assert_eq!(
        done,
        [(4, 1), (6, 1), (10, 1)],
        "sim/a, the main red and sim/b, each at its close: the tasks the lead time counts"
    );
    assert_eq!(
        day.top
            .longest
            .iter()
            .map(|s| (s.slug.as_str(), s.status.as_str(), (s.since - D) / H))
            .collect::<Vec<_>>(),
        [
            ("sim/old", "ready", -48.0),
            ("tests/release-red-2", "ready", -24.0),
            ("sim/c", "review", 11.0)
        ]
    );
    assert_eq!(day.subtasks.lead, None);
    assert_eq!(
        day.rounds,
        [
            Round {
                number: 1,
                changes: 1,
                approved: 1
            },
            Round {
                number: 2,
                changes: 1,
                approved: 0
            },
            Round {
                number: 3,
                changes: 0,
                approved: 1
            },
        ]
    );
    assert_eq!(
        day.reds,
        [
            Red {
                slug: "tests/release-red-2".into(),
                series: "release".into(),
                filed: at(-24.0),
                closed: None,
                status: "ready".into(),
                between: Some("0123456..89abcde".into()),
                landed: None,
            },
            Red {
                slug: "tests/main-red-1".into(),
                series: "main".into(),
                filed: at(2.0),
                closed: Some(at(6.0)),
                status: "done".into(),
                between: None,
                landed: None,
            },
        ]
    );
    assert_eq!((day.decisions.clone(), day.git.clone()), (None, None));
    assert!(
        day.hours
            .iter()
            .all(|h| h.decisions.is_none() && h.git.is_none())
    );
}

#[test]
fn a_day_read_after_it_ended_is_closed_with_all_its_hours_and_nothing_after_it() {
    let read = board();
    let day = day("sim", D - DAY, D + 5.0 * H, D + 5.0 * H, &read);
    assert!(day.closed);
    assert_eq!(day.until, D, "the figures stop at the day's end");
    assert_eq!(day.hours.len(), 24);
    assert_eq!(day.hours[23].top.held, counts(&[("ready", 2)]));
    let entered: u32 = day
        .hours
        .iter()
        .flat_map(|h| h.top.entered.each().map(|(_, n)| *n))
        .sum();
    assert_eq!(
        entered, 1,
        "the release red made in the day; nothing of the next day"
    );
    assert_eq!(day.reds.len(), 1, "the main red was filed the next day");
    assert!(day.rounds.is_empty());
    assert_eq!(day.top.lead, None);
}

#[test]
fn a_read_at_midnight_has_its_first_hour_and_counts_nothing_yet() {
    let day = day("sim", D, D, D, &board());
    assert_eq!(day.hours.len(), 1);
    assert_eq!(day.hours[0].top.held, counts(&[("ready", 2)]));
    assert_eq!(day.hours[0].top.entered, ByStatus::default());
    assert_eq!(day.hours[0].top.unclaimed_hours, 0.0);
}

#[test]
fn a_landing_with_no_approve_before_it_is_a_landing_and_no_wait() {
    let read = ReportRead {
        workflow: false,
        places: None,
        tasks: vec![task(
            "sim/x",
            1.0,
            "done",
            2.0,
            Thread::default()
                .made(1.0, "o")
                .claim(1.0, "e")
                .verdict(2.0, "o", "landed sim/x on m"),
        )],
        ..ReportRead::default()
    };
    let day = day("sim", D, at(3.0), at(3.0), &read);
    assert_eq!(day.hours[2].landed, 1);
    assert!(day.hours[2].waits.approved_to_landed.is_empty());
    assert_eq!(day.hours[2].top.entered, counts(&[("done", 1)]));
}

#[test]
fn an_executor_that_left_resumes_by_the_next_claim_and_its_release_is_not_resuming() {
    let read = ReportRead {
        workflow: false,
        places: None,
        tasks: vec![task(
            "sim/y",
            0.0,
            "in_progress",
            5.0,
            Thread::default()
                .made(0.0, "o")
                .claim(0.0, "e")
                .moved(1.0, "e", "in_progress", "review")
                .joined(1.0, "r")
                .verdict(2.0, "r", "changes: no")
                .post(2.5, "r")
                .left(3.0, "e")
                .claim(5.0, "e2"),
        )],
        ..ReportRead::default()
    };
    let day = day("sim", D, at(6.0), at(6.0), &read);
    assert_eq!(
        waits(&day.hours[5].waits.changes_to_resumed),
        [("sim/y", 3.0)],
        "the reviewer's post and the executor's leaving are not its work"
    );
    assert!(day.hours[2].waits.changes_to_resumed.is_empty());
    assert!(
        day.hours
            .iter()
            .all(|h| h.waits.changes_to_review.is_empty()),
        "it never went back to review"
    );
}

#[test]
fn a_review_with_no_reviewer_by_its_end_is_no_wait_and_a_reopened_task_enters_review_again() {
    let read = ReportRead {
        workflow: false,
        places: None,
        tasks: vec![task(
            "sim/z",
            0.0,
            "review",
            4.0,
            Thread::default()
                .made(0.0, "o")
                .claim(0.0, "e")
                .moved(1.0, "e", "in_progress", "review")
                .moved(2.0, "o", "review", "done")
                .moved(3.0, "o", "done", "review")
                .joined(4.0, "r"),
        )],
        ..ReportRead::default()
    };
    let day = day("sim", D, at(5.0), at(5.0), &read);
    assert!(day.hours[1].waits.review_to_reviewer.is_empty());
    assert_eq!(day.hours[3].top.entered, counts(&[("review", 1)]));
    assert_eq!(
        waits(&day.hours[4].waits.review_to_reviewer),
        [("sim/z", 1.0)]
    );
    assert_eq!(day.hours[2].top.entered, counts(&[("done", 1)]));
    assert_eq!(
        (day.hours[2].top.done, day.top.lead),
        (Some(0), None),
        "in review again, it is no task done: no close to count or to time"
    );
}

/// Back in review with its reviewer still holding the role, a task has no wait for a reviewer:
/// nothing on the board tells when that reviewer came back, not its posts nor its verdict, which
/// would make the wait the review itself. A session that joins as its reviewer, the one that had
/// let the role go included, or claims a review subtask of it, ends the wait as in a first round.
#[test]
fn a_re_review_whose_reviewer_kept_the_role_waits_only_for_a_join_or_a_review_claim() {
    let kept = |slug: &str, thread: Thread| {
        let status = if slug == "sim/post" { "done" } else { "review" };
        let updated = (thread.0.last().unwrap().ts - D) / H;
        task(slug, 0.0, status, updated, thread)
    };
    let base = || {
        Thread::default()
            .made(0.0, "o")
            .claim(0.0, "e")
            .moved(1.0, "e", "in_progress", "review")
            .joined(1.0, "r")
            .verdict(2.0, "r", "changes: no")
            .post(2.5, "r")
            .moved(3.0, "e", "in_progress", "review")
            .post(3.25, "e")
    };
    let mut review = task(
        "sim/claimed/review",
        3.0,
        "in_progress",
        4.25,
        Thread::default().made(3.0, "o").claim(4.25, "r3"),
    );
    review.parent = Some("sim/claimed".into());
    review.instruction = true;
    let read = ReportRead {
        workflow: false,
        places: None,
        tasks: vec![
            kept(
                "sim/post",
                base().post(4.5, "r").verdict(5.0, "r", "approve"),
            ),
            kept("sim/other", base().joined(4.0, "r2").post(4.5, "r")),
            kept("sim/claimed", base()),
            kept(
                "sim/back",
                base()
                    .line("system", 3.5, "r", "r released reviewer (released)")
                    .joined(4.5, "r"),
            ),
            review,
        ],
        ..ReportRead::default()
    };
    let day = day("sim", D, at(6.0), at(6.0), &read);
    let staffed: Vec<(usize, &str, f64)> = (day.hours.iter().enumerate())
        .flat_map(|(n, hour)| {
            waits(&hour.waits.review_to_reviewer)
                .into_iter()
                .map(move |(slug, hours)| (n, slug, hours))
        })
        .filter(|(n, _, _)| *n >= 2)
        .collect();
    assert_eq!(
        staffed,
        [
            (4, "sim/other", 1.0),
            (4, "sim/claimed", 1.25),
            (4, "sim/back", 1.5)
        ],
        "a new reviewer's join at 04:00, a review subtask's claim at 04:15, the reviewer's join \
         again at 04:30 after it let the role go; none for the one whose kept reviewer posted and \
         approved"
    );
}

/// A task done ([`Flow::done`]) counts once, in the hour of its last close, while it holds done at
/// the read, as `tasks stats` counts its tasks done per day: one approved, sent back to review by
/// its landing and approved again counts at the second approve; one closed the day before and
/// closed again today counts today alone; one cancelled after it was done, or reopened and still
/// in review, counts in no hour. `entered.done` keeps every entry into done, and the day's lead
/// time is of the tasks done alone.
#[test]
fn a_task_done_counts_once_at_its_last_close_and_one_cancelled_or_reopened_since_in_none() {
    let (o, e, r) = ("o", "e", "r");
    let worked = |made: f64| {
        Thread::default()
            .made(made, o)
            .claim(made, e)
            .moved(made + 0.5, e, "in_progress", "review")
            .joined(made + 0.5, r)
    };
    let again = task(
        "sim/again",
        1.0,
        "done",
        4.0,
        worked(1.0)
            .verdict(2.0, r, "approve: fine")
            .moved(3.0, e, "done", "review")
            .verdict(4.0, r, "approve: the mer")
            .verdict(4.5, o, "landed sim/again"),
    );
    let next = task(
        "sim/next",
        -30.0,
        "done",
        5.0,
        worked(-30.0)
            .verdict(-20.0, r, "approve")
            .moved(1.0, e, "done", "review")
            .verdict(5.0, r, "approve"),
    );
    // Cancelled by a hand edit after its approve: no line says so, its `updated` dates it.
    let cancelled = task(
        "sim/cancelled",
        0.0,
        "cancelled",
        7.0,
        worked(0.0).verdict(6.0, r, "approve"),
    );
    let reopened = task(
        "sim/reopened",
        0.0,
        "review",
        9.0,
        worked(0.0)
            .verdict(8.0, r, "approve")
            .moved(9.0, e, "done", "review"),
    );
    let mut slice = task(
        "sim/again/slice",
        1.0,
        "done",
        7.0,
        Thread::default()
            .made(1.0, o)
            .claim(1.0, e)
            .verdict(7.0, e, "done: merged in"),
    );
    slice.parent = Some("sim/again".into());
    let read = ReportRead {
        workflow: false,
        places: None,
        tasks: vec![next, cancelled, reopened, again, slice],
        ..ReportRead::default()
    };
    let today = day("sim", D, at(10.0), at(10.0), &read);
    let per_hour = |pick: fn(&ReportHour) -> u32| -> Vec<(usize, u32)> {
        (today.hours.iter().enumerate())
            .filter(|(_, hour)| pick(hour) > 0)
            .map(|(n, hour)| (n, pick(hour)))
            .collect()
    };
    assert_eq!(
        per_hour(|h| h.top.done.unwrap()),
        [(4, 1), (5, 1)],
        "sim/again at its second approve, sim/next at its close today"
    );
    assert!(today.hours.iter().all(|h| h.top.done.is_some()));
    assert_eq!(
        per_hour(|h| h.top.entered.done),
        [(2, 1), (4, 1), (5, 1), (6, 1), (8, 1)],
        "every entry into done: both of sim/again's, sim/next's, the cancelled one's and the \
         reopened one's"
    );
    assert_eq!(per_hour(|h| h.top.entered.cancelled), [(7, 1)]);
    assert_eq!(per_hour(|h| h.subtasks.done.unwrap()), [(7, 1)]);
    let lead = today.top.lead.unwrap();
    assert_eq!(
        (lead.n, lead.median / H),
        (2, 19.0),
        "creation to last close of sim/again (3 h) and sim/next (35 h)"
    );
    assert!((lead.p90 / H - 31.8).abs() < 1e-9, "{}", lead.p90 / H);
    let yesterday = day("sim", D - DAY, D, at(10.0), &read);
    let done: u32 = yesterday.hours.iter().filter_map(|h| h.top.done).sum();
    let entered: u32 = yesterday.hours.iter().map(|h| h.top.entered.done).sum();
    assert_eq!(
        (done, entered, yesterday.top.lead),
        (0, 1, None),
        "read after sim/next closed again today, its first close yesterday is no task done"
    );
    // `tasks stats` counts its tasks done per day over the same tasks by the same rule: the last
    // close of each top-level task done now, here by UTC day.
    let closes: Vec<f64> = (read.tasks.iter())
        .filter(|task| task.parent.is_none())
        .filter_map(|task| {
            let facts = Facts {
                status: &task.status,
                created: task.created,
                updated: task.updated,
            };
            stages::stages(facts, &task.lines, at(10.0)).done_at()
        })
        .collect();
    let utc_day = |at: f64| symphony_api::run_day(day_start(at));
    let stats = stages::stats::summarize(&[], &closes, at(10.0), 1, &utc_day);
    let rolled: Vec<(String, usize)> = [&yesterday, &today]
        .iter()
        .map(|day| {
            let done: u32 = day.hours.iter().filter_map(|h| h.top.done).sum();
            (day.day.clone(), usize::try_from(done).unwrap())
        })
        .collect();
    assert_eq!(stats.done_per_day, rolled);
}

#[test]
fn the_folder_is_the_one_named_else_beside_the_board_and_none_for_a_named_server() {
    let cwd = std::env::temp_dir();
    let env = |vars: &[(&str, &str)]| Env {
        vars: vars
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect(),
        cwd: cwd.clone(),
    };
    assert_eq!(
        folder(&env(&[(FOLDER_VAR, "/x/reports"), ("TASKS_SERVER", "1")])),
        Ok(PathBuf::from("/x/reports"))
    );
    let refused = folder(&env(&[("TASKS_SERVER", "127.0.0.1:1")])).unwrap_err();
    assert!(refused.contains(FOLDER_VAR), "{refused}");
    assert!(refused.contains("no reports unless"), "{refused}");
    let other = day_files(&env(&[("TASKS_SERVER", "1")]), "f", "F_DIR", "f files").unwrap_err();
    assert!(other.contains("no f files unless F_DIR"), "{other}");
    // An empty setting names nothing: the refusal for a named server stands.
    assert!(folder(&env(&[(FOLDER_VAR, ""), ("TASKS_SERVER", "1")])).is_err());
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let live = folder(&Env {
        vars: BTreeMap::new(),
        cwd: here.into(),
    })
    .unwrap();
    let common = crate::git::common_dir(here).unwrap();
    assert_eq!(live, common.join("symphony/reports"));
    let scratch = folder(&Env {
        vars: BTreeMap::from([("TASKS_DATABASE".into(), "t1".into())]),
        cwd: here.into(),
    })
    .unwrap();
    assert_eq!(scratch, common.join("symphony/reports.t1"));
}

#[test]
fn a_task_approved_the_day_before_and_landed_after_midnight_lands_today_with_its_wait() {
    let read = ReportRead {
        workflow: false,
        places: None,
        tasks: vec![task(
            "sim/late",
            -5.0,
            "done",
            -0.25,
            Thread::default()
                .made(-5.0, "o")
                .claim(-4.0, "e")
                .moved(-1.0, "e", "in_progress", "review")
                .joined(-0.5, "r")
                .verdict(-0.25, "r", "approve: fine")
                .verdict(0.5, "o", "landed sim/late on m"),
        )],
        ..ReportRead::default()
    };
    let today = day("sim", D, at(1.5), at(1.5), &read);
    assert_eq!(today.hours[0].landed, 1);
    assert_eq!(
        waits(&today.hours[0].waits.approved_to_landed),
        [("sim/late", 0.75)],
        "from the approve at 23:45 to the landing at 00:30"
    );
    assert!(today.rounds.is_empty(), "its verdict was the day before");
    let yesterday = day("sim", D - DAY, D + 1.5 * H, D + 1.5 * H, &read);
    assert_eq!(yesterday.hours.iter().map(|h| h.landed).sum::<u32>(), 0);
    assert_eq!(yesterday.rounds.len(), 1);
}

#[test]
fn the_longest_in_their_status_are_the_ten_oldest_by_since_then_slug_whatever_the_read_order() {
    // Twelve ready tasks, read in an order that is neither their age nor their names'.
    let made = [
        5.0, -3.0, 2.0, -3.0, 7.0, 1.0, -1.0, 4.0, 0.5, 6.0, 3.0, -2.0,
    ];
    let tasks = made
        .iter()
        .enumerate()
        .map(|(i, hours)| {
            let slug = format!("sim/t{:02}", made.len() - 1 - i);
            task(
                &slug,
                *hours,
                "ready",
                *hours,
                Thread::default().made(*hours, "o"),
            )
        })
        .collect();
    let read = ReportRead {
        tasks,
        ..ReportRead::default()
    };
    let day = day("sim", D, at(9.0), at(9.0), &read);
    let longest: Vec<(&str, f64)> = day
        .top
        .longest
        .iter()
        .map(|s| (s.slug.as_str(), (s.since - D) / H))
        .collect();
    assert_eq!(
        longest,
        [
            ("sim/t08", -3.0),
            ("sim/t10", -3.0),
            ("sim/t00", -2.0),
            ("sim/t05", -1.0),
            ("sim/t03", 0.5),
            ("sim/t06", 1.0),
            ("sim/t09", 2.0),
            ("sim/t01", 3.0),
            ("sim/t04", 4.0),
            ("sim/t11", 5.0),
        ],
        "two made at once by their slugs; the two youngest left out"
    );
}

/// The day file's text written by hand is serde's, byte for byte: the wire fixture, which fills
/// every field (an hour's decisions and git, a spread, a red closed and one open, a card waiting
/// since no line), the fixture board's day under way and its day before, a day with nothing in it,
/// and strings and numbers serde escapes or writes as `null`.
#[test]
fn a_day_file_s_text_is_the_one_serde_writes() {
    let fixture: ReportDay = serde_json::from_str(include_str!(
        "../../../symphony-api/fixtures/report-day.json"
    ))
    .unwrap();
    let decisions = fixture.decisions.as_ref().unwrap();
    assert!(
        !decisions.open.is_empty() && !decisions.withdrawn.is_empty() && fixture.git.is_some(),
        "the fixture fills the lists the day file can hold"
    );
    let mut odd = day("p \"q\" \\ \n\t\u{1}é☃", D, at(2.0), at(2.0), &board());
    odd.read = f64::NAN;
    odd.hours[0].top.task_hours.review = f64::INFINITY;
    // An hour that did not count its tasks done, as one of a file written before they were.
    odd.hours[1].top.done = None;
    odd.hours[1].waits.changes_to_review.push(Wait {
        slug: "sim/\u{7f}\u{2028}".into(),
        seconds: -0.0,
    });
    // Tokens: the sums kept, an hour that gained some, one that gained none, and a landed task.
    let sum = symphony_api::reports::TokenSum {
        role: "exec\"utor".into(),
        round: 2,
        model: "m".into(),
        speed: "fast".into(),
        input: u64::MAX,
        cache_write_5m: 1,
        cache_write_1h: 2,
        cache_read: 3,
        output: 4,
        calls: 5,
    };
    odd.usage = Some(vec![sum.clone(), sum.clone()]);
    // The odd day's first hour gained some; its second, none known.
    odd.hours[0].tokens = Some(vec![sum.clone()]);
    // One with a kind and lines, one with lines of none, one with a kind alone, and one with
    // neither, as a file before they were kept names it.
    odd.landed = vec![
        symphony_api::reports::Landed {
            slug: "sim/a".into(),
            at: 1.5,
            spent: vec![sum],
            kind: Some("fe\"ature".into()),
            lines: Some(symphony_api::reports::Lines {
                added: u64::MAX,
                removed: 7,
            }),
        },
        symphony_api::reports::Landed {
            slug: "sim/b".into(),
            lines: Some(symphony_api::reports::Lines::default()),
            ..symphony_api::reports::Landed::default()
        },
        symphony_api::reports::Landed {
            kind: Some("docs".into()),
            ..symphony_api::reports::Landed::default()
        },
        symphony_api::reports::Landed::default(),
    ];
    let days = [
        fixture,
        day("sim", D, at(13.5), at(13.5), &board()),
        day("sim", D - DAY, D, D, &board()),
        ReportDay::default(),
        odd,
    ];
    for day in &days {
        let ours = String::from_utf8(json::day(day)).unwrap();
        let serde = serde_json::to_string(day).unwrap();
        assert_eq!(ours, serde, "{}", day.project);
    }
    let odd = String::from_utf8(json::day(&days[4])).unwrap();
    assert!(odd.contains(r#""done":null"#), "{odd}");
    assert!(odd.contains(r#""tokens":[{"role":"exec\"utor""#), "{odd}");
    assert!(
        odd.contains(r#""tokens":null"#) && odd.contains(r#""spent":[]"#),
        "{odd}"
    );
    assert!(
        odd.contains(r#""kind":"fe\"ature","lines":{"added":18446744073709551615,"removed":7}}"#),
        "{odd}"
    );
    assert!(
        odd.contains(r#""slug":"sim/b","at":0.0,"spent":[],"lines":{"added":0,"removed":0}}"#),
        "{odd}"
    );
    assert!(
        odd.ends_with(r#"{"slug":"","at":0.0,"spent":[]}]}"#),
        "{odd}"
    );
}

#[test]
fn a_task_landed_in_the_day_is_named_once_at_its_last_landing_with_what_it_took() {
    let sum = |role: &str, round: u32, n: u64| symphony_api::reports::TokenSum {
        role: role.into(),
        round,
        model: "m".into(),
        speed: "standard".into(),
        input: n,
        ..Default::default()
    };
    // Made an hour before its first landing, which can be the day before.
    let landed = |slug: &str, hours: &[f64]| {
        let made = hours.iter().copied().fold(0.0, f64::min) - 1.0;
        let mut thread = Thread::default().made(made, "o").claim(made, "e");
        for hour in hours {
            thread = thread.verdict(*hour, "o", &format!("landed {slug} on m"));
        }
        task(slug, made, "done", 9.0, thread)
    };
    let mut sub = landed("sim/b/part", &[1.0]);
    sub.parent = Some("sim/b".into());
    let mut kinded = landed("sim/a", &[5.0]);
    kinded.kind = Some("fix".into());
    let read = ReportRead {
        tasks: vec![
            // Landed twice in the day, and once the day before.
            landed("sim/b", &[-3.0, 2.0, 5.0]),
            kinded,
            landed("sim/quiet", &[4.0]),
            landed("sim/tomorrow", &[30.0]),
            landed("sim/yesterday", &[-1.0]),
            sub,
            task(
                "sim/open",
                0.0,
                "ready",
                0.0,
                Thread::default().made(0.0, "o"),
            ),
        ],
        spent: vec![
            ("sim/a".into(), sum("executor", 1, 7)),
            ("sim/b".into(), sum("executor", 1, 1)),
            ("sim/b".into(), sum("reviewer", 2, 2)),
            ("sim/yesterday".into(), sum("executor", 1, 9)),
        ],
        ..ReportRead::default()
    };
    let day = day("sim", D, D + DAY, at(40.0), &read);
    let told: Vec<(&str, f64, Vec<u64>, Option<&str>)> = (day.landed.iter())
        .map(|task| {
            let spent = task.spent.iter().map(|sum| sum.input).collect();
            let kind = task.kind.as_deref();
            (task.slug.as_str(), (task.at - D) / HOUR, spent, kind)
        })
        .collect();
    assert_eq!(
        told,
        [
            ("sim/quiet", 4.0, vec![], None),
            ("sim/a", 5.0, vec![7], Some("fix")),
            ("sim/b", 5.0, vec![1, 2], None),
        ],
        "top-level tasks alone, in the day alone, oldest first then by slug, each with its kind"
    );
    assert!(
        day.landed.iter().all(|task| task.lines.is_none()),
        "the git roll-up's to fill"
    );
    assert_eq!(day.usage, None, "the roll-up's to fill");
}

#[test]
fn a_day_file_is_written_whole_read_back_and_none_where_there_is_none() {
    let folder = std::env::temp_dir().join(format!("symphony-reports-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    let written = day("sim", D, at(13.5), at(13.5), &board());
    assert_eq!(read(&folder, &written.day), Ok(None));
    assert!(!closed(&folder, &written.day), "no file is no final file");
    write(&folder, &written).unwrap();
    assert_eq!(read(&folder, &written.day), Ok(Some(written.clone())));
    let text = std::fs::read_to_string(folder.join(format!("{}.json", written.day))).unwrap();
    assert_eq!(
        text.find('\n'),
        Some(text.len() - 1),
        "compact JSON on one line, ended by a newline"
    );
    assert!(!closed(&folder, &written.day), "a day under way");
    let ended = day("sim", D - DAY, D, D, &board());
    write(&folder, &ended).unwrap();
    assert!(closed(&folder, &ended.day), "a day read after its end");
    std::fs::remove_file(folder.join(format!("{}.json", ended.day))).unwrap();
    let names = || -> Vec<String> {
        std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    };
    let name = format!("{}.json", written.day);
    assert_eq!(
        names(),
        std::slice::from_ref(&name),
        "no partial file is left"
    );
    // A folder in the file's place: the written file cannot take it, and goes.
    let path = folder.join(&name);
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir_all(path.join("held")).unwrap();
    let refused = write(&folder, &written).unwrap_err();
    assert!(refused.contains(&name), "{refused}");
    assert_eq!(
        names(),
        std::slice::from_ref(&name),
        "no partial file is left"
    );
    std::fs::remove_dir_all(&path).unwrap();
    std::fs::write(folder.join("1999-01-01.json"), "{").unwrap();
    assert!(
        read(&folder, "1999-01-01")
            .unwrap_err()
            .contains("1999-01-01.json")
    );
    assert!(!closed(&folder, "1999-01-01"), "a file that does not read");
    std::fs::write(
        folder.join("1999-01-02.json"),
        r#"{"closed":true,"later":[1]}"#,
    )
    .unwrap();
    assert!(
        closed(&folder, "1999-01-02"),
        "only `closed` is read, whatever else the file holds"
    );
    for (n, text) in [r#"{"closed":"yes"}"#, r#"{"hours":[]}"#, "[true]"]
        .iter()
        .enumerate()
    {
        let day = format!("1999-02-0{}", n + 1);
        std::fs::write(folder.join(format!("{day}.json")), text).unwrap();
        assert!(!closed(&folder, &day), "{text}: no `closed` that is true");
    }
    std::fs::remove_dir_all(&folder).unwrap();
}
