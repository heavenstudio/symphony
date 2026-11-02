//! HistoryTest and PostedAtTest: each card carries a dated, append-only history under it.

use symphony::register::{State, Who};

use crate::{card, entry};

#[test]
fn entries_parse_time_author_verdict_and_text() {
    let entries = card(&[
        "- [-] **a** — one.",
        "  [2026-09-24 14:02] owner · rejected — needs better names",
        "  [2026-09-24 15:10] agent — Options are in **b**.",
        "    Second line of the reply.",
        "  [2026-09-25] owner · approved",
    ])
    .entries();
    assert_eq!(
        entries
            .iter()
            .map(|e| (e.ts.as_deref(), e.who, e.verdict.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            (Some("2026-09-24 14:02"), Who::Owner, Some("rejected")),
            (Some("2026-09-24 15:10"), Who::Agent, None),
            (Some("2026-09-25"), Who::Owner, Some("approved")),
        ]
    );
    assert_eq!(entries[0].text, "needs better names");
    assert_eq!(
        entries[1].text,
        "Options are in **b**.\nSecond line of the reply."
    );
    assert_eq!(entries[2].text, "");
}

#[test]
fn legacy_lines_become_undated_entries() {
    let entries = card(&["- [x] **a** — 1", "  owner words", "  ↳ agent words"]).entries();
    assert_eq!(
        entries,
        vec![
            entry(None, Who::Owner, None, "owner words"),
            entry(None, Who::Agent, None, "agent words")
        ]
    );
}

#[test]
fn resolved_marker() {
    assert_eq!(card(&["- [r] **a** — 1"]).state, State::Resolved);
}

#[test]
fn dismissed_marker() {
    assert_eq!(card(&["- [~] **a** — 1"]).state, State::Dismissed);
}

#[test]
fn decided_at_is_the_last_owner_verdict() {
    let lines = [
        "- [x] **a** — 1",
        "  [2026-09-24 10:00] owner · rejected — no",
        "  [2026-09-24 11:00] agent — changed",
        "  [2026-09-25 09:30] owner · approved",
    ];
    assert_eq!(
        card(&lines).decided_at().as_deref(),
        Some("2026-09-25 09:30")
    );
    assert_eq!(card(&["- [ ] **b** — 2"]).decided_at(), None);
}

#[test]
fn a_card_was_posted_at_its_first_dated_line() {
    let found = card(&[
        "- [ ] **a** — new",
        "  owner words",
        "  [2026-09-25 10:00] agent — ![x](/x.png)",
        "  [2026-09-25 11:00] owner · approved",
    ]);
    assert_eq!(found.posted_at().as_deref(), Some("2026-09-25 10:00"));
}

#[test]
fn a_card_with_no_dated_line_has_no_posting_time() {
    assert_eq!(card(&["- [ ] **a** — new"]).posted_at(), None);
}
