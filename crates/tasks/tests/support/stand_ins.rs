//! Runnable stand-ins for the agents' programs (`claude`, `codex`) in a folder a test puts on a
//! launch's search path: a launch refuses an agent whose program it does not find there
//! (`symphony_board::launch::off`), and no test may pass or fail by what this machine has
//! installed. The library's tests, the launch tests and the command line's take them from here.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// `folder` made, holding a runnable stand-in for each program of `names`.
pub fn stand_ins(folder: &Path, names: &[&str]) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(folder).unwrap();
    for name in names {
        let program = folder.join(name);
        std::fs::write(&program, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// A folder of the system's temporary one, of this process and `name`, holding stand-ins for
/// `names` alone: removed when dropped.
pub struct StandIns(PathBuf);

impl StandIns {
    pub fn new(name: &str, names: &[&str]) -> StandIns {
        let folder = std::env::temp_dir().join(format!("stand-ins-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        stand_ins(&folder, names);
        StandIns(folder)
    }

    /// The folder as a search path of its own.
    pub fn path(&self) -> OsString {
        self.0.clone().into_os_string()
    }
}

impl Drop for StandIns {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
