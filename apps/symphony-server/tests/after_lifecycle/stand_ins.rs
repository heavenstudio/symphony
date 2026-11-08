//! The end of every fixture stand-in a server starts itself ([`StandIns`]), which holds none of
//! the case's [`super::Descendants`] endpoints.

use super::{cleanup_failed, mkfifo, wait_cleanup};
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The FIFO, below a case's scratch (its fixture `HOME`), that a stand-in opens for reading before
/// it writes anything, and then finds still in place (`ps.sh`).
pub(super) const STAND_IN_LIFETIME: &str = "stand-ins/lifetime";

/// The end of every fixture stand-in a server starts itself. A server starts its census `ps`
/// without a fork, handing it none of the case's endpoints, and its stop kills that shell alone:
/// a child the shell had started (`mv`) still renamed a file into scratch after the server had
/// ended. The stand-in holds this FIFO open for reading instead, as does everything it starts,
/// and the case holds its one writer, whose write fails once none of them is left. An open for
/// reading never creates a path, so no interleaving with the withdrawal creates one.
pub(super) struct StandIns {
    fifo: PathBuf,
    writer: File,
    closed: bool,
}

impl StandIns {
    pub(super) fn open(home: &Path) -> Self {
        let fifo = home.join(STAND_IN_LIFETIME);
        fs::create_dir(fifo.parent().unwrap()).unwrap();
        mkfifo(&fifo);
        // A FIFO's writer opens only beside a reader: this one is held for that open alone, so the
        // case holds no reader of its own. Its writer, held until the case ends, lets every
        // stand-in's open return at once.
        let opening = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&fifo)
            .unwrap();
        let writer = fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&fifo)
            .unwrap();
        drop(opening);
        Self {
            fifo,
            writer,
            closed: false,
        }
    }

    /// Whether no stand-in holds the lifetime: a write to a FIFO nobody reads fails with EPIPE
    /// (std ignores SIGPIPE). While one does, each look leaves a byte it never reads.
    fn ended(mut writer: &File) -> Result<bool, String> {
        match writer.write(&[0]) {
            Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(false),
            Ok(_) => Ok(false),
            other => Err(format!("fixture stand-in lifetime: {other:?}")),
        }
    }

    /// Withdraw the FIFO in one unlink, so a stand-in that opens it later fails and one that
    /// opened it before finds it gone, and either writes nothing; wait for every one that found
    /// it in place to end; then remove its folder, in which no stand-in creates anything. Once.
    pub(super) fn close(&mut self) {
        if std::mem::replace(&mut self.closed, true) {
            return;
        }
        let fifo = &self.fifo;
        if let Err(error) = fs::remove_file(fifo) {
            cleanup_failed(&format!(
                "cannot withdraw the stand-in lifetime {}: {error}",
                fifo.display()
            ));
        }
        let writer = &self.writer;
        wait_cleanup(
            || {
                format!(
                    "fixture stand-ins holding {} to exit before scratch removal",
                    fifo.display()
                )
            },
            || Self::ended(writer),
            Instant::elapsed,
        );
        let folder = fifo.parent().unwrap();
        if let Err(error) = fs::remove_dir(folder) {
            cleanup_failed(&format!(
                "cannot remove the stand-in folder {}: {error}",
                folder.display()
            ));
        }
    }
}

impl Drop for StandIns {
    fn drop(&mut self) {
        self.close();
    }
}
