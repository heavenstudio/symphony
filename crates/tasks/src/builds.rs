//! Where a project's build of a Symphony binary is. The recipes `symphony init` installs
//! (`crates/symphony/shipped/symphony.just`) run a checkout's `.target/bin/NAME` link, else its main
//! checkout's, else the release the machine runs ([`released`]: `current/bin/NAME` in the machine
//! folder), else `NAME` on `PATH`; the project's build setup makes the links, a release's install
//! the machine's, and nothing here builds. So a project with no build of its own runs the release,
//! and one that builds its own keeps running it. What runs the board for the whole repository, the
//! sweep work finishing starts ([`crate::sweep::trigger`]) and the Symphony's idle sweep and a
//! cancel's cleanup, runs the main checkout's build ([`of_main`]): its link, else the machine's
//! release, else `NAME` on `PATH`, never a linked worktree's, whose build may hold code that has not
//! landed.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// The folder of a checkout's links to its builds.
pub const LINKS: &str = ".target/bin";

/// The machine folder's link to the release the machine runs, and a release's folder of binaries
/// under it.
pub const CURRENT: &str = "current";
const RELEASE_BINARIES: &str = "bin";

/// What a search past a checkout's own links reads: the machine folder, when one is known, and the
/// `PATH` value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Beyond {
    pub machine: Option<PathBuf>,
    pub path: Option<OsString>,
}

impl Beyond {
    /// As the variables `var` reads say: the machine folder by its rule
    /// ([`crate::project::machine_folder`]; none where it cannot be named) and `PATH`.
    pub fn of(var: impl Fn(&str) -> Option<OsString>) -> Beyond {
        Beyond {
            machine: crate::project::machine_folder(&var, cfg!(target_os = "macos")).ok(),
            path: var("PATH"),
        }
    }

    /// This process's.
    pub fn of_process() -> Beyond {
        Beyond::of(|name| std::env::var_os(name))
    }

    /// `PATH` alone: no machine folder is read.
    pub fn path(path: Option<&OsStr>) -> Beyond {
        Beyond {
            machine: None,
            path: path.map(OsStr::to_os_string),
        }
    }

    /// `name` past every link: the machine's release's ([`released`]), else on the search path.
    pub fn find(&self, name: &str) -> Option<PathBuf> {
        (self.machine.as_deref())
            .and_then(|machine| released(machine, name))
            .or_else(|| on_path(name, self.path.as_deref()))
    }
}

/// `name` in the release the machine folder `machine` runs, through its `current` link, when it
/// runs: none while no release is installed, the link dangles or the release holds no such binary.
pub fn released(machine: &Path, name: &str) -> Option<PathBuf> {
    let binary = machine.join(CURRENT).join(RELEASE_BINARIES).join(name);
    runnable(&binary).then_some(binary)
}

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

/// The build of `name` the main checkout `main` runs for the whole repository: its link, else the
/// machine's release's, else `name` on the search path ([`Beyond::find`]).
pub fn of_main(main: &Path, name: &str, beyond: &Beyond) -> Option<PathBuf> {
    link(main, name).or_else(|| beyond.find(name))
}

/// What a run that found no build of `name` for the main checkout `main` says: where to put one.
pub fn missing(main: &Path, name: &str) -> String {
    format!(
        "no build of {name} for {}: link the project's build at {}, install a release on the \
         machine or put {name} on PATH",
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
        let beyond = Beyond::path(Some(&path));
        program(&linked.join(LINKS).join("tasks")); // a branch's build
        assert_eq!(
            of_main(&main_of(&linked), "tasks", &beyond),
            Some(on_path_build)
        );
        let mains = program(&main.join(LINKS).join("tasks"));
        assert_eq!(of_main(&main_of(&linked), "tasks", &beyond), Some(mains));
        assert_eq!(of_main(&main, "agent-send", &Beyond::default()), None);
        assert_eq!(
            missing(&main, "tasks"),
            format!(
                "no build of tasks for {}: link the project's build at {}/.target/bin/tasks, \
                 install a release on the machine or put tasks on PATH",
                main.display(),
                main.display()
            )
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A machine folder at `dir/machine` whose `current` names the release `name`, holding `tasks`.
    fn machine(dir: &Path, name: &str) -> (PathBuf, PathBuf) {
        let machine = dir.join("machine");
        let built = program(&machine.join("releases").join(name).join("bin/tasks"));
        std::os::unix::fs::symlink(Path::new("releases").join(name), machine.join(CURRENT))
            .unwrap();
        (machine, built)
    }

    #[test]
    fn the_machines_release_runs_through_its_current_link_and_only_what_it_holds() {
        let dir = scratch("released");
        assert_eq!(released(&dir.join("machine"), "tasks"), None, "no folder");
        let (machine, built) = machine(&dir, "release-1");
        let through = machine.join("current/bin/tasks");
        assert_eq!(released(&machine, "tasks"), Some(through.clone()));
        assert_eq!(through.canonicalize().unwrap(), built);
        assert_eq!(released(&machine, "agent-send"), None, "not in it");
        std::fs::write(machine.join("current/bin/symphony"), "not runnable\n").unwrap();
        assert_eq!(released(&machine, "symphony"), None);
        std::fs::remove_dir_all(machine.join("releases")).unwrap();
        assert_eq!(released(&machine, "tasks"), None, "a dangling link");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_search_takes_a_checkouts_link_then_the_machines_release_then_the_search_path() {
        let dir = scratch("beyond");
        let (main, _) = checkouts(&dir);
        let (machine, _) = machine(&dir, "release-1");
        let on_path_build = program(&dir.join("path/tasks"));
        program(&dir.join("path/agent-send"));
        let beyond = Beyond {
            machine: Some(machine.clone()),
            path: Some(dir.join("path").into_os_string()),
        };
        let release = machine.join("current/bin/tasks");
        assert_eq!(beyond.find("tasks"), Some(release.clone()));
        assert_eq!(of_main(&main, "tasks", &beyond), Some(release));
        assert_eq!(
            beyond.find("agent-send"),
            Some(dir.join("path/agent-send")),
            "what the release lacks is the search path's"
        );
        assert_eq!(
            Beyond::path(beyond.path.as_deref()).find("tasks"),
            Some(on_path_build),
            "no machine folder read"
        );
        let mains = program(&main.join(LINKS).join("tasks"));
        assert_eq!(of_main(&main, "tasks", &beyond), Some(mains));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_search_reads_the_machine_folder_and_the_search_path_from_the_variables() {
        let vars = |name: &str| match name {
            "SYMPHONY_MACHINE_DIR" => Some(OsString::from("/m")),
            "PATH" => Some(OsString::from("/bin")),
            _ => None,
        };
        assert_eq!(
            Beyond::of(vars),
            Beyond {
                machine: Some(PathBuf::from("/m")),
                path: Some(OsString::from("/bin")),
            }
        );
        assert_eq!(Beyond::of(|_| None), Beyond::default(), "no home, no PATH");
    }
}
