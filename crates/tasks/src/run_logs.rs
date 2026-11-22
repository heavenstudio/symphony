//! A run's log (docs/workflow/RUNS.md, *Logs*): what the command of a run, or of a step of one, wrote on its
//! stdout and stderr, without its terminal escape codes ([`Plain`]), in one file:
//! `logs/YYYY-MM-DD/RUN.log` in the day files' folder ([`crate::runs`]), the UTC day it was asked
//! for. Its recorder ([`crate::cli::recorder`]) reads piped streams as they come, copying each on to
//! where it went ([`Copying`]), or what a caller's regular files gained once the command ended
//! ([`Files`]). Day folders more than [`KEPT_DAYS`] days old go as the day files' own do, under
//! their lock ([`prune`]); [`read`] answers part of one for the Symphony's read
//! ([`symphony_api::RUN_LOG_PATH`]).

use crate::cli::machine_work::recorded;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::os::fd::AsRawFd as _;
use std::os::unix::fs::{FileExt as _, OpenOptionsExt as _};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use symphony_api::{RUN_LOG_MOST, RunLog, run_day};

/// The logs' folder in the day files' folder.
pub const FOLDER: &str = "logs";
/// How many days a day folder of logs is kept after its day.
pub const KEPT_DAYS: u32 = 14;
const DAY_S: f64 = 86_400.0;
/// The most one read of a stream takes.
const CHUNK: usize = 64 * 1024;

/// The log of the run `run` asked for at `at`, relative to the day files' folder.
pub fn name(run: &str, at: f64) -> String {
    format!("{FOLDER}/{}/{run}.log", run_day(at))
}

/// The log `name` made new in the day files' folder `runs`, its day's folder made as needed: never
/// one that is there already, nor anything through a link of that name.
pub fn create(runs: &Path, name: &str) -> io::Result<File> {
    let path = runs.join(name);
    if let Some(day) = path.parent() {
        fs::create_dir_all(day)?;
    }
    OpenOptions::new()
        .append(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

/// Terminal escape codes taken out of a stream as it comes, one cut between two reads included:
/// ECMA-48's control sequences (`ESC [`, its parameters, a final byte), its strings (`ESC ]`,
/// `ESC P`, `ESC X`, `ESC ^` or `ESC _`, to `ESC \` or BEL) and its other escapes (`ESC`, any
/// intermediate bytes, a final byte). Everything else is kept: text, UTF-8 whatever its bytes, and
/// the other control characters, a carriage return and a newline among them. A newline ends a
/// string never ended, so one stray escape never takes the rest of a log.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Plain(State);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Text,
    /// After `ESC`.
    Escape,
    /// After `ESC` and an intermediate byte (0x20 to 0x2F).
    Intermediate,
    /// In `ESC [`, until a final byte (0x40 to 0x7E).
    Control,
    /// In a string, until `ESC \` or BEL.
    Str,
    /// After `ESC` in a string.
    StrEscape,
}

impl Plain {
    /// `bytes`, the next of a stream, appended to `out` without the escape codes in them.
    pub fn strip(&mut self, bytes: &[u8], out: &mut Vec<u8>) {
        const ESC: u8 = 0x1b;
        for &byte in bytes {
            let (next, kept) = match (self.0, byte) {
                (State::Text, ESC) => (State::Escape, false),
                (State::Text, _) => (State::Text, true),
                (State::Escape, b'[') => (State::Control, false),
                (State::Escape, b']' | b'P' | b'X' | b'^' | b'_') => (State::Str, false),
                (State::Escape | State::Intermediate, 0x20..=0x2f) => (State::Intermediate, false),
                (State::Escape | State::Intermediate, 0x30..=0x7e) => (State::Text, false),
                (State::Escape | State::Intermediate | State::Control, ESC) => {
                    (State::Escape, false)
                }
                // A control character in a control sequence acts as it would anywhere.
                (State::Control, 0x00..=0x1f) => (State::Control, true),
                (State::Control, 0x40..=0x7e) => (State::Text, false),
                (State::Control, 0x20..=0x3f) => (State::Control, false),
                (State::Str, 0x07) => (State::Text, false),
                (State::Str | State::StrEscape, ESC) => (State::StrEscape, false),
                (State::StrEscape, b'\\') => (State::Text, false),
                (State::Str | State::StrEscape, b'\n') => (State::Text, true),
                (State::Str | State::StrEscape, _) => (State::Str, false),
                // What no escape takes ends it, and is kept.
                (State::Escape | State::Intermediate | State::Control, _) => (State::Text, true),
            };
            self.0 = next;
            if kept {
                out.push(byte);
            }
        }
    }
}

/// One stream of a command's: read from `from`, the end its command writes to, and copied on to
/// `to`, where it went before.
#[derive(Debug)]
pub struct Stream {
    pub from: File,
    pub to: File,
}

/// A command's streams copied on a thread of their own ([`Copying::start`]) until each ended, its
/// log's bytes counted as they are written.
#[derive(Debug)]
pub struct Copying {
    written: Arc<AtomicU64>,
    ended: mpsc::Receiver<()>,
    thread: std::thread::JoinHandle<()>,
}

impl Copying {
    /// `streams` copied on, each read as it comes and written on whole before the next read, and
    /// plain into `log` in the order they were read. A stream whose `to` can no longer be written
    /// (its reader went) is read no more, so its command's next write there finds that, as it would
    /// have; its log is still written while the others run.
    pub fn start(streams: Vec<Stream>, log: File) -> io::Result<Copying> {
        let written = Arc::new(AtomicU64::new(0));
        let (end, ended) = mpsc::channel();
        let counted = Arc::clone(&written);
        let thread = std::thread::Builder::new()
            .name("run-log".into())
            .spawn(move || {
                copy(streams, log, &counted);
                let _ = end.send(());
            })?;
        Ok(Copying {
            written,
            ended,
            thread,
        })
    }

    /// The bytes its log holds once every stream ended, or `within` passed: what a process that
    /// outlived its command, still holding a stream, writes later is not waited for.
    pub fn drained(&self, within: Duration) -> u64 {
        let _ = self.ended.recv_timeout(within);
        self.written.load(Ordering::SeqCst)
    }

    /// Once every stream ended.
    pub fn join(self) {
        let _ = self.thread.join();
    }
}

/// [`Copying::start`]'s copy, until every stream ended.
fn copy(mut streams: Vec<Stream>, mut log: File, written: &AtomicU64) {
    let mut plain = vec![Plain::default(); streams.len()];
    let mut logging = true;
    let mut buffer = vec![0; CHUNK];
    let mut text = Vec::with_capacity(CHUNK);
    while !streams.is_empty() {
        let mut polls: Vec<libc::pollfd> = streams
            .iter()
            .map(|stream| libc::pollfd {
                fd: stream.from.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        let count = libc::nfds_t::try_from(polls.len()).unwrap_or(libc::nfds_t::MAX);
        // SAFETY: `polls` holds `count` pollfds, alive through the call.
        if unsafe { libc::poll(polls.as_mut_ptr(), count, -1) } < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        let mut ended = Vec::new();
        for (at, poll) in polls.iter().enumerate() {
            if poll.revents == 0 {
                continue;
            }
            let stream = &mut streams[at];
            let read = match stream.from.read(&mut buffer) {
                Ok(0) => {
                    ended.push(at);
                    continue;
                }
                Ok(read) => read,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                    ) =>
                {
                    continue;
                }
                Err(_) => {
                    ended.push(at);
                    continue;
                }
            };
            let bytes = &buffer[..read];
            if forward(&stream.to, bytes).is_err() {
                ended.push(at);
            }
            if logging {
                text.clear();
                plain[at].strip(bytes, &mut text);
                logging = log.write_all(&text).is_ok();
                if logging {
                    written.fetch_add(text.len() as u64, Ordering::SeqCst);
                }
            }
        }
        for at in ended.into_iter().rev() {
            streams.remove(at);
            plain.remove(at);
        }
    }
}

/// The regular files a caller handed a run's command as its stdout and stderr, which the command
/// writes as it would unrecorded, each opened again to read and marked where it ended when the run
/// started: what the command added past that mark is copied into the run's log once it ended
/// ([`Files::copy`]).
#[derive(Debug)]
pub struct Files(Vec<(File, u64)>);

impl Files {
    /// Those of `streams` (copies of the caller's stdout and stderr) that are regular files, a file
    /// both name taken once, each marked at its length now; one that cannot be opened again to read
    /// is left out.
    pub fn mark(streams: impl IntoIterator<Item = File>) -> Files {
        use std::os::unix::fs::MetadataExt as _;
        let mut files: Vec<(File, u64, (u64, u64))> = Vec::new();
        for stream in streams {
            let Ok(meta) = stream.metadata() else {
                continue;
            };
            let identity = (meta.dev(), meta.ino());
            if !meta.file_type().is_file() || files.iter().any(|(.., seen)| *seen == identity) {
                continue;
            }
            let reader = reopened(&stream).ok().filter(|reader| {
                reader
                    .metadata()
                    .is_ok_and(|read| (read.dev(), read.ino()) == identity)
            });
            if let Some(reader) = reader {
                files.push((reader, meta.len(), identity));
            }
        }
        Files(
            files
                .into_iter()
                .map(|(file, mark, _)| (file, mark))
                .collect(),
        )
    }

    /// What each file holds past its mark now, plain, appended to `log`, one file after the other:
    /// the bytes written. A file shorter than its mark gives nothing.
    pub fn copy(&self, mut log: &File) -> u64 {
        let mut buffer = vec![0; CHUNK];
        let mut text = Vec::with_capacity(CHUNK);
        let mut written = 0;
        for (file, mark) in &self.0 {
            let end = file.metadata().map_or(0, |meta| meta.len());
            let mut plain = Plain::default();
            let mut at = *mark;
            while at < end {
                let want = usize::try_from(end - at).map_or(CHUNK, |left| left.min(CHUNK));
                let read = match file.read_at(&mut buffer[..want], at) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => read,
                };
                at += read as u64;
                text.clear();
                plain.strip(&buffer[..read], &mut text);
                if log.write_all(&text).is_err() {
                    return written;
                }
                written += text.len() as u64;
            }
        }
        written
    }
}

/// A caller's [`Files`] and the log they are copied into, once, by whichever asks first
/// ([`Ending::copy`]): on macOS the recorder's thread that sees the command end, as it does, so a
/// caller that writes to its file again at once (its next command's `>` cutting it) seldom comes
/// first.
#[derive(Debug)]
pub struct Ending {
    files: Files,
    log: File,
    written: OnceLock<u64>,
}

impl Ending {
    pub fn new(files: Files, log: File) -> Ending {
        Ending {
            files,
            log,
            written: OnceLock::new(),
        }
    }

    /// The bytes the log holds: the files' copy the first time, what that copy wrote after, a
    /// caller at the same time waiting for it.
    pub fn copy(&self) -> u64 {
        *self.written.get_or_init(|| self.files.copy(&self.log))
    }
}

/// `file` opened again, to read: a caller's stream is most often open to write alone.
fn reopened(file: &File) -> io::Result<File> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStringExt as _;
        let mut path = vec![0_u8; usize::try_from(libc::PATH_MAX).unwrap_or(1024)];
        // SAFETY: F_GETPATH writes at most PATH_MAX bytes, a NUL among them, into `path`.
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETPATH, path.as_mut_ptr()) } == -1 {
            return Err(io::Error::last_os_error());
        }
        let length = path
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(path.len());
        path.truncate(length);
        let path = std::path::PathBuf::from(std::ffi::OsString::from_vec(path));
        #[cfg(test)]
        tests::before_reopen(&path);
        crate::runs::regular(&path, OpenOptions::new().read(true))
    }
    #[cfg(not(target_os = "macos"))]
    {
        // The proc link names the live descriptor, including an unlinked or anonymous file.
        // Follow that kernel-owned link directly: its target text need not be an openable path.
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(format!("/proc/self/fd/{}", file.as_raw_fd()))
    }
}

/// Where a run's log is written from: its command's streams copied as they come ([`Copying`]), or
/// its caller's files once its command ended ([`Ending`]).
#[derive(Debug)]
pub enum Source {
    Copying(Copying),
    Files(Arc<Ending>),
}

impl Source {
    /// The caller's files a source copies from, `files` and `log`.
    pub fn files(files: Files, log: File) -> Source {
        Source::Files(Arc::new(Ending::new(files, log)))
    }

    /// The bytes its log holds once its command ended: a copy's once its streams ended or `within`
    /// passed, the files' once what they gained is copied.
    pub fn ended(&self, within: Duration) -> u64 {
        match self {
            Source::Copying(copying) => copying.drained(within),
            Source::Files(ending) => ending.copy(),
        }
    }

    /// Its files' copy, for the thread that sees its command end to make.
    pub fn ending(&self) -> Option<Arc<Ending>> {
        match self {
            Source::Copying(_) => None,
            Source::Files(ending) => Some(Arc::clone(ending)),
        }
    }

    /// Once a copy's streams ended.
    pub fn join(self) {
        if let Source::Copying(copying) = self {
            copying.join();
        }
    }
}

/// `bytes` written whole to `to`, waiting while it cannot take them yet.
fn forward(mut to: &File, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        match to.write(bytes) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(wrote) => bytes = &bytes[wrote..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                let mut poll = libc::pollfd {
                    fd: to.as_raw_fd(),
                    events: libc::POLLOUT,
                    revents: 0,
                };
                // SAFETY: one pollfd, alive through the call.
                unsafe { libc::poll(&raw mut poll, 1, -1) };
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Each day folder of the logs in the day files' folder `runs` of a day more than `kept_days` days
/// before `now`'s removed, with every log in it; nothing else there, and no link of such a name.
pub(crate) fn prune(runs: &Path, now: f64, kept_days: u32) {
    let oldest = run_day(now - f64::from(kept_days) * DAY_S);
    let Ok(entries) = fs::read_dir(runs.join(FOLDER)) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if crate::runs::date(name)
            && name < oldest.as_str()
            && entry.file_type().is_ok_and(|kind| kind.is_dir())
        {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// What a read of a log asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    /// Its last bytes, this many.
    Tail(u64),
    /// Its bytes from one place, to another or its end.
    Range { from: u64, to: Option<u64> },
}

/// Why a log was not read.
#[derive(Debug)]
pub enum Unread {
    /// The run named is no run's id: 32 hex digits.
    NotARun,
    /// No log of that run is there to read: none made, gone with its day, or not a regular file.
    Missing(String),
    /// It is there, and its read failed.
    Failed(io::Error),
}

/// The part `asked` of the log of `run` in the day files' folder `runs`, never more than
/// [`RUN_LOG_MOST`] bytes: read only from a regular file in a day folder of the logs, never through
/// a link, and never waiting on a FIFO.
pub fn read(runs: &Path, run: &str, asked: Asked) -> Result<RunLog, Unread> {
    if !recorded::nonce(run) {
        return Err(Unread::NotARun);
    }
    let (log, file) = find(runs, run)?;
    let size = file.metadata().map_err(Unread::Failed)?.len();
    let (from, to) = match asked {
        Asked::Tail(bytes) => (size.saturating_sub(bytes.min(RUN_LOG_MOST)), size),
        Asked::Range { from, to } => {
            let from = from.min(size);
            let to = to.unwrap_or(size).clamp(from, size);
            (from, to.min(from.saturating_add(RUN_LOG_MOST)))
        }
    };
    let mut bytes = vec![
        0;
        usize::try_from(to - from)
            .map_err(io::Error::other)
            .map_err(Unread::Failed)?
    ];
    file.read_exact_at(&mut bytes, from)
        .map_err(Unread::Failed)?;
    Ok(RunLog {
        run: run.to_owned(),
        log,
        size,
        from,
        to,
        text: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

/// The log of `run` in `runs`, by its name and opened: in the newest day folder that holds one.
fn find(runs: &Path, run: &str) -> Result<(String, File), Unread> {
    let logs = runs.join(FOLDER);
    let missing = || Unread::Missing(format!("no log of the run {run}"));
    let entries = match fs::read_dir(&logs) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(missing()),
        Err(error) => return Err(Unread::Failed(error)),
    };
    let mut days: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| crate::runs::date(name))
        .collect();
    days.sort_unstable_by(|a, b| b.cmp(a));
    for day in days {
        let path = logs.join(&day).join(format!("{run}.log"));
        match crate::runs::regular(&path, OpenOptions::new().read(true)) {
            Ok(file) => return Ok((format!("{FOLDER}/{day}/{run}.log"), file)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(Unread::Missing(format!(
                    "the log of the run {run} is not a regular file in the logs' folder: {error}"
                )));
            }
        }
    }
    Err(missing())
}

#[cfg(test)]
#[path = "run_logs_tests.rs"]
mod tests;
