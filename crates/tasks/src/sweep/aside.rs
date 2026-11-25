//! A finished worktree's evidence, set aside before the worktree goes. What git ignores under its
//! artifacts folder (the project's `[project] artifacts`, `artifacts/` by default: a task's pictures
//! its thread names, a test's leftovers, a fixture repository) is moved into the main checkout's
//! artifacts folder, in [`SWEPT`] under the worktree's folder name, so removing the worktree loses
//! none of it; a removal git then refuses moves it back. Its build output is never set aside (a
//! folder tagged as a cache by the convention's signature, as Cargo tags its build folders, or
//! compiled code): it goes with the worktree. An entry there the sweep cannot read keeps the
//! worktree, saying why, as what it could not read would otherwise go with it. A move is a rename
//! on one disk to a name nothing holds: a worktree whose evidence is on another disk than the main
//! checkout keeps it, and stays. Nothing removes what was set aside ([`super::kept`]).

use super::git;
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::io::Read as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
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

/// What of a worktree's artifacts folder is set aside, as one look listed it ([`list`]), and what
/// that look read of each file, so a look after it opens only the files new or changed since.
#[derive(Debug, Default)]
pub struct Listed {
    /// Each entry, a path of the worktree, `/`-ended for a folder.
    pub entries: Vec<String>,
    /// What the look read of each file.
    pub seen: Seen,
}

/// What a look read of each file's first bytes, by its path, beside the file as it was then
/// ([`Stamp`]): a file still the same is not opened again.
#[derive(Debug, Default, Clone)]
pub struct Seen(HashMap<PathBuf, (Stamp, Head)>);

/// A file as a look can tell it changed: where it is, its length, and when its content and its
/// record last changed (a record change the file's owner cannot set back).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    device: u64,
    inode: u64,
    len: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

impl Stamp {
    fn of(meta: &fs::Metadata) -> Stamp {
        Stamp {
            device: meta.dev(),
            inode: meta.ino(),
            len: meta.size(),
            modified: (meta.mtime(), meta.mtime_nsec()),
            changed: (meta.ctime(), meta.ctime_nsec()),
        }
    }
}

/// What a file's first bytes say it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Head {
    /// Compiled code: an executable or a library (Mach-O, ELF or WebAssembly), or an archive of
    /// objects, as an `.rlib` is.
    Compiled,
    /// A cache tag's: the convention's [`SIGNATURE`] first.
    Tag,
    /// Anything else, a file that cannot be opened among them.
    Other,
}

/// The file a folder tagged as a cache holds by the Cache Directory Tagging convention
/// (<https://bford.info/cachedir/>), which Cargo writes in each build folder it makes.
const CACHE_TAG: &str = "CACHEDIR.TAG";

/// The 43 bytes a [`CACHE_TAG`] begins with by the convention: a file of that name without them
/// tags nothing, and its folder is read as any other.
const SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";

/// The first bytes of compiled code: ELF, the four orders of Mach-O, a fat Mach-O, WebAssembly and
/// an archive of objects.
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

/// What the regular file `path` is by its first bytes. Opened without following a link or waiting
/// on a pipe, either of which may have taken its place since it was looked at; one that cannot be
/// opened or read is [`Head::Other`], so it is set aside.
fn head_of(path: &Path) -> Head {
    let mut head = Vec::with_capacity(SIGNATURE.len());
    let read = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .and_then(|file| file.take(SIGNATURE.len() as u64).read_to_end(&mut head));
    if read.is_err() {
        Head::Other
    } else if head.starts_with(SIGNATURE) {
        Head::Tag
    } else if COMPILED.iter().any(|magic| head.starts_with(magic)) {
        Head::Compiled
    } else {
        Head::Other
    }
}

/// How a look lists a folder: the names of what it holds, each entry the listing could not read an
/// error.
type Names<'a> = &'a dyn Fn(&Path) -> io::Result<Vec<io::Result<OsString>>>;

/// The names of what folder `dir` holds, as the file system lists them.
fn names_in(dir: &Path) -> io::Result<Vec<io::Result<OsString>>> {
    Ok(fs::read_dir(dir)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect())
}

/// What of the artifacts folder `folder` in worktree `tree` is set aside: what git ignores there,
/// less its build output ([`Look::walk`]), which goes with the worktree; a folder holding some is
/// set aside entry by entry around it. `seen` is what a look before read, the files still the same
/// not opened again. Refused, saying why, when git cannot list them or an entry cannot be read. A
/// worktree with no such folder has nothing to list, and git is not asked.
pub fn list(tree: &Path, folder: &str, seen: Seen) -> Result<Listed, String> {
    list_with(tree, folder, seen, &names_in)
}

fn list_with(tree: &Path, folder: &str, seen: Seen, names: Names<'_>) -> Result<Listed, String> {
    if fs::symlink_metadata(tree.join(folder)).is_err() {
        return Ok(Listed {
            entries: Vec::new(),
            seen,
        });
    }
    let below = format!("{folder}/");
    let unlisted = || format!("git cannot list what its {folder}/ holds");
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
    )
    .ok_or_else(unlisted)?;
    let text = String::from_utf8(done.stdout).map_err(|_| unlisted())?;
    let mut look = Look { seen, names };
    let mut entries = Vec::new();
    for name in text
        .split('\0')
        .filter_map(|record| record.strip_prefix("!! "))
        .filter(|name| name.starts_with(&below) || *name == folder)
    {
        let name = name.trim_end_matches('/');
        look.walk(&tree.join(name), name, &mut entries)?;
    }
    Ok(Listed {
        entries,
        seen: look.seen,
    })
}

/// One look at a worktree's artifacts folder: what it read of each file, and how it lists a folder.
struct Look<'a> {
    seen: Seen,
    names: Names<'a>,
}

impl Look<'_> {
    /// Add to `kept` what of `entry` (`path`, named as a path of the worktree) is set aside, `/`-ended
    /// for a folder, and say whether build output was left out of it: nothing is kept of build
    /// output (a regular file of [`Head::Compiled`], a folder [`Look::tagged`]), an entry that holds
    /// none is kept whole, and a folder that holds some is kept as what is kept of each of its
    /// entries, whole when one of their names is not UTF-8. A link is kept as itself, and an entry
    /// gone since it was listed is nothing to keep. One that cannot be read (its record, a folder's
    /// listing or an entry of it) refuses the whole look, saying why: what it did not read would go
    /// with the worktree.
    fn walk(&mut self, path: &Path, entry: &str, kept: &mut Vec<String>) -> Result<bool, String> {
        let unread = |error: io::Error| format!("{} cannot be read: {error}", path.display());
        let meta = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(unread(error)),
        };
        if !meta.is_dir() {
            if meta.is_file() && self.head(path, &meta) == Head::Compiled {
                return Ok(true);
            }
            kept.push(entry.to_string());
            return Ok(false);
        }
        if self.tagged(path) {
            return Ok(true);
        }
        let mut names = Vec::new();
        let mut unnamed = false;
        for name in (self.names)(path).map_err(unread)? {
            match name.map_err(unread)?.into_string() {
                Ok(name) => names.push(name),
                Err(_) => unnamed = true,
            }
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "a busy board path keeps std's in-place sort (crate::order): the sweep"
        )]
        names.sort_unstable();
        let start = kept.len();
        let mut left_out = false;
        for name in names {
            left_out |= self.walk(&path.join(&name), &format!("{entry}/{name}"), kept)?;
        }
        if left_out && !unnamed {
            return Ok(true);
        }
        kept.truncate(start);
        kept.push(format!("{entry}/"));
        Ok(false)
    }

    /// What the regular file `path`, as `meta` describes it, is by its first bytes: what the look
    /// before read when the file is the same since, else read now.
    fn head(&mut self, path: &Path, meta: &fs::Metadata) -> Head {
        let stamp = Stamp::of(meta);
        if let Some((was, head)) = self.seen.0.get(path)
            && *was == stamp
        {
            return *head;
        }
        let head = head_of(path);
        self.seen.0.insert(path.to_path_buf(), (stamp, head));
        head
    }

    /// Whether folder `dir` is tagged as a cache: its [`CACHE_TAG`] a regular file, not a link,
    /// that begins with the convention's [`SIGNATURE`].
    fn tagged(&mut self, dir: &Path) -> bool {
        let tag = dir.join(CACHE_TAG);
        fs::symlink_metadata(&tag)
            .is_ok_and(|meta| meta.is_file() && self.head(&tag, &meta) == Head::Tag)
    }
}

/// What the entries of `tree` that [`list`] lists hold on disk.
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
    if error.kind() == io::ErrorKind::AlreadyExists {
        return format!(
            "{} cannot be moved to {}: something is there already, and the sweep never replaces it",
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

/// Rename `from` to `to` while nothing is there: refused (`AlreadyExists`) when something is, even
/// something put there a moment before, in the one call that renames, on macOS (`renamex_np` with
/// `RENAME_EXCL`) and Linux (`renameat2` with `RENAME_NOREPLACE`); a rename alone replaces a file
/// or an empty folder there. A volume that cannot refuse so in the call (`ENOTSUP`, `EINVAL`) has
/// `to` looked for first.
fn rename_new(from: &Path, to: &Path) -> io::Result<()> {
    or_looked(from, to, rename_exclusive(from, to))
}

/// What the exclusive rename of `from` to `to` came to (`tried`), or on a volume that cannot make
/// one, `to` looked for and then a rename.
fn or_looked(from: &Path, to: &Path, tried: io::Result<()>) -> io::Result<()> {
    match tried {
        Err(error) if matches!(error.raw_os_error(), Some(libc::ENOTSUP | libc::EINVAL)) => {
            if fs::symlink_metadata(to).is_ok() {
                return Err(io::Error::from(io::ErrorKind::AlreadyExists));
            }
            fs::rename(from, to)
        }
        done => done,
    }
}

/// The paths `from` and `to` as the C calls take them.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn c_paths(from: &Path, to: &Path) -> io::Result<(std::ffi::CString, std::ffi::CString)> {
    use std::os::unix::ffi::OsStrExt as _;
    Ok((
        std::ffi::CString::new(from.as_os_str().as_bytes())?,
        std::ffi::CString::new(to.as_os_str().as_bytes())?,
    ))
}

#[cfg(target_os = "macos")]
fn rename_exclusive(from: &Path, to: &Path) -> io::Result<()> {
    let (from, to) = c_paths(from, to)?;
    // SAFETY: both are live NUL-terminated paths, which the call only reads; nothing outlives it.
    let done = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    if done == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn rename_exclusive(from: &Path, to: &Path) -> io::Result<()> {
    let (from, to) = c_paths(from, to)?;
    // SAFETY: both are live NUL-terminated paths, relative to the current folder for AT_FDCWD,
    // which the call only reads; nothing outlives it.
    let done = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if done == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn rename_exclusive(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::from_raw_os_error(libc::ENOTSUP))
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
    /// checkout `main`, as a look lists it now, `seen` what a look before read; none when there is
    /// nothing to move. Refused, with every entry put back, when git cannot list them, an entry
    /// cannot be read, they are on another disk, or one cannot be moved or would land on something
    /// there already: the worktree then stays.
    pub fn take(
        main: &Path,
        folder: &str,
        tree: &Path,
        seen: Seen,
    ) -> Result<Option<Aside>, String> {
        Aside::take_with(main, folder, tree, seen, &names_in)
    }

    fn take_with(
        main: &Path,
        folder: &str,
        tree: &Path,
        seen: Seen,
        names: Names<'_>,
    ) -> Result<Option<Aside>, String> {
        let entries = list_with(tree, folder, seen, names)?.entries;
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
    /// nothing there is ever replaced ([`rename_new`]).
    fn move_all(&mut self, tree: &Path, folder: &str, entries: &[String]) -> Result<(), String> {
        for entry in entries {
            let from = tree.join(entry.trim_end_matches('/'));
            let into = target(&self.to, folder, entry);
            let moved = into
                .parent()
                .map_or(Ok(()), |parent| self.make(parent))
                .and_then(|()| rename_new(&from, &into));
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
    /// cannot go back, something there now among the causes, stays where it went, kept all the same:
    /// neither is replaced. The folders made for them go, the deepest first, each only when empty.
    pub fn put_back(mut self) {
        self.undo();
    }

    fn undo(&mut self) {
        for (from, into) in self.moved.drain(..).rev() {
            let _ = rename_new(&into, &from);
        }
        for dir in self.made.drain(..).rev() {
            let _ = fs::remove_dir(dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::children::Child as _;

    const FOLDER: &str = "artifacts";

    /// What a first look at `folder` of `tree` would set aside.
    fn entries(tree: &Path, folder: &str) -> Result<Vec<String>, String> {
        list(tree, folder, Seen::default()).map(|listed| listed.entries)
    }

    #[test]
    fn a_name_taken_gets_the_next_number() {
        let main =
            std::env::temp_dir().join(format!("symphony-sweep-aside-{}", std::process::id()));
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
            let done = std::process::Command::of("git")
                .args(args)
                .current_dir(root)
                .output()
                .unwrap();
            assert!(done.status.success(), "git {args:?}: {done:?}");
        }
    }

    fn scratch(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!(
            "symphony-sweep-aside-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        (base.clone(), base.join("main"), base.join("tree"))
    }

    #[test]
    fn nothing_ignored_under_artifacts_sets_nothing_aside() {
        let (base, main, tree) = scratch("none");
        repo(&tree, FOLDER);
        assert_eq!(entries(&tree, FOLDER), Ok(vec![]));
        assert!(
            Aside::take(&main, FOLDER, &tree, Seen::default())
                .unwrap()
                .is_none()
        );
        assert!(!main.exists(), "nothing was made in the main checkout");
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_folder_git_cannot_list_refuses_the_set_aside() {
        let (base, main, tree) = scratch("nogit");
        fs::create_dir_all(tree.join("artifacts")).unwrap();
        fs::write(tree.join("artifacts/a.png"), "a\n").unwrap();
        assert_eq!(
            entries(&tree, FOLDER),
            Err("git cannot list what its artifacts/ holds".into())
        );
        assert_eq!(
            Aside::take(&main, FOLDER, &tree, Seen::default()).unwrap_err(),
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
            Ok(vec![
                "artifacts/a.png".to_string(),
                "artifacts/b.png".to_string()
            ])
        );
        // The main checkout's swept folder is a file: no folder can be made below it.
        fs::create_dir_all(main.join("artifacts")).unwrap();
        fs::write(swept(&main, FOLDER), "not a folder\n").unwrap();
        let refused = Aside::take(&main, FOLDER, &tree, Seen::default()).unwrap_err();
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
                "{} cannot be moved to {}: something is there already, and the sweep never \
                 replaces it",
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
        let aside = Aside::take(&main, FOLDER, &tree, Seen::default())
            .unwrap()
            .unwrap();
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
            Ok(vec!["out/shots/a.png".to_string()])
        );
        let aside = Aside::take(&main, "out/shots", &tree, Seen::default())
            .unwrap()
            .unwrap();
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

        let aside = Aside::take(&main, FOLDER, &tree, Seen::default())
            .unwrap()
            .unwrap();
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

    /// A look with nothing read yet, listing folders as the file system does.
    fn look() -> Look<'static> {
        Look {
            seen: Seen::default(),
            names: &names_in,
        }
    }

    /// What one look keeps of `entry` of `tree`, and whether it left build output out.
    fn walked(tree: &Path, entry: &str) -> (Vec<String>, bool) {
        let mut kept = Vec::new();
        let left_out = look().walk(&tree.join(entry), entry, &mut kept).unwrap();
        (kept, left_out)
    }

    #[test]
    fn compiled_code_is_known_by_its_first_bytes_and_a_cache_by_its_signed_tag() {
        let (base, _, tree) = scratch("built");
        let is = |path: &str, bytes: &[u8]| {
            put(&tree, path, bytes);
            walked(&tree, path) == (vec![], true)
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
            assert_eq!(head_of(&tree.join(name)), Head::Compiled, "{name}");
        }
        for (name, head) in [
            ("png", &b"\x89PNG\r\n\x1a\n"[..]),
            ("short", b"\x7fE"),
            ("empty", b""),
            ("text", b"!<arch> but text"),
            ("signature", TAG),
        ] {
            assert!(!is(name, head), "{name}");
            assert_eq!(walked(&tree, name), (vec![name.to_string()], false));
        }
        assert_eq!(head_of(&tree.join("signature")), Head::Tag);
        assert_eq!(head_of(&tree.join("missing")), Head::Other);
        let tagged = |name: &str| {
            fs::create_dir_all(tree.join(name)).unwrap();
            look().tagged(&tree.join(name))
        };
        assert!(!tagged("plain"), "a folder with no tag");
        put(&tree, "cache/CACHEDIR.TAG", TAG);
        assert!(tagged("cache"));
        // The signature alone, with no line after it, as the convention allows.
        put(&tree, "bare/CACHEDIR.TAG", &TAG[..SIGNATURE.len()]);
        assert!(tagged("bare"));
        // A file of that name without the signature tags nothing: an empty one, another text, the
        // signature cut short, or not at its start, or in another case.
        for (name, text) in [
            ("empty", &b""[..]),
            ("other", b"made by hand\n"),
            ("short", &TAG[..SIGNATURE.len() - 1]),
            ("later", b"\nSignature: 8a477f597d28d172789f06886806bc55\n"),
            ("case", b"signature: 8a477f597d28d172789f06886806bc55\n"),
        ] {
            let folder = format!("unsigned-{name}");
            put(&tree, &format!("{folder}/CACHEDIR.TAG"), text);
            assert!(!tagged(&folder), "{name}");
        }
        // A tag that is a link is no tag, nor a folder of that name, and a link is never build
        // output itself.
        fs::create_dir_all(tree.join("linked")).unwrap();
        std::os::unix::fs::symlink(
            tree.join("cache/CACHEDIR.TAG"),
            tree.join("linked/CACHEDIR.TAG"),
        )
        .unwrap();
        assert!(!tagged("linked"));
        fs::create_dir_all(tree.join("foldered/CACHEDIR.TAG")).unwrap();
        assert!(!tagged("foldered"));
        std::os::unix::fs::symlink(tree.join("elf"), tree.join("to-elf")).unwrap();
        assert_eq!(walked(&tree, "to-elf"), (vec!["to-elf".to_string()], false));
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_folder_with_an_unsigned_cache_tag_is_set_aside_with_what_it_holds() {
        let (base, main, tree) = scratch("unsigned");
        repo(&tree, FOLDER);
        // A review's pictures beside a CACHEDIR.TAG a tool wrote without the signature, and a
        // Cargo build folder, signed, beside them.
        put(
            &tree,
            "artifacts/review/CACHEDIR.TAG",
            b"a cache, says a stray tool\n",
        );
        put(&tree, "artifacts/review/shot.png", b"\x89PNG\r\n\x1a\nshot");
        put(
            &tree,
            "artifacts/review/after/shot.png",
            b"\x89PNG\r\n\x1a\nafter",
        );
        put(&tree, "artifacts/review/target/CACHEDIR.TAG", TAG);
        put(&tree, "artifacts/review/target/debug/app", b"\x7fELFapp");
        assert_eq!(
            entries(&tree, FOLDER).unwrap(),
            [
                "artifacts/review/CACHEDIR.TAG",
                "artifacts/review/after/",
                "artifacts/review/shot.png"
            ]
        );
        let aside = Aside::take(&main, FOLDER, &tree, Seen::default())
            .unwrap()
            .unwrap();
        for (file, bytes) in [
            ("review/shot.png", &b"\x89PNG\r\n\x1a\nshot"[..]),
            ("review/after/shot.png", b"\x89PNG\r\n\x1a\nafter"),
            ("review/CACHEDIR.TAG", b"a cache, says a stray tool\n"),
        ] {
            assert_eq!(fs::read(aside.to.join(file)).unwrap(), bytes, "{file}");
        }
        assert!(!aside.to.join("review/target").exists());
        assert!(tree.join("artifacts/review/target/debug/app").is_file());
        fs::remove_dir_all(&base).unwrap();
    }

    /// A listing of folders as the file system's, but for `broken`, whose listing is `how`: refused,
    /// or broken off after what it read with an error in place of the rest.
    fn breaking(
        broken: PathBuf,
        how: &'static str,
    ) -> impl Fn(&Path) -> io::Result<Vec<io::Result<OsString>>> {
        move |dir: &Path| {
            if dir != broken.as_path() {
                return names_in(dir);
            }
            if how == "refused" {
                return Err(io::Error::from_raw_os_error(libc::EACCES));
            }
            let mut names = names_in(dir)?;
            names.push(Err(io::Error::from_raw_os_error(libc::EIO)));
            Ok(names)
        }
    }

    #[test]
    fn an_entry_that_cannot_be_read_keeps_the_worktree_and_says_why() {
        let (base, main, tree) = scratch("unread");
        repo(&tree, FOLDER);
        put(&tree, "artifacts/notes.txt", b"ok");
        // A folder holding build output beside a picture, whose listing breaks off, and a folder
        // holding none whose listing is refused: neither can say what it holds.
        put(&tree, "artifacts/review/shot.png", b"\x89PNG\r\n\x1a\nshot");
        put(&tree, "artifacts/review/target/CACHEDIR.TAG", TAG);
        put(&tree, "artifacts/review/target/debug/app", b"\x7fELFapp");
        put(&tree, "artifacts/shots/a.png", b"\x89PNG\r\n\x1a\na");
        for (folder, how, error) in [
            ("review", "broken off", libc::EIO),
            ("shots", "refused", libc::EACCES),
        ] {
            let broken = tree.join("artifacts").join(folder);
            let names = breaking(broken.clone(), how);
            let why = format!(
                "{} cannot be read: {}",
                broken.display(),
                io::Error::from_raw_os_error(error)
            );
            assert_eq!(
                list_with(&tree, FOLDER, Seen::default(), &names).unwrap_err(),
                why,
                "{how}"
            );
            assert_eq!(
                Aside::take_with(&main, FOLDER, &tree, Seen::default(), &names).unwrap_err(),
                why,
                "{how}"
            );
            assert!(!main.exists(), "{how}: nothing was moved");
        }
        for file in ["notes.txt", "review/shot.png", "shots/a.png"] {
            assert!(tree.join("artifacts").join(file).is_file(), "{file}");
        }
        // Read whole, the same folder sets aside its picture and leaves its build out.
        assert_eq!(
            entries(&tree, FOLDER).unwrap(),
            [
                "artifacts/notes.txt",
                "artifacts/review/shot.png",
                "artifacts/shots/"
            ]
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn an_entry_gone_since_it_was_listed_is_nothing_to_keep_and_one_unreadable_is_refused() {
        let (base, _, tree) = scratch("gone");
        let mut kept = Vec::new();
        assert_eq!(
            look().walk(
                &tree.join("artifacts/gone.png"),
                "artifacts/gone.png",
                &mut kept
            ),
            Ok(false)
        );
        assert!(kept.is_empty());
        // A folder that may be listed but not searched: its entries' records cannot be read.
        put(&tree, "artifacts/locked/a.png", b"a");
        let locked = tree.join("artifacts/locked");
        fs::set_permissions(&locked, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
        let walked = look().walk(&locked, "artifacts/locked", &mut kept);
        fs::set_permissions(&locked, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let why = walked.unwrap_err();
        assert!(
            why.starts_with(&format!(
                "{} cannot be read: ",
                locked.join("a.png").display()
            )),
            "{why}"
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_later_look_opens_only_the_files_new_or_changed_since() {
        let (base, _, tree) = scratch("seen");
        repo(&tree, FOLDER);
        put(&tree, "artifacts/review/shot.png", b"\x89PNG\r\n\x1a\nshot");
        put(&tree, "artifacts/review/server", b"\x7fELFserver");
        put(&tree, "artifacts/review/target/CACHEDIR.TAG", TAG);
        let first = list(&tree, FOLDER, Seen::default()).unwrap();
        assert_eq!(first.entries, ["artifacts/review/shot.png"]);
        let review = tree.join("artifacts/review");
        assert_eq!(
            first
                .seen
                .0
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>(),
            [
                review.join("server"),
                review.join("shot.png"),
                review.join("target/CACHEDIR.TAG")
            ]
            .into(),
            "every file read once, the tag among them, and nothing below a tagged folder"
        );
        // What the first look read is taken as it read it while the file is the same: a record
        // that says otherwise of a file unchanged is believed, which shows it was not read again.
        let mut seen = first.seen.clone();
        let stamp = seen.0[&review.join("shot.png")].0;
        seen.0
            .insert(review.join("shot.png"), (stamp, Head::Compiled));
        assert_eq!(
            list(&tree, FOLDER, seen.clone()).unwrap().entries,
            Vec::<String>::new()
        );
        // A file changed since is read again, and so is one new since.
        put(
            &tree,
            "artifacts/review/shot.png",
            b"\x89PNG\r\n\x1a\nshot, again",
        );
        put(&tree, "artifacts/review/late.png", b"\x89PNG\r\n\x1a\nlate");
        let again = list(&tree, FOLDER, seen).unwrap();
        assert_eq!(
            again.entries,
            ["artifacts/review/late.png", "artifacts/review/shot.png"]
        );
        assert_eq!(again.seen.0[&review.join("shot.png")].1, Head::Other);
        assert_eq!(again.seen.0[&review.join("late.png")].1, Head::Other);
        // A file replaced by another of the same length, at once, is another file all the same.
        let mut seen = again.seen.clone();
        let stamp = seen.0[&review.join("late.png")].0;
        seen.0
            .insert(review.join("late.png"), (stamp, Head::Compiled));
        let replacement = tree.join("artifacts/late.png.new");
        fs::write(&replacement, b"\x89PNG\r\n\x1a\nLATE").unwrap();
        fs::rename(&replacement, review.join("late.png")).unwrap();
        assert!(
            list(&tree, FOLDER, seen)
                .unwrap()
                .entries
                .contains(&"artifacts/review/late.png".to_string())
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_rename_never_replaces_what_is_there() {
        let (base, _, _) = scratch("rename");
        fs::create_dir_all(&base).unwrap();
        let at = |name: &str| base.join(name);
        fs::write(at("a.png"), "a\n").unwrap();
        fs::write(at("b.png"), "b\n").unwrap();
        fs::create_dir_all(at("shots/one")).unwrap();
        fs::create_dir_all(at("empty")).unwrap();
        // A file onto a file, and a folder onto an empty folder, which a rename alone replaces.
        for (from, to) in [("a.png", "b.png"), ("shots", "empty")] {
            let refused = rename_new(&at(from), &at(to)).unwrap_err();
            assert_eq!(
                refused.kind(),
                io::ErrorKind::AlreadyExists,
                "{from}: {refused}"
            );
        }
        assert_eq!(fs::read_to_string(at("a.png")).unwrap(), "a\n");
        assert_eq!(fs::read_to_string(at("b.png")).unwrap(), "b\n");
        assert!(at("shots/one").is_dir());
        assert_eq!(fs::read_dir(at("empty")).unwrap().count(), 0);
        // To a name nothing holds, it renames; from one gone, it says so.
        rename_new(&at("a.png"), &at("c.png")).unwrap();
        assert_eq!(fs::read_to_string(at("c.png")).unwrap(), "a\n");
        assert!(!at("a.png").exists());
        assert_eq!(
            rename_new(&at("a.png"), &at("d.png")).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_volume_that_cannot_rename_exclusively_has_the_name_looked_for_first() {
        let (base, _, _) = scratch("looked");
        fs::create_dir_all(&base).unwrap();
        let at = |name: &str| base.join(name);
        fs::write(at("a.png"), "a\n").unwrap();
        fs::write(at("b.png"), "b\n").unwrap();
        for unable in [libc::ENOTSUP, libc::EINVAL] {
            let tried = Err(io::Error::from_raw_os_error(unable));
            let refused = or_looked(&at("a.png"), &at("b.png"), tried).unwrap_err();
            assert_eq!(refused.kind(), io::ErrorKind::AlreadyExists, "{unable}");
            assert_eq!(fs::read_to_string(at("b.png")).unwrap(), "b\n");
        }
        let tried = Err(io::Error::from_raw_os_error(libc::ENOTSUP));
        or_looked(&at("a.png"), &at("c.png"), tried).unwrap();
        assert_eq!(fs::read_to_string(at("c.png")).unwrap(), "a\n");
        // Any other answer of the exclusive rename is its own: nothing is tried after it.
        let failed = or_looked(
            &at("c.png"),
            &at("d.png"),
            Err(io::Error::from_raw_os_error(libc::EIO)),
        );
        assert_eq!(failed.unwrap_err().raw_os_error(), Some(libc::EIO));
        assert!(or_looked(&at("c.png"), &at("d.png"), Ok(())).is_ok());
        assert!(at("c.png").is_file() && !at("d.png").exists());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_name_that_is_not_utf8_keeps_its_folder_whole_and_no_folder_lists_nothing() {
        use std::os::unix::ffi::OsStringExt as _;
        let (base, _, tree) = scratch("unnamed");
        assert_eq!(entries(&tree, FOLDER), Ok(vec![]), "no artifacts folder");
        repo(&tree, FOLDER);
        put(&tree, "artifacts/review/shot.png", b"\x89PNG\r\n\x1a\nshot");
        put(&tree, "artifacts/review/server", b"\x7fELFserver");
        // A name no String holds, which this file system would not make: listed by a stand-in.
        let review = tree.join("artifacts/review");
        let names = |dir: &Path| {
            let mut names = names_in(dir)?;
            if dir == review.as_path() {
                names.push(Ok(OsString::from_vec(b"shot-\xff.png".to_vec())));
            }
            Ok(names)
        };
        assert_eq!(
            list_with(&tree, FOLDER, Seen::default(), &names)
                .unwrap()
                .entries,
            ["artifacts/review/"],
            "kept whole, its build output with it, as its parts cannot all be named"
        );
        assert_eq!(
            entries(&tree, FOLDER).unwrap(),
            ["artifacts/review/shot.png"]
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn what_cannot_go_back_stays_where_it_went() {
        let (base, main, tree) = scratch("stays");
        repo(&tree, FOLDER);
        put(&tree, "artifacts/a.png", b"a\n");
        let aside = Aside::take(&main, FOLDER, &tree, Seen::default())
            .unwrap()
            .unwrap();
        // Something new at its place in the worktree, since: neither is replaced.
        put(&tree, "artifacts/a.png", b"new\n");
        let to = aside.to.clone();
        aside.put_back();
        assert_eq!(fs::read(tree.join("artifacts/a.png")).unwrap(), b"new\n");
        assert_eq!(fs::read(to.join("a.png")).unwrap(), b"a\n");
        fs::remove_dir_all(&base).unwrap();
    }
}
