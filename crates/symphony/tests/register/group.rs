//! GroupTest: where a card sits, and when a tidy may resolve it.

use symphony::register::{Group, resolve};

use crate::card;
use crate::common::{plain, star_foundry};

fn group(lines: &[&str]) -> Group {
    card(lines).group(&plain())
}

fn resolvable(lines: &[&str]) -> bool {
    card(lines).resolvable(&plain())
}

const QUESTION: [&str; 3] = [
    "- [x] **a** — 1",
    "  [2026-10-03 08:02] owner · approved — go with the base one",
    "  [2026-10-03 08:21] agent · question — Which picture is the base one?",
];

fn question_and(more: &[&'static str]) -> Vec<&'static str> {
    let mut lines = QUESTION.to_vec();
    lines.extend_from_slice(more);
    lines
}

#[test]
fn new_card_needs_the_owner() {
    assert_eq!(group(&["- [ ] **a** — 1"]), Group::Decide);
}

#[test]
fn an_answered_approval_is_settled_and_an_answered_question_waits_to_confirm() {
    // An approval's note is an instruction: once an agent has acted on it, the card is done.
    assert_eq!(
        group(&[
            "- [x] **a** — 1",
            "  [2026-09-24] owner · approved — at least 21",
            "  [2026-09-24] agent — Recorded as a floor."
        ]),
        Group::Resolved
    );
    assert_eq!(
        group(&[
            "- [?] **a** — 1",
            "  [2026-09-24] owner · question — what is a pod?",
            "  [2026-09-24] agent — A bolt-on module."
        ]),
        Group::Confirm
    );
}

#[test]
fn a_pending_card_is_decide_even_with_an_agent_note() {
    assert_eq!(
        group(&["- [ ] **a** — 1", "  [2026-09-24] agent — revised: shorter"]),
        Group::Decide
    );
}

#[test]
fn group_order_puts_new_decisions_first_then_answers_to_confirm() {
    assert_eq!(
        Group::ALL.iter().map(|g| g.key()).collect::<Vec<_>>(),
        vec!["decide", "auto", "confirm", "agent", "rejected", "resolved"]
    );
}

#[test]
fn owner_words_wait_on_an_agent() {
    assert_eq!(
        group(&[
            "- [x] **a** — 1",
            "  [2026-09-24] owner · approved — at least 21"
        ]),
        Group::Agent
    );
    assert_eq!(
        group(&["- [-] **a** — 1", "  [2026-09-24] owner · rejected"]),
        Group::Agent
    );
    assert_eq!(group(&["- [?] **a** — 1"]), Group::Agent);
}

#[test]
fn a_dismissed_card_is_resolved_and_asks_nothing_of_an_agent() {
    let found = card(&["- [~] **a** — 1", "  [2026-09-25 15:40] owner · dismissed"]);
    assert_eq!(found.group(&plain()), Group::Resolved);
    assert!(!found.open(&plain()));
}

#[test]
fn a_dismissal_with_a_comment_still_waits_for_a_reply() {
    let found = card(&[
        "- [~] **a** — 1",
        "  [2026-09-25 15:40] owner · dismissed — duplicate of b",
    ]);
    assert!(found.open(&plain()));
    assert_eq!(found.group(&plain()), Group::Agent);
}

#[test]
fn rejected_and_answered_is_closed() {
    assert_eq!(
        group(&[
            "- [-] **a** — 1",
            "  [2026-09-24] owner · rejected — no",
            "  [2026-09-24] agent — Replaced by **b**."
        ]),
        Group::Rejected
    );
}

#[test]
fn plain_approval_and_resolved_are_resolved() {
    assert_eq!(group(&["- [x] **a** — 1"]), Group::Resolved);
    assert_eq!(
        group(&["- [x] **a** — 1", "  [2026-09-24] owner · approved"]),
        Group::Resolved
    );
    assert_eq!(
        group(&[
            "- [x] **a** — 1",
            "  [2026-09-24] owner · approved — hm",
            "  [2026-09-24] agent — done",
            "  [2026-09-25] owner · approved",
        ]),
        Group::Resolved
    );
    assert_eq!(
        group(&["- [r] **a** — 1", "  [2026-09-25] agent · resolved"]),
        Group::Resolved
    );
}

#[test]
fn golden_approvals_wait_for_application_and_survive_tidy() {
    let head = "- [x] **golden-change** — Review the captured frame. *Golden:* frame";
    let approved = "  [2026-10-28 12:00] owner · approved";
    let lines = [head, approved];
    let sf = &star_foundry();
    let found = card(&lines);
    assert_eq!(found.group(sf), Group::Agent);
    assert!(found.open(sf));
    assert!(!found.resolvable(sf));
    let (tidied, changed) = resolve(&(lines.join("\n") + "\n"), "2026-10-28 12:05", sf);
    assert!(changed.is_empty());
    assert!(tidied.contains("- [x] **golden-change**"));
    let applied = card(&[
        head,
        approved,
        "  [2026-10-28 12:10] agent — Blessed the reviewed frame.",
    ]);
    assert_eq!(applied.group(sf), Group::Resolved);
    assert!(!applied.open(sf));
    assert!(applied.resolvable(sf));
    for (mark, verdict, group) in [
        ("-", "rejected", Group::Agent),
        ("?", "question", Group::Agent),
        ("~", "dismissed", Group::Resolved),
        ("r", "resolved", Group::Resolved),
    ] {
        let head = format!("- [{mark}] **golden-change** — Frame. *Golden:* frame");
        let owner = format!("  [2026-10-28 12:00] owner · {verdict}");
        assert_eq!(card(&[&head, &owner]).group(sf), group, "{verdict}");
    }
}

#[test]
fn a_project_without_the_golden_frame_kind_settles_an_approved_golden_card() {
    let lines = [
        "- [x] **golden-change** — Review the captured frame. *Golden:* frame",
        "  [2026-10-28 12:00] owner · approved",
    ];
    let found = card(&lines);
    assert_eq!(found.group(&plain()), Group::Resolved);
    assert!(!found.open(&plain()));
    assert!(found.resolvable(&plain()));
    let (tidied, changed) = resolve(&(lines.join("\n") + "\n"), "2026-10-28 12:05", &plain());
    assert_eq!(changed, ["golden-change"]);
    assert!(tidied.contains("- [r] **golden-change**"), "{tidied}");
}

#[test]
fn an_agent_question_under_an_approval_keeps_the_card_open_for_the_owner() {
    let found = card(&QUESTION);
    assert!(!found.resolvable(&plain()));
    assert_eq!(found.group(&plain()), Group::Decide);
    assert_eq!(
        found.entries().last().unwrap().verdict.as_deref(),
        Some("question")
    );
}

#[test]
fn an_agent_question_is_not_tidied() {
    let (text, changed) = resolve(&(QUESTION.join("\n") + "\n"), "2026-10-03 09:00", &plain());
    assert!(changed.is_empty());
    assert!(text.contains("- [x] **a**"), "{text}");
    assert!(!text.contains("resolved"), "{text}");
}

#[test]
fn a_plain_agent_line_still_resolves_an_approval() {
    let lines = [
        QUESTION[0],
        QUESTION[1],
        "  [2026-10-03 08:21] agent — Done, the base one is C.",
    ];
    let found = card(&lines);
    assert!(found.resolvable(&plain()));
    assert_eq!(found.group(&plain()), Group::Resolved);
    assert_eq!(
        resolve(&(lines.join("\n") + "\n"), "2026-10-03 09:00", &plain()).1,
        vec!["a"]
    );
}

#[test]
fn a_plain_agent_line_ending_in_a_question_mark_is_still_an_answer() {
    let found = card(&[
        QUESTION[0],
        QUESTION[1],
        "  [2026-10-03 08:21] agent — Recorded as C, is that what you meant?",
    ]);
    assert!(!found.asks_owner());
    assert!(found.resolvable(&plain()));
    assert_eq!(found.group(&plain()), Group::Resolved);
}

#[test]
fn a_later_plain_agent_line_withdraws_the_question() {
    let found = card(&question_and(&[
        "  [2026-10-03 08:40] agent — Found it myself: C.",
    ]));
    assert!(!found.asks_owner());
    assert!(found.resolvable(&plain()));
    assert_eq!(found.group(&plain()), Group::Resolved);
}

#[test]
fn a_later_resolved_line_does_not_withdraw_it() {
    assert!(!card(&question_and(&["  [2026-10-03 08:40] agent · resolved"])).asks_owner());
}

#[test]
fn the_owner_answering_the_question_reopens_it_for_the_agent_then_resolves() {
    let answered = question_and(&["  [2026-10-03 09:00] owner — C"]);
    let found = card(&answered);
    assert!(!found.resolvable(&plain()));
    assert_eq!(found.group(&plain()), Group::Agent);
    let mut done = answered.clone();
    done.push("  [2026-10-03 09:05] agent — Recorded C.");
    assert!(card(&done).resolvable(&plain()));
}

#[test]
fn a_plain_reapproval_after_the_question_resolves() {
    assert!(resolvable(&question_and(&[
        "  [2026-10-03 09:00] owner · approved"
    ])));
}

#[test]
fn an_agent_question_needs_the_owner_on_any_card_state() {
    for mark in [" ", "-", "?"] {
        let head = format!("- [{mark}] **a** — 1");
        assert_eq!(
            group(&[&head, "  [2026-10-03 08:21] agent · question — Which one?"]),
            Group::Decide,
            "{mark}"
        );
    }
}

#[test]
fn a_question_on_a_resolved_or_dismissed_card_does_not_reopen_it() {
    for mark in ["r", "~"] {
        let head = format!("- [{mark}] **a** — 1");
        assert_eq!(
            group(&[&head, "  [2026-10-03 08:21] agent · question — Which one?"]),
            Group::Resolved,
            "{mark}"
        );
    }
}

#[test]
fn resolvable_only_when_the_owner_had_the_last_word_and_it_was_a_plain_approval() {
    assert!(resolvable(&["- [x] **a** — 1"]));
    assert!(resolvable(&[
        "- [x] **a** — 1",
        "  [2026-09-24] owner · approved — x",
        "  [2026-09-24] agent — y",
        "  [2026-09-25] owner · approved",
    ]));
    assert!(!resolvable(&[
        "- [x] **a** — 1",
        "  [2026-09-24] owner · approved — x"
    ]));
    assert!(resolvable(&[
        "- [x] **a** — 1",
        "  [2026-09-24] owner · approved — x",
        "  [2026-09-24] agent — y"
    ]));
    assert!(!resolvable(&[
        "- [x] **a** — 1",
        "  [2026-09-24] owner · approved — x",
        "  [2026-09-24] agent — y",
        "  [2026-09-24] owner — one more thing",
    ]));
    assert!(!resolvable(&["- [r] **a** — 1"]));
    assert!(!resolvable(&["- [ ] **a** — 1"]));
}

#[test]
fn a_comment_nobody_answered_blocks_resolving_even_after_a_plain_reapproval() {
    let found = card(&[
        "- [x] **a** — 1",
        "  [2026-09-24 10:00] owner · approved — at least 21",
        "  [2026-09-24 10:05] owner · approved",
    ]);
    assert!(!found.resolvable(&plain()));
    assert!(found.open(&plain()));
    assert_eq!(found.group(&plain()), Group::Agent);
    assert_eq!(found.unanswered(), vec!["at least 21"]);
}

#[test]
fn a_comment_answered_then_reapproved_resolves() {
    let found = card(&[
        "- [x] **a** — 1",
        "  [2026-09-24 10:00] owner · approved — at least 21",
        "  [2026-09-24 11:00] agent — done",
        "  [2026-09-24 12:00] owner · approved",
    ]);
    assert!(found.resolvable(&plain()));
    assert!(found.unanswered().is_empty());
}

#[test]
fn open_means_agent_work() {
    let found = crate::cards(&[
        "- [x] **a** — 1",
        "  [2026-09-24] owner · approved — note",
        "- [x] **b** — 2",
        "  [2026-09-24] owner · approved — note",
        "  [2026-09-24] agent — reply",
    ]);
    assert_eq!(
        found.iter().map(|c| c.open(&plain())).collect::<Vec<_>>(),
        vec![true, false]
    );
}
