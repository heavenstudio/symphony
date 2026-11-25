//! The machine's slots for scratch boards' servers ([`Slots`]). Every PostgreSQL server holds one
//! System V shared memory segment for its life, the interlock it keeps beside its mapped shared
//! memory, and `initdb` takes one at a time as it makes a cluster. macOS allows 32 machine-wide
//! (`kern.sysv.shmmni`), shared by the projects' own boards, every test's scratch board, other
//! users' servers and the segments dead servers left behind, and `shmget` refuses a start past the
//! last ("No space left on device").
//!
//! So a server that ends with its starter holds a slot for its life ([`super::Postgres`]): a file
//! of the machine's folder of slots ([`Slots::machine`], under `/tmp`, which a boot clears with the
//! segments) locked with `flock`, taken before `initdb` and handed to the server's supervisor
//! ([`Slot::handed`]), from which the server and each of its backends inherit it, so the kernel
//! lets it go once the last of them has ended, however it ends. A server that outlives its starter,
//! a project's own board's, takes none.
//!
//! The slots are half the machine's limit ([`share`]), so the rest stays for the projects' own
//! boards, and fewer while the segments others hold leave fewer free ([`Room`]): a segment no
//! slot's server made (a project's board, another user's server, one a dead server left behind)
//! counts against the limit. A start counts and takes under the folder's own lock ([`TAKING`]), so
//! two never both take the last: it takes the lowest free slot while fewer are held than there is
//! room for. Otherwise it says so once and waits on the held slots' locks themselves, a thread
//! blocked on each ([`first_granted`]), until the kernel grants one, then decides again with that
//! one in hand: never a poll. A wait that reaches its backstop ([`BACKSTOP`]), which only a stall
//! reaches, is refused naming the processes the held slots say hold them, and a start with no slot
//! held and no room is refused at once, as nothing it could wait for makes room.
//!
//! A slot's file says who holds it, a pid on one line: its starter's while it starts the server,
//! then the server's once it is ready ([`Handed::serving`]). The count takes a segment whose
//! creator is that pid for the slot's own.

use crate::channel::{RecvTimeoutError, channel};
use crate::server::{ServerError, io};
use crate::sys::{command, load_said, output, pid_alive};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{ErrorKind, Read as _};
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
use std::os::unix::fs::{DirBuilderExt as _, FileExt as _, MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Where the machine's folder of slots is made, beside the boards' socket folders.
const PLACE: &str = "/tmp";
/// In the folder: the lock a start counts and takes under.
const TAKING: &str = "taking";
/// In the folder: slot `N` is the file `slot-N`.
const SLOT: &str = "slot-";
/// How long a start waits for a slot before it is refused. A slot comes free as soon as any one
/// of the servers holding one ends, each a test's or a command's, so only a stall reaches this:
/// past the longest one measured on this machine under load (a page's renderer given no task for
/// 88 s, test-time's floor).
pub const BACKSTOP: Duration = Duration::from_secs(120);
/// The slots of a machine whose limit cannot be read: macOS's share of its own.
const UNREAD: usize = 16;
/// The lowest number a slot's descriptor is handed to the supervisor under: above the standard
/// streams and the two descriptors its script opens for itself (3 and 4).
const HANDED_FROM: libc::c_int = 10;

/// The slots scratch boards' servers take: `count` locked files in `folder`, waited for
/// `backstop` at most.
#[derive(Debug, Clone, PartialEq)]
pub struct Slots {
    folder: PathBuf,
    count: usize,
    backstop: Duration,
}

impl Slots {
    /// This machine's: `symphony-pg-slots-UID` under `/tmp`, its user's own, and half the
    /// machine's limit of segments ([`share`]).
    pub fn machine() -> Slots {
        // SAFETY: geteuid reads nothing of ours and cannot fail.
        let user = unsafe { libc::geteuid() };
        Slots::at(
            &Path::new(PLACE).join(format!("symphony-pg-slots-{user}")),
            share(limit()),
        )
    }

    /// `count` slots, one at least, in `folder`, made where it is not there: a test's, apart from
    /// the machine's. The machine's limit and the segments in use still bound how many are held.
    pub fn at(folder: &Path, count: usize) -> Slots {
        Slots {
            folder: folder.into(),
            count: count.max(1),
            backstop: BACKSTOP,
        }
    }

    /// The same slots, a start waiting `backstop` at most for one.
    #[must_use]
    pub fn backstop(self, backstop: Duration) -> Slots {
        Slots { backstop, ..self }
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    pub fn count(&self) -> usize {
        self.count
    }

    /// A slot, taken: the lowest free one while fewer are held than the machine has room for
    /// ([`Room`]), else the first a held one's holder lets go, `waiting` told once, before the
    /// wait, what it waits on. Refused once the backstop passes, naming what holds them, and at
    /// once where no slot is held and there is no room, which nothing would make.
    pub fn take(&self, waiting: impl Fn(&str)) -> Result<Slot, ServerError> {
        self.made()?;
        let until = Instant::now() + self.backstop;
        let mut granted: Option<Locked> = None;
        let mut told = false;
        loop {
            let taking = self.taking(until)?;
            let seen = self.seen(granted.as_ref().map(|locked| locked.index))?;
            let servers: Vec<i64> = seen.held.iter().filter_map(|held| held.pid).collect();
            let room = Room::of(self.count, limit(), segments().as_deref(), &servers);
            match granted.take().or(seen.free) {
                Some(locked) if seen.held.len() < room.cap => {
                    let slot = Slot(locked);
                    slot.said(i64::from(std::process::id()));
                    return Ok(slot);
                }
                ours => drop(ours),
            }
            drop(taking);
            if seen.held.is_empty() {
                return Err(ServerError::NotUp(room.none(&self.folder)));
            }
            if !told {
                waiting(&format!(
                    "· {} of the machine's slots for scratch boards' servers are held ({}), and \
                     its shared memory has room for {}: this start waits for one to come free",
                    seen.held.len(),
                    self.folder.display(),
                    room.said(self.count)
                ));
                told = true;
            }
            let holders: Vec<(usize, Option<i64>)> = seen
                .held
                .iter()
                .map(|held| (held.index, held.pid))
                .collect();
            let files = seen.held.into_iter().map(|held| (held.index, held.file));
            match first_granted(files.collect(), until) {
                Ok(Some(locked)) => granted = Some(locked),
                Ok(None) => return Err(ServerError::NotUp(self.stalled(&holders, &room))),
                Err(problem) => {
                    return Err(io("could not wait on the slots in", &self.folder)(problem));
                }
            }
        }
    }

    /// The refusal of a start that waited the backstop for a slot, the `holders` it waited on
    /// named, each by the slot and the pid its file names.
    fn stalled(&self, holders: &[(usize, Option<i64>)], room: &Room) -> String {
        let named: Vec<String> = holders
            .iter()
            .map(|&(index, pid)| match pid {
                Some(pid) if pid_alive(pid) => format!("slot {index} by pid {pid}"),
                Some(pid) => format!("slot {index} by pid {pid}, which has ended"),
                None => format!("slot {index} by a holder that names none"),
            })
            .collect();
        format!(
            "no slot for a scratch board's server came free in the {} s this start waited for one \
             ({}): {} of the machine's slots are held ({}), and its shared memory has room for \
             {}; they are held by {}. Nothing was started: run it again once one of them has ended",
            self.backstop.as_secs_f64(),
            load_said(),
            holders.len(),
            self.folder.display(),
            room.said(self.count),
            named.join(", ")
        )
    }

    /// The folder, made private to this user where it is not there; refused where it is there and
    /// is not a folder of this user alone, as a board's socket folder is.
    fn made(&self) -> Result<(), ServerError> {
        let folder = &self.folder;
        match fs::DirBuilder::new().mode(0o700).create(folder) {
            Ok(()) => return Ok(()),
            Err(problem) if problem.kind() == ErrorKind::AlreadyExists => {}
            Err(problem) => return Err(io("could not make", folder)(problem)),
        }
        let there = fs::symlink_metadata(folder).map_err(io("could not read", folder))?;
        // SAFETY: geteuid reads nothing of ours and cannot fail.
        let user = unsafe { libc::geteuid() };
        if there.is_dir() && there.uid() == user && there.permissions().mode() & 0o077 == 0 {
            Ok(())
        } else {
            Err(ServerError::Io(format!(
                "{} is there and is not a folder this user alone reads: a scratch board's server \
                 takes its slot nowhere else",
                folder.display()
            )))
        }
    }

    /// The folder's own lock, which a start counts and takes under, granted by `until` at most.
    fn taking(&self, until: Instant) -> Result<Locked, ServerError> {
        let path = self.folder.join(TAKING);
        let file = open(&path)?;
        match file.try_lock() {
            Ok(()) => return Ok(Locked::new(0, file)),
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Error(problem)) => return Err(io("could not lock", &path)(problem)),
        }
        match first_granted(vec![(0, file)], until) {
            Ok(Some(locked)) => Ok(locked),
            Ok(None) => Err(ServerError::NotUp(format!(
                "{} was held for the {} s this start waited for it, by another start counting the \
                 slots, which takes it for a moment ({}). Nothing was started: run it again",
                path.display(),
                self.backstop.as_secs_f64(),
                load_said()
            ))),
            Err(problem) => Err(io("could not lock", &path)(problem)),
        }
    }

    /// The slots as they stand: the held ones, and the lowest free one, locked, unless `ours`
    /// names the one this start holds already, which is passed over. A slot whose file is not
    /// there yet is free, and one is made only when none that is there is free: the lowest.
    fn seen(&self, ours: Option<usize>) -> Result<Seen, ServerError> {
        let there = self.there()?;
        let mut seen = Seen {
            free: None,
            held: Vec::new(),
        };
        for &index in &there {
            if Some(index) != ours {
                self.look(index, ours.is_none(), &mut seen)?;
            }
        }
        if ours.is_none()
            && seen.free.is_none()
            && let Some(missing) = (0..self.count).find(|index| !there.contains(index))
        {
            self.look(missing, true, &mut seen)?;
        }
        Ok(seen)
    }

    /// Slot `index`, looked at: held, or free and kept in `seen` where it is the first free one a
    /// start that `wants` one meets, else let go at once.
    fn look(&self, index: usize, wants: bool, seen: &mut Seen) -> Result<(), ServerError> {
        let path = self.path(index);
        let mut file = open(&path)?;
        match file.try_lock() {
            Ok(()) => {
                let locked = Locked::new(index, file);
                if wants && seen.free.is_none() {
                    seen.free = Some(locked);
                }
            }
            Err(TryLockError::WouldBlock) => {
                let mut said = String::new();
                let pid = file
                    .read_to_string(&mut said)
                    .ok()
                    .and_then(|_| said.trim().parse().ok());
                seen.held.push(Held { index, pid, file });
            }
            Err(TryLockError::Error(problem)) => {
                return Err(io("could not lock", &path)(problem));
            }
        }
        Ok(())
    }

    /// The slots whose files are there, of the first `count`.
    fn there(&self) -> Result<BTreeSet<usize>, ServerError> {
        let entries = fs::read_dir(&self.folder).map_err(io("could not read", &self.folder))?;
        let mut there = BTreeSet::new();
        for entry in entries {
            let entry = entry.map_err(io("could not read", &self.folder))?;
            if let Some(index) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.strip_prefix(SLOT))
                .and_then(|index| index.parse::<usize>().ok())
                .filter(|&index| index < self.count)
            {
                there.insert(index);
            }
        }
        Ok(there)
    }

    fn path(&self, index: usize) -> PathBuf {
        self.folder.join(format!("{SLOT}{index}"))
    }
}

/// `path` opened to be locked and written, made where it is not there.
fn open(path: &Path) -> Result<File, ServerError> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(io("could not open", path))
}

/// The slots as a start saw them under the folder's lock.
struct Seen {
    free: Option<Locked>,
    held: Vec<Held>,
}

/// A slot another holds: which, the pid its file names, and its file, for a wait on its lock.
struct Held {
    index: usize,
    pid: Option<i64>,
    file: File,
}

/// A file this process has locked, let go when it goes. Let go explicitly, not by its close: a
/// sibling thread's fork copies the descriptor until that child's exec.
struct Locked {
    index: usize,
    file: Option<File>,
}

impl Locked {
    fn new(index: usize, file: File) -> Locked {
        Locked {
            index,
            file: Some(file),
        }
    }
}

impl Drop for Locked {
    fn drop(&mut self) {
        if let Some(file) = &self.file {
            let _ = file.unlock();
        }
    }
}

/// A slot this process holds, let go when it goes, until it is handed to the server it is for
/// ([`Slot::handed`]).
pub struct Slot(Locked);

impl Slot {
    /// Which slot it is: its file is `slot-N` in the folder.
    pub fn index(&self) -> usize {
        self.0.index
    }

    /// Its file, while this process holds it.
    fn file(&self) -> &File {
        self.0
            .file
            .as_ref()
            .expect("a slot not handed has its file")
    }

    /// Its file says `pid` holds it; a write that fails leaves the count taking that holder's
    /// segment for another's, and stops nothing.
    fn said(&self, pid: i64) {
        said(self.file(), pid);
    }

    /// A copy of its descriptor for a child to inherit, at [`HANDED_FROM`] or above and
    /// close-on-exec in this process, as every descriptor here is.
    pub(in crate::server) fn descriptor(&self) -> std::io::Result<OwnedFd> {
        // SAFETY: F_DUPFD_CLOEXEC on a descriptor this process holds open makes a new one.
        let copy =
            unsafe { libc::fcntl(self.file().as_raw_fd(), libc::F_DUPFD_CLOEXEC, HANDED_FROM) };
        if copy < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: `copy` was just made and nothing else owns it.
        Ok(unsafe { OwnedFd::from_raw_fd(copy) })
    }

    /// The slot once a child started with its [`Slot::descriptor`]: that child and what it starts
    /// hold the lock from now on, so this process's copy closes without letting it go.
    pub(in crate::server) fn handed(mut self) -> Handed {
        Handed(self.0.file.take().expect("a slot is handed once"))
    }
}

/// A slot handed to the server it is for, this process's copy of it: closed, never unlocked, when
/// it goes.
pub(in crate::server) struct Handed(File);

impl Handed {
    /// Its file names the server `pid`, which made the segment the slot counts for, once it is
    /// ready.
    pub(in crate::server) fn serving(&self, pid: i64) {
        said(&self.0, pid);
    }
}

/// `file`, a slot's, says `pid` holds it.
fn said(file: &File, pid: i64) {
    let _ = file
        .set_len(0)
        .and_then(|()| file.write_all_at(format!("{pid}\n").as_bytes(), 0));
}

/// The first of `files` whose lock the kernel grants, each waited on by a thread of its own
/// blocked in `flock`, by `until` at most: none once that passes. A grant after the first, or
/// after `until`, is let go at once, so a thread left blocked holds nothing but its wait. An error
/// is the answer only when every lock failed.
fn first_granted(files: Vec<(usize, File)>, until: Instant) -> std::io::Result<Option<Locked>> {
    let (said, heard) = channel();
    let claimed = Arc::new(AtomicBool::new(false));
    let waits = files.len();
    for (index, file) in files {
        let said = said.clone();
        let first = Arc::clone(&claimed);
        let spawned = std::thread::Builder::new()
            .name(format!("slot {index}"))
            .spawn(move || {
                let granted = loop {
                    match file.lock() {
                        Err(problem) if problem.kind() == ErrorKind::Interrupted => {}
                        granted => break granted,
                    }
                };
                match granted {
                    Ok(()) => {
                        let locked = Locked::new(index, file);
                        if !first.swap(true, Ordering::AcqRel) {
                            let _ = said.send(Ok(locked));
                        }
                    }
                    Err(problem) => {
                        let _ = said.send(Err(problem));
                    }
                }
            });
        if let Err(problem) = spawned {
            claimed.store(true, Ordering::Release);
            return Err(problem);
        }
    }
    drop(said);
    let mut failed = Vec::new();
    loop {
        let left = until.saturating_duration_since(Instant::now());
        match heard.recv_timeout(left) {
            Ok(Ok(locked)) => return Ok(Some(locked)),
            Ok(Err(problem)) => {
                failed.push(problem);
                if failed.len() == waits {
                    return Err(failed.remove(0));
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if !claimed.swap(true, Ordering::AcqRel) {
                    return Ok(None);
                }
                // A thread claimed just before this and is sending its grant: it is the answer,
                // whatever failed lock comes before it.
                while let Ok(answer) = heard.recv() {
                    if let Ok(locked) = answer {
                        return Ok(Some(locked));
                    }
                }
                return Ok(None);
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(failed
                    .pop()
                    .unwrap_or_else(|| std::io::Error::other("no slot's wait answered")));
            }
        }
    }
}

/// How many slots the machine has room for now: `count`, or fewer where its limit of segments
/// less those held by others leaves fewer.
#[derive(Debug, PartialEq)]
struct Room {
    cap: usize,
    /// The machine's limit and the segments in use that no held slot's server made, where both
    /// were read.
    others: Option<(usize, usize)>,
}

impl Room {
    /// The room for `count` slots on a machine of `limit` segments, `creators` the creators of
    /// those in use, `servers` the pids the held slots name. All `count` where either is unread.
    fn of(count: usize, limit: Option<usize>, creators: Option<&[i64]>, servers: &[i64]) -> Room {
        match (limit, creators) {
            (Some(limit), Some(creators)) => {
                let others = creators
                    .iter()
                    .filter(|creator| !servers.contains(creator))
                    .count();
                Room {
                    cap: count.min(limit.saturating_sub(others)),
                    others: Some((limit, others)),
                }
            }
            _ => Room {
                cap: count,
                others: None,
            },
        }
    }

    /// The room said, of `count` slots.
    fn said(&self, count: usize) -> String {
        match self.others {
            Some((limit, others)) => format!(
                "{} (at most {count}; {others} of its {limit} segments are held by servers that \
                 hold no slot)",
                self.cap
            ),
            None => format!("{} (its segments in use unread)", self.cap),
        }
    }

    /// The refusal of a start that met no room and no slot held to wait for.
    fn none(&self, folder: &Path) -> String {
        let (limit, others) = self.others.unwrap_or_default();
        format!(
            "the machine's shared memory has no room for a scratch board's server: {others} of its \
             {limit} segments are held by servers that hold no slot ({}), and none holds one to \
             wait for (`ipcs -m` lists them). Nothing was started",
            folder.display()
        )
    }
}

/// The slots of a machine whose limit of segments is `limit`: half of it, one at least, so the
/// rest stays for the projects' own boards; [`UNREAD`] where it cannot be read.
fn share(limit: Option<usize>) -> usize {
    limit.map_or(UNREAD, |limit| (limit / 2).max(1))
}

/// The machine's limit of System V shared memory segments: `kern.sysv.shmmni`.
#[cfg(target_os = "macos")]
fn limit() -> Option<usize> {
    let mut value = [0_u8; 8];
    let mut size = value.len();
    // SAFETY: a NUL-terminated name, and a buffer of `size` bytes the call writes at most.
    let read = unsafe {
        libc::sysctlbyname(
            c"kern.sysv.shmmni".as_ptr(),
            value.as_mut_ptr().cast(),
            &raw mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if read != 0 {
        return None;
    }
    let number = match size {
        4 => i64::from(i32::from_ne_bytes(value[..4].try_into().ok()?)),
        8 => i64::from_ne_bytes(value),
        _ => return None,
    };
    usize::try_from(number).ok()
}

/// The machine's limit of System V shared memory segments: `kernel.shmmni`.
#[cfg(target_os = "linux")]
fn limit() -> Option<usize> {
    fs::read_to_string("/proc/sys/kernel/shmmni")
        .ok()?
        .trim()
        .parse()
        .ok()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn limit() -> Option<usize> {
    None
}

/// The creators of the segments in use on the machine, as `ipcs -m -p` lists them: none when it
/// cannot be run or fails.
fn segments() -> Option<Vec<i64>> {
    let said = output(command("ipcs").args(["-m", "-p"])).ok()?;
    said.status
        .success()
        .then(|| creators(&String::from_utf8_lossy(&said.stdout)))
}

/// The creators of the segments `ipcs -m -p` lists in `said`: macOS's lines are `m ID KEY MODE
/// OWNER GROUP CPID LPID`, Linux's `SHMID OWNER CPID LPID`; its other lines are headings.
fn creators(said: &str) -> Vec<i64> {
    said.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let (id, creator) = match fields.as_slice() {
                ["m", id, _, _, _, _, creator, _] | [id, _, creator, _] => (id, creator),
                _ => return None,
            };
            id.parse::<i64>().ok()?;
            creator.parse().ok()
        })
        .collect()
}

#[cfg(test)]
mod tests;
