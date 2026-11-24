//! The plugin as the server runs it: the program, a notice on its stdin and a real `curl`, against
//! a fake mail server on a private loopback port that answers as it is told and keeps what it was
//! sent. No mail reaches anyone, and the password is a made-up one.

#![expect(
    clippy::unwrap_used,
    reason = "the helpers beside the tests assert as they do: an unwrap is the failure"
)]

use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

const ADDRESS: &str = "ana@example.com";
const LINK: &str = "https://mac.example.ts.net/symphony/cards#notice-sender";
const PASSWORD: &str = "pa\"ss word";
/// `\0apikey\0` and the password, as AUTH PLAIN carries them.
const LOGIN: &str = "AGFwaWtleQBwYSJzcyB3b3Jk";
const ID: &str = "0193f1c2-7a4e-7b3c-9d2e-5f6a7b8c9d0e";

fn message(way: &str) -> String {
    format!(
        "{{\"notice\":1,\"id\":\"{ID}\",\"kind\":\"card\",\"project\":\"symphony\",\
         \"link\":\"{LINK}\",\"to\":{{\"way\":\"{way}\",\"address\":\"{ADDRESS}\"}}}}\n"
    )
}

/// What the fake mail server answers where it does not take what it was sent.
#[derive(Clone, Copy)]
enum Answer {
    Takes,
    Greeting(u16),
    Auth(u16),
    Rcpt(u16),
    Message(u16),
}

/// A mail server on a loopback port, for one connection.
struct Fake {
    port: u16,
    heard: mpsc::Receiver<Vec<String>>,
}

impl Fake {
    fn start(answer: Answer) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (said, heard) = mpsc::channel();
        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let _ = said.send(serve(stream, answer));
        });
        Fake { port, heard }
    }

    /// Every line the client sent, once it hung up.
    fn heard(&self) -> Vec<String> {
        self.heard
            .recv_timeout(test_time::FLOOR)
            .expect("the fake mail server heard a whole conversation")
    }
}

fn serve(stream: TcpStream, answer: Answer) -> Vec<String> {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut writer = stream;
    let mut say = |text: &str| writer.write_all(format!("{text}\r\n").as_bytes());
    let mut heard = Vec::new();
    if let Answer::Greeting(code) = answer {
        let _ = say(&format!("{code} not now"));
        return heard;
    }
    say("220 fake.example ESMTP").unwrap();
    let mut body = false;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return heard;
        }
        let line = line.trim_end_matches(['\r', '\n']).to_string();
        heard.push(line.clone());
        if body {
            if line == "." {
                body = false;
                let _ = match answer {
                    Answer::Message(code) => say(&format!("{code} 5.7.1 {ADDRESS} not taken")),
                    _ => say("250 2.0.0 queued"),
                };
            }
            continue;
        }
        let command = line
            .split(' ')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        let _ = match (command.as_str(), answer) {
            ("EHLO", _) => say("250-fake.example\r\n250 AUTH PLAIN"),
            ("AUTH", _) => {
                say("334 ").unwrap();
                let mut login = String::new();
                reader.read_line(&mut login).unwrap();
                heard.push(login.trim_end().to_string());
                match answer {
                    Answer::Auth(code) => say(&format!("{code} 5.7.8 {ADDRESS} not you")),
                    _ => say("235 2.7.0 ok"),
                }
            }
            ("RCPT", Answer::Rcpt(code)) => say(&format!("{code} 5.1.1 <{ADDRESS}> unknown")),
            ("DATA", _) => {
                body = true;
                say("354 go ahead")
            }
            ("QUIT", _) => {
                let _ = say("221 bye");
                return heard;
            }
            _ => say("250 ok"),
        };
    }
}

/// A machine folder whose `notices/` sends by the mail server at `port`, logging in.
fn machine(port: u16) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "symphony-notice-email-send-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let notices = dir.join("notices");
    std::fs::create_dir_all(&notices).unwrap();
    std::fs::set_permissions(&notices, std::fs::Permissions::from_mode(0o700)).unwrap();
    let private = |name: &str, text: &str| {
        std::fs::write(notices.join(name), text).unwrap();
        std::fs::set_permissions(notices.join(name), std::fs::Permissions::from_mode(0o600))
            .unwrap();
    };
    private(
        "email.toml",
        &format!(
            "server = \"smtp://127.0.0.1:{port}\"\nfrom = \"symphony@example.com\"\n\
             user = \"apikey\"\n"
        ),
    );
    private("email.token", &format!("{PASSWORD}\n"));
    dir
}

/// The program's exit status and stderr, handed `message` on stdin with `machine` as its machine
/// folder and `path` as its `PATH` where some.
fn run(machine: &Path, message: &str, path: Option<&Path>) -> (i32, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_symphony-notice-email"));
    command
        .env("SYMPHONY_MACHINE_DIR", machine)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = path {
        command.env("PATH", path);
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(message.as_bytes())
        .unwrap();
    let done = child.wait_with_output().unwrap();
    assert!(done.stdout.is_empty(), "{done:?}");
    let said = String::from_utf8(done.stderr).unwrap();
    for hidden in [
        ADDRESS,
        LINK,
        PASSWORD,
        LOGIN,
        "not taken",
        "not you",
        "unknown",
    ] {
        assert!(!said.contains(hidden), "stderr says {hidden:?}: {said}");
    }
    (done.status.code().unwrap(), said)
}

#[test]
fn a_notice_is_sent_as_one_mail_to_its_address() {
    let fake = Fake::start(Answer::Takes);
    let (code, said) = run(&machine(fake.port), &message("email"), None);
    assert_eq!((code, said.as_str()), (0, ""));
    let heard = fake.heard();
    let has = |line: &str| heard.iter().any(|heard| heard == line);
    for line in [
        "AUTH PLAIN",
        LOGIN,
        "MAIL FROM:<symphony@example.com>",
        &format!("RCPT TO:<{ADDRESS}>"),
        "DATA",
        "From: Symphony <symphony@example.com>",
        &format!("To: {ADDRESS}"),
        "Subject: A card waits on you in symphony",
        &format!("Message-ID: <{ID}@example.com>"),
        "Auto-Submitted: auto-generated",
        "A card waits on you in symphony:",
        LINK,
        ".",
        "QUIT",
    ] {
        assert!(has(line), "{line:?} not in {heard:#?}");
    }
    assert!(
        heard
            .iter()
            .any(|line| line.starts_with("Date: ") && line.ends_with(" +0000"))
    );
    assert_eq!(
        heard.iter().filter(|line| line.starts_with("RCPT")).count(),
        1
    );
}

#[test]
fn a_refusal_for_now_is_tried_again_and_a_refusal_is_not() {
    for (answer, status, words) in [
        (
            Answer::Rcpt(550),
            1,
            "answered 550 to RCPT, refusing the person's address",
        ),
        (
            Answer::Rcpt(452),
            75,
            "answered 452 to RCPT, asking to be called later",
        ),
        (
            Answer::Auth(535),
            1,
            "answered 535 to AUTH, refusing the login",
        ),
        (Answer::Auth(454), 75, "answered 454 to AUTH"),
        (
            Answer::Message(554),
            1,
            "answered 554 to the message, refusing the message",
        ),
        (Answer::Message(451), 75, "answered 451 to the message"),
        (Answer::Greeting(421), 75, "answered 421 on connecting"),
        (Answer::Greeting(554), 1, "answered 554 on connecting"),
    ] {
        let fake = Fake::start(answer);
        let (code, said) = run(&machine(fake.port), &message("email"), None);
        assert_eq!(code, status, "{said}");
        assert!(said.contains(words), "{said}");
        let heard = fake.heard();
        let to_body = heard.iter().any(|line| line == LINK);
        assert_eq!(to_body, matches!(answer, Answer::Message(_)), "{heard:#?}");
    }
}

#[test]
fn a_mail_server_nobody_answers_at_is_tried_again() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let (code, said) = run(&machine(port), &message("email"), None);
    assert_eq!(code, 75, "{said}");
    assert!(
        said.contains("not reached") && said.contains("curl: (7)"),
        "{said}"
    );
}

#[test]
fn what_cannot_be_sent_is_refused_before_anything_is_reached() {
    let empty = machine(9);
    std::fs::remove_dir_all(empty.join("notices")).unwrap();
    let (code, said) = run(&empty, &message("email"), None);
    assert_eq!(code, 1);
    assert!(said.contains("the email plugin is not set up"), "{said}");

    let (code, said) = run(&machine(9), &message("slack"), None);
    assert_eq!(code, 1);
    assert_eq!(
        said,
        "this plugin sends a notice by `email`, and this one is by `slack`\n"
    );

    let (code, said) = run(&machine(9), "{\"notice\":2}\n", None);
    assert_eq!(code, 1);
    assert!(said.contains("format 2"), "{said}");

    let nowhere = machine(9).join("bin");
    std::fs::create_dir_all(&nowhere).unwrap();
    let (code, said) = run(&machine(9), &message("email"), Some(&nowhere));
    assert_eq!(code, 1);
    assert!(said.contains("curl is not on PATH"), "{said}");
}
