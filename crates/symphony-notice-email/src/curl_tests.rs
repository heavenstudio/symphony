use super::*;
use std::fs;
use std::io::{BufRead as _, BufReader};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::mpsc;
use symphony_notice::To;

/// The settings a machine folder of its own holds: `server`, and a login where `user` is some.
fn settings(name: &str, server: &str, user: Option<&str>) -> Settings {
    let dir = std::env::temp_dir().join(format!(
        "symphony-notice-email-curl-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let notices: PathBuf = dir.join(FOLDER);
    fs::create_dir_all(&notices).unwrap();
    fs::set_permissions(&notices, fs::Permissions::from_mode(0o700)).unwrap();
    let login = user.map_or(String::new(), |user| format!("user = \"{user}\"\n"));
    let text = format!("server = \"{server}\"\nfrom = \"symphony@example.com\"\n{login}");
    if user.is_some() {
        fs::write(notices.join(TOKEN), "pa\"ss\\word\n").unwrap();
        fs::set_permissions(notices.join(TOKEN), fs::Permissions::from_mode(0o600)).unwrap();
    }
    fs::write(notices.join(SETTINGS), text).unwrap();
    fs::set_permissions(notices.join(SETTINGS), fs::Permissions::from_mode(0o600)).unwrap();
    crate::settings::read(&dir).unwrap()
}

fn mail() -> Mail {
    Mail {
        headers: vec!["Subject: A \"card\"".into(), "To: ana@example.com".into()],
        body: "Line; one\r\n.\r\n@file <x\r\n".into(),
    }
}

#[test]
fn the_whole_request_is_one_config_curl_reads_on_stdin() {
    let config = config(
        &settings("config", "smtp://smtp.example.com:587", Some("apikey")),
        "ana@example.com",
        &mail(),
    );
    assert_eq!(
        config,
        "url = \"smtp://smtp.example.com:587\"\n\
         mail-from = \"symphony@example.com\"\n\
         mail-rcpt = \"ana@example.com\"\n\
         user = \"apikey:pa\\\"ss\\\\word\"\n\
         ssl-reqd\n\
         header = \"Subject: A \\\"card\\\"\"\n\
         header = \"To: ana@example.com\"\n\
         form-string = \"=Line; one\\r\\n.\\r\\n@file <x\\r\\n\"\n"
    );
}

#[test]
fn implicit_tls_and_a_loopback_relay_ask_for_no_starttls_and_no_login_sends_none() {
    for server in ["smtps://smtp.example.com", "smtp://127.0.0.1:2525"] {
        let config = config(&settings("tls", server, None), "a@b", &mail());
        assert!(
            !config.contains("ssl-reqd") && !config.contains("user ="),
            "{config}"
        );
    }
}

#[test]
fn a_value_is_quoted_as_curl_reads_it_back() {
    assert_eq!(quoted("plain"), "plain");
    assert_eq!(quoted("a\\b\"c\r\n\td"), "a\\\\b\\\"c\\r\\n\\td");
}

/// curl's verbose account of a conversation that ended in `answer` to `command`, its last line
/// curl's own, which names the code alone, as curl's do.
fn account(command: &str, answer: &str) -> String {
    let code = answer.get(..3).unwrap();
    format!(
        "*   Trying 127.0.0.1:2525...\n\
         * Connected to 127.0.0.1 (127.0.0.1) port 2525\n\
         < 220 mail.example.com ESMTP\n\
         > EHLO host\n\
         < 250-mail.example.com\n\
         < 250 AUTH PLAIN\n\
         > AUTH PLAIN\n\
         < 334 \n\
         > AHVzZXIAcGFzcw==\n\
         {command}\
         < {answer}\n\
         > QUIT\n\
         < 221 2.0.0 Bye\n\
         curl: (55) failed: {code}\n"
    )
}

fn later(code: Option<i32>, said: &str) -> String {
    match outcome(code, said) {
        Err(Unsent::Later(why)) => why,
        other => panic!("{other:?}"),
    }
}

fn never(code: Option<i32>, said: &str) -> String {
    match outcome(code, said) {
        Err(Unsent::Never(why)) => why,
        other => panic!("{other:?}"),
    }
}

#[test]
fn curl_ending_well_is_sent() {
    assert_eq!(outcome(Some(0), "< 250 ok\n"), Ok(()));
}

#[test]
fn the_last_answer_decides_a_4xx_later_and_a_5xx_never() {
    let rcpt = "< 235 ok\n> MAIL FROM:<s@example.com>\n< 250 ok\n> RCPT TO:<ana@example.com>\n";
    let why = later(
        Some(55),
        &account(rcpt, "451 4.7.1 Try later, ana@example.com"),
    );
    assert!(
        why.contains("answered 451 to RCPT, asking to be called later"),
        "{why}"
    );
    let why = never(
        Some(55),
        &account(rcpt, "550 5.1.1 <ana@example.com> unknown"),
    );
    assert!(
        why.contains("answered 550 to RCPT, refusing the person's address"),
        "{why}"
    );
    assert!(
        !why.contains("unknown"),
        "the server's words are never said: {why}"
    );
}

#[test]
fn a_refused_login_is_named_by_auth_and_never_by_the_encoded_line() {
    let why = never(Some(67), &account("", "535 5.7.8 Authentication failed"));
    assert!(
        why.contains(
            "answered 535 to AUTH, refusing the login: check `user` in notices/email.toml"
        ),
        "{why}"
    );
    assert!(
        !why.contains("AHVzZXIAcGFzcw") && !why.contains("Authentication failed"),
        "{why}"
    );
    let why = later(
        Some(67),
        &account("", "454 4.7.0 Temporary authentication failure"),
    );
    assert!(why.contains("answered 454 to AUTH"), "{why}");
}

#[test]
fn the_answer_after_the_body_is_the_messages() {
    let data = "< 235 ok\n> MAIL FROM:<s@e>\n< 250 ok\n> RCPT TO:<a@e>\n< 250 ok\n> DATA\n\
                < 354 go ahead\n} [234 bytes data]\n* upload completely sent off: 234 bytes\n";
    let why = never(
        Some(8),
        &account(data, "554 5.7.1 Message rejected as spam"),
    );
    assert!(
        why.contains("answered 554 to the message, refusing the message"),
        "{why}"
    );
    let why = later(Some(8), &account(data, "451 4.3.0 Queue full"));
    assert!(why.contains("answered 451 to the message"), "{why}");
    let data = "> DATA\n";
    let why = never(Some(8), &account(data, "503 5.5.1 No recipients"));
    assert!(why.contains("answered 503 to DATA ("), "{why}");
}

#[test]
fn a_refused_from_and_a_refused_greeting_are_named() {
    let why = never(
        Some(55),
        &account("< 235 ok\n> MAIL FROM:<s@e>\n", "553 not yours"),
    );
    assert!(
        why.contains("553 to MAIL, refusing `from` in notices/email.toml"),
        "{why}"
    );
    let why = later(
        Some(8),
        "< 421 4.3.2 Too busy\ncurl: (8) Weird server reply\n",
    );
    assert!(why.contains("answered 421 on connecting"), "{why}");
    let why = never(
        Some(8),
        "< 554 No SMTP service here\ncurl: (8) Weird server reply\n",
    );
    assert!(why.contains("answered 554 on connecting ("), "{why}");
}

#[test]
fn an_answer_to_quit_and_a_refusal_curl_moved_past_decide_nothing() {
    // An EHLO the server does not know, which curl follows with HELO, and a connection that broke
    // after: the last answer was a 250, so the exit decides.
    let said = "< 220 hi\n> EHLO host\n< 502 unknown\n> HELO host\n< 250 ok\n> QUIT\n< 500 bye\n\
                curl: (56) Recv failure: Connection reset by peer\n";
    let why = later(Some(56), said);
    assert!(
        why.contains("not reached or stopped answering (curl: (56) Recv failure"),
        "{why}"
    );
}

#[test]
fn with_no_answer_the_exit_decides() {
    for code in [6, 7, 28, 35, 52, 55, 56] {
        let why = later(Some(code), &format!("curl: ({code}) something\n"));
        assert!(
            why.contains(&format!("(curl: ({code}) something)")),
            "{why}"
        );
    }
    let why = later(None, "");
    assert!(why.contains("curl ended: None"), "{why}");
    for code in [1, 2, 3, 8, 60, 64, 67] {
        let why = never(Some(code), &format!("curl: ({code}) refused\n"));
        assert!(why.contains("curl could not send it"), "{why}");
    }
}

#[test]
fn an_answer_line_that_is_no_code_is_passed_over() {
    let why = later(
        Some(7),
        "< \n< 4x1 odd\n< ok\ncurl: (7) Failed to connect\n",
    );
    assert!(why.contains("not reached"), "{why}");
}

#[test]
fn a_hidden_value_is_taken_out_in_either_case() {
    let hidden = ["pa\"ss", "ana@example.com", "https://h/x", ""];
    assert_eq!(
        redact(
            "ANA@Example.com said pa\"ss, at https://h/x and ana@example.com.",
            &hidden
        ),
        "<hidden> said <hidden>, at <hidden> and <hidden>."
    );
    assert_eq!(redact("nothing here", &hidden), "nothing here");
    assert_eq!(redact("café ana@example.com é", &hidden), "café <hidden> é");
}

#[test]
fn a_mail_server_that_offers_no_starttls_where_it_is_needed_is_told_nothing() {
    // A mail server that greets and answers EHLO with no STARTTLS, and keeps what it heard after.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (said, heard) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.write_all(b"220 hi\r\n").unwrap();
        let mut lines = BufReader::new(stream.try_clone().unwrap()).lines();
        let ehlo = lines.next().unwrap().unwrap();
        stream.write_all(b"250-hi\r\n250 AUTH PLAIN\r\n").unwrap();
        let rest: Vec<String> = lines.map_while(Result::ok).collect();
        let _ = said.send((ehlo, rest));
    });
    let settings = settings(
        "starttls",
        &format!("smtp://127.0.0.1:{port}"),
        Some("apikey"),
    )
    .requiring_starttls();
    let to = To::new("email", "ana@example.com").unwrap();
    let notice = Notice::new("n", symphony_notice::Kind::Card, "p", "https://h/", to).unwrap();
    let sent = send("curl", &settings, &notice, &mail());
    let Err(Unsent::Never(why)) = sent else {
        panic!("{sent:?}");
    };
    assert!(why.contains("curl could not send it (curl: (64)"), "{why}");
    let (ehlo, rest) = heard.recv_timeout(test_time::FLOOR).unwrap();
    assert!(ehlo.starts_with("EHLO "), "{ehlo}");
    let told = rest.join("\n");
    for never in ["AUTH", "MAIL", "RCPT", "ana@", "pa\"ss"] {
        assert!(
            !told.contains(never),
            "{never} was sent in the clear: {told}"
        );
    }
}
