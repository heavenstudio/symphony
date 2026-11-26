//! The sign-ins under way and the sessions: a sign-in is taken once, by the browser that began it,
//! within its ten minutes; at most a thousand wait, the oldest dropped; a session holds twelve
//! hours, ends at sign-out, and is found by its id alone.

use super::*;

const NOW: u64 = 1_800_000_000;

fn ana() -> Who {
    Who {
        service: "google",
        subject: "110248495921238986420".into(),
    }
}

/// PKCE's S256 challenge is the base64url of the verifier's SHA-256: RFC 7636's own example.
#[test]
fn the_challenge_is_the_s256_of_the_verifier_as_rfc_7636_gives_it() {
    let flow = Flow {
        state: String::new(),
        nonce: String::new(),
        verifier: "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into(),
        to: "/".into(),
    };
    assert_eq!(
        flow.challenge(),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
}

/// A sign-in begun is taken back whole by its own browser, once: a second way back with its state
/// finds nothing.
#[test]
fn a_sign_in_is_taken_once_by_the_browser_that_began_it() {
    let mut kept = Kept::default();
    let begun = kept.begin("/docs/?x=1".into(), NOW).unwrap();
    assert_ne!(begun.state, begun.nonce);
    assert_ne!(begun.nonce, begun.verifier);
    let taken = kept
        .take(&begun.state, Some(&begun.state), NOW + 1)
        .unwrap();
    assert_eq!(taken, begun);
    assert_eq!(
        kept.take(&begun.state, Some(&begun.state), NOW + 2),
        Err(UNKNOWN)
    );
}

/// A way back whose browser holds no flow cookie, or another sign-in's, is refused and leaves the
/// sign-in under way for its own browser.
#[test]
fn a_way_back_from_another_browser_is_refused_and_leaves_the_sign_in_for_its_own() {
    let mut kept = Kept::default();
    let begun = kept.begin("/".into(), NOW).unwrap();
    let other = kept.begin("/".into(), NOW).unwrap();
    for cookie in [None, Some(other.state.as_str()), Some(""), Some("x")] {
        assert_eq!(
            kept.take(&begun.state, cookie, NOW),
            Err(UNBOUND),
            "{cookie:?}"
        );
    }
    let longer = format!("{}x", begun.state);
    assert_eq!(kept.take(&begun.state, Some(&longer), NOW), Err(UNBOUND));
    assert!(kept.take(&begun.state, Some(&begun.state), NOW).is_ok());
    assert!(kept.take(&other.state, Some(&other.state), NOW).is_ok());
}

/// A state no sign-in began is refused, whatever the cookie says.
#[test]
fn a_forged_state_is_refused() {
    let mut kept = Kept::default();
    kept.begin("/".into(), NOW).unwrap();
    let forged = "A".repeat(43);
    assert_eq!(kept.take(&forged, Some(&forged), NOW), Err(UNKNOWN));
}

/// A sign-in is taken within its ten minutes, to the second, and is gone after one taken late.
#[test]
fn a_sign_in_waits_ten_minutes_for_its_way_back() {
    let mut kept = Kept::default();
    let fresh = kept.begin("/".into(), NOW).unwrap();
    let stale = kept.begin("/".into(), NOW).unwrap();
    assert!(
        kept.take(&fresh.state, Some(&fresh.state), NOW + FLOW_SECONDS - 1)
            .is_ok()
    );
    assert_eq!(
        kept.take(&stale.state, Some(&stale.state), NOW + FLOW_SECONDS),
        Err(STALE)
    );
    assert_eq!(
        kept.take(&stale.state, Some(&stale.state), NOW + FLOW_SECONDS),
        Err(UNKNOWN)
    );
}

/// Sign-ins past their ten minutes are dropped as another begins, and at most a thousand wait:
/// the oldest goes first.
#[test]
fn at_most_a_thousand_sign_ins_wait_the_oldest_dropped_first() {
    let mut kept = Kept::default();
    let old = kept.begin("/".into(), NOW - FLOW_SECONDS).unwrap();
    let first = kept.begin("/".into(), NOW).unwrap();
    assert_eq!(kept.flows.len(), 1, "the stale one is dropped");
    let mut last = None;
    for _ in 0..FLOWS {
        last = Some(kept.begin("/".into(), NOW).unwrap());
    }
    assert_eq!(kept.flows.len(), FLOWS);
    assert_eq!(
        kept.take(&first.state, Some(&first.state), NOW),
        Err(UNKNOWN)
    );
    assert_eq!(kept.take(&old.state, Some(&old.state), NOW), Err(UNKNOWN));
    let last = last.unwrap();
    assert!(kept.take(&last.state, Some(&last.state), NOW).is_ok());
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
