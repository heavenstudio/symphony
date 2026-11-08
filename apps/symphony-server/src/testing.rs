//! Existing review/store fixtures, with private repositories and databases for hub observations.

#[expect(
    dead_code,
    reason = "The hub uses the real store fixture's clock and database subset"
)]
#[path = "../../../crates/tasks/tests/store/common.rs"]
pub mod board;
#[path = "../../../crates/symphony/tests/common/tree.rs"]
mod tree;
pub use crate::shelf;
pub use tree::Tree;

pub fn git(root: &std::path::Path, args: &[&str]) {
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(args)
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_COMMON_DIR"),
    )
    .unwrap();
    assert!(
        done.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&done.stderr)
    );
}

/// A FIFO at `path`, held open for reading and writing, so neither a fixture child's open for
/// writing nor this one blocks.
pub fn fifo(path: &std::path::Path) -> std::fs::File {
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: name is a live, terminated path; the FIFO is owned by this fixture.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap()
}

/// Wait for a fixture record until the caller's deadline, retrying interrupted polls.
fn wait_entered(fd: std::os::fd::RawFd, deadline: std::time::Instant) {
    let mut poll = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "fixture child readiness deadline passed"
        );
        // Round up so a sub-millisecond remainder still waits for readiness.
        let timeout = i32::try_from(remaining.as_nanos().div_ceil(1_000_000)).unwrap();
        // SAFETY: poll points to one initialized pollfd; poll reports invalid descriptors.
        let ready = unsafe { libc::poll(&mut poll, 1, timeout) };
        let error = if ready < 0 {
            Some(std::io::Error::last_os_error())
        } else if poll.revents & libc::POLLNVAL != 0 {
            Some(std::io::Error::from_raw_os_error(libc::EBADF))
        } else {
            None
        };
        if let Some(error) = error {
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            panic!("fixture child readiness poll failed: {error}");
        }
        if ready > 0 {
            break;
        }
    }
}

/// The pid a fixture child writes to the FIFO `file` as it enters, waited for up to 30 s.
pub fn entered(file: &std::fs::File) -> i64 {
    use std::io::Read;
    use std::os::fd::AsRawFd;
    wait_entered(
        file.as_raw_fd(),
        std::time::Instant::now() + std::time::Duration::from_secs(30),
    );
    // The shell writes this short record atomically; one read also bounds malformed records
    // that omit their newline instead of hanging a line reader after the readiness deadline.
    let mut record = [0; 32];
    let mut reader = file;
    let count = reader.read(&mut record).unwrap();
    std::str::from_utf8(&record[..count])
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

/// Asserts that the stand-in `crate::fixture_script::write` installed at `program` is the
/// machine's one copy of its fixture script, which that write ran once, to its end, before handing
/// it out. macOS holds a new file's first exec while it assesses the file (25 to 31 s at a load of
/// 120, against milliseconds for the next exec), so that wait is setup, never part of the budget
/// of the case that starts the stand-in.
pub fn assert_primed(program: &std::path::Path) {
    use std::os::unix::fs::MetadataExt;
    let mut body = program.as_os_str().to_owned();
    body.push(".body");
    let body = std::fs::read_to_string(&body).unwrap();
    let copy = crate::fixture_script::machine_copy::machine_copy(
        &crate::fixture_script::tracked_wrapper(&body),
    );
    let (installed, shared) = (
        std::fs::metadata(program).unwrap(),
        std::fs::metadata(&copy).unwrap(),
    );
    assert_eq!(
        (installed.dev(), installed.ino()),
        (shared.dev(), shared.ino()),
        "{} is not the machine's already-run copy {} of its fixture script",
        program.display(),
        copy.display()
    );
}

pub fn repository(tree: &Tree) -> std::path::PathBuf {
    let root = tree.path("main");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-b", "main"]);
    git(&root, &["config", "user.name", "Review fixture"]);
    git(&root, &["config", "user.email", "review@example.invalid"]);
    git(&root, &["config", "core.hooksPath", "/dev/null"]);
    tree.write(
        "main/docs/TOPIC.md",
        "# Topic\n\n- [ ] **initial-card** — Initial question.\n",
    );
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "Initial document"]);
    root.canonicalize().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SIGNALS: AtomicUsize = AtomicUsize::new(0);

    extern "C" fn interrupted(_: libc::c_int) {
        SIGNALS.fetch_add(1, Ordering::Relaxed);
    }

    #[test]
    fn readiness_without_a_record_reports_its_deadline() {
        use std::os::fd::AsRawFd;
        let tree = Tree::new();
        let file = fifo(&tree.path("empty"));
        let began = std::time::Instant::now();
        let panic = std::panic::catch_unwind(|| {
            wait_entered(
                file.as_raw_fd(),
                began + std::time::Duration::from_millis(1),
            );
        })
        .unwrap_err();
        let elapsed = began.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(10),
            "fixture wait ignored the caller's deadline: {elapsed:?}"
        );
        let message = match panic.downcast::<String>() {
            Ok(message) => *message,
            Err(panic) => (*panic.downcast::<&str>().unwrap()).to_owned(),
        };
        assert_eq!(message, "fixture child readiness deadline passed");
    }

    /// A stand-in is primed only as the machine's copy `fixture_script::write` links; a file
    /// written in place with the same bytes and body is new to macOS, and is refused.
    #[test]
    fn only_the_machine_copy_of_a_fixture_script_passes_as_primed() {
        let tree = Tree::new();
        let body = "#!/bin/sh\nexit 0\n";
        let linked = tree.path("linked");
        crate::fixture_script::write(&linked, body, false);
        assert_primed(&linked);
        let fresh = tree.write("fresh", &std::fs::read_to_string(&linked).unwrap());
        tree.write("fresh.body", body);
        let refused = std::panic::catch_unwind(|| assert_primed(&fresh)).unwrap_err();
        let message = *refused.downcast::<String>().unwrap();
        assert!(
            message.contains("is not the machine's already-run copy"),
            "{message}"
        );
    }

    #[test]
    fn readiness_on_an_invalid_descriptor_reports_the_os_error() {
        let panic = std::panic::catch_unwind(|| {
            wait_entered(
                std::os::fd::RawFd::MAX,
                std::time::Instant::now() + std::time::Duration::from_secs(30),
            );
        })
        .unwrap_err();
        assert_eq!(
            *panic.downcast::<String>().unwrap(),
            format!(
                "fixture child readiness poll failed: {}",
                std::io::Error::from_raw_os_error(libc::EBADF)
            )
        );
    }

    #[test]
    fn entered_reads_pid_after_signals_interrupt_its_wait() {
        let tree = Tree::new();
        let mut file = fifo(&tree.path("entered"));
        // SIGURG has no other fixture owner. Restore the process-wide disposition even if
        // the readiness assertion unwinds; only this test's waiting thread receives it.
        struct Restore(libc::sigaction);
        impl Drop for Restore {
            fn drop(&mut self) {
                // SAFETY: the saved disposition came from sigaction below.
                assert_eq!(
                    unsafe { libc::sigaction(libc::SIGURG, &self.0, std::ptr::null_mut()) },
                    0
                );
            }
        }
        // SAFETY: zero initializes the action and its mask; sigaction fills the old action.
        let _restore = unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            let mut old = std::mem::zeroed();
            action.sa_sigaction = interrupted as *const () as usize;
            libc::sigemptyset(&mut action.sa_mask);
            assert_eq!(libc::sigaction(libc::SIGURG, &action, &mut old), 0);
            Restore(old)
        };
        std::thread::scope(|scope| {
            // SAFETY: this thread stays alive through the scoped sender's join, even on panic.
            let thread = unsafe { libc::pthread_self() };
            let reader = file.try_clone().unwrap();
            scope.spawn(move || {
                // Keep the FIFO empty across multiple signal deliveries so the waiting call
                // must survive interruptions before the atomic pid record becomes readable.
                for _ in 0..20 {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    // SAFETY: the scope keeps the targeted thread alive.
                    assert_eq!(unsafe { libc::pthread_kill(thread, libc::SIGURG) }, 0);
                }
                writeln!(file, "12345").unwrap();
            });
            assert_eq!(entered(&reader), 12345);
            assert!(
                SIGNALS.load(Ordering::Relaxed) > 0,
                "signal reached the waiting thread"
            );
        });
    }
}
