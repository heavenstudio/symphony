//! A folder of one test's own under the system's temporary folder, removed when it is dropped,
//! and the private files a notice plugin keeps in it. The unit tests and the plugin's tests share
//! it.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new() -> Scratch {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "symphony-notice-whatsapp-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("a scratch folder");
        Scratch(dir.canonicalize().expect("the scratch folder's path"))
    }

    /// The folder, as a machine folder.
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// Its `notices/`, its owner's alone, holding each of `files` (a name and what it holds), each
    /// its owner's alone.
    pub(crate) fn notices(&self, files: &[(&str, &str)]) -> PathBuf {
        let folder = self.0.join("notices");
        fs::create_dir_all(&folder).expect("the notices folder");
        mode(&folder, 0o700);
        for (name, text) in files {
            let path = folder.join(name);
            fs::write(&path, text).expect("a file in the notices folder");
            mode(&path, 0o600);
        }
        folder
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// `path` given the permissions `mode`.
pub(crate) fn mode(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("the mode set");
}
