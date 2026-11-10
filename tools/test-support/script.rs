//! Fixture paths share a script per interpreter; each path's body is interpreter input.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

#[path = "machine_copy.rs"]
pub mod machine_copy;

/// How the including crate starts a child, which a machine copy's first run follows: the wrapper
/// it includes this file through names it (`script_process_cwd.rs`, `script_board.rs`,
/// `script_std.rs`).
fn run_fixture(command: &mut std::process::Command) -> std::io::Result<std::process::ExitStatus> {
    super::run_fixture(command)
}

/// The tracked prepared script for `body`'s interpreter.
pub fn tracked_wrapper(body: &str) -> PathBuf {
    wrapper_for(Path::new(env!("CARGO_MANIFEST_DIR")), body)
}

pub(super) fn wrapper_for(manifest: &Path, body: &str) -> PathBuf {
    let name = if body.starts_with("#!/bin/sh\n") {
        "script-sh.sh"
    } else {
        "script.sh"
    };
    let wrapper = manifest.join("../../tools/test-support").join(name);
    assert!(
        wrapper.is_file(),
        "owned fixture script {}",
        wrapper.display()
    );
    wrapper
}

fn write_body(path: &Path, body: &str) -> PathBuf {
    let mut source = path.as_os_str().to_owned();
    source.push(".body");
    fs::write(source, body).unwrap();
    tracked_wrapper(body)
}

/// Install or rewrite a stand-in. Its path shares executable bytes: change it only through this
/// function, never by writing or chmodding the path. Permission and malformed-executable cases keep
/// owned real files. Native links require the fixture and the machine's copy on the same
/// filesystem; on another, they link the tracked script.
pub fn write(path: &Path, body: &str, native: bool) {
    let tracked = write_body(path, body);
    // The driver still executes the requested path, preserving argv[0] for the body lookup.
    // Only the shared script is subject to macOS's first-execution assessment, and the tracked
    // one is a fresh file in every checkout and verify generation: link the machine's one copy of
    // its bytes, assessed once per content on this machine.
    let wrapper = machine_copy::machine_copy(&tracked);
    // Unlink the fixture entry rather than writing through it: it may share a tracked wrapper.
    // Replacing the entry also selects the right interpreter after a rewrite or a plain-file stub.
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("remove fixture {}: {error}", path.display()),
    }
    if native {
        // Native discovery canonicalizes the executable and requires it inside the target tree.
        match fs::hard_link(&wrapper, path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
                fs::hard_link(tracked, path).unwrap();
            }
            Err(error) => panic!("link fixture {}: {error}", path.display()),
        }
    } else {
        symlink(wrapper, path).unwrap();
    }
}
