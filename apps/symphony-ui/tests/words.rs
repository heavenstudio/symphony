//! The board's words: a register's moment, when a kept copy was saved and what a task page says
//! of the board. Home's greeting, ages and lead are the web client's
//! (apps/symphony-web/src/home.tsx).

use symphony_ui::words::{BoardStatus, Moment, device_notice, saved_at};

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
fn a_saved_copy_says_its_time_its_day_before_or_its_date() {
    let now = at("2026-11-11 19:40");
    assert_eq!(saved_at(at("2026-11-11 14:32"), now), "14:32");
    assert_eq!(
        saved_at(at("2026-11-11 00:05"), now),
        "00:05",
        "midnight's hour"
    );
    assert_eq!(
        saved_at(at("2026-11-11 19:40"), now),
        "19:40",
        "this minute"
    );
    assert_eq!(saved_at(at("2026-11-10 23:59"), now), "yesterday 23:59");
    assert_eq!(saved_at(at("2026-11-09 08:00"), now), "9 Nov 08:00");
    assert_eq!(saved_at(at("2026-03-01 12:00"), now), "1 Mar 12:00");
    assert_eq!(
        saved_at(at("2025-12-31 23:00"), now),
        "31 Dec 2025 23:00",
        "another year names it"
    );
    assert_eq!(
        saved_at(at("2026-01-01 00:00"), at("2026-01-01 08:00")),
        "00:00",
        "the first minute of a year"
    );
    assert_eq!(
        saved_at(at("2025-12-31 22:00"), at("2026-01-01 08:00")),
        "yesterday 22:00",
        "across a year"
    );
    assert_eq!(
        saved_at(at("2024-02-29 10:00"), at("2024-03-01 10:00")),
        "yesterday 10:00",
        "a leap day"
    );
    assert_eq!(
        saved_at(at("2024-02-29 10:00"), at("2024-03-02 10:00")),
        "29 Feb 10:00",
        "a leap day's date"
    );
    assert_eq!(
        saved_at(at("2026-11-12 09:00"), now),
        "12 Nov 09:00",
        "a clock behind the saved copy names its date, not a negative day"
    );
}

#[test]
fn a_page_from_this_device_says_it_checks_until_the_server_does_not_answer() {
    assert_eq!(
        device_notice("14:32", false),
        "Shown from this device, saved 14:32. Checking for changes…"
    );
    assert_eq!(
        device_notice("yesterday 14:32", true),
        "Not reachable: shown from this device, saved yesterday 14:32"
    );
}

#[test]
fn the_board_status_tells_a_slow_board_from_an_unavailable_one() {
    let late = BoardStatus {
        available: Some(false),
        slow: true,
    };
    assert_eq!(
        late.notice(),
        "The task board is slow to answer. Retrying…",
        "reached, but the answer came late"
    );
    let refused = BoardStatus {
        available: Some(false),
        slow: false,
    };
    assert_eq!(
        refused.notice(),
        "The task board is unavailable. Reconnecting…",
        "refused, or a server that does not say it is slow"
    );
    for slow in [false, true] {
        let back = BoardStatus {
            available: Some(true),
            slow,
        };
        assert_eq!(back.notice(), "", "{back:?}");
        let unsaid = BoardStatus {
            available: None,
            slow,
        };
        assert_eq!(unsaid.notice(), "", "{unsaid:?}");
    }
}
