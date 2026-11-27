//! The calls the service makes outside, each by running `curl`: a sign-in's code exchanged at the
//! provider's token endpoint ([`post`]), and, for GitHub, who the access token it answered is for
//! ([`get`]).
//!
//! `curl` reads the whole request from its standard input (`--config -`): the URL, the headers and
//! the form, the client secret, the code, the verifier and the access token among them. So none of
//! them is in an argument or an environment variable of any process, and `-q` keeps a `~/.curlrc`
//! from adding to the request (an `insecure` among it). `curl` verifies the endpoint's certificate,
//! as it does unless told not to, follows no redirect, uses no proxy, whatever proxy variable the
//! environment sets, and speaks only the endpoint's own scheme, `https` for every provider's. What
//! it says when it fails is redacted of every value it was handed, as it is and form-encoded, before
//! anything else sees it.

use serde::Deserialize;
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

impl Answer {
    /// `: CODE` where the body is a JSON object whose `error` is an OAuth error's code, 1 to 64
    /// lowercase letters and `_`, as `invalid_grant`; nothing otherwise, as the provider's body could
    /// say anything, a value it was sent among it.
    pub(crate) fn error(&self) -> String {
        #[derive(Deserialize)]
        struct Said {
            error: Option<String>,
        }
        (serde_json::from_str::<Said>(&self.body).ok())
            .and_then(|said| said.error)
            .filter(|error| {
                !error.is_empty()
                    && error.len() <= 64
                    && error.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            })
            .map_or_else(String::new, |error| format!(": {error}"))
    }
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
    request(
        url,
        "POST",
        &[
            "Content-Type: application/x-www-form-urlencoded",
            "Accept: application/json",
        ],
        Some(form),
    )
}

/// What `curl` reads on its standard input to ask `url` by `method` with `headers`, and `form` as
/// its body where there is one.
fn request(url: &str, method: &str, headers: &[&str], form: Option<&str>) -> String {
    // curl's quoted values read `\\` and `\"`; a form-encoded body and these headers hold no
    // newline (an access token is visible ASCII alone, `crate::github`), and the body starts with
    // a name, never the `@` that would have curl read a file.
    let line = |name: &str, value: &str| {
        format!(
            "{name} = \"{}\"\n",
            value.replace('\\', "\\\\").replace('"', "\\\"")
        )
    };
    let mut said = [
        line("url", url),
        line("noproxy", "*"),
        line("request", method),
    ]
    .concat();
    for header in headers.iter().chain(&["User-Agent: symphony-sign-in"]) {
        said.push_str(&line("header", header));
    }
    if let Some(form) = form {
        said.push_str(&line("data-binary", form));
    }
    said
}

/// `text` with every value of `hidden` in it said as `<hidden>`, as it is and form-encoded, as the
/// request's body carries it.
pub(crate) fn redacted(text: &str, hidden: &[&str]) -> String {
    let mut forms: Vec<String> = (hidden.iter())
        .filter(|value| !value.is_empty())
        .flat_map(|value| {
            [
                (*value).to_string(),
                form_urlencoded::byte_serialize(value.as_bytes()).collect(),
            ]
        })
        .collect();
    // The longest first, so no form is left half said where another holds part of it.
    forms.sort_by_key(|form| std::cmp::Reverse(form.len()));
    (forms.iter()).fold(text.to_string(), |text, form| {
        text.replace(form, "<hidden>")
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
    run(program, url, &config(url, form), hidden)
}

/// The answer to a GET of `url` with `headers` by `program`, `curl` or a test's own, whatever its
/// status. `hidden` is every value of the headers that is a secret, which nothing it says names.
///
/// # Errors
///
/// As [`post`]'s.
pub(crate) fn get(
    program: &str,
    url: &str,
    headers: &[&str],
    hidden: &[&str],
) -> Result<Answer, String> {
    run(program, url, &request(url, "GET", headers, None), hidden)
}

/// The answer to the request `config` says, asked of `url` by `program`.
fn run(program: &str, url: &str, config: &str, hidden: &[&str]) -> Result<Answer, String> {
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
        let _ = stdin.write_all(config.as_bytes());
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
