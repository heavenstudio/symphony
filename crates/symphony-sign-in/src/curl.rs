//! The one call the service makes outside: a sign-in's code exchanged at the provider's token
//! endpoint, by running `curl`.
//!
//! `curl` reads the whole request from its standard input (`--config -`): the URL, the headers and
//! the form, the client secret, the code and the verifier among it. So none of them is in an
//! argument or an environment variable of any process, and `-q` keeps a `~/.curlrc` from adding to
//! the request (an `insecure` among it). `curl` verifies the endpoint's certificate, as it does
//! unless told not to, follows no redirect, uses no proxy, whatever proxy variable the environment
//! sets, and speaks only the endpoint's own scheme, `https` for Google's. What it says when it fails
//! is redacted of every value it was handed before anything else sees it.

use std::io::{ErrorKind, Write as _};
use std::process::{Command, Stdio};

/// The most seconds the call may take.
const SECONDS: &str = "30";

/// The endpoint's answer: its HTTP status and its body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Answer {
    pub(crate) status: u16,
    pub(crate) body: String,
}

/// The arguments `curl` runs with to post to `url`: the scheme `url` names alone, `https` for any
/// endpoint but a test's loopback stand-in.
pub(crate) fn args(url: &str) -> [&'static str; 13] {
    let scheme = if url.starts_with("https://") {
        "=https"
    } else {
        "=http"
    };
    [
        "-q",
        "--config",
        "-",
        "--silent",
        "--show-error",
        "--max-time",
        SECONDS,
        "--max-redirs",
        "0",
        "--proto",
        scheme,
        "--write-out",
        "\n%{http_code}",
    ]
}

/// What `curl` reads on its standard input to post `form`, form-encoded, to `url`.
fn config(url: &str, form: &str) -> String {
    // curl's quoted values read `\\` and `\"`; a form-encoded body and these headers hold no
    // newline, and the body starts with a name, never the `@` that would have curl read a file.
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
        line("header", "Content-Type: application/x-www-form-urlencoded"),
        line("header", "Accept: application/json"),
        line("header", "User-Agent: symphony-sign-in"),
        line("data-binary", form),
    ]
    .concat()
}

/// `text` with every value of `hidden` in it said as `<hidden>`.
pub(crate) fn redacted(text: &str, hidden: &[&str]) -> String {
    (hidden.iter())
        .filter(|value| !value.is_empty())
        .fold(text.to_string(), |text, value| {
            text.replace(value, "<hidden>")
        })
}

/// The answer to `form` posted to `url` by `program`, `curl` or a test's own, whatever its status.
/// `hidden` is every value of the form that is a secret, which nothing it says names.
///
/// # Errors
///
/// `program` is not there, did not start, failed, or said no status: said redacted.
pub(crate) fn post(
    program: &str,
    url: &str,
    form: &str,
    hidden: &[&str],
) -> Result<Answer, String> {
    let mut child = Command::new(program)
        .args(args(url))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            if error.kind() == ErrorKind::NotFound {
                format!("{program} is not on PATH: the service reaches the provider through curl, so install it")
            } else {
                format!("{program} did not start: {error}")
            }
        })?;
    // curl reads the whole of its config before it connects or writes anything, so writing it all
    // before reading curl's output never waits on curl; dropping the handle closes its standard
    // input. A curl that did not read it all exits failed, which its status says below.
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(config(url, form).as_bytes());
    }
    let done = child
        .wait_with_output()
        .map_err(|error| format!("{program} was not waited for: {error}"))?;
    let said = |bytes: &[u8]| redacted(String::from_utf8_lossy(bytes).trim(), hidden);
    if !done.status.success() {
        return Err(format!(
            "{program} reached no answer from the provider ({}): {}",
            done.status,
            said(&done.stderr)
        ));
    }
    let out = String::from_utf8_lossy(&done.stdout);
    let (body, status) = out.rsplit_once('\n').unwrap_or(("", &out));
    let status = status.trim().parse::<u16>().map_err(|error| {
        format!("{program} said no HTTP status of the provider's answer: {error}")
    })?;
    Ok(Answer {
        status,
        body: body.into(),
    })
}

#[cfg(test)]
#[path = "curl_tests.rs"]
mod tests;
