//! Where a project's build of a Symphony binary is. The recipes `symphony init` installs
//! (`crates/symphony/shipped/symphony.just`) run a checkout's `.target/bin/NAME` link, else its main
//! checkout's, else `NAME` on `PATH`; the project's build setup makes the links, and nothing here
//! builds. What runs the board for the whole repository, the sweep work finishing starts
//! ([`crate::sweep::trigger`]) and the Symphony's idle sweep and a cancel's cleanup, runs the main
//! checkout's build ([`of_main`]): its link, else `NAME` on `PATH`, never a linked worktree's, whose
//! build may hold code that has not landed.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The folder of a checkout's links to its builds.
pub const LINKS: &str = ".target/bin";

/// `root`'s link to its build of `name` when it runs.
pub fn link(root: &Path, name: &str) -> Option<PathBuf> {
    let link = root.join(LINKS).join(name);
    runnable(&link).then_some(link)
}

/// `name` in the first folder of `path` (a `PATH` value) holding it runnable.
pub fn on_path(name: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path?)
        .filter(|folder| !folder.as_os_str().is_empty())
        .map(|folder| folder.join(name))
        .find(|candidate| runnable(candidate))
}

/// Whether `path` is a file with an execute bit set, through any link.
pub fn runnable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// The main checkout of the checkout at `root`: the folder holding its common dir when that is a
/// `.git`, read from the checkout's own marker with no child process; `root` itself for a main
/// checkout, a folder git does not hold, or a common dir kept elsewhere.
pub fn main_of(root: &Path) -> PathBuf {
    crate::git::checkout_common_dir(root)
        .and_then(|common| crate::sweep::trigger::main_checkout(&common).map(Path::to_path_buf))
        .unwrap_or_else(|| root.to_path_buf())
}

/// The build of `name` the main checkout `main` runs for the whole repository: its link, else `name`
/// on `path`.
pub fn of_main(main: &Path, name: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    link(main, name).or_else(|| on_path(name, path))
}

/// What a run that found no build of `name` for the main checkout `main` says: where to put one.
pub fn missing(main: &Path, name: &str) -> String {
    format!(
        "no build of {name} for {}: link the project's build at {} or put {name} on PATH",
        main.display(),
        main.join(LINKS).join(name).display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A folder of its own under the temporary one, for test `name`, every link in it resolved.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("symphony-builds-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn program(path: &Path) -> PathBuf {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_path_buf()
    }

    /// A main checkout at `main/` and a linked worktree of it at `linked/`, with the markers git
    /// writes.
    fn checkouts(dir: &Path) -> (PathBuf, PathBuf) {
        std::fs::create_dir_all(dir.join("main/.git/worktrees/linked")).unwrap();
        std::fs::write(dir.join("main/.git/worktrees/linked/commondir"), "../..\n").unwrap();
        std::fs::create_dir_all(dir.join("linked")).unwrap();
        std::fs::write(
            dir.join("linked/.git"),
            format!(
                "gitdir: {}\n",
                dir.join("main/.git/worktrees/linked").display()
            ),
        )
        .unwrap();
        (dir.join("main"), dir.join("linked"))
    }

    #[test]
    fn a_link_runs_only_when_it_names_an_executable_file() {
        let dir = scratch("link");
        assert_eq!(link(&dir, "tasks"), None);
        std::fs::create_dir_all(dir.join(LINKS)).unwrap();
        std::fs::write(dir.join(LINKS).join("tasks"), "not runnable\n").unwrap();
        assert_eq!(link(&dir, "tasks"), None);
        let built = program(&dir.join(LINKS).join("tasks"));
        assert_eq!(link(&dir, "tasks"), Some(built));
        std::fs::create_dir_all(dir.join(LINKS).join("symphony")).unwrap();
        assert_eq!(link(&dir, "symphony"), None, "a folder never runs");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_search_path_is_read_in_order_past_empty_and_missing_folders() {
        let dir = scratch("path");
        let second = program(&dir.join("b/tasks"));
        program(&dir.join("c/tasks"));
        std::fs::create_dir_all(dir.join("a")).unwrap();
        let path = std::env::join_paths([
            PathBuf::new(),
            dir.join("a"),
            dir.join("gone"),
            dir.join("b"),
            dir.join("c"),
        ])
        .unwrap();
        assert_eq!(on_path("tasks", Some(&path)), Some(second));
        assert_eq!(on_path("agent-send", Some(&path)), None);
        assert_eq!(on_path("tasks", None), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_linked_worktrees_main_checkout_is_its_common_dirs_folder_and_any_other_is_itself() {
        let dir = scratch("main");
        let (main, linked) = checkouts(&dir);
        assert_eq!(main_of(&linked), main);
        assert_eq!(main_of(&main), main);
        let plain = dir.join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        assert_eq!(main_of(&plain), plain, "no checkout");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_main_checkouts_build_is_its_link_else_the_search_paths_never_a_worktrees() {
        let dir = scratch("of-main");
        let (main, linked) = checkouts(&dir);
        let on_path_build = program(&dir.join("path/tasks"));
        let path = dir.join("path").into_os_string();
        program(&linked.join(LINKS).join("tasks")); // a branch's build
        assert_eq!(
            of_main(&main_of(&linked), "tasks", Some(&path)),
            Some(on_path_build)
        );
        let mains = program(&main.join(LINKS).join("tasks"));
        assert_eq!(
            of_main(&main_of(&linked), "tasks", Some(&path)),
            Some(mains)
        );
        assert_eq!(of_main(&main, "agent-send", None), None);
        assert_eq!(
            missing(&main, "tasks"),
            format!(
                "no build of tasks for {}: link the project's build at {}/.target/bin/tasks or \
                 put tasks on PATH",
                main.display(),
                main.display()
            )
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
