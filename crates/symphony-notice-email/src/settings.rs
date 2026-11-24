//! What the plugin sends by: `notices/email.toml` in the machine folder, and the password in
//! `notices/email.token` beside it where the settings name a `user`
//! (`docs/operations/NOTICE_EMAIL.md`, "Its settings").
//!
//! The folder is the server's own, found by the server's rule ([`machine_folder`]), never a
//! project's: nothing of the plugin is in a `symphony.toml`. `notices/` is a folder its owner alone
//! reaches, and each file in it a plain file its owner alone reads, as they hold what sends mail in
//! the server's name; anything else is refused before a file is read. A refusal names the file and
//! its rule, never what it holds, and the password is printed nowhere ([`Secret`]).

use serde::Deserialize;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::mail;

/// The variable that names the machine folder, as the server reads it.
pub(crate) const MACHINE_DIR: &str = "SYMPHONY_MACHINE_DIR";

/// The machine folder, by the server's rule (`symphony_board::project::machine_folder`, repeated
/// here so the plugin links no board crate): [`MACHINE_DIR`] when set, else the platform's data
/// folder for the Symphony, read from `var`, on macOS when `macos`.
pub(crate) fn machine_folder(
    var: impl Fn(&str) -> Option<OsString>,
    macos: bool,
) -> Result<PathBuf, String> {
    let absolute = |name: &str| {
        var(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    if let Some(named) = var(MACHINE_DIR).filter(|value| !value.is_empty()) {
        let path = PathBuf::from(&named);
        return if path.is_absolute() {
            Ok(path)
        } else {
            Err(format!(
                "{MACHINE_DIR} names {}, not an absolute folder",
                path.display()
            ))
        };
    }
    let home = absolute("HOME");
    let found = if macos {
        home.map(|home| home.join("Library/Application Support/Symphony"))
    } else {
        absolute("XDG_DATA_HOME")
            .map(|data| data.join("symphony"))
            .or_else(|| home.map(|home| home.join(".local/share/symphony")))
    };
    found.ok_or_else(|| "the machine folder needs HOME, an absolute folder".into())
}

/// How the connection to the mail server is kept private.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tls {
    /// `smtps://`: TLS from the first byte.
    Implicit,
    /// `smtp://` to another machine: STARTTLS, or nothing is sent.
    Required,
    /// `smtp://` to this machine's loopback: a relay on the machine itself, as written.
    Plain,
}

/// The SMTP password. It has no `Display`, and its `Debug` says only that it is one.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Secret(String);

impl Secret {
    /// The password itself, for the one place that sends it.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(hidden)")
    }
}

/// The settings, every value checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Settings {
    server: String,
    tls: Tls,
    from: String,
    login: Option<(String, Secret)>,
}

impl Settings {
    /// The mail server's URL, `smtps://HOST[:PORT]` or `smtp://HOST[:PORT]`.
    pub(crate) fn server(&self) -> &str {
        &self.server
    }

    pub(crate) fn tls(&self) -> Tls {
        self.tls
    }

    /// The address the mail is sent from.
    pub(crate) fn from(&self) -> &str {
        &self.from
    }

    /// The user and password the mail server is logged in with; none for a relay that takes
    /// mail without a login.
    pub(crate) fn login(&self) -> Option<(&str, &Secret)> {
        self.login
            .as_ref()
            .map(|(user, password)| (user.as_str(), password))
    }

    /// The same, needing STARTTLS whatever its host: a test's mail server is on the loopback.
    #[cfg(test)]
    pub(crate) fn requiring_starttls(mut self) -> Settings {
        self.tls = Tls::Required;
        self
    }
}

/// `notices/email.toml` as it is written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    server: String,
    from: String,
    user: Option<String>,
}

/// The folder in the machine folder that every notice plugin keeps its settings in.
pub(crate) const FOLDER: &str = "notices";
/// This plugin's settings, in [`FOLDER`].
pub(crate) const SETTINGS: &str = "email.toml";
/// The password, in [`FOLDER`].
pub(crate) const TOKEN: &str = "email.token";

/// The most bytes a password has.
const PASSWORD_MOST: usize = 4096;

/// The settings the machine folder `folder` holds.
///
/// # Errors
///
/// One line naming the file and what is wrong with it: `notices/` missing, a link, or reached by
/// others; a file missing, not a plain file or read by others; settings that do not read or break
/// a rule; a `user` with no password beside it, or a password with no `user`. Never what a file
/// holds.
pub(crate) fn read(folder: &Path) -> Result<Settings, String> {
    let notices = folder.join(FOLDER);
    let settings = notices.join(SETTINGS);
    match fs::symlink_metadata(&notices) {
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Err(format!(
                "the email plugin is not set up: {} does not exist \
                 (docs/operations/NOTICE_EMAIL.md)",
                settings.display()
            ));
        }
        Err(error) => return Err(format!("{}: {error}", notices.display())),
        Ok(meta) if !meta.is_dir() => {
            return Err(format!("{} is not a folder", notices.display()));
        }
        Ok(meta) if meta.permissions().mode() & 0o077 != 0 => {
            return Err(format!(
                "{} is open to others: chmod 700 it",
                notices.display()
            ));
        }
        Ok(_) => {}
    }
    let Some(text) = private(&settings)? else {
        return Err(format!(
            "the email plugin is not set up: {} does not exist \
             (docs/operations/NOTICE_EMAIL.md)",
            settings.display()
        ));
    };
    let file: File = toml::from_str(&text).map_err(|error| {
        // toml's own rendering quotes the line, which may hold what was written by mistake;
        // its message and line number alone name the fault.
        let line = (error.span())
            .and_then(|span| text.get(..span.start))
            .map_or(1, |before| before.matches('\n').count() + 1);
        format!("{} line {line}: {}", settings.display(), error.message())
    })?;
    let (server, tls) =
        server(&file.server).map_err(|why| format!("{}: {why}", settings.display()))?;
    if !mail::address(&file.from) {
        return Err(format!(
            "{}: `from` is no plain email address: {}",
            settings.display(),
            mail::ADDRESS_RULE
        ));
    }
    let token = notices.join(TOKEN);
    let password = private(&token)?;
    let login = match (file.user, password) {
        (None, None) => None,
        (Some(_), None) => {
            return Err(format!(
                "{} names a `user`, and {} does not exist: write the password there",
                settings.display(),
                token.display()
            ));
        }
        (None, Some(_)) => {
            return Err(format!(
                "{} holds a password, and {} names no `user` to log in with",
                token.display(),
                settings.display()
            ));
        }
        (Some(user), Some(password)) => {
            if user.is_empty() || user.contains(':') || user.chars().any(char::is_control) {
                return Err(format!(
                    "{}: `user` is empty, holds a `:` or a control character",
                    settings.display()
                ));
            }
            let password = password.trim();
            if password.is_empty()
                || password.len() > PASSWORD_MOST
                || password.chars().any(char::is_control)
            {
                return Err(format!(
                    "{}: it holds no password of one line (at most {PASSWORD_MOST} bytes, no \
                     control character)",
                    token.display()
                ));
            }
            Some((user, Secret(password.to_string())))
        }
    };
    Ok(Settings {
        server,
        tls,
        from: file.from,
        login,
    })
}

/// The text of the file at `path`, where it is a plain file its owner alone reads; none where it
/// does not exist.
fn private(path: &Path) -> Result<Option<String>, String> {
    let named = path.display();
    let meta = match fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{named}: {error}")),
        Ok(meta) => meta,
    };
    if !meta.is_file() {
        return Err(format!("{named} is not a plain file"));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(format!("{named} is readable by others: chmod 600 it"));
    }
    fs::read_to_string(path)
        .map(Some)
        .map_err(|error| format!("{named}: {}", error.kind()))
}

/// The mail server `url` names, and how its connection is kept private: `smtps://HOST[:PORT]`,
/// or `smtp://HOST[:PORT]`, which needs STARTTLS but to this machine's loopback.
fn server(url: &str) -> Result<(String, Tls), String> {
    const RULE: &str = "`server` is no `smtps://HOST[:PORT]` or `smtp://HOST[:PORT]`";
    let (rest, implicit) = match (url.strip_prefix("smtps://"), url.strip_prefix("smtp://")) {
        (Some(rest), _) => (rest, true),
        (None, Some(rest)) => (rest, false),
        (None, None) => return Err(RULE.into()),
    };
    let (host, port) = if let Some(bracketed) = rest.strip_prefix('[') {
        let (inside, after) = bracketed.split_once(']').ok_or(RULE)?;
        let ipv6 = !inside.is_empty()
            && inside
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.');
        if !ipv6 {
            return Err(RULE.into());
        }
        (rest.get(..inside.len() + 2).ok_or(RULE)?, after)
    } else {
        let end = rest.find(':').unwrap_or(rest.len());
        let (host, after) = rest.split_at(end);
        let name = !host.is_empty()
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
        if !name {
            return Err(RULE.into());
        }
        (host, after)
    };
    if let Some(port) = port.strip_prefix(':') {
        let number = port
            .parse::<u16>()
            .ok()
            .filter(|number| *number > 0 && port.chars().all(|c| c.is_ascii_digit()));
        if number.is_none() {
            return Err(format!("{RULE}: its port is no number from 1 to 65535"));
        }
    } else if !port.is_empty() {
        return Err(format!("{RULE}: it has a path, a query or a login in it"));
    }
    let loopback = matches!(
        host.to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "[::1]"
    );
    let tls = match (implicit, loopback) {
        (true, _) => Tls::Implicit,
        (false, false) => Tls::Required,
        (false, true) => Tls::Plain,
    };
    Ok((url.to_string(), tls))
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
