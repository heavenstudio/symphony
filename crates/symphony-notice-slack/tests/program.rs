//! The program as the server runs it: a notice on stdin, a machine folder of the test's own, and
//! its exit status and stderr. Only notices it refuses before reaching Slack are handed to it: no
//! token is ever kept for it here, so it never reaches Slack or sends anyone a message (the send
//! itself is the library's tests', against a stand-in for Slack).

#![expect(
    clippy::unwrap_used,
    reason = "the helpers beside the tests assert as they do: an unwrap is the failure"
)]

use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const ADDRESS: &str = "U024BE7LH";
const LINK: &str = "https://mac.example.ts.net/symphony/cards#notice-sender";

/// An empty machine folder of the test's own, named for `name`.
fn machine(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "symphony-notice-slack-program-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The message of a card going `to` the address `address` by the way `way`.
fn message(way: &str, address: &str) -> String {
    format!(
        r#"{{"notice":1,"id":"0193f1c2-7a4e","kind":"card","project":"symphony","link":"{LINK}","to":{{"way":"{way}","address":"{address}"}}}}"#
    ) + "\n"
}

/// The program's exit status and stderr once handed `stdin`, its machine folder `name`'s.
fn run(name: &str, stdin: &str) -> (i32, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_symphony-notice-slack"))
        .env("SYMPHONY_MACHINE_DIR", machine(name))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let done = child.wait_with_output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&done.stdout),
        "",
        "it prints nothing"
    );
    let errors = String::from_utf8(done.stderr).unwrap();
    for secret in [ADDRESS, LINK, "mac.example.ts.net", "ana@example.com"] {
        assert!(!errors.contains(secret), "{errors} names {secret}");
    }
    (done.status.code().unwrap(), errors)
}

#[test]
fn with_no_token_kept_a_notice_is_never_sent() {
    let (status, errors) = run("no-token", &message("slack", ADDRESS));
    assert_eq!(status, 1, "{errors}");
    assert!(errors.starts_with("no token was read: "), "{errors}");
    assert!(errors.contains("notices"), "{errors}");
    assert!(
        errors.ends_with('\n') && errors.lines().count() == 1,
        "{errors:?}"
    );
}

#[test]
fn a_notice_of_another_way_is_never_sent() {
    let (status, errors) = run("other-way", &message("email", "ana@example.com"));
    assert_eq!(status, 1);
    assert_eq!(
        errors,
        "this program sends by slack, and the notice goes by email\n"
    );
}

#[test]
fn a_channel_is_never_sent_a_notice() {
    let (status, errors) = run("channel", &message("slack", "C024BE7LH"));
    assert_eq!(status, 1);
    assert_eq!(
        errors,
        "the address is no Slack member id (U or W, then capital letters and digits)\n"
    );
}

#[test]
fn a_message_it_does_not_take_is_never_sent() {
    let (status, errors) = run("unread", "not a notice\n");
    assert_eq!(status, 1);
    assert!(errors.starts_with("the message is no JSON"), "{errors}");

    let (status, errors) = run(
        "format-2",
        &message("slack", ADDRESS).replace(r#""notice":1"#, r#""notice":2"#),
    );
    assert_eq!(status, 1);
    assert!(errors.contains("format 2"), "{errors}");
}
