//! Rework against the branch's commits: a record of changes ends at the first commit its branch
//! moved to from the tip it judged after the record, however late its record comes, whatever the
//! task's status since, and wherever the branch went after, once the board has learnt it; a new
//! record of changes on a tip the branch came back to, its text the same or not, asks rework of its
//! own in PULL and the alarm alike, and a retry of the record answered asks none of either.

use super::*;

#[test]
fn adversarial_a_new_tip_ends_rework_when_it_was_made_though_its_record_comes_later() {
    let mut c = Case::new();
    c.ten_against_four();
    // 9 in review against 5 in progress for 26 hours under a record of changes.
    c.tip("sim/review-0", "old", c.f.now());
    c.waits("sim/review-0", "old: changes");
    c.f.advance(26.0 * HOUR);
    // The new tip; two hours on, the landing's record of it.
    let fixed = c.f.now();
    c.tip("sim/review-0", "fixed", fixed);
    c.f.advance(2.0 * HOUR);
    c.waits("sim/review-0", "fixed: certificate, review");
    assert_eq!(c.alarm_at(fixed + 4.0 * HOUR - 1.0), None);
    let raised = c.digest_at(fixed + 4.0 * HOUR).alarm;
    let alarm = raised.expect("four hours over since the new tip").alarm;
    assert_eq!(
        (alarm.counts.review, alarm.counts.in_progress, alarm.since),
        (10, 4, fixed)
    );
}

#[test]
fn adversarial_rework_survives_a_status_bounce_until_a_new_tip() {
    let mut c = Case::new();
    c.ten_against_four();
    let asked = c.f.now();
    c.tip("sim/review-0", "old-tip", asked);
    c.waits("sim/review-0", "old-tip: changes");
    // Into progress and back an hour on, its tip kept: still the tip a reviewer asked changes of.
    c.f.advance(HOUR);
    let who = c.executor("sim/review-0");
    let bounce = ["in_progress", "review"];
    for status in bounce {
        c.f.store.move_to("sim/review-0", status, &who).unwrap();
    }
    assert_eq!(c.alarm_at(asked + 6.0 * HOUR), None);
    let fixed = c.f.now();
    c.tip("sim/review-0", "new-tip", fixed);
    for at in [fixed + HOUR, fixed + 4.0 * HOUR - 1.0] {
        let after = (at - fixed) / HOUR;
        assert_eq!(c.alarm_at(at), None, "{after} h after the new tip");
    }
    let raised = c.digest_at(fixed + 4.0 * HOUR).alarm;
    let alarm = raised.expect("four hours over since the new tip").alarm;
    assert_eq!(alarm.since, fixed);
}

#[test]
fn adversarial_a_later_commit_keeps_the_end_of_rework_the_first_one_made() {
    let mut c = Case::new();
    c.ten_against_four();
    c.tip("sim/review-0", "asked", c.f.now());
    c.waits("sim/review-0", "asked: changes");
    // The first commit after the tip judged, five hours on, ends the rework: 10 against 4 again.
    c.f.advance(5.0 * HOUR);
    let fixed = c.f.now();
    c.tip("sim/review-0", "fixed", fixed);
    assert_eq!(c.alarm_at(fixed + 5.0 * HOUR), Some((10, 4)));
    let again = c.f.now();
    c.tip("sim/review-0", "fixed-again", again);
    assert_eq!(
        c.alarm_at(again),
        Some((10, 4)),
        "another commit moves that end never"
    );
}

#[test]
fn adversarial_a_tip_amended_away_keeps_the_end_the_board_learnt_whatever_follows() {
    let mut c = Case::new();
    c.ten_against_four();
    c.tip("sim/review-0", "asked", c.f.now());
    c.waits("sim/review-0", "asked: changes");
    // Amended five hours on: a branch that no longer holds the tip judged, only its tip now.
    c.f.advance(5.0 * HOUR);
    let amended = c.f.now();
    c.tips.remove("sim/review-0");
    c.tip("sim/review-0", "amended", amended);
    assert_eq!(c.alarm_at(amended + 5.0 * HOUR), Some((10, 4)));
    // Another commit, which the repository now answers in the amended one's place: the end the
    // board learnt stands.
    let later = c.f.now();
    c.tip("sim/review-0", "later", later);
    assert_eq!(c.alarm_at(later), Some((10, 4)), "the counts never fell");
}

#[test]
fn adversarial_a_new_record_of_changes_on_a_tip_the_branch_came_back_to_is_rework_of_its_own() {
    let back = answered_then_back("verdict-1");
    assert_eq!(
        back.first_end(),
        Some(back.fix()),
        "the first request's end"
    );
    back.c.waits("sim/review-0", "asked: changes, review");
    back.asked_again_clears_after_the_hour();
}

#[test]
fn adversarial_the_same_record_of_changes_from_a_fresh_verdict_on_a_tip_come_back_to_is_rework() {
    let back = answered_then_back("verdict-1");
    back.c
        .waits_of("sim/review-0", "asked: changes", "verdict-2");
    back.asked_again_clears_after_the_hour();
}

#[test]
fn adversarial_a_retried_record_on_a_tip_come_back_to_asks_no_rework_of_pull_or_the_alarm() {
    let back = answered_then_back("verdict-1");
    back.c
        .waits_of("sim/review-0", "asked: changes", "verdict-1");
    let d = back.c.digest_at(back.back + HOUR);
    let counted = (
        d.count("review_unreviewed"),
        d.count("in_progress_orphaned"),
    );
    assert_eq!(
        counted,
        (10, 4),
        "the request was answered: {:?}",
        d.lines()
    );
    assert_eq!(standing(&d), Some((10, 4)), "and the alarm counts it so");
    assert_eq!(back.first_end(), Some(back.fix()));
}

#[test]
fn overlapping_late_receipt_after_a_fix_and_return_keeps_ten_four_raised_seven_hours_later() {
    let mut c = Case::new();
    c.ten_against_four();
    let asked = c.f.now();
    c.tip("sim/review-0", "asked", asked);
    c.tip("sim/review-0", "fixed", asked + 1.0);
    c.tip("sim/review-0", "asked", asked + 2.0);
    let first = symphony_board::Store::unchecked_on(
        c.f.other_database(),
        Box::new(move || asked),
        Box::new(|_| false),
        crate::common::HOST,
        &symphony_board::Opening::write(),
    );
    c.f.advance(3.0);
    let late = c.f.late(move || {
        let task = first
            .record_waits("sim/review-0", "asked: changes", "verdict-1", "owner")
            .unwrap();
        let assessed = symphony_board::alarm::Assessed::of(&task).unwrap();
        let record = Written {
            tip: "asked",
            at: assessed.at,
            sid: assessed.sid.unwrap(),
        };
        let fixed = Tip {
            commit: "fixed".into(),
            at: asked + 1.0,
        };
        first.record_fixed("sim/review-0", record, &fixed).unwrap();
    });
    late.record_waits("sim/review-0", "asked: changes", "verdict-1", "owner")
        .unwrap();
    let d = c.digest_at(asked + 7.0 * HOUR);
    assert_eq!(
        (
            d.count("review_unreviewed"),
            d.count("in_progress_orphaned")
        ),
        (10, 4),
        "{:?}",
        d.lines()
    );
    assert_eq!(standing(&d), Some((10, 4)), "{:?}", d.lines());
    let task = c.f.store.get("sim/review-0").unwrap();
    assert_eq!(
        symphony_board::alarm::Assessed::of(&task).unwrap().at,
        asked
    );
    let occurrences =
        c.f.store
            .status_lines(asked)
            .unwrap()
            .into_iter()
            .filter(|line| line.slug == "sim/review-0" && line.text.ends_with("asked: changes"))
            .count();
    assert_eq!(occurrences, 1);
}

#[test]
fn same_time_occurrences_stay_distinct_in_the_digest_cache_and_before_its_window() {
    let mut c = Case::new();
    c.ten_against_four();
    let at = c.f.now();
    c.tip("sim/review-0", "asked", at);
    c.waits_of("sim/review-0", "asked: changes", "verdict-1");
    let first =
        symphony_board::alarm::Assessed::of(&c.f.store.get("sim/review-0").unwrap()).unwrap();
    let record = Written {
        tip: "asked",
        at,
        sid: first.sid.unwrap(),
    };
    let end = Tip {
        commit: "fixed".into(),
        at,
    };
    c.f.store
        .record_fixed("sim/review-0", record, &end)
        .unwrap();
    // A return and a second independent verdict at precisely the same permitted message time.
    c.tip("sim/review-0", "fixed", at);
    c.tip("sim/review-0", "asked", at);
    c.waits_of("sim/review-0", "asked: changes", "verdict-2");
    let next =
        symphony_board::alarm::Assessed::of(&c.f.store.get("sim/review-0").unwrap()).unwrap();
    assert_eq!(next.at, first.at);
    assert_ne!(next.sid, first.sid);
    for hours in [7.0, 26.0] {
        let d = c.digest_at(at + hours * HOUR);
        assert_eq!(
            (
                d.count("review_unreviewed"),
                d.count("in_progress_orphaned")
            ),
            (9, 5),
            "{:?}",
            d.lines()
        );
        assert_eq!(standing(&d), None, "{:?}", d.lines());
        let task = c.f.store.get("sim/review-0").unwrap();
        assert_eq!(
            symphony_board::alarm::kept_fix(&task, record),
            Some(end.clone())
        );
    }
}

#[test]
fn historical_timestamp_ends_need_a_unique_matching_thread_occurrence() {
    use serde_json::json;
    for evidence in ["unique", "ambiguous", "malformed", "empty"] {
        let c = Case::new();
        c.ten_against_four();
        let at = c.f.now();
        c.waits_of("sim/review-0", "asked: changes", "verdict-1");
        if evidence == "ambiguous" {
            c.waits_of("sim/review-0", "asked: changes", "verdict-2");
        }
        let saved = match evidence {
            "malformed" => json!({"commit": "fixed", "at": "bad"}),
            "empty" => json!({"commit": "", "at": at}),
            _ => json!({"commit": "fixed", "at": at}),
        };
        // A historical board kept no sequence beside the standing assessment, only time-keyed
        // saved ends. The real thread still contains the unique sequence of every occurrence.
        symphony_board::Raw::of(&c.f.other_database())
            .row("task", "sim/review-0")
            .set(json!({
                "fields._board.fixed": {"asked": {at.to_string(): saved}},
                "fields._board.assessed": {"id": "historical", "at": at},
            }))
            .unwrap();
        let d = c.digest_at(at + 26.0 * HOUR);
        let expected = if evidence == "unique" {
            (10, 4)
        } else {
            (9, 5)
        };
        assert_eq!(
            (
                d.count("review_unreviewed"),
                d.count("in_progress_orphaned")
            ),
            expected,
            "{evidence}: {:?}",
            d.lines()
        );
        assert_eq!(
            standing(&d),
            (evidence == "unique").then_some((10, 4)),
            "{evidence}: {:?}",
            d.lines()
        );
    }
}

#[test]
fn malformed_branch_evidence_never_answers_an_assessment() {
    let bad_ends = [
        Tip {
            commit: "fixed".into(),
            at: f64::NAN,
        },
        Tip {
            commit: String::new(),
            at: 0.0,
        },
    ];
    for bad_end in bad_ends {
        let c = Case::new();
        c.ten_against_four();
        c.waits_of("sim/review-0", "asked: changes", "verdict-1");
        c.f.advance(7.0 * HOUR);
        let tips = |_task: &symphony_board::Task, after: Option<Written<'_>>| {
            Some(if after.is_some() {
                bad_end.clone()
            } else {
                Tip {
                    commit: "asked".into(),
                    at: c.f.now(),
                }
            })
        };
        let d = digest_with_tips(&c.f.store, &[], None, Some(&tips), c.f.now(), &[]).unwrap();
        assert_eq!(
            (
                d.count("review_unreviewed"),
                d.count("in_progress_orphaned")
            ),
            (9, 5),
            "{bad_end:?}: {:?}",
            d.lines()
        );
        assert_eq!(standing(&d), None, "{bad_end:?}: {:?}", d.lines());
    }
}

/// A request of changes its branch answered, then came back from to the tip it judged.
struct Back {
    c: Case,
    /// When changes were asked of the tip `asked`.
    asked: f64,
    /// When the branch first moved past it, to `fixed`.
    fixed: f64,
    /// When it came back to `asked`.
    back: f64,
}

/// Changes asked of tip `asked` (the assessment `assessment` names), its fix five hours on, the
/// alarm raised at 10 against 4 five hours after that, and the branch back at `asked`.
fn answered_then_back(assessment: &str) -> Back {
    let mut c = Case::new();
    c.ten_against_four();
    let asked = c.f.now();
    c.tip("sim/review-0", "asked", asked);
    c.waits_of("sim/review-0", "asked: changes", assessment);
    c.f.advance(5.0 * HOUR);
    let fixed = c.f.now();
    c.tip("sim/review-0", "fixed", fixed);
    assert_eq!(c.alarm_at(fixed + 5.0 * HOUR), Some((10, 4)));
    let back = c.f.now();
    c.tip("sim/review-0", "asked", back);
    Back {
        c,
        asked,
        fixed,
        back,
    }
}

impl Back {
    /// The commit that answered the first request, and when.
    fn fix(&self) -> Tip {
        Tip {
            commit: "fixed".into(),
            at: self.fixed,
        }
    }

    /// Where the board keeps the first request's rework ended.
    fn first_end(&self) -> Option<Tip> {
        let task = self.c.f.store.get("sim/review-0").unwrap();
        let first = Written {
            tip: "asked",
            at: self.asked,
            sid: self
                .c
                .f
                .store
                .status_lines(self.asked)
                .unwrap()
                .into_iter()
                .find(|line| line.slug == "sim/review-0" && line.text.ends_with("asked: changes"))
                .unwrap()
                .sid,
        };
        symphony_board::alarm::kept_fix(&task, first)
    }

    /// Changes asked again on coming back: 9 against 5 in PULL and the alarm from then on, which
    /// clears after the hour, the first request keeping its end.
    fn asked_again_clears_after_the_hour(&self) {
        for at in [self.back, self.back + 0.5 * HOUR] {
            let on = (at - self.back) / HOUR;
            assert_eq!(
                self.c.alarm_at(at),
                Some((9, 5)),
                "{on} h into the new request"
            );
        }
        let d = self.c.digest_at(self.back + HOUR);
        let counted = (
            d.count("review_unreviewed"),
            d.count("in_progress_orphaned"),
        );
        assert_eq!(counted, (9, 5), "{:?}", d.lines());
        assert_eq!(
            standing(&d),
            None,
            "an hour at twice clears it: {:?}",
            d.lines()
        );
        assert_eq!(
            self.first_end(),
            Some(self.fix()),
            "the first request keeps its end"
        );
    }
}

#[test]
fn adversarial_rework_s_task_taken_from_review_keeps_the_commit_that_ended_it() {
    // The record of changes in the window, and before it.
    let rework = [5.0 * HOUR, 26.0 * HOUR];
    for lasted in rework {
        let mut c = Case::new();
        c.ten_against_four();
        c.tip("sim/review-0", "asked", c.f.now());
        c.waits("sim/review-0", "asked: changes");
        c.f.advance(lasted);
        let fixed = c.f.now();
        c.tip("sim/review-0", "fixed", fixed);
        assert_eq!(c.alarm_at(fixed + 5.0 * HOUR), Some((10, 4)));
        // Back into progress: 9 against 5, at most twice, for less than the hour that clears.
        let who = c.executor("sim/review-0");
        c.f.store
            .move_to("sim/review-0", "in_progress", &who)
            .unwrap();
        let taken = c.f.now();
        for at in [taken, taken + 0.5 * HOUR] {
            let on = (at - taken) / HOUR;
            assert_eq!(
                c.alarm_at(at),
                Some((9, 5)),
                "{on} h on, {lasted} s of rework"
            );
        }
        assert_eq!(c.alarm_at(taken + HOUR), None, "an hour at twice clears it");
    }
}
