//! What each run covered, in the repository's git common dir: `symphony/release/runs/ID.json`,
//! one file per run written whole, and each check's log under `logs/ID/`. The newest
//! [`KEPT_RUNS`] records and the logs of the newest [`KEPT_LOGS`] runs stay; a run removes the
//! rest once it wrote its own. `--status` reads them and the release tags, and writes nothing.
//! While a run holds `run.lock`, `running.json` says which run it is, when its time ends and, while
//! a check of it runs, that check's process group ([`Checking`]): what a later run reads when the
//! run was killed while its check ran on. Its record so far is kept beside its logs
//! (`logs/ID/run.json`) until it writes the whole one, so a run that ended without recording itself
//! (killed, or the machine stopped) is recorded as interrupted by the next run that goes
//! ([`recover`]), and its logs stay with it.
//!
//! A build older than the rename to `just release` kept all of it in `symphony/nightly/`, which
//! the first run of a build that knew the new name moved here, and wrote records of version 1,
//! whose kinds were `nightly` and, for an out-of-cycle release, `release`: a record of version 1
//! reads as today's ([`renamed`]). That folder, if anything makes it again, is neither read nor
//! moved.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::board;
use super::checks::Ran;
use super::crates::Publication;
use super::git::short;
use super::group::Stamp;
use super::tags::Release;
use symphony::clock::stamp_at;

/// The records a run keeps.
pub(crate) const KEPT_RUNS: usize = 60;
/// The runs whose logs a run keeps.
pub(crate) const KEPT_LOGS: usize = 14;
/// A release run older than this is overdue: the schedule runs once a day.
const OVERDUE_SECONDS: f64 = 36.0 * 3600.0;
/// The result of a run that ended without recording itself, as a later run recorded it.
pub(crate) const INTERRUPTED: &str = "interrupted";
/// A run's record so far, in its log folder, while it runs.
const SO_FAR: &str = "run.json";
/// The version of the records and the lock's word this build writes: 2 since the rename to `just
/// release`, whose kinds are `release`, `out-of-cycle` and `refresh`. Version 1 said `nightly` for
/// `release` and `release` for `out-of-cycle` ([`renamed`]).
pub(crate) const VERSION: u32 = 2;
/// The folder's name in `symphony/` before the rename, under which a record of version 1 names its
/// logs.
const LEGACY: &str = "nightly";
/// The lock one run at a time holds, in the folder.
const LOCK: &str = "run.lock";

/// A run's record. A record written out names what its run knows and leaves the rest as a run
/// that reached none of it has it ([`Default`]), so a new field is written where it is known.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Run {
    pub(crate) version: u32,
    pub(crate) id: String,
    /// `release`, `out-of-cycle` for an out-of-cycle release of a named commit, or `refresh` for a
    /// `--refresh`.
    pub(crate) kind: String,
    /// The candidate.
    pub(crate) commit: String,
    /// The remote's main as fetched.
    pub(crate) tip: String,
    pub(crate) started_at: f64,
    /// When it ended, by the wall clock: a sleep of the machine under it is counted.
    pub(crate) ended_at: f64,
    /// `green`, `red`, `unchanged` (the candidate was released already), `error` (it stopped
    /// short) or [`INTERRUPTED`] (it ended without recording itself, and a later run recorded it).
    pub(crate) result: String,
    #[serde(default)]
    pub(crate) error: Option<String>,
    /// The release before the candidate.
    #[serde(default)]
    pub(crate) previous: Option<String>,
    #[serde(default)]
    pub(crate) checks: Vec<Ran>,
    /// The release it tagged, and whether the tag reached the remote.
    #[serde(default)]
    pub(crate) tag: Option<String>,
    #[serde(default)]
    pub(crate) pushed: bool,
    #[serde(default)]
    pub(crate) board: Board,
    /// The remote it released from, and the releases tagged here that it found missing there, once
    /// it asked.
    #[serde(default)]
    pub(crate) remote: String,
    #[serde(default)]
    pub(crate) unpushed: Option<Vec<String>>,
    /// What started it, when the cadence did (docs/workflow/RELEASES.md): the landings it counted.
    #[serde(default)]
    pub(crate) cause: Option<String>,
    /// What the release it tagged, or the release a `--refresh` named, refreshed on the machine
    /// (docs/workflow/RELEASES.md).
    #[serde(default)]
    pub(crate) refreshed: Option<Refresh>,
    /// What the release it tagged, or the release a `--refresh` named, published on crates.io
    /// before its refresh ([`super::crates`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) published: Option<Publication>,
    /// The release candidate it reran, when it was a rerun of one ([`super::candidate`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) rerun: Option<Rerun>,
    /// How long the machine slept under it, in seconds: what the wall clock counted of it past
    /// the clock that stops in sleep ([`super::span`]). Left out of a run that did not sleep.
    #[serde(default, skip_serializing_if = "super::span::none")]
    pub(crate) slept: f64,
    /// What the tree it checked told of its checks' inputs, asked before the checks
    /// ([`super::inputs`]); none for a tree that tells none, or a run that reached no check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) inputs: Option<super::inputs::Inputs>,
    /// The system, Chrome and PostgreSQL its checks ran on, which no tree pins; none for a run
    /// that reached no check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) environment: Option<super::inputs::Environment>,
    /// What judged each check it asked of GitHub's runners ([`super::hosted`]): the workflow run,
    /// or this machine and why; none for a run that asked nothing there.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) hosted: Vec<super::hosted::Hosted>,
}

/// A rerun of the release candidate: the commit first judged, the round it is (2 for the first
/// rerun), and each check or step it kept green from an earlier round instead of running it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Rerun {
    pub(crate) first: String,
    pub(crate) round: u32,
    #[serde(default)]
    pub(crate) carried: Vec<Carried>,
}

/// A check, step or part of a step a rerun kept green: its name and the commit and run it was
/// green on. With no digest that commit's tree is the rerun's; with one, the digest of its inputs
/// there is the one the rerun's tree tells ([`super::inputs`]), and it was not run on this commit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Carried {
    pub(crate) name: String,
    pub(crate) commit: String,
    pub(crate) run: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) digest: Option<String>,
}

/// What a refresh step ends as when its project finishes it (writers of its board to rebuild, a
/// board from before the writer record): green here, filed nowhere (docs/workflow/RELEASES.md).
pub(crate) const THEIRS: &str = "the project's to finish";
/// What a step that is its project's to finish is said with: the command that says what taking the
/// release asks of the project.
pub(crate) const TAKES: &str =
    "`just symphony-cli release-notes` in the project says what taking the release asks of it";

/// A release's refresh: the release, its install's step and one for each project whose board it
/// judged, what it said, the machine's server's line last, and the steps among them that are their
/// projects' to finish.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Refresh {
    pub(crate) release: String,
    #[serde(default)]
    pub(crate) steps: Vec<Ran>,
    #[serde(default)]
    pub(crate) said: Vec<String>,
    /// The steps, by name, whose board its project finishes: each green, its line in `said` saying
    /// what finishes it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) theirs: Vec<String>,
}

impl Refresh {
    /// Whether every step of it is green: none red, the projects' to finish among them.
    pub(crate) fn green(&self) -> bool {
        self.steps.iter().all(Ran::green)
    }

    /// Whether its step `name` ended as its project's to finish ([`THEIRS`]).
    pub(crate) fn is_theirs(&self, name: &str) -> bool {
        self.theirs.iter().any(|theirs| theirs == name)
    }

    /// Its steps the board is told of as a release's reds: the install, the registry, and each
    /// project's board step only while green, which closes a red an older build filed for it. A
    /// board that did not move is its update's failure, filed once on its update's own task
    /// ([`super::updates`]), never as a red of its step too.
    pub(crate) fn told(&self) -> impl Iterator<Item = &Ran> {
        (self.steps.iter()).filter(|ran| {
            let board = ran.name != super::install::STEP && ran.name != super::refresh::STEP;
            !self.is_theirs(&ran.name) && (!board || ran.green())
        })
    }
}

/// The run holding the lock: which, of which kind, its process, when it started and when its time
/// ends, and the check it runs, while it runs one. A build older than the rename wrote no version,
/// and its kinds as version 1 named them ([`renamed`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Running {
    #[serde(default)]
    pub(crate) version: u32,
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) kind: String,
    pub(crate) pid: u32,
    pub(crate) started_at: f64,
    pub(crate) until: f64,
    #[serde(default)]
    pub(crate) check: Option<Checking>,
}

/// A check running: its name, the process group it runs in (its leader's id) and when that leader
/// started, which tells the group from a later one given the same id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Checking {
    pub(crate) name: String,
    pub(crate) group: i64,
    #[serde(default)]
    pub(crate) start: Option<Stamp>,
}

/// Say in `folder` that `running` holds the lock.
pub(crate) fn hold(folder: &Path, running: &Running) -> Result<(), String> {
    let path = folder.join("running.json");
    let text = serde_json::to_string_pretty(running).map_err(|e| e.to_string())? + "\n";
    symphony::text::write_whole(&path, &text).map_err(|e| symphony::text::os_error(&e, &path))
}

/// The run that said it holds the lock in `folder`, when one did and it reads, its kind as this
/// build names it.
pub(crate) fn holder(folder: &Path) -> Option<Running> {
    let text = fs::read_to_string(folder.join("running.json")).ok()?;
    let mut running: Running = serde_json::from_str(&text).ok()?;
    if running.version < VERSION {
        running.kind = kind_of(&running.kind).into();
        running.version = VERSION;
    }
    Some(running)
}

/// A kind as a record of version 1 named it, as this build names it.
fn kind_of(old: &str) -> &str {
    match old {
        "nightly" => "release",
        "release" => "out-of-cycle",
        other => other,
    }
}

/// `run`, read from `folder`, as this build records it: a record of version 1, which a build older
/// than the rename wrote, takes today's kind, and each log it names under that build's folder
/// (`symphony/nightly/`) is named where it is now, under `folder`, which a run moved it to.
pub(crate) fn renamed(mut run: Run, folder: &Path) -> Run {
    if run.version >= VERSION {
        return run;
    }
    run.kind = kind_of(&run.kind).into();
    run.version = VERSION;
    if let Some(old) = folder.parent().map(|parent| parent.join(LEGACY))
        && old != folder
    {
        let rebase = |ran: &mut Ran| {
            if let Ok(rest) = ran.log.strip_prefix(&old) {
                ran.log = folder.join(rest);
            }
        };
        let refreshed = (run.refreshed.iter_mut()).flat_map(|refresh| refresh.steps.iter_mut());
        for ran in run.checks.iter_mut().chain(refreshed) {
            rebase(ran);
            ran.steps.iter_mut().for_each(rebase);
        }
    }
    run
}

/// The run `id` no longer holds the lock in `folder`.
pub(crate) fn release(folder: &Path, id: &str) {
    if holder(folder).is_some_and(|running| running.id == id) {
        let _ = fs::remove_file(folder.join("running.json"));
    }
}

/// What the run did on the board.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Board {
    #[serde(default)]
    pub(crate) filed: Vec<String>,
    #[serde(default)]
    pub(crate) updated: Vec<String>,
    #[serde(default)]
    pub(crate) closed: Vec<String>,
    #[serde(default)]
    pub(crate) error: Option<String>,
    /// Each red check or step it filed or told a line, with its task.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) reds: Vec<Held>,
}

/// A red the board holds: its key, the check or step it is of, and its task.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Held {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) slug: String,
}

/// The release run's folder in the git common dir `common`.
pub(crate) fn folder(common: &Path) -> PathBuf {
    common.join(symphony_board::folders::RELEASE)
}

/// The run's lock, open: the folder the run records in, where it is, and whether it was taken.
///
/// Taken, it lets go of the lock itself as it drops: a `flock` belongs to the open file, and a child
/// another thread forks while it is held keeps a copy of that file until its exec (a spawn that
/// fails, until the kernel tears its child down), so a close alone would leave the lock held by that
/// child for as long, and a run, a `--due` or a `--drop` started right after would find it held.
pub(crate) struct Lock {
    pub(crate) folder: PathBuf,
    pub(crate) path: PathBuf,
    pub(crate) file: File,
    /// Taken; held by another run (`WouldBlock`); or the attempt failed.
    pub(crate) taken: Result<(), TryLockError>,
}

impl Drop for Lock {
    fn drop(&mut self) {
        if self.taken.is_ok() {
            let _ = self.file.unlock();
        }
    }
}

/// Open the lock of the runs of `common` and try to take it, without waiting: in [`folder`], made
/// when missing.
pub(crate) fn lock(common: &Path) -> Result<Lock, String> {
    let folder = folder(common);
    fs::create_dir_all(&folder).map_err(|e| symphony::text::os_error(&e, &folder))?;
    let path = folder.join(LOCK);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| symphony::text::os_error(&e, &path))?;
    let taken = file.try_lock();
    Ok(Lock {
        folder,
        path,
        file,
        taken,
    })
}

/// Where the run `id`'s check logs go in `folder`.
pub(crate) fn logs(folder: &Path, id: &str) -> PathBuf {
    folder.join("logs").join(id)
}

/// Write `run` whole into `folder`; its record so far, beside its logs, goes.
pub(crate) fn write(folder: &Path, run: &Run) -> Result<(), String> {
    let runs = folder.join("runs");
    fs::create_dir_all(&runs).map_err(|e| symphony::text::os_error(&e, &runs))?;
    let path = runs.join(format!("{}.json", run.id));
    let text = serde_json::to_string_pretty(run).map_err(|e| e.to_string())? + "\n";
    symphony::text::write_whole(&path, &text).map_err(|e| symphony::text::os_error(&e, &path))?;
    let _ = fs::remove_file(logs(folder, &run.id).join(SO_FAR));
    Ok(())
}

/// Keep `run`'s record so far in its log folder `logs`, which a later run reads if this one ends
/// without writing its whole record.
pub(crate) fn so_far(logs: &Path, run: &Run) {
    if let Ok(text) = serde_json::to_string_pretty(run) {
        let _ = symphony::text::write_whole(&logs.join(SO_FAR), &(text + "\n"));
    }
}

/// Record, in `folder`, each run that ended without recording itself, as the run `by` finds them
/// at `now`: the run `holder` said held the lock (whose process is gone, since `by` holds it now),
/// and each run whose logs no record names. Each is recorded [`INTERRUPTED`], from its record so far
/// when it kept one: what it was doing, when it was last heard of and how it was found. What it
/// recorded, oldest first.
pub(crate) fn recover(folder: &Path, holder: Option<&Running>, by: &str, now: f64) -> Vec<Run> {
    let recorded = |id: &str| folder.join("runs").join(format!("{id}.json")).exists();
    let mut found: Vec<String> = Vec::new();
    if let Some(holder) = holder.filter(|holder| holder.id != by && !recorded(&holder.id)) {
        found.push(holder.id.clone());
    }
    if let Ok(entries) = fs::read_dir(folder.join("logs")) {
        let mut orphans: Vec<String> = (entries.filter_map(Result::ok))
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|id| id != by && !recorded(id) && !found.contains(id))
            .collect();
        orphans.sort();
        found.extend(orphans);
    }
    let mut runs: Vec<Run> = found
        .iter()
        .map(|id| interrupted(folder, id, holder.filter(|h| h.id == *id), by, now))
        .collect();
    runs.sort_by(|a, b| a.started_at.total_cmp(&b.started_at));
    for run in &runs {
        let _ = write(folder, run);
    }
    runs
}

/// The run `id` of `folder`, which ended without recording itself, as the run `by` found it at
/// `now`, with `holder` when it was the one `running.json` named.
fn interrupted(folder: &Path, id: &str, holder: Option<&Running>, by: &str, now: f64) -> Run {
    let logs = logs(folder, id);
    let so_far = (fs::read_to_string(logs.join(SO_FAR)).ok())
        .and_then(|text| serde_json::from_str::<Run>(&text).ok())
        .filter(|run| run.id == id)
        .map(|run| renamed(run, folder));
    let heard = last_heard(&logs);
    let mut run = so_far.unwrap_or_else(|| Run {
        version: VERSION,
        id: id.to_string(),
        kind: (holder.map(|h| h.kind.clone()))
            .filter(|kind| !kind.is_empty())
            .unwrap_or_else(|| "release".into()),
        started_at: holder.map_or_else(|| heard.map_or(now, |(first, _)| first), |h| h.started_at),
        ..Run::default()
    });
    run.ended_at = heard.map_or(run.started_at, |(_, last)| last.max(run.started_at));
    let doing = match holder.and_then(|holder| holder.check.as_ref()) {
        Some(check) => format!("while `{}` ran", check.name),
        None => match unfinished(&logs, &run.checks) {
            Some(name) => format!("while `{name}` ran"),
            None => match run.checks.last() {
                Some(last) => format!("after `{}` ended", last.name),
                None => "before any check ended".into(),
            },
        },
    };
    let how = match holder {
        Some(holder) => format!(
            "its process {} was gone when the run {by} took the lock at {}",
            holder.pid,
            local(now)
        ),
        None => format!(
            "the run {by} found its logs, which no record named, at {}",
            local(now)
        ),
    };
    run.result = INTERRUPTED.into();
    run.error = Some(format!(
        "ended without recording (killed, or the machine stopped) {doing}; {how}; its logs: {}",
        logs.display()
    ));
    run
}

/// The first and last times a file under `logs` was written, in seconds since the epoch: when its
/// run was first and last heard of.
fn last_heard(logs: &Path) -> Option<(f64, f64)> {
    let seconds = |time: SystemTime| {
        time.duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64())
    };
    let times: Vec<f64> = (fs::read_dir(logs).ok()?.filter_map(Result::ok))
        .filter_map(|entry| entry.metadata().ok()?.modified().ok())
        .map(seconds)
        .collect();
    let first = times.iter().copied().reduce(f64::min)?;
    let last = times.iter().copied().reduce(f64::max)?;
    Some((first, last))
}

/// The check whose log in `logs` was written last, when none of `done` is it: the check that ran
/// when its run ended.
fn unfinished(logs: &Path, done: &[Ran]) -> Option<String> {
    let mut written: Vec<(SystemTime, String)> = (fs::read_dir(logs).ok()?.filter_map(Result::ok))
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let check = name.strip_suffix(".log")?.to_string();
            Some((entry.metadata().ok()?.modified().ok()?, check))
        })
        .collect();
    written.sort();
    let (_, last) = written.pop()?;
    (!done.iter().any(|ran| board::key(&ran.name) == last)).then_some(last)
}

/// Every run recorded in `folder`, oldest first, as this build records it ([`renamed`]); a record
/// that does not read is left out.
pub(crate) fn read(folder: &Path) -> Vec<Run> {
    let Ok(entries) = fs::read_dir(folder.join("runs")) else {
        return Vec::new();
    };
    let mut runs: Vec<Run> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| fs::read_to_string(path).ok())
        .filter_map(|text| serde_json::from_str::<Run>(&text).ok())
        .map(|run| renamed(run, folder))
        .collect();
    runs.sort_by(|a, b| {
        a.started_at
            .total_cmp(&b.started_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    runs
}

/// Remove from `folder` every record but the newest `runs`, and every run's logs but the newest
/// `logs` runs': what [`read`] reads, and the log folders of runs it no longer keeps.
pub(crate) fn prune(folder: &Path, runs: usize, logs: usize) {
    let all = read(folder);
    let ids: Vec<&str> = all.iter().rev().map(|run| run.id.as_str()).collect();
    for id in ids.iter().skip(runs) {
        let _ = fs::remove_file(folder.join("runs").join(format!("{id}.json")));
    }
    let kept: Vec<&str> = ids.iter().take(logs).copied().collect();
    let Ok(entries) = fs::read_dir(folder.join("logs")) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !kept.contains(&name.as_str()) && entry.path().is_dir() {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// The newest green run of `runs`, oldest first as [`read`] gives them.
pub(crate) fn last_green(runs: &[Run]) -> Option<&Run> {
    runs.iter().rev().find(|run| run.result == "green")
}

/// How long ago `then` was at `now`, in a few words.
pub(crate) fn age(now: f64, then: f64) -> String {
    let seconds = (now - then).max(0.0);
    let minutes = (seconds / 60.0).floor();
    if minutes < 60.0 {
        format!("{minutes:.0} min ago")
    } else if minutes < 48.0 * 60.0 {
        format!("{:.0} h ago", (minutes / 60.0).floor())
    } else {
        format!("{:.0} days ago", (minutes / 1440.0).floor())
    }
}

/// `seconds` since the epoch as this machine's local minute.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a time since the epoch fits the platform's time_t"
)]
pub(crate) fn local(seconds: f64) -> String {
    stamp_at(seconds as libc::time_t)
}

/// One line on `run`: its result, when, how long ago, its commit, how long the machine slept under
/// it when it did, and what was red or tagged.
fn line(run: &Run, now: f64) -> String {
    let mut said = format!(
        "{}, {} ({}), on {}",
        run.result,
        local(run.started_at),
        age(now, run.started_at),
        short(&run.commit)
    );
    if let Some(slept) = super::span::said(run.slept) {
        said.push_str(&format!("; the machine slept {slept} under it"));
    }
    for judged in &run.hosted {
        said.push_str(&format!("; {}", judged.said()));
    }
    if let Some(rerun) = &run.rerun {
        said.push_str(&format!(
            " (round {} of the release candidate first judged at {}{})",
            rerun.round,
            short(&rerun.first),
            match rerun.carried.len() {
                0 => String::new(),
                1 => ", 1 carried green".into(),
                n => format!(", {n} carried green"),
            }
        ));
    }
    let red: Vec<String> = (run.checks.iter())
        .flat_map(super::steps::reds)
        .map(|ran| format!("{} ({})", ran.name, ran.ended()))
        .collect();
    if !red.is_empty() {
        said.push_str(&format!("; red: {}", red.join(", ")));
    }
    if let Some(error) = &run.error {
        said.push_str(&format!("; {error}"));
    }
    if let Some(tag) = &run.tag {
        let pushed = if run.pushed { "" } else { ", not pushed" };
        said.push_str(&format!("; tagged {tag}{pushed}"));
    }
    let open: Vec<&str> = (run.board.filed.iter())
        .chain(&run.board.updated)
        .map(String::as_str)
        .collect();
    if !open.is_empty() {
        said.push_str(&format!("; red tasks: {}", open.join(", ")));
    }
    said
}

/// What `--status` prints: the newest release among `releases` (newest first, each with its
/// commit), the last release run of `runs` and its age at `now`, the last out-of-cycle release
/// when one was later, the last interrupted run while its logs are kept (among the newest
/// [`KEPT_LOGS`]) when it is not the last release run, and the last green run.
pub(crate) fn status(runs: &[Run], releases: &[(Release, String)], now: f64) -> String {
    let mut text = match releases.first() {
        Some((release, commit)) => {
            format!("newest release: {} ({})\n", release.name(), short(commit))
        }
        None => "newest release: none yet\n".to_string(),
    };
    let last = runs.iter().rev().find(|run| run.kind == "release");
    match last {
        Some(last) => {
            let overdue = if now - last.started_at > OVERDUE_SECONDS {
                " (overdue: the release run runs once a day)"
            } else {
                ""
            };
            text.push_str(&format!("last run: {}{overdue}\n", line(last, now)));
        }
        None => text.push_str("last run: none yet\n"),
    }
    if let Some(release) = (runs.iter().rev())
        .find(|run| run.kind == "out-of-cycle")
        .filter(|release| last.is_none_or(|last| release.started_at > last.started_at))
    {
        text.push_str(&format!(
            "last out-of-cycle release: {}\n",
            line(release, now)
        ));
    }
    if let Some(cut) = (runs.iter().rev().take(KEPT_LOGS))
        .find(|run| run.result == INTERRUPTED)
        .filter(|cut| last.is_none_or(|last| last.id != cut.id))
    {
        text.push_str(&format!("last interrupted run: {}\n", line(cut, now)));
    }
    match last_green(runs) {
        Some(green) => text.push_str(&format!(
            "last green: {} ({}), on {}\n",
            local(green.started_at),
            age(now, green.started_at),
            short(&green.commit)
        )),
        None => text.push_str("last green: none recorded\n"),
    }
    let asked = (runs.iter().rev()).find_map(|run| Some((run, run.unpushed.as_ref()?)));
    if let Some((run, unpushed)) = asked.filter(|(_, unpushed)| !unpushed.is_empty()) {
        text.push_str(&format!(
            "not on {remote}, as the last run to ask found: {} (tagged here: {} delivers one)\n",
            unpushed.join(", "),
            push_line(&run.remote, &unpushed[0]),
            remote = run.remote,
        ));
    }
    text
}

/// What `--status` says of the newest refresh among `runs` at `now`: its release, which run made
/// it and when, and how many of its steps are left to their projects, then each line it said (the
/// install, the switch, each board, `current` and the machine's server), each red step with how it
/// ended and each step that is its project's to finish, then what finishes those ([`BY_HAND`],
/// [`takes`]).
pub(crate) fn refresh_status(runs: &[Run], now: f64) -> String {
    let Some((run, refresh)) =
        (runs.iter().rev()).find_map(|run| Some((run, run.refreshed.as_ref()?)))
    else {
        return "last refresh: none recorded\n".into();
    };
    let verdict = if refresh.green() { "green" } else { "red" };
    let left = match refresh.theirs.len() {
        0 => String::new(),
        1 => ", 1 step left to its project".into(),
        n => format!(", {n} steps left to their projects"),
    };
    let mut text = format!(
        "last refresh: {verdict}{left}, of {} by the run {}, {} ({})\n",
        refresh.release,
        run.id,
        local(run.started_at),
        age(now, run.started_at)
    );
    for said in &refresh.said {
        text.push_str(&format!("  {said}\n"));
    }
    for ran in &refresh.steps {
        if refresh.is_theirs(&ran.name) {
            text.push_str(&format!(
                "  {} {THEIRS}, its schema update's task filed (below); log {}\n",
                ran.name,
                ran.log.display()
            ));
        } else if !ran.green() {
            text.push_str(&format!(
                "  {} red: {}; log {}\n",
                ran.name,
                ran.ended(),
                ran.log.display()
            ));
        }
    }
    if !refresh.theirs.is_empty() {
        text.push_str(BY_HAND);
        text.push_str(&takes());
    }
    text
}

/// What `--status` says of the newest publication on crates.io among `runs` at `now`: its release,
/// which run made it and when, red when an upload or a read of it failed, then each line it said.
pub(crate) fn publish_status(runs: &[Run], now: f64) -> String {
    let Some((run, published)) =
        (runs.iter().rev()).find_map(|run| Some((run, run.published.as_ref()?)))
    else {
        return "last publish: none recorded\n".into();
    };
    let mut text = format!(
        "last publish: {}of {} by the run {}, {} ({})\n",
        if published.failed { "red, " } else { "" },
        published.release,
        run.id,
        local(run.started_at),
        age(now, run.started_at)
    );
    for line in published.lines() {
        text.push_str(&format!("  {line}\n"));
    }
    text
}

/// What a refresh leaves to the projects whose boards it could not move, which `--status` says
/// under it.
const BY_HAND: &str = "  by hand: each writer named behind the release is rebuilt at its schema and \
                       run once, which records it; a board from before the writer record is \
                       brought up by its own `just tasks migrate` in its main checkout; then an \
                       orchestrator's `just release --refresh` finishes the rest\n";

/// Where a project left a step reads what taking the release asks of it, which `--status` says
/// under [`BY_HAND`].
fn takes() -> String {
    format!("  {TAKES}\n")
}

/// The push that delivers the release `tag` to `remote`, and nothing else.
pub(crate) fn push_line(remote: &str, tag: &str) -> String {
    format!("`git push --no-follow-tags {remote} refs/tags/{tag}:refs/tags/{tag}`")
}

/// A child caught between its fork and its exec, as the board's tests catch one.
#[cfg(test)]
#[path = "../../../../tools/test-support/mid_spawn.rs"]
mod mid_spawn;

#[cfg(test)]
mod tests {
    use super::mid_spawn::MidSpawn;
    use super::*;
    use crate::test_tree::Tree;

    const NOW: f64 = 1_800_000_000.0;

    fn run(id: &str, kind: &str, result: &str, started_at: f64) -> Run {
        Run {
            version: VERSION,
            id: id.into(),
            kind: kind.into(),
            commit: format!("{id:0<40}"),
            tip: format!("{id:0<40}"),
            started_at,
            ended_at: started_at + 60.0,
            result: result.into(),
            remote: "origin".into(),
            ..Run::default()
        }
    }

    fn red_check(name: &str) -> Ran {
        Ran {
            name: name.into(),
            command: "just verify".into(),
            exit_code: Some(101),
            signal: None,
            seconds: 3.0,
            log: PathBuf::from("/l"),
            steps: Vec::new(),
            error: None,
            timed_out: false,
            interrupted: false,
            load: None,
            alone: None,
        }
    }

    #[test]
    fn a_release_missing_from_the_remote_is_said_until_a_run_finds_it_there() {
        let mut refused = run("a", "release", "green", NOW - 7200.0);
        refused.tag = Some("release-2026-11-11".into());
        refused.unpushed = Some(vec!["release-2026-11-11".into()]);
        let mut unchanged = run("b", "release", "unchanged", NOW - 3600.0);
        unchanged.unpushed = Some(vec!["release-2026-11-11".into()]);
        let stopped = run("c", "release", "error", NOW - 60.0);
        let said = status(
            &[refused.clone(), unchanged.clone(), stopped.clone()],
            &[],
            NOW,
        );
        assert!(
            said.ends_with(
                "not on origin, as the last run to ask found: release-2026-11-11 (tagged here: \
                 `git push --no-follow-tags origin refs/tags/release-2026-11-11:refs/tags/release-2026-11-11` \
                 delivers one)\n"
            ),
            "a run that did not ask leaves the last answer standing: {said}"
        );
        let mut pushed = unchanged;
        pushed.id = "d".into();
        pushed.started_at = NOW - 30.0;
        pushed.unpushed = Some(Vec::new());
        let said = status(&[refused, stopped, pushed], &[], NOW);
        assert!(!said.contains("not on origin"), "{said}");
    }

    #[test]
    fn the_lock_holder_is_said_and_released_by_its_own_run_alone() {
        let tree = Tree::new();
        let folder = tree.path("release");
        fs::create_dir_all(&folder).unwrap();
        assert_eq!(holder(&folder), None);
        let running = Running {
            version: VERSION,
            id: "r1".into(),
            kind: "release".into(),
            pid: 7,
            started_at: NOW,
            until: NOW + 60.0,
            check: Some(Checking {
                name: "verify".into(),
                group: 4242,
                start: Some(Stamp::At(NOW)),
            }),
        };
        hold(&folder, &running).unwrap();
        assert_eq!(holder(&folder), Some(running.clone()));
        release(&folder, "another");
        assert_eq!(holder(&folder), Some(running));
        release(&folder, "r1");
        assert_eq!(holder(&folder), None);
        tree.write("release/running.json", "{ not json");
        assert_eq!(holder(&folder), None);
    }

    #[test]
    fn runs_are_written_whole_and_read_back_oldest_first() {
        let tree = Tree::new();
        let folder = tree.path("release");
        assert!(read(&folder).is_empty(), "nothing recorded yet");
        let later = run("b", "release", "green", NOW);
        let earlier = run("a", "release", "red", NOW - 100.0);
        write(&folder, &later).unwrap();
        write(&folder, &earlier).unwrap();
        tree.write("release/runs/broken.json", "{ not json");
        tree.write("release/runs/notes.txt", "not a record");
        assert_eq!(read(&folder), [earlier, later]);
        assert_eq!(
            tree.names("release/runs"),
            ["a.json", "b.json", "broken.json", "notes.txt"]
        );
    }

    #[test]
    fn a_run_keeps_the_newest_records_and_the_newest_runs_logs() {
        let tree = Tree::new();
        let folder = tree.path("release");
        for n in 0..5 {
            let id = format!("r{n}");
            write(&folder, &run(&id, "release", "green", NOW + f64::from(n))).unwrap();
            tree.write(&format!("release/logs/{id}/verify.log"), "log");
        }
        tree.write("release/logs/stray/verify.log", "a run with no record");
        prune(&folder, 3, 2);
        assert_eq!(
            tree.names("release/runs"),
            ["r2.json", "r3.json", "r4.json"]
        );
        assert_eq!(tree.names("release/logs"), ["r3", "r4"]);
        prune(&tree.path("none"), 3, 2);
    }

    #[test]
    fn the_last_green_run_is_the_newest_green_one() {
        let runs = [
            run("a", "release", "green", NOW - 300.0),
            run("b", "out-of-cycle", "green", NOW - 200.0),
            run("c", "release", "red", NOW - 100.0),
        ];
        assert_eq!(last_green(&runs).unwrap().id, "b");
        assert!(last_green(&runs[2..]).is_none());
    }

    #[test]
    fn an_age_reads_in_minutes_hours_then_days() {
        assert_eq!(age(NOW, NOW), "0 min ago");
        assert_eq!(age(NOW, NOW - 59.0 * 60.0), "59 min ago");
        assert_eq!(age(NOW, NOW - 61.0 * 60.0), "1 h ago");
        assert_eq!(age(NOW, NOW - 47.9 * 3600.0), "47 h ago");
        assert_eq!(age(NOW, NOW - 72.0 * 3600.0), "3 days ago");
        assert_eq!(
            age(NOW, NOW + 100.0),
            "0 min ago",
            "a clock behind reads as now"
        );
    }

    #[test]
    fn the_status_names_the_newest_release_the_last_run_and_the_last_green() {
        let mut last = run("last", "release", "red", NOW - 3.0 * 3600.0);
        last.checks = vec![red_check("verify")];
        last.board.filed = vec!["tests/release-red-verify".into()];
        let mut green = run("green", "release", "green", NOW - 27.0 * 3600.0);
        green.tag = Some("release-2026-11-11".into());
        green.pushed = true;
        let releases = [(
            Release::parse("release-2026-11-11").unwrap(),
            "c".repeat(40),
        )];
        let said = status(&[green, last], &releases, NOW);
        let lines: Vec<&str> = said.lines().collect();
        assert_eq!(
            lines[0],
            "newest release: release-2026-11-11 (cccccccccccc)"
        );
        assert!(lines[1].starts_with("last run: red, "), "{said}");
        assert!(lines[1].contains("(3 h ago), on last00000000"), "{said}");
        assert!(
            lines[1].ends_with("; red: verify (exit 101); red tasks: tests/release-red-verify"),
            "{said}"
        );
        assert!(lines[2].starts_with("last green: "), "{said}");
        assert!(lines[2].ends_with("(27 h ago), on green0000000"), "{said}");
        assert_eq!(lines.len(), 3, "{said}");
    }

    #[test]
    fn a_run_the_machine_slept_under_says_for_how_long_in_its_record_and_the_status() {
        let mut slept = run("slept", "release", "red", NOW - 3600.0);
        slept.checks = vec![red_check("verify")];
        slept.slept = 1589.0;
        let said = status(std::slice::from_ref(&slept), &[], NOW);
        let last = said.lines().nth(1).unwrap();
        assert!(
            last.contains(
                "(1 h ago), on slept0000000; the machine slept 26 min under it; red: verify (exit 101)"
            ),
            "{said}"
        );
        let text = serde_json::to_string(&slept).unwrap();
        assert!(text.contains("\"slept\":1589.0"), "{text}");
        assert_eq!(serde_json::from_str::<Run>(&text).unwrap(), slept);

        // One that did not sleep says nothing new, and a record written before reads as awake.
        let awake = run("awake", "release", "red", NOW - 3600.0);
        assert!(!status(std::slice::from_ref(&awake), &[], NOW).contains("slept"));
        let text = serde_json::to_string(&awake).unwrap();
        assert!(!text.contains("slept"), "{text}");
        assert_eq!(serde_json::from_str::<Run>(&text).unwrap().slept, 0.0);
    }

    #[test]
    fn the_status_names_the_red_steps_of_a_check_red_in_its_steps() {
        let step = |name: &str, code: i32| Ran {
            name: format!("verify/{name}"),
            exit_code: Some(code),
            ..red_check("x")
        };
        let mut last = run("last", "release", "red", NOW - 3600.0);
        last.checks = vec![Ran {
            exit_code: Some(1),
            steps: vec![step("lint", 0), step("size-check", 1), step("tests", 101)],
            ..red_check("verify")
        }];
        let said = status(&[last.clone()], &[], NOW);
        assert!(
            said.contains("; red: verify/size-check (exit 1), verify/tests (exit 101)\n"),
            "{said}"
        );
        // Stopped while it ran: the check itself too.
        last.checks[0].timed_out = true;
        let said = status(&[last], &[], NOW);
        assert!(
            said.contains(
                "; red: verify (timed out: stopped after 3 s, when the run's time was up), \
                 verify/size-check (exit 1), verify/tests (exit 101)\n"
            ),
            "{said}"
        );
    }

    #[test]
    fn the_status_of_a_machine_that_never_ran_one_says_so() {
        assert_eq!(
            status(&[], &[], NOW),
            "newest release: none yet\nlast run: none yet\nlast green: none recorded\n"
        );
    }

    #[test]
    fn an_old_release_run_is_overdue_and_a_later_out_of_cycle_release_is_named() {
        let last = run("last", "release", "green", NOW - 40.0 * 3600.0);
        let mut release = run("rel", "out-of-cycle", "green", NOW - 3600.0);
        release.tag = Some("release-2026-11-12.2".into());
        let said = status(&[last.clone(), release.clone()], &[], NOW);
        assert!(
            said.contains("(overdue: the release run runs once a day)"),
            "{said}"
        );
        assert!(
            said.contains("last out-of-cycle release: green, ")
                && said.contains("; tagged release-2026-11-12.2, not pushed"),
            "{said}"
        );
        let mut older = release;
        older.started_at = NOW - 50.0 * 3600.0;
        let said = status(&[older, last], &[], NOW);
        assert!(
            !said.contains("out-of-cycle"),
            "an earlier release run is not news: {said}"
        );
    }

    /// `file` in `tree` holding `text`, last written at `at` seconds since the epoch.
    fn written_at(tree: &Tree, file: &str, text: &str, at: f64) {
        tree.write(file, text);
        let opened = fs::File::options()
            .write(true)
            .open(tree.path(file))
            .unwrap();
        let when = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs_f64(at);
        opened.set_modified(when).unwrap();
    }

    #[test]
    fn a_run_that_ended_without_recording_is_recorded_interrupted_saying_what_it_did() {
        let tree = Tree::new();
        let folder = tree.path("release");
        // The run running.json named, killed while `verify` ran, after `lint` passed.
        let mut killed = run("k1", "release", "", NOW - 3600.0);
        let mut lint = red_check("lint");
        lint.exit_code = Some(0);
        killed.checks = vec![lint];
        fs::create_dir_all(tree.path("release/logs/k1")).unwrap();
        so_far(&tree.path("release/logs/k1"), &killed);
        written_at(&tree, "release/logs/k1/lint.log", "ok", NOW - 3500.0);
        written_at(&tree, "release/logs/k1/verify.log", "...", NOW - 3000.0);
        let holder = Running {
            version: VERSION,
            id: "k1".into(),
            kind: "release".into(),
            pid: 99_999,
            started_at: NOW - 3600.0,
            until: NOW,
            check: Some(Checking {
                name: "verify".into(),
                group: 99_999,
                start: None,
            }),
        };
        // Logs no record names: one of a run killed during `verify` that kept no record so far,
        // one of a run killed between checks.
        written_at(&tree, "release/logs/k0/verify.log", "...", NOW - 7200.0);
        let mut between = run("k2", "out-of-cycle", "", NOW - 1800.0);
        between.checks = vec![red_check("lint")];
        fs::create_dir_all(tree.path("release/logs/k2")).unwrap();
        so_far(&tree.path("release/logs/k2"), &between);
        written_at(&tree, "release/logs/k2/lint.log", "red", NOW - 1700.0);
        // A run recorded, and the run that finds them, which has its own logs and no record yet.
        write(&folder, &run("done", "release", "green", NOW - 9000.0)).unwrap();
        tree.write("release/logs/done/verify.log", "ok");
        tree.write("release/logs/now/verify.log", "...");

        let found = recover(&folder, Some(&holder), "now", NOW);
        let ids: Vec<&str> = found.iter().map(|run| run.id.as_str()).collect();
        assert_eq!(ids, ["k0", "k1", "k2"], "oldest first");
        let logs = |id: &str| {
            tree.path(&format!("release/logs/{id}"))
                .display()
                .to_string()
        };
        let k0 = &found[0];
        assert_eq!(
            (k0.kind.as_str(), k0.result.as_str(), k0.commit.as_str()),
            ("release", INTERRUPTED, "")
        );
        assert_eq!((k0.started_at, k0.ended_at), (NOW - 7200.0, NOW - 7200.0));
        assert_eq!(
            k0.error.as_deref(),
            Some(
                format!(
                    "ended without recording (killed, or the machine stopped) while `verify` ran; \
                     the run now found its logs, which no record named, at {}; its logs: {}",
                    local(NOW),
                    logs("k0")
                )
                .as_str()
            )
        );
        let k1 = &found[1];
        assert_eq!(k1.commit, killed.commit, "its record so far");
        assert_eq!(k1.checks, killed.checks);
        assert_eq!(k1.started_at, NOW - 3600.0);
        assert!(
            k1.ended_at >= NOW - 3000.0,
            "last heard of: {}",
            k1.ended_at
        );
        assert_eq!(
            k1.error.as_deref(),
            Some(
                format!(
                    "ended without recording (killed, or the machine stopped) while `verify` ran; \
                     its process 99999 was gone when the run now took the lock at {}; its logs: {}",
                    local(NOW),
                    logs("k1")
                )
                .as_str()
            )
        );
        let k2 = &found[2];
        assert_eq!(k2.kind, "out-of-cycle");
        assert!(
            k2.error.as_deref().unwrap().starts_with(
                "ended without recording (killed, or the machine stopped) after `lint` ended;"
            ),
            "{k2:?}"
        );

        // Recorded, they are runs like any other, their records so far gone and their logs kept.
        let all: Vec<String> = read(&folder).into_iter().map(|run| run.id).collect();
        assert_eq!(all, ["done", "k0", "k1", "k2"]);
        assert!(!tree.path("release/logs/k1/run.json").exists());
        assert!(
            recover(&folder, Some(&holder), "now", NOW).is_empty(),
            "once"
        );
        prune(&folder, KEPT_RUNS, KEPT_LOGS);
        assert_eq!(
            tree.names("release/logs"),
            ["done", "k0", "k1", "k2"],
            "each recorded run keeps its logs"
        );
    }

    #[test]
    fn a_run_that_left_nothing_but_said_it_ran_is_recorded_before_any_check() {
        let tree = Tree::new();
        let folder = tree.path("release");
        let holder = Running {
            version: VERSION,
            id: "k".into(),
            kind: String::new(),
            pid: 5,
            started_at: NOW - 60.0,
            until: NOW,
            check: None,
        };
        let found = recover(&folder, Some(&holder), "now", NOW);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "release");
        assert_eq!(
            (found[0].started_at, found[0].ended_at),
            (NOW - 60.0, NOW - 60.0)
        );
        assert!(
            found[0].error.as_deref().unwrap().starts_with(
                "ended without recording (killed, or the machine stopped) before any check ended; \
                 its process 5 was gone"
            ),
            "{:?}",
            found[0].error
        );
        assert!(
            recover(
                &folder,
                Some(&Running {
                    id: "now".into(),
                    ..holder
                }),
                "now",
                NOW
            )
            .is_empty(),
            "the finder is not found"
        );
    }

    #[test]
    fn the_status_names_the_last_interrupted_run_while_its_logs_are_kept() {
        let mut cut = run("cut", "release", INTERRUPTED, NOW - 7200.0);
        cut.error = Some(
            "ended without recording (killed, or the machine stopped) while `verify` ran".into(),
        );
        let green = run("green", "release", "green", NOW - 3600.0);
        let said = status(&[cut.clone(), green.clone()], &[], NOW);
        assert!(said.contains("last run: green, "), "{said}");
        assert!(
            said.contains(&format!(
                "last interrupted run: interrupted, {} (2 h ago), on cut000000000; ended without \
                 recording (killed, or the machine stopped) while `verify` ran\n",
                local(NOW - 7200.0)
            )),
            "{said}"
        );
        let said = status(std::slice::from_ref(&cut), &[], NOW);
        assert!(said.contains("last run: interrupted, "), "{said}");
        assert!(!said.contains("last interrupted run"), "said once: {said}");
        let mut later: Vec<Run> = vec![cut];
        for n in 0..u32::try_from(KEPT_LOGS).unwrap() {
            later.push(run(
                &format!("n{n:02}"),
                "release",
                "green",
                NOW - 3000.0 + f64::from(n),
            ));
        }
        let said = status(&later, &[], NOW);
        assert!(!said.contains("interrupted"), "its logs are gone: {said}");
    }

    #[test]
    fn a_run_that_could_not_check_says_why() {
        let mut last = run("last", "release", "error", NOW - 60.0);
        last.error = Some("cannot fetch origin".into());
        let said = status(&[last], &[], NOW);
        assert!(
            said.contains("last run: error, ") && said.contains("; cannot fetch origin\n"),
            "{said}"
        );
    }

    #[test]
    fn the_newest_refresh_is_said_with_each_line_and_each_red_step() {
        assert_eq!(refresh_status(&[], NOW), "last refresh: none recorded\n");
        let mut older = run("older", "release", "green", NOW - 7200.0);
        older.refreshed = Some(Refresh {
            release: "release-2026-11-11".into(),
            said: vec!["an older line".into()],
            ..Refresh::default()
        });
        let green = |name: &str| Ran {
            exit_code: Some(0),
            ..red_check(name)
        };
        let mut newer = run("newer", "release", "green", NOW - 3600.0);
        newer.refreshed = Some(Refresh {
            release: "release-2026-11-12".into(),
            steps: vec![
                green("refresh-alpha"),
                red_check("refresh-beta"),
                green("refresh-gamma"),
            ],
            said: vec![
                "alpha: its board moved from schema 18 to 19".into(),
                "gamma: the project's to finish: its board stays at schema 18".into(),
            ],
            theirs: vec!["refresh-gamma".into()],
        });
        let after = run("after", "release", "red", NOW - 60.0);
        let by_hand = "  by hand: each writer named behind the release is rebuilt at its schema \
                       and run once, which records it; a board from before the writer record is \
                       brought up by its own `just tasks migrate` in its main checkout; then an \
                       orchestrator's `just release --refresh` finishes the rest\n  `just \
                       symphony-cli release-notes` in the project says what taking the release \
                       asks of it\n";
        assert_eq!(
            refresh_status(&[older, newer.clone(), after], NOW),
            format!(
                "last refresh: red, 1 step left to its project, of release-2026-11-12 by the run \
                 newer, {} (1 h ago)\n  \
                 alpha: its board moved from schema 18 to 19\n  \
                 gamma: the project's to finish: its board stays at schema 18\n  \
                 refresh-beta red: exit 101; log /l\n  \
                 refresh-gamma the project's to finish, its schema update's task filed (below); log /l\n\
                 {by_hand}",
                local(NOW - 3600.0)
            )
        );
        // The projects' to finish alone leave the refresh green, and what finishes them is said.
        let refresh = newer.refreshed.as_mut().unwrap();
        refresh.steps.remove(1);
        let said = refresh_status(&[newer.clone()], NOW);
        assert!(
            said.starts_with(
                "last refresh: green, 1 step left to its project, of release-2026-11-12"
            ) && said.ends_with(&format!(
                "refresh-gamma the project's to finish, its schema update's task filed (below); log /l\n{by_hand}"
            )),
            "{said}"
        );
        // Two left to their projects are counted so.
        let refresh = newer.refreshed.as_mut().unwrap();
        refresh.steps.push(green("refresh-delta"));
        refresh.theirs.push("refresh-delta".into());
        let said = refresh_status(&[newer.clone()], NOW);
        assert!(
            said.starts_with("last refresh: green, 2 steps left to their projects, of "),
            "{said}"
        );
        // A red with none of theirs, and a green one, leave nothing by hand.
        let refresh = newer.refreshed.as_mut().unwrap();
        refresh.steps = vec![red_check("refresh-beta")];
        refresh.theirs.clear();
        let said = refresh_status(&[newer], NOW);
        assert!(
            said.starts_with("last refresh: red, of ")
                && said.contains("refresh-beta red: exit 101; log /l\n")
                && !said.contains("by hand"),
            "{said}"
        );
        let mut green = run("green", "release", "green", NOW - 60.0);
        green.refreshed = Some(Refresh {
            release: "release-2026-11-12".into(),
            said: vec!["alpha: its board is at schema 19: nothing to move".into()],
            ..Refresh::default()
        });
        assert!(!refresh_status(&[green], NOW).contains("by hand"));
    }

    #[test]
    fn the_newest_publication_is_said_with_each_line_and_red_when_an_upload_failed() {
        assert_eq!(publish_status(&[], NOW), "last publish: none recorded\n");
        let mut older = run("older", "release", "green", NOW - 7200.0);
        older.published = Some(Publication::not("release-2026-11-11", "off", false));
        let mut newer = run("newer", "release", "green", NOW - 3600.0);
        newer.published = Some(Publication {
            release: "release-2026-11-12".into(),
            skipped: vec!["alpha 1.0.0".into()],
            left: vec!["bravo 1.0.0".into()],
            why: Some("cargo publish failed (error: refused; log /l)".into()),
            failed: true,
            ..Publication::default()
        });
        // A later run that published nothing, as a refused one, leaves the newer one said.
        let after = run("after", "release", "red", NOW - 60.0);
        assert_eq!(
            publish_status(&[older, newer.clone(), after], NOW),
            format!(
                "last publish: red, of release-2026-11-12 by the run newer, {} (1 h ago)\n  \
                 on crates.io already: alpha 1.0.0\n  \
                 not published: cargo publish failed (error: refused; log /l): bravo 1.0.0\n  \
                 the release stays tagged; `just release --refresh`, or the next release, \
                 publishes what is left\n",
                local(NOW - 3600.0)
            )
        );
        newer.published = Some(Publication {
            release: "release-2026-11-12".into(),
            uploaded: vec!["bravo 1.0.0".into()],
            ..Publication::default()
        });
        assert!(
            publish_status(&[newer], NOW).ends_with(" (1 h ago)\n  published bravo 1.0.0\n"),
            "a green one"
        );
    }

    #[test]
    fn a_step_its_project_stopped_is_green_and_never_told_to_the_board() {
        let green = |name: &str| Ran {
            exit_code: Some(0),
            ..red_check(name)
        };
        let refresh = Refresh {
            release: "release-2026-11-12".into(),
            steps: vec![green("refresh-alpha"), green("refresh-gamma")],
            said: Vec::new(),
            theirs: vec!["refresh-gamma".into()],
        };
        assert!(refresh.green());
        assert!(refresh.is_theirs("refresh-gamma") && !refresh.is_theirs("refresh-alpha"));
        let told: Vec<&str> = refresh.told().map(|ran| ran.name.as_str()).collect();
        assert_eq!(told, ["refresh-alpha"]);
        // Recorded only when there is one, and read back the same; a record without it reads.
        let text = serde_json::to_string(&refresh).unwrap();
        assert!(text.contains("\"theirs\":[\"refresh-gamma\"]"), "{text}");
        assert_eq!(serde_json::from_str::<Refresh>(&text).unwrap(), refresh);
        let none = Refresh {
            theirs: Vec::new(),
            ..refresh
        };
        let text = serde_json::to_string(&none).unwrap();
        assert!(!text.contains("theirs"), "{text}");
        assert_eq!(serde_json::from_str::<Refresh>(&text).unwrap(), none);
        assert!(none.told().count() == 2);
    }

    /// A record as a build older than the rename wrote it: version 1, of `kind`, with a red check
    /// whose log, and whose one step's log, are under `folder`.
    fn old_record(id: &str, kind: &str, folder: &Path) -> String {
        let mut old = run(id, kind, "red", NOW);
        old.version = 1;
        let mut step = red_check("verify/tests");
        step.log = folder.join(format!("logs/{id}/verify-tests.log"));
        let mut check = red_check("verify");
        check.log = folder.join(format!("logs/{id}/verify.log"));
        check.steps = vec![step];
        old.checks = vec![check];
        serde_json::to_string_pretty(&old).unwrap()
    }

    #[test]
    fn a_record_an_older_build_wrote_reads_as_todays_its_logs_where_they_are_now() {
        let tree = Tree::new();
        let (old, new) = (tree.path("symphony/nightly"), tree.path("symphony/release"));
        tree.write(
            "symphony/release/runs/a.json",
            &old_record("a", "nightly", &old),
        );
        tree.write(
            "symphony/release/runs/b.json",
            &old_record("b", "release", &old),
        );
        tree.write(
            "symphony/release/runs/c.json",
            &old_record("c", "refresh", &old),
        );
        // A log elsewhere, named by hand, is left as it is.
        let elsewhere = tree.path("elsewhere");
        tree.write(
            "symphony/release/runs/d.json",
            &old_record("d", "nightly", &elsewhere),
        );
        let mut today = run("e", "release", "green", NOW + 1.0);
        today.checks = vec![red_check("verify")];
        today.checks[0].log = old.join("logs/e/verify.log");
        write(&new, &today).unwrap();

        let runs = read(&new);
        let kinds: Vec<(&str, &str, u32)> = (runs.iter())
            .map(|run| (run.id.as_str(), run.kind.as_str(), run.version))
            .collect();
        assert_eq!(
            kinds,
            [
                ("a", "release", VERSION),
                ("b", "out-of-cycle", VERSION),
                ("c", "refresh", VERSION),
                ("d", "release", VERSION),
                ("e", "release", VERSION),
            ]
        );
        assert_eq!(runs[0].checks[0].log, new.join("logs/a/verify.log"));
        assert_eq!(
            runs[0].checks[0].steps[0].log,
            new.join("logs/a/verify-tests.log")
        );
        assert_eq!(runs[3].checks[0].log, elsewhere.join("logs/d/verify.log"));
        assert_eq!(
            runs[4], today,
            "a record of this build's version is read as written"
        );
        // Read where an older build kept them, nothing is renamed but the kinds.
        assert_eq!(
            renamed(runs[0].clone(), &old),
            runs[0],
            "read once, read the same"
        );
        let mut raw: Run = serde_json::from_str(&old_record("f", "nightly", &old)).unwrap();
        raw.refreshed = Some(Refresh {
            release: "release-2026-11-12".into(),
            steps: vec![raw.checks[0].clone()],
            ..Refresh::default()
        });
        let at_old = renamed(raw, &old);
        assert_eq!(at_old.kind, "release");
        assert_eq!(at_old.checks[0].log, old.join("logs/f/verify.log"));
        let refreshed = renamed(
            Run {
                version: 1,
                ..at_old.clone()
            },
            &new,
        );
        assert_eq!(
            refreshed.refreshed.unwrap().steps[0].log,
            new.join("logs/f/verify.log"),
            "a refresh's step too"
        );
    }

    #[test]
    fn the_lock_word_of_an_older_build_names_its_kind_as_today() {
        let tree = Tree::new();
        let folder = tree.path("release");
        for (old, today) in [
            ("nightly", "release"),
            ("release", "out-of-cycle"),
            ("refresh", "refresh"),
            ("", ""),
        ] {
            tree.write(
                "release/running.json",
                &format!(
                    r#"{{"id": "r", "kind": "{old}", "pid": 7, "started_at": 1.0, "until": 2.0}}"#
                ),
            );
            let running = holder(&folder).unwrap();
            assert_eq!((running.kind.as_str(), running.version), (today, VERSION));
        }
        let ours = Running {
            version: VERSION,
            id: "r".into(),
            kind: "release".into(),
            pid: 7,
            started_at: 1.0,
            until: 2.0,
            check: None,
        };
        hold(&folder, &ours).unwrap();
        assert_eq!(
            holder(&folder),
            Some(ours),
            "this build's word is read as said"
        );
    }

    #[test]
    fn an_older_builds_run_found_interrupted_is_recorded_once_as_today() {
        let tree = Tree::new();
        let folder = tree.path("release");
        let mut killed = run("k", "release", "", NOW - 600.0);
        killed.version = 1;
        fs::create_dir_all(tree.path("release/logs/k")).unwrap();
        so_far(&tree.path("release/logs/k"), &killed);
        let found = recover(&folder, None, "now", NOW);
        assert_eq!(found.len(), 1);
        assert_eq!(
            (found[0].kind.as_str(), found[0].version),
            ("out-of-cycle", VERSION)
        );
        assert_eq!(
            read(&folder)[0].kind,
            "out-of-cycle",
            "written as this build's version, it is not renamed twice"
        );
    }

    /// Whether another open of the lock at `path` finds it held.
    fn busy(path: &Path) -> bool {
        let file = File::options().read(true).write(true).open(path).unwrap();
        matches!(file.try_lock(), Err(TryLockError::WouldBlock))
    }

    /// The folder a build older than the rename kept the runs in, made again and its lock held by
    /// that build's run: the lock is this build's own folder's, and the old folder is neither read
    /// nor moved.
    #[test]
    fn the_folder_an_older_build_makes_again_is_neither_read_nor_moved() {
        let tree = Tree::new();
        let common = tree.path("git");
        let old = tree.path("git/symphony/nightly");
        tree.write(
            "git/symphony/nightly/runs/old.json",
            &old_record("old", "nightly", &old),
        );
        tree.write("git/symphony/nightly/run.lock", "");
        let theirs = File::options()
            .read(true)
            .write(true)
            .open(old.join(LOCK))
            .unwrap();
        theirs.try_lock().unwrap();

        let ours = lock(&common).unwrap();
        assert!(
            ours.taken.is_ok(),
            "an older build's lock holds no run here"
        );
        assert_eq!(
            (ours.folder.as_path(), ours.path.as_path()),
            (
                folder(&common).as_path(),
                folder(&common).join(LOCK).as_path()
            )
        );
        assert!(busy(&ours.path));
        assert!(read(&ours.folder).is_empty(), "its records are not read");
        assert!(old.join("runs/old.json").exists(), "nor moved");
        assert!(!folder(&common).join("runs").exists());
        theirs.unlock().unwrap();
    }

    /// A child another thread forks while the run's lock is held keeps a copy of the lock's file
    /// until its exec, and a lock let go by closing its file alone stays held by that copy. The
    /// run's lock lets go of itself, so a run that tries it next takes it at once, whatever child
    /// forked while it was held.
    #[test]
    fn the_runs_lock_let_go_is_free_at_once_though_a_child_forked_meanwhile_has_not_execd() {
        let tree = Tree::new();
        let common = tree.path("git");
        let closed = tree.path("closed");
        fs::create_dir_all(&closed).unwrap();
        let held = lock(&common).unwrap();
        assert!(held.taken.is_ok());
        let bare = File::create(closed.join(LOCK)).unwrap();
        bare.try_lock().unwrap();
        let child = MidSpawn::caught(
            std::process::Command::new("true"),
            std::process::Command::spawn,
        );
        drop((held, bare));
        assert!(
            busy(&closed.join(LOCK)),
            "a lock let go by its close alone is still held by the child's copy of its file"
        );
        let next = lock(&common).unwrap();
        assert!(
            next.taken.is_ok(),
            "the run's lock, let go, is still held by a child forked while it was held"
        );
        drop(child);
    }
}
