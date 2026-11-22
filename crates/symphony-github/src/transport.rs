//! How a request reaches GitHub: a [`Transport`], which the exchange is written against, and
//! [`Curl`], the one that makes the request by running `curl`.
//!
//! `curl` reads the whole request from its standard input (`--config -`): the URL, the method,
//! the headers with the token among them, and the body. So the token is in no argument, no URL
//! and no environment variable of any process, and `-q` keeps a `~/.curlrc` from adding to the
//! request. No redirect is followed, so the token goes to the one host named and nowhere else.
//! What `curl` says when it fails is redacted before anything else sees it.
//!
//! A download GitHub answers with a redirect (a job's log, an artifact's archive) is fetched by a
//! second `curl` that is given no token ([`Transport::fetch`]): its URL, which carries the
//! download's own signature, is read from standard input too, and is said nowhere past its `?`.

use crate::credential::Secret;
use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};

/// A request, relative to the API's base.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// `GET`, `POST`, `PATCH` or `PUT`.
    pub method: &'static str,
    /// The path and query, from the first `/`.
    pub path: String,
    /// The JSON it sends.
    pub body: Option<String>,
}

/// An answer: its status, its headers by lowercase name, and its body.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

/// How long a rate limit asks for, as its answer said it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    /// `retry-after`: this many seconds from now, never fewer than none.
    For(i64),
    /// `x-ratelimit-reset`: until this second since 1970.
    Until(i64),
    /// The answer named no time.
    Unsaid,
}

impl Wait {
    /// The second since 1970 it ends at, counting a `retry-after` from `now`; none where the
    /// answer named no time.
    #[must_use]
    pub fn until(self, now: i64) -> Option<i64> {
        match self {
            Wait::For(seconds) => Some(now.saturating_add(seconds)),
            Wait::Until(second) => Some(second),
            Wait::Unsaid => None,
        }
    }
}

impl Response {
    /// The header `name` as a whole number, where it reads as one.
    fn number(&self, name: &str) -> Option<i64> {
        self.headers.get(name)?.trim().parse().ok()
    }
}

/// The wait `response` asks for where it is GitHub's rate limit, the one rule every call is
/// judged by: a 429, or a 403 that carries `retry-after` or says no request remains
/// (`x-ratelimit-remaining: 0`), which is what GitHub documents of its primary and secondary
/// limits. No other status is one, whatever its headers: the last request a window allows also
/// says none remains. A secondary limit answered 403 with neither header is told only by its
/// message, whose words GitHub does not document, and is not read as one.
///
/// The wait is `retry-after` where it reads as seconds, else the window's reset, else unsaid.
#[must_use]
pub fn rate_limited(response: &Response) -> Option<Wait> {
    let limited = response.status == 429
        || (response.status == 403
            && (response.headers.contains_key("retry-after")
                || response.number("x-ratelimit-remaining") == Some(0)));
    limited.then(|| {
        (response
            .number("retry-after")
            .map(|seconds| Wait::For(seconds.max(0))))
        .or_else(|| response.number("x-ratelimit-reset").map(Wait::Until))
        .unwrap_or(Wait::Unsaid)
    })
}

/// A request that was given no answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// Nothing reached the host: the request was not made.
    NotSent(String),
    /// The host may have taken the request: its result is not known.
    Unknown(String),
}

/// What makes a request.
pub trait Transport {
    /// The host's answer to `request`, whatever its status.
    ///
    /// # Errors
    ///
    /// No answer came, and whether the request may have been taken.
    fn send(&self, request: &Request) -> Result<Response, Failure>;

    /// What `url` serves, written to the file `to`: its status. A download GitHub redirected a
    /// request to (a job's log, an artifact), so `url` is another host's and is asked without
    /// the token. A transport that downloads nothing says so.
    ///
    /// # Errors
    ///
    /// No answer came.
    fn fetch(&self, url: &str, to: &Path) -> Result<u16, Failure> {
        let _ = to;
        Err(Failure::NotSent(format!(
            "this transport downloads nothing: {}",
            url.split('?').next().unwrap_or_default()
        )))
    }
}

/// The API of GitHub.com, the only host the adapter's own program reaches.
pub const GITHUB: &str = "https://api.github.com";

/// Requests made by `curl`.
pub struct Curl {
    base: String,
    token: Secret,
    program: String,
}

impl Curl {
    /// Requests to `base` carrying `token`.
    ///
    /// # Errors
    ///
    /// `base` is neither an `https://` origin nor a loopback one, which a fixture answers at.
    pub fn new(base: &str, token: Secret) -> Result<Curl, String> {
        let loopback = base
            .strip_prefix("http://127.0.0.1:")
            .is_some_and(|port| !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()));
        let secure = base.strip_prefix("https://").is_some_and(|host| {
            !host.is_empty()
                && host
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        });
        if !loopback && !secure {
            return Err(format!("no request is made to {base}: not an https origin"));
        }
        Ok(Curl {
            base: base.into(),
            token,
            program: "curl".into(),
        })
    }

    /// The same, run as `program` rather than the `curl` on `PATH`.
    #[must_use]
    pub fn program(mut self, program: &str) -> Curl {
        self.program = program.into();
        self
    }

    /// What `curl` reads on its standard input for `request`.
    fn config(&self, request: &Request) -> Result<String, Failure> {
        let plain = request.path.starts_with('/')
            && request
                .path
                .chars()
                .all(|c| c.is_ascii_graphic() && c != '"' && c != '\\');
        if !plain {
            return Err(Failure::NotSent(format!(
                "no request is made to the path {:?}",
                request.path
            )));
        }
        // curl's quoted values read `\\` and `\"`; nothing else here needs an escape, as a
        // serialized JSON body and a header hold no raw newline or tab.
        let line = |name: &str, value: &str| {
            format!(
                "{name} = \"{}\"\n",
                value.replace('\\', "\\\\").replace('"', "\\\"")
            )
        };
        let mut lines = vec![
            line("url", &format!("{}{}", self.base, request.path)),
            line("request", request.method),
            line(
                "header",
                &format!("Authorization: Bearer {}", self.token.expose()),
            ),
            line("header", "Accept: application/vnd.github+json"),
            line("header", "X-GitHub-Api-Version: 2022-11-28"),
            line("header", "User-Agent: symphony-github"),
        ];
        if let Some(body) = &request.body {
            if body.starts_with('@') || body.chars().any(|c| c == '\n' || c == '\r') {
                return Err(Failure::NotSent(
                    "a request body is one line of JSON".into(),
                ));
            }
            lines.push(line("header", "Content-Type: application/json"));
            lines.push(line("data-binary", body));
        }
        let config = lines.concat();
        Ok(config)
    }

    /// Whether a download may be asked of `url`: an `https://` URL, or a loopback one where the
    /// API itself is a fixture's, with nothing a quoted `curl` value cannot hold.
    fn fetches(&self, url: &str) -> bool {
        let plain = (url.chars()).all(|c| c.is_ascii_graphic() && c != '"' && c != '\\');
        let loopback =
            self.base.starts_with("http://127.0.0.1:") && url.starts_with("http://127.0.0.1:");
        plain && (url.starts_with("https://") || loopback)
    }
}

/// The longest one download may take, and the most it may hold: a job's log or an artifact of a
/// step's logs.
const FETCH_SECONDS: &str = "600";
const FETCH_BYTES: &str = "1073741824";

/// The answer in what `curl --include` printed: the last block of headers, past any interim
/// `1xx` answer, then the body.
fn parse(output: &[u8]) -> Result<Response, String> {
    let mut rest = output;
    loop {
        let end = rest
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .ok_or_else(|| "the answer has no end of headers".to_string())?;
        let head = String::from_utf8_lossy(&rest[..end]).into_owned();
        rest = &rest[end + 4..];
        let mut lines = head.split("\r\n");
        let status = lines
            .next()
            .filter(|line| line.starts_with("HTTP/"))
            .and_then(|line| line.split(' ').nth(1))
            .and_then(|code| code.parse::<u16>().ok())
            .ok_or_else(|| "the answer has no status line".to_string())?;
        if (100..200).contains(&status) {
            continue;
        }
        let headers = lines
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
            .collect();
        return Ok(Response {
            status,
            headers,
            body: String::from_utf8_lossy(rest).into_owned(),
        });
    }
}

/// What a started `curl` printed once it was handed `config`, the whole request, on its standard
/// input and ended.
fn handed(mut child: Child, config: &str) -> Result<Output, Failure> {
    let wrote = child
        .stdin
        .take()
        .map(|mut stdin| stdin.write_all(config.as_bytes()));
    let done = child
        .wait_with_output()
        .map_err(|error| Failure::Unknown(format!("curl was not waited for: {error}")))?;
    if let Some(Err(error)) = wrote {
        return Err(Failure::NotSent(format!(
            "curl did not read the request: {error}"
        )));
    }
    Ok(done)
}

impl Transport for Curl {
    fn send(&self, request: &Request) -> Result<Response, Failure> {
        let config = self.config(request)?;
        let said = |text: &str| self.token.redact(text.trim());
        let child = Command::new(&self.program)
            .args([
                "-q",
                "--config",
                "-",
                "--silent",
                "--show-error",
                "--include",
                "--max-time",
                "60",
                "--max-redirs",
                "0",
                "--proto",
                "=http,https",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                Failure::NotSent(if error.kind() == std::io::ErrorKind::NotFound {
                    format!(
                        "{} is not on PATH: the adapter reaches GitHub through curl, so install \
                         it; nothing was sent",
                        self.program
                    )
                } else {
                    format!("{} did not start: {error}; nothing was sent", self.program)
                })
            })?;
        let done = handed(child, &config)?;
        if !done.status.success() {
            let why = format!(
                "curl failed ({}): {}",
                done.status,
                said(&String::from_utf8_lossy(&done.stderr))
            );
            // 2 is curl refusing to start, 3 a URL it cannot read, 5 to 7 a proxy or host it
            // could not resolve or connect to: in each the request was never written.
            return Err(match done.status.code() {
                Some(2 | 3 | 5 | 6 | 7) => Failure::NotSent(why),
                _ => Failure::Unknown(why),
            });
        }
        parse(&done.stdout).map_err(|why| Failure::Unknown(said(&why)))
    }

    fn fetch(&self, url: &str, to: &Path) -> Result<u16, Failure> {
        let named = url.split('?').next().unwrap_or_default().to_string();
        let file = to.to_string_lossy();
        if !self.fetches(url)
            || file
                .chars()
                .any(|c| c == '"' || c == '\\' || c.is_control())
        {
            return Err(Failure::NotSent(format!(
                "no download is asked of {named} into {file}"
            )));
        }
        let config = format!("url = \"{url}\"\noutput = \"{file}\"\n");
        let child = Command::new(&self.program)
            .args([
                "-q",
                "--config",
                "-",
                "--silent",
                "--show-error",
                "--max-time",
                FETCH_SECONDS,
                "--max-filesize",
                FETCH_BYTES,
                "--max-redirs",
                "0",
                "--proto",
                "=http,https",
                "--write-out",
                "%{http_code}",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                Failure::NotSent(format!("{} did not start: {error}", self.program))
            })?;
        let done = handed(child, &config)?;
        if !done.status.success() {
            // What curl says of a failure names the URL, whose query is the download's signature.
            let why = String::from_utf8_lossy(&done.stderr).replace(url, &named);
            return Err(Failure::Unknown(format!(
                "curl failed ({}) downloading {named}: {}",
                done.status,
                self.token.redact(why.trim())
            )));
        }
        String::from_utf8_lossy(&done.stdout)
            .trim()
            .parse::<u16>()
            .map_err(|error| {
                Failure::Unknown(format!("the download of {named} has no status: {error}"))
            })
    }
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
