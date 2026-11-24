use super::*;
use crate::stand_in::{Folder, Slack, TOKEN, folder, unanswered};
use std::fs;
use std::os::unix::fs::PermissionsExt;

fn curl(api: &str) -> Curl {
    Curl::new(api, Secret::new(TOKEN).unwrap())
}

/// A `curl` in `dir` that runs `script` under `sh`.
fn program(dir: &Folder, script: &str) -> String {
    let path = dir.join("curl");
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
             noproxy = \"*\"\n\
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
    // A curl that says the line it was handed with the token on it.
    let dir = folder("says-token");
    let program = program(&dir, "grep Authorization >&2; exit 28");
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
    let dir = folder("not-runnable");
    let program = program(&dir, "exit 0");
    fs::set_permissions(&program, fs::Permissions::from_mode(0o600)).unwrap();
    // A file in curl's place that is not executable, or a folder: each fails the same on every
    // hand.
    for unrunnable in [program.as_str(), dir.to_str().unwrap()] {
        let said = curl("https://slack.com/api")
            .program(unrunnable)
            .post("chat.postMessage", "{}");
        let Err(Unsent::Never(why)) = said else {
            panic!("{unrunnable}: {said:?}");
        };
        assert!(
            why.starts_with(&format!("{unrunnable} did not start: ")),
            "{why}"
        );
    }
}

#[test]
fn a_curl_that_reads_no_request_says_why_it_failed() {
    let dir = folder("reads-nothing");
    let program = program(&dir, "echo 'curl: (2) no URL specified' >&2; exit 2");
    let said = curl("https://slack.com/api")
        .program(&program)
        .post("chat.postMessage", "{}");
    let Err(Unsent::Never(why)) = said else {
        panic!("{said:?}");
    };
    assert!(why.ends_with(": curl: (2) no URL specified"), "{why}");
}

#[test]
fn a_fault_curl_meets_again_is_never_sent_and_any_other_is_worth_handing_again() {
    let dir = folder("exits");
    let exits = |codes: &[i32]| -> Vec<(i32, Result<Answer, Unsent>)> {
        (codes.iter())
            .map(|&code| {
                let program = program(&dir, &format!("cat >/dev/null; exit {code}"));
                let said = curl("https://slack.com/api")
                    .program(&program)
                    .post("chat.postMessage", "{}");
                (code, said)
            })
            .collect()
    };
    for (code, said) in exits(&[1, 2, 3, 4, 26, 48, 60, 64, 77]) {
        let Err(Unsent::Never(why)) = said else {
            panic!("{code}: {said:?}");
        };
        assert!(
            why.starts_with("curl could not make the call to Slack (")
                && why.contains("second hand"),
            "{code}: {why}"
        );
    }
    for (code, said) in exits(&[5, 6, 7, 8, 16, 18, 28, 35, 51, 52, 55, 56, 92]) {
        let Err(Unsent::Later(why)) = said else {
            panic!("{code}: {said:?}");
        };
        assert!(
            why.starts_with("curl reached no answer from Slack ("),
            "{code}: {why}"
        );
    }
}

#[test]
fn a_proxy_the_environment_names_is_never_used() {
    // curl takes a proxy from `http_proxy`, `HTTPS_PROXY`, `ALL_PROXY` and their kin; `*` turns
    // it off for every host, so the token goes to the API's host alone.
    let config = curl("https://slack.com/api").config("chat.postMessage", "{}");
    let lines: Vec<&str> = config.lines().collect();
    assert_eq!(lines.get(1), Some(&"noproxy = \"*\""), "{config}");
    assert_eq!(
        lines.iter().filter(|line| line.contains("proxy")).count(),
        1,
        "{config}"
    );
}

#[test]
fn a_curl_that_says_no_status_is_worth_handing_again() {
    let dir = folder("no-status");
    let program = program(&dir, "cat >/dev/null; printf 'body'");
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
