//! A scratch place, a test's or an agent's: under the temporary folder, or under a checkout's
//! `artifacts/`. A board kept in one ([`super::Home`]) is a fixture's, so its server ends with the
//! process that started it ([`Lifeline`]), where the board of a repository's own common dir
//! outlives every caller.

use std::io::{PipeReader, PipeWriter};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

/// Whether `path` lies in a scratch place: under the temporary folder (`TMPDIR`, as given and as
/// resolved, `/tmp` or `/private/tmp`), or under a folder named `artifacts` whose parent is a
/// checkout (it holds `.git`). Read as a path: the place itself need not exist.
pub fn scratch(path: &Path) -> bool {
    scratch_in(path, &temporary_folders())
}

/// [`scratch`], the temporary folders being `temporary`.
fn scratch_in(path: &Path, temporary: &[PathBuf]) -> bool {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return false;
    }
    if temporary.iter().any(|folder| path.starts_with(folder)) {
        return true;
    }
    path.ancestors()
        .skip(1)
        .filter(|folder| folder.file_name().is_some_and(|name| name == "artifacts"))
        .any(|artifacts| {
            artifacts
                .parent()
                .is_some_and(|checkout| checkout.join(".git").exists())
        })
}

/// The temporary folders a scratch place can be under.
fn temporary_folders() -> Vec<PathBuf> {
    let given = std::env::temp_dir();
    let mut folders = vec![PathBuf::from("/tmp"), PathBuf::from("/private/tmp")];
    if let Ok(resolved) = given.canonicalize() {
        folders.push(resolved);
    }
    if given.is_absolute() && given != Path::new("/") {
        folders.push(given);
    }
    folders
}

/// This process's lifeline: a pipe whose one writer this process holds until it ends and hands to
/// no child, so a reader of it reads its end exactly when this process ends, however it ends (a
/// kill, a crash, a test runner's timeout). A copy of the reader is what a supervised server's
/// watchdog reads ([`super::Home`]).
pub(super) struct Lifeline {
    reader: PipeReader,
    /// Never written to and never dropped: a static holds it for the process's life.
    _writer: PipeWriter,
}

impl Lifeline {
    /// This process's one lifeline, made at its first use.
    pub(super) fn of_this_process() -> std::io::Result<&'static Lifeline> {
        static LIFELINE: OnceLock<Result<Lifeline, String>> = OnceLock::new();
        LIFELINE
            .get_or_init(|| {
                crate::sys::pipe()
                    .map(|(reader, writer)| Lifeline {
                        reader,
                        _writer: writer,
                    })
                    .map_err(|problem| problem.to_string())
            })
            .as_ref()
            .map_err(|problem| std::io::Error::other(problem.clone()))
    }

    /// A reader of the lifeline, to hand a child as its input.
    pub(super) fn reader(&self) -> std::io::Result<PipeReader> {
        self.reader.try_clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary() -> Vec<PathBuf> {
        vec![
            PathBuf::from("/tmp"),
            PathBuf::from("/private/var/folders/9h/x/T"),
            PathBuf::from("/var/folders/9h/x/T/"),
        ]
    }

    #[test]
    fn a_path_under_a_temporary_folder_is_scratch_whether_it_is_there_or_gone() {
        for path in [
            "/private/var/folders/9h/x/T/review-test-50966-25/main/.git/symphony/tasks/data",
            "/var/folders/9h/x/T/gone",
            "/tmp/symphony-browser-projects-1-0/alpha",
        ] {
            assert!(scratch_in(Path::new(path), &temporary()), "{path}");
        }
    }

    #[test]
    fn a_repository_of_its_own_and_a_lookalike_of_a_temporary_folder_are_not_scratch() {
        for path in [
            "/Users/o/Documents/symphony/.git/symphony/tasks/data",
            "/private/var/folders/9h/x/Tother/data",
            "/tmpfs/data",
            "relative/tmp/data",
            "/tmp/../Users/o/.git/symphony/tasks/data",
            "/",
        ] {
            assert!(!scratch_in(Path::new(path), &temporary()), "{path}");
        }
    }

    #[test]
    fn a_folder_under_a_checkouts_artifacts_is_scratch_and_one_named_so_elsewhere_is_not() {
        let base = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("scratch-artifacts-{}", std::process::id()));
        let checkout = base.join("checkout");
        std::fs::create_dir_all(checkout.join(".git")).unwrap();
        std::fs::create_dir_all(base.join("plain/artifacts")).unwrap();
        // The temporary folder itself is left out, so only the artifacts rule can answer.
        let none: Vec<PathBuf> = Vec::new();
        assert!(scratch_in(
            &checkout.join("artifacts/after_lifecycle/run-1/main/.git/symphony/tasks/data"),
            &none
        ));
        assert!(!scratch_in(&checkout.join("artifacts"), &none));
        assert!(!scratch_in(&base.join("plain/artifacts/run/data"), &none));
        assert!(!scratch_in(&checkout.join("docs/data"), &none));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn this_machines_temporary_folder_is_one_as_given_and_as_resolved() {
        let given = std::env::temp_dir();
        assert!(scratch(&given.join("a-fixture")));
        assert!(scratch(&given.canonicalize().unwrap().join("a-fixture")));
        assert!(scratch(Path::new("/private/tmp/a-fixture")));
    }

    #[test]
    fn the_lifeline_is_one_per_process_and_its_reader_waits_while_this_process_runs() {
        use std::os::fd::AsRawFd as _;
        let lifeline = Lifeline::of_this_process().unwrap();
        assert!(std::ptr::eq(lifeline, Lifeline::of_this_process().unwrap()));
        let reader = lifeline.reader().unwrap();
        // Polled, never made non-blocking: every clone of the reader is one open pipe, which every
        // scratch server's supervisor this process started reads, and a watchdog that met a
        // non-blocking read would take it for the lifeline's end and end its server.
        let mut watched = libc::pollfd {
            fd: reader.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one pollfd, live for the call; a timeout of 0 returns at once.
        let ready = unsafe { libc::poll(&raw mut watched, 1, 0) };
        assert_eq!(
            (ready, watched.revents),
            (0, 0),
            "the writer is held: no end, and nothing written"
        );
    }
}
