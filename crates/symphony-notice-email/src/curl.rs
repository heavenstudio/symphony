//! How a mail reaches the mail server: `curl` over SMTP, reading the whole request on its stdin
//! (`--config -`): the server, the login with its password, the envelope's addresses, the headers
//! and the body. So none of them is in an argument or the environment of any process, and `-q`
//! keeps a `~/.curlrc` from adding to the request. The body goes as one text part
//! (`form-string`), which curl dot-stuffs; `--proto =smtp,smtps` keeps it to mail, off this
//! machine's loopback the connection is TLS or nothing is sent, and no proxy is used, whatever
//! proxy variable the server's environment sets, so the login goes to the mail server alone.
//!
//! What came of it is read from curl's verbose account of the conversation, held in memory and
//! never printed ([`outcome`]): the mail server's last answer decides, a `4xx` [`Unsent::Later`]
//! and a `5xx` [`Unsent::Never`], as curl's exit status says neither for the login (`67` for a
//! `454` and a `535` alike) nor for the message (`8` for a `451` and a `554` alike). With no such
//! answer, curl's exit status decides, by [`LASTING`]. What is said names the SMTP command and the
//! code, never the server's words, which often repeat the address, and curl's own line goes out
//! with the password, the address and the link taken out.

use std::io::{ErrorKind, Write as _};
use std::process::{Command, Stdio};
use symphony_notice::{Notice, Unsent, unstarted};

use crate::mail::Mail;
use crate::settings::{FOLDER, SETTINGS, Settings, TOKEN, Tls};

/// The longest one send may take, connecting, logging in and handing the mail over.
const MOST_SECONDS: &str = "60";

/// Sends `mail` of `notice` by the mail server `settings` name, running `program` as `curl`. A
/// `curl` that is not there is [`Unsent::Never`], and one that did not start as [`unstarted`] says.
pub(crate) fn send(
    program: &str,
    settings: &Settings,
    notice: &Notice,
    mail: &Mail,
) -> Result<(), Unsent> {
    let request = config(settings, notice.to().address(), mail);
    let mut child = Command::new(program)
        .args([
            "-q",
            "--config",
            "-",
            "--silent",
            "--show-error",
            "--verbose",
            "--max-time",
            MOST_SECONDS,
            "--proto",
            "=smtp,smtps",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            if error.kind() == ErrorKind::NotFound {
                Unsent::Never(format!(
                    "{program} is not on PATH: the email plugin sends through curl, so install it"
                ))
            } else {
                unstarted(&error, format!("{program} did not start: {error}"))
            }
        })?;
    // curl reads the whole of its config before it connects, so writing it all first is safe.
    // A write curl did not read is said by its exit: it then sent nothing.
    let _ = child
        .stdin
        .take()
        .map(|mut stdin| stdin.write_all(request.as_bytes()));
    let done = child
        .wait_with_output()
        .map_err(|error| Unsent::Later(format!("{program} was not waited for: {error}")))?;
    let hidden = [
        settings
            .login()
            .map_or("", |(_, password)| password.expose()),
        notice.to().address(),
        notice.link(),
    ];
    outcome(done.status.code(), &String::from_utf8_lossy(&done.stderr)).map_err(|unsent| {
        match unsent {
            Unsent::Later(why) => Unsent::Later(redact(&why, &hidden)),
            Unsent::Never(why) => Unsent::Never(redact(&why, &hidden)),
        }
    })
}

/// What `curl` reads on its stdin to send `mail` to `to` by `settings`.
pub(crate) fn config(settings: &Settings, to: &str, mail: &Mail) -> String {
    let line = |name: &str, value: &str| format!("{name} = \"{}\"\n", quoted(value));
    let mut lines = vec![
        line("url", settings.server()),
        line("noproxy", "*"),
        line("mail-from", settings.from()),
        line("mail-rcpt", to),
    ];
    if let Some((user, password)) = settings.login() {
        lines.push(line("user", &format!("{user}:{}", password.expose())));
    }
    if settings.tls() == Tls::Required {
        lines.push("ssl-reqd\n".into());
    }
    lines.extend(mail.headers.iter().map(|header| line("header", header)));
    lines.push(line("form-string", &format!("={}", mail.body)));
    lines.concat()
}

/// `value` as a quoted value of a curl config file reads it back: `\` and `"` escaped, and the
/// line breaks and tabs written as their escapes.
fn quoted(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

/// The SMTP commands a refusal is named by: what follows each `>` of curl's account that is one
/// of these. Any other `>` line (the login's own, encoded) is never read for a name.
const COMMANDS: [&str; 8] = [
    "EHLO", "HELO", "STARTTLS", "AUTH", "MAIL", "RCPT", "DATA", "QUIT",
];

/// What curl's exit `code` (none when a signal ended it) and its verbose account `said` on
/// stderr mean for the notice.
pub(crate) fn outcome(code: Option<i32>, said: &str) -> Result<(), Unsent> {
    if code == Some(0) {
        return Ok(());
    }
    // The last answer the mail server gave, and the command it answered: the greeting has none,
    // and the answer after the body is the message's.
    let mut command: Option<&str> = None;
    let mut last: Option<(u16, Option<&str>)> = None;
    for line in said.lines() {
        if let Some(sent) = line.strip_prefix("> ") {
            let word = sent
                .split(' ')
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase();
            if let Some(known) = COMMANDS.into_iter().find(|known| *known == word) {
                command = Some(known);
            }
        } else if let Some(answer) = line.strip_prefix("< ") {
            let Some(number) = (answer.get(..3))
                .filter(|digits| digits.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|digits| digits.parse::<u16>().ok())
            else {
                continue;
            };
            if command == Some("QUIT") {
                continue;
            }
            last = Some((number, command));
            if number == 354 && command == Some("DATA") {
                command = Some("the message");
            }
        }
    }
    let curl = said
        .lines()
        .rev()
        .find(|line| line.starts_with("curl: ("))
        .map_or_else(|| format!("curl ended: {code:?}"), str::to_string);
    if let Some((number, command)) = last.filter(|(number, _)| *number >= 400) {
        let answered = match command {
            None => "on connecting".to_string(),
            Some("the message") => "to the message".to_string(),
            Some(command) => format!("to {command}"),
        };
        return Err(if number < 500 {
            Unsent::Later(format!(
                "not sent this time: the mail server answered {number} {answered}, asking to \
                 be called later ({curl})"
            ))
        } else {
            Unsent::Never(format!(
                "not sent: the mail server answered {number} {answered}{} ({curl})",
                refused(command)
            ))
        });
    }
    match code {
        Some(code) if LASTING.contains(&code) => Err(Unsent::Never(format!(
            "not sent: curl could not send it, and will not on a second hand ({curl})"
        ))),
        _ => Err(Unsent::Later(format!(
            "not sent this time: the mail server was not reached or stopped answering ({curl})"
        ))),
    }
}

/// curl's exits that say a fault a second hand meets again, so the notice is never sent: 1 a
/// protocol this curl lacks or `--proto` refuses, 2 curl failed to initialise, 3 a URL that does
/// not parse, 4 a feature this build lacks, 26 a config line this curl does not take, 48 an option
/// libcurl does not know, 60 a certificate that does not verify, 64 TLS required and not offered,
/// 77 CA certificates that cannot be read. Any other exit, a signal's among them, is worth handing
/// again: a host not resolved or not reached, out of time, a handshake or a connection cut off.
const LASTING: [i32; 9] = [1, 2, 3, 4, 26, 48, 60, 64, 77];

/// What a `5xx` answer to `command` refuses, for whoever reads why.
fn refused(command: Option<&str>) -> String {
    match command {
        Some("AUTH") => format!(
            ", refusing the login: check `user` in {FOLDER}/{SETTINGS} and the password in \
             {FOLDER}/{TOKEN}"
        ),
        Some("MAIL") => format!(", refusing `from` in {FOLDER}/{SETTINGS}"),
        Some("RCPT") => ", refusing the person's address".into(),
        Some("the message") => ", refusing the message".into(),
        _ => String::new(),
    }
}

/// `text` with every one of `hidden` that is not empty written `<hidden>`, its ASCII letters
/// matched in either case, as a mail server may write an address in capitals.
pub(crate) fn redact(text: &str, hidden: &[&str]) -> String {
    let mut text = text.to_string();
    for secret in hidden.iter().filter(|secret| !secret.is_empty()) {
        let needle = secret.to_ascii_lowercase();
        let mut out = String::with_capacity(text.len());
        let mut rest = text.as_str();
        // ASCII lowercase keeps every byte where it was, so a place found in the lowered text is
        // the same place, on a character's start, in the text itself.
        while let Some(at) = rest.to_ascii_lowercase().find(&needle) {
            out.push_str(rest.get(..at).unwrap_or_default());
            out.push_str("<hidden>");
            rest = rest.get(at + needle.len()..).unwrap_or_default();
        }
        out.push_str(rest);
        text = out;
    }
    text
}

#[cfg(test)]
#[path = "curl_tests.rs"]
mod tests;
