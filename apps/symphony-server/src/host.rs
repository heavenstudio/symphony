//! The host rule as the server follows it (`symphony::machine::host`: which build is the machine's
//! one server), and the hand-off a starter whose build must replace the running server asks for.

use std::path::{Path, PathBuf};

use symphony::machine::Registry;
pub use symphony::machine::host::{Build, Lack, Verdict, judge, worktree_of};

/// The hand-off a starter asks of the running server, in the machine folder: replace yourself with
/// `exe`, serving from `root` (the starter's main checkout) with its build and client in `install`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Handover {
    pub exe: PathBuf,
    pub root: PathBuf,
    pub install: PathBuf,
}

/// The request's file in the machine folder.
pub const HANDOVER: &str = "handover.json";

impl Handover {
    /// Leave the request in `machine` for the running server, whole.
    ///
    /// # Errors
    ///
    /// When it cannot be written.
    pub fn ask(&self, machine: &Path) -> Result<(), String> {
        let path = machine.join(HANDOVER);
        let next = machine.join(format!("{HANDOVER}.{}", std::process::id()));
        let text = serde_json::to_string(self).expect("a hand-off serializes");
        let written = std::fs::write(&next, text).and_then(|()| std::fs::rename(&next, &path));
        if written.is_err() {
            let _ = std::fs::remove_file(&next);
        }
        written.map_err(|e| symphony::text::os_error(&e, &path))
    }

    /// The request left in `machine`, taken (its file removed), when it is one this server may
    /// follow: an executable file, an install folder and a registered project's main checkout,
    /// every one absolute. A request that is not is removed and said.
    pub fn take(machine: &Path) -> Option<Result<Handover, String>> {
        let path = machine.join(HANDOVER);
        let text = std::fs::read_to_string(&path).ok()?;
        let _ = std::fs::remove_file(&path);
        let request: Handover = match serde_json::from_slice(text.as_bytes()) {
            Ok(request) => request,
            Err(e) => return Some(Err(format!("{}: {e}", path.display()))),
        };
        let registered = Registry::read(machine).is_ok_and(|registry| {
            registry
                .projects
                .iter()
                .any(|entry| entry.root == request.root)
        });
        let problem = if ![&request.exe, &request.root, &request.install]
            .iter()
            .all(|path| path.is_absolute())
        {
            Some("names a relative path")
        } else if !request.exe.is_file() {
            Some("names no executable file")
        } else if !request.install.is_dir() {
            Some("names no install folder")
        } else if !registered {
            Some("names a project the registry does not hold")
        } else {
            None
        };
        Some(match problem {
            Some(problem) => Err(format!("the hand-off request {problem}: {text}")),
            None => Ok(request),
        })
    }
}

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;
