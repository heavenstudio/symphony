//! The executables the cases run, each run once in setup, before any case's wait. macOS assesses a
//! new file before its first instruction, one file at a time machine-wide, and a case's start was
//! that file's first run: under load the queue held a fresh copy of the server image past its
//! 45 s ready wait, and the stand-ins' first run past their handshakes. A file already assessed
//! runs in milliseconds, so every installation links the one server image, every stand-in links
//! the machine's one copy of the driver's bytes (`machine_copy`), and the generations run one board
//! CLI, each run here first.
//!
//! The server image and the board CLI are build outputs, which a rebuild during the run replaces:
//! the linker writes a new file and the output's name moves to it. So each is this process's own
//! hard link to the output's file as setup found it ([`private_link`]): the same file, so it shares
//! the output's first run and costs no copy, and one no rebuild changes, so an installation's stamp
//! and the replacements' exec keep the bytes setup ran and hashed.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;
use symphony_board::sys::{self, Command, Stdio};

#[path = "../../../../tools/test-support/machine_copy.rs"]
mod machine_copy;

/// A machine copy's first run starts under the spawn lock the board's `sys` takes for every spawn.
fn run_fixture(command: &mut std::process::Command) -> std::io::Result<std::process::ExitStatus> {
    symphony_board::sys::Spawning::status_locked(command)
}

/// This process's folder of private links, in the temporary folder the runner gives it, removed as
/// the process exits.
fn private_folder() -> &'static Path {
    static FOLDER: OnceLock<PathBuf> = OnceLock::new();
    FOLDER.get_or_init(|| {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let folder = std::env::temp_dir().join(format!(
            "symphony-after-reload-images-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&folder).unwrap();
        let folder = folder.canonicalize().unwrap();
        extern "C" fn remove() {
            if let Some(folder) = FOLDER.get() {
                let _ = fs::remove_dir_all(folder);
            }
        }
        // SAFETY: atexit's one argument is `remove`, a function of no arguments that cannot unwind
        // (it ignores the removal's error); at exit it reads the static, which holds the folder
        // once this initializer has returned and nothing before.
        assert_eq!(
            unsafe { libc::atexit(remove) },
            0,
            "the private links' removal at exit is set"
        );
        folder
    })
}

/// `built` as a file of `folder`'s, under its own name: a hard link to the build output's file, so a
/// rebuild, which moves the output's name to a new file, leaves this one the bytes it held. A copy
/// where `folder` is on another volume, run once by the caller like the link.
pub(super) fn private_link(built: &Path, folder: &Path) -> PathBuf {
    let link = folder.join(built.file_name().unwrap());
    match fs::hard_link(built, &link) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
            fs::copy(built, &link).unwrap_or_else(|error| {
                panic!(
                    "cannot copy {} to {}: {error}",
                    built.display(),
                    link.display()
                )
            });
        }
        Err(error) => panic!(
            "cannot link {} as {}: {error}",
            built.display(),
            link.display()
        ),
    }
    link
}

/// `binary --help` run once, answering with its own usage line, which starts `usage`; how long that
/// took.
fn run_once(binary: &Path, usage: &str) -> f64 {
    let began = Instant::now();
    let done = sys::output(
        Command::new(binary)
            .arg("--help")
            .env_clear()
            .stdin(Stdio::null()),
    )
    .unwrap();
    assert!(
        done.status.success() && done.stdout.starts_with(usage.as_bytes()),
        "{} must answer help with {usage:?} before any start: {done:?}",
        binary.display()
    );
    began.elapsed().as_secs_f64()
}

/// The built server image, as this process's link, run once by this process before any start: the
/// uninstalled starts, the installations' links and each replacement's exec all run this one file.
pub(super) fn server_image() -> &'static Path {
    static IMAGE: OnceLock<PathBuf> = OnceLock::new();
    IMAGE.get_or_init(|| {
        let image = private_link(
            Path::new(env!("CARGO_BIN_EXE_symphony-server")),
            private_folder(),
        );
        println!(
            "after_reload: the server image's first run in this process took {:.3}s",
            run_once(&image, "usage: symphony-server ")
        );
        image
    })
}

/// An installation stamp's `binary` and `image` for `path`: the hash of its bytes and the file they
/// were read from, through links, as the server's own stamp records them (src/reload/stamp.rs).
fn stamp_of(path: &Path) -> (String, Value) {
    let binary = Sha256::digest(fs::read(path).unwrap())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let meta = fs::metadata(path).unwrap();
    (
        binary,
        json!({
            "device": meta.dev(),
            "inode": meta.ino(),
            "size": meta.len(),
            "modified": [meta.mtime(), meta.mtime_nsec()],
        }),
    )
}

/// [`server_image`]'s stamp, taken once per process, so neither a case's setup nor its installed
/// start reads the image.
pub(super) fn server_stamp() -> &'static (String, Value) {
    static STAMP: OnceLock<(String, Value)> = OnceLock::new();
    STAMP.get_or_init(|| {
        let began = Instant::now();
        let stamp = stamp_of(server_image());
        println!(
            "after_reload: the server image's stamp took {:.3}s",
            began.elapsed().as_secs_f64()
        );
        stamp
    })
}

/// The board CLI the generations run (`SYMPHONY_TASKS_CLI`), as this process's link of `built`, run
/// once here: the verdict commit runs it inside a case's wait for its check (`commit::app_actor`).
pub(super) fn board_cli(built: &Path) -> PathBuf {
    let cli = private_link(built, private_folder());
    println!(
        "after_reload: the board CLI's first run in this process took {:.3}s",
        run_once(&cli, "usage: just tasks ")
    );
    cli
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

#[cfg(test)]
mod tests {
    use super::{private_link, stamp_of};
    use std::fs;
    use std::path::PathBuf;

    /// A folder of this case's own, removed when it ends.
    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A rebuild of the server during a run writes a new file and moves the output's name to it (the
    /// linker renames its output into place; cargo moves the result under the output's name). The
    /// stamp setup writes for an installation of the private link, its hash and the file it was
    /// taken of, still names the installed file after both, so its start takes the stamp as
    /// current, unread. One of the output's name would follow the rebuild, and its start would find
    /// the stamp stale and build at once.
    #[test]
    fn rebuilding_the_server_binary_mid_run_leaves_the_installed_stamp_current() {
        let scratch = Scratch(std::env::temp_dir().join(format!(
            "symphony-after-reload-rebuilt-{}",
            std::process::id()
        )));
        let _ = fs::remove_dir_all(&scratch.0);
        // A build output of this case's own, linked the way setup links the real one, installed.
        let output = scratch.0.join("target/debug/symphony-server");
        let private = scratch.0.join("private");
        let installed = scratch.0.join("installation/bin/symphony-server");
        for folder in [
            output.parent().unwrap(),
            &private,
            installed.parent().unwrap(),
        ] {
            fs::create_dir_all(folder).unwrap();
        }
        fs::write(&output, "the image the run began with").unwrap();
        std::os::unix::fs::symlink(private_link(&output, &private), &installed).unwrap();
        let stamp = stamp_of(&installed);

        let next = output.with_file_name("symphony-server.ld_next");
        fs::write(&next, "the image a rebuild made").unwrap();
        fs::rename(&next, &output).unwrap();
        let rebuilt = stamp_of(&output);
        assert_ne!(
            rebuilt.0, stamp.0,
            "the rebuild replaced the output's bytes"
        );
        assert_ne!(rebuilt.1, stamp.1, "and its file");
        assert_eq!(
            stamp_of(&installed),
            stamp,
            "the installation is still the file its stamp names"
        );
        // The uplift: the output moved under deps/ and linked back under its name.
        let deps = output.with_file_name("deps/symphony_server-0123");
        fs::create_dir_all(deps.parent().unwrap()).unwrap();
        fs::rename(&output, &deps).unwrap();
        fs::hard_link(&deps, &output).unwrap();
        assert_eq!(stamp_of(&installed), stamp);
    }
}
