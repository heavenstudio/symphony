use super::*;
use std::cell::Cell;

/// A condition that holds is returned at the first look.
#[test]
fn a_condition_that_holds_returns_at_its_first_look() {
    let looks = Cell::new(0);
    let value = until("ready", FLOOR, || {
        looks.set(looks.get() + 1);
        Some("ready")
    });
    assert_eq!((value.unwrap(), looks.get()), ("ready", 1));
}

/// A condition that holds at its fifth look returns its value then: the wait counts on the
/// condition, not on a time.
#[test]
fn a_condition_that_holds_at_a_later_look_returns_its_value_then() {
    let looks = Cell::new(0u64);
    let value = until("the fifth look", FLOOR, || {
        looks.set(looks.get() + 1);
        (looks.get() == 5).then_some(looks.get() * 10)
    });
    assert_eq!(value.unwrap(), 50);
    assert_eq!(looks.get(), 5);
}

/// A condition that never holds fails once its backstop has passed, after a last look past it,
/// with a stall that names the condition, how often and how long it looked, and the load.
#[test]
fn a_condition_that_never_holds_fails_at_its_backstop_saying_it_was_a_stall() {
    let backstop = Duration::from_millis(50);
    let looks = Cell::new(0u64);
    let stall = until_true("the ghost replaced", backstop, || {
        looks.set(looks.get() + 1);
        false
    })
    .unwrap_err();
    let said = stall.to_string();
    eprintln!("wait: {said}");
    assert!(stall.waited() >= backstop, "{:?}", stall.waited());
    assert_eq!(stall.looks(), looks.get());
    assert!(
        stall.looks() >= 2,
        "a look at once and one past the backstop: {said}"
    );
    let head = format!(
        "the ghost replaced: stalled: still not so after {} looks, and the wait's wall-clock backstop of 50.0ms ran out after ",
        looks.get()
    );
    assert!(said.starts_with(&head), "{said}");
    assert!(said.contains("; load average "), "{said}");
    assert_eq!(
        format!("{stall:?}"),
        said,
        "a stall unwrapped reads as it says"
    );
}

/// A backstop of none looks once.
#[test]
fn a_backstop_of_none_looks_once() {
    let looks = Cell::new(0u64);
    let stall = until_true("never", Duration::ZERO, || {
        looks.set(looks.get() + 1);
        false
    })
    .unwrap_err();
    assert_eq!((looks.get(), stall.looks()), (1, 1));
}

/// The backstop is six times the allowance, never under the floor, and never overflows.
#[test]
fn the_backstop_is_six_times_the_allowance_never_under_the_floor() {
    let secs = Duration::from_secs;
    assert_eq!(backstop(Duration::ZERO), FLOOR);
    assert_eq!(backstop(secs(10)), FLOOR);
    assert_eq!(backstop(secs(20)), FLOOR);
    assert_eq!(backstop(secs(21)), secs(126));
    assert_eq!(backstop(secs(30)), secs(180));
    assert_eq!(backstop(Duration::MAX), Duration::MAX);
}

/// A stall says the load average to two places, or that there was none to read.
#[test]
fn a_stall_says_the_load_or_that_there_was_none() {
    let stall = |load| Stall {
        what: "the board drawn".into(),
        looks: 1207,
        backstop: Duration::from_secs(180),
        waited: Duration::from_millis(180_040),
        load,
    };
    assert_eq!(
        stall(Some([312.5, 288.0, 190.25])).to_string(),
        "the board drawn: stalled: still not so after 1207 looks, and the wait's wall-clock backstop of 180.0s ran out after 180.0s; load average 312.50 288.00 190.25 over 1, 5 and 15 minutes"
    );
    assert_eq!(
        stall(None).to_string(),
        "the board drawn: stalled: still not so after 1207 looks, and the wait's wall-clock backstop of 180.0s ran out after 180.0s; no load average to read"
    );
}

/// This machine's load average is three readings, none below none.
#[test]
fn the_load_average_is_three_readings_at_or_above_none() {
    let [one, five, fifteen] = load().expect("a load average");
    for reading in [one, five, fifteen] {
        assert!(reading.is_finite() && reading >= 0.0, "{reading}");
    }
}
