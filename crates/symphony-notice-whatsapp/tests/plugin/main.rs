//! The plugin as the server runs it: the built program, a notice on its stdin, its settings and
//! token in a scratch machine folder, and the real `curl` reaching a stand-in for the Graph API on
//! a private port ([`fake`]). Its exit status and its stderr are what the server reads
//! (docs/engineering/NOTICES.md, "The hand-off").

mod boundary;
mod fake;
mod scratch;

use fake::Fake;
use scratch::{Scratch, mode};
use serde_json::{Value, json};
use std::ffi::OsStr;
use std::io::Write as _;
use std::net::TcpListener;
use std::process::{Command, Stdio};

const ID: &str = "0193f1c2-7a4e-7b3c-9d2e-5f6a7b8c9d0e";
const NUMBER: &str = "+15551234567";
const LINK: &str = "https://mac.example.ts.net/symphony/cards#notice-sender";
/// A token as a test writes one: no real token is in the tree.
const TOKEN: &str = "EAAG-test-token-not-a-real-one";

/// What Meta answers a message it took.
const TAKEN: &str = r#"{"messaging_product":"whatsapp","contacts":[{"input":"+15551234567","wa_id":"15551234567"}],"messages":[{"id":"wamid.HBgLMTU1NTEyMzQ1NjcVAgARGBI"}]}"#;

/// The message of a notice of `kind` by `way` to `address`.
fn notice(kind: &str, way: &str, address: &str) -> String {
    json!({
        "notice": 1,
        "id": ID,
        "kind": kind,
        "project": "symphony",
        "link": LINK,
        "to": {"way": way, "address": address},
    })
    .to_string()
        + "\n"
}

/// A machine folder whose `notices/` holds settings sending through `api` and the token.
fn machine(api: &str) -> Scratch {
    let scratch = Scratch::new();
    let settings = format!(
        "phone_number_id = \"1234567890\"\nlanguage = \"en_US\"\napi = \"{api}\"\n\n\
         [templates]\nother = \"symphony_waits\"\ncard = \"symphony_card\"\n"
    );
    scratch.notices(&[("whatsapp.toml", &settings), ("whatsapp.token", TOKEN)]);
    scratch
}

/// What the program answered: its exit status and what it said on stderr.
struct Ran {
    status: Option<i32>,
    said: String,
}

/// The program run as the server runs it, with `machine` as its machine folder and `message` on
/// its stdin: no argument, and no environment but `PATH`, so no proxy is read and no other folder.
fn run(machine: &Scratch, message: &str) -> Ran {
    run_in(machine.path().as_os_str(), message, &[])
}

/// [`run`] with `SYMPHONY_MACHINE_DIR` naming `machine`, whatever it is, and each of `env` set.
fn run_in(machine: &OsStr, message: &str, env: &[(&str, &str)]) -> Ran {
    let mut child = Command::new(env!("CARGO_BIN_EXE_symphony-notice-whatsapp"))
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("SYMPHONY_MACHINE_DIR", machine)
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the plugin starts");
    (child.stdin.take().expect("its stdin"))
        .write_all(message.as_bytes())
        .expect("the notice written");
    let done = child.wait_with_output().expect("the plugin ends");
    assert!(done.stdout.is_empty(), "it prints nothing on stdout");
    let said = String::from_utf8_lossy(&done.stderr).into_owned();
    for secret in [TOKEN, NUMBER, "15551234567", LINK] {
        assert!(!said.contains(secret), "it says what it must not: {said}");
    }
    Ran {
        status: done.status.code(),
        said,
    }
}

#[test]
fn a_card_is_sent_by_its_template_to_the_number_with_the_link_and_nothing_is_said() {
    let fake = Fake::answering(200, TAKEN);
    let machine = machine(&fake.api);
    let ran = run(&machine, &notice("card", "whatsapp", "+1 (555) 123-4567"));
    assert_eq!((ran.status, ran.said.as_str()), (Some(0), ""));

    let request = fake.request();
    assert_eq!(request.line, "POST /v27.0/1234567890/messages HTTP/1.1");
    assert_eq!(request.headers["authorization"], format!("Bearer {TOKEN}"));
    assert_eq!(request.headers["content-type"], "application/json");
    assert!(!request.headers.contains_key("expect"));
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(
        body,
        json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": NUMBER,
            "type": "template",
            "template": {
                "name": "symphony_card",
                "language": {"code": "en_US"},
                "components": [{"type": "body", "parameters": [{"type": "text", "text": LINK}]}],
            },
        })
    );
    assert!(fake.read_none(), "one request for one notice");
}

#[test]
fn a_proxy_the_servers_environment_names_is_never_used() {
    // Every variable curl takes a proxy from names a loopback port nothing listens on: a curl that
    // took one would reach no API at all.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let proxy = format!("http://127.0.0.1:{port}");
    let fake = Fake::answering(200, TAKEN);
    let machine = machine(&fake.api);
    let ran = run_in(
        machine.path().as_os_str(),
        &notice("card", "whatsapp", NUMBER),
        &[
            ("http_proxy", &proxy),
            ("https_proxy", &proxy),
            ("HTTPS_PROXY", &proxy),
            ("all_proxy", &proxy),
            ("ALL_PROXY", &proxy),
        ],
    );
    assert_eq!((ran.status, ran.said.as_str()), (Some(0), ""));
    assert_eq!(
        fake.request().headers["authorization"],
        format!("Bearer {TOKEN}")
    );
}

#[test]
fn a_kind_this_build_does_not_know_is_sent_by_the_template_of_any_other() {
    let fake = Fake::answering(200, TAKEN);
    let machine = machine(&fake.api);
    assert_eq!(
        run(&machine, &notice("digest", "whatsapp", NUMBER)).status,
        Some(0)
    );
    let body: Value = serde_json::from_str(&fake.request().body).unwrap();
    assert_eq!(body["template"]["name"], "symphony_waits");
}

/// What Meta answers of an error: its code and words, with the number, the link and the token in
/// them, as no answer of Meta's is known to carry but one might.
fn refusal(code: u64) -> String {
    json!({
        "error": {
            "message": format!("({code}) Refused for {NUMBER}"),
            "type": "OAuthException",
            "code": code,
            "error_data": {
                "messaging_product": "whatsapp",
                "details": format!("Sent to 15551234567 about {LINK} with {TOKEN}"),
            },
            "fbtrace_id": "Az8or2yhqkZfEZ-_4Qn_Bam",
        }
    })
    .to_string()
}

#[test]
fn a_refusal_is_a_1_saying_metas_code_without_the_number_the_link_or_the_token() {
    let fake = Fake::answering(400, &refusal(132_001));
    let machine = machine(&fake.api);
    let ran = run(&machine, &notice("card", "whatsapp", NUMBER));
    assert_eq!(ran.status, Some(1));
    assert_eq!(
        ran.said,
        "WhatsApp refused the notice: HTTP 400, code 132001: (132001) Refused for [hidden] Sent \
         to [hidden] about [hidden] with [hidden]\n"
    );
}

#[test]
fn a_rate_limit_or_a_service_down_is_a_75() {
    for (status, body) in [
        (400, refusal(130_429)),
        (400, refusal(131_056)),
        (429, String::new()),
        (503, "<html>Service Unavailable</html>".into()),
    ] {
        let fake = Fake::answering(status, &body);
        let machine = machine(&fake.api);
        let ran = run(&machine, &notice("card", "whatsapp", NUMBER));
        assert_eq!(ran.status, Some(75), "{status}: {}", ran.said);
        assert!(
            ran.said
                .starts_with(&format!("WhatsApp refused the notice: HTTP {status}"))
        );
    }
}

#[test]
fn no_answer_is_a_75_saying_nothing_reached_meta() {
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let machine = machine(&format!("http://127.0.0.1:{port}/v27.0"));
    let ran = run(&machine, &notice("card", "whatsapp", NUMBER));
    assert_eq!(ran.status, Some(75), "{}", ran.said);
    assert!(
        ran.said
            .starts_with("WhatsApp did not answer: curl failed (")
            && ran.said.ends_with("; nothing reached it\n"),
        "{}",
        ran.said
    );
}

#[test]
fn a_notice_it_cannot_send_is_a_1_and_reaches_nothing() {
    let fake = Fake::answering(200, TAKEN);
    let machine = machine(&fake.api);
    for (message, said) in [
        (
            notice("card", "email", "ana@example.com"),
            "the notice goes by `email`, and this program sends by `whatsapp`\n",
        ),
        (
            notice("card", "whatsapp", "15551234567"),
            "the address is no WhatsApp number: it does not start with `+` and its country code\n",
        ),
        (
            "{\"notice\":2}\n".into(),
            "the notice is of format 2, and this program reads format 1: take a build of it that \
             reads format 2\n",
        ),
    ] {
        let ran = run(&machine, &message);
        assert_eq!((ran.status, ran.said.as_str()), (Some(1), said));
    }
    assert!(fake.read_none());
}

#[test]
fn settings_or_a_token_that_do_not_read_are_a_1_naming_the_file_and_reach_nothing() {
    let fake = Fake::answering(200, TAKEN);
    let machine = machine(&fake.api);
    let notices = machine.path().join("notices");
    let card = notice("card", "whatsapp", NUMBER);

    mode(&notices.join("whatsapp.token"), 0o644);
    let ran = run(&machine, &card);
    let token = notices.join("whatsapp.token");
    assert_eq!(
        (ran.status, ran.said),
        (
            Some(1),
            format!(
                "{} is open to others than its owner; chmod 600 it\n",
                token.display()
            )
        )
    );

    mode(&notices, 0o755);
    let ran = run(&machine, &card);
    assert_eq!(
        (ran.status, ran.said),
        (
            Some(1),
            format!(
                "{} is open to others than its owner; chmod 700 it\n",
                notices.display()
            )
        )
    );

    mode(&notices, 0o700);
    std::fs::remove_file(notices.join("whatsapp.toml")).unwrap();
    let ran = run(&machine, &card);
    assert_eq!(ran.status, Some(1));
    assert!(
        ran.said
            .starts_with(&notices.join("whatsapp.toml").display().to_string()),
        "{}",
        ran.said
    );

    let empty = Scratch::new();
    let ran = run(&empty, &card);
    assert_eq!(ran.status, Some(1));
    assert!(ran.said.contains("NOTICE_WHATSAPP.md"), "{}", ran.said);

    let ran = run_in(OsStr::new("relative/machine"), &card, &[]);
    assert_eq!(
        (ran.status, ran.said.as_str()),
        (
            Some(1),
            "SYMPHONY_MACHINE_DIR names relative/machine, not an absolute folder\n"
        )
    );
    assert!(fake.read_none());
}
