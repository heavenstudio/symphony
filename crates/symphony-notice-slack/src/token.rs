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
use std::io;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
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
    let path = notices.join(TOKEN_FILE);
    let folder_meta = fs::symlink_metadata(&notices).map_err(missing(&notices))?;
    if folder_meta.file_type().is_symlink() {
        return Err(linked(&notices, "folder"));
    }
    owned(
        &notices,
        &folder_meta,
        "folder",
        fs::Metadata::is_dir,
        "700",
    )?;
    // The file is opened without following a link, and what is checked is the opened file's own,
    // so nothing swapped in between the check and the read is read.
    let shown = path.display();
    let file = (fs::OpenOptions::new().read(true))
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&path)
        .map_err(|error| match (error.raw_os_error(), error.kind()) {
            (Some(libc::ELOOP), _) => linked(&path, "file"),
            (_, io::ErrorKind::NotFound) => missing(&path)(error),
            _ => format!("no token was read: {shown}: {}", error.kind()),
        })?;
    let meta = (file.metadata())
        .map_err(|error| format!("no token was read: {shown}: {}", error.kind()))?;
    owned(&path, &meta, "file", fs::Metadata::is_file, "600")?;
    // From the file opened and checked, never opened again by its path.
    let text = io::read_to_string(file)
        .map_err(|error| format!("no token was read: {shown}: {}", error.kind()))?;
    Secret::new(&text).map_err(|why| format!("no token was read from {shown}: {why}"))
}

/// What a `path` that could not be reached is said as, with where the token is kept.
fn missing(path: &Path) -> impl Fn(io::Error) -> String + '_ {
    move |error| {
        format!(
            "no token was read: {}: {error}; keep the Slack app's bot token in \
             {NOTICES}/{TOKEN_FILE} of the machine folder (docs/operations/NOTICE_SLACK.md)",
            path.display()
        )
    }
}

/// What a link at `path` in the place of a `what` is said as.
fn linked(path: &Path, what: &str) -> String {
    format!(
        "no token was read: {} is a link; keep the {what} itself there",
        path.display()
    )
}

/// Whether `meta`, of the `path` read, is a `what` (`is` says) that its owner alone may read,
/// which `chmod mode` makes it.
fn owned(
    path: &Path,
    meta: &fs::Metadata,
    what: &str,
    is: fn(&fs::Metadata) -> bool,
    mode: &str,
) -> Result<(), String> {
    let shown = path.display();
    if !is(meta) {
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
