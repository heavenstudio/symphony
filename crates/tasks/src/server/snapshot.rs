//! [`Home::snapshot`]: the board's store kept as it was, a copy-on-write clone of its data folder in
//! `snapshots/` beside it, named by when it was taken (`2026-11-12T07-45-00Z`, UTC).
//!
//! A snapshot is taken only while no process serves the store, under the starters' lock, so no start
//! of the board's server can begin while it is taken, and holding the store's own lock (`LOCK` in
//! its folder, which SurrealKV holds for as long as a server has the store open, whatever path that
//! server was given), so no server has it open and none can open it meanwhile: never a copy of files
//! a server is writing. It is one clone of the whole folder (`clonefile` on macOS,
//! `cp --reflink=always` on Linux), which takes no room until the store moves on from it. A clone is
//! made under a name of its own (`.taking`) and renamed once whole, so a snapshot that is listed is
//! whole; one cut short is removed by the next.
//!
//! The launcher takes one before each start of the board's server, under the lock that start
//! holds: the store as the last server left it, its write-ahead log not yet replayed, which is what
//! a start that loses the log loses. Before a start that replays the log in a private server
//! ([`Home::replay`]) it is [`Home::keep`], on every board, scratch or not: that replay never runs
//! without it, as a store that went back holds a log segment no server applies and the replay's
//! clean stop removes; a volume that cannot clone gets a plain copy, said as one, and a store that
//! can be neither cloned nor copied is not replayed and the start refused. Before any other start it
//! is [`Home::snapshot_before_start`], on a board outside a scratch place unless a test says
//! otherwise, only ever a clone, and one that cannot be taken never keeps the server from starting.
//! Neither takes a second snapshot of a store a snapshot already holds as it is, its log and
//! manifest file for file ([`Kept::Already`]): a start that failed leaves the store as it was. A
//! reclaim keeps the old store the same way ([`Before::Reclaim`]) before any server of its own starts
//! on it, as it removes that store once the new one is in its place.
//!
//! Which are kept: every snapshot whose write-ahead log holds anything ([`Snapshot::log`]), until the
//! owner removes it; of the others the newest [`KEEP`]. A server that stopped cleanly leaves its
//! store's `wal/` empty, so the snapshots pruned by count are of stores whose every write was in
//! their tables. One taken after a server ended uncleanly (killed, crashed, cut off), before a
//! replay, or of a store that went back, holds a log no other copy may hold: a start replays that
//! log, and a start that loses it, or a clean stop of a store that went back, deletes it.

use super::starters::Doing;
use super::{DATA, Home, LOG, ServerError, scratch};
use crate::sys::FileLock;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The snapshots' folder in the board's folder.
pub const FOLDER: &str = "snapshots";
/// How many snapshots whose log is empty are kept: the newest.
pub const KEEP: usize = 3;
/// Where a snapshot is cloned until it is whole.
const TAKING: &str = ".taking";
/// What is said after a snapshot that holds a log ([`Snapshot::log`]).
pub const KEPT: &str = " (its write-ahead log holds writes not yet in its tables: kept until the \
                         owner removes it)";
/// The store's own lock in its folder, which SurrealKV holds while a server has the store open.
pub const STORE_LOCK: &str = "LOCK";
/// The store's write-ahead log, a folder of segments, in its folder.
const WAL: &str = "wal";
/// The store's manifest, a folder, in its folder.
const MANIFEST: &str = "manifest";

/// One snapshot of the board's store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// Its name, when it was taken, in UTC: `YYYY-MM-DDTHH-MM-SSZ`, and `.N` after a second taken in
    /// the same second.
    pub name: String,
    /// Its folder, a store a server can be started on (a clone of it, never the snapshot itself).
    pub path: PathBuf,
    /// Whether its write-ahead log holds anything ([`holds_log`]): such a snapshot is never pruned.
    pub log: bool,
}

/// What taking a snapshot did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Taken {
    /// The snapshot taken.
    pub snapshot: Snapshot,
    /// The older ones removed: those whose log is empty, beyond the newest [`KEEP`] of them.
    pub removed: Vec<String>,
}

/// The snapshot a start keeps of the store before it replays the store's log ([`Home::keep`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kept {
    /// Cloned now, copy-on-write.
    Cloned(PathBuf),
    /// Copied now, byte by byte, as the volume cannot clone.
    Copied(PathBuf),
    /// Taken earlier: its log and manifest are the store's now, file for file.
    Already(PathBuf),
}

impl Kept {
    /// Its folder.
    pub fn path(&self) -> &Path {
        match self {
            Kept::Cloned(path) | Kept::Copied(path) | Kept::Already(path) => path,
        }
    }
}

/// What a store is kept before ([`Home::keep`]): what removes what it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Before {
    /// A start's private replay of its log, whose clean stop removes a segment no server applied
    /// ([`Home::replay`]).
    Replay,
    /// `tasks-reclaim`, which removes the old store once the new one is in its place
    /// ([`Home::reclaim`]).
    Reclaim,
    /// A move of the board to another engine, whose own server on the store closes it again
    /// before it is moved aside ([`Home::move_to`]).
    Move,
}

impl Before {
    /// What the server's log says it was kept before.
    fn this(self) -> &'static str {
        match self {
            Before::Replay => "this start",
            Before::Reclaim => "this reclaim",
            Before::Move => "this move",
        }
    }

    /// What its refusal says it was to be kept before.
    fn what(self) -> &'static str {
        match self {
            Before::Replay => "the replay of its log",
            Before::Reclaim => "its reclaim",
            Before::Move => "its move to another engine",
        }
    }

    /// What its refusal says was left undone.
    fn untouched(self) -> &'static str {
        match self {
            Before::Replay => "it was not replayed and no server was started on it",
            Before::Reclaim => "it was not reclaimed, and nothing was exported or removed",
            Before::Move => "it was not moved, and no server was started on it",
        }
    }

    /// What is run again once there is room.
    fn command(self) -> &'static str {
        match self {
            Before::Replay => "the board's command",
            Before::Reclaim => "`tasks-reclaim`",
            Before::Move => "`tasks-move`",
        }
    }
}

/// Why a snapshot was not taken.
enum Untaken {
    /// A process has the store open, said: never a copy of files a server is writing.
    Open(String),
    /// Anything else, said.
    Failed(String),
}

/// Whether the store at `data` holds a write-ahead log: a segment in `wal/` with anything in it. A
/// store a server stopped cleanly holds none; one an unclean end left holds the writes its tables
/// lack, and one that went back holds a segment its manifest counts as applied and never was.
pub fn holds_log(data: &Path) -> bool {
    fs::read_dir(data.join(WAL)).is_ok_and(|segments| {
        segments.filter_map(Result::ok).any(|segment| {
            segment
                .metadata()
                .is_ok_and(|meta| meta.is_file() && meta.len() > 0)
        })
    })
}

impl Home {
    /// Where its snapshots are.
    pub fn snapshots_folder(&self) -> PathBuf {
        self.dir.join(FOLDER)
    }

    /// Every snapshot of its store, the newest first: the folders in `snapshots/` named as one is.
    pub fn snapshots(&self) -> Result<Vec<Snapshot>, String> {
        let folder = self.snapshots_folder();
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(format!("could not read {}: {e}", folder.display())),
        };
        let mut found: Vec<Snapshot> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                (named(&name) && entry.file_type().ok()?.is_dir()).then(|| Snapshot {
                    log: holds_log(&entry.path()),
                    path: entry.path(),
                    name,
                })
            })
            .collect();
        crate::order::sort_by(&mut found, &|a, b| order(&b.name).cmp(&order(&a.name)));
        Ok(found)
    }

    /// The folders of the snapshots kept until the owner removes them, those whose log holds
    /// anything, the oldest first; and the clone cut short (`.taking`), when a start or a snapshot
    /// ended in the middle of one. What `tasks database` lists.
    pub fn kept(&self) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let mut kept: Vec<PathBuf> = self
            .snapshots()
            .unwrap_or_default()
            .into_iter()
            .filter(|one| one.log)
            .map(|one| one.path)
            .collect();
        kept.reverse();
        let taking = self.snapshots_folder().join(TAKING);
        let partial = fs::symlink_metadata(&taking)
            .is_ok()
            .then_some(taking)
            .into_iter()
            .collect();
        (kept, partial)
    }

    /// Take a snapshot of its store now, at `now` (seconds since the epoch), under the starters'
    /// lock: refused while a process serves the store, or when there is none.
    pub fn snapshot(&self, now: f64) -> Result<Taken, String> {
        self.require_surreal().map_err(|why| why.to_string())?;
        let _held = self.lock(Doing::Snapshotting).map_err(|e| e.to_string())?;
        self.snapshot_held(now)
    }

    /// [`Home::snapshot`], the starters' lock held by the caller.
    fn snapshot_held(&self, now: f64) -> Result<Taken, String> {
        self.take(now, false)
            .map(|(taken, _)| taken)
            .map_err(|untaken| match untaken {
                Untaken::Open(why) => format!(
                    "{why}: a snapshot is taken only while no process serves the store, never of \
                     files a server is writing, so none was taken; one is taken before the \
                     server's next start, and one of a store held stopped meanwhile is taken by \
                     this command"
                ),
                Untaken::Failed(why) => why,
            })
    }

    /// A snapshot of its store taken now, at `now`, the starters' lock held, with whether it is a
    /// plain copy: one only when `copying` and the volume cannot clone. The older ones whose log is
    /// empty are pruned past the newest [`KEEP`].
    fn take(&self, now: f64, copying: bool) -> Result<(Taken, bool), Untaken> {
        let failed = Untaken::Failed;
        let data = self.dir.join(DATA);
        if !data.is_dir() {
            return Err(failed(format!(
                "the board has no store at {} to take a snapshot of",
                data.display()
            )));
        }
        if let Some(running) = self.serving() {
            return Err(Untaken::Open(format!(
                "the board's server (pid {}) runs on {}",
                running.pid,
                data.display()
            )));
        }
        // Held until the clone is whole: no server has the store open, however it named its path,
        // and none opens it meanwhile.
        let _store = match hold_store(&data).map_err(failed)? {
            Ok(held) => held,
            Err(holder) => {
                return Err(Untaken::Open(format!(
                    "{} holds the store's own lock, {}, so a server has the store open, by \
                     whatever path it was given",
                    holder.map_or_else(
                        || "a process".into(),
                        |pid| format!("a process (pid {pid})")
                    ),
                    data.join(STORE_LOCK).display()
                )));
            }
        };
        let folder = self.snapshots_folder();
        self.make(&folder).map_err(|e| failed(e.to_string()))?;
        // Under the lock no other snapshot is being taken: a clone under way is one cut short.
        let taking = folder.join(TAKING);
        if fs::symlink_metadata(&taking).is_ok() {
            fs::remove_dir_all(&taking)
                .map_err(|e| failed(format!("could not remove {}: {e}", taking.display())))?;
        }
        let copied = match clone(&data, &taking) {
            Ok(()) => false,
            Err(why) if copying => {
                let _ = fs::remove_dir_all(&taking);
                copy(&data, &taking).map_err(|again| {
                    let _ = fs::remove_dir_all(&taking);
                    failed(format!(
                        "it could not be cloned ({why}) nor copied ({again})"
                    ))
                })?;
                true
            }
            Err(why) => {
                let _ = fs::remove_dir_all(&taking);
                return Err(failed(format!(
                    "could not clone {} to {} copy-on-write ({why}), and a snapshot is a plain \
                     copy only before a replay of the store's log or a reclaim",
                    data.display(),
                    taking.display()
                )));
            }
        };
        let name = free_name(&folder, &stamp(now));
        let path = folder.join(&name);
        fs::rename(&taking, &path).map_err(|e| {
            let _ = fs::remove_dir_all(&taking);
            failed(format!(
                "could not move {} to {}: {e}",
                taking.display(),
                path.display()
            ))
        })?;
        let log = holds_log(&path);
        let mut removed = Vec::new();
        let prunable = self
            .snapshots()
            .map_err(failed)?
            .into_iter()
            .filter(|one| !one.log);
        for old in prunable.skip(KEEP) {
            fs::remove_dir_all(&old.path)
                .map_err(|e| failed(format!("could not remove {}: {e}", old.path.display())))?;
            removed.push(old.name);
        }
        let snapshot = Snapshot { name, path, log };
        Ok((Taken { snapshot, removed }, copied))
    }

    /// The snapshot that holds the store as it is now, its log and manifest file for file
    /// ([`log_and_manifest`]), the newest if more than one does.
    fn holding_now(&self) -> Option<Snapshot> {
        let now = log_and_manifest(&self.dir.join(DATA))?;
        self.snapshots()
            .ok()?
            .into_iter()
            .find(|one| log_and_manifest(&one.path).as_ref() == Some(&now))
    }

    /// The snapshot kept of the store before `before`, a replay of its log ([`Home::replay`]) or a
    /// reclaim ([`Home::reclaim`]), the starters' lock held, at `now`: taken now, a clone, or a
    /// plain copy where the volume cannot clone; none more when one already holds the store as it
    /// is ([`Kept::Already`]). A replay's holds the log the replay applies, so it is never pruned;
    /// a reclaim's is the store the reclaim removes, kept as any snapshot is. Said in the server's
    /// log. A store that can be neither cloned nor copied, or that a process has open, is refused,
    /// so neither runs without it.
    pub(super) fn keep(&self, now: f64, before: Before) -> Result<Kept, ServerError> {
        let this = before.this();
        if let Some(already) = self.holding_now() {
            self.note_in_log(&format!(
                "· the store before {this} is as the snapshot {} holds it{}: none more taken",
                already.path.display(),
                if already.log { KEPT } else { "" }
            ));
            return Ok(Kept::Already(already.path));
        }
        let data = self.dir.join(DATA);
        let refused = |why: String, then: &str| {
            let message = format!(
                "the board's store at {} could not be kept before {}: {why}; {}{then}",
                data.display(),
                before.what(),
                before.untouched()
            );
            match before {
                Before::Replay | Before::Move => ServerError::Io(message),
                Before::Reclaim => ServerError::Reclaim(message),
            }
        };
        let (taken, copied) = self.take(now, true).map_err(|untaken| match untaken {
            Untaken::Open(why) => refused(why, ""),
            Untaken::Failed(why) => refused(
                why,
                &format!(
                    "; where the volume cannot clone, its plain copy needs free room equal to the \
                     whole store ({}): make room, or clone it by hand, and run {} again",
                    crate::size(super::bytes(&data)),
                    before.command()
                ),
            ),
        })?;
        let path = taken.snapshot.path;
        self.note_in_log(&format!(
            "· a snapshot of the store before {this}{}: {}{}",
            if copied {
                ", a plain copy, as this volume cannot clone"
            } else {
                ""
            },
            path.display(),
            if taken.snapshot.log { KEPT } else { "" }
        ));
        Ok(if copied {
            Kept::Copied(path)
        } else {
            Kept::Cloned(path)
        })
    }

    /// Whether this board's servers take a snapshot before each start: one outside a scratch place
    /// does, unless a test said ([`Home::snapshotting`]). A start that replays the store's log
    /// keeps one whatever this says ([`Home::keep`]).
    fn snapshots_before_start(&self) -> bool {
        self.snapshotting.unwrap_or_else(|| !scratch(&self.dir))
    }

    /// The snapshot the launcher takes before a start that replays nothing, the starters' lock
    /// held: of the store as the last server left it, when there is one and this board takes them,
    /// and none more when one already holds it as it is. Whatever happens is said in the server's
    /// log; a snapshot that cannot be taken never keeps the server from starting.
    pub(super) fn snapshot_before_start(&self) {
        if !self.snapshots_before_start() || !self.dir.join(DATA).is_dir() {
            return;
        }
        if let Some(already) = self.holding_now() {
            self.note_in_log(&format!(
                "· the store before this start is as the snapshot {} holds it{}: none more taken",
                already.path.display(),
                if already.log { KEPT } else { "" }
            ));
            return;
        }
        self.note_in_log(&match self.snapshot_held(crate::store::now()) {
            Ok(taken) => format!(
                "· a snapshot of the store before this start: {}{}",
                taken.snapshot.path.display(),
                if taken.snapshot.log { KEPT } else { "" }
            ),
            Err(why) => format!("· no snapshot of the store before this start: {why}"),
        });
    }

    /// `said` appended to the server's log as a line of its own; one that cannot be written is
    /// said nowhere.
    pub(super) fn note_in_log(&self, said: &str) {
        let log = self.dir.join(LOG);
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&log) {
            use std::io::Write;
            let _ = writeln!(file, "{said}");
        }
    }
}

/// The store at `store`'s log and manifest, file by file: each file in its `wal/` and `manifest/`,
/// by its path there, with its length and when it was last written, which a clone and a copy keep.
/// Two stores alike in it hold the same log under the same manifest. None for a store without
/// either folder, or one that cannot be read, which is like no other.
fn log_and_manifest(store: &Path) -> Option<Vec<(PathBuf, u64, SystemTime)>> {
    let mut found = Vec::new();
    for part in [WAL, MANIFEST] {
        for entry in fs::read_dir(store.join(part)).ok()? {
            let entry = entry.ok()?;
            let meta = entry.metadata().ok()?;
            if meta.is_file() {
                found.push((
                    Path::new(part).join(entry.file_name()),
                    meta.len(),
                    meta.modified().ok()?,
                ));
            }
        }
    }
    found.sort_unstable();
    Some(found)
}

/// Whether the volume of `folder`, which must exist, clones a folder copy-on-write, asked as a
/// snapshot asks it: one clone of a probe folder holding a file, made in `folder`, both removed
/// after. What a snapshot taken there is, a clone or (before a replay or a reclaim) a plain copy,
/// follows from it, whatever the system: APFS clones and HFS+ does not, btrfs and XFS clone and
/// ext4 does not.
pub fn clones(folder: &Path) -> bool {
    static PROBES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let probe = folder.join(format!(
        ".clone-probe-{}-{}",
        std::process::id(),
        PROBES.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let (from, to) = (probe.join("from"), probe.join("to"));
    // Made one level at a time, so a folder that is not there is never made: it clones nothing.
    if fs::create_dir(&probe).is_err() {
        return false;
    }
    let cloned = fs::create_dir(&from)
        .and_then(|()| fs::write(from.join("file"), b"probe"))
        .is_ok()
        && clone(&from, &to).is_ok();
    let _ = fs::remove_dir_all(&probe);
    cloned
}

#[cfg(test)]
thread_local! {
    /// Why a clone made on this thread fails, while a test says one does: a volume that cannot
    /// clone, so the plain copy's path runs where every volume a test runs on clones.
    pub(super) static CLONE_FAILS: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

/// Clone the folder `from` whole to `to`, which must not exist, copy-on-write: one `clonefile` on
/// macOS, `cp -R --reflink=always` on Linux; why not, when it cannot.
pub(super) fn clone(from: &Path, to: &Path) -> Result<(), String> {
    #[cfg(test)]
    if let Some(why) = CLONE_FAILS.get() {
        return Err(why.into());
    }
    #[cfg(target_os = "macos")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        /// `clonefile`'s `CLONE_NOFOLLOW`: a link is cloned as itself.
        const CLONE_NOFOLLOW: u32 = 1;
        let (Ok(src), Ok(dst)) = (
            CString::new(from.as_os_str().as_bytes()),
            CString::new(to.as_os_str().as_bytes()),
        ) else {
            return Err("a path holds a NUL".into());
        };
        // SAFETY: both are NUL-terminated paths that outlive the call, which reads only them.
        if unsafe { libc::clonefile(src.as_ptr(), dst.as_ptr(), CLONE_NOFOLLOW) } != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        cp(
            &[
                "-R",
                "--no-dereference",
                "--preserve=all",
                "--reflink=always",
            ],
            from,
            to,
        )
    }
}

/// Copy the folder `from` whole to `to`, which must not exist, byte by byte, keeping each file's
/// times.
pub(super) fn copy(from: &Path, to: &Path) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        cp(&["-R", "-p"], from, to)
    } else {
        cp(&["-R", "--no-dereference", "--preserve=all"], from, to)
    }
}

/// `cp OPTIONS FROM TO`, its refusal what it said.
fn cp(options: &[&str], from: &Path, to: &Path) -> Result<(), String> {
    let out = crate::sys::output(crate::sys::command("cp").args(options).arg(from).arg(to))
        .map_err(|problem| format!("cp: {problem}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// The store at `data`'s own lock ([`STORE_LOCK`]), taken until the hold returned drops; when another
/// process holds it, the pid it wrote there, if it reads as one. The lock is the one SurrealKV takes
/// (an exclusive `flock`), so a server that has the store open holds it whatever path it was given,
/// and one that opens it meanwhile is refused. A store with no lock file yet gets an empty one, which
/// SurrealKV fills when it opens the store.
///
/// The hold lets go of the lock itself as it drops ([`FileLock`]): a `flock` belongs to the open
/// file, and a child another thread forks while it is held keeps a copy of that file until its exec
/// (a spawn that fails, until the kernel tears its child down): a close alone would leave the lock
/// held by that child for as long, and a server started right after the snapshot would find its
/// store locked.
fn hold_store(data: &Path) -> Result<Result<FileLock, Option<i64>>, String> {
    let lock = data.join(STORE_LOCK);
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock)
        .map_err(|e| format!("could not open {}: {e}", lock.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(Ok(FileLock(file))),
        Err(fs::TryLockError::WouldBlock) => Ok(Err(fs::read_to_string(&lock)
            .ok()
            .and_then(|pid| pid.trim().parse().ok()))),
        Err(fs::TryLockError::Error(e)) => Err(format!("could not lock {}: {e}", lock.display())),
    }
}

/// `base`, or `base.N` with the first `N` from 1 that names nothing in `folder`.
fn free_name(folder: &Path, base: &str) -> String {
    if fs::symlink_metadata(folder.join(base)).is_err() {
        return base.to_string();
    }
    (1..)
        .map(|n| format!("{base}.{n}"))
        .find(|name| fs::symlink_metadata(folder.join(name)).is_err())
        .unwrap_or_else(|| base.to_string())
}

/// Whether `name` is a snapshot's: `YYYY-MM-DDTHH-MM-SSZ`, then `.N` or nothing.
pub(super) fn named(name: &str) -> bool {
    const SHAPE: &[u8; 20] = b"0000-00-00T00-00-00Z";
    let bytes = name.as_bytes();
    let (stamp, after) = bytes.split_at(bytes.len().min(SHAPE.len()));
    let shape = stamp.len() == SHAPE.len()
        && stamp.iter().zip(SHAPE).all(|(byte, want)| match want {
            b'0' => byte.is_ascii_digit(),
            want => byte == want,
        });
    shape
        && (after.is_empty()
            || after
                .strip_prefix(b".")
                .is_some_and(|n| !n.is_empty() && n.iter().all(u8::is_ascii_digit)))
}

/// How snapshots are ordered by date: by their time, then by the count after it.
pub(super) fn order(name: &str) -> (&str, u64) {
    match name.split_once('.') {
        Some((stamp, n)) => (stamp, n.parse().unwrap_or(0)),
        None => (name, 0),
    }
}

/// `seconds` since the epoch as a snapshot's name: its UTC time, `YYYY-MM-DDTHH-MM-SSZ`.
pub fn stamp(seconds: f64) -> String {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a time in seconds is far inside i64"
    )]
    let seconds = seconds.floor() as i64;
    let (days, day) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    let (year, month, date) = civil(days);
    format!(
        "{year:04}-{month:02}-{date:02}T{:02}-{:02}-{:02}Z",
        day / 3600,
        day % 3600 / 60,
        day % 60
    )
}

/// The proleptic Gregorian date `days` after 1970-01-01: year, month, day.
fn civil(days: i64) -> (i64, i64, i64) {
    // Howard Hinnant's `civil_from_days`, over eras of 400 years from 0000-03-01.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let date = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, date)
}

/// A child caught between its fork and its exec, as the release run's tests catch one.
#[cfg(test)]
#[path = "../../../../tools/test-support/mid_spawn.rs"]
mod mid_spawn;

#[cfg(test)]
mod tests {
    use super::mid_spawn::MidSpawn;
    use super::{
        Before, CLONE_FAILS, FOLDER, KEPT, Kept, STORE_LOCK, TAKING, civil, clones, free_name,
        hold_store, holds_log, named, order, stamp,
    };
    use crate::children::Child as _;
    use crate::server::{Home, LOG, ServerError};
    use crate::sys::FileLock;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    /// A board folder of its own under the temporary folder, `name`'s, with a store whose log,
    /// manifest and tables hold a few bytes each; served by no `surreal`.
    fn store(name: &str) -> (PathBuf, Home) {
        let dir = std::env::temp_dir().join(format!("snapshot-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for part in ["data/wal", "data/manifest", "data/sstables"] {
            fs::create_dir_all(dir.join(part)).unwrap();
        }
        fs::write(dir.join("data/wal/00000000000000000017.wal"), b"a log").unwrap();
        fs::write(
            dir.join("data/manifest/00000000000000000000.manifest"),
            b"m",
        )
        .unwrap();
        fs::write(dir.join("data/sstables/1.sst"), b"rows").unwrap();
        let home = Home::new(&dir, &dir.join("no-surreal"));
        (dir, home)
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap_or_default()
    }

    #[test]
    fn a_snapshot_is_named_by_its_utc_second() {
        assert_eq!(stamp(0.0), "1970-01-01T00-00-00Z");
        assert_eq!(stamp(59.9), "1970-01-01T00-00-59Z", "the second floored");
        assert_eq!(stamp(1_762_933_512.0), "2025-11-12T07-45-12Z");
        // 2026-11-12 08:45 in Rome, the board's loss.
        assert_eq!(stamp(1_794_469_500.0), "2026-11-12T07-45-00Z");
        assert_eq!(stamp(951_782_400.0), "2000-02-29T00-00-00Z", "a leap day");
        assert_eq!(
            stamp(4_107_542_399.0),
            "2100-02-28T23-59-59Z",
            "2100 is no leap year"
        );
        assert_eq!(stamp(-1.0), "1969-12-31T23-59-59Z");
        assert_eq!(civil(-719_468), (0, 3, 1));
    }

    #[test]
    fn only_a_name_of_that_shape_is_a_snapshot_and_they_order_by_date_then_count() {
        for good in [
            "2026-11-12T07-45-00Z",
            "2026-11-12T07-45-00Z.1",
            "2026-11-12T07-45-00Z.12",
        ] {
            assert!(named(good), "{good}");
        }
        for bad in [
            "",
            ".taking",
            "data",
            "2026-11-12T07-45-00",
            "2026-11-12T07:45:00Z",
            "2026-11-12 07-45-00Z",
            "2026-11-12T07-45-00Z.",
            "2026-11-12T07-45-00Z.x",
            "2026-11-12T07-45-00Z-old",
            "x026-11-12T07-45-00Z",
            "2026-1é-12T07-45-00Z",
            "2026-11-12T07-45-00é",
        ] {
            assert!(!named(bad), "{bad}");
        }
        let mut names = vec![
            "2026-11-12T07-45-00Z.10",
            "2026-11-12T07-45-00Z",
            "2026-11-12T07-45-00Z.2",
            "2026-11-11T23-59-59Z",
        ];
        names.sort_by(|a, b| order(b).cmp(&order(a)));
        assert_eq!(
            names,
            [
                "2026-11-12T07-45-00Z.10",
                "2026-11-12T07-45-00Z.2",
                "2026-11-12T07-45-00Z",
                "2026-11-11T23-59-59Z"
            ]
        );
    }

    #[test]
    fn a_store_holds_a_log_when_a_segment_in_its_wal_holds_anything() {
        let data = std::env::temp_dir().join(format!("snapshot-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&data).unwrap();
        assert!(!holds_log(&data), "no wal/ at all");
        let wal = data.join("wal");
        std::fs::create_dir(&wal).unwrap();
        assert!(
            !holds_log(&data),
            "an empty wal/, as a clean stop leaves it"
        );
        std::fs::write(wal.join("00000000000000000003.wal"), "").unwrap();
        std::fs::create_dir(wal.join("not-a-segment")).unwrap();
        assert!(!holds_log(&data), "an empty segment, and a folder");
        std::fs::write(wal.join("00000000000000000004.wal"), "x").unwrap();
        assert!(holds_log(&data));
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn the_stores_own_lock_is_taken_while_free_and_names_its_holder_while_held() {
        let data = std::env::temp_dir().join(format!("snapshot-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&data).unwrap();
        // No lock file yet: one is made, empty, and taken.
        let held = hold_store(&data).unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(data.join(STORE_LOCK)).unwrap(), "");
        drop(held);
        // Held as SurrealKV holds it, on a file of its own that names its pid: that pid is said,
        // and what it wrote is left as it was.
        std::fs::write(data.join(STORE_LOCK), "4242\n").unwrap();
        let holder = std::fs::File::open(data.join(STORE_LOCK)).unwrap();
        holder.try_lock().unwrap();
        // Let go as the store's hold lets go of its own, by itself: a close alone leaves it held
        // by any child another test's thread forked meanwhile, until that child's exec.
        let holder = FileLock(holder);
        assert_eq!(hold_store(&data).unwrap().err(), Some(Some(4242)));
        std::fs::write(data.join(STORE_LOCK), "not a pid").unwrap();
        assert_eq!(hold_store(&data).unwrap().err(), Some(None));
        drop(holder);
        let held = hold_store(&data).unwrap();
        assert!(held.is_ok());
        assert_eq!(
            std::fs::read_to_string(data.join(STORE_LOCK)).unwrap(),
            "not a pid"
        );
        drop(held);
        // A folder that is no store's: refused, not taken.
        let missing = data.join("missing");
        let Err(refused) = hold_store(&missing) else {
            panic!("a folder that is no store's was taken");
        };
        assert!(refused.starts_with(&format!(
            "could not open {}: ",
            missing.join(STORE_LOCK).display()
        )));
        let _ = std::fs::remove_dir_all(&data);
    }

    /// A child another thread forks while a lock is held keeps a copy of the lock's file until its
    /// exec, and a lock let go by closing its file alone stays held by that copy, which a parallel
    /// run of the test above can meet. The store's hold lets go of its lock itself, so the lock is
    /// free at once, whatever child forked while it was held.
    #[test]
    fn the_stores_lock_let_go_is_free_at_once_though_a_child_forked_meanwhile_has_not_execd() {
        let dir = std::env::temp_dir().join(format!("snapshot-forked-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let (data, closed) = (dir.join("data"), dir.join("closed"));
        for folder in [&data, &closed] {
            fs::create_dir_all(folder).unwrap();
        }
        let held = hold_store(&data).unwrap().unwrap();
        let bare = fs::File::create(closed.join(STORE_LOCK)).unwrap();
        bare.try_lock().unwrap();
        let child = MidSpawn::caught(
            std::process::Command::of("true"),
            crate::sys::Spawning::spawn_locked,
        );
        drop((held, bare));
        assert!(
            hold_store(&closed).unwrap().is_err(),
            "a lock let go by its close alone is still held by the child's copy of its file"
        );
        assert!(
            hold_store(&data).unwrap().is_ok(),
            "the store's lock, let go, is still held by a child forked while it was held"
        );
        drop(child);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_second_snapshot_in_the_same_second_takes_the_next_count() {
        let folder = std::env::temp_dir().join(format!("snapshot-names-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let base = "2026-11-12T07-45-00Z";
        assert_eq!(free_name(&folder, base), base);
        std::fs::create_dir(folder.join(base)).unwrap();
        assert_eq!(free_name(&folder, base), format!("{base}.1"));
        std::fs::create_dir(folder.join(format!("{base}.1"))).unwrap();
        assert_eq!(free_name(&folder, base), format!("{base}.2"));
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// A store is kept whole before a replay, said in the server's log, and kept again only once
    /// its log or manifest moved: a start whose replay failed, leaving the store as it was, keeps
    /// none more. A store with no manifest is like no other, so it is kept each time; each holds a
    /// log, so none is pruned, however many there are.
    #[test]
    fn a_store_is_kept_whole_and_again_only_once_its_log_or_manifest_moved() {
        let (dir, home) = store("kept");
        let at = 1_794_469_500.0;
        // A clone where the volume clones (APFS, btrfs, XFS), a copy where it cannot.
        let first = home.keep(at, Before::Replay).unwrap();
        let (Kept::Cloned(path) | Kept::Copied(path)) = &first else {
            panic!("{first:?}");
        };
        assert_eq!(matches!(first, Kept::Cloned(_)), clones(&dir));
        assert_eq!(path, &dir.join(FOLDER).join("2026-11-12T07-45-00Z"));
        for (file, bytes) in [
            ("wal/00000000000000000017.wal", &b"a log"[..]),
            ("manifest/00000000000000000000.manifest", b"m"),
            ("sstables/1.sst", b"rows"),
        ] {
            assert_eq!(fs::read(path.join(file)).unwrap(), bytes, "{file}");
        }
        assert!(
            read(&dir.join(LOG)).ends_with(&format!(
                "· a snapshot of the store before this start: {}{KEPT}\n",
                path.display()
            )),
            "{}",
            read(&dir.join(LOG))
        );
        assert_eq!(
            home.keep(at + 60.0, Before::Replay).unwrap(),
            Kept::Already(path.clone())
        );
        assert!(read(&dir.join(LOG)).ends_with(&format!(
            "· the store before this start is as the snapshot {} holds it{KEPT}: none more taken\n",
            path.display()
        )));
        assert_eq!(home.kept(), (vec![path.clone()], vec![]));
        // A reclaim of the store as it is keeps none more either, and says it was before it.
        assert_eq!(
            home.keep(at + 90.0, Before::Reclaim).unwrap(),
            Kept::Already(path.clone())
        );
        assert!(read(&dir.join(LOG)).ends_with(&format!(
            "· the store before this reclaim is as the snapshot {} holds it{KEPT}: none more \
             taken\n",
            path.display()
        )));

        fs::write(dir.join("data/wal/00000000000000000018.wal"), b"more").unwrap();
        let second = home.keep(at, Before::Reclaim).unwrap();
        assert!(!matches!(second, Kept::Already(_)), "{second:?}");
        assert_eq!(
            second.path(),
            dir.join(FOLDER).join("2026-11-12T07-45-00Z.1")
        );
        assert!(
            read(&dir.join(LOG)).ends_with(&format!(
                "· a snapshot of the store before this reclaim: {}{KEPT}\n",
                second.path().display()
            )),
            "{}",
            read(&dir.join(LOG))
        );
        assert!(
            !path.join("wal/00000000000000000018.wal").exists(),
            "a kept store took a later write of the store's"
        );
        assert_eq!(
            home.kept().0,
            vec![path.clone(), second.path().to_path_buf()],
            "the oldest first"
        );

        fs::remove_dir_all(dir.join("data/manifest")).unwrap();
        for _ in 0..3 {
            assert!(!matches!(
                home.keep(at, Before::Replay).unwrap(),
                Kept::Already(_)
            ));
        }
        assert_eq!(home.kept().0.len(), 5, "past KEEP, as each holds a log");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Makes every clone on this thread fail, saying `why`, until it drops: a volume that cannot
    /// clone.
    struct CannotClone;

    impl CannotClone {
        fn saying(why: &'static str) -> CannotClone {
            CLONE_FAILS.set(Some(why));
            CannotClone
        }
    }

    impl Drop for CannotClone {
        fn drop(&mut self) {
            CLONE_FAILS.set(None);
        }
    }

    /// Where the volume cannot clone, a store is kept before a replay or a reclaim by a plain copy:
    /// whole, its files' times kept, said as a plain copy in the server's log, and never pruned
    /// while its log holds writes. Its times kept, the next keep of the store as it is finds the
    /// copy holds it and copies nothing more. The snapshot before a start that replays nothing is
    /// only ever a clone: none is taken, and the server's log says why.
    #[test]
    fn a_store_that_cannot_be_cloned_is_kept_by_a_plain_copy_whole_and_said_as_one() {
        let (dir, home) = store("copied");
        let at = 1_794_469_500.0;
        let _cannot = CannotClone::saying("this volume cannot clone");
        assert!(!clones(&dir), "the probe asks the clone a snapshot makes");
        let first = home.keep(at, Before::Replay).unwrap();
        let Kept::Copied(path) = &first else {
            panic!("{first:?}");
        };
        assert_eq!(path, &dir.join(FOLDER).join("2026-11-12T07-45-00Z"));
        for file in [
            "wal/00000000000000000017.wal",
            "manifest/00000000000000000000.manifest",
            "sstables/1.sst",
        ] {
            let (copy, original) = (path.join(file), dir.join("data").join(file));
            assert_eq!(
                fs::read(&copy).unwrap(),
                fs::read(&original).unwrap(),
                "{file}"
            );
            assert_eq!(
                fs::metadata(&copy).unwrap().modified().unwrap(),
                fs::metadata(&original).unwrap().modified().unwrap(),
                "{file}: its time"
            );
        }
        assert!(
            read(&dir.join(LOG)).ends_with(&format!(
                "· a snapshot of the store before this start, a plain copy, as this volume cannot \
                 clone: {}{KEPT}\n",
                path.display()
            )),
            "{}",
            read(&dir.join(LOG))
        );
        assert_eq!(
            home.kept(),
            (vec![path.clone()], vec![]),
            "nothing cut short"
        );
        assert_eq!(
            home.keep(at + 60.0, Before::Reclaim).unwrap(),
            Kept::Already(path.clone()),
            "the copy kept the times its store's log and manifest are known by"
        );

        fs::write(dir.join("data/wal/00000000000000000018.wal"), b"more").unwrap();
        let second = home.keep(at, Before::Reclaim).unwrap();
        assert_eq!(
            second,
            Kept::Copied(dir.join(FOLDER).join("2026-11-12T07-45-00Z.1"))
        );
        assert!(read(&dir.join(LOG)).ends_with(&format!(
            "· a snapshot of the store before this reclaim, a plain copy, as this volume cannot \
             clone: {}{KEPT}\n",
            second.path().display()
        )));

        let home = home.snapshotting(true);
        fs::write(dir.join("data/wal/00000000000000000019.wal"), b"later").unwrap();
        home.snapshot_before_start();
        assert_eq!(
            home.snapshots().unwrap().len(),
            2,
            "no plain copy before a start"
        );
        assert!(
            read(&dir.join(LOG)).ends_with(&format!(
                "· no snapshot of the store before this start: could not clone {} to {} \
                 copy-on-write (this volume cannot clone), and a snapshot is a plain copy only \
                 before a replay of the store's log or a reclaim\n",
                dir.join("data").display(),
                dir.join(FOLDER).join(TAKING).display()
            )),
            "{}",
            read(&dir.join(LOG))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The volume a test runs on is asked, not its system: the probe clones as a snapshot does,
    /// and leaves nothing behind.
    #[test]
    fn whether_a_volume_clones_is_asked_of_the_volume_and_leaves_nothing() {
        let dir = std::env::temp_dir().join(format!("snapshot-clones-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let (from, to) = (dir.join("from"), dir.join("to"));
        fs::create_dir(&from).unwrap();
        fs::write(from.join("file"), b"x").unwrap();
        assert_eq!(clones(&dir), super::clone(&from, &to).is_ok());
        let left: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.starts_with(".clone-probe-"))
            .collect();
        assert!(left.is_empty(), "{left:?}");
        assert!(!clones(&dir.join("missing")), "no folder, no clone");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A store that can be neither cloned nor copied is not replayed: the refusal says so, and
    /// leaves nothing cut short. One a process has open is refused the same way, naming it.
    #[test]
    fn a_store_that_cannot_be_kept_refuses_the_replay() {
        let (dir, home) = store("unkept");
        let folder = dir.join(FOLDER);
        fs::create_dir_all(&folder).unwrap();
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();
        let refused = home.keep(0.0, Before::Replay).unwrap_err().to_string();
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
        let data = dir.join("data");
        assert!(
            refused.starts_with(&format!(
                "the board's store at {} could not be kept before the replay of its log: it could \
                 not be cloned (",
                data.display()
            )) && refused.contains(") nor copied ("),
            "{refused}"
        );
        // The room a plain copy takes: the whole store.
        let room = crate::size(crate::server::bytes(&data));
        assert!(
            refused.ends_with(&format!(
                "); it was not replayed and no server was started on it; where the volume cannot \
                 clone, its plain copy needs free room equal to the whole store ({room}): make \
                 room, or clone it by hand, and run the board's command again"
            )),
            "{refused}"
        );
        assert_eq!(home.kept(), (vec![], vec![]), "nothing cut short is left");
        // Before a reclaim, a reclaim's refusal, in its own words.
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();
        let refused = home.keep(0.0, Before::Reclaim).unwrap_err();
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
        let ServerError::Reclaim(said) = &refused else {
            panic!("{refused:?}");
        };
        assert!(
            said.starts_with(&format!(
                "the board's store at {} could not be kept before its reclaim: it could not be \
                 cloned (",
                data.display()
            )) && said.ends_with(&format!(
                "); it was not reclaimed, and nothing was exported or removed; where the volume \
                 cannot clone, its plain copy needs free room equal to the whole store ({room}): \
                 make room, or clone it by hand, and run `tasks-reclaim` again"
            )),
            "{said}"
        );
        assert_eq!(home.kept(), (vec![], vec![]));

        let lock = data.join(STORE_LOCK);
        fs::write(&lock, "4242\n").unwrap();
        let holder = fs::File::open(&lock).unwrap();
        holder.try_lock().unwrap();
        assert_eq!(
            home.keep(0.0, Before::Replay).unwrap_err().to_string(),
            format!(
                "the board's store at {} could not be kept before the replay of its log: a process \
                 (pid 4242) holds the store's own lock, {}, so a server has the store open, by \
                 whatever path it was given; it was not replayed and no server was started on it",
                data.display(),
                lock.display()
            )
        );
        drop(holder);
        assert!(home.snapshots().unwrap().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    /// The snapshot before a start that replays nothing is taken only by a board that takes them,
    /// once for a store as it is; a clone cut short is listed until the next snapshot removes it.
    #[test]
    fn a_start_takes_no_second_snapshot_of_a_store_one_holds_as_it_is() {
        let (dir, home) = store("routine");
        let home = home.snapshotting(true);
        home.snapshot_before_start();
        let taken = home.snapshots().unwrap();
        assert_eq!(taken.len(), 1, "{taken:?}");
        home.snapshot_before_start();
        assert_eq!(home.snapshots().unwrap(), taken, "none more");
        assert!(read(&dir.join(LOG)).ends_with(&format!(
            "· the store before this start is as the snapshot {} holds it{KEPT}: none more taken\n",
            taken[0].path.display()
        )));

        let cut = dir.join(FOLDER).join(TAKING);
        fs::create_dir(&cut).unwrap();
        assert_eq!(
            home.kept(),
            (vec![taken[0].path.clone()], vec![cut.clone()])
        );
        fs::write(
            dir.join("data/wal/00000000000000000017.wal"),
            b"a longer log",
        )
        .unwrap();
        home.snapshot_before_start();
        assert_eq!(home.snapshots().unwrap().len(), 2, "the store moved on");
        assert!(!cut.exists(), "the clone cut short is gone");

        let (scratch, by_its_place) = store("routine-scratch");
        by_its_place.snapshot_before_start();
        assert!(
            by_its_place.snapshots().unwrap().is_empty(),
            "a scratch place takes none"
        );
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&scratch);
    }
}
