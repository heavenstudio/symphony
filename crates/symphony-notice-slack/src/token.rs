//! The bot token: where it is kept, read so that nothing prints it.
//!
//! Every notice sender keeps what it needs under the machine folder's `notices/`, named for its
//! way; Slack's is a token alone, `notices/slack.token`. The folder is its owner's alone (mode
//! 700) and so is the file (600): either readable by anyone else, or a link in the place of
//! either, is refused, the token unread. So the mode checked is the mode of what is read: a link
//! could lead to a file in a folder others write, swapped there for a link to another of the
//! owner's secrets.

use std::fmt;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// The folder of the machine folder that holds what each notice sender keeps.
pub const NOTICES: &str = "notices";
/// The file in [`NOTICES`] that holds the Slack app's bot token.
pub const TOKEN_FILE: &str = "slack.token";

/// The most characters a token has.
const LONGEST: usize = 4096;

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
/// are their owner's alone, neither of them a link.
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

/// Whether `path` is a `what` (`is` says), not a link, that its owner alone may read, which
/// `chmod mode` makes it.
fn owned(path: &Path, what: &str, is: fn(&fs::Metadata) -> bool, mode: &str) -> Result<(), String> {
    let shown = path.display();
    let meta = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "no token was read: {shown}: {error}; keep the Slack app's bot token in \
             {NOTICES}/{TOKEN_FILE} of the machine folder (docs/operations/NOTICE_SLACK.md)"
        )
    })?;
    if meta.file_type().is_symlink() {
        return Err(format!(
            "no token was read: {shown} is a link; keep the {what} itself there"
        ));
    }
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
