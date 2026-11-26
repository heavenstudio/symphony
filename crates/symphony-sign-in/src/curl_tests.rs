//! The token request through `curl`: its secrets in no argument, no `~/.curlrc` read, the
//! certificate verified, no redirect followed, no proxy, the endpoint's own scheme alone; and what
//! it says when it fails names none of the values it was handed.

use super::*;
use crate::stand_in::{answer, folder, serving, write};
use hyper::StatusCode;
use std::sync::{Arc, Mutex};

/// curl runs with `-q` first, so no `~/.curlrc` is read, reads its request on stdin, follows no
/// redirect, and speaks HTTPS alone to an `https://` endpoint; nothing turns its certificate check
/// off, and no value of the request is an argument.
#[test]
fn curl_reads_no_curlrc_follows_no_redirect_and_speaks_https_alone_to_google() {
    let google = args("https://oauth2.googleapis.com/token");
    assert_eq!(google[..3], ["-q", "--config", "-"]);
    let pairs: Vec<_> = google.windows(2).map(|pair| (pair[0], pair[1])).collect();
    assert!(pairs.contains(&("--proto", "=https")), "{google:?}");
    assert!(pairs.contains(&("--max-redirs", "0")), "{google:?}");
    for refused in ["-k", "--insecure", "-L", "--location", "--proxy", "-x"] {
        assert!(!google.contains(&refused), "{refused}: {google:?}");
    }
    assert!(args("http://127.0.0.1:1/token").contains(&"=http"));
    let said = config(
        "https://oauth2.googleapis.com/token",
        "a=1&client_secret=s%22%5C",
    );
    assert!(said.contains("noproxy = \"*\"\n"), "{said}");
    assert!(
        said.contains("data-binary = \"a=1&client_secret=s%22%5C\"\n"),
        "{said}"
    );
    assert!(!said.contains("insecure"), "{said}");
    assert_eq!(
        config("u", "a\"b\\c").lines().last(),
        Some(r#"data-binary = "a\"b\\c""#)
    );
}

/// The form reaches the endpoint whole, as its body, and the answer's status and body come back.
#[tokio::test(flavor = "multi_thread")]
async fn the_form_is_posted_whole_and_the_answer_comes_back() {
    let heard: Arc<Mutex<Option<(String, String, String)>>> = Arc::default();
    let kept = heard.clone();
    let address = serving(move |request: hyper::Request<hyper::body::Incoming>| {
        let kept = kept.clone();
        async move {
            use http_body_util::BodyExt as _;
            let kind = request.headers()["content-type"]
                .to_str()
                .unwrap()
                .to_string();
            let method = request.method().to_string();
            let body = request.into_body().collect().await.unwrap().to_bytes();
            *kept.lock().unwrap() = Some((method, kind, String::from_utf8(body.to_vec()).unwrap()));
            answer(StatusCode::CREATED, r#"{"id_token":"a.b.c"}"#)
        }
    })
    .await;
    let url = format!("http://{address}/token");
    let answered = tokio::task::spawn_blocking(move || {
        post(
            "curl",
            &url,
            "grant_type=authorization_code&code=4%2F0x",
            &[],
        )
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        answered,
        Answer {
            status: 201,
            body: r#"{"id_token":"a.b.c"}"#.into()
        }
    );
    assert_eq!(
        heard.lock().unwrap().clone().unwrap(),
        (
            "POST".into(),
            "application/x-www-form-urlencoded".into(),
            "grant_type=authorization_code&code=4%2F0x".into()
        )
    );
}

/// What curl says when it fails names no value it was handed: here a curl that says its whole
/// request on stderr, and one that reaches no endpoint.
#[test]
fn what_curl_says_when_it_fails_names_none_of_its_secrets() {
    let bin = folder("curl");
    let saying = write(&bin, "curl", "#!/bin/sh\ncat >&2\nexit 7\n", 0o700);
    let hidden = [
        "GOCSPX-stand-in-secret-0000",
        "4/0stand-in-code",
        "verifier-0000",
    ];
    let form = "client_secret=GOCSPX-stand-in-secret-0000&code=4/0stand-in-code&code_verifier=verifier-0000";
    let said = post(
        saying.to_str().unwrap(),
        "https://oauth2.googleapis.com/token",
        form,
        &hidden,
    )
    .unwrap_err();
    assert!(said.contains("<hidden>"), "{said}");
    for secret in hidden {
        assert!(!said.contains(secret), "{said}");
    }
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/token", closed.local_addr().unwrap());
    drop(closed);
    let unreached = post("curl", &url, form, &hidden).unwrap_err();
    assert!(unreached.contains("reached no answer"), "{unreached}");
    for secret in hidden {
        assert!(!unreached.contains(secret), "{unreached}");
    }
    let missing = post("/no/such/curl", &url, form, &hidden).unwrap_err();
    assert!(missing.contains("is not on PATH"), "{missing}");
    let unrunnable = write(&bin, "unrunnable", "#!/bin/sh\n", 0o600);
    let unstarted = post(unrunnable.to_str().unwrap(), &url, form, &hidden).unwrap_err();
    assert!(unstarted.contains("did not start"), "{unstarted}");
    let silent = write(
        &bin,
        "silent",
        "#!/bin/sh\ncat > /dev/null\nprintf 'no status'\n",
        0o700,
    );
    let statusless = post(silent.to_str().unwrap(), &url, form, &hidden).unwrap_err();
    assert!(statusless.contains("said no HTTP status"), "{statusless}");
}

/// Redaction says each value as `<hidden>` wherever it is, and passes over an empty one.
#[test]
fn redaction_hides_every_value_and_passes_over_an_empty_one() {
    assert_eq!(
        redacted("a secret, code and secret", &["secret", "", "code"]),
        "a <hidden>, <hidden> and <hidden>"
    );
}

/// Redaction hides each value form-encoded too, as the request's body carries it, wherever the
/// encoded form and the value itself are both said, and one value inside another's form.
#[test]
fn redaction_hides_each_value_form_encoded_as_well() {
    let hidden = ["4/0stand-in-code", "s+cret/=", "a b"];
    assert_eq!(
        redacted(
            "code=4%2F0stand-in-code&client_secret=s%2Bcret%2F%3D&x=a+b; 4/0stand-in-code s+cret/=",
            &hidden
        ),
        "code=<hidden>&client_secret=<hidden>&x=<hidden>; <hidden> <hidden>"
    );
    assert_eq!(
        redacted("4/0 and 4%2F0", &["4/0", "4%2F0"]),
        "<hidden> and <hidden>"
    );
}

/// A curl that says its whole request on stderr, the form as the body carries it, encoded, names
/// none of its values when it fails.
#[test]
fn what_curl_says_of_a_form_encoded_body_names_none_of_its_secrets() {
    let bin = folder("curl-encoded");
    let saying = write(&bin, "curl", "#!/bin/sh\ncat >&2\nexit 7\n", 0o700);
    let hidden = [
        "GOCSPX-stand-in/secret+0",
        "4/0stand-in-code",
        "verifier-0000",
    ];
    let form = form_urlencoded::Serializer::new(String::new())
        .append_pair("client_secret", hidden[0])
        .append_pair("code", hidden[1])
        .append_pair("code_verifier", hidden[2])
        .finish();
    assert!(form.contains("4%2F0stand-in-code"), "{form}");
    let said = post(
        saying.to_str().unwrap(),
        "https://oauth2.googleapis.com/token",
        &form,
        &hidden,
    )
    .unwrap_err();
    assert!(said.contains("code=<hidden>"), "{said}");
    for secret in hidden {
        let encoded: String = form_urlencoded::byte_serialize(secret.as_bytes()).collect();
        assert!(!said.contains(secret), "{said}");
        assert!(!said.contains(&encoded), "{said}");
    }
}
