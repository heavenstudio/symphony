//! The one request the plugin makes, by `curl`. `curl` reads the whole request on its standard
//! input (`--config -`): the URL, the headers with the token among them, and the body. So the token
//! is in no argument and no environment variable of any process, and `-q` keeps a `~/.curlrc` from
//! adding to the request. No redirect is followed, so the token goes to the API's host alone. What
//! `curl` says when it fails has the token taken out before anything else sees it.

use crate::settings::Secret;
use std::io::Write as _;
use std::process::{Command, Stdio};
use symphony_notice::Unsent;

/// The longest the request may take, connecting included, before it is a notice not sent this
/// time.
const SECONDS: &str = "30";

/// What the API answered: its HTTP status and its body.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Answer {
    pub(crate) status: u16,
    pub(crate) body: String,
}

/// The answer to the JSON `body` posted to `url` with `token`, made by `program`, whatever its
/// status.
///
/// No answer is [`Unsent::Later`], saying whether the request may have reached the API; a `curl`
/// that is not there is [`Unsent::Never`], as handing the notice again finds it absent again.
pub(crate) fn post(program: &str, url: &str, token: &Secret, body: &str) -> Result<Answer, Unsent> {
    let started = Command::new(program)
        .args([
            "-q",
            "--config",
            "-",
            "--silent",
            "--show-error",
            "--max-time",
            SECONDS,
            "--max-redirs",
            "0",
            // `{}` and `[]` in a URL are curl's globs, which would make one request several.
            "--globoff",
            "--proto",
            "=http,https",
            "--write-out",
            "\\n%{response_code}",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match started {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(Unsent::Never(format!(
                "{program} is not on PATH: this program reaches WhatsApp through curl, so install \
                 it; nothing was sent"
            )));
        }
        Err(error) => {
            return Err(Unsent::Later(format!(
                "{program} did not start: {error}; nothing was sent"
            )));
        }
    };
    let config = config(url, token, body);
    let wrote = (child.stdin.take()).map(|mut stdin| stdin.write_all(config.as_bytes()));
    let done = child
        .wait_with_output()
        .map_err(|error| Unsent::Later(format!("{program} was not waited for: {error}")))?;
    let said = |text: &[u8]| token.redact(String::from_utf8_lossy(text).trim());
    if let Some(Err(error)) = wrote {
        return Err(Unsent::Later(format!(
            "{program} did not read the request: {}; nothing was sent",
            said(error.to_string().as_bytes())
        )));
    }
    if !done.status.success() {
        // 5 to 7 are a proxy or a host curl could not resolve or connect to: the request was never
        // written. After any other failure it may have reached the API.
        let reached = match done.status.code() {
            Some(5..=7) => "nothing reached it",
            _ => "it may have taken the notice",
        };
        return Err(Unsent::Later(format!(
            "WhatsApp did not answer: curl failed ({}): {}; {reached}",
            done.status,
            said(&done.stderr)
        )));
    }
    let out = String::from_utf8_lossy(&done.stdout);
    let answer = (out.rsplit_once('\n'))
        .and_then(|(body, status)| Some((body, status.trim().parse::<u16>().ok()?)))
        .filter(|&(_, status)| status != 0);
    match answer {
        Some((body, status)) => Ok(Answer {
            status,
            body: body.to_string(),
        }),
        None => Err(Unsent::Later(
            "WhatsApp did not answer: curl printed no status; it may have taken the notice".into(),
        )),
    }
}

/// What `curl` reads on its standard input: the request to `url` with `token`, posting `body`.
fn config(url: &str, token: &Secret, body: &str) -> String {
    // A quoted value of curl's reads `\\` and `\"`; nothing else here needs an escape, as the URL
    // and the token are printable and a serialized JSON body holds no raw newline or tab.
    let line = |name: &str, value: &str| {
        format!(
            "{name} = \"{}\"\n",
            value.replace('\\', "\\\\").replace('"', "\\\"")
        )
    };
    [
        line("url", url),
        line("request", "POST"),
        line(
            "header",
            &format!("Authorization: Bearer {}", token.expose()),
        ),
        line("header", "Content-Type: application/json"),
        // No `Expect: 100-continue` on a longer body: the request goes at once, whole.
        line("header", "Expect:"),
        line("header", "User-Agent: symphony-notice-whatsapp"),
        line("data-binary", body),
    ]
    .concat()
}

#[cfg(test)]
#[path = "curl_tests.rs"]
mod tests;
