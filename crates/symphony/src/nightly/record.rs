//! What each run covered, in the repository's git common dir: `symphony/nightly/runs/ID.json`,
//! one file per run written whole, and each check's log under `logs/ID/`. The newest
//! [`KEPT_RUNS`] records and the logs of the newest [`KEPT_LOGS`] runs stay; a run removes the
//! rest once it wrote its own. `--status` reads them and the release tags, and writes nothing.
//! While a run holds `run.lock`, `running.json` says which run it is, when its time ends and, while
//! a check of it runs, that check's process group ([`Checking`]): what a later run reads when the
//! run was killed while its check ran on. Its record so far is kept beside its logs
//! (`logs/ID/run.json`) until it writes the whole one, so a run that ended without recording itself
//! (killed, or the machine stopped) is recorded as interrupted by the next run that goes
//! ([`recover`]), and its logs stay with it.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::board;
use super::checks::Ran;
use super::git::short;
use super::group::Stamp;
use super::tags::Release;
use crate::clock::stamp_at;

/// The records a run keeps.
pub(crate) const KEPT_RUNS: usize = 60;
/// The runs whose logs a run keeps.
pub(crate) const KEPT_LOGS: usize = 14;
/// A night older than this is overdue: the schedule runs once a day.
const OVERDUE_SECONDS: f64 = 36.0 * 3600.0;
/// The result of a run that ended without recording itself, as a later run recorded it.
pub(crate) const INTERRUPTED: &str = "interrupted";
/// A run's record so far, in its log folder, while it runs.
const SO_FAR: &str = "run.json";

/// A run's record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Run {
    pub(crate) version: u32,
    pub(crate) id: String,
    /// `nightly`, or `release` for an out-of-cycle release of a named commit.
    pub(crate) kind: String,
    /// The candidate.
    pub(crate) commit: String,
    /// The remote's main as fetched.
    pub(crate) tip: String,
    pub(crate) started_at: f64,
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
    /// What it left out, and why: a registered project it could not conciliate.
    #[serde(default)]
    pub(crate) skipped: Vec<String>,
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
    /// What started it, when the cadence did (docs/RELEASES.md): the landings it counted.
    #[serde(default)]
    pub(crate) cause: Option<String>,
    /// What the release it tagged, or the release a `--refresh` named, refreshed on the machine
    /// (docs/RELEASES.md).
    #[serde(default)]
    pub(crate) refreshed: Option<Refresh>,
}

/// A release's refresh: the release, one step for each project the machine serves that builds on
/// the Symphony (its pin and its board), and what it said, the shared server's line last.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Refresh {
    pub(crate) release: String,
    #[serde(default)]
    pub(crate) steps: Vec<Ran>,
    #[serde(default)]
    pub(crate) said: Vec<String>,
}

impl Refresh {
    /// Whether every step of it is green.
    pub(crate) fn green(&self) -> bool {
        self.steps.iter().all(Ran::green)
    }
}

/// The run holding the lock: which, of which kind, its process, when it started and when its time
/// ends, and the check it runs, while it runs one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Running {
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
    crate::text::write_whole(&path, &text).map_err(|e| crate::text::os_error(&e, &path))
}

/// The run that said it holds the lock in `folder`, when one did and it reads.
pub(crate) fn holder(folder: &Path) -> Option<Running> {
    let text = fs::read_to_string(folder.join("running.json")).ok()?;
    serde_json::from_str(&text).ok()
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
}

/// The nightly's folder in the git common dir `common`.
pub(crate) fn folder(common: &Path) -> PathBuf {
    common.join("symphony").join("nightly")
}

/// Where the run `id`'s check logs go in `folder`.
pub(crate) fn logs(folder: &Path, id: &str) -> PathBuf {
    folder.join("logs").join(id)
}

/// Write `run` whole into `folder`; its record so far, beside its logs, goes.
pub(crate) fn write(folder: &Path, run: &Run) -> Result<(), String> {
    let runs = folder.join("runs");
    fs::create_dir_all(&runs).map_err(|e| crate::text::os_error(&e, &runs))?;
    let path = runs.join(format!("{}.json", run.id));
    let text = serde_json::to_string_pretty(run).map_err(|e| e.to_string())? + "\n";
    crate::text::write_whole(&path, &text).map_err(|e| crate::text::os_error(&e, &path))?;
    let _ = fs::remove_file(logs(folder, &run.id).join(SO_FAR));
    Ok(())
}

/// Keep `run`'s record so far in its log folder `logs`, which a later run reads if this one ends
/// without writing its whole record.
pub(crate) fn so_far(logs: &Path, run: &Run) {
    if let Ok(text) = serde_json::to_string_pretty(run) {
        let _ = crate::text::write_whole(&logs.join(SO_FAR), &(text + "\n"));
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
        .filter(|run| run.id == id);
    let heard = last_heard(&logs);
    let mut run = so_far.unwrap_or_else(|| Run {
        version: 1,
        id: id.to_string(),
        kind: (holder.map(|h| h.kind.clone()))
            .filter(|kind| !kind.is_empty())
            .unwrap_or_else(|| "nightly".into()),
        commit: String::new(),
        tip: String::new(),
        started_at: holder.map_or_else(|| heard.map_or(now, |(first, _)| first), |h| h.started_at),
        ended_at: 0.0,
        result: String::new(),
        error: None,
        previous: None,
        checks: Vec::new(),
        skipped: Vec::new(),
        tag: None,
        pushed: false,
        board: Board::default(),
        remote: String::new(),
        unpushed: None,
        cause: None,
        refreshed: None,
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

/// Every run recorded in `folder`, oldest first; a record that does not read is left out.
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

/// One line on `run`: its result, when, how long ago, its commit and what was red or tagged.
fn line(run: &Run, now: f64) -> String {
    let mut said = format!(
        "{}, {} ({}), on {}",
        run.result,
        local(run.started_at),
        age(now, run.started_at),
        short(&run.commit)
    );
    let red: Vec<String> = (run.checks.iter())
        .filter(|ran| !ran.green())
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
/// commit), the last night of `runs` and its age at `now`, the last out-of-cycle release run when
/// one was later, the last interrupted run while its logs are kept (among the newest
/// [`KEPT_LOGS`]) when it is not the last night, and the last green run.
pub(crate) fn status(runs: &[Run], releases: &[(Release, String)], now: f64) -> String {
    let mut text = match releases.first() {
        Some((release, commit)) => {
            format!("newest release: {} ({})\n", release.name(), short(commit))
        }
        None => "newest release: none yet\n".to_string(),
    };
    let night = runs.iter().rev().find(|run| run.kind == "nightly");
    match night {
        Some(night) => {
            let overdue = if now - night.started_at > OVERDUE_SECONDS {
                " (overdue: the nightly runs once a day)"
            } else {
                ""
            };
            text.push_str(&format!("last night: {}{overdue}\n", line(night, now)));
        }
        None => text.push_str("last night: never run\n"),
    }
    if let Some(release) = (runs.iter().rev())
        .find(|run| run.kind == "release")
        .filter(|release| night.is_none_or(|night| release.started_at > night.started_at))
    {
        text.push_str(&format!(
            "last out-of-cycle release: {}\n",
            line(release, now)
        ));
    }
    if let Some(cut) = (runs.iter().rev().take(KEPT_LOGS))
        .find(|run| run.result == INTERRUPTED)
        .filter(|cut| night.is_none_or(|night| night.id != cut.id))
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
/// it and when, then each line it said (a project's pin and board, the shared server) and each red
/// step with how it ended.
pub(crate) fn refresh_status(runs: &[Run], now: f64) -> String {
    let Some((run, refresh)) =
        (runs.iter().rev()).find_map(|run| Some((run, run.refreshed.as_ref()?)))
    else {
        return "last refresh: none recorded\n".into();
    };
    let verdict = if refresh.green() { "green" } else { "red" };
    let mut text = format!(
        "last refresh: {verdict}, of {} by the run {}, {} ({})\n",
        refresh.release,
        run.id,
        local(run.started_at),
        age(now, run.started_at)
    );
    for said in &refresh.said {
        text.push_str(&format!("  {said}\n"));
    }
    for red in refresh.steps.iter().filter(|ran| !ran.green()) {
        text.push_str(&format!(
            "  {} red: {}; log {}\n",
            red.name,
            red.ended(),
            red.log.display()
        ));
    }
    if !refresh.green() {
        text.push_str(BY_HAND);
    }
    text
}

/// What a red refresh leaves to people, which `--status` says under it.
const BY_HAND: &str = "  by hand: a project whose own gate or hooks refuse the move lands its \
                       branch symphony/RELEASE through its own landing; a main checkout off its \
                       default branch or with tracked edits is put right by its owner; then an \
                       orchestrator's `just nightly --refresh` finishes the rest\n";

/// The push that delivers the release `tag` to `remote`, and nothing else.
pub(crate) fn push_line(remote: &str, tag: &str) -> String {
    format!("`git push --no-follow-tags {remote} refs/tags/{tag}:refs/tags/{tag}`")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_tree::Tree;

    const NOW: f64 = 1_800_000_000.0;

    fn run(id: &str, kind: &str, result: &str, started_at: f64) -> Run {
        Run {
            version: 1,
            id: id.into(),
            kind: kind.into(),
            commit: format!("{id:0<40}"),
            tip: format!("{id:0<40}"),
            started_at,
            ended_at: started_at + 60.0,
            result: result.into(),
            error: None,
            previous: None,
            checks: Vec::new(),
            skipped: Vec::new(),
            tag: None,
            pushed: false,
            board: Board::default(),
            remote: "origin".into(),
            unpushed: None,
            cause: None,
            refreshed: None,
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
            error: None,
            timed_out: false,
            interrupted: false,
            load: None,
        }
    }

    #[test]
    fn a_release_missing_from_the_remote_is_said_until_a_run_finds_it_there() {
        let mut refused = run("a", "nightly", "green", NOW - 7200.0);
        refused.tag = Some("release-2026-11-11".into());
        refused.unpushed = Some(vec!["release-2026-11-11".into()]);
        let mut unchanged = run("b", "nightly", "unchanged", NOW - 3600.0);
        unchanged.unpushed = Some(vec!["release-2026-11-11".into()]);
        let stopped = run("c", "nightly", "error", NOW - 60.0);
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
        let folder = tree.path("nightly");
        fs::create_dir_all(&folder).unwrap();
        assert_eq!(holder(&folder), None);
        let running = Running {
            id: "r1".into(),
            kind: "nightly".into(),
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
        tree.write("nightly/running.json", "{ not json");
        assert_eq!(holder(&folder), None);
    }

    #[test]
    fn runs_are_written_whole_and_read_back_oldest_first() {
        let tree = Tree::new();
        let folder = tree.path("nightly");
        assert!(read(&folder).is_empty(), "nothing recorded yet");
        let later = run("b", "nightly", "green", NOW);
        let earlier = run("a", "nightly", "red", NOW - 100.0);
        write(&folder, &later).unwrap();
        write(&folder, &earlier).unwrap();
        tree.write("nightly/runs/broken.json", "{ not json");
        tree.write("nightly/runs/notes.txt", "not a record");
        assert_eq!(read(&folder), [earlier, later]);
        assert_eq!(
            tree.names("nightly/runs"),
            ["a.json", "b.json", "broken.json", "notes.txt"]
        );
    }

    #[test]
    fn a_run_keeps_the_newest_records_and_the_newest_runs_logs() {
        let tree = Tree::new();
        let folder = tree.path("nightly");
        for n in 0..5 {
            let id = format!("r{n}");
            write(&folder, &run(&id, "nightly", "green", NOW + f64::from(n))).unwrap();
            tree.write(&format!("nightly/logs/{id}/verify.log"), "log");
        }
        tree.write("nightly/logs/stray/verify.log", "a run with no record");
        prune(&folder, 3, 2);
        assert_eq!(
            tree.names("nightly/runs"),
            ["r2.json", "r3.json", "r4.json"]
        );
        assert_eq!(tree.names("nightly/logs"), ["r3", "r4"]);
        prune(&tree.path("none"), 3, 2);
    }

    #[test]
    fn the_last_green_run_is_the_newest_green_one() {
        let runs = [
            run("a", "nightly", "green", NOW - 300.0),
            run("b", "release", "green", NOW - 200.0),
            run("c", "nightly", "red", NOW - 100.0),
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
    fn the_status_names_the_newest_release_the_last_night_and_the_last_green() {
        let mut night = run("night", "nightly", "red", NOW - 3.0 * 3600.0);
        night.checks = vec![red_check("verify")];
        night.board.filed = vec!["tests/nightly-red-verify".into()];
        let mut green = run("green", "nightly", "green", NOW - 27.0 * 3600.0);
        green.tag = Some("release-2026-11-11".into());
        green.pushed = true;
        let releases = [(
            Release::parse("release-2026-11-11").unwrap(),
            "c".repeat(40),
        )];
        let said = status(&[green, night], &releases, NOW);
        let lines: Vec<&str> = said.lines().collect();
        assert_eq!(
            lines[0],
            "newest release: release-2026-11-11 (cccccccccccc)"
        );
        assert!(lines[1].starts_with("last night: red, "), "{said}");
        assert!(lines[1].contains("(3 h ago), on night0000000"), "{said}");
        assert!(
            lines[1].ends_with("; red: verify (exit 101); red tasks: tests/nightly-red-verify"),
            "{said}"
        );
        assert!(lines[2].starts_with("last green: "), "{said}");
        assert!(lines[2].ends_with("(27 h ago), on green0000000"), "{said}");
        assert_eq!(lines.len(), 3, "{said}");
    }

    #[test]
    fn the_status_of_a_machine_that_never_ran_one_says_so() {
        assert_eq!(
            status(&[], &[], NOW),
            "newest release: none yet\nlast night: never run\nlast green: none recorded\n"
        );
    }

    #[test]
    fn an_old_night_is_overdue_and_a_later_release_run_is_named() {
        let night = run("night", "nightly", "green", NOW - 40.0 * 3600.0);
        let mut release = run("rel", "release", "green", NOW - 3600.0);
        release.tag = Some("release-2026-11-12.2".into());
        let said = status(&[night.clone(), release.clone()], &[], NOW);
        assert!(
            said.contains("(overdue: the nightly runs once a day)"),
            "{said}"
        );
        assert!(
            said.contains("last out-of-cycle release: green, ")
                && said.contains("; tagged release-2026-11-12.2, not pushed"),
            "{said}"
        );
        let mut older = release;
        older.started_at = NOW - 50.0 * 3600.0;
        let said = status(&[older, night], &[], NOW);
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
        let folder = tree.path("nightly");
        // The run running.json named, killed while `verify` ran, after `lint` passed.
        let mut killed = run("k1", "nightly", "", NOW - 3600.0);
        let mut lint = red_check("lint");
        lint.exit_code = Some(0);
        killed.checks = vec![lint];
        fs::create_dir_all(tree.path("nightly/logs/k1")).unwrap();
        so_far(&tree.path("nightly/logs/k1"), &killed);
        written_at(&tree, "nightly/logs/k1/lint.log", "ok", NOW - 3500.0);
        written_at(&tree, "nightly/logs/k1/verify.log", "...", NOW - 3000.0);
        let holder = Running {
            id: "k1".into(),
            kind: "nightly".into(),
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
        written_at(&tree, "nightly/logs/k0/verify.log", "...", NOW - 7200.0);
        let mut between = run("k2", "release", "", NOW - 1800.0);
        between.checks = vec![red_check("lint")];
        fs::create_dir_all(tree.path("nightly/logs/k2")).unwrap();
        so_far(&tree.path("nightly/logs/k2"), &between);
        written_at(&tree, "nightly/logs/k2/lint.log", "red", NOW - 1700.0);
        // A run recorded, and the run that finds them, which has its own logs and no record yet.
        write(&folder, &run("done", "nightly", "green", NOW - 9000.0)).unwrap();
        tree.write("nightly/logs/done/verify.log", "ok");
        tree.write("nightly/logs/now/verify.log", "...");

        let found = recover(&folder, Some(&holder), "now", NOW);
        let ids: Vec<&str> = found.iter().map(|run| run.id.as_str()).collect();
        assert_eq!(ids, ["k0", "k1", "k2"], "oldest first");
        let logs = |id: &str| {
            tree.path(&format!("nightly/logs/{id}"))
                .display()
                .to_string()
        };
        let k0 = &found[0];
        assert_eq!(
            (k0.kind.as_str(), k0.result.as_str(), k0.commit.as_str()),
            ("nightly", INTERRUPTED, "")
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
        assert_eq!(k2.kind, "release");
        assert!(
            k2.error.as_deref().unwrap().starts_with(
                "ended without recording (killed, or the machine stopped) after `lint` ended;"
            ),
            "{k2:?}"
        );

        // Recorded, they are runs like any other, their records so far gone and their logs kept.
        let all: Vec<String> = read(&folder).into_iter().map(|run| run.id).collect();
        assert_eq!(all, ["done", "k0", "k1", "k2"]);
        assert!(!tree.path("nightly/logs/k1/run.json").exists());
        assert!(
            recover(&folder, Some(&holder), "now", NOW).is_empty(),
            "once"
        );
        prune(&folder, KEPT_RUNS, KEPT_LOGS);
        assert_eq!(
            tree.names("nightly/logs"),
            ["done", "k0", "k1", "k2"],
            "each recorded run keeps its logs"
        );
    }

    #[test]
    fn a_run_that_left_nothing_but_said_it_ran_is_recorded_before_any_check() {
        let tree = Tree::new();
        let folder = tree.path("nightly");
        let holder = Running {
            id: "k".into(),
            kind: String::new(),
            pid: 5,
            started_at: NOW - 60.0,
            until: NOW,
            check: None,
        };
        let found = recover(&folder, Some(&holder), "now", NOW);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "nightly");
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
        let mut cut = run("cut", "nightly", INTERRUPTED, NOW - 7200.0);
        cut.error = Some(
            "ended without recording (killed, or the machine stopped) while `verify` ran".into(),
        );
        let green = run("green", "nightly", "green", NOW - 3600.0);
        let said = status(&[cut.clone(), green.clone()], &[], NOW);
        assert!(said.contains("last night: green, "), "{said}");
        assert!(
            said.contains(&format!(
                "last interrupted run: interrupted, {} (2 h ago), on cut000000000; ended without \
                 recording (killed, or the machine stopped) while `verify` ran\n",
                local(NOW - 7200.0)
            )),
            "{said}"
        );
        let said = status(std::slice::from_ref(&cut), &[], NOW);
        assert!(said.contains("last night: interrupted, "), "{said}");
        assert!(!said.contains("last interrupted run"), "said once: {said}");
        let mut later: Vec<Run> = vec![cut];
        for n in 0..u32::try_from(KEPT_LOGS).unwrap() {
            later.push(run(
                &format!("n{n:02}"),
                "nightly",
                "green",
                NOW - 3000.0 + f64::from(n),
            ));
        }
        let said = status(&later, &[], NOW);
        assert!(!said.contains("interrupted"), "its logs are gone: {said}");
    }

    #[test]
    fn a_run_that_could_not_check_says_why() {
        let mut night = run("night", "nightly", "error", NOW - 60.0);
        night.error = Some("cannot fetch origin".into());
        let said = status(&[night], &[], NOW);
        assert!(
            said.contains("last night: error, ") && said.contains("; cannot fetch origin\n"),
            "{said}"
        );
    }

    #[test]
    fn the_newest_refresh_is_said_with_each_line_and_each_red_step() {
        assert_eq!(refresh_status(&[], NOW), "last refresh: none recorded\n");
        let mut older = run("older", "nightly", "green", NOW - 7200.0);
        older.refreshed = Some(Refresh {
            release: "release-2026-11-11".into(),
            steps: Vec::new(),
            said: vec!["an older line".into()],
        });
        let mut newer = run("newer", "nightly", "green", NOW - 3600.0);
        let mut green = red_check("refresh-alpha");
        green.exit_code = Some(0);
        newer.refreshed = Some(Refresh {
            release: "release-2026-11-12".into(),
            steps: vec![green, red_check("refresh-beta")],
            said: vec!["alpha: its pin is on the branch symphony/release-2026-11-12".into()],
        });
        let after = run("after", "nightly", "red", NOW - 60.0);
        assert_eq!(
            refresh_status(&[older, newer, after], NOW),
            format!(
                "last refresh: red, of release-2026-11-12 by the run newer, {} (1 h ago)\n  \
                 alpha: its pin is on the branch symphony/release-2026-11-12\n  \
                 refresh-beta red: exit 101; log /l\n  \
                 by hand: a project whose own gate or hooks refuse the move lands its branch \
                 symphony/RELEASE through its own landing; a main checkout off its default \
                 branch or with tracked edits is put right by its owner; then an orchestrator's \
                 `just nightly --refresh` finishes the rest\n",
                local(NOW - 3600.0)
            )
        );
        // A green one leaves nothing by hand.
        let mut green = run("green", "nightly", "green", NOW - 60.0);
        green.refreshed = Some(Refresh {
            release: "release-2026-11-12".into(),
            steps: Vec::new(),
            said: vec!["alpha: its board is at schema 17, its pin's: nothing to move".into()],
        });
        assert!(!refresh_status(&[green], NOW).contains("by hand"));
    }
}
