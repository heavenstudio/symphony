//! PULL digest: the pipeline alarm ([`symphony_board::alarm`]), raised while review holds more than
//! twice the work in progress for 4 hours and cleared once it has held no more for an hour; and
//! rework, a task in review whose tip a reviewer asked changes of, counted in progress. What held
//! when the replay's window opened is read from the board, never assumed (`adversarial_…`); how
//! the branch's commits end rework is [`rework`]'s.

mod rework;

use super::{Case, slugs};
use symphony_board::NewTask;
use symphony_board::alarm::{LOOKBACK_SECONDS, Thresholds, Tip, Written};
use symphony_board::cli::pull::{ALARM, Digest, REWORK_NOTE, digest_with_tips};
use symphony_board::sys::local_minute;

const HOUR: f64 = 3600.0;
const MINUTE: f64 = 60.0;

/// What the repository answers of a branch whose tips were `made` (or moved to) as its reflog
/// tells, oldest first: its tip now, or what it first moved to from a commit after a record
/// written at a time judged it, from the newest time at it by then (else the oldest); none while
/// it has not moved from it since, the tip when it is none of them.
pub(super) fn branch(made: &[Tip], after: Option<Written<'_>>) -> Option<Tip> {
    let now = made.last()?.clone();
    let Some(record) = after else {
        return Some(now);
    };
    let at_judged = |tip: &Tip| tip.commit == record.tip;
    let by_then = (made.iter()).rposition(|tip| at_judged(tip) && tip.at <= record.at);
    match by_then.or_else(|| made.iter().position(at_judged)) {
        Some(n) => made[n..].iter().find(|tip| !at_judged(tip)).cloned(),
        None => Some(now),
    }
}

impl Case {
    /// `slug`'s branch now at `commit`, made at `at`, after those made before.
    fn tip(&mut self, slug: &str, commit: &str, at: f64) {
        let tip = Tip {
            commit: commit.into(),
            at,
        };
        self.tips.entry(slug.into()).or_default().push(tip);
    }

    /// What `slug`'s landing waits on, recorded as the landing tool records it: the tip it judged,
    /// then the names, of an assessment made now.
    fn waits(&self, slug: &str, record: &str) {
        let now = format!("look@{}", self.f.now());
        self.waits_of(slug, record, &now);
    }

    /// [`Case::waits`] of the assessment `assessment` names.
    fn waits_of(&self, slug: &str, record: &str, assessment: &str) {
        let land = self.f.session("worker");
        let store = &self.f.store;
        store.record_waits(slug, record, assessment, &land).unwrap();
    }

    /// 4 tasks in progress, then 10 taken to review a minute apart, `sim/review-0` first: review is
    /// more than twice in progress from the ninth, which this returns the time of.
    fn ten_against_four(&self) -> f64 {
        for n in 0..4 {
            self.task(&format!("sim/work-{n}"), "in_progress");
        }
        let mut over = 0.0;
        for n in 0..10 {
            self.task(&format!("sim/review-{n}"), "review");
            if n == 8 {
                over = self.f.now();
            }
            self.f.advance(MINUTE);
        }
        over
    }

    /// The digest with the board's clock at `at`.
    fn digest_at(&self, at: f64) -> Digest {
        self.f.advance(at - self.f.now());
        self.digest()
    }

    /// What the alarm counts at `at` while it stands ([`standing`]).
    fn alarm_at(&self, at: f64) -> Option<(usize, usize)> {
        standing(&self.digest_at(at))
    }
}

/// The alarm's counts in `d`, review then in progress, while it stands; none while it does not.
fn standing(d: &Digest) -> Option<(usize, usize)> {
    let counts = d.alarm.as_ref().map(|raised| raised.alarm.counts);
    counts.map(|counts| (counts.review, counts.in_progress))
}

/// The time a digest line names: the minute alone on the day of the digest.
fn minute(at: f64) -> String {
    local_minute(at)[11..].to_string()
}

#[test]
fn ten_in_review_against_four_in_progress_for_four_hours_raises_the_alarm_naming_the_five_longest()
{
    let mut c = Case::new();
    let over = c.ten_against_four();
    // What the landing of each of the five longest waits on, as the landing tool records it; the
    // fifth's record judged a tip its branch has since moved past.
    let missing = [
        "certificate, review",
        "nightly",
        "owner",
        "conflict",
        "certificate",
    ];
    let numbered = missing.iter().enumerate();
    for (n, on) in numbered {
        let slug = format!("sim/review-{n}");
        c.waits(&slug, &format!("tip{n}: {on}"));
        c.tip(&slug, &format!("tip{n}"), over);
    }
    c.tip("sim/review-4", "tip4-fixed", over + MINUTE);
    let notes = [
        "missing certificate, review",
        "missing nightly",
        "missing owner",
        "missing conflict",
        "its tip is newer than its landing's last look",
    ];
    let d = c.digest_at(over + 4.0 * HOUR);
    let raised = d.alarm.as_ref().expect("raised after four hours");
    assert_eq!(
        (raised.alarm.counts.review, raised.alarm.counts.in_progress),
        (10, 4)
    );
    assert!(
        d.entries().contains(&(ALARM.into(), "review".into())),
        "its raising is news: {:?}",
        d.entries()
    );
    let lines = d.lines();
    assert!(lines[0].ends_with(" review_alarm=10/4"), "{}", lines[0]);
    let longest: Vec<String> = (notes.iter().enumerate())
        .map(|(n, note)| {
            let entered = crate::common::NOW + n as f64 * MINUTE;
            format!(
                "  review_alarm p2 sim/review-{n} — T sim/review-{n}   (in review since {}; {note})",
                minute(entered)
            )
        })
        .collect();
    assert_eq!(
        lines[1..7],
        [
            vec![format!(
                "  review_alarm: review 10, in progress 4, more than 2× since {}",
                minute(over)
            )],
            longest
        ]
        .concat(),
        "the counts, then the five longest in review, longest first, each with what it lacks"
    );
    assert!(
        !lines[7].starts_with(&format!("  {ALARM}")),
        "five named, no more: {}",
        lines[7]
    );
    assert_eq!(
        slugs(&d, "review_unreviewed").len(),
        10,
        "the categories still list every one"
    );
}

#[test]
fn the_same_for_three_hours_raises_nothing() {
    let c = Case::new();
    let over = c.ten_against_four();
    for at in [over + 3.0 * HOUR, over + 4.0 * HOUR - 1.0] {
        let d = c.digest_at(at);
        assert_eq!(standing(&d), None, "at {}", at - over);
        assert!(!d.lines()[0].contains(ALARM), "{}", d.lines()[0]);
        let entries = d.entries();
        assert!(
            !entries.iter().any(|(name, _)| name == ALARM),
            "{entries:?}"
        );
    }
}

#[test]
fn falling_back_to_twice_in_progress_clears_the_alarm_only_after_an_hour() {
    let c = Case::new();
    let over = c.ten_against_four();
    c.digest_at(over + 5.0 * HOUR);
    let reviewer = c.f.session("reviewer");
    for slug in ["sim/review-0", "sim/review-1"] {
        c.f.store.take_role(slug, &reviewer, "reviewer").unwrap();
        c.f.store.verdict(slug, &reviewer, "approve", "ok").unwrap();
    }
    let fell = c.f.now();
    let dip = c.digest_at(fell + HOUR - 1.0);
    let still = dip.alarm.as_ref().expect("under for less than an hour");
    assert_eq!(
        dip.lines()[1],
        format!(
            "  review_alarm: review 8, in progress 4, at most 2× since {}; it clears after 1 h of that",
            minute(fell)
        )
    );
    assert_eq!(
        still.longest[0].task.slug, "sim/review-2",
        "the two approved are done"
    );
    let cleared = c.digest_at(fell + HOUR);
    assert_eq!(standing(&cleared), None, "an hour at twice clears it");
    let header = &cleared.lines()[0];
    assert!(!header.contains(ALARM), "{header}");
}

#[test]
fn a_review_mirror_and_an_instruction_subtask_count_with_the_work_they_serve() {
    let c = Case::new();
    for n in 0..4 {
        c.task(&format!("sim/work-{n}"), "in_progress");
        // An agent's instruction under work in progress is no work of its own.
        let sub = c.sub(&format!("sim/work-{n}/look"), &format!("sim/work-{n}"));
        c.task_with(sub, "in_progress");
    }
    for n in 0..8 {
        let slug = format!("sim/review-{n}");
        c.task(&slug, "review");
        c.task(&format!("review/{slug}"), "review");
    }
    assert_eq!(
        c.alarm_at(c.f.now() + 5.0 * HOUR),
        None,
        "8 in review against 4 in progress, mirrors and instructions left out"
    );
    // A mirror of work the board does not hold is work of its own.
    c.task("review/gone", "review");
    let at = c.f.now();
    assert_eq!(c.alarm_at(at + 4.0 * HOUR), Some((9, 4)));
}

#[test]
fn the_board_s_thresholds_say_when_it_raises() {
    let c = Case::new();
    let over = c.ten_against_four();
    c.f.advance(over + 4.0 * HOUR - c.f.now());
    let patient = c.f.other().alarming(Thresholds {
        ratio: 2,
        raise_hours: 6,
        clear_hours: 1,
    });
    let lenient = c.f.other().alarming(Thresholds {
        ratio: 3,
        raise_hours: 4,
        clear_hours: 1,
    });
    let raised = [&c.f.store, &patient, &lenient].map(|store| {
        let d = digest_with_tips(store, &[], None, None, c.f.now(), &[]).unwrap();
        d.alarm.is_some()
    });
    assert_eq!(
        raised,
        [true, false, false],
        "the defaults; six hours to raise; 10 is not more than three times 4"
    );
}

#[test]
fn every_orchestrator_hears_the_whole_board_s_alarm() {
    let c = Case::new();
    let over = c.ten_against_four();
    let orchestrator = c.f.session("orchestrator");
    c.task_with(NewTask::new("docs/own", "Own", "owner"), "ready");
    c.f.store
        .take_role("docs/own", &orchestrator, "orchestrator")
        .unwrap();
    let d = c.digest_at(over + 4.0 * HOUR);
    let board = c.f.store.board().unwrap();
    let heard = d.only_for(&board, &orchestrator);
    assert_eq!(
        heard.alarm.map(|raised| raised.longest.len()),
        Some(5),
        "none of the work is its own, and it hears the alarm whole"
    );
}

// -- rework ------------------------------------------------------------------------------

#[test]
fn a_task_in_review_whose_tip_a_reviewer_asked_changes_of_is_in_progress_its_executor_executing() {
    let mut c = Case::new();
    let now = c.f.now();
    for slug in ["sim/a", "sim/b"] {
        c.task(slug, "review");
        c.tip(slug, &format!("{slug}-1"), now);
    }
    c.waits("sim/a", "sim/a-1: changes, review");
    c.waits("sim/b", "sim/b-1: changes");
    let gone = c.executor("sim/b");
    c.f.store.leave(&gone).unwrap();
    let d = c.digest();
    assert_eq!(d.executing, 1, "sim/a's executor is at work on it");
    assert!(d.lines()[0].contains(" executing=1 "), "{}", d.lines()[0]);
    assert_eq!(
        slugs(&d, "in_progress_orphaned"),
        ["sim/b"],
        "nobody executes sim/b's rework"
    );
    assert_eq!(d.notes.get("sim/b").map(String::as_str), Some(REWORK_NOTE));
    assert_eq!(
        slugs(&d, "review_unreviewed"),
        [""; 0],
        "neither waits on a reviewer"
    );
    // A new tip on sim/a, and sim/b's record asking nothing of its tip now: both wait on review.
    c.tip("sim/a", "sim/a-2", now + MINUTE);
    c.waits("sim/b", "sim/b-1: review");
    let d = c.digest();
    assert_eq!(d.executing, 0);
    assert_eq!(slugs(&d, "in_progress_orphaned"), [""; 0]);
    assert_eq!(slugs(&d, "review_unreviewed"), ["sim/a", "sim/b"]);
}

#[test]
fn rework_counts_in_progress_in_the_alarm_until_its_new_tip_is_made() {
    let mut c = Case::new();
    c.ten_against_four();
    // A reviewer asks changes of one tip at once: 9 in review against 5 in progress, at most twice.
    let asked = c.f.now();
    c.tip("sim/review-0", "tip0", asked);
    c.waits("sim/review-0", "tip0: changes");
    assert_eq!(
        c.alarm_at(asked + 5.0 * HOUR),
        None,
        "rework does not wait on review"
    );
    // The new tip, made then, waits on review: 10 against 4 again from that moment.
    let fixed = c.f.now();
    c.tip("sim/review-0", "tip0-fixed", fixed);
    assert_eq!(c.alarm_at(fixed + 4.0 * HOUR - 1.0), None);
    let d = c.digest_at(fixed + 4.0 * HOUR);
    let raised = d.alarm.expect("four hours over since the new tip");
    assert_eq!(
        (
            raised.alarm.counts.review,
            raised.alarm.counts.in_progress,
            raised.alarm.since
        ),
        (10, 4, fixed)
    );
}

// -- what held when the window opened ------------------------------------------------------------

#[test]
fn adversarial_old_changes_record_still_bounds_review_after_a_new_tip() {
    let mut c = Case::new();
    c.ten_against_four();
    // 9 in review against 5 in progress for 26 hours: the record is older than the window.
    let asked = c.f.now();
    c.tip("sim/review-0", "tip0", asked);
    c.waits("sim/review-0", "tip0: changes");
    c.f.advance(26.0 * HOUR);
    let fixed = c.f.now();
    c.tip("sim/review-0", "tip0-fixed", fixed);
    for at in [fixed + HOUR, fixed + 4.0 * HOUR - 1.0] {
        assert_eq!(c.alarm_at(at), None, "{} h", (at - fixed) / HOUR);
    }
    let raised = c.digest_at(fixed + 4.0 * HOUR).alarm;
    let alarm = raised.expect("four hours over since the new tip").alarm;
    assert_eq!(
        (alarm.counts.review, alarm.counts.in_progress, alarm.since),
        (10, 4, fixed)
    );
}

#[test]
fn adversarial_short_dips_never_clear_an_alarm_even_after_a_day() {
    let c = Case::new();
    let over = c.ten_against_four();
    assert_eq!(c.alarm_at(over + 5.0 * HOUR), Some((10, 4)));
    let exec = c.executor("sim/review-9");
    // Nine times half an hour at 9 against 5, then three hours at 10 against 4: no dip clears it,
    // and the four hours that raised it leave the day's window part way through.
    for round in 0..9 {
        c.f.store
            .move_to("sim/review-9", "in_progress", &exec)
            .unwrap();
        c.f.advance(30.0 * MINUTE);
        c.f.store.move_to("sim/review-9", "review", &exec).unwrap();
        c.f.advance(3.0 * HOUR);
        assert_eq!(standing(&c.digest()), Some((10, 4)), "round {round}");
    }
    let raised_out = c.f.now() - (over + 5.0 * HOUR);
    assert!(raised_out > LOOKBACK_SECONDS, "{raised_out} s");
}

#[test]
fn adversarial_a_hand_over_of_work_in_progress_before_the_window_was_in_progress_all_through() {
    let c = Case::new();
    for n in 0..5 {
        c.task(&format!("sim/work-{n}"), "in_progress");
    }
    for n in 0..10 {
        c.task(&format!("sim/review-{n}"), "review");
    }
    // 10 in review against 5 in progress, at most twice, for 30 hours; then the lost executor's
    // work is handed over.
    c.f.advance(30.0 * HOUR);
    let next = c.f.session("worker");
    let task = c.f.store.claim("sim/work-0", &next).unwrap();
    assert_eq!(task.holders("executor"), [next.as_str()]);
    for at in [c.f.now(), c.f.now() + 5.0 * HOUR] {
        assert_eq!(c.alarm_at(at), None);
    }
}

#[test]
fn adversarial_four_field_values_that_read_like_moves_hide_no_status_before_the_window() {
    let c = Case::new();
    for n in 0..4 {
        c.task(&format!("sim/work-{n}"), "in_progress");
    }
    for n in 0..8 {
        c.task(&format!("sim/review-{n}"), "review");
    }
    // 8 in review against 4 in progress all through: the claim before the window lies under four
    // field values whose text reads like a move.
    let who = c.executor("sim/work-0");
    let note = serde_json::json!("worker moved it in_progress → review");
    for _ in 0..4 {
        c.f.advance(1.0);
        c.f.store
            .set_field("sim/work-0", "game", "note", &note, &who)
            .unwrap();
    }
    c.f.advance(25.0 * HOUR);
    c.f.store.claim("sim/work-0", &c.worker()).unwrap();
    for at in [c.f.now(), c.f.now() + 0.5 * HOUR] {
        assert_eq!(c.alarm_at(at), None);
    }
}

#[test]
fn adversarial_a_bounce_at_the_window_s_edge_lasts_no_time() {
    let c = Case::new();
    for n in 0..4 {
        c.task(&format!("sim/work-{n}"), "in_progress");
    }
    for n in 0..8 {
        c.task(&format!("sim/review-{n}"), "review");
    }
    c.f.advance(25.0 * HOUR);
    let edge = c.f.now();
    let who = c.executor("sim/review-0");
    let bounce = ["in_progress", "review"];
    for status in bounce {
        c.f.store.move_to("sim/review-0", status, &who).unwrap();
    }
    assert_eq!(
        c.alarm_at(edge + LOOKBACK_SECONDS),
        None,
        "never more than twice"
    );
}

// -- the tree as it stood ------------------------------------------------------------------------

#[test]
fn adversarial_a_mirror_s_task_made_later_takes_its_review_over_from_then_on() {
    let c = Case::new();
    for n in 0..4 {
        c.task(&format!("sim/work-{n}"), "in_progress");
    }
    for n in 0..8 {
        c.task(&format!("sim/review-{n}"), "review");
    }
    // A review mirror of work the board does not hold is work of its own: 9 against 4.
    c.task("review/sim/late", "review");
    c.f.advance(5.0 * HOUR);
    assert_eq!(c.alarm_at(c.f.now()), Some((9, 4)));
    // The work it reviews is made, in review: 9 against 4 still, the mirror counted with it.
    c.task("sim/late", "review");
    let made = c.f.now();
    for at in [made, made + 2.0 * HOUR] {
        assert_eq!(c.alarm_at(at), Some((9, 4)), "{} h on", (at - made) / HOUR);
    }
}

#[test]
fn adversarial_an_instruction_taken_from_under_its_task_counts_from_then_on() {
    let c = Case::new();
    let over = c.ten_against_four();
    c.task_with(c.sub("sim/work-0/helper", "sim/work-0"), "in_progress");
    assert_eq!(
        c.alarm_at(over + 5.0 * HOUR),
        Some((10, 4)),
        "an instruction counts with the task it serves"
    );
    c.f.store
        .set_parent("sim/work-0/helper", None, "owner")
        .unwrap();
    // Its own work from now: 10 against 5, at most twice, for less than the hour that clears.
    let taken = c.f.now();
    for at in [taken, taken + HOUR - 1.0] {
        assert_eq!(c.alarm_at(at), Some((10, 5)), "{} s on", at - taken);
    }
    assert_eq!(c.alarm_at(taken + HOUR), None, "an hour at twice clears it");
}
