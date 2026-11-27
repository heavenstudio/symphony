//! Google by OpenID Connect: where a visitor is sent, the code exchanged with the verifier and the
//! client's secret, and every claim the service relies on checked in the id token Google answers:
//! its issuer, its audience, its life, its nonce and its subject.

use super::*;
use crate::flow::Flow;
use crate::settings::Secret;
use crate::stand_in::{CLIENT_ID, CLIENT_SECRET, CODE, Google, SUBJECT, claims, id_token, now};
use std::collections::BTreeMap;

const NONCE: &str = "n-0S6_WzA2Mj";

fn client() -> Client {
    Client {
        id: CLIENT_ID.into(),
        secret: Secret::stand_in(CLIENT_SECRET),
    }
}

fn flow() -> Flow {
    Flow {
        state: "af0ifjsldkj".into(),
        nonce: NONCE.into(),
        verifier: "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into(),
        to: "/".into(),
    }
}

/// A visitor is sent to Google's own endpoint for the client and its way back, the scope
/// `openid` alone, and the sign-in's state, nonce and S256 challenge, each escaped.
#[test]
fn a_visitor_is_sent_to_google_with_the_sign_ins_state_nonce_and_challenge() {
    let to = Provider::google().authorize(
        &client(),
        "https://board.example.com/.sign-in/callback",
        &flow(),
    );
    let (endpoint, query) = to.split_once('?').unwrap();
    assert_eq!(endpoint, "https://accounts.google.com/o/oauth2/v2/auth");
    let asked: BTreeMap<String, String> = form_urlencoded::parse(query.as_bytes())
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect();
    let expected: BTreeMap<String, String> = [
        ("response_type", "code"),
        ("client_id", CLIENT_ID),
        (
            "redirect_uri",
            "https://board.example.com/.sign-in/callback",
        ),
        ("scope", "openid"),
        ("state", "af0ifjsldkj"),
        ("nonce", NONCE),
        (
            "code_challenge",
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
        ),
        ("code_challenge_method", "S256"),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_string(), value.to_string()))
    .collect();
    assert_eq!(asked, expected);
    assert!(
        query.contains("redirect_uri=https%3A%2F%2Fboard.example.com%2F.sign-in%2Fcallback"),
        "{query}"
    );
    assert!(!to.contains(CLIENT_SECRET));
}

/// Google's endpoints are its own, and its id tokens' issuers its two.
#[test]
fn googles_endpoints_are_constants() {
    let google = Provider::google();
    assert_eq!(google.token, "https://oauth2.googleapis.com/token");
    assert_eq!(google.curl, "curl");
    assert_eq!(
        ISSUERS,
        ["https://accounts.google.com", "accounts.google.com"]
    );
}

/// The code is exchanged with the verifier, the way back and the client's id and secret, and the
/// id token Google answers comes back; a refusal says Google's error code alone, never what else
/// its body says, and an answer with no id token is refused.
#[tokio::test(flavor = "multi_thread")]
async fn the_code_is_exchanged_for_the_id_token_and_a_refusal_says_googles_error_alone() {
    let google = Google::new().await;
    let provider = google.provider();
    let redirect = "https://board.example.com/.sign-in/callback";
    let flow = flow();
    google.signs_in(&flow.challenge(), redirect, &claims(NONCE));
    let exchange = |provider: Provider, verifier: String| {
        tokio::task::spawn_blocking(move || provider.exchange(&client(), redirect, CODE, &verifier))
    };
    let token = exchange(provider.clone(), flow.verifier.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(subject(&token, CLIENT_ID, NONCE, now()), Ok(SUBJECT.into()));
    let form: BTreeMap<String, String> = google.heard()[0].iter().cloned().collect();
    let expected: BTreeMap<String, String> = [
        ("grant_type", "authorization_code"),
        ("code", CODE),
        ("code_verifier", flow.verifier.as_str()),
        ("redirect_uri", redirect),
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_string(), value.to_string()))
    .collect();
    assert_eq!(form, expected);

    let wrong = exchange(provider.clone(), "another-verifier".into())
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(
        wrong,
        "Google's token endpoint answered 400 with no id token: invalid_grant"
    );
    google.answers(&flow.challenge(), redirect, 200, r#"{"access_token":"x"}"#);
    let tokenless = exchange(provider.clone(), flow.verifier.clone())
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(
        tokenless,
        "Google's token endpoint answered 200 with no id token"
    );
    google.answers(
        &flow.challenge(),
        redirect,
        401,
        r#"{"error":"Invalid client: GOCSPX-stand-in-secret-0000","id_token":"a.b.c"}"#,
    );
    let said = exchange(provider, flow.verifier)
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(
        said,
        "Google's token endpoint answered 401 with no id token"
    );
}

/// A good id token names Ana's subject: an audience given as a list holding the client is taken
/// when the authorized party is the client, and either of Google's issuers is.
#[test]
fn a_good_id_token_names_its_subject() {
    let now = now();
    let good = claims(NONCE);
    assert_eq!(
        subject(&id_token(&good), CLIENT_ID, NONCE, now),
        Ok(SUBJECT.into())
    );
    let mut listed = good.clone();
    listed["aud"] = serde_json::json!([CLIENT_ID, "another-client"]);
    assert_eq!(
        subject(&id_token(&listed), CLIENT_ID, NONCE, now),
        Ok(SUBJECT.into())
    );
    let mut bare = good.clone();
    bare["iss"] = serde_json::json!("accounts.google.com");
    bare.as_object_mut().unwrap().remove("azp");
    assert_eq!(
        subject(&id_token(&bare), CLIENT_ID, NONCE, now),
        Ok(SUBJECT.into())
    );
    let mut early = good;
    early["iat"] = serde_json::json!(now + SKEW);
    assert_eq!(
        subject(&id_token(&early), CLIENT_ID, NONCE, now),
        Ok(SUBJECT.into())
    );
}

/// Every claim the service relies on refuses the token when it is wrong, each for its own reason.
#[test]
fn an_id_token_wrong_in_any_claim_the_service_relies_on_is_refused() {
    let now = now();
    let cases: [(&str, serde_json::Value, &str); 14] = [
        ("iss", serde_json::json!("https://evil.example"), ISSUER),
        (
            "iss",
            serde_json::json!("https://accounts.google.com/"),
            ISSUER,
        ),
        ("aud", serde_json::json!("another-client"), AUDIENCE),
        ("aud", serde_json::json!(["another-client"]), AUDIENCE),
        ("aud", serde_json::json!([]), AUDIENCE),
        ("azp", serde_json::json!("another-client"), AUDIENCE),
        ("exp", serde_json::json!(now), EXPIRED),
        ("exp", serde_json::json!(now - 3600), EXPIRED),
        ("iat", serde_json::json!(now + SKEW + 1), EARLY),
        ("nonce", serde_json::json!("another"), NONCE_REFUSED),
        ("nonce", serde_json::json!(null), NONCE_REFUSED),
        ("sub", serde_json::json!(""), SUBJECT_REFUSED),
        ("sub", serde_json::json!("x".repeat(256)), SUBJECT_REFUSED),
        ("sub", serde_json::json!("has space"), SUBJECT_REFUSED),
    ];
    for (claim, value, why) in cases {
        let mut claims = claims(NONCE);
        claims[claim] = value.clone();
        assert_eq!(
            subject(&id_token(&claims), CLIENT_ID, NONCE, now),
            Err(why),
            "{claim} = {value}"
        );
    }
    let mut listed = claims(NONCE);
    listed["aud"] = serde_json::json!([CLIENT_ID, "another-client"]);
    listed.as_object_mut().unwrap().remove("azp");
    assert_eq!(
        subject(&id_token(&listed), CLIENT_ID, NONCE, now),
        Err(AUDIENCE)
    );
    let mut nonceless = claims(NONCE);
    nonceless.as_object_mut().unwrap().remove("nonce");
    assert_eq!(
        subject(&id_token(&nonceless), CLIENT_ID, NONCE, now),
        Err(NONCE_REFUSED)
    );
    for missing in ["iss", "sub", "aud", "exp", "iat"] {
        let mut claims = claims(NONCE);
        claims.as_object_mut().unwrap().remove(missing);
        assert_eq!(
            subject(&id_token(&claims), CLIENT_ID, NONCE, now),
            Err(MALFORMED),
            "{missing}"
        );
    }
}

/// A token that is not three base64url parts of a JSON object, or whose times are not integer
/// seconds, is refused as malformed.
#[test]
fn a_token_not_three_parts_of_json_claims_is_malformed() {
    let now = now();
    let good = id_token(&claims(NONCE));
    let mut parts = good.split('.');
    let (header, body) = (parts.next().unwrap(), parts.next().unwrap());
    let mut fractional = claims(NONCE);
    fractional["exp"] = serde_json::json!(1.5);
    for token in [
        String::new(),
        "a.b".into(),
        format!("{good}.d"),
        format!("{header}.!!!.c"),
        format!("{header}.{}.c", URL_SAFE_NO_PAD.encode(b"[1,2]")),
        format!("{header}.{}.c", URL_SAFE_NO_PAD.encode(b"not json")),
        id_token(&fractional),
    ] {
        assert_eq!(
            subject(&token, CLIENT_ID, NONCE, now),
            Err(MALFORMED),
            "{token}"
        );
    }
    let padded = format!("{header}.{body}==.c");
    assert_eq!(subject(&padded, CLIENT_ID, NONCE, now), Ok(SUBJECT.into()));
}

const NONCE_REFUSED: &str = super::NONCE;
const SUBJECT_REFUSED: &str = super::SUBJECT;
