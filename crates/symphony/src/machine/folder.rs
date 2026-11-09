//! Where the machine folder is: the test's [`DIR_ENV`](super::DIR_ENV) when it names one, else
//! the platform's data folder for the Symphony, by the rule the board's command line reads the
//! registry by ([`symphony_board::project::machine_folder`]).

use std::ffi::OsString;
use std::path::PathBuf;

#[cfg(test)]
use super::DIR_ENV;

/// The machine folder: [`DIR_ENV`](super::DIR_ENV) when set, else the platform's data folder for
/// the Symphony.
///
/// # Errors
///
/// When [`DIR_ENV`](super::DIR_ENV) names a relative path, or the platform's folder needs a `HOME`
/// the process does not have.
pub fn dir() -> Result<PathBuf, String> {
    folder(|name| std::env::var_os(name), cfg!(target_os = "macos"))
}

/// [`dir`] read from `var`, on macOS when `macos`.
fn folder(var: impl Fn(&str) -> Option<OsString>, macos: bool) -> Result<PathBuf, String> {
    symphony_board::project::machine_folder(var, macos)
}

#[cfg(test)]
#[path = "folder_tests.rs"]
mod tests;
