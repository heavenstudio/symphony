//! The sessions: a session holds twelve hours, ends at sign-out, and is found by its id alone.

use super::*;

const NOW: u64 = 1_800_000_000;

fn ana() -> Who {
    Who {
        service: "google",
        subject: "110248495921238986420".into(),
    }
}

/// A session is found by its id for twelve hours, to the second, and not by any other id.
#[test]
fn a_session_holds_twelve_hours_by_its_id_alone() {
    let mut kept = Kept::default();
    let id = kept.open(ana(), NOW).unwrap();
    assert_eq!(id.len(), 43);
    assert_eq!(kept.who(&id, NOW + SESSION_SECONDS - 1), Some(ana()));
    assert_eq!(kept.who(&id, NOW + SESSION_SECONDS), None);
    assert_eq!(kept.who(&format!("{id}x"), NOW), None);
    assert_eq!(kept.who("", NOW), None);
    let other = kept.open(ana(), NOW).unwrap();
    assert_ne!(id, other);
}

/// Sign-out ends the session it names, and that one alone; sessions past their end are dropped
/// as another opens.
#[test]
fn a_session_ends_at_its_sign_out_and_past_its_end() {
    let mut kept = Kept::default();
    let ended = kept.open(ana(), NOW - SESSION_SECONDS).unwrap();
    let id = kept.open(ana(), NOW).unwrap();
    let other = kept.open(ana(), NOW).unwrap();
    assert_eq!(kept.sessions.len(), 2, "the ended one is dropped");
    assert!(kept.close(&id));
    assert!(!kept.close(&id));
    assert!(!kept.close(&ended));
    assert_eq!(kept.who(&id, NOW), None);
    assert_eq!(kept.who(&other, NOW), Some(ana()));
}
