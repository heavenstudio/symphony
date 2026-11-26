//! The one request the plugin makes, by `curl`. `curl` reads the whole request on its standard
//! input (`--config -`): the URL, the headers with the token among them, and the body. So the token
//! is in no argument and no environment variable of any process, and `-q` keeps a `~/.curlrc` from
//! adding to the request. No redirect is followed and no proxy is used, whatever proxy variable the
//! server's environment sets, so the token goes to the API's host alone. What `curl` says when it
//! fails has the token taken out before anything else sees it.

use crate::settings::Secret;
use std::io::Write as _;
use std::process::{Command, Stdio};
use symphony_notice::{Unsent, unstarted};

/// The longest the request may take, connecting included, before it is a notice not sent this
/// time.
const SECONDS: &str = "30";

/// curl's exits that say a fault a second hand meets again, so the notice is never sent: 1 a
/// protocol this curl lacks or `--proto` refuses, 2 curl failed to initialise, 3 a URL that does
/// not parse, 4 a feature this build lacks, 26 a config line this curl does not take, 48 an option
/// libcurl does not know, 60 a certificate that does not verify, 64 TLS required and not offered,
/// 77 CA certificates that cannot be read. Any other exit is worth handing again.
const LASTING: [i32; 9] = [1, 2, 3, 4, 26, 48, 60, 64, 77];

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
/// that is not there, or that fails by one of [`LASTING`], is [`Unsent::Never`], as handing the
/// notice again finds the same again, and one that did not start is as [`unstarted`] says.
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
            let why = format!("{program} did not start: {error}; nothing was sent");
            return Err(unstarted(&error, why));
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
        let (status, stderr) = (done.status, said(&done.stderr));
        // Each of `LASTING` stops curl before the request is written: a request it cannot make, or
        // a TLS session it will not open.
        if done
            .status
            .code()
            .is_some_and(|code| LASTING.contains(&code))
        {
            return Err(Unsent::Never(format!(
                "curl could not make the request ({status}), and will not on a second hand: \
                 {stderr}; nothing was sent"
            )));
        }
        // 6 and 7 are a host curl could not resolve or connect to (5, a proxy's, cannot come, as
        // `noproxy` uses none): the request was never written. After any other failure it may have
        // reached the API.
        let reached = match done.status.code() {
            Some(5..=7) => "nothing reached it",
            _ => "it may have taken the notice",
        };
        return Err(Unsent::Later(format!(
            "WhatsApp did not answer: curl failed ({status}): {stderr}; {reached}"
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
        line("noproxy", "*"),
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
