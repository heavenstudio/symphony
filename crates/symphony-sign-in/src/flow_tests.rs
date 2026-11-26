//! A sign-in under way travels in its browser's flow cookie: taken back whole by the browser that
//! began it, within its ten minutes, and refused for a cookie that is not the service's, altered,
//! another sign-in's, signed before a restart, or past its end. Nothing is kept, so no number of
//! sign-ins begun pushes one out.

use super::*;

const NOW: u64 = 1_800_000_000;

/// A seal of a key the test names.
fn seal() -> Seal {
    Seal([1; 64])
}

/// `said` signed under `seal` as a flow cookie is, whatever it says.
fn signed(seal: &Seal, said: &[u8]) -> String {
    let said = URL_SAFE_NO_PAD.encode(said);
    let tag = URL_SAFE_NO_PAD.encode(seal.mac(&said).finalize().into_bytes());
    format!("{said}.{tag}")
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

/// A sign-in begun is taken back whole by the browser whose cookie it gave, the path to come back
/// to with its query among it; each is fresh, and its cookie is base64url and one dot, which no
/// cookie escapes. Nothing is kept: the cookie taken again is the same sign-in, since the provider,
/// which takes a code once, is what takes a sign-in once.
#[test]
fn a_sign_in_is_taken_back_whole_from_the_cookie_of_the_browser_that_began_it() {
    let seal = Seal::new().unwrap();
    let (begun, cookie) = seal.begin("/docs/?x=1&y=a.b", NOW).unwrap();
    assert_ne!(begun.state, begun.nonce);
    assert_ne!(begun.nonce, begun.verifier);
    assert_eq!(begun.to, "/docs/?x=1&y=a.b");
    assert_eq!(cookie.matches('.').count(), 1, "{cookie}");
    assert!(
        (cookie.bytes()).all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)),
        "{cookie}"
    );
    let taken = seal.take(&begun.state, Some(&cookie), NOW + 1).unwrap();
    assert_eq!(taken, begun);
    assert_eq!(seal.take(&begun.state, Some(&cookie), NOW + 2), Ok(begun));
    let (again, _) = seal.begin("/docs/?x=1&y=a.b", NOW).unwrap();
    assert_ne!(again.state, taken.state);
    assert_ne!(again.verifier, taken.verifier);
}

/// A way back whose browser holds no flow cookie, or another sign-in's, is refused as not its own.
#[test]
fn a_way_back_from_a_browser_without_its_cookie_or_with_another_sign_ins_is_unbound() {
    let seal = seal();
    let (begun, _) = seal.begin("/", NOW).unwrap();
    let (_, other) = seal.begin("/", NOW).unwrap();
    assert_eq!(seal.take(&begun.state, None, NOW), Err(UNBOUND));
    assert_eq!(seal.take(&begun.state, Some(&other), NOW), Err(UNBOUND));
    let (mine, cookie) = seal.begin("/", NOW).unwrap();
    for state in [format!("{}x", mine.state), String::new(), "x".into()] {
        assert_eq!(
            seal.take(&state, Some(&cookie), NOW),
            Err(UNBOUND),
            "{state}"
        );
    }
}

/// A flow cookie the service did not sign, or signed and since altered in any part, or signed
/// under the key of an earlier start, is refused as forged.
#[test]
fn a_forged_or_altered_cookie_or_one_from_before_a_restart_is_refused() {
    let seal = seal();
    let (begun, cookie) = seal.begin("/docs/", NOW).unwrap();
    let (said, tag) = cookie.split_once('.').unwrap();
    let state = begun.state.as_str();
    let text = String::from_utf8(URL_SAFE_NO_PAD.decode(said).unwrap()).unwrap();
    let altered = |text: &str| format!("{}.{tag}", URL_SAFE_NO_PAD.encode(text));
    let forged = [
        String::new(),
        "forged".into(),
        state.into(),
        format!("{said}."),
        format!(".{tag}"),
        format!("{said}.{tag}x"),
        format!("{said}.!{tag}"),
        format!("{said}.{}", tag.get(..tag.len() - 2).unwrap()),
        format!("{said}x.{tag}"),
        altered(&text.replace("/docs/", "/admin")),
        altered(&text.replacen('1', "9", 1)),
        altered(&text.replace(&begun.verifier, &"A".repeat(43))),
        format!("{cookie}.{tag}"),
        Seal([2; 64]).begin("/docs/", NOW).unwrap().1,
    ];
    for cookie in forged {
        assert_eq!(
            seal.take(state, Some(&cookie), NOW),
            Err(FORGED),
            "{cookie}"
        );
    }
}

/// A cookie the service signed that does not read as one it writes is refused as forged too,
/// never taken: not text, too few parts, or an end that is not a number.
#[test]
fn a_signed_cookie_that_does_not_read_is_refused_as_forged() {
    let seal = seal();
    for said in [
        &b"\xff\xfe"[..],
        b"1900000000.state.nonce.verifier",
        b"soon.state.nonce.verifier./",
    ] {
        let cookie = signed(&seal, said);
        assert_eq!(
            seal.take("state", Some(&cookie), NOW),
            Err(FORGED),
            "{cookie}"
        );
    }
    let good = signed(&seal, b"1900000000.state.nonce.verifier./a.b");
    let taken = seal.take("state", Some(&good), NOW).unwrap();
    assert_eq!(
        (taken.verifier.as_str(), taken.to.as_str()),
        ("verifier", "/a.b")
    );
}

/// A sign-in is taken within its ten minutes, to the second, and refused from then on.
#[test]
fn a_sign_in_waits_ten_minutes_for_its_way_back() {
    let seal = seal();
    let (begun, cookie) = seal.begin("/", NOW).unwrap();
    assert!(
        seal.take(&begun.state, Some(&cookie), NOW + FLOW_SECONDS - 1)
            .is_ok()
    );
    for late in [NOW + FLOW_SECONDS, NOW + FLOW_SECONDS + 1, u64::MAX] {
        assert_eq!(seal.take(&begun.state, Some(&cookie), late), Err(STALE));
    }
}

/// The path to come back to is kept up to its bound, so the cookie stays well inside what a
/// browser keeps of one with its name and flags; a longer one comes back to `/`.
#[test]
fn a_way_back_past_its_bound_comes_back_to_the_root_and_the_cookie_stays_small() {
    let seal = seal();
    let longest = format!("/{}", "a".repeat(TO_BYTES - 1));
    let (kept, cookie) = seal.begin(&longest, NOW).unwrap();
    assert_eq!(kept.to, longest);
    let set = format!(
        "__Host-symphony-sign-in-flow={cookie}; Max-Age=600; Path=/; HttpOnly; SameSite=Lax; Secure"
    );
    assert!(set.len() < 3500, "{}", set.len());
    let (dropped, cookie) = seal.begin(&format!("{longest}a"), NOW).unwrap();
    assert_eq!(dropped.to, "/");
    assert_eq!(
        seal.take(&dropped.state, Some(&cookie), NOW).unwrap().to,
        "/"
    );
}

/// The key says nothing of itself.
#[test]
fn a_seal_says_nothing_of_its_key() {
    assert_eq!(format!("{:?}", seal()), "Seal(hidden)");
}

/// However many sign-ins are begun, none pushes another out: a thousand and one begun after it, the
/// first is taken back as it began.
#[test]
fn no_number_of_sign_ins_begun_pushes_one_out() {
    let seal = seal();
    let (first, cookie) = seal.begin("/first", NOW).unwrap();
    for _ in 0..1001 {
        seal.begin("/", NOW).unwrap();
    }
    assert_eq!(seal.take(&first.state, Some(&cookie), NOW), Ok(first));
}
