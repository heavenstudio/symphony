//! `symphony-notice-whatsapp`: the notice plugin that sends one person's notice by WhatsApp
//! (`docs/operations/NOTICE_WHATSAPP.md`), the server's and apart from the core.
//!
//! The server starts it with no arguments and writes the notice on its stdin
//! (docs/engineering/NOTICES.md); [`symphony_notice::take`] reads it and answers the exit status.
//! A notice goes as a template message through Meta's WhatsApp Cloud API: the template the
//! settings name for its kind, in their language, whose body takes one parameter, the notice's
//! link. It goes from the business's number the settings name to the person's number, which must
//! carry its `+` and country code. Its settings and its token are in the machine folder's
//! `notices/` ([`settings`]), and the request is made by `curl` ([`curl`]).
//!
//! It says why a notice was not sent on stderr, and never the token, the person's number or the
//! link: what Meta answers has each taken out before it is said.

#![forbid(unsafe_code)]

mod curl;
mod settings;

use serde_json::{Value, json};
use settings::Settings;
use std::path::Path;
use std::process::ExitCode;
use symphony_notice::{Notice, Unsent, WHATSAPP, ways};

fn main() -> ExitCode {
    symphony_notice::take(|notice| {
        let machine = symphony_notice::machine_folder(
            |name| std::env::var_os(name),
            cfg!(target_os = "macos"),
        )
        .map_err(Unsent::Never)?;
        send(notice, &machine, "curl")
    })
}

/// `notice` sent by WhatsApp with the settings and the token in `machine`'s `notices/`, the
/// request made by the `curl` program `curl`.
///
/// What the notice holds is checked before anything is read: a notice this program cannot send
/// is refused whatever the settings. A setting or a token that does not read is
/// [`Unsent::Never`], as handing the notice again finds it so again.
fn send(notice: &Notice, machine: &Path, curl: &str) -> Result<(), Unsent> {
    let number = number(notice)?;
    let folder = settings::folder(machine).map_err(Unsent::Never)?;
    let settings = Settings::load(&folder).map_err(Unsent::Never)?;
    let token = settings.token().map_err(Unsent::Never)?;
    let url = format!("{}/{}/messages", settings.api(), settings.phone_number_id());
    let body = message(notice, &settings, &number).to_string();
    let answer = curl::post(curl, &url, &token, &body)?;
    let hidden = [
        token.expose(),
        notice.to().address(),
        &number,
        number.trim_start_matches('+'),
        notice.link(),
    ];
    answered(answer.status, &answer.body, &hidden)
}

/// The number `notice` goes to, `+` and its digits. Refused unless it goes by WhatsApp to a number
/// its rule takes (`symphony_notice::ways::whatsapp`, which a profile's number is held to as well):
/// written with its `+` and country code, as without the `+` Meta puts the country code of the
/// number it is sent from before it, which may make another person's number.
fn number(notice: &Notice) -> Result<String, Unsent> {
    let to = notice.to();
    if to.way() != WHATSAPP {
        return Err(Unsent::Never(format!(
            "the notice goes by `{}`, and this program sends by `{WHATSAPP}`",
            to.way()
        )));
    }
    ways::whatsapp(to.address())
        .map_err(|why| Unsent::Never(format!("the address is no WhatsApp number: {why}")))
}

/// The message the Cloud API is sent for `notice` to `number`: the template of its kind, whose
/// body's one parameter is the link.
fn message(notice: &Notice, settings: &Settings, number: &str) -> Value {
    json!({
        "messaging_product": "whatsapp",
        "recipient_type": "individual",
        "to": number,
        "type": "template",
        "template": {
            "name": settings.template(notice.kind()),
            "language": {"code": settings.language()},
            "components": [{
                "type": "body",
                "parameters": [{"type": "text", "text": notice.link()}],
            }],
        },
    })
}

/// The codes of Meta's errors that say to try again later: a service down or overloaded (2,
/// 131016, 133004), a rate or a throughput reached (4, 80007, 130429), too many messages to the
/// same person at once (131056), an account in maintenance (131057) and an error Meta does not
/// know (131000). Every other error is one that sending the same request again meets again.
const LATER: [u64; 9] = [
    2, 4, 80007, 130_429, 131_000, 131_016, 131_056, 131_057, 133_004,
];

/// What the API's answer of `status` and `body` says of the notice: sent on a 2xx; else not sent,
/// this time ([`Unsent::Later`]) on a 429, a 5xx or a code of [`LATER`], and for good
/// ([`Unsent::Never`]) on any other. Why says the status, Meta's code and its words, with each of
/// `hidden` taken out.
fn answered(status: u16, body: &str, hidden: &[&str]) -> Result<(), Unsent> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    let answer: Value = serde_json::from_str(body).unwrap_or_default();
    let error = &answer["error"];
    let code = error["code"].as_u64();
    let words: Vec<&str> = [
        error["message"].as_str(),
        error["error_data"]["details"].as_str(),
    ]
    .into_iter()
    .flatten()
    .collect();
    let code_said = code
        .map(|code| format!(", code {code}"))
        .unwrap_or_default();
    let words_said = if words.is_empty() {
        String::new()
    } else {
        format!(": {}", clean(&words.join(" "), hidden))
    };
    let why = format!("WhatsApp refused the notice: HTTP {status}{code_said}{words_said}");
    if status == 429 || status >= 500 || code.is_some_and(|code| LATER.contains(&code)) {
        Err(Unsent::Later(why))
    } else {
        Err(Unsent::Never(why))
    }
}

/// The most characters of Meta's words a refusal says.
const WORDS_MOST: usize = 300;

/// `text` as one line said on stderr: each of `hidden` taken out, the longest first so none is
/// left in part, each control character a space, and no more than [`WORDS_MOST`] characters.
fn clean(text: &str, hidden: &[&str]) -> String {
    let mut hidden: Vec<&str> = hidden.iter().copied().filter(|h| !h.is_empty()).collect();
    hidden.sort_by_key(|h| std::cmp::Reverse(h.len()));
    let mut text = text.to_string();
    for h in hidden {
        text = text.replace(h, "[hidden]");
    }
    let mut line: String = (text.chars())
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(WORDS_MOST)
        .collect();
    if text.chars().count() > WORDS_MOST {
        line.push('…');
    }
    line
}

#[cfg(test)]
#[path = "../tests/plugin/scratch.rs"]
mod scratch;

#[cfg(test)]
mod tests;
