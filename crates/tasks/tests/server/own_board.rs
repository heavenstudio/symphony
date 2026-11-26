//! A board of its own for a test: a repository of its own, its board in its git dir in the live
//! board's shape ([`Repository`]), made on PostgreSQL by the first command that needs it and
//! stopped when the repository goes.

use crate::children::Child as _;
use std::path::{Path, PathBuf};
use symphony_board::cli::Env;
use symphony_board::sys::Command;

/// `kill -NAME PID`, for a process the test started.
pub fn signal(pid: i64, name: &str) {
    use symphony_board::sys::Spawning as _;
    let _ = std::process::Command::of("kill")
        .arg(format!("-{name}"))
        .arg(pid.to_string())
        .status_locked();
}

/// A repository of its own, its board the one in its git dir in the live board's shape: reached
/// from the repository with no `TASKS_*` variable naming it, made by the first command that needs
/// one, on PostgreSQL as a project's is. Its server, which outlives that command outside a scratch
/// place, is stopped when the repository goes ([`symphony_board::server::Home::stop_board`]).
pub struct Repository {
    pub dir: PathBuf,
}

impl Repository {
    /// A new repository, in a folder named for `name` in the test target's folder.
    pub fn new(name: &str) -> Repository {
        let dir =
            Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let done =
            symphony_board::sys::output(Command::of("git").args(["init", "-q"]).arg(&dir)).unwrap();
        assert!(done.status.success(), "git init: {done:?}");
        Repository {
            dir: dir.canonicalize().unwrap(),
        }
    }

    pub fn common(&self) -> PathBuf {
        self.dir.join(".git")
    }

    /// What a command in the repository reads: no variable naming a board.
    pub fn env(&self) -> Env {
        Env {
            vars: Default::default(),
            cwd: self.dir.clone(),
        }
    }
}

impl Drop for Repository {
    fn drop(&mut self) {
        let _ = symphony_board::server::Home::in_common(&self.common()).stop_board();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
