//! Where the machine folder is: the test's [`DIR_ENV`] when it names one, else the platform's data
//! folder for the Symphony.

use std::ffi::OsString;
use std::path::PathBuf;

use super::DIR_ENV;

/// The machine folder: [`DIR_ENV`] when set, else the platform's data folder for the Symphony.
///
/// # Errors
///
/// When [`DIR_ENV`] names a relative path, or the platform's folder needs a `HOME` the process does
/// not have.
pub fn dir() -> Result<PathBuf, String> {
    folder(|name| std::env::var_os(name), cfg!(target_os = "macos"))
}

/// [`dir`] read from `var`, on macOS when `macos`.
fn folder(var: impl Fn(&str) -> Option<OsString>, macos: bool) -> Result<PathBuf, String> {
    let absolute = |name: &str| {
        var(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    if let Some(named) = var(DIR_ENV).filter(|value| !value.is_empty()) {
        let path = PathBuf::from(&named);
        return if path.is_absolute() {
            Ok(path)
        } else {
            Err(format!(
                "{DIR_ENV} names {}, not an absolute folder",
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

#[cfg(test)]
#[path = "folder_tests.rs"]
mod tests;
