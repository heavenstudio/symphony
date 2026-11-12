//! A finished worktree's evidence, set aside before the worktree goes. What git ignores under its
//! artifacts folder (the project's `[project] artifacts`, `artifacts/` by default: a task's pictures
//! its thread names, a test's leftovers, a fixture repository) is moved into the main checkout's
//! artifacts folder, in [`SWEPT`] under the worktree's folder name, so removing the worktree loses
//! none of it; a removal git then refuses moves it back. Its build output is never set aside (a
//! folder tagged as a cache, as Cargo's build folders are, or compiled code): it goes with the
//! worktree. A move is a rename on one disk: a worktree whose evidence is on another disk than the
//! main checkout keeps it, and stays. Nothing removes what was set aside ([`super::kept`]).

use super::git;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

/// The folder in the project's artifacts folder of the main checkout where a removed worktree's
/// artifacts go.
pub const SWEPT: &str = "swept";

/// Where the main checkout `main` keeps what the sweep set aside, `folder` the project's artifacts
/// folder.
pub fn swept(main: &Path, folder: &str) -> PathBuf {
    main.join(folder).join(SWEPT)
}

/// Where the artifacts of worktree `tree` go in the main checkout `main`: `FOLDER/swept/NAME`, NAME
/// the worktree's folder name, then `NAME-2`, `NAME-3`… while one is taken.
pub fn destination(main: &Path, folder: &str, tree: &Path) -> Option<PathBuf> {
    let name = tree.file_name()?.to_string_lossy().into_owned();
    let swept = swept(main, folder);
    (1..1000)
        .map(|n| {
            if n == 1 {
                swept.join(&name)
            } else {
                swept.join(format!("{name}-{n}"))
            }
        })
        .find(|at| fs::symlink_metadata(at).is_err())
}

/// What of the artifacts folder `folder` in worktree `tree` is set aside: what git ignores there,
/// each as it lists it (`FOLDER/` for a folder ignored whole), less its build output ([`built`]),
/// which goes with the worktree; a folder holding some is set aside entry by entry around it. None
/// when git cannot say. A worktree with no such folder has nothing to list, and git is not asked.
pub fn entries(tree: &Path, folder: &str) -> Option<Vec<String>> {
    if fs::symlink_metadata(tree.join(folder)).is_err() {
        return Some(Vec::new());
    }
    let below = format!("{folder}/");
    let done = git::read(
        tree,
        &[
            "status",
            "--porcelain",
            "-z",
            "--ignored=matching",
            "--untracked-files=all",
            "--",
            &below,
        ],
    )?;
    let text = String::from_utf8(done.stdout).ok()?;
    let mut kept = Vec::new();
    for name in text
        .split('\0')
        .filter_map(|record| record.strip_prefix("!! "))
        .filter(|name| name.starts_with(&below) || *name == folder)
    {
        without_build(tree, name, &mut kept);
    }
    Some(kept)
}

/// The file every cache folder holds by the Cache Directory Tagging convention, Cargo's build
/// folders among them.
const CACHE_TAG: &str = "CACHEDIR.TAG";

/// Whether `path` is build output, never set aside: a folder tagged as a cache ([`CACHE_TAG`], as
/// Cargo tags each build folder it makes), or a file of compiled code (an executable or a library,
/// Mach-O, ELF or WebAssembly, or an archive of objects, as an `.rlib` is), by its first bytes. A
/// link is neither.
fn built(path: &Path, meta: &fs::Metadata) -> bool {
    if meta.is_dir() {
        return fs::symlink_metadata(path.join(CACHE_TAG)).is_ok_and(|tag| tag.is_file());
    }
    if !meta.is_file() {
        return false;
    }
    let mut head = [0u8; 8];
    let read = fs::File::open(path).and_then(|mut file| {
        use std::io::Read as _;
        file.read(&mut head)
    });
    let Ok(read) = read else {
        return false;
    };
    let head = &head[..read];
    const COMPILED: [&[u8]; 8] = [
        b"\x7fELF",
        b"\xfe\xed\xfa\xce",
        b"\xfe\xed\xfa\xcf",
        b"\xce\xfa\xed\xfe",
        b"\xcf\xfa\xed\xfe",
        b"\xca\xfe\xba\xbe",
        b"\0asm",
        b"!<arch>\n",
    ];
    COMPILED.iter().any(|magic| head.starts_with(magic))
}

/// Whether folder `dir` holds build output anywhere below it, its links not followed.
fn holds_build(dir: &Path) -> bool {
    let Ok(children) = fs::read_dir(dir) else {
        return false;
    };
    children.flatten().any(|child| {
        let path = child.path();
        fs::symlink_metadata(&path)
            .is_ok_and(|meta| built(&path, &meta) || (meta.is_dir() && holds_build(&path)))
    })
}

/// Add to `kept` what of `entry` (a path of worktree `tree` as git lists it, `/`-ended for a
/// folder) is set aside: nothing when it is build output, itself when it holds none, else each of
/// its own entries so. An entry it cannot read, or a name that is not UTF-8 inside one, is kept
/// whole: the set-aside keeps more, never less.
fn without_build(tree: &Path, entry: &str, kept: &mut Vec<String>) {
    let path = tree.join(entry.trim_end_matches('/'));
    let Ok(meta) = fs::symlink_metadata(&path) else {
        kept.push(entry.to_string());
        return;
    };
    if built(&path, &meta) {
        return;
    }
    if !meta.is_dir() || !holds_build(&path) {
        kept.push(entry.to_string());
        return;
    }
    let names: Option<Vec<String>> = fs::read_dir(&path).ok().and_then(|children| {
        children
            .flatten()
            .map(|child| child.file_name().into_string().ok())
            .collect()
    });
    let Some(mut names) = names else {
        kept.push(entry.to_string());
        return;
    };
    names.sort();
    let base = entry.trim_end_matches('/');
    for name in names {
        let child = format!("{base}/{name}");
        let folder = fs::symlink_metadata(path.join(&name)).is_ok_and(|meta| meta.is_dir());
        without_build(
            tree,
            &if folder { format!("{child}/") } else { child },
            kept,
        );
    }
}

/// What the entries of `tree` that [`entries`] lists hold on disk.
pub fn bytes(tree: &Path, entries: &[String]) -> u64 {
    entries
        .iter()
        .map(|entry| super::size::bytes(&tree.join(entry.trim_end_matches('/'))))
        .sum()
}

/// Where `entry` of a worktree goes under `to`: the folder itself for the artifacts folder
/// `folder`, else its path below that folder.
fn target(to: &Path, folder: &str, entry: &str) -> PathBuf {
    let below = entry.strip_prefix(folder).unwrap_or("").trim_matches('/');
    if below.is_empty() {
        to.to_path_buf()
    } else {
        to.join(below)
    }
}

/// The device `path` is on, or the nearest folder above it that is there.
fn device(path: &Path) -> Option<u64> {
    path.ancestors()
        .find_map(|at| fs::symlink_metadata(at).ok())
        .map(|meta| meta.dev())
}

/// Why the artifacts folder `folder` of worktree `tree` cannot be set aside in the main checkout
/// `main` by a rename, if it cannot: it is on another disk.
pub fn movable(main: &Path, folder: &str, tree: &Path) -> Result<(), String> {
    let (from, into) = (tree.join(folder), swept(main, folder));
    match (device(&from), device(&into)) {
        (Some(a), Some(b)) if a == b => Ok(()),
        _ => Err(format!(
            "its {folder}/ is on another disk than {}, and the sweep sets evidence aside by moving \
             it, never by copying it",
            into.display()
        )),
    }
}

/// Why `from` could not be moved to `into`.
fn refused(from: &Path, into: &Path, error: &io::Error) -> String {
    if error.raw_os_error() == Some(libc::EXDEV) {
        return format!(
            "{} is on another disk than {}, and the sweep sets evidence aside by moving it, never \
             by copying it",
            from.display(),
            into.display()
        );
    }
    format!(
        "{} cannot be moved to {}: {error}",
        from.display(),
        into.display()
    )
}

/// A worktree's artifacts, moved: what went where, and the folders made for them, to put back.
#[derive(Debug)]
pub struct Aside {
    /// The folder they went to.
    pub to: PathBuf,
    moved: Vec<(PathBuf, PathBuf)>,
    made: Vec<PathBuf>,
}

impl Aside {
    /// Move what git ignores under the artifacts folder `folder` of worktree `tree` into the main
    /// checkout `main`; none when there is nothing to move. Refused, with every entry put back,
    /// when git cannot list them, they are on another disk, or one cannot be moved or would land on
    /// something there already: the worktree then stays.
    pub fn take(main: &Path, folder: &str, tree: &Path) -> Result<Option<Aside>, String> {
        let entries = entries(tree, folder)
            .ok_or_else(|| format!("git cannot list what its {folder}/ holds"))?;
        if entries.is_empty() {
            return Ok(None);
        }
        movable(main, folder, tree)?;
        let to = destination(main, folder, tree)
            .ok_or_else(|| format!("no free name under {}", swept(main, folder).display()))?;
        let mut aside = Aside {
            to,
            moved: Vec::new(),
            made: Vec::new(),
        };
        aside.move_all(tree, folder, &entries)?;
        Ok(Some(aside))
    }

    /// Move each of `entries` of worktree `tree` below [`Aside::to`]; refused, every one moved put
    /// back, when one cannot be, or its place there is taken (a name another move just made):
    /// nothing there is ever replaced.
    fn move_all(&mut self, tree: &Path, folder: &str, entries: &[String]) -> Result<(), String> {
        for entry in entries {
            let from = tree.join(entry.trim_end_matches('/'));
            let into = target(&self.to, folder, entry);
            let moved = into
                .parent()
                .map_or(Ok(()), |parent| self.make(parent))
                .and_then(|()| {
                    if fs::symlink_metadata(&into).is_ok() {
                        return Err(io::Error::from(io::ErrorKind::AlreadyExists));
                    }
                    fs::rename(&from, &into)
                });
            if let Err(error) = moved {
                let why = refused(&from, &into, &error);
                self.undo();
                return Err(why);
            }
            self.moved.push((from, into));
        }
        Ok(())
    }

    /// Make `dir` and every folder above it not there yet, each remembered to remove on
    /// [`Aside::put_back`].
    fn make(&mut self, dir: &Path) -> io::Result<()> {
        let missing: Vec<&Path> = dir
            .ancestors()
            .take_while(|at| fs::symlink_metadata(at).is_err())
            .collect();
        for at in missing.into_iter().rev() {
            match fs::create_dir(at) {
                Ok(()) => self.made.push(at.to_path_buf()),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    /// Move every entry back where it was: the removal it was set aside for did not happen. One that
    /// cannot go back stays where it went, kept all the same. The folders made for them go, the
    /// deepest first, each only when empty.
    pub fn put_back(mut self) {
        self.undo();
    }

    fn undo(&mut self) {
        for (from, into) in self.moved.drain(..).rev() {
            let _ = fs::rename(into, from);
        }
        for dir in self.made.drain(..).rev() {
            let _ = fs::remove_dir(dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOLDER: &str = "artifacts";

    #[test]
    fn a_name_taken_gets_the_next_number() {
        let main = std::env::temp_dir().join(format!("sf-sweep-aside-{}", std::process::id()));
        let _ = fs::remove_dir_all(&main);
        let tree = Path::new("/wt/agent-a5a6");
        assert_eq!(
            destination(&main, FOLDER, tree),
            Some(main.join("artifacts/swept/agent-a5a6"))
        );
        assert_eq!(
            destination(&main, "out/shots", tree),
            Some(main.join("out/shots/swept/agent-a5a6"))
        );
        fs::create_dir_all(main.join("artifacts/swept/agent-a5a6")).unwrap();
        fs::write(main.join("artifacts/swept/agent-a5a6-2"), "a file\n").unwrap();
        assert_eq!(
            destination(&main, FOLDER, tree),
            Some(main.join("artifacts/swept/agent-a5a6-3"))
        );
        assert_eq!(destination(&main, FOLDER, Path::new("/")), None);
        fs::remove_dir_all(&main).unwrap();
    }

    /// A scratch repository at `root` that ignores what is under `folder` but its `kept.md`, which
    /// it tracks.
    fn repo(root: &Path, folder: &str) {
        let _ = fs::remove_dir_all(root);
        fs::create_dir_all(root.join(folder)).unwrap();
        fs::write(
            root.join(".gitignore"),
            format!("/{folder}/*\n!/{folder}/kept.md\n"),
        )
        .unwrap();
        fs::write(root.join(folder).join("kept.md"), "tracked\n").unwrap();
        for args in [
            &["init", "-q"][..],
            &["add", "."],
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-qm",
                "init",
            ],
        ] {
            let done = std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .unwrap();
            assert!(done.status.success(), "git {args:?}: {done:?}");
        }
    }

    fn scratch(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let base =
            std::env::temp_dir().join(format!("sf-sweep-aside-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        (base.clone(), base.join("main"), base.join("tree"))
    }

    #[test]
    fn nothing_ignored_under_artifacts_sets_nothing_aside() {
        let (base, main, tree) = scratch("none");
        repo(&tree, FOLDER);
        assert_eq!(entries(&tree, FOLDER), Some(vec![]));
        assert!(Aside::take(&main, FOLDER, &tree).unwrap().is_none());
        assert!(!main.exists(), "nothing was made in the main checkout");
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_folder_git_cannot_list_refuses_the_set_aside() {
        let (base, main, tree) = scratch("nogit");
        fs::create_dir_all(tree.join("artifacts")).unwrap();
        fs::write(tree.join("artifacts/a.png"), "a\n").unwrap();
        assert_eq!(entries(&tree, FOLDER), None);
        assert_eq!(
            Aside::take(&main, FOLDER, &tree).unwrap_err(),
            "git cannot list what its artifacts/ holds"
        );
        assert!(tree.join("artifacts/a.png").exists());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn an_entry_that_cannot_move_stays_where_it_was() {
        let (base, main, tree) = scratch("stuck");
        repo(&tree, FOLDER);
        fs::write(tree.join("artifacts/a.png"), "a\n").unwrap();
        fs::write(tree.join("artifacts/b.png"), "b\n").unwrap();
        assert_eq!(
            entries(&tree, FOLDER),
            Some(vec![
                "artifacts/a.png".to_string(),
                "artifacts/b.png".to_string()
            ])
        );
        // The main checkout's swept folder is a file: no folder can be made below it.
        fs::create_dir_all(main.join("artifacts")).unwrap();
        fs::write(swept(&main, FOLDER), "not a folder\n").unwrap();
        let refused = Aside::take(&main, FOLDER, &tree).unwrap_err();
        assert!(
            refused.starts_with(&format!(
                "{} cannot be moved to {}: ",
                tree.join("artifacts/a.png").display(),
                swept(&main, FOLDER).join("tree/a.png").display()
            )),
            "{refused}"
        );
        for (file, text) in [("a.png", "a\n"), ("b.png", "b\n")] {
            assert_eq!(
                fs::read_to_string(tree.join("artifacts").join(file)).unwrap(),
                text
            );
        }
        assert_eq!(
            fs::read_to_string(swept(&main, FOLDER)).unwrap(),
            "not a folder\n"
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_move_onto_a_name_there_already_is_refused_and_the_first_goes_back() {
        let (base, main, tree) = scratch("taken");
        repo(&tree, FOLDER);
        fs::write(tree.join("artifacts/a.png"), "a\n").unwrap();
        fs::write(tree.join("artifacts/b.png"), "b\n").unwrap();
        // Another sweep's move put a b.png in the folder this one picked while it was free, as a
        // race of two sweeps would: it is never replaced.
        let to = swept(&main, FOLDER).join("tree");
        fs::create_dir_all(&to).unwrap();
        fs::write(to.join("b.png"), "theirs\n").unwrap();
        let mut aside = Aside {
            to: to.clone(),
            moved: Vec::new(),
            made: Vec::new(),
        };
        let listed = entries(&tree, FOLDER).unwrap();
        assert_eq!(
            aside.move_all(&tree, FOLDER, &listed).unwrap_err(),
            format!(
                "{} cannot be moved to {}: entity already exists",
                tree.join("artifacts/b.png").display(),
                to.join("b.png").display()
            )
        );
        for (file, text) in [("a.png", "a\n"), ("b.png", "b\n")] {
            assert_eq!(
                fs::read_to_string(tree.join("artifacts").join(file)).unwrap(),
                text
            );
        }
        assert_eq!(fs::read_to_string(to.join("b.png")).unwrap(), "theirs\n");
        assert!(!to.join("a.png").exists());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn the_folders_made_for_a_move_go_back_the_deepest_first() {
        let (base, main, _) = scratch("made");
        let to = swept(&main, FOLDER).join("tree");
        let mut aside = Aside {
            to: to.clone(),
            moved: Vec::new(),
            made: Vec::new(),
        };
        aside.make(&to).unwrap();
        assert_eq!(
            aside.made,
            [
                base.clone(),
                main.clone(),
                main.join("artifacts"),
                swept(&main, FOLDER),
                to.clone()
            ]
        );
        // One that holds something stays, and so do those above it.
        fs::write(main.join("artifacts/note.md"), "theirs\n").unwrap();
        aside.put_back();
        assert!(!swept(&main, FOLDER).exists());
        assert!(main.join("artifacts/note.md").exists());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn what_was_set_aside_goes_back_and_the_folders_made_for_it_go() {
        let (base, main, tree) = scratch("back");
        repo(&tree, FOLDER);
        fs::write(tree.join("artifacts/a.png"), "a\n").unwrap();
        fs::create_dir_all(tree.join("artifacts/shots")).unwrap();
        fs::write(tree.join("artifacts/shots/b.png"), "b\n").unwrap();
        let aside = Aside::take(&main, FOLDER, &tree).unwrap().unwrap();
        assert_eq!(aside.to, swept(&main, FOLDER).join("tree"));
        assert_eq!(fs::read_to_string(aside.to.join("a.png")).unwrap(), "a\n");
        assert_eq!(
            fs::read_to_string(aside.to.join("shots/b.png")).unwrap(),
            "b\n"
        );
        assert!(!tree.join("artifacts/a.png").exists());
        assert!(
            tree.join("artifacts/kept.md").exists(),
            "a tracked file stays"
        );
        aside.put_back();
        assert_eq!(
            fs::read_to_string(tree.join("artifacts/a.png")).unwrap(),
            "a\n"
        );
        assert_eq!(
            fs::read_to_string(tree.join("artifacts/shots/b.png")).unwrap(),
            "b\n"
        );
        assert!(!main.exists(), "the folders made for them go");
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_projects_own_folder_is_the_one_set_aside() {
        let (base, main, tree) = scratch("own");
        repo(&tree, "out/shots");
        fs::write(tree.join("out/shots/a.png"), "a\n").unwrap();
        fs::create_dir_all(tree.join("artifacts")).unwrap();
        fs::write(tree.join("artifacts/other.png"), "not evidence\n").unwrap();
        assert_eq!(
            entries(&tree, "out/shots"),
            Some(vec!["out/shots/a.png".to_string()])
        );
        let aside = Aside::take(&main, "out/shots", &tree).unwrap().unwrap();
        assert_eq!(aside.to, main.join("out/shots/swept/tree"));
        assert_eq!(fs::read_to_string(aside.to.join("a.png")).unwrap(), "a\n");
        assert!(tree.join("artifacts/other.png").exists());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn evidence_on_another_disk_says_so() {
        let error = io::Error::from_raw_os_error(libc::EXDEV);
        assert_eq!(
            refused(
                Path::new("/wt/artifacts"),
                Path::new("/main/artifacts/swept/wt"),
                &error
            ),
            "/wt/artifacts is on another disk than /main/artifacts/swept/wt, and the sweep sets \
             evidence aside by moving it, never by copying it"
        );
        // A folder not there yet is judged by the nearest one above it: `/` and `/dev` are two.
        assert_eq!(movable(Path::new("/"), FOLDER, Path::new("/")), Ok(()));
        assert_eq!(
            movable(Path::new("/"), FOLDER, Path::new("/dev/no-such-tree")),
            Err(
                "its artifacts/ is on another disk than /artifacts/swept, and the sweep sets \
                 evidence aside by moving it, never by copying it"
                    .into()
            )
        );
    }

    #[test]
    fn an_entry_goes_below_the_folder_it_is_set_aside_in() {
        let to = Path::new("/main/artifacts/swept/wt");
        assert_eq!(target(to, FOLDER, "artifacts/"), to);
        assert_eq!(target(to, FOLDER, "artifacts"), to);
        assert_eq!(
            target(to, FOLDER, "artifacts/solar-swift/"),
            to.join("solar-swift")
        );
        assert_eq!(target(to, FOLDER, "artifacts/a/b.png"), to.join("a/b.png"));
        assert_eq!(target(to, "out/shots", "out/shots/a.png"), to.join("a.png"));
    }

    fn put(tree: &Path, path: &str, bytes: &[u8]) {
        let at = tree.join(path);
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        fs::write(at, bytes).unwrap();
    }

    const TAG: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55\n";

    #[test]
    fn build_output_is_never_set_aside_and_what_is_around_it_is() {
        let (base, main, tree) = scratch("build");
        repo(&tree, FOLDER);
        // A review's picture beside a Cargo build folder, and a proof's receipt beside the program it
        // built; a fixture's build folder alone; a note.
        put(&tree, "artifacts/review/shot.png", b"\x89PNG\r\n\x1a\nshot");
        put(&tree, "artifacts/review/target/CACHEDIR.TAG", TAG);
        put(&tree, "artifacts/review/target/debug/app", b"\x7fELFapp");
        put(&tree, "artifacts/review/proof/receipt.json", b"{}");
        put(
            &tree,
            "artifacts/review/proof/symphony-server",
            b"\xcf\xfa\xed\xfe\x0c\0\0\x01server",
        );
        put(&tree, "artifacts/fixture/CACHEDIR.TAG", TAG);
        put(
            &tree,
            "artifacts/fixture/debug/libwork.rlib",
            b"!<arch>\nobj",
        );
        put(&tree, "artifacts/notes.txt", b"ok");
        let kept = entries(&tree, FOLDER).unwrap();
        assert_eq!(
            kept,
            [
                "artifacts/notes.txt",
                "artifacts/review/proof/receipt.json",
                "artifacts/review/shot.png"
            ]
        );
        let held: u64 = kept
            .iter()
            .map(|entry| super::super::size::bytes(&tree.join(entry)))
            .sum();
        assert_eq!(
            bytes(&tree, &kept),
            held,
            "what is set aside, the build not counted"
        );
        assert!(held < super::super::size::bytes(&tree.join("artifacts")));

        let aside = Aside::take(&main, FOLDER, &tree).unwrap().unwrap();
        let to = main.join("artifacts/swept/tree");
        assert_eq!(aside.to, to);
        assert_eq!(
            fs::read(to.join("review/shot.png")).unwrap(),
            b"\x89PNG\r\n\x1a\nshot"
        );
        assert_eq!(
            fs::read(to.join("review/proof/receipt.json")).unwrap(),
            b"{}"
        );
        assert_eq!(fs::read(to.join("notes.txt")).unwrap(), b"ok");
        for build in ["review/target", "review/proof/symphony-server", "fixture"] {
            assert!(!to.join(build).exists(), "{build} is not set aside");
            assert!(
                tree.join("artifacts").join(build).exists(),
                "{build} stays to go with it"
            );
        }
        // Put back, everything is where it was.
        aside.put_back();
        assert!(tree.join("artifacts/review/shot.png").is_file());
        assert!(tree.join("artifacts/review/proof/receipt.json").is_file());
        assert!(!main.join("artifacts").exists(), "{}", main.display());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn compiled_code_is_known_by_its_first_bytes_and_a_cache_by_its_tag() {
        let (base, _, tree) = scratch("built");
        let is = |path: &str, bytes: &[u8]| {
            put(&tree, path, bytes);
            let at = tree.join(path);
            built(&at, &fs::symlink_metadata(&at).unwrap())
        };
        for (name, head) in [
            ("elf", &b"\x7fELF\x02\x01"[..]),
            ("macho32", b"\xfe\xed\xfa\xce"),
            ("macho64", b"\xfe\xed\xfa\xcf"),
            ("macho32le", b"\xce\xfa\xed\xfe"),
            ("macho64le", b"\xcf\xfa\xed\xfe"),
            ("fat", b"\xca\xfe\xba\xbe"),
            ("wasm", b"\0asm\x01\0\0\0"),
            ("rlib", b"!<arch>\n"),
        ] {
            assert!(is(name, head), "{name}");
        }
        for (name, head) in [
            ("png", &b"\x89PNG\r\n\x1a\n"[..]),
            ("short", b"\x7fE"),
            ("empty", b""),
            ("text", b"!<arch> but text"),
        ] {
            assert!(!is(name, head), "{name}");
        }
        fs::create_dir_all(tree.join("plain")).unwrap();
        let dir = |name: &str| {
            built(
                &tree.join(name),
                &fs::symlink_metadata(tree.join(name)).unwrap(),
            )
        };
        assert!(!dir("plain"), "a folder with no tag");
        put(&tree, "cache/CACHEDIR.TAG", TAG);
        assert!(dir("cache"));
        // A tag that is a link is no tag, and a link is never build output itself.
        fs::create_dir_all(tree.join("linked")).unwrap();
        std::os::unix::fs::symlink(
            tree.join("cache/CACHEDIR.TAG"),
            tree.join("linked/CACHEDIR.TAG"),
        )
        .unwrap();
        assert!(!dir("linked"));
        std::os::unix::fs::symlink(tree.join("elf"), tree.join("to-elf")).unwrap();
        assert!(!dir("to-elf"));
        fs::remove_dir_all(&base).unwrap();
    }
}
