//! One machine-wide copy of each fixture script, keyed by its content. macOS assesses an
//! executable file before its first instruction, one file at a time machine-wide, and every
//! verify run checks out fresh files: under load that queue alone has held a four-line script past
//! PATIENCE. A link to a file already assessed runs at once, so every checkout links one copy,
//! made and first executed once per content on this machine.
//! Only std and libc: any fixture that links a script may include this file.

use std::fs;
use std::hash::{DefaultHasher, Hasher};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The copies' folder: under the system's temporary root, not TMPDIR, which verify makes fresh
/// for each run.
fn machine_root() -> PathBuf {
    Path::new("/tmp").join(format!("sf-fixture-scripts-{}", uid()))
}

fn uid() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

/// The machine-wide copy of `source`'s bytes, executable; made once per content.
pub fn machine_copy(source: &Path) -> PathBuf {
    copy_into(&private(&machine_root()), source)
}

/// `root`, made private to this user, or refused when another user could write into it: a
/// folder in a shared temporary root may have been made by anyone, and its copies are executed.
fn private(root: &Path) -> PathBuf {
    match fs::DirBuilder::new().mode(0o700).create(root) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => panic!("cannot make {}: {error}", root.display()),
    }
    let meta = fs::symlink_metadata(root).unwrap();
    let uid = uid();
    assert!(
        meta.is_dir() && meta.uid() == uid && meta.mode() & 0o022 == 0,
        "{} must be a folder of uid {uid} that no one else can write: {:?}, uid {}, mode {:o}",
        root.display(),
        meta.file_type(),
        meta.uid(),
        meta.mode() & 0o7777
    );
    root.to_path_buf()
}

/// The copy's name: the source's file name after a 128-bit key of its bytes, so a changed script
/// gets its own copy and never another content's.
fn name(source: &Path, bytes: &[u8]) -> String {
    let key = [0_u8, 1].map(|salt| {
        let mut hasher = DefaultHasher::new();
        hasher.write_u8(salt);
        hasher.write(bytes);
        hasher.finish()
    });
    format!(
        "{:016x}{:016x}-{}",
        key[0],
        key[1],
        source.file_name().unwrap().to_str().unwrap()
    )
}

/// Whether `path` is this user's executable regular file holding exactly `bytes`.
fn holds(path: &Path, bytes: &[u8]) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.uid() == uid() && meta.mode() & 0o777 == 0o755)
        && fs::read(path).is_ok_and(|held| held == bytes)
}

fn copy_into(root: &Path, source: &Path) -> PathBuf {
    let bytes = fs::read(source)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", source.display()));
    let path = root.join(name(source, &bytes));
    if holds(&path, &bytes) {
        return path;
    }
    let partial = write_partial(root, &path, &bytes);
    // A link publishes only a complete file, and never replaces one another run published first:
    // that copy's assessment, done or under way, is the one every link shares.
    match fs::hard_link(&partial, &path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if !holds(&path, &bytes) {
                // Damaged (cut short, rewritten or rechmodded): replaced whole, at once.
                fs::rename(&partial, &path).unwrap();
            }
        }
        Err(error) => panic!("cannot publish {}: {error}", path.display()),
    }
    match fs::remove_file(&partial) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("cannot remove {}: {error}", partial.display()),
    }
    path
}

/// A complete executable file of `bytes` beside `path`, under a name no other writer takes.
fn write_partial(root: &Path, path: &Path, bytes: &[u8]) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let partial = root.join(format!(
        ".{}.{}.{}.partial",
        path.file_name().unwrap().to_str().unwrap(),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .open(&partial)
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", partial.display()));
    file.write_all(bytes).unwrap();
    // Exactly 0755 whatever the umask: holds() requires it.
    file.set_permissions(fs::Permissions::from_mode(0o755))
        .unwrap();
    partial
}

#[cfg(test)]
#[path = "machine_copy_tests.rs"]
mod tests;
