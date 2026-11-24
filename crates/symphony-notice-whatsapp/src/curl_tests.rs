//! `post` run against a stand-in for `curl`: a script that keeps what it was handed and prints or
//! fails as the case asks. The plugin's own tests run the real `curl` against a stand-in for the
//! Graph API.

use super::*;
use crate::scratch::{Scratch, mode};
use std::path::{Path, PathBuf};

const TOKEN_TEXT: &str = "EAAG-test-token-not-a-real-one";
const URL: &str = "https://graph.facebook.com/v27.0/1234567890/messages";

fn token() -> Secret {
    Secret::new(TOKEN_TEXT).unwrap()
}

/// A `curl` in `scratch` that keeps its stdin in `stdin` and its arguments in `args`, then runs
/// `then`.
fn curl(scratch: &Scratch, then: &str) -> PathBuf {
    let dir = scratch.path().display();
    let program = scratch.path().join("curl");
    std::fs::write(
        &program,
        format!("#!/bin/sh\ncat > '{dir}/stdin'\nprintf '%s\\n' \"$@\" > '{dir}/args'\n{then}\n"),
    )
    .unwrap();
    mode(&program, 0o700);
    program
}

fn kept(scratch: &Scratch, name: &str) -> String {
    std::fs::read_to_string(scratch.path().join(name)).unwrap()
}

fn post_by(program: &Path, body: &str) -> Result<Answer, Unsent> {
    post(program.to_str().unwrap(), URL, &token(), body)
}

#[test]
fn the_request_and_its_token_go_on_stdin_and_the_answer_is_its_status_and_body() {
    let scratch = Scratch::new();
    let program = curl(&scratch, r#"printf '{"messages":[{"id":"wamid.1"}]}\n200'"#);
    let body = r#"{"to":"+15551234567"}"#;
    assert_eq!(
        post_by(&program, body),
        Ok(Answer {
            status: 200,
            body: r#"{"messages":[{"id":"wamid.1"}]}"#.into()
        })
    );
    assert_eq!(kept(&scratch, "stdin"), config(URL, &token(), body));
    let args = kept(&scratch, "args");
    let args: Vec<&str> = args.lines().collect();
    assert_eq!(args.first(), Some(&"-q"), "{args:?}");
    for (flag, value) in [
        ("--config", "-"),
        ("--max-redirs", "0"),
        ("--max-time", SECONDS),
        ("--write-out", "\\n%{response_code}"),
    ] {
        let at = args.iter().position(|arg| *arg == flag).unwrap();
        assert_eq!(args[at + 1], value);
    }
    assert!(args.contains(&"--globoff"), "{args:?}");
    assert!(
        !args
            .iter()
            .any(|arg| arg.contains(TOKEN_TEXT) || arg.contains("1555"))
    );
}

#[test]
fn the_config_quotes_every_value_and_sends_no_expect() {
    let body = r#"{"text":"a \"quoted\" \\ back"}"#;
    let lines = [
        format!(r#"url = "{URL}""#),
        r#"request = "POST""#.into(),
        format!(r#"header = "Authorization: Bearer {TOKEN_TEXT}""#),
        r#"header = "Content-Type: application/json""#.into(),
        r#"header = "Expect:""#.into(),
        r#"header = "User-Agent: symphony-notice-whatsapp""#.into(),
        r#"data-binary = "{\"text\":\"a \\\"quoted\\\" \\\\ back\"}""#.into(),
    ];
    assert_eq!(config(URL, &token(), body), lines.join("\n") + "\n");
}

#[test]
fn an_answer_of_any_status_is_the_apis_to_judge() {
    let scratch = Scratch::new();
    let program = curl(&scratch, r#"printf '{"error":{"code":131026}}\n400'"#);
    let answer = post_by(&program, "{}").unwrap();
    assert_eq!(
        (answer.status, answer.body.as_str()),
        (400, r#"{"error":{"code":131026}}"#)
    );
    let empty = curl(&scratch, r"printf '\n503'");
    assert_eq!(
        post_by(&empty, "{}").unwrap(),
        Answer {
            status: 503,
            body: String::new()
        }
    );
}

#[test]
fn no_answer_is_later_saying_whether_it_may_have_reached_meta_and_never_the_token() {
    let scratch = Scratch::new();
    for (exit, reached) in [
        (5, "nothing reached it"),
        (6, "nothing reached it"),
        (7, "nothing reached it"),
        (28, "it may have taken the notice"),
        (52, "it may have taken the notice"),
    ] {
        let program = curl(
            &scratch,
            &format!("echo \"curl: ({exit}) failed with Bearer {TOKEN_TEXT}\" >&2\nexit {exit}"),
        );
        let Err(Unsent::Later(why)) = post_by(&program, "{}") else {
            panic!("{exit} is a notice not sent this time");
        };
        assert!(
            why.starts_with("WhatsApp did not answer: curl failed (")
                && why.contains("failed with Bearer [hidden]")
                && why.ends_with(reached),
            "{exit}: {why}"
        );
        assert!(!why.contains(TOKEN_TEXT));
    }
    for printed in ["", "no status", r"\n000", r"{}\n"] {
        let program = curl(&scratch, &format!("printf '{printed}'"));
        assert_eq!(
            post_by(&program, "{}"),
            Err(Unsent::Later(
                "WhatsApp did not answer: curl printed no status; it may have taken the notice"
                    .into()
            )),
            "{printed:?}"
        );
    }
}

#[test]
fn a_curl_that_is_not_there_is_never_sent() {
    let scratch = Scratch::new();
    let absent = scratch.path().join("no-curl");
    let Err(Unsent::Never(why)) = post_by(&absent, "{}") else {
        panic!("an absent curl is never sent");
    };
    assert!(why.ends_with("is not on PATH: this program reaches WhatsApp through curl, so install it; nothing was sent"), "{why}");
}

#[test]
fn a_curl_that_does_not_start_is_later() {
    let scratch = Scratch::new();
    let unrunnable = scratch.path().join("curl");
    std::fs::write(&unrunnable, "").unwrap();
    mode(&unrunnable, 0o600);
    let Err(Unsent::Later(why)) = post_by(&unrunnable, "{}") else {
        panic!("a curl that does not start is later");
    };
    assert!(
        why.starts_with(&format!("{} did not start: ", unrunnable.display()))
            && why.ends_with("; nothing was sent"),
        "{why}"
    );
}
