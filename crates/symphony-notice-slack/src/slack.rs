//! Slack's side of a notice: the member id it goes to, the message posted there, and what Slack's
//! answer means for the exit status.
//!
//! `chat.postMessage` with a member id as its `channel` posts in the app's direct messages with
//! that member, which needs the bot token's `chat:write` scope alone. The text says what waits
//! and links the page; no link is unfurled, so Slack never fetches the page. Slack answers
//! `{"ok": true}` when it sent, and `{"ok": false, "error": WORD}` when it did not: a word of
//! [`LATER`] is worth handing again, any other is not.

use crate::curl::Answer;
use serde_json::{Value, json};
use symphony_notice::{Kind, Notice, Unsent};

/// The Web API method that sends a message.
pub(crate) const POST_MESSAGE: &str = "chat.postMessage";

/// The errors Slack answers when it did not send this time and may next time: it limits the
/// rate, it is down or slow, or the workspace is moving into an Enterprise organization.
const LATER: [&str; 6] = [
    "ratelimited",
    "service_unavailable",
    "internal_error",
    "fatal_error",
    "request_timeout",
    "team_added_to_org",
];

/// Whether `address` is a Slack member id: `U`, or `W` in an Enterprise organization, then
/// capital letters and digits. A channel's id (`C…`, `G…`, `D…`) is none, so a notice is never
/// posted to a whole channel.
pub(crate) fn member(address: &str) -> bool {
    let mut chars = address.chars();
    matches!(chars.next(), Some('U' | 'W'))
        && address.len() > 1
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// The message's text in Slack's markup: what waits, in which project, and the link to it.
pub(crate) fn text(notice: &Notice) -> String {
    let project = notice.project();
    let what = match notice.kind() {
        Kind::Card => format!("A card waits on you in *{project}*."),
        Kind::Question => format!("An agent's question waits on you in *{project}*."),
        Kind::Verdict => format!("A reviewer gave a verdict in *{project}*."),
        _ => format!("Something on the Symphony waits on you in *{project}*."),
    };
    format!("{what} <{}|Open it>", link(notice.link()))
}

/// `url` as Slack's `<url|label>` reads it: `&` written `&amp;` as Slack's markup asks, and the
/// `<`, `>` and `|` that would end the link percent-encoded, as a browser sends them.
fn link(url: &str) -> String {
    url.replace('&', "&amp;")
        .replace('<', "%3C")
        .replace('>', "%3E")
        .replace('|', "%7C")
}

/// The body of the `chat.postMessage` that sends `notice`: one line of JSON.
pub(crate) fn body(notice: &Notice) -> String {
    json!({
        "channel": notice.to().address(),
        "text": text(notice),
        "unfurl_links": false,
        "unfurl_media": false,
    })
    .to_string()
}

/// What `answer` says came of the post.
///
/// # Errors
///
/// [`Unsent::Later`] for HTTP 429 or 5xx, an error of [`LATER`], or a success whose body says
/// neither that it sent nor that it did not; [`Unsent::Never`] for any other status and any
/// other error, named by its word where it is one.
pub(crate) fn answered(answer: &Answer) -> Result<(), Unsent> {
    let status = answer.status;
    if status == 429 {
        return Err(Unsent::Later(
            "Slack limits the rate: HTTP 429, call again later".into(),
        ));
    }
    if status >= 500 {
        return Err(Unsent::Later(format!("Slack answered HTTP {status}")));
    }
    if !(200..300).contains(&status) {
        return Err(Unsent::Never(format!(
            "Slack refused the call: HTTP {status}"
        )));
    }
    let unknown = |why: &str| {
        Unsent::Later(format!(
            "Slack's answer (HTTP {status}) says neither that it sent nor that it did not: {why}"
        ))
    };
    let answer: Value = serde_json::from_str(&answer.body)
        .map_err(|error| unknown(&format!("no JSON, {error}")))?;
    match answer.get("ok") {
        Some(Value::Bool(true)) => Ok(()),
        Some(Value::Bool(false)) => Err(refused(answer.get("error").and_then(Value::as_str))),
        _ => Err(unknown("no `ok`")),
    }
}

/// Why Slack did not send, by its error `word`: one it names that is no plain word (lowercase
/// letters, digits and underscores) is not repeated.
fn refused(word: Option<&str>) -> Unsent {
    let plain = |word: &&str| {
        !word.is_empty()
            && word.len() <= 64
            && (word.chars()).all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    };
    match word.filter(plain) {
        Some(word) if LATER.contains(&word) => {
            Unsent::Later(format!("Slack did not send it this time: {word}"))
        }
        Some(word) => Unsent::Never(format!(
            "Slack refused it: {word} (docs/operations/NOTICE_SLACK.md says what each means)"
        )),
        None => Unsent::Never("Slack refused it, naming no error".into()),
    }
}

#[cfg(test)]
#[path = "slack_tests.rs"]
mod tests;
