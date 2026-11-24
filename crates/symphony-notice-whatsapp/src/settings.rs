//! What the plugin keeps, in the machine folder's `notices/` (`docs/operations/NOTICE_WHATSAPP.md`):
//! its settings, `notices/whatsapp.toml`, and its token, `notices/whatsapp.token` unless the
//! settings name another file. The folder is its owner's alone (mode 700) and so is each file
//! (mode 600), each refused otherwise, or where it is a link, before anything in it is read. So
//! the mode checked is the mode of what is read: a link could lead to a file in a folder others
//! write, swapped there for a link to another of the owner's secrets. A refusal names the file and
//! what is wrong with it, with its line and a key of the settings' own, never a value the file
//! holds nor a key it does not know ([`unread`]).

use serde::Deserialize;
use std::fmt;
use std::fs;
use std::io;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use symphony_notice::Kind;

/// The folder of the machine folder that every notice plugin keeps its files in.
pub(crate) const FOLDER: &str = "notices";
/// The settings, in [`FOLDER`].
pub(crate) const SETTINGS: &str = "whatsapp.toml";
/// The token where the settings name no other file, in [`FOLDER`].
pub(crate) const TOKEN: &str = "whatsapp.token";
/// The Graph API it sends through where the settings name no other: Meta's newest version.
pub(crate) const API: &str = "https://graph.facebook.com/v27.0";

/// The folder `machine` keeps the notice plugins' files in, refused unless it is a folder its
/// owner's alone, and no link.
pub(crate) fn folder(machine: &Path) -> Result<PathBuf, String> {
    let folder = machine.join(FOLDER);
    let named = folder.display();
    let meta = fs::symlink_metadata(&folder).map_err(|error| {
        format!(
            "{named}: {error}; it holds this program's settings and token \
             (docs/operations/NOTICE_WHATSAPP.md)"
        )
    })?;
    if meta.file_type().is_symlink() {
        return Err(format!("{named} is a link; keep the folder itself there"));
    }
    if !meta.is_dir() {
        return Err(format!("{named} is not a folder"));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(format!(
            "{named} is open to others than its owner; chmod 700 it"
        ));
    }
    Ok(folder)
}

/// What the file at `path` holds, refused unless it is a plain file its owner's alone, and no
/// link. It is opened without following a link, and what is checked is the opened file's own, so
/// nothing swapped in between the check and the read is read.
fn private(path: &Path) -> Result<String, String> {
    let named = path.display();
    let file = (fs::OpenOptions::new().read(true))
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| match error.raw_os_error() {
            Some(libc::ELOOP) => format!("{named} is a link; keep the file itself there"),
            _ => format!("{named}: {error}"),
        })?;
    let meta = file
        .metadata()
        .map_err(|error| format!("{named}: {error}"))?;
    if !meta.is_file() {
        return Err(format!("{named} is not a file"));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(format!(
            "{named} is open to others than its owner; chmod 600 it"
        ));
    }
    // From the file opened and checked, never opened again by its path.
    io::read_to_string(file).map_err(|error| format!("{named}: {error}"))
}

/// The settings file as it is written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Written {
    phone_number_id: String,
    language: String,
    templates: Templates,
    token_file: Option<String>,
    api: Option<String>,
}

/// The template each kind of notice is sent by: `other` for a kind with none of its own, and for
/// a kind this build does not know.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Templates {
    other: String,
    card: Option<String>,
    question: Option<String>,
    verdict: Option<String>,
}

/// The settings, each checked when they are read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Settings {
    phone_number_id: String,
    language: String,
    templates: Templates,
    token_file: PathBuf,
    api: String,
}

impl Settings {
    /// The settings in `folder`, the notice plugins' folder.
    pub(crate) fn load(folder: &Path) -> Result<Settings, String> {
        let path = folder.join(SETTINGS);
        let text = private(&path)?;
        Settings::read(&text, folder).map_err(|why| format!("{}: {why}", path.display()))
    }

    /// The settings `text` holds, a token file it names none of found in `folder`.
    pub(crate) fn read(text: &str, folder: &Path) -> Result<Settings, String> {
        let file: Written = toml::from_str(text).map_err(|error| unread(text, &error))?;
        let id = &file.phone_number_id;
        if id.is_empty() || id.len() > 32 || !id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(
                "`phone_number_id` is no phone number's id: 1 to 32 digits, as WhatsApp Manager \
                 shows it"
                    .into(),
            );
        }
        if !language(&file.language) {
            return Err("`language` is no template language's code, such as `en_US`".into());
        }
        let templates = &file.templates;
        for (key, name) in [
            ("other", Some(&templates.other)),
            ("card", templates.card.as_ref()),
            ("question", templates.question.as_ref()),
            ("verdict", templates.verdict.as_ref()),
        ] {
            if name.is_some_and(|name| !template(name)) {
                return Err(format!(
                    "`templates.{key}` is no template's name: 1 to 512 lowercase letters, digits \
                     and underscores"
                ));
            }
        }
        let api = file.api.as_deref().unwrap_or(API).trim_end_matches('/');
        if !reachable(api) {
            return Err(
                "`api` is no https URL of the Graph API, such as `https://graph.facebook.com/v27.0`"
                    .into(),
            );
        }
        let token_file = match &file.token_file {
            None => folder.join(TOKEN),
            Some(path) if Path::new(path).is_absolute() => PathBuf::from(path),
            // Not repeated: an operator who pasted the token itself there would see it printed.
            Some(_) => return Err("`token_file` is no absolute path".into()),
        };
        Ok(Settings {
            phone_number_id: file.phone_number_id,
            language: file.language,
            templates: file.templates,
            token_file,
            api: api.to_string(),
        })
    }

    /// The id of the business's phone number it sends from.
    pub(crate) fn phone_number_id(&self) -> &str {
        &self.phone_number_id
    }

    /// The code of the language its templates are approved in.
    pub(crate) fn language(&self) -> &str {
        &self.language
    }

    /// The Graph API's URL, its version in it, with no `/` at its end.
    pub(crate) fn api(&self) -> &str {
        &self.api
    }

    /// The template a notice of `kind` is sent by: its own, else `other`'s.
    pub(crate) fn template(&self, kind: Kind) -> &str {
        let own = match kind {
            Kind::Card => self.templates.card.as_deref(),
            Kind::Question => self.templates.question.as_deref(),
            Kind::Verdict => self.templates.verdict.as_deref(),
            _ => None,
        };
        own.unwrap_or(&self.templates.other)
    }

    /// The token, read from its file, which must be a plain file its owner's alone holding one
    /// token.
    pub(crate) fn token(&self) -> Result<Secret, String> {
        let text = private(&self.token_file)?;
        Secret::new(&text).map_err(|why| format!("{} {why}", self.token_file.display()))
    }
}

/// Every key the settings file has, a template's among them.
const KEYS: [&str; 9] = [
    "phone_number_id",
    "language",
    "templates",
    "token_file",
    "api",
    "other",
    "card",
    "question",
    "verdict",
];

/// What a settings `text` that does not read is refused with: its line, and what is wrong there,
/// naming no key but one of [`KEYS`] and never a value.
///
/// toml's own rendering of an error quotes the line it is on, which may hold anything an operator
/// pasted there, so its message alone is said, and only where it holds nothing of the file's:
/// toml's syntax errors are fixed words, and a missing key is named by the program's own. serde's
/// messages for a value of another type than its key takes (`invalid type: integer
/// `106540352242922``) and for a key the program does not know (`unknown field `TOKEN``) quote
/// what the file holds, so they are said in the program's own words, naming the key only where it
/// is one of [`KEYS`].
fn unread(text: &str, error: &toml::de::Error) -> String {
    let line = (error.span())
        .and_then(|at| text.get(..at.start))
        .map(|before| before.matches('\n').count() + 1);
    let message = error.message();
    // serde's words, which quote the value; toml's own `invalid value` (a bad unicode escape) has
    // no colon and quotes nothing, so it is said as the syntax error it is.
    let quotes_a_value = ["invalid type: ", "invalid value: ", "unknown variant `"]
        .iter()
        .any(|serde_says| message.starts_with(serde_says));
    let said = if let Some(unknown) = message.strip_prefix("unknown field `") {
        let own = KEYS.iter().find(|key| {
            (unknown.strip_prefix(**key))
                .is_some_and(|after| after == "`" || after.starts_with("`,"))
        });
        match own {
            Some(key) => format!("`{key}` is not a key where it is written"),
            None => "a key this program does not know".into(),
        }
    } else if quotes_a_value {
        let key = (line.and_then(|line| text.lines().nth(line - 1)))
            .and_then(|written| written.split_once('='))
            .map(|(key, _)| key.trim())
            .filter(|key| key.split('.').all(|part| KEYS.contains(&part.trim())));
        match key {
            Some(key) => format!("`{key}` holds a value of another type than it takes"),
            None => "a value is of another type than its key takes".into(),
        }
    } else {
        message.to_string()
    };
    match line {
        Some(line) => format!("line {line}: {said}"),
        None => said,
    }
}

/// Whether `code` is a template language's code: two or three lowercase letters, then perhaps an
/// underscore and a region of two to four letters or digits (`en`, `en_US`, `pt_BR`).
fn language(code: &str) -> bool {
    let (base, region) = code
        .split_once('_')
        .map_or((code, None), |(base, region)| (base, Some(region)));
    (2..=3).contains(&base.len())
        && base.bytes().all(|b| b.is_ascii_lowercase())
        && region.is_none_or(|region| {
            (2..=4).contains(&region.len()) && region.bytes().all(|b| b.is_ascii_alphanumeric())
        })
}

/// Whether `name` is a template's name as Meta takes one: lowercase letters, digits and
/// underscores, at most 512.
fn template(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 512
        && (name.bytes()).all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Whether the token may be sent to `api`: an `https` URL, or a loopback `http` one, which a test's
/// stand-in for the Graph API answers at, with nothing a quoted `curl` value or a path would read
/// otherwise.
fn reachable(api: &str) -> bool {
    let plain = |rest: &str| {
        (rest.chars()).all(|c| c.is_ascii_graphic() && !matches!(c, '"' | '\\' | '?' | '#'))
    };
    if let Some(rest) = api.strip_prefix("https://") {
        let host = rest.split('/').next().unwrap_or_default();
        return !host.is_empty()
            && (host.chars()).all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
            && plain(rest);
    }
    api.strip_prefix("http://127.0.0.1:").is_some_and(|rest| {
        let port = rest.split('/').next().unwrap_or_default();
        !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) && plain(rest)
    })
}

/// A token. It has no `Display`, and its `Debug` shows none of it.
pub(crate) struct Secret(String);

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(redacted)")
    }
}

impl Secret {
    /// The token `text` holds, its surrounding whitespace dropped. A refusal is said of the file,
    /// and repeats none of it.
    pub(crate) fn new(text: &str) -> Result<Secret, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("is empty".into());
        }
        if text.len() > 4096
            || !(text.chars()).all(|c| c.is_ascii_graphic() && c != '"' && c != '\\')
        {
            return Err("holds more than one token of printable characters".into());
        }
        Ok(Secret(text.into()))
    }

    /// The token itself, for the one request that sends it.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }

    /// `text` with every occurrence of the token taken out.
    pub(crate) fn redact(&self, text: &str) -> String {
        text.replace(&self.0, "[hidden]")
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
