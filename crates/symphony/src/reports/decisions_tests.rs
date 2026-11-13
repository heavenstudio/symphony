//! A card's history read back hour by hour: who took it out of the owner's queue, how and when (the
//! owner's answer, an auto-approval, a supersession or another agent's line withdrawing it), how
//! long the cards still in it have waited, and what a card with no dated line, a line continuing
//! the one above, a stamp naming no minute and a line dated before the one above count as.

use super::*;
use crate::board::Doc;
use crate::clock::stamp_at;
use crate::register::parse_decisions;
use crate::reports::{HOUR, day_start};
use symphony_api::reports::ReportHour;

/// A UTC midnight in January, far from any change of clocks.
fn midnight() -> f64 {
    day_start(1_800_000_000.0)
}

/// The local stamp of `hours` after midnight, as a card's line writes it.
fn at(hours: f64) -> String {
    #[allow(clippy::cast_possible_truncation)]
    stamp_at((midnight() + hours * HOUR) as libc::time_t)
}

/// A card `slug` marked `mark`, a line for each of `lines`: the hours after midnight it was written
/// at, and what follows its stamp.
fn card(slug: &str, mark: char, lines: &[(f64, &str)]) -> Vec<String> {
    let mut text = vec![format!("- [{mark}] **{slug}** — Is this it?")];
    text.extend(
        lines
            .iter()
            .map(|(hours, rest)| format!("  [{}] {rest}", at(*hours))),
    );
    text
}

/// `cards` as one register document.
fn doc(cards: &[Vec<String>]) -> Doc {
    let mut lines = vec!["### Topic".to_string()];
    for card in cards {
        lines.extend(card.iter().cloned());
    }
    Doc {
        path: "docs/DECISIONS.md".into(),
        comments: Vec::new(),
        decisions: parse_decisions(&lines),
    }
}

/// A day file with its hours up to `until` hours after midnight, filled from `docs`.
fn filled(until: f64, docs: &[Doc]) -> ReportDay {
    let start = midnight();
    let mut day = ReportDay {
        start,
        until: start + until * HOUR,
        hours: (0..24u32)
            .take_while(|hour| *hour == 0 || f64::from(*hour) < until)
            .map(|hour| ReportHour {
                start: start + f64::from(hour) * HOUR,
                ..ReportHour::default()
            })
            .collect(),
        ..ReportDay::default()
    };
    fill(&mut day, docs, &Plugins::new(Vec::new()));
    day
}

/// Each hour's open, answered, auto-approved and withdrawn counts.
fn counts(day: &ReportDay) -> Vec<(u32, u32, u32, u32)> {
    day.hours
        .iter()
        .map(|hour| {
            let counts = hour.decisions.unwrap();
            (
                counts.open,
                counts.answered,
                counts.auto_approved,
                counts.withdrawn,
            )
        })
        .collect()
}

fn answers(list: &[Answer]) -> Vec<(&str, f64, &str)> {
    list.iter()
        .map(|a| (a.slug.as_str(), (a.at - midnight()) / HOUR, a.how.as_str()))
        .collect()
}

#[test]
fn a_card_the_owner_approves_waits_until_the_owner_s_line_and_is_answered_then() {
    let docs = [doc(&[card(
        "pick-one",
        'x',
        &[(1.25, "agent — Posted."), (3.5, "owner · approved")],
    )])];
    let day = filled(5.0, &docs);
    assert_eq!(
        counts(&day),
        [
            (0, 0, 0, 0),
            (1, 0, 0, 0),
            (1, 0, 0, 0),
            (0, 1, 0, 0),
            (0, 0, 0, 0)
        ],
        "posted at 01:15, open at the end of hours 1 and 2, answered in hour 3"
    );
    let decisions = day.decisions.unwrap();
    assert!(decisions.open.is_empty());
    assert_eq!(
        answers(&decisions.answered),
        [("pick-one", 3.5, "approved")]
    );
    assert!(decisions.auto_approved.is_empty() && decisions.withdrawn.is_empty());
}

#[test]
fn the_lists_run_by_time_then_slug_across_documents_whatever_their_order() {
    let open = |slug: &str, hours: f64| card(slug, ' ', &[(hours, "agent — Posted.")]);
    let answered = |slug: &str, hours: f64| {
        card(
            slug,
            'x',
            &[(0.5, "agent — Posted."), (hours, "owner · approved")],
        )
    };
    let mut other = doc(&[
        open("late", 3.0),
        answered("b-then", 2.5),
        card("undated", ' ', &[]),
    ]);
    other.path = "docs/OTHER.md".into();
    let docs = [
        doc(&[
            open("early", 1.0),
            answered("c-last", 4.0),
            open("a-tie", 3.0),
        ]),
        other,
        doc(&[answered("a-first", 2.5)]),
    ];
    let day = filled(5.0, &docs);
    let decisions = day.decisions.unwrap();
    let open: Vec<(&str, Option<f64>)> = decisions
        .open
        .iter()
        .map(|card| {
            (
                card.slug.as_str(),
                card.since.map(|s| (s - midnight()) / HOUR),
            )
        })
        .collect();
    assert_eq!(
        open,
        [
            ("undated", None),
            ("early", Some(1.0)),
            ("a-tie", Some(3.0)),
            ("late", Some(3.0)),
        ],
        "a card waiting since no line first, then by since and slug"
    );
    assert_eq!(
        answers(&decisions.answered),
        [
            ("a-first", 2.5, "approved"),
            ("b-then", 2.5, "approved"),
            ("c-last", 4.0, "approved"),
        ]
    );
}

#[test]
fn an_auto_approval_counts_apart_and_a_card_posted_resolved_was_never_asked() {
    let docs = [doc(&[
        card(
            "auto",
            'a',
            &[
                (0.5, "agent — Posted."),
                (
                    2.0,
                    "agent · auto-approved — the owner approved its like twice",
                ),
            ],
        ),
        card(
            "posted-decided",
            'a',
            &[(
                1.5,
                "agent · auto-approved — the owner's verdict on `pick` decides it",
            )],
        ),
        card(
            "settled",
            'r',
            &[(1.0, "agent · resolved — the owner's verdict on it stands")],
        ),
    ])];
    let day = filled(3.0, &docs);
    assert_eq!(
        counts(&day),
        [(1, 0, 0, 0), (1, 0, 1, 0), (0, 0, 1, 0)],
        "`auto` from 00:30 until its auto-approval at 02:00, `posted-decided` auto-approved as it \
         was posted, and `settled`, posted resolved, in no count"
    );
    let decisions = day.decisions.unwrap();
    assert_eq!(
        answers(&decisions.auto_approved),
        [
            ("posted-decided", 1.5, "auto-approved"),
            ("auto", 2.0, "auto-approved")
        ]
    );
    assert!(decisions.answered.is_empty() && decisions.withdrawn.is_empty());
    assert!(decisions.open.is_empty());
}

#[test]
fn a_superseded_card_is_withdrawn_and_its_replacement_answered_by_the_owner() {
    let docs = [doc(&[
        card(
            "old-ask",
            '~',
            &[
                (0.0, "agent — Posted."),
                (1.5, "agent · dismissed — superseded by **new-ask**"),
            ],
        ),
        card(
            "new-ask",
            'x',
            &[(1.5, "agent — Posted."), (2.5, "owner · approved — B")],
        ),
    ])];
    let day = filled(3.0, &docs);
    assert_eq!(
        counts(&day),
        [(1, 0, 0, 0), (1, 0, 0, 1), (0, 1, 0, 0)],
        "the supersession at 01:30 decided nothing: withdrawn, no auto-approval; the owner \
         answered the new card at 02:30"
    );
    let decisions = day.decisions.unwrap();
    assert_eq!(
        answers(&decisions.withdrawn),
        [("old-ask", 1.5, "dismissed")]
    );
    assert_eq!(answers(&decisions.answered), [("new-ask", 2.5, "approved")]);
    assert!(decisions.auto_approved.is_empty());
}

#[test]
fn an_agent_s_question_waits_on_the_owner_until_an_agent_withdraws_it() {
    let docs = [doc(&[card(
        "asked",
        'x',
        &[
            (0.0, "agent — Posted."),
            (0.5, "owner · approved"),
            (1.5, "agent · question — the note says both; which one?"),
            (2.5, "agent — Withdrawn: the earlier verdict says which."),
        ],
    )])];
    let day = filled(3.0, &docs);
    assert_eq!(
        counts(&day),
        [(0, 1, 0, 0), (1, 0, 0, 0), (0, 0, 0, 1)],
        "the owner answered it in hour 0, the agent's question put it back in hour 1, and an \
         agent's line withdrew it in hour 2"
    );
    let decisions = day.decisions.unwrap();
    assert_eq!(answers(&decisions.answered), [("asked", 0.5, "approved")]);
    assert_eq!(answers(&decisions.withdrawn), [("asked", 2.5, "replied")]);
}

#[test]
fn an_answer_recorded_on_the_card_the_owner_approved_is_counted_nowhere() {
    let docs = [doc(&[card(
        "approved-before",
        'x',
        &[
            (-3.0, "agent — Posted."),
            (-2.0, "owner · approved"),
            (
                1.0,
                "agent — Recorded as answered: the owner's words here decide `later-ask`.",
            ),
        ],
    )])];
    let day = filled(2.0, &docs);
    assert_eq!(
        counts(&day),
        [(0, 0, 0, 0), (0, 0, 0, 0)],
        "the card stays resolved before and after the line"
    );
}

#[test]
fn an_owner_s_question_answered_by_an_agent_waits_again_to_be_confirmed_since_the_answer() {
    let docs = [doc(&[card(
        "why",
        '?',
        &[
            (0.25, "agent — Posted."),
            (1.0, "owner · question — why this one?"),
            (2.0, "agent — Because the other breaks the phone layout."),
        ],
    )])];
    let day = filled(4.0, &docs);
    assert_eq!(
        counts(&day),
        [(1, 0, 0, 0), (0, 1, 0, 0), (1, 0, 0, 0), (1, 0, 0, 0)]
    );
    let decisions = day.decisions.unwrap();
    assert_eq!(answers(&decisions.answered), [("why", 1.0, "question")]);
    assert_eq!(decisions.open.len(), 1);
    assert_eq!(
        decisions.open[0].since,
        Some(midnight() + 2.0 * HOUR),
        "it has waited since the agent's answer put it back, not since it was posted"
    );
}

#[test]
fn an_owner_s_comment_takes_a_card_out_until_an_agent_answers_it() {
    let docs = [doc(&[card(
        "words",
        ' ',
        &[
            (0.0, "agent — Posted."),
            (1.0, "owner — shorter words, please"),
            (2.5, "agent — Shortened."),
        ],
    )])];
    let day = filled(3.0, &docs);
    assert_eq!(counts(&day), [(1, 0, 0, 0), (0, 1, 0, 0), (1, 0, 0, 0)]);
    let decisions = day.decisions.unwrap();
    assert_eq!(answers(&decisions.answered), [("words", 1.0, "commented")]);
    assert_eq!(decisions.open[0].since, Some(midnight() + 2.5 * HOUR));
}

#[test]
fn a_card_with_no_dated_line_counts_as_it_stands_at_every_hour_with_no_wait_to_tell() {
    let mut old = vec!["- [ ] **old** — From before lines were dated.".to_string()];
    old.push("  ↳ an agent's undated answer".to_string());
    let docs = [doc(&[old, card("later", ' ', &[(1.0, "agent — Posted.")])])];
    let day = filled(2.0, &docs);
    assert_eq!(
        counts(&day),
        [(1, 0, 0, 0), (2, 0, 0, 0)],
        "the undated card at every hour; the later one from the hour it was posted"
    );
    let open = day.decisions.unwrap().open;
    assert_eq!(
        open.iter()
            .map(|card| (card.slug.as_str(), card.since))
            .collect::<Vec<_>>(),
        [("old", None), ("later", Some(midnight() + HOUR))],
        "no history first, then the longest waiting"
    );
}

#[test]
fn a_deeper_line_continues_the_one_above_and_a_stamp_naming_no_minute_is_no_line() {
    let mut lines = card(
        "deep",
        'x',
        &[(0.0, "agent — Posted."), (2.0, "owner · approved")],
    );
    // Under the agent's line, an indented line reading as a dated owner verdict continues it.
    lines.insert(2, format!("      [{}] owner · approved", at(1.0)));
    // The hidden line pairing an owner's entry with a reply written without seeing it is no entry.
    lines.push(
        symphony_docs::syntax::unseen_owner_line("owner-key", "reply-key")
            .trim_end()
            .to_string(),
    );
    // A day that does not exist: a dated line for the register, no minute for the history.
    lines.push("  [2027-02-30 10:00] agent — Never.".to_string());
    let docs = [doc(&[lines])];
    let day = filled(3.0, &docs);
    assert_eq!(
        counts(&day),
        [(1, 0, 0, 0), (1, 0, 0, 0), (0, 1, 0, 0)],
        "the continuation is no answer; the owner's own line in hour 2 is"
    );
}

#[test]
fn a_line_dated_before_the_one_above_is_read_at_the_one_above_s_minute() {
    let docs = [doc(&[card(
        "skew",
        'x',
        &[(2.0, "agent — Posted."), (1.0, "owner · approved")],
    )])];
    let day = filled(3.0, &docs);
    assert_eq!(counts(&day), [(0, 0, 0, 0), (0, 0, 0, 0), (0, 1, 0, 0)]);
    assert_eq!(
        answers(&day.decisions.unwrap().answered),
        [("skew", 2.0, "approved")]
    );
}

#[test]
fn what_was_answered_before_the_day_counts_for_its_state_but_not_in_its_lists() {
    let docs = [doc(&[
        card(
            "yesterday",
            'x',
            &[(-20.0, "agent — Posted."), (-10.0, "owner · approved")],
        ),
        card("waiting", ' ', &[(-5.0, "agent — Posted.")]),
    ])];
    let day = filled(1.0, &docs);
    assert_eq!(counts(&day), [(1, 0, 0, 0)]);
    let decisions = day.decisions.unwrap();
    assert!(decisions.answered.is_empty());
    assert_eq!(decisions.open[0].since, Some(midnight() - 5.0 * HOUR));
}

#[test]
fn the_hour_under_way_counts_up_to_the_read_and_the_lists_stop_there() {
    let docs = [doc(&[card(
        "soon",
        'x',
        &[(0.0, "agent — Posted."), (0.75, "owner · approved")],
    )])];
    let day = filled(0.5, &docs);
    assert_eq!(
        counts(&day),
        [(1, 0, 0, 0)],
        "read at 00:30: not answered yet"
    );
    let decisions = day.decisions.unwrap();
    assert!(decisions.answered.is_empty());
    assert_eq!(decisions.open[0].slug, "soon");
}

#[test]
fn the_mark_of_a_card_s_lines_is_their_last_verdict_an_agent_s_question_setting_none() {
    let lines = |rest: &[&str]| -> Vec<String> {
        rest.iter()
            .map(|rest| format!("  [{}] {rest}", at(0.0)))
            .collect()
    };
    assert_eq!(mark(&lines(&["agent — Posted."])), State::Pending);
    assert_eq!(
        mark(&lines(&[
            "owner · rejected — no",
            "agent · question — sure?"
        ])),
        State::Rejected
    );
    assert_eq!(
        mark(&lines(&["owner · question — why?", "agent · resolved"])),
        State::Resolved
    );
    assert_eq!(
        mark(&lines(&["owner · approved", "owner · fine"])),
        State::Approved,
        "a word that is no state sets none"
    );
}
