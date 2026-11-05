//! The executables the cases run, each run once in setup, before any case's wait. macOS assesses a
//! new file before its first instruction, one file at a time machine-wide, and a case's start was
//! that file's first run: under load the queue held a fresh copy of the server image past its
//! 45 s ready wait, and the stand-ins' first run past their handshakes. A file already assessed
//! runs in milliseconds, so every installation links the one built image and every stand-in links
//! the machine's one copy of the driver's bytes (`machine_copy`), and both are run here first.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;
use symphony_board::sys::{self, Command, Stdio};

#[path = "../../../../../tools/gate/tests/verify/machine_copy.rs"]
mod machine_copy;

/// A machine copy's first run starts under the spawn lock the board's `sys` takes for every spawn.
fn run_fixture(command: &mut std::process::Command) -> std::io::Result<std::process::ExitStatus> {
    symphony_board::sys::Spawning::status_locked(command)
}

/// The built server image, run once by this process before any start: the uninstalled starts, the
/// installations' links and each replacement's exec all run this one file.
pub(super) fn server_image() -> &'static Path {
    static IMAGE: OnceLock<PathBuf> = OnceLock::new();
    IMAGE.get_or_init(|| {
        let image = PathBuf::from(env!("CARGO_BIN_EXE_symphony-server"));
        let began = Instant::now();
        let done = sys::output(
            Command::new(&image)
                .arg("--help")
                .env_clear()
                .stdin(Stdio::null()),
        )
        .unwrap();
        assert!(
            done.status.success() && done.stdout.starts_with(b"usage: symphony-server "),
            "the built image must answer help before any start: {done:?}"
        );
        println!(
            "after_reload: the server image's first run in this process took {:.3}s",
            began.elapsed().as_secs_f64()
        );
        image
    })
}

/// The machine's copy of `compiled`'s bytes, published once per content and run once by this
/// process before it is handed out.
pub(super) fn stand_in(compiled: &Path) -> PathBuf {
    let began = Instant::now();
    let copy = machine_copy::machine_copy(compiled);
    println!(
        "after_reload: the stand-ins' copy {} was ready after {:.3}s",
        copy.display(),
        began.elapsed().as_secs_f64()
    );
    copy
}
