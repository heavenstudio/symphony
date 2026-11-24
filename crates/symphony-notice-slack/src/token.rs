//! The bot token: where it is kept, read so that nothing prints it.
//!
//! Every notice sender keeps what it needs under the machine folder's `notices/`, named for its
//! way; Slack's is a token alone, `notices/slack.token`. The folder is its owner's alone (mode
//! 700) and so is the file (600): either readable by anyone else is refused, the token unread.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// The folder of the machine folder that holds what each notice sender keeps.
pub const NOTICES: &str = "notices";
/// The file in [`NOTICES`] that holds the Slack app's bot token.
pub const TOKEN_FILE: &str = "slack.token";

/// The variable that names the machine folder, as the server reads it.
const MACHINE_DIR: &str = "SYMPHONY_MACHINE_DIR";

/// The most characters a token has.
const LONGEST: usize = 4096;

/// The machine folder, by the server's rule (`symphony_board::project::machine_folder`, which
/// this program does not link): `SYMPHONY_MACHINE_DIR` when `var` names it, else on macOS (when
/// `macos`) `~/Library/Application Support/Symphony`, elsewhere `$XDG_DATA_HOME/symphony` or
/// `~/.local/share/symphony`. An empty variable is unset.
///
/// # Errors
///
/// `SYMPHONY_MACHINE_DIR` names a relative path, or the platform's folder needs a `HOME` the
/// process does not have.
pub fn machine_folder(
    var: impl Fn(&str) -> Option<OsString>,
    macos: bool,
) -> Result<PathBuf, String> {
    let set = |name: &str| {
        var(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    if let Some(named) = set(MACHINE_DIR) {
        return if named.is_absolute() {
            Ok(named)
        } else {
            Err(format!(
                "{MACHINE_DIR} names {}, not an absolute folder",
                named.display()
            ))
        };
    }
    let absolute = |name: &str| set(name).filter(|path| path.is_absolute());
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

/// A token. It has no `Display`, and its `Debug` says only that it is one.
#[derive(Clone)]
pub(crate) struct Secret(String);

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(redacted)")
    }
}

impl Secret {
    /// A token of `text`, its surrounding whitespace dropped.
    ///
    /// # Errors
    ///
    /// It is empty, or holds a character a request header cannot carry safely.
    pub(crate) fn new(text: &str) -> Result<Secret, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("it is empty".into());
        }
        if text.len() > LONGEST
            || !(text.chars()).all(|c| c.is_ascii_graphic() && c != '"' && c != '\\')
        {
            return Err("it holds more than one token of printable characters".into());
        }
        Ok(Secret(text.into()))
    }

    /// The token itself, for the one place that sends it.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }

    /// `text` with every occurrence of the token replaced.
    pub(crate) fn redact(&self, text: &str) -> String {
        text.replace(&self.0, "[redacted]")
    }
}

/// The token kept in the machine `folder`: `notices/slack.token`, in a folder and a file that
/// are their owner's alone.
///
/// # Errors
///
/// One line naming the path and what is wrong with it, and what to do; never any of its content.
pub(crate) fn read(folder: &Path) -> Result<Secret, String> {
    let notices = folder.join(NOTICES);
    let file = notices.join(TOKEN_FILE);
    owned(&notices, "folder", fs::Metadata::is_dir, "700")?;
    owned(&file, "file", fs::Metadata::is_file, "600")?;
    let shown = file.display();
    let text = fs::read_to_string(&file)
        .map_err(|error| format!("no token was read: {shown}: {}", error.kind()))?;
    Secret::new(&text).map_err(|why| format!("no token was read from {shown}: {why}"))
}

/// Whether `path` is a `what` (`is` says) that its owner alone may read, which `chmod mode` makes
/// it.
fn owned(path: &Path, what: &str, is: fn(&fs::Metadata) -> bool, mode: &str) -> Result<(), String> {
    let shown = path.display();
    let meta = fs::metadata(path).map_err(|error| {
        format!(
            "no token was read: {shown}: {error}; keep the Slack app's bot token in \
             {NOTICES}/{TOKEN_FILE} of the machine folder (docs/operations/NOTICE_SLACK.md)"
        )
    })?;
    if !is(&meta) {
        return Err(format!("no token was read: {shown} is not a {what}"));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(format!(
            "no token was read: {shown} is open to others; chmod {mode} it"
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "token_tests.rs"]
mod tests;
