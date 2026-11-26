//! GitHub as an OAuth app: where a visitor is sent, with no scope; the code exchanged with the
//! verifier and the app's secret for an access token, taken only as GitHub answers one; the token
//! used once to ask whose it is, in no argument and nothing said; and the subject the account's
//! numeric id alone.

use super::*;
use crate::settings::Secret;
use crate::stand_in::{
    CODE, GITHUB_CLIENT_ID, GITHUB_CLIENT_SECRET, GITHUB_ID, GITHUB_TOKEN, GitHub, folder,
    github_client, write,
};
use std::collections::BTreeMap;

const REDIRECT: &str = "https://board.example.com/.sign-in/github-callback";

fn flow() -> Flow {
    Flow {
        state: "af0ifjsldkj".into(),
        nonce: "n-0S6_WzA2Mj".into(),
        verifier: "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into(),
        to: "/".into(),
    }
}

/// `form` as names and values.
fn named(form: &[(String, String)]) -> BTreeMap<String, String> {
    form.iter().cloned().collect()
}

/// A visitor is sent to GitHub's own endpoint for the app and its way back, with the sign-in's
/// state and S256 challenge, each escaped; no scope is asked, and no nonce, which GitHub has no
/// id token to carry.
#[test]
fn a_visitor_is_sent_to_github_with_the_sign_ins_state_and_challenge_and_no_scope() {
    let to = Provider::github().authorize(&github_client(), REDIRECT, &flow());
    let (endpoint, query) = to.split_once('?').unwrap();
    assert_eq!(endpoint, "https://github.com/login/oauth/authorize");
    let query: BTreeMap<String, String> = form_urlencoded::parse(query.as_bytes())
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect();
    let expected: BTreeMap<String, String> = [
        ("client_id", GITHUB_CLIENT_ID),
        ("redirect_uri", REDIRECT),
        ("state", "af0ifjsldkj"),
        (
            "code_challenge",
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
        ),
        ("code_challenge_method", "S256"),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_string(), value.to_string()))
    .collect();
    assert_eq!(query, expected);
    assert!(
        to.contains("redirect_uri=https%3A%2F%2Fboard.example.com%2F"),
        "{to}"
    );
}

/// GitHub's endpoints are constants, every one `https://`, reached through `curl`.
#[test]
fn githubs_endpoints_are_constants() {
    let github = Provider::github();
    assert_eq!(github.authorize, "https://github.com/login/oauth/authorize");
    assert_eq!(github.token, "https://github.com/login/oauth/access_token");
    assert_eq!(github.user, "https://api.github.com/user");
    assert_eq!(github.curl, "curl");
    assert_eq!(SERVICE, "github");
}

/// The code is exchanged with the app's id and secret, the way back and the verifier, and the
/// access token GitHub answers is taken; a code GitHub does not take, which it says `200` with an
/// `error`, is refused with that error's code alone, and so is an answer of another status, with
/// no token, or with one that is not visible ASCII within its bound.
#[tokio::test(flavor = "multi_thread")]
async fn the_code_is_exchanged_for_the_access_token_and_a_refusal_says_githubs_error_alone() {
    let github = GitHub::new().await;
    let provider = github.provider();
    let flow = flow();
    github.signs_in(&flow.challenge(), REDIRECT);
    let exchange = |provider: Provider, verifier: String| {
        tokio::task::spawn_blocking(move || {
            provider.exchange(&github_client(), REDIRECT, CODE, &verifier)
        })
    };
    let token = exchange(provider.clone(), flow.verifier.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(token, GITHUB_TOKEN);
    let expected: BTreeMap<String, String> = [
        ("client_id", GITHUB_CLIENT_ID),
        ("client_secret", GITHUB_CLIENT_SECRET),
        ("code", CODE),
        ("redirect_uri", REDIRECT),
        ("code_verifier", flow.verifier.as_str()),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_string(), value.to_string()))
    .collect();
    assert_eq!(named(&github.heard()[0]), expected);

    let again = exchange(provider.clone(), flow.verifier.clone())
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(
        again,
        "GitHub's token endpoint answered 200 with no access token: bad_verification_code"
    );
    github.signs_in(&flow.challenge(), REDIRECT);
    let wrong = exchange(provider.clone(), "another-verifier".into())
        .await
        .unwrap()
        .unwrap_err();
    assert!(wrong.ends_with(": bad_verification_code"), "{wrong}");

    let answers = [
        (500, format!(r#"{{"access_token":"{GITHUB_TOKEN}"}}"#)),
        (200, r#"{"token_type":"bearer"}"#.to_string()),
        (200, r#"{"access_token":""}"#.to_string()),
        (
            200,
            r#"{"access_token":"gho_a\nurl = \"https://evil.example\""}"#.to_string(),
        ),
        (200, r#"{"access_token":"gho a"}"#.to_string()),
        (200, format!(r#"{{"access_token":"{}"}}"#, "a".repeat(256))),
        (200, r#"{"access_token":7}"#.to_string()),
        (200, "not json".to_string()),
    ];
    for (status, body) in answers {
        github.answers(&flow.challenge(), REDIRECT, status, &body);
        let refused = exchange(provider.clone(), flow.verifier.clone())
            .await
            .unwrap()
            .unwrap_err();
        assert_eq!(
            refused,
            format!("GitHub's token endpoint answered {status} with no access token"),
            "{body}"
        );
    }
    let longest = "a".repeat(255);
    github.answers(
        &flow.challenge(),
        REDIRECT,
        200,
        &format!(r#"{{"access_token":"{longest}"}}"#),
    );
    let taken = exchange(provider, flow.verifier).await.unwrap().unwrap();
    assert_eq!(taken, longest);
}

/// The access token is asked whose it is with the token as a bearer, in the API's pinned version,
/// as the service; GitHub's answer comes back whole, and an answer of any other status is said by
/// that status alone.
#[tokio::test(flavor = "multi_thread")]
async fn the_token_is_asked_whose_it_is_and_another_status_is_said_by_itself() {
    let github = GitHub::new().await;
    let provider = github.provider();
    let ask = |provider: Provider, token: &'static str| {
        tokio::task::spawn_blocking(move || provider.user(token))
    };
    let answered = ask(provider.clone(), GITHUB_TOKEN).await.unwrap().unwrap();
    assert_eq!(subject(&answered), Ok(GITHUB_ID.to_string()));
    let asked = &github.asked()[0];
    assert_eq!(
        asked[hyper::header::AUTHORIZATION],
        format!("Bearer {GITHUB_TOKEN}")
    );
    assert_eq!(asked[hyper::header::ACCEPT], "application/vnd.github+json");
    assert_eq!(asked["x-github-api-version"], "2022-11-28");
    assert_eq!(asked[hyper::header::USER_AGENT], "symphony-sign-in");
    let refused = ask(provider.clone(), "gho_another")
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(refused, "GitHub's user endpoint answered 401");
    github.says(503, r#"{"message":"gho_StandIn"}"#);
    let unavailable = ask(provider, GITHUB_TOKEN).await.unwrap().unwrap_err();
    assert_eq!(unavailable, "GitHub's user endpoint answered 503");
}

/// What curl says when it fails names neither the access token nor the app's secret: here a curl
/// that says its whole request on stderr.
#[test]
fn what_curl_says_when_a_call_fails_names_no_token_or_secret() {
    let bin = folder("github-curl");
    let saying = write(&bin, "curl", "#!/bin/sh\ncat >&2\nexit 7\n", 0o700);
    let provider = Provider::stand_in(
        "https://github.com/login/oauth/access_token",
        "https://api.github.com/user",
        saying.to_str().unwrap(),
    );
    let said = provider.user(GITHUB_TOKEN).unwrap_err();
    assert!(said.contains("Authorization: Bearer <hidden>"), "{said}");
    assert!(!said.contains(GITHUB_TOKEN), "{said}");
    let client = Client {
        id: GITHUB_CLIENT_ID.into(),
        secret: Secret::stand_in("s3cret/+="),
    };
    let said = provider
        .exchange(&client, REDIRECT, "c0de/+", "verifier-0000")
        .unwrap_err();
    for secret in [
        "s3cret/+=",
        "s3cret%2F%2B%3D",
        "c0de/+",
        "c0de%2F%2B",
        "verifier-0000",
    ] {
        assert!(!said.contains(secret), "{secret}: {said}");
    }
}

/// The subject is the account's id, a whole number above 0, said in decimal; nothing else of the
/// answer is read.
#[test]
fn the_subject_is_the_accounts_numeric_id() {
    for (user, id) in [
        (r#"{"login":"ana","id":77001234,"type":"User"}"#, "77001234"),
        (r#"{"id":1}"#, "1"),
        (r#"{"id":18446744073709551615}"#, "18446744073709551615"),
        (r#" {"id": 42, "login": null} "#, "42"),
    ] {
        assert_eq!(subject(user), Ok(id.to_string()), "{user}");
    }
}

/// An answer that names no id, or one that is not a whole number above 0, is refused: never the
/// login in its place, nor a string of digits.
#[test]
fn an_answer_with_no_numeric_id_is_refused() {
    for user in [
        r#"{"login":"ana"}"#,
        r#"{"id":null,"login":"ana"}"#,
        r#"{"id":"77001234"}"#,
        r#"{"id":1.5}"#,
        r#"{"id":1e3}"#,
        r#"{"id":-3}"#,
        r#"{"id":0}"#,
        r#"{"id":18446744073709551616}"#,
        r#"{"id":true}"#,
        r#"{"id":[1]}"#,
        r#"[{"id":1}]"#,
        r#""77001234""#,
        "77001234",
        "",
        "not json",
    ] {
        assert_eq!(subject(user), Err(NO_ID), "{user}");
    }
}
