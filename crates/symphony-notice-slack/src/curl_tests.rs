use super::*;
use crate::stand_in::{Slack, TOKEN, folder, unanswered};
use std::fs;
use std::os::unix::fs::PermissionsExt;

fn curl(api: &str) -> Curl {
    Curl::new(api, Secret::new(TOKEN).unwrap())
}

/// A `curl` at a path of its own that runs `script` under `sh`.
fn program(name: &str, script: &str) -> String {
    let path = folder(name).join("curl");
    fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path.display().to_string()
}

#[test]
fn the_whole_request_is_curls_standard_input() {
    let body = r#"{"channel":"U1","text":"a \"quoted\" \\ line"}"#;
    assert_eq!(
        curl("https://slack.com/api/").config("chat.postMessage", body),
        format!(
            "url = \"https://slack.com/api/chat.postMessage\"\n\
             request = \"POST\"\n\
             header = \"Authorization: Bearer {TOKEN}\"\n\
             header = \"Content-Type: application/json; charset=utf-8\"\n\
             header = \"User-Agent: symphony-notice-slack\"\n\
             data-binary = \"{{\\\"channel\\\":\\\"U1\\\",\\\"text\\\":\\\"a \\\\\\\"quoted\\\\\\\" \\\\\\\\ line\\\"}}\"\n"
        )
    );
}

#[test]
fn a_post_reaches_the_api_and_reads_its_answer() {
    let slack = Slack::answering(200, r#"{"ok":true}"#);
    let body = r#"{"channel":"U1","text":"hello"}"#;
    assert_eq!(
        curl(&slack.api).post("chat.postMessage", body),
        Ok(Answer {
            status: 200,
            body: r#"{"ok":true}"#.into()
        })
    );
    let heard = slack.heard();
    assert_eq!(heard.head[0], "POST /api/chat.postMessage HTTP/1.1");
    assert_eq!(
        heard.header("authorization"),
        Some(format!("Bearer {TOKEN}").as_str())
    );
    assert_eq!(
        heard.header("content-type"),
        Some("application/json; charset=utf-8")
    );
    assert_eq!(heard.body, body);
}

#[test]
fn an_answer_of_any_status_is_read_whole() {
    let slack = Slack::answering(429, "slow down\nplease");
    assert_eq!(
        curl(&slack.api).post("chat.postMessage", "{}"),
        Ok(Answer {
            status: 429,
            body: "slow down\nplease".into()
        })
    );
}

#[test]
fn no_host_answering_is_worth_handing_again() {
    let said = curl(&unanswered()).post("chat.postMessage", "{}");
    let Err(Unsent::Later(why)) = said else {
        panic!("{said:?}");
    };
    assert!(
        why.starts_with("curl reached no answer from Slack ("),
        "{why}"
    );
}

#[test]
fn what_curl_says_of_a_failure_never_shows_the_token() {
    // The third line curl is handed is the one with the token.
    let program = program(
        "says-token",
        "read -r a; read -r b; read -r c; echo \"$c\" >&2; exit 28",
    );
    let said = curl("https://slack.com/api")
        .program(&program)
        .post("chat.postMessage", "{}");
    let Err(Unsent::Later(why)) = said else {
        panic!("{said:?}");
    };
    assert!(why.contains("Authorization: Bearer [redacted]"), "{why}");
    assert!(!why.contains(TOKEN), "{why}");
}

#[test]
fn no_curl_is_never_sent_and_says_to_install_it() {
    let said = curl("https://slack.com/api")
        .program("symphony-notice-slack-no-such-curl")
        .post("chat.postMessage", "{}");
    assert_eq!(
        said,
        Err(Unsent::Never(
            "symphony-notice-slack-no-such-curl is not on PATH: this program reaches Slack \
             through curl, so install it"
                .into()
        ))
    );
}

#[test]
fn a_curl_that_cannot_start_is_never_sent() {
    let program = program("not-runnable", "exit 0");
    fs::set_permissions(&program, fs::Permissions::from_mode(0o600)).unwrap();
    let said = curl("https://slack.com/api")
        .program(&program)
        .post("chat.postMessage", "{}");
    let Err(Unsent::Never(why)) = said else {
        panic!("{said:?}");
    };
    assert!(
        why.starts_with(&format!("{program} did not start: ")),
        "{why}"
    );
}

#[test]
fn a_curl_that_reads_no_request_says_why_it_failed() {
    let program = program("reads-nothing", "echo 'curl: no URL specified' >&2; exit 2");
    let said = curl("https://slack.com/api")
        .program(&program)
        .post("chat.postMessage", "{}");
    let Err(Unsent::Later(why)) = said else {
        panic!("{said:?}");
    };
    assert!(why.ends_with("): curl: no URL specified"), "{why}");
}

#[test]
fn a_curl_that_says_no_status_is_worth_handing_again() {
    let program = program("no-status", "cat >/dev/null; printf 'body'");
    let said = curl("https://slack.com/api")
        .program(&program)
        .post("chat.postMessage", "{}");
    let Err(Unsent::Later(why)) = said else {
        panic!("{said:?}");
    };
    assert!(
        why.starts_with("curl said no HTTP status of Slack's answer: "),
        "{why}"
    );
}
