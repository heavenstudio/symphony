//! A Web API call made by running `curl`.
//!
//! `curl` reads the whole request from its standard input (`--config -`): the URL, the method,
//! the headers with the token among them, and the body. So the token is in no argument, no URL
//! and no environment variable of any process, and `-q` keeps a `~/.curlrc` from adding to the
//! request. No redirect is followed and no proxy is used, whatever proxy variable the server's
//! environment sets, so the token goes to the one host named and nowhere else. What `curl` says
//! when it fails is redacted before anything else sees it.

use crate::token::Secret;
use std::io::{ErrorKind, Write as _};
use std::process::{Command, Stdio};
use symphony_notice::Unsent;

/// The most seconds one call may take.
const SECONDS: &str = "30";

/// curl's exits that say a fault a second hand meets again, so the notice is never sent: 1 a
/// protocol this curl lacks or `--proto` refuses, 2 curl failed to initialise, 3 a URL that does
/// not parse, 4 a feature this build lacks, 26 a config line this curl does not take, 48 an option
/// libcurl does not know, 60 a certificate that does not verify, 64 TLS required and not offered,
/// 77 CA certificates that cannot be read. Any other exit is worth handing again.
const LASTING: [i32; 9] = [1, 2, 3, 4, 26, 48, 60, 64, 77];

/// Slack's answer: its HTTP status and its body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Answer {
    pub(crate) status: u16,
    pub(crate) body: String,
}

/// Calls to the Web API at `api`, carrying `token`, run as `program`.
pub(crate) struct Curl {
    api: String,
    token: Secret,
    program: String,
}

impl Curl {
    /// Calls to the Web API at `api` (Slack's, or a test's loopback stand-in) carrying `token`.
    pub(crate) fn new(api: &str, token: Secret) -> Curl {
        Curl {
            api: api.trim_end_matches('/').into(),
            token,
            program: "curl".into(),
        }
    }

    /// The same, run as `program` rather than the `curl` on `PATH`.
    #[cfg(test)]
    pub(crate) fn program(mut self, program: &str) -> Curl {
        self.program = program.into();
        self
    }

    /// What `curl` reads on its standard input to post `body`, one line of JSON, to `method`.
    fn config(&self, method: &str, body: &str) -> String {
        // curl's quoted values read `\\` and `\"`; nothing else here needs an escape, as a
        // serialized JSON body and a header hold no raw newline. A body is a JSON object, so it
        // never starts with the `@` that would have curl read a file.
        let line = |name: &str, value: &str| {
            format!(
                "{name} = \"{}\"\n",
                value.replace('\\', "\\\\").replace('"', "\\\"")
            )
        };
        [
            line("url", &format!("{}/{method}", self.api)),
            line("noproxy", "*"),
            line("request", "POST"),
            line(
                "header",
                &format!("Authorization: Bearer {}", self.token.expose()),
            ),
            line("header", "Content-Type: application/json; charset=utf-8"),
            line("header", "User-Agent: symphony-notice-slack"),
            line("data-binary", body),
        ]
        .concat()
    }

    /// Slack's answer to `body` posted to `method`, whatever its status.
    ///
    /// # Errors
    ///
    /// [`Unsent::Never`] when `curl` cannot be started or fails by one of [`LASTING`];
    /// [`Unsent::Later`] when it ran and no answer came, the host unreached or the call timed out.
    pub(crate) fn post(&self, method: &str, body: &str) -> Result<Answer, Unsent> {
        let mut child = Command::new(&self.program)
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
                "--proto",
                "=http,https",
                "--write-out",
                "\n%{http_code}",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                Unsent::Never(if error.kind() == ErrorKind::NotFound {
                    format!(
                        "{} is not on PATH: this program reaches Slack through curl, so install it",
                        self.program
                    )
                } else {
                    format!("{} did not start: {error}", self.program)
                })
            })?;
        // curl reads the whole of its config before it connects or writes anything, so writing it
        // all before reading curl's output never waits on curl, however long it is; dropping the
        // handle closes its standard input. A curl that did not read it all exits failed, which
        // its status says below.
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(self.config(method, body).as_bytes());
        }
        let done = child
            .wait_with_output()
            .map_err(|error| Unsent::Later(format!("curl was not waited for: {error}")))?;
        let said = |bytes: &[u8]| self.token.redact(String::from_utf8_lossy(bytes).trim());
        if !done.status.success() {
            let (status, stderr) = (done.status, said(&done.stderr));
            return Err(match done.status.code() {
                Some(code) if LASTING.contains(&code) => Unsent::Never(format!(
                    "curl could not make the call to Slack ({status}), and will not on a second \
                     hand: {stderr}"
                )),
                _ => Unsent::Later(format!(
                    "curl reached no answer from Slack ({status}): {stderr}"
                )),
            });
        }
        let out = String::from_utf8_lossy(&done.stdout);
        let (body, status) = out.rsplit_once('\n').unwrap_or(("", &out));
        let status = status.trim().parse::<u16>().map_err(|error| {
            Unsent::Later(format!(
                "curl said no HTTP status of Slack's answer: {error}"
            ))
        })?;
        Ok(Answer {
            status,
            body: body.into(),
        })
    }
}

#[cfg(test)]
#[path = "curl_tests.rs"]
mod tests;
