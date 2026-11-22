//! A child caught between its fork and its exec, for a test of a lock that must be free at once
//! though such a child still holds a copy of its file. The including crate names how it starts a
//! child: the command, and the spawn its own rules allow.

use std::io;
use std::process::{Child, Command};

/// A child another thread spawns, caught between its fork and its exec: it holds a copy of
/// every descriptor this process had open when it forked, as each child std forks does until
/// its exec, and with it the lock of each that is a locked file. It execs, and is waited for,
/// once this drops.
pub struct MidSpawn {
    go: io::PipeWriter,
    spawner: Option<std::thread::JoinHandle<()>>,
}

impl MidSpawn {
    /// Another thread's `spawn` of `command`, returned once its child has forked, before it
    /// execs.
    pub fn caught(mut command: Command, spawn: fn(&mut Command) -> io::Result<Child>) -> MidSpawn {
        use std::io::Read as _;
        use std::os::fd::AsRawFd as _;
        use std::os::unix::process::CommandExt as _;
        let (mut forked, forked_by) = io::pipe().unwrap();
        let (go_on, go) = io::pipe().unwrap();
        let fds = (forked_by.as_raw_fd(), go_on.as_raw_fd(), go.as_raw_fd());
        let spawner = std::thread::spawn(move || {
            let (forked_fd, go_on_fd, go_fd) = fds;
            // SAFETY: close, write and read are async-signal-safe and touch only the child's
            // copies of three pipe ends, which stay open in this process until the spawn
            // returns. Its copy of the writer closed, the child goes on at a byte or at the
            // reader's end, once this process lets go of the writer however it ends.
            unsafe {
                command.pre_exec(move || {
                    let mut byte = [0u8];
                    libc::close(go_fd);
                    retried(|| libc::write(forked_fd, byte.as_ptr().cast(), 1));
                    retried(|| libc::read(go_on_fd, byte.as_mut_ptr().cast(), 1));
                    Ok(())
                });
            }
            let child = spawn(&mut command);
            drop((forked_by, go_on));
            child.unwrap().wait().unwrap();
        });
        let mut byte = [0];
        forked
            .read_exact(&mut byte)
            .expect("the child forked and said so");
        MidSpawn {
            go,
            spawner: Some(spawner),
        }
    }
}

/// `call`, made again while it fails interrupted by a signal; it allocates nothing, so a forked
/// child may make it before its exec.
fn retried(mut call: impl FnMut() -> isize) {
    while call() < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {}
}

impl Drop for MidSpawn {
    fn drop(&mut self) {
        use std::io::Write as _;
        let _ = self.go.write_all(b"x");
        if let Some(spawner) = self.spawner.take() {
            let _ = spawner.join();
        }
    }
}
