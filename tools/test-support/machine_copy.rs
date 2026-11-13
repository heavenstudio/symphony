//! One machine-wide copy of each fixture script, keyed by its content. macOS assesses an
//! executable file before its first instruction, one file at a time machine-wide, and every
//! verify run checks out fresh files: under load that queue alone has held a four-line script past
//! its fixture's deadline. A link to a file already assessed runs at once, so every checkout
//! links one copy, made once per content on this machine and executed by each process that hands
//! it out before it does (`primed`), so no case's own clock pays a copy's first run.
//! Only std: fixture harnesses include this file through `script.rs` or directly.
//! The parent module supplies the same child-start boundary as its harness (`run_fixture`).

use std::fs;
use std::hash::{DefaultHasher, Hasher};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

/// The copies' folder: under the system's temporary root, not TMPDIR, which verify makes fresh
/// for each run. Resolved (macOS's /tmp is a link), so a copy's path is what a link to it resolves to.
fn machine_root() -> PathBuf {
    fs::canonicalize("/tmp")
        .unwrap()
        .join(format!("symphony-fixture-scripts-{}", uid()))
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// This process's effective uid. std names no `geteuid`, and a file a process creates is owned by
/// its effective uid, so the owner of one it creates and removes at once is the answer.
pub(crate) fn uid() -> u32 {
    static UID: OnceLock<u32> = OnceLock::new();
    *UID.get_or_init(|| {
        let probe = std::env::temp_dir().join(format!(
            ".symphony-fixture-uid.{}.{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&probe)
            .unwrap_or_else(|error| panic!("cannot create {}: {error}", probe.display()));
        let uid = file.metadata().unwrap().uid();
        fs::remove_file(&probe).unwrap();
        uid
    })
}

/// The machine-wide copy of `source`'s bytes, executable; made once per content.
pub fn machine_copy(source: &Path) -> PathBuf {
    copy_into(&private(&machine_root()), source)
}

/// `root`, made private to this user, or refused when another user could write into it: a
/// folder in a shared temporary root may have been made by anyone, and its copies are executed.
pub(crate) fn private(root: &Path) -> PathBuf {
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
pub(crate) fn holds(path: &Path, bytes: &[u8]) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.uid() == uid() && meta.mode() & 0o777 == 0o755)
        && fs::read(path).is_ok_and(|held| held == bytes)
}

/// The copy of `source`'s bytes in `root`, published when missing or damaged, and executed once by
/// this process before it is handed out.
pub(crate) fn copy_into(root: &Path, source: &Path) -> PathBuf {
    let path = published(root, source);
    primed(root, &path);
    path
}

/// The copy of `source`'s bytes in `root`, published when missing or damaged, not yet run.
pub(crate) fn published(root: &Path, source: &Path) -> PathBuf {
    let bytes = fs::read(source)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", source.display()));
    let path = root.join(name(source, &bytes));
    if !holds(&path, &bytes) {
        publish(root, &path, &bytes);
    }
    path
}

/// Run `copy` once, unless this process already ran the file it now is. A new copy's first
/// execution is the assessment every link waits on, so it is paid here, in setup, never inside the
/// deadline of the case that runs a stand-in. A copy another process published, possibly still in
/// its first run, or one a cleaner removed and this run published again, is run here too: a run of
/// a file already assessed costs milliseconds, and concurrent runs share one assessment. The run's
/// scratch folder goes in `root`, a folder of this user's.
pub(crate) fn primed(root: &Path, copy: &Path) {
    /// One run per file (device, inode): a thread wanting a file another thread is running waits
    /// for that run to end, and runs of different files go on side by side.
    type Runs = Mutex<Vec<((u64, u64), Arc<OnceLock<()>>)>>;
    static RUNS: Runs = Mutex::new(Vec::new());
    let meta = fs::metadata(copy).unwrap();
    let file = (meta.dev(), meta.ino());
    let run = {
        let mut runs = RUNS.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, run)) = runs.iter().find(|(known, _)| *known == file) {
            Arc::clone(run)
        } else {
            let run = Arc::new(OnceLock::new());
            runs.push((file, Arc::clone(&run)));
            run
        }
    };
    run.get_or_init(|| prime(root, copy));
}

/// Execute `copy` with no arguments, no input and a bare environment, through a link in a scratch
/// folder of its own: a script that writes beside its `$0`'s folder writes there, and the folder
/// goes after. The scripts exit at once so started (a stand-in finds no body; the lifecycle
/// commands refuse or answer nothing); what they exit with is not the point. It starts the way the
/// including crate starts its children (`run_fixture`, beside `mod fixture_script`): under its
/// spawn lock where it has one, since macOS assesses the file after the exec, and a descriptor the
/// child copied would be held through the assessment.
fn prime(root: &Path, copy: &Path) {
    let folder = prime_folder(root, NEXT.fetch_add(1, Ordering::Relaxed));
    let link = folder.join("bin").join(copy.file_name().unwrap());
    std::os::unix::fs::symlink(copy, &link).unwrap();
    let mut command = std::process::Command::new(&link);
    command
        .current_dir(&folder)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let ran = super::run_fixture(&mut command);
    fs::remove_dir_all(&folder).unwrap();
    if let Err(error) = ran {
        panic!("cannot run {} once: {error}", copy.display());
    }
}

/// An empty scratch folder for run `run` of this process, with its `bin`. This process takes each
/// run's number once, so a folder already of that name was left by a run killed mid-way under a
/// process id since reused: it is removed and made again.
pub(crate) fn prime_folder(root: &Path, run: usize) -> PathBuf {
    let folder = root.join(format!(".prime.{}.{run}", std::process::id()));
    let make = || fs::DirBuilder::new().mode(0o700).create(&folder);
    match make() {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            fs::remove_dir_all(&folder).unwrap();
            make().unwrap_or_else(|error| panic!("cannot make {}: {error}", folder.display()));
        }
        Err(error) => panic!("cannot make {}: {error}", folder.display()),
    }
    fs::DirBuilder::new()
        .mode(0o700)
        .create(folder.join("bin"))
        .unwrap();
    folder
}

/// Publish a complete copy of `bytes` at `path`.
fn publish(root: &Path, path: &Path, bytes: &[u8]) {
    let partial = write_partial(root, path, bytes);
    // A link publishes only a complete file, and never replaces one another run published first:
    // that copy's assessment, done or under way, is the one every link shares.
    match fs::hard_link(&partial, path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if !holds(path, bytes) {
                // Damaged (cut short, rewritten or rechmodded): replaced whole, at once.
                fs::rename(&partial, path).unwrap();
            }
        }
        Err(error) => panic!("cannot publish {}: {error}", path.display()),
    }
    match fs::remove_file(&partial) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("cannot remove {}: {error}", partial.display()),
    }
}

/// A complete executable file of `bytes` beside `path`, under a name no other writer takes.
fn write_partial(root: &Path, path: &Path, bytes: &[u8]) -> PathBuf {
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
