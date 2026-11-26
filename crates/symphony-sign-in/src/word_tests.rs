//! The word the service signs is one the server's own check takes, for the session's person and
//! the origin, holding five minutes from its issue; under another key or for another origin the
//! server refuses it.

use super::*;
use crate::stand_in::{ORIGIN, key, now};
use symphony::signed_in::{self, Refused, Settings, SignedIn};

fn ana() -> Who {
    Who {
        service: "google",
        subject: "110248495921238986420".into(),
    }
}

fn claims(word: &str) -> serde_json::Value {
    let body = word.split('.').nth(1).unwrap();
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(body).unwrap()).unwrap()
}

/// The server's check takes the word, as the person whose Google subject it names, made for the
/// origin; it names no display name, and its header is the format's.
#[test]
fn the_servers_own_check_takes_the_word_for_the_sessions_person() {
    let now = now();
    let word = signed(&key(), &ana(), ORIGIN, now);
    let settings = Settings::new(&key().public(), ORIGIN).unwrap();
    assert_eq!(
        signed_in::verify(&word, &settings, std::time::SystemTime::now()),
        Ok(SignedIn {
            service: "google".into(),
            subject: "110248495921238986420".into(),
            display: None,
            origin: ORIGIN.into(),
        })
    );
    assert_eq!(HEADER, signed_in::HEADER);
    let claims = claims(&word);
    assert_eq!(claims["iat"], now);
    assert_eq!(claims["exp"], now + LIFETIME);
    assert_eq!(claims.as_object().unwrap().len(), 5, "{claims}");
    let header = word.split('.').next().unwrap();
    assert_eq!(
        URL_SAFE_NO_PAD.decode(header).unwrap(),
        br#"{"alg":"EdDSA","typ":"JWT"}"#
    );
    const { assert!(LIFETIME <= signed_in::MAX_LIFETIME) };
    assert!(word.len() <= signed_in::MAX_BYTES);
    assert_eq!(ana().signin(), "google:110248495921238986420");
}

/// The server refuses the word under another key, for another origin, and once it has expired.
#[test]
fn the_server_refuses_the_word_under_another_key_origin_or_past_its_life() {
    let now = now();
    let word = signed(&key(), &ana(), ORIGIN, now);
    let other = Key::from_seed(&[9; 32]);
    let elsewhere = Settings::new(&other.public(), ORIGIN).unwrap();
    let at = std::time::SystemTime::now();
    assert_eq!(
        signed_in::verify(&word, &elsewhere, at),
        Err(Refused::Signature)
    );
    let another = Settings::new(&key().public(), "https://other.example.com").unwrap();
    assert_eq!(
        signed_in::verify(&word, &another, at),
        Err(Refused::Audience)
    );
    let ours = Settings::new(&key().public(), ORIGIN).unwrap();
    let later = at + std::time::Duration::from_secs(LIFETIME);
    assert_eq!(signed_in::verify(&word, &ours, later), Err(Refused::Stale));
}
