//! Symphony's own run records ([MACHINE_WORK](../../../../../docs/MACHINE_WORK.md)): `tools/cargo.sh`
//! writes one file per build or test run in `sf-runs/` in the git common dir, so the runs of every
//! worktree are read in one place. Its first line, written as the run starts, says what runs and
//! where; a second, written as it ends, its exit status and how long it took. A run cut short leaves
//! no second line: once its process is gone it is no work, and its record is removed.
//!
//! This is the built-in reader, used when a project names no reader of its own. Liveness is read
//! anew at every tick (the process alive and started no later than its record), so a run is placed
//! by current evidence for as long as it runs, however long, and leaves no history behind.

use super::{Freshness, Kind, Machine, Operation, State};
use crate::Task;
use crate::rows::TaskSessionRow;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The records' folder in the git common dir.
pub const DIR: &str = "sf-runs";
/// A record's file name ends so; anything else in the folder is not a run.
const SUFFIX: &str = ".run";
/// How many records one read looks at; the rest wait for the next.
const MAX_RECORDS: usize = 4096;
/// A record no longer than this ends in no newline when its writer is still writing it.
const MAX_BYTES: u64 = 64 * 1024;
/// A finished run no task claims is removed this long after it ended (one day).
pub const UNCLAIMED_SECONDS: f64 = 86_400.0;
/// How far a process's start may be dated after its record's write and still be the run's: the
/// wrapper writes the record as its first act, so its start is before it, but stamps are rounded.
const START_SLACK: f64 = 1.0;

/// The first line: what runs, where, and for whom.
#[derive(Deserialize)]
struct Started {
    v: u8,
    kind: String,
    pid: i64,
    checkout: String,
    #[serde(default)]
    branch: String,
    #[serde(default)]
    tip: String,
    #[serde(default)]
    handle: String,
    #[serde(default)]
    command: String,
    #[serde(default)]
    launcher: String,
}

/// The second line: how it ended.
#[derive(Deserialize)]
struct Ending {
    exit: i32,
    seconds: f64,
}

/// One run, as its record says.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    /// Its record's file name: the run's id.
    pub id: String,
    /// Build or test.
    pub kind: Kind,
    /// The wrapper's process, which lives as long as the run.
    pub pid: i64,
    /// The checkout it runs in, physical.
    pub checkout: String,
    /// The branch checked out there when it started, "" when detached.
    pub branch: String,
    /// The commit checked out there when it started, "" when unknown.
    pub tip: String,
    /// The board session it ran for (`TASKS_SESSION`), when its caller said.
    pub handle: Option<String>,
    /// The command, as `cargo ARGS`.
    pub command: String,
    /// The binary `tools/rs-bin.sh` built for running it: a launcher's build, never evidence.
    pub launcher: Option<String>,
    /// When it started, in seconds since the epoch.
    pub started: f64,
    /// How it ended; none while it runs.
    pub ended: Option<Ended>,
}

/// How a run ended.
#[derive(Clone, Debug, PartialEq)]
pub struct Ended {
    /// Cargo's exit status; 128 and a signal's number for one a signal ended.
    pub exit: i32,
    /// How long it ran, in whole seconds.
    pub seconds: f64,
    /// When it ended, in seconds since the epoch.
    pub at: f64,
}

/// What one read of the folder found.
#[derive(Debug, Default)]
pub struct Records {
    /// Runs going now: the process alive and started no later than its record.
    pub going: Vec<Run>,
    /// Runs that ended, with their end.
    pub finished: Vec<Run>,
    /// Records that read as no run: cut short by something other than their writer, or another
    /// program's file. Left where they are.
    pub unreadable: usize,
}

/// The run the record `id` holds, written at `written` (its file's modification time): none when
/// its first line is not whole or not a run. A second line that is not whole yet is no end.
fn parse(id: &str, text: &str, written: f64) -> Option<Run> {
    let mut lines = text.split_inclusive('\n');
    let first = lines.next().filter(|line| line.ends_with('\n'))?;
    let started: Started = serde_json::from_str(first).ok()?;
    let kind = match started.kind.as_str() {
        "build" => Kind::Build,
        "test" => Kind::Test,
        _ => return None,
    };
    if started.v != 1 || started.pid <= 0 || !Path::new(&started.checkout).is_absolute() {
        return None;
    }
    let ending = lines
        .next()
        .filter(|line| line.ends_with('\n'))
        .and_then(|line| serde_json::from_str::<Ending>(line).ok())
        .filter(|ending| ending.seconds.is_finite() && ending.seconds >= 0.0);
    let text = |value: String| Some(value).filter(|value| !value.is_empty());
    Some(Run {
        id: id.to_owned(),
        kind,
        pid: started.pid,
        checkout: started.checkout,
        branch: started.branch,
        tip: started.tip,
        handle: text(started.handle),
        command: started.command,
        launcher: text(started.launcher),
        // A record is written once as its run starts and once more as it ends, so until then its
        // modification time is its start.
        started: ending
            .as_ref()
            .map_or(written, |ending| written - ending.seconds),
        ended: ending.map(|ending| Ended {
            exit: ending.exit,
            seconds: ending.seconds,
            at: written,
        }),
    })
}

/// A record's text and its modification time in seconds since the epoch; none when it cannot be
/// read (it was removed meanwhile, or is too long to be a record).
fn load(path: &Path) -> Option<(String, f64)> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return None;
    }
    let written = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs_f64();
    Some((std::fs::read_to_string(path).ok()?, written))
}

/// Every record in the run folder of the git common dir `common`. `alive(pid, written)` says
/// whether the process `pid` is the one that wrote its record at `written` ([`alive`]). A record
/// with no end whose process is gone is read once more, in case it ended meanwhile, and otherwise
/// removed: a run cut short is no work and no evidence. No folder is no run.
pub fn read(common: &Path, alive: &dyn Fn(i64, f64) -> bool) -> Result<Records, String> {
    let dir = common.join(DIR);
    let mut records = Records::default();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(records),
        Err(error) => return Err(format!("run records {}: {error}", dir.display())),
    };
    let mut names: Vec<String> = entries
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.ends_with(SUFFIX) && !name.starts_with('.'))
        .collect();
    names.sort();
    if names.len() > MAX_RECORDS {
        records.unreadable += names.len() - MAX_RECORDS;
        names.truncate(MAX_RECORDS);
    }
    for name in names {
        let path = dir.join(&name);
        let Some(run) = load(&path).and_then(|(text, written)| parse(&name, &text, written)) else {
            // Removed since it was listed, or not a run.
            if path.exists() {
                records.unreadable += 1;
            }
            continue;
        };
        if run.ended.is_some() {
            records.finished.push(run);
        } else if alive(run.pid, run.started) {
            records.going.push(run);
        } else {
            match load(&path).and_then(|(text, written)| parse(&name, &text, written)) {
                Some(again) if again.ended.is_some() => records.finished.push(again),
                _ => remove(&path),
            }
        }
    }
    Ok(records)
}

/// Remove a record; one already gone (another reader removed it) is as good.
fn remove(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Whether `pid` is the process that wrote a record at `written`: it is alive, and the kernel dates
/// its start no later than that write (a pid used again by a later process started after it). A
/// start that cannot be read leaves the pid's own answer.
pub fn alive(pid: i64, written: f64) -> bool {
    use crate::sys::{Start, boot_clock, pid_alive, process_starts};
    if !pid_alive(pid) {
        return false;
    }
    match process_starts(&[pid]).pop() {
        Some(Start::At(at)) => at <= written + START_SLACK,
        Some(Start::AfterBoot(started)) => match boot_clock() {
            // Its age on the clock since boot against the record's on the wall clock.
            Some(now) if now.boot == started.boot => {
                let age = now.at - started.at;
                let record = crate::sweep::trigger::now() - written;
                age + START_SLACK >= record
            }
            _ => true,
        },
        Some(Start::Ended) => false,
        Some(Start::Unreadable) | None => true,
    }
}

/// Who a run works for: the board session and the task, as [`Owners::of`] binds them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Owner {
    pub handle: Option<String>,
    pub task: Option<String>,
}

/// What a run is attributed by: the board's tasks, each in-process agent's binding by handle, and
/// the launch records.
pub struct Owners<'a> {
    pub tasks: &'a [Task],
    pub bindings: &'a BTreeMap<String, crate::Binding>,
    pub records: &'a [TaskSessionRow],
}

const FINISHED: [&str; 2] = ["done", "cancelled"];

/// Whether `task` is the one the branch `branch` is named for: its slug, the branch a review task
/// was filed for, or the branch its executor recorded.
fn named_for(task: &Task, branch: &str) -> bool {
    !branch.is_empty()
        && (task.slug == branch
            || crate::on_review::branch_of(&task.slug) == branch
            || task.branch.as_deref() == Some(branch))
}

/// The path with its links resolved, or as written when it cannot be.
fn real(path: &str) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path))
}

impl Owners<'_> {
    /// The run's session and task. The session is the one its caller named (`TASKS_SESSION`), else
    /// the one session working in its checkout: a subagent whose binding recorded it there, or an
    /// agent launched there whose launch is open; two or more there name none. The task is the open
    /// one that session holds as executor or reviewer, an instruction subtask giving way to its
    /// parent when it holds both; of several, the one the branch is named for. Without one, the
    /// open task the branch is named for, its slug first. Never a finished task, nor a guess between
    /// two.
    pub fn of(&self, run: &Run) -> Owner {
        let handle = run
            .handle
            .clone()
            .or_else(|| self.working_in(&run.checkout));
        let held = handle
            .as_deref()
            .and_then(|handle| self.held(handle, &run.branch));
        let task = held.or_else(|| self.named(&run.branch));
        Owner { handle, task }
    }

    fn working_in(&self, checkout: &str) -> Option<String> {
        let here = real(checkout);
        let bound = (self.bindings.iter())
            .filter(|(_, binding)| binding.in_process && !binding.worktree.is_empty())
            .filter(|(_, binding)| real(&binding.worktree) == here)
            .map(|(handle, _)| handle.as_str());
        let launched = (self.records.iter())
            .filter(|record| record.closed.is_none() && !record.worktree.is_empty())
            .filter(|record| real(&record.worktree) == here)
            .map(|record| record.handle.as_str());
        let mut handles: Vec<&str> = bound.chain(launched).collect();
        handles.sort_unstable();
        handles.dedup();
        match handles[..] {
            [one] => Some(one.to_owned()),
            _ => None,
        }
    }

    fn held(&self, handle: &str, branch: &str) -> Option<String> {
        let held: Vec<&Task> = (self.tasks.iter())
            .filter(|task| !FINISHED.contains(&task.status.as_str()))
            .filter(|task| {
                (task.roles.iter()).any(|role| {
                    role.handle == handle && matches!(role.role.as_str(), "executor" | "reviewer")
                })
            })
            .collect();
        let held: Vec<&Task> = (held.iter())
            .filter(|task| {
                !(task.instruction
                    && (task.parent.as_deref())
                        .is_some_and(|parent| held.iter().any(|other| other.slug == parent)))
            })
            .copied()
            .collect();
        match held[..] {
            [one] => Some(one.slug.clone()),
            _ => {
                let named: Vec<&&Task> = held.iter().filter(|t| named_for(t, branch)).collect();
                match named[..] {
                    [one] => Some(one.slug.clone()),
                    _ => None,
                }
            }
        }
    }

    fn named(&self, branch: &str) -> Option<String> {
        let open = || (self.tasks.iter()).filter(|task| !FINISHED.contains(&task.status.as_str()));
        if branch.is_empty() {
            return None;
        }
        if let Some(task) = open().find(|task| task.slug == branch) {
            return Some(task.slug.clone());
        }
        let named: Vec<&Task> = open().filter(|task| named_for(task, branch)).collect();
        match named[..] {
            [one] => Some(one.slug.clone()),
            _ => None,
        }
    }
}

/// A going run as the board's machine work holds it: an operation of `machine`, running, its id the
/// record's, attributed to `owner`.
pub fn operation(run: &Run, machine: &str, owner: Owner) -> Operation {
    Operation {
        machine_id: Some(machine.to_owned()),
        operation_id: Some(run.id.clone()),
        parent_id: None,
        handle: owner.handle,
        task: owner.task,
        checkout: run.checkout.clone(),
        recipe: run.command.clone(),
        kind: run.kind.clone(),
        state: State::Running,
        resource: "cargo".into(),
        reason: "run record".into(),
        phase_since: Some(run.started),
    }
}

/// This machine as its run records show it: `operations` running now, read at this tick, in an
/// inventory that is partial (other projects' and other tools' builds are not in it), so its
/// occupancy stays unknown.
pub fn machine(id: &str, label: &str, sequence: u64, operations: Vec<Operation>) -> Machine {
    Machine {
        machine_id: id.to_owned(),
        label: label.to_owned(),
        producer_id: format!("runs:{id}"),
        sequence,
        age_ms: Some(0),
        freshness: Freshness::Incomplete,
        complete: false,
        reason: "run records of this project read now; other work on this machine unknown".into(),
        counts: None,
        operations,
    }
}

/// What to keep of the finished runs, each with the task it was bound to: the newest per task (its
/// last finished run, the evidence its page can show) and an unclaimed one for
/// [`UNCLAIMED_SECONDS`] after it ended. The rest are removed from `common`'s run folder: older runs
/// of a task, a launcher's builds, and unclaimed runs past that time.
pub fn keep_last(
    common: &Path,
    finished: Vec<(Run, Option<String>)>,
    now: f64,
) -> BTreeMap<String, Run> {
    let dir = common.join(DIR);
    let mut last: BTreeMap<String, Run> = BTreeMap::new();
    for (run, task) in finished {
        let ended = run.ended.as_ref().map_or(run.started, |ended| ended.at);
        match task {
            _ if run.launcher.is_some() => remove(&dir.join(&run.id)),
            None if now - ended > UNCLAIMED_SECONDS => remove(&dir.join(&run.id)),
            None => {}
            Some(task) => {
                let newer = |kept: &Run| {
                    let at = |run: &Run| run.ended.as_ref().map_or(run.started, |e| e.at);
                    (at(&run), &run.id) > (at(kept), &kept.id)
                };
                match last.get(&task) {
                    Some(kept) if !newer(kept) => remove(&dir.join(&run.id)),
                    Some(kept) => {
                        remove(&dir.join(&kept.id));
                        last.insert(task, run);
                    }
                    None => {
                        last.insert(task, run);
                    }
                }
            }
        }
    }
    last
}

#[cfg(test)]
#[path = "tests/runs.rs"]
mod tests;
