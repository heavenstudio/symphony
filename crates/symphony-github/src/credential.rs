//! The token, read from the file the settings name and held so that nothing prints it: the
//! settings carry where it is, never what it is, and every line the adapter says of a failure
//! goes through [`Secret::redact`].

use std::fmt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// A token. It has no `Display`, and its `Debug` says only that it is one.
#[derive(Clone)]
pub struct Secret(String);

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
    pub fn new(text: &str) -> Result<Secret, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("the token file is empty".into());
        }
        if text.len() > 4096
            || !text
                .chars()
                .all(|c| c.is_ascii_graphic() && c != '"' && c != '\\')
        {
            return Err("the token file holds more than one token of printable characters".into());
        }
        Ok(Secret(text.into()))
    }

    /// The token itself, for the one place that sends it.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// `text` with every occurrence of the token replaced.
    #[must_use]
    pub fn redact(&self, text: &str) -> String {
        text.replace(&self.0, "[redacted]")
    }
}

/// The token in the file at `path`, which must be outside every folder of `repository` (its
/// checkouts and its git folder), a plain file, and readable by its owner alone.
///
/// # Errors
///
/// One line naming the path and what is wrong with it; never any of its content.
pub fn read(path: &Path, repository: &[&Path]) -> Result<Secret, String> {
    let named = path.display();
    if !path.is_absolute() {
        return Err(format!("token_file {named} is not an absolute path"));
    }
    let real = path
        .canonicalize()
        .map_err(|error| format!("token_file {named}: {error}"))?;
    for folder in repository {
        let folder = folder
            .canonicalize()
            .unwrap_or_else(|_| folder.to_path_buf());
        if real.starts_with(&folder) {
            return Err(format!(
                "token_file {named} is inside the repository; keep it outside"
            ));
        }
    }
    let meta = std::fs::metadata(&real).map_err(|error| format!("token_file {named}: {error}"))?;
    if !meta.is_file() {
        return Err(format!("token_file {named} is not a file"));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(format!(
            "token_file {named} is readable by others; chmod 600 it"
        ));
    }
    let text = std::fs::read_to_string(&real)
        .map_err(|error| format!("token_file {named}: {}", error.kind()))?;
    Secret::new(&text).map_err(|why| format!("token_file {named}: {why}"))
}

#[cfg(test)]
#[path = "credential_tests.rs"]
mod tests;
