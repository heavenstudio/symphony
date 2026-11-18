//! How a request reaches GitHub: a [`Transport`], which the exchange is written against, and
//! [`Curl`], the one that makes the request by running `curl`.
//!
//! `curl` reads the whole request from its standard input (`--config -`): the URL, the method,
//! the headers with the token among them, and the body. So the token is in no argument, no URL
//! and no environment variable of any process, and `-q` keeps a `~/.curlrc` from adding to the
//! request. No redirect is followed, so the token goes to the one host named and nowhere else.
//! What `curl` says when it fails is redacted before anything else sees it.

use crate::credential::Secret;
use std::collections::BTreeMap;
use std::io::Write as _;
use std::process::{Command, Stdio};

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
}

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

impl Transport for Curl {
    fn send(&self, request: &Request) -> Result<Response, Failure> {
        let config = self.config(request)?;
        let said = |text: &str| self.token.redact(text.trim());
        let mut child = Command::new(&self.program)
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
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
