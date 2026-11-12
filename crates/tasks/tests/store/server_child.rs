//! A fixture shell owns its server and EOF watchdog. Closing its input and waiting for the shell
//! waits for both children, including during partial setup and panic unwinding.

use std::io;
use std::io::Write as _;
use std::process::ExitStatus;
use symphony_board::sys::Child;

#[derive(Debug)]
pub struct ServerChild {
    pub(super) shell: Child,
    /// The server's own process, as the shell said it: none until it has.
    pub server: Option<u32>,
}

impl ServerChild {
    pub fn shutdown(&mut self) -> io::Result<ExitStatus> {
        drop(self.shell.stdin.take());
        self.shell.wait()
    }
}

impl Drop for ServerChild {
    fn drop(&mut self) {
        if let Err(why) = self.shutdown() {
            // Cleanup must not replace the setup/assertion failure, especially while unwinding.
            let _ = writeln!(
                io::stderr().lock(),
                "fixture server shell {} could not be reaped: {why}",
                self.shell.id()
            );
        }
    }
}
