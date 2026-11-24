//! The notice message: what the server hands a notice plugin, one person's notice and that
//! person's address at a time, and the plugin's side of that hand-off
//! (docs/engineering/NOTICES.md, docs/product/analysis/users-and-identity.md, "What the Symphony
//! ships for notices").
//!
//! A notice carries a link alone: its id, what it tells of ([`Kind`]), the project, the page it is
//! about and where it goes ([`To`]). No title, question or thread's words go with it, and no name:
//! what a notice carries outside the Symphony is a later round's card.
//!
//! The message is one JSON object of format [`FORMAT`]. [`Notice::line`] writes it and
//! [`Notice::read`] reads it, refusing one that breaks a rule of [`Notice::new`] and one of a format
//! it does not read. A field it does not know is ignored, so what a newer server adds reaches an
//! older plugin unread, and a kind it does not know is read as [`Kind::Other`]. A plugin's `main`
//! is [`take`]: the one notice on stdin, handed to its send, and the exit status saying what came
//! of it.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::io::{self, Read, Write};
use std::process::ExitCode;

/// The format's major, written as the message's `notice`: a message of another is refused by name.
pub const FORMAT: u64 = 1;

/// The way a notice goes by mail: its address is an email address.
pub const EMAIL: &str = "email";
/// The way a notice goes by Slack: its address is the person's Slack user id.
pub const SLACK: &str = "slack";
/// The way a notice goes by WhatsApp: its address is the person's number with its country code
/// (`+15551234567`).
pub const WHATSAPP: &str = "whatsapp";

/// A plugin's exit status for a notice it sent.
pub const SENT: u8 = 0;
/// A plugin's exit status for a notice it did not send this time, worth handing again: sysexits'
/// `EX_TEMPFAIL`.
pub const LATER: u8 = 75;
/// A plugin's exit status for a notice it did not send, which handing again will not send.
pub const NEVER: u8 = 1;

/// The most characters an id has.
pub const ID_MOST: usize = 128;

/// What a notice tells of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Kind {
    /// A card waits on them.
    Card,
    /// An agent's question on a task waits on them.
    Question,
    /// A reviewer gave a verdict.
    Verdict,
    /// A kind this build does not know, from a newer server: sent as a notice that something on the
    /// Symphony waits on them, at its link. Never made: [`Notice::new`] refuses it.
    #[serde(other)]
    Other,
}

impl Kind {
    /// How the message writes it: `card`, `question`, `verdict`; `other` for [`Kind::Other`].
    pub fn word(self) -> &'static str {
        match self {
            Kind::Card => "card",
            Kind::Question => "question",
            Kind::Verdict => "verdict",
            Kind::Other => "other",
        }
    }
}

/// Where a notice goes: the way its person chose and the address their profile names for it.
#[derive(Clone, PartialEq, Eq)]
pub struct To {
    way: String,
    address: String,
}

impl To {
    /// The address `address` by the way `way`: [`EMAIL`], [`SLACK`], [`WHATSAPP`], or another
    /// way's word.
    ///
    /// # Errors
    ///
    /// When the way is no word (lowercase letters and digits, single dashes between them), or the
    /// address is empty, holds a control character or a space at either end. A refusal never
    /// repeats the address.
    pub fn new(way: &str, address: &str) -> Result<To, String> {
        if !word(way) {
            return Err(format!(
                "the way {way:?} is no word (lowercase letters, digits and single dashes)"
            ));
        }
        if address.trim().is_empty() {
            return Err("the address is empty".to_string());
        }
        if address.chars().any(char::is_control) {
            return Err("the address holds a control character".to_string());
        }
        if address.trim() != address {
            return Err("the address has a space at an end".to_string());
        }
        Ok(To {
            way: way.to_string(),
            address: address.to_string(),
        })
    }

    pub fn way(&self) -> &str {
        &self.way
    }

    pub fn address(&self) -> &str {
        &self.address
    }
}

/// The address is never printed, so a plugin that prints what it was handed logs none.
impl fmt::Debug for To {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("To")
            .field("way", &self.way)
            .field("address", &Hidden)
            .finish()
    }
}

/// One person's notice, by one way: every field is checked when it is made, so a notice is one a
/// plugin can send.
#[derive(Clone, PartialEq, Eq)]
pub struct Notice {
    id: String,
    kind: Kind,
    project: String,
    link: String,
    to: To,
}

impl Notice {
    /// The notice `id` of `kind` in `project`, about the page at `link`, going `to` one address.
    ///
    /// # Errors
    ///
    /// When the id is not 1 to [`ID_MOST`] ASCII letters, digits, dots, dashes and underscores; the
    /// kind is [`Kind::Other`]; the project is no project id (lowercase letters and digits, single
    /// dashes between them, as `[project] id`); or the link is no absolute `http` or `https` URL of
    /// printable ASCII. A refusal never repeats the link.
    pub fn new(id: &str, kind: Kind, project: &str, link: &str, to: To) -> Result<Notice, String> {
        if kind == Kind::Other {
            return Err(
                "`other` is the kind a reader gives one it does not know, never made".into(),
            );
        }
        Notice::checked(id, kind, project, link, to)
    }

    /// [`Notice::new`]'s checks but the kind's, which a reader takes as it reads it.
    fn checked(id: &str, kind: Kind, project: &str, link: &str, to: To) -> Result<Notice, String> {
        let id_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_');
        if id.is_empty() || id.len() > ID_MOST || !id.chars().all(id_char) {
            return Err(format!(
                "the id {id:?} is none: 1 to {ID_MOST} ASCII letters, digits, dots, dashes and \
                 underscores"
            ));
        }
        if !word(project) {
            return Err(format!(
                "the project {project:?} is no project id (lowercase letters, digits and single \
                 dashes)"
            ));
        }
        if !absolute(link) {
            return Err("the link is no absolute http or https URL of printable ASCII".into());
        }
        Ok(Notice {
            id: id.to_string(),
            kind,
            project: project.to_string(),
            link: link.to_string(),
            to,
        })
    }

    /// The id the notice has on every hand of it: the same notice to the same address by the same
    /// way, handed again after [`LATER`], keeps it, so a plugin can tell it sent it already.
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// The project's id, as in its `/<id>/` path.
    pub fn project(&self) -> &str {
        &self.project
    }

    /// The page it is about, an absolute URL.
    pub fn link(&self) -> &str {
        &self.link
    }

    pub fn to(&self) -> &To {
        &self.to
    }

    /// The message: one JSON object on one line, a newline after it.
    pub fn line(&self) -> String {
        let wire = Wire {
            notice: FORMAT,
            id: self.id.clone(),
            kind: self.kind,
            project: self.project.clone(),
            link: self.link.clone(),
            to: WireTo {
                way: self.to.way.clone(),
                address: self.to.address.clone(),
            },
        };
        serde_json::to_string(&wire).expect("a notice serializes") + "\n"
    }

    /// The notice `message` holds, the whitespace around it ignored.
    ///
    /// # Errors
    ///
    /// When it is no JSON object, names no format or another than [`FORMAT`], lacks a field or
    /// breaks a rule of [`Notice::new`] or [`To::new`].
    pub fn read(message: impl AsRef<[u8]>) -> Result<Notice, String> {
        let message: Value = serde_json::from_slice(message.as_ref())
            .map_err(|e| format!("the message is no JSON: {e}"))?;
        let Some(fields) = message.as_object() else {
            return Err("the message is no JSON object".into());
        };
        match fields.get("notice") {
            None => return Err("the message is no notice: it names no format (`notice`)".into()),
            Some(format) if format.as_u64() == Some(FORMAT) => {}
            Some(format) if format.is_u64() => {
                return Err(format!(
                    "the notice is of format {format}, and this program reads format {FORMAT}: \
                     take a build of it that reads format {format}"
                ));
            }
            Some(format) => {
                return Err(format!(
                    "the notice's format (`notice`) is {format}, where a whole number is"
                ));
            }
        }
        let wire: Wire = serde_json::from_value(message)
            .map_err(|e| format!("the notice of format {FORMAT} is refused: {e}"))?;
        let to = To::new(&wire.to.way, &wire.to.address)?;
        Notice::checked(&wire.id, wire.kind, &wire.project, &wire.link, to)
    }
}

/// The link and the address are never printed, so a plugin that prints what it was handed logs
/// neither.
impl fmt::Debug for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Notice")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("project", &self.project)
            .field("link", &Hidden)
            .field("to", &self.to)
            .finish()
    }
}

/// Why a plugin did not send a notice, and whether handing it again may.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unsent {
    /// Not sent this time, worth handing again ([`LATER`]): the service did not answer, or asked to
    /// be called later.
    Later(String),
    /// Not sent, and handing it again will not send it ([`NEVER`]): the service refused the address
    /// or the message.
    Never(String),
}

/// A plugin's `main`: reads the one notice on stdin, hands it to `send`, and answers the exit
/// status saying what came of it, [`SENT`], [`LATER`] or [`NEVER`], with why on stderr when it is
/// not sent. A message it cannot read is never sent.
pub fn take(send: impl FnOnce(&Notice) -> Result<(), Unsent>) -> ExitCode {
    ExitCode::from(take_from(io::stdin().lock(), io::stderr().lock(), send))
}

/// [`take`] from `input`, saying why it did not send on `errors`.
fn take_from(
    mut input: impl Read,
    mut errors: impl Write,
    send: impl FnOnce(&Notice) -> Result<(), Unsent>,
) -> u8 {
    let mut message = Vec::new();
    let (status, why) = match input.read_to_end(&mut message) {
        Err(e) => (
            LATER,
            format!("the notice could not be read from stdin: {e}"),
        ),
        Ok(_) => match Notice::read(&message) {
            Err(why) => (NEVER, why),
            Ok(notice) => match send(&notice) {
                Ok(()) => return SENT,
                Err(Unsent::Later(why)) => (LATER, why),
                Err(Unsent::Never(why)) => (NEVER, why),
            },
        },
    };
    // Nothing is left to say it on when stderr is gone: the status still says it.
    let _ = writeln!(errors, "{why}");
    status
}

/// What a printed notice shows in the place of its link and its address.
struct Hidden;

impl fmt::Debug for Hidden {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<hidden>")
    }
}

/// Whether `text` is a word: lowercase ASCII letters and digits, single dashes between them, the
/// rule of `[project] id` and of a way.
fn word(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with('-')
        && !text.ends_with('-')
        && !text.contains("--")
        && (text.chars()).all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Whether `link` is an absolute `http` or `https` URL with a host, all of it printable ASCII.
fn absolute(link: &str) -> bool {
    let rest = (link.strip_prefix("https://")).or_else(|| link.strip_prefix("http://"));
    rest.is_some_and(|rest| {
        rest.chars()
            .next()
            .is_some_and(|c| !matches!(c, '/' | '?' | '#'))
            && link.chars().all(|c| c.is_ascii_graphic())
    })
}

/// The message as it is written.
#[derive(Serialize, Deserialize)]
struct Wire {
    notice: u64,
    id: String,
    kind: Kind,
    project: String,
    link: String,
    to: WireTo,
}

#[derive(Serialize, Deserialize)]
struct WireTo {
    way: String,
    address: String,
}

#[cfg(test)]
mod tests;
