//! The signed word's check: what a service signs and the server takes, each part of the format
//! refused on its own, the freshness bounds at their edges, and the settings file read as the
//! machine folder keeps it.

use super::*;
use crate::test_tree::Tree;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

const ORIGIN: &str = "https://board.example.com";
/// The server's clock in every case: a time well after the epoch.
const NOW: u64 = 1_800_000_000;

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn public(seed: u8) -> String {
    let bytes = key(seed).verifying_key().to_bytes();
    format!(
        "ed25519:{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    )
}

fn settings() -> Settings {
    Settings::new(&public(1), ORIGIN).unwrap()
}

fn at(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

fn part(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// The claims a good word holds: Ana, signed in by Google, for [`ORIGIN`], issued a minute ago for
/// five minutes.
fn claims() -> Value {
    json!({
        "iss": "google",
        "sub": "110248495921238986420",
        "name": "Ana Lima",
        "aud": ORIGIN,
        "iat": NOW - 60,
        "exp": NOW + 240,
    })
}

/// The header and the claims, as they are written, signed by `seed`'s key.
fn signed_raw(seed: u8, header: &str, claims: &str) -> String {
    let input = format!("{}.{}", part(header.as_bytes()), part(claims.as_bytes()));
    let signature = key(seed).sign(input.as_bytes());
    format!("{input}.{}", part(&signature.to_bytes()))
}

fn signed_by(seed: u8, claims: &Value) -> String {
    signed_raw(seed, r#"{"alg":"EdDSA","typ":"JWT"}"#, &claims.to_string())
}

fn signed(claims: &Value) -> String {
    signed_by(1, claims)
}

/// `claims` with `field` set to `value`, or taken out for `Value::Null`.
fn with(field: &str, value: Value) -> Value {
    let mut claims = claims();
    if value.is_null() {
        claims.as_object_mut().unwrap().remove(field);
    } else {
        claims[field] = value;
    }
    claims
}

fn checked(token: &str) -> Result<SignedIn, Refused> {
    verify(token, &settings(), at(NOW))
}

#[test]
fn a_word_the_key_signed_fresh_for_this_origin_says_who_signed_the_person_in() {
    assert_eq!(
        checked(&signed(&claims())),
        Ok(SignedIn {
            service: "google".into(),
            subject: "110248495921238986420".into(),
            display: Some("Ana Lima".into()),
            origin: ORIGIN.into(),
        })
    );
    // No display name, no `typ`, a key's id, a start already passed and claims the format does not
    // hold, groups among them: taken, and nothing of the rest read.
    let bare = json!({
        "iss": "corp-sso", "sub": "ana", "aud": ORIGIN, "iat": NOW, "exp": NOW + 1,
        "nbf": NOW - 5, "groups": ["admins"], "roles": "owner", "email": "ana@example.com",
    });
    let token = signed_raw(1, r#"{"alg":"EdDSA","kid":"2026-11"}"#, &bare.to_string());
    let ana = checked(&token).unwrap();
    assert_eq!(
        (ana.service.as_str(), ana.subject.as_str(), ana.display),
        ("corp-sso", "ana", None)
    );
}

#[test]
fn a_request_carries_one_word_of_visible_ascii_at_most_the_longest_taken() {
    assert_eq!(carried(&[]), None);
    assert_eq!(carried(&[b"a.b.c"]), Some(Ok("a.b.c")));
    assert_eq!(
        carried(&[b"a.b.c", b"a.b.c"]),
        Some(Err(Refused::Repeated)),
        "two headers, even alike"
    );
    let longest = vec![b'a'; MAX_BYTES];
    assert_eq!(
        carried(&[&longest]).map(|word| word.map(str::len)),
        Some(Ok(MAX_BYTES))
    );
    let longer = vec![b'a'; MAX_BYTES + 1];
    assert_eq!(carried(&[&longer]), Some(Err(Refused::Oversized)));
    for odd in [
        &b"a.b c"[..],
        b"a.b\tc",
        "a.b.\u{e9}".as_bytes(),
        b"a.b.\x7f",
    ] {
        assert_eq!(carried(&[odd]), Some(Err(Refused::Malformed)), "{odd:?}");
    }
    // A value no part may hold is the word's to refuse, as one it cannot read.
    assert_eq!(carried(&[b""]), Some(Ok("")));
    assert_eq!(checked(""), Err(Refused::Malformed));
}

#[test]
fn a_word_longer_than_the_longest_taken_is_refused_before_it_is_read() {
    let long = with("name", json!("a".repeat(MAX_BYTES)));
    assert_eq!(checked(&signed(&long)), Err(Refused::Oversized));
}

#[test]
fn a_word_that_is_not_three_unpadded_base64url_parts_of_json_is_malformed() {
    let good = signed(&claims());
    let parts: Vec<&str> = good.split('.').collect();
    let (head, body, signature) = (parts[0], parts[1], parts[2]);
    // The standard alphabet's `+` and `/` for base64url's `-` and `_`.
    let standard = signature.replace('-', "+").replace('_', "/");
    assert_ne!(standard, signature, "the signature holds a - or a _");
    for bad in [
        format!("{head}.{body}"),
        format!("{head}.{body}.{signature}.{signature}"),
        format!("{head}.{body}.{signature}."),
        format!("{head}=.{body}.{signature}"),
        format!("{head}.{body}.{signature}=="),
        format!("{head}.{body}.{standard}"),
        format!("{}.{body}.{signature}", part(b"not json")),
        format!("{}.{body}.{signature}", part(br#"["EdDSA"]"#)),
        // A signature a byte short.
        format!("{head}.{body}.{}", part(&[7; 63])),
    ] {
        assert_eq!(checked(&bad), Err(Refused::Malformed), "{bad}");
    }
    // A base64url part with a bit set past its last byte is not the one way to write those bytes.
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let last = signature.as_bytes()[signature.len() - 1];
    let value = alphabet.iter().position(|c| *c == last).unwrap();
    assert_eq!(
        value & 0xf,
        0,
        "a 64-byte part ends in two bits of its last byte"
    );
    let loose = format!(
        "{head}.{body}.{}{}",
        &signature[..signature.len() - 1],
        char::from(alphabet[value | 1])
    );
    assert_eq!(checked(&loose), Err(Refused::Malformed));
}

#[test]
fn claims_the_format_holds_broken_or_missing_are_malformed_once_the_signature_verifies() {
    for (field, value) in [
        ("sub", Value::Null),
        ("sub", json!("")),
        ("sub", json!(7)),
        ("iss", Value::Null),
        ("aud", Value::Null),
        ("aud", json!([ORIGIN])),
        ("iat", Value::Null),
        ("iat", json!(-1)),
        ("iat", json!(1.5)),
        ("iat", json!("1800000000")),
        ("exp", Value::Null),
        ("nbf", json!(1.5)),
        ("name", json!(["Ana"])),
    ] {
        assert_eq!(
            checked(&signed(&with(field, value.clone()))),
            Err(Refused::Malformed),
            "{field}: {value}"
        );
    }
    let listed = r#"["google","ana","https://board.example.com",1799999940,1800000240]"#;
    assert_eq!(
        checked(&signed_raw(1, r#"{"alg":"EdDSA"}"#, listed)),
        Err(Refused::Malformed),
        "claims as an array, which serde would read in order"
    );
    let twice = claims().to_string().replacen('{', r#"{"sub":"bob","#, 1);
    assert_eq!(
        checked(&signed_raw(1, r#"{"alg":"EdDSA"}"#, &twice)),
        Err(Refused::Malformed),
        "a subject given twice"
    );
}

#[test]
fn a_header_of_another_algorithm_or_a_member_the_format_does_not_hold_is_refused() {
    let body = claims().to_string();
    for header in [
        r#"{"alg":"none"}"#,
        r#"{"alg":"HS256"}"#,
        r#"{"alg":"ES256"}"#,
        r#"{"alg":"eddsa"}"#,
        r#"{"alg":"EdDSA","typ":"JOSE"}"#,
    ] {
        assert_eq!(
            checked(&signed_raw(1, header, &body)),
            Err(Refused::Algorithm),
            "{header}"
        );
    }
    for header in [
        r#"{"typ":"JWT"}"#,
        r#"{"alg":"EdDSA","crit":["exp"]}"#,
        r#"{"alg":"EdDSA","jwk":{"kty":"OKP"}}"#,
        r#"{"alg":"EdDSA","jku":"https://keys.example.com"}"#,
        r#"{"alg":"EdDSA","x5c":[]}"#,
        r#"{"alg":"none","alg":"EdDSA"}"#,
    ] {
        assert_eq!(
            checked(&signed_raw(1, header, &body)),
            Err(Refused::Malformed),
            "{header}"
        );
    }
}

#[test]
fn a_word_tampered_with_or_signed_by_another_key_is_refused_by_its_signature() {
    let good = signed(&claims());
    let parts: Vec<&str> = good.split('.').collect();
    let bob = part(with("sub", json!("bob")).to_string().as_bytes());
    assert_eq!(
        checked(&format!("{}.{bob}.{}", parts[0], parts[2])),
        Err(Refused::Signature),
        "claims changed after signing"
    );
    let header = part(br#"{"alg":"EdDSA"}"#);
    assert_eq!(
        checked(&format!("{header}.{}.{}", parts[1], parts[2])),
        Err(Refused::Signature),
        "a header changed after signing"
    );
    let mut flipped = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
    flipped[10] ^= 1;
    assert_eq!(
        checked(&format!("{}.{}.{}", parts[0], parts[1], part(&flipped))),
        Err(Refused::Signature)
    );
    assert_eq!(checked(&signed_by(2, &claims())), Err(Refused::Signature));
    // A signature over the claims alone, without the header, is not one over the word.
    let alone = key(1).sign(parts[1].as_bytes());
    assert_eq!(
        checked(&format!(
            "{}.{}.{}",
            parts[0],
            parts[1],
            part(&alone.to_bytes())
        )),
        Err(Refused::Signature)
    );
    // A forged claim is never read: a stale word with a bad signature is refused by its signature.
    let stale = signed_by(2, &with("exp", json!(NOW - 1)));
    assert_eq!(checked(&stale), Err(Refused::Signature));
}

#[test]
fn a_service_is_a_lowercase_word_a_profile_can_name_and_never_the_daemons() {
    for name in [
        "google",
        "github",
        "corp-sso",
        "okta.eu_1",
        "0auth",
        &"a".repeat(64),
    ] {
        assert!(service(name), "{name}");
        assert!(
            checked(&signed(&with("iss", json!(name)))).is_ok(),
            "{name}"
        );
    }
    for name in [
        "",
        "tailscale",
        "Google",
        "-sso",
        ".sso",
        "_sso",
        "corp:sso",
        "corp sso",
        "https://accounts.google.com",
        "caf\u{e9}",
        &"a".repeat(65),
    ] {
        assert!(!service(name), "{name}");
        assert_eq!(
            checked(&signed(&with("iss", json!(name)))),
            Err(Refused::Service),
            "{name}"
        );
    }
}

#[test]
fn a_word_made_for_another_origin_is_refused() {
    for other in [
        "https://other.example.com",
        "https://board.example.com/",
        "https://BOARD.example.com",
        "http://board.example.com",
        "https://board.example.com:443",
        "app:board",
    ] {
        assert_eq!(
            checked(&signed(&with("aud", json!(other)))),
            Err(Refused::Audience),
            "{other}"
        );
    }
}

#[test]
fn a_word_holds_at_most_fifteen_minutes_from_its_issue_and_never_ends_before_it() {
    let life = |seconds: u64| {
        let mut claims = with("iat", json!(NOW - 10));
        claims["exp"] = json!(NOW - 10 + seconds);
        checked(&signed(&claims))
    };
    assert!(life(MAX_LIFETIME).is_ok());
    assert_eq!(life(MAX_LIFETIME + 1), Err(Refused::Lifetime));
    assert_eq!(life(0), Err(Refused::Lifetime));
    let mut backwards = with("iat", json!(NOW + 5));
    backwards["exp"] = json!(NOW + 4);
    assert_eq!(checked(&signed(&backwards)), Err(Refused::Lifetime));
}

#[test]
fn a_word_issued_or_starting_beyond_the_skew_is_early_and_one_at_its_expiry_is_stale() {
    let issued = |iat: u64| {
        let mut claims = with("iat", json!(iat));
        claims["exp"] = json!(iat + 60);
        checked(&signed(&claims))
    };
    assert!(issued(NOW + SKEW).is_ok(), "a clock ahead by the skew");
    assert_eq!(issued(NOW + SKEW + 1), Err(Refused::Early));
    assert!(checked(&signed(&with("nbf", json!(NOW + SKEW)))).is_ok());
    assert_eq!(
        checked(&signed(&with("nbf", json!(NOW + SKEW + 1)))),
        Err(Refused::Early),
        "not yet valid, by its start"
    );
    let ends = |exp: u64| {
        let mut claims = with("exp", json!(exp));
        claims["iat"] = json!(exp - 60);
        checked(&signed(&claims))
    };
    assert!(ends(NOW + 1).is_ok());
    assert_eq!(ends(NOW), Err(Refused::Stale));
    assert_eq!(ends(NOW - 1), Err(Refused::Stale));
    // A clock before the epoch reads as the epoch: every word is early there.
    let before = UNIX_EPOCH - Duration::from_secs(1);
    assert_eq!(
        verify(&signed(&claims()), &settings(), before),
        Err(Refused::Early)
    );
}

#[test]
fn the_key_is_ed25519_and_64_lowercase_hex_digits_of_a_point_that_is_no_weak_key() {
    assert!(Settings::new(&public(1), ORIGIN).is_ok());
    let hex = public(1).strip_prefix("ed25519:").unwrap().to_string();
    let not_a_point = (2u8..)
        .map(|y| {
            let mut bytes = [0u8; 32];
            bytes[0] = y;
            bytes
        })
        .find(|bytes| VerifyingKey::from_bytes(bytes).is_err())
        .unwrap();
    let weak = format!("ed25519:01{}", "00".repeat(31));
    for (bad, why) in [
        (
            hex.clone(),
            "the key is not ed25519: and 64 lowercase hex digits",
        ),
        (
            format!("ED25519:{hex}"),
            "the key is not ed25519: and 64 lowercase hex digits",
        ),
        (
            format!("ed25519:{}", hex.to_uppercase()),
            "the key is not ed25519: and 64 lowercase hex digits",
        ),
        (
            format!("ed25519:{}", &hex[..62]),
            "the key is not ed25519: and 64 lowercase hex digits",
        ),
        (
            format!("ed25519:{hex}00"),
            "the key is not ed25519: and 64 lowercase hex digits",
        ),
        (
            format!("ed25519:{}zz", &hex[..62]),
            "the key is not ed25519: and 64 lowercase hex digits",
        ),
        (
            format!("ed25519:+{}", &hex[..63]),
            "the key is not ed25519: and 64 lowercase hex digits",
        ),
        (
            format!(
                "ed25519:{}",
                not_a_point
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            ),
            "the key is no Ed25519 public key",
        ),
        (
            weak,
            "the key is a weak Ed25519 key, which a forged signature passes",
        ),
    ] {
        assert_eq!(Settings::new(&bad, ORIGIN).unwrap_err(), why, "{bad}");
    }
}

#[test]
fn the_origin_is_a_scheme_and_a_lowercase_host_with_a_port_or_none_and_nothing_after() {
    for good in [
        "https://board.example.com",
        "http://board.lan:8080",
        "https://10.0.0.5:8443",
        "http://board.lan:443",
        "https://board.lan:80",
        "https://[fd7a:115c:a1e0::5]",
        "https://[::1]:8765",
    ] {
        assert_eq!(Settings::new(&public(1), good).unwrap().origin(), good);
    }
    for bad in [
        "board.example.com",
        "ftp://board.example.com",
        "HTTPS://board.example.com",
        "https://Board.example.com",
        "https://",
        "https://board.example.com/",
        "https://board.example.com/tasks",
        "https://board.example.com?x=1",
        "https://board.example.com#top",
        "https://ana@board.example.com",
        "https://board.example.com:",
        "https://board.example.com:0",
        "https://board.example.com:+80",
        "https://board.example.com:65536",
        "https://board.example.com:80:80",
        // A browser never names the scheme's own port, so no word made for it would be taken.
        "https://board.example.com:443",
        "http://board.lan:80",
        "https://[::1]:443",
        "https://[]",
        "https://[fd7a::5",
        "https://[fd7a::g]",
        "https://[fd7a::5]x",
        "https://board example.com",
    ] {
        let why = Settings::new(&public(1), bad).unwrap_err();
        assert!(
            why.starts_with("the origin is not http:// or https://"),
            "{bad}: {why}"
        );
        assert_eq!(
            why,
            Settings::new(&public(1), "").unwrap_err(),
            "the rule, never the value"
        );
    }
}

#[test]
fn the_file_names_its_key_and_origin_and_nothing_else_said_by_the_rule_never_the_value() {
    let text = format!("key = \"{}\"\norigin = \"{ORIGIN}\"\n", public(1));
    assert_eq!(Settings::parse(&text), Ok(settings()));
    for bad in [
        format!("origin = \"{ORIGIN}\"\n"),
        format!("key = \"{}\"\n", public(1)),
        format!("{text}audience = \"{ORIGIN}\"\n"),
        format!("key = \"{}\"\norigin = 7\n", public(1)),
        format!("key = \"{}", public(1)),
    ] {
        let why = Settings::parse(&bad).unwrap_err();
        assert!(!why.contains(&public(1)), "{why}");
        assert!(!why.contains("board.example.com"), "{why}");
    }
}

/// A machine folder whose sign-in file holds `text`, mode `mode`.
fn machine(text: &str, mode: u32) -> Tree {
    let tree = Tree::new();
    tree.write(FILE, text);
    std::fs::set_permissions(tree.path(FILE), std::fs::Permissions::from_mode(mode)).unwrap();
    tree
}

#[test]
fn the_machine_folder_names_the_settings_in_a_file_no_other_account_may_write() {
    let text = format!("key = \"{}\"\norigin = \"{ORIGIN}\"\n", public(1));
    assert_eq!(
        Settings::read(&machine(&text, 0o600).path("")),
        Ok(Some(settings()))
    );
    assert_eq!(
        Settings::read(&machine(&text, 0o644).path("")),
        Ok(Some(settings()))
    );
    assert_eq!(
        Settings::read(&Tree::new().path("")),
        Ok(None),
        "no file, no key"
    );

    let open = Settings::read(&machine(&text, 0o666).path("")).unwrap_err();
    assert!(
        open.ends_with(
            "sign-in.toml may be written by another account, so no signed word is verified by it: \
             chmod go-w it"
        ),
        "{open}"
    );
    let linked = Tree::new();
    linked.write("elsewhere.toml", &text);
    std::os::unix::fs::symlink(linked.path("elsewhere.toml"), linked.path(FILE)).unwrap();
    let why = Settings::read(&linked.path("")).unwrap_err();
    assert!(
        why.ends_with(
            "sign-in.toml is a link, so no signed word is verified by it: keep the file itself there"
        ),
        "{why}"
    );
    let folder = Tree::new();
    std::fs::create_dir(folder.path(FILE)).unwrap();
    let why = Settings::read(&folder.path("")).unwrap_err();
    assert!(why.ends_with("sign-in.toml is not a file"), "{why}");

    let broken = Settings::read(&machine("key = 7\n", 0o600).path("")).unwrap_err();
    assert!(
        broken.contains("sign-in.toml: "),
        "said by its path: {broken}"
    );

    // A file that is not text is refused by its path, never shown.
    let binary = machine("", 0o600);
    std::fs::write(binary.path(FILE), b"key = \"\xff\xfe secret\"\n").unwrap();
    let unread = Settings::read(&binary.path("")).unwrap_err();
    assert!(unread.contains("sign-in.toml: "), "{unread}");
    assert!(!unread.contains("secret"), "{unread}");
}
