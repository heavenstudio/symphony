//! A temporary project tree shared by unit and integration tests.
#![allow(dead_code)]
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A temporary project root, removed with everything under it when dropped.
pub struct Tree {
    pub root: PathBuf,
}

impl Tree {
    pub fn new() -> Tree {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!("review-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Tree { root }
    }

    /// A tree whose `symphony.toml` names Star Foundry's plugin, for what the game's components,
    /// regions and rules draw.
    pub fn star_foundry() -> Tree {
        let tree = Tree::new();
        tree.write(
            "symphony.toml",
            "[plugins]\ncrates = [\"star-foundry-symphony\"]\n",
        );
        tree
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// Write `text` at `rel`, making its folders.
    pub fn write(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.path(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }

    pub fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.path(rel)).unwrap()
    }

    /// The names in the folder `rel`, sorted: what a write left beside its file.
    pub fn names(&self, rel: &str) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.path(rel))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        names
    }

    /// A file opened for reading before a write, and its permission bits, set to `mode` first.
    pub fn open_with_mode(&self, rel: &str, mode: u32) -> std::fs::File {
        use std::os::unix::fs::PermissionsExt;
        let path = self.path(rel);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        std::fs::File::open(path).unwrap()
    }

    pub fn mode(&self, rel: &str) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(self.path(rel))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
