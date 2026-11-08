//! The board's words: a register's moment, the greeting, an age, the lead line and what a task
//! page says of the board.

use symphony_ui::words::{Moment, age, board_status, greeting, lead};

fn at(text: &str) -> Moment {
    Moment::parse(text).unwrap_or_else(|| panic!("{text} does not parse"))
}

#[test]
fn a_moment_reads_a_registers_date_with_or_without_its_time() {
    assert_eq!(
        at("2026-10-08 07:42").minutes() - at("2026-10-08").minutes(),
        7 * 60 + 42
    );
    assert_eq!(
        at("2026-10-08").minutes() - at("2026-10-07").minutes(),
        24 * 60
    );
    assert_eq!(
        at("2025-01-01").minutes() - at("2024-12-31").minutes(),
        24 * 60,
        "across a year"
    );
    assert_eq!(
        at("2024-03-01").minutes() - at("2024-02-28").minutes(),
        2 * 24 * 60,
        "a leap day"
    );
    for bad in [
        "",
        "yesterday",
        "2026-13-01",
        "2026-10-08 25:00",
        "2026-10-08T07:42",
    ] {
        assert_eq!(Moment::parse(bad), None, "{bad:?}");
    }
}

#[test]
fn the_greeting_follows_the_hour() {
    assert_eq!(greeting(at("2026-10-08 00:00")), "Good morning");
    assert_eq!(greeting(at("2026-10-08 11:59")), "Good morning");
    assert_eq!(greeting(at("2026-10-08 12:00")), "Good afternoon");
    assert_eq!(greeting(at("2026-10-08 17:59")), "Good afternoon");
    assert_eq!(greeting(at("2026-10-08 18:00")), "Good evening");
}

#[test]
fn an_age_is_new_then_hours_then_days() {
    let now = at("2026-10-08 12:00");
    assert_eq!(age(Some("2026-10-08 11:30"), now), "new");
    assert_eq!(age(Some("2026-10-08 09:00"), now), "3 h");
    assert_eq!(age(Some("2026-10-07 12:00"), now), "1 d");
    assert_eq!(age(Some("2026-10-01"), now), "7 d");
    assert_eq!(
        age(Some("2026-10-09 08:00"), now),
        "new",
        "a clock behind the register is not negative"
    );
    assert_eq!(age(Some("soon"), now), "");
    assert_eq!(age(None, now), "");
}

#[test]
fn the_lead_counts_cards_and_documents_in_the_singular_and_plural() {
    assert_eq!(
        lead(37, 9),
        "37 cards wait on you across 9 documents. Start with the one below."
    );
    assert_eq!(
        lead(1, 1),
        "1 card waits on you across 1 document. Start with the one below."
    );
    assert_eq!(lead(0, 0), "Your queue is empty.");
}

#[test]
fn the_board_status_tells_a_slow_board_from_an_unavailable_one() {
    assert_eq!(
        board_status(Some(false), Some(true)),
        "The task board is slow to answer. Retrying…",
        "reached, but the answer came late"
    );
    for slow in [Some(false), None] {
        assert_eq!(
            board_status(Some(false), slow),
            "The task board is unavailable. Reconnecting…",
            "refused, or a server that does not say: {slow:?}"
        );
    }
    for slow in [None, Some(false), Some(true)] {
        assert_eq!(board_status(Some(true), slow), "", "back: {slow:?}");
        assert_eq!(board_status(None, slow), "", "no availability: {slow:?}");
    }
}
