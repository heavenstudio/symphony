//! What the board's own recorder (`tasks run`, [`crate::cli::run`]) keeps of a build or test it
//! wraps, in the work folder ([`folder`]): `operations/ID.json`, one operation each, rewritten whole
//! (a file of its own renamed over it) when it is queued, starts, beats and finishes, beside
//! `operations/ID.lease`, which its recorder holds locked for as long as it runs; and
//! `history.jsonl`, every finished operation, one a line, oldest first, each appended in one
//! write, until it reaches [`ROTATE_BYTES`] and becomes `history.1.jsonl` in place of the one
//! before. The Symphony reads the operations as native evidence ([`read`],
//! [`Inventory::recorded`]) beside a project's own reader; none of it is on the board.
//!
//! An operation is kept in the history before its record says it finished. An unfinished
//! operation whose lease nobody holds lost its recorder: a reader counts it as uncertainty, never
//! as work or as done, until the next recorder's [`sweep`] keeps it and finishes it without an
//! exit. Finished operations stay [`RETAIN`] seconds for a reader to see them finish, then go.
//!
//! A reader that asks often keeps what it read ([`Kept`]) and reads again only the files that are
//! not the ones it read.

use super::native::{Inventory, MAX_ROWS};
use super::{Kind, Operation, State};
use crate::cli::Env;
use crate::sys::FileLock;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

mod kept;

pub use kept::{Kept, Reads};

/// The work folder's name in `symphony/tasks/` of the git common dir.
pub const FOLDER: &str = "work";
/// Names the work folder in place of the board's: a test's own.
pub const FOLDER_VAR: &str = "TASKS_WORK";
/// The operations' folder in the work folder.
pub const OPERATIONS: &str = "operations";
/// Finished operations, oldest first, one a line.
pub const HISTORY: &str = "history.jsonl";
/// The history before the current one, replaced at each rotation.
pub const ROTATED: &str = "history.1.jsonl";
/// The size at which the history rotates: the two files hold between this and twice this, some
/// 3,000 to 6,000 operations.
pub const ROTATE_BYTES: u64 = 2 * 1024 * 1024;
/// Held by the one sweep or rotation at a time; nothing waits for it.
pub const LOCK: &str = "history.lock";
/// How long a finished operation stays in the operations' folder, in seconds.
pub const RETAIN: f64 = 3600.0;
/// Names, in a recorded command's environment, the operation its recorder runs it as: a recorder
/// that command starts records that operation as its parent.
pub const PARENT_VAR: &str = "TASKS_RUN_PARENT";
/// Names, in the environment of a command recorded as a run or a step, its pipeline: a recorder
/// that command starts with `--step` records a step of that pipeline. Removed from the environment
/// of any other recorded command.
pub const PIPELINE_VAR: &str = "SYMPHONY_RUN_PIPELINE";
/// What started a run when it was no session: `release`, `land`, `on_review`, set by what starts it.
pub const TRIGGER_VAR: &str = "SYMPHONY_RUN_TRIGGER";
/// The longest record file read.
const MAX_BYTES: u64 = 65_536;
/// The longest text field kept.
pub const MAX_TEXT: usize = 4096;

/// Which column work goes to while it runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Build,
    Test,
}

/// Where an operation is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// Recorded, its command not started yet.
    Queued,
    /// Its command started.
    Running,
    /// Its command ended, or its recorder did before it could say how.
    Finished,
}

/// One operation, live in the operations' folder and kept in the history alike. Times are seconds
/// since the epoch on the recorder's clock: shown and kept, never a reader's freshness.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// Always `1`, a field added later included: a reader ignores a field it does not know, and
    /// takes a record of another version for one it cannot read, its machine unknown until swept.
    pub version: u8,
    /// 32 hex digits, its file's name.
    pub operation_id: String,
    /// The recorded operation whose command started this recorder, if one did.
    pub parent_id: Option<String>,
    pub phase: Phase,
    /// The recipe or the command line.
    pub label: String,
    pub state: Step,
    /// The session it ran for (`--as` or `TASKS_SESSION`).
    pub handle: Option<String>,
    /// The one task that session held when it began (`--task` names one); none when it held none
    /// or several.
    pub task: Option<String>,
    /// The folder its command ran in.
    pub checkout: String,
    pub branch: Option<String>,
    pub commit: Option<String>,
    /// The installation's identity, when the machine folder has one.
    pub machine_id: Option<String>,
    pub host: String,
    /// The recorder's process.
    pub pid: u32,
    /// Its command's, once started.
    pub child_pid: Option<u32>,
    pub queued_at: f64,
    pub started_at: Option<f64>,
    pub ended_at: Option<f64>,
    /// When its recorder last said it still runs.
    pub beat_at: f64,
    pub exit_code: Option<i32>,
    /// The signal that ended its command, if one did.
    pub signal: Option<i32>,
    pub reason: String,
    /// What makes it a run, or a step of one, which the day files keep a line of
    /// ([`crate::runs`]); left out of an operation of the work folder alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<Run>,
    /// The latest reading of its command's processes its recorder wrote with a beat, where it
    /// watches them (macOS); left out before the first, and by a recorder that does not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reading: Option<CpuReading>,
}

/// What a recorder that watches its command read of the command's processes while they ran.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CpuReading {
    /// When it read them, in seconds since the epoch.
    pub at: f64,
    /// The most CPU seconds any reading so far found they had taken, kept to the millisecond.
    pub cpu_s: f64,
    /// Whether a reading so far knew it missed some ([`Run::cpu_partial`]).
    pub partial: bool,
    /// How many processes this reading read: the command's and each under it.
    pub processes: u32,
}

impl CpuReading {
    /// Whether it reads as a recorder writes one.
    fn valid(&self) -> bool {
        self.at.is_finite() && self.cpu_s.is_finite() && self.cpu_s >= 0.0 && self.processes > 0
    }
}

/// An operation's part as a run or a step: what its line in the day files says beside the record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Run {
    /// Its run's pipeline (`--pipeline`, or for a step, that of the run it ran inside).
    pub pipeline: String,
    /// A step's name (`--step`); none for a run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// [`TRIGGER_VAR`], else its session's handle, else `owner`.
    pub trigger: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// The tree of the commit it checked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree: Option<String>,
    /// Whether the checkout differed from that commit when it began.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirty: Option<bool>,
    /// The earlier run it repeats on the same tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rerun_of: Option<String>,
    /// The CPU seconds of its command and the children it reaped, once it finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_s: Option<f64>,
    /// Whether those seconds are known short ([`symphony_api::RunLine::cpu_partial`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_partial: Option<bool>,
    /// The day files' folder its lines go to, an absolute path: where a sweep that finishes it
    /// writes its end line, whatever folder the sweeping recorder's own lines go to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs: Option<String>,
    /// Its log, relative to `runs` ([`crate::run_logs`]): where its command's output is kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log: Option<String>,
    /// The bytes its log held as it finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_bytes: Option<u64>,
    /// Once finished, the line its runner refused its command line with, when its log says that
    /// and no work ([`crate::runs::refused`]): it built and ran nothing, so it is no evidence about
    /// its commit, and a stamp's reader passes it over (`crate::stamps::read`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
}

impl Run {
    /// Whether it reads as a recorder writes one.
    fn valid(&self) -> bool {
        symphony_api::run_name(&self.pipeline)
            && self.step.as_deref().is_none_or(symphony_api::run_name)
            && super::projection::identity(&self.trigger)
            && [
                &self.project,
                &self.tree,
                &self.runs,
                &self.log,
                &self.refused,
            ]
            .iter()
            .all(|text| text.as_ref().is_none_or(|text| text.len() <= MAX_TEXT))
            && self.log.as_deref().is_none_or(|log| {
                Path::new(log)
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_)))
            })
            && self
                .runs
                .as_deref()
                .is_none_or(|runs| Path::new(runs).is_absolute())
            && self.rerun_of.as_deref().is_none_or(nonce)
            && self.cpu_s.is_none_or(|cpu| cpu.is_finite() && cpu >= 0.0)
    }
}

/// 32 hex digits: an operation's id.
pub fn nonce(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// A new operation id, from the system's randomness.
pub fn new_id() -> io::Result<String> {
    let mut bytes = [0; 16];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// `text` cut to [`MAX_TEXT`] bytes, at a character's edge.
pub fn bounded(text: &str) -> String {
    let mut end = text.len().min(MAX_TEXT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default().to_string()
}

impl Record {
    /// Whether it reads as a recorder writes one, in a file named `stem`.
    pub fn valid(&self, stem: &str) -> bool {
        let times = [
            Some(self.queued_at),
            self.started_at,
            self.ended_at,
            Some(self.beat_at),
        ];
        let steps = match self.state {
            Step::Queued => {
                self.started_at.is_none()
                    && self.ended_at.is_none()
                    && self.exit_code.is_none()
                    && self.signal.is_none()
            }
            Step::Running => {
                self.started_at.is_some()
                    && self.ended_at.is_none()
                    && self.exit_code.is_none()
                    && self.signal.is_none()
            }
            Step::Finished => {
                self.ended_at.is_some() && !(self.exit_code.is_some() && self.signal.is_some())
            }
        };
        let name = |text: &Option<String>| text.as_deref().is_none_or(super::projection::identity);
        self.version == 1
            && nonce(&self.operation_id)
            && self.operation_id == stem
            && self
                .parent_id
                .as_deref()
                .is_none_or(|id| nonce(id) && id != self.operation_id)
            && name(&self.handle)
            && name(&self.task)
            && [&self.label, &self.checkout, &self.host, &self.reason]
                .iter()
                .all(|text| text.len() <= MAX_TEXT)
            && [&self.branch, &self.commit]
                .iter()
                .all(|text| text.as_ref().is_none_or(|text| text.len() <= MAX_TEXT))
            && times.iter().flatten().all(|time| time.is_finite())
            && self.pid != 0
            && self.child_pid != Some(0)
            && self.run.as_ref().is_none_or(Run::valid)
            && self
                .reading
                .as_ref()
                .is_none_or(|reading| reading.valid() && self.started_at.is_some())
            && steps
    }

    /// How long its command ran, once it finished.
    pub fn duration(&self) -> Option<f64> {
        Some(self.ended_at? - self.started_at?)
    }

    /// The operation a reader shows for it on `machine_id`: its command's phase, queued or running,
    /// since it was queued or started. Its recorder's beats change it, so each is new evidence.
    fn operation(&self, machine_id: Option<&str>) -> Operation {
        let reason = match (self.state, self.started_at) {
            (Step::Queued, _) => "recorded; its command not started yet".to_string(),
            (Step::Running, Some(started)) => format!(
                "running; its recorder said so {:.0} s after it started",
                (self.beat_at - started).max(0.0)
            ),
            _ => self.reason.clone(),
        };
        Operation {
            machine_id: machine_id.map(str::to_owned),
            operation_id: Some(self.operation_id.clone()),
            parent_id: self.parent_id.clone(),
            handle: self.handle.clone(),
            task: self.task.clone(),
            checkout: self.checkout.clone(),
            recipe: self.label.clone(),
            kind: match self.phase {
                Phase::Build => Kind::Build,
                Phase::Test => Kind::Test,
            },
            state: if self.state == Step::Queued {
                State::Queued
            } else {
                State::Running
            },
            resource: "tasks run".into(),
            reason,
            phase_since: Some(self.started_at.unwrap_or(self.queued_at)),
        }
    }
}

/// Where `env`'s board keeps its recorded work: the folder `TASKS_WORK` names, from its working
/// directory when it is no whole path (a recorder apart from its command works elsewhere); for a board
/// `TASKS_SERVER` names without it, none; else `symphony/tasks/work/` in the git common dir of its
/// working directory, `work.<database>/` for a database other than the live one.
pub fn folder(env: &Env) -> Option<PathBuf> {
    if let Some(named) = env.var(FOLDER_VAR) {
        return Some(env.cwd.join(named));
    }
    if env.var("TASKS_SERVER").is_some() {
        return None;
    }
    let common = crate::git::common_dir(&env.cwd).ok()?;
    let name = match env.var("TASKS_DATABASE") {
        Some(database) if database != crate::DATABASE => format!("{FOLDER}.{database}"),
        _ => FOLDER.to_string(),
    };
    Some(common.join(crate::server::FOLDER).join(name))
}

/// The live board's work folder in the git common dir `common`: what the Symphony reads.
pub fn in_common(common: &Path) -> PathBuf {
    common.join(crate::server::FOLDER).join(FOLDER)
}

/// Whether the lease at `path` is held: by a live recorder, since a lock ends with its process.
/// None when the lease cannot be asked.
fn held(path: &Path) -> Option<bool> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Some(false),
        Err(_) => return None,
    };
    match file.try_lock_shared() {
        Ok(()) => {
            // A forked child may still hold this description after our file closes.
            file.unlock().ok()?;
            Some(false)
        }
        Err(TryLockError::WouldBlock) => Some(true),
        Err(TryLockError::Error(_)) => None,
    }
}

/// The process the command of the operation whose lease is at `path` is, as its recorder wrote it
/// there (`PID` and a newline); none when it wrote none.
fn command_of(path: &Path) -> Option<u32> {
    let mut text = String::new();
    File::open(path)
        .ok()?
        .take(16)
        .read_to_string(&mut text)
        .ok()?;
    text.strip_suffix('\n')?.parse().ok().filter(|pid| *pid > 0)
}

/// Whether `pid` is a process now (this user's or another's).
fn running(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 only asks whether the process is there.
    let there = unsafe { libc::kill(pid, 0) } == 0;
    there || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Whether an operation in the work folder `folder` other than `ours`, asked for by `before`, may
/// yet end as a run `matches` takes: its recorder holds its lease, made by then, its command is
/// not still running (one that runs now had not ended by `before`, so it is no run that ended by
/// then), and its record is not written yet, or says it has not finished and `matches` takes it.
/// A lease made after `before` is later work's. Its command is the process its lease names, else
/// its record's.
pub fn pending(folder: &Path, ours: &str, before: f64, matches: impl Fn(&Record) -> bool) -> bool {
    let Ok(entries) = fs::read_dir(folder.join(OPERATIONS)) else {
        return false;
    };
    for entry in entries.flatten() {
        let lease = entry.path();
        let stem = lease
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| nonce(stem) && *stem != ours);
        if stem.is_none()
            || lease
                .extension()
                .is_none_or(|extension| extension != "lease")
            || changed(&lease).is_none_or(|made| made > before)
            || held(&lease) != Some(true)
        {
            continue;
        }
        let command = command_of(&lease);
        if command.is_some_and(running) {
            continue;
        }
        match record(&lease.with_extension("json")) {
            Reading::Failed => return true,
            Reading::Record(found, _)
                if found.state != Step::Finished
                    && found.queued_at <= before
                    && command.or(found.child_pid).is_none_or(|pid| !running(pid))
                    && matches(&found) =>
            {
                return true;
            }
            Reading::Record(..) | Reading::NotOne => {}
        }
    }
    false
}

/// What one read of a record file found.
#[derive(Debug)]
enum Reading {
    /// A record, with its bytes.
    Record(Box<Record>, Vec<u8>),
    /// A file read whole that is no record this build reads: too long, not a file, not one, or of
    /// another version. Its writer renamed it there whole, so no later read makes it one.
    NotOne,
    /// Nothing read: the file was not there, or opening or reading it failed, as it may not later.
    Failed,
}

/// The record at `path`, read once.
fn record(path: &Path) -> Reading {
    let Ok(file) = File::open(path) else {
        return Reading::Failed;
    };
    match file.metadata() {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Reading::NotOne,
        Err(_) => return Reading::Failed,
    }
    let mut bytes = Vec::new();
    if file.take(MAX_BYTES + 1).read_to_end(&mut bytes).is_err() {
        return Reading::Failed;
    }
    let stem = path.file_stem().and_then(|stem| stem.to_str());
    match (crate::objects::parse::<Record>(&bytes), stem) {
        (Some(found), Some(stem)) if bytes.len() as u64 <= MAX_BYTES && found.valid(stem) => {
            Reading::Record(Box::new(found), bytes)
        }
        _ => Reading::NotOne,
    }
}

/// The record files in `operations`, by name; none for a folder that is not there.
fn names(operations: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(operations) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let path = entry?.path();
        let visible = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| !name.starts_with('.'));
        if visible
            && path
                .extension()
                .is_some_and(|extension| extension == "json")
        {
            paths.push(path);
        }
    }
    crate::order::sort_by(&mut paths, &Ord::cmp);
    Ok(paths)
}

/// One read of a work folder's operations: each running or queued one whose recorder holds its
/// lease, each finished one, with the bytes it was read as; uncertain when any could not be read,
/// lost its recorder, or more were there than a read takes.
#[derive(Debug, Default, Clone)]
pub struct Found {
    pub records: Vec<(Record, Vec<u8>)>,
    pub uncertain: bool,
}

impl Found {
    /// Whether it found nothing at all, certain of it.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty() && !self.uncertain
    }
}

/// The operations in the work folder `folder`, read and never changed. A folder not made yet holds
/// none: the recorder makes it before its first record.
pub fn read(folder: &Path) -> Found {
    let mut found = Found::default();
    let Ok(paths) = names(&folder.join(OPERATIONS)) else {
        found.uncertain = true;
        return found;
    };
    for path in paths {
        if found.records.len() >= MAX_ROWS {
            found.uncertain = true;
            break;
        }
        let Reading::Record(first, bytes) = record(&path) else {
            found.uncertain = true;
            continue;
        };
        if first.state == Step::Finished {
            found.records.push((*first, bytes));
            continue;
        }
        match held(&path.with_extension("lease")) {
            Some(true) => found.records.push((*first, bytes)),
            // A recorder that finished between the two reads left its lease with its record done.
            Some(false) => match record(&path) {
                Reading::Record(again, bytes) if again.state == Step::Finished => {
                    found.records.push((*again, bytes));
                }
                _ => found.uncertain = true,
            },
            None => found.uncertain = true,
        }
    }
    found
}

impl Inventory {
    /// The operations `found` read, on `machine_id`: each bound to the task its recorder recorded,
    /// none included, which no receiver attribution rewrites.
    pub fn recorded(found: &Found, machine_id: Option<&str>) -> Result<Self, String> {
        if machine_id.is_some_and(|id| !super::projection::identity(id)) {
            return Err("native inventory trusted identity is invalid".into());
        }
        let mut inventory = Inventory {
            machine_id: machine_id.map(str::to_owned),
            uncertain: found.uncertain,
            operations: Vec::new(),
            finished: Vec::new(),
            revisions: Default::default(),
            recorded: Default::default(),
        };
        for (record, bytes) in &found.records {
            let id = record.operation_id.clone();
            if inventory
                .revisions
                .insert(id.clone(), bytes.clone())
                .is_some()
            {
                return Err("native inventory repeats an operation nonce".into());
            }
            inventory.recorded.insert(id);
            let operation = record.operation(machine_id);
            if record.state == Step::Finished {
                inventory.finished.push(operation);
            } else {
                inventory.operations.push(operation);
            }
        }
        Ok(inventory)
    }
}

/// Write `record` to `path` whole: to a file of this write's own beside it, renamed over it, so a
/// reader never meets half of one.
pub fn write(path: &Path, record: &Record) -> io::Result<()> {
    let mut text = serde_json::to_string(record).map_err(io::Error::other)?;
    text.push('\n');
    replace(path, text.as_bytes())
}

/// `bytes` at `path`, through a file of this write's own renamed over it.
fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let folder = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let partial = folder.join(format!(
        ".{name}.{}.{}",
        std::process::id(),
        WRITES.fetch_add(1, Ordering::Relaxed)
    ));
    let written = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .and_then(|mut file| file.write_all(bytes))
        .and_then(|()| fs::rename(&partial, path));
    if written.is_err() {
        let _ = fs::remove_file(&partial);
    }
    written
}

/// The work folder's lock when nobody else holds it: one sweep or rotation at a time, and none
/// waits for another.
fn try_locked(folder: &Path) -> io::Result<Option<FileLock>> {
    fs::create_dir_all(folder)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(folder.join(LOCK))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(FileLock(file))),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

/// Every operation the history of `folder` keeps, oldest first, each once: [`ROTATED`], then
/// [`HISTORY`]. A line that is not a record (one a write was cut short in) is passed over.
pub fn history(folder: &Path) -> io::Result<Vec<Record>> {
    let mut seen = BTreeSet::new();
    let mut kept = Vec::new();
    for name in [ROTATED, HISTORY] {
        let bytes = match fs::read(folder.join(name)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for line in bytes.split(|byte| *byte == b'\n') {
            if let Some(record) = crate::objects::parse::<Record>(line)
                && seen.insert(record.operation_id.clone())
            {
                kept.push(record);
            }
        }
    }
    Ok(kept)
}

/// Every run the work folder `folder` holds, each once: the history's, and the operations still in
/// the folder ([`read`]), which say more of one the history keeps too. A folder not there holds
/// none. What the `tests` stamp is read from, by the task page and the land alike.
pub fn runs(folder: &Path) -> Vec<Record> {
    let mut found: Vec<Record> = history(folder).unwrap_or_default();
    for (live, _) in read(folder).records {
        match found
            .iter_mut()
            .find(|run| run.operation_id == live.operation_id)
        {
            Some(kept) => *kept = live,
            None => found.push(live),
        }
    }
    found
}

/// Keep `finished` in the history of `folder`: one line appended in one write, which waits for
/// no other recorder; a history that reached [`ROTATE_BYTES`] then becomes [`ROTATED`], unless
/// another recorder is rotating or sweeping now.
pub fn keep(folder: &Path, finished: &Record) -> io::Result<()> {
    fs::create_dir_all(folder)?;
    let mut line = serde_json::to_vec(finished).map_err(io::Error::other)?;
    line.push(b'\n');
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(folder.join(HISTORY))?;
    file.write_all(&line)?;
    if file.metadata()?.len() >= ROTATE_BYTES {
        rotate(folder, &file)?;
    }
    Ok(())
}

/// The history `appended` was appended to, renamed over the rotated one, unless the lock is held or
/// another recorder rotated it first.
fn rotate(folder: &Path, appended: &File) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt as _;
    let Some(_held) = try_locked(folder)? else {
        return Ok(());
    };
    let path = folder.join(HISTORY);
    let (now, ours) = (fs::metadata(&path)?, appended.metadata()?);
    if (now.dev(), now.ino()) == (ours.dev(), ours.ino()) {
        fs::rename(&path, folder.join(ROTATED))?;
    }
    Ok(())
}

/// When `path` last changed, in seconds since the epoch.
fn changed(path: &Path) -> Option<f64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    Some(
        modified
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs_f64(),
    )
}

/// The visible files in `folder` and the hidden ones a write of its own left there.
fn entries(folder: &Path) -> io::Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let (mut visible, mut hidden) = (Vec::new(), Vec::new());
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') {
            hidden.push(path);
        } else {
            visible.push(path);
        }
    }
    crate::order::sort_by(&mut visible, &Ord::cmp);
    Ok((visible, hidden))
}

/// Tidy the work folder `folder` at `now`, under its lock, or not at all while another recorder
/// tidies it: each unfinished operation whose recorder is gone is kept in the history, then
/// finished without an exit, as its recorder last said it ran. A record no live recorder holds that
/// read whole is not one this build reads (too long, malformed, or of another version) goes at
/// once, with its lease; one that could not be read is left as it is for a later sweep. A finished
/// operation changed more than [`RETAIN`] ago goes, as does anything else
/// there older than that which no live recorder holds: a write's own file left by a writer that
/// ended, a lease with no record. A lost run or step also gets its end line, as it is kept, in the
/// day files its record names, where its start line went. What this sweep finished.
pub fn sweep(folder: &Path, now: f64) -> io::Result<Vec<Record>> {
    let operations = folder.join(OPERATIONS);
    if !operations.is_dir() {
        return Ok(Vec::new());
    }
    let Some(_held) = try_locked(folder)? else {
        return Ok(Vec::new());
    };
    let old = |path: &Path| changed(path).is_some_and(|at| now - at > RETAIN);
    for place in [folder, operations.as_path()] {
        for partial in entries(place)?.1 {
            if old(&partial) {
                let _ = fs::remove_file(&partial);
            }
        }
    }
    let mut lost = Vec::new();
    for path in entries(&operations)?.0 {
        let lease = path.with_extension("lease");
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("json") => {}
            Some("lease") => {
                if !path.with_extension("json").exists() && held(&path) == Some(false) && old(&path)
                {
                    let _ = fs::remove_file(&path);
                }
                continue;
            }
            _ => continue,
        }
        // A live recorder's, or one that cannot be asked.
        if held(&lease) != Some(false) {
            continue;
        }
        let mut found = match record(&path) {
            Reading::Record(found, _) => *found,
            // No recorder holds it and no reader reads it, now or later: it goes at once, so the
            // machine it makes uncertain is certain again.
            Reading::NotOne => {
                let _ = fs::remove_file(&path);
                let _ = fs::remove_file(&lease);
                continue;
            }
            // One that could not be read now is left for a sweep that can.
            Reading::Failed => continue,
        };
        if !lease.exists() {
            // Its recorder finished it and let its lease go.
            if old(&path) {
                let _ = fs::remove_file(&path);
            }
            continue;
        }
        match found.state {
            // It ended between saying so and letting its lease go.
            Step::Finished => {
                let _ = fs::remove_file(&lease);
            }
            Step::Queued | Step::Running => {
                found.state = Step::Finished;
                found.ended_at = Some(found.beat_at);
                found.exit_code = None;
                found.signal = None;
                found.reason = "its recorder ended before saying how its command did".into();
                keep(folder, &found)?;
                let runs = found.run.as_ref().and_then(|run| run.runs.as_deref());
                if let (Some(runs), Some(line)) = (runs, crate::runs::line(&found)) {
                    let wait = crate::runs::BRIEF_WAIT;
                    let _ = crate::runs::append(Path::new(runs), &line, now, wait);
                }
                write(&path, &found)?;
                let _ = fs::remove_file(&lease);
                lost.push(found);
            }
        }
    }
    Ok(lost)
}
