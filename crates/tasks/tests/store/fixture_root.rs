//! This layout's fixture source root. The caller supplies its actual compiling package.

use std::path::{Path, PathBuf};

/// The script the fixture's servers are started by, which the workspace owns.
const SCRIPT: &str = "tools/fixture_process.sh";

pub(super) fn workspace(manifest: &Path) -> PathBuf {
    let root = manifest
        .join("../..")
        .canonicalize()
        .expect("the fixture workspace root");
    assert!(
        root.join(SCRIPT).is_file(),
        "the fixture workspace supplies its owned server script"
    );
    root
}

#[cfg(test)]
mod tests {
    use super::{SCRIPT, workspace};
    use std::fs;
    use std::io::ErrorKind;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::panic::catch_unwind;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Layout {
        outer: PathBuf,
        own: PathBuf,
        package: PathBuf,
    }

    impl Layout {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let outer = loop {
                let path = std::env::temp_dir().join(format!(
                    "fixture-root-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&path) {
                    Ok(()) => break path.canonicalize().unwrap(),
                    Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                    Err(error) => panic!("{}: {error}", path.display()),
                }
            };
            let own = outer.join("arbitrary own workspace");
            let package = own.join("crates/native-caller");
            fs::create_dir_all(&package).unwrap();
            for root in [&outer, &own] {
                fs::create_dir(root.join("tools")).unwrap();
                fs::write(root.join(SCRIPT), root.as_os_str().as_encoded_bytes()).unwrap();
            }
            Self {
                outer,
                own,
                package,
            }
        }

        fn script(&self) -> PathBuf {
            self.own.join(SCRIPT)
        }

        fn refuses(&self, package: &Path) {
            assert!(catch_unwind(|| workspace(package)).is_err());
            assert!(self.outer.join(SCRIPT).is_file());
        }
    }

    impl Drop for Layout {
        fn drop(&mut self) {
            let _ = fs::set_permissions(self.own.join("tools"), fs::Permissions::from_mode(0o700));
            let _ = fs::remove_dir_all(&self.outer);
        }
    }

    #[test]
    fn the_compiling_workspace_wins_over_a_same_named_hostile_ancestor_and_cwd() {
        let layout = Layout::new();
        assert!(!std::env::current_dir().unwrap().starts_with(&layout.outer));
        assert_eq!(workspace(&layout.package), layout.own);
        assert_ne!(workspace(&layout.package), layout.outer);
    }

    #[test]
    fn a_missing_own_script_refuses_even_with_a_complete_parent() {
        let layout = Layout::new();
        fs::remove_file(layout.script()).unwrap();
        layout.refuses(&layout.package);
    }

    #[test]
    fn dangling_links_and_directories_are_not_own_script_files() {
        let layout = Layout::new();
        let path = layout.script();
        fs::remove_file(&path).unwrap();
        symlink("missing", &path).unwrap();
        layout.refuses(&layout.package);
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        layout.refuses(&layout.package);
    }

    #[test]
    fn the_actual_package_path_must_canonicalize_before_any_script_can_be_accepted() {
        let layout = Layout::new();
        layout.refuses(&layout.own.join("crates/absent"));
        let cycle = layout.own.join("cycle");
        symlink("cycle", &cycle).unwrap();
        layout.refuses(&cycle.join("crates/native-caller"));
    }

    #[test]
    fn file_presence_follows_links_and_does_not_seal_their_contents_or_readability() {
        let layout = Layout::new();
        let script = layout.script();
        fs::remove_file(&script).unwrap();
        symlink(layout.outer.join(SCRIPT), &script).unwrap();
        assert_eq!(workspace(&layout.package), layout.own);
        fs::remove_file(&script).unwrap();
        fs::write(&script, "unreadable to an ordinary uid").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o0)).unwrap();
        assert!(script.is_file(), "metadata is not a read-permission check");
        assert_eq!(workspace(&layout.package), layout.own);
        // Privileged test users may still read mode000 files; neither outcome is a root seal.
        if let Err(error) = fs::read(&script) {
            assert_eq!(error.kind(), ErrorKind::PermissionDenied);
        }
    }

    #[test]
    fn uncertain_directory_access_is_never_replaced_by_an_ancestor() {
        let layout = Layout::new();
        let tools = layout.own.join("tools");
        fs::set_permissions(&tools, fs::Permissions::from_mode(0o0)).unwrap();
        let visible = layout.script().is_file();
        let result = catch_unwind(|| workspace(&layout.package));
        fs::set_permissions(&tools, fs::Permissions::from_mode(0o700)).unwrap();
        if visible {
            assert_eq!(
                result.unwrap(),
                layout.own,
                "privileged metadata still selects own"
            );
        } else {
            assert!(
                result.is_err(),
                "uncertain metadata must refuse, never use the parent"
            );
        }
    }
}
