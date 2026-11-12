//! What each run covered, in the repository's git common dir: `symphony/nightly/runs/ID.json`,
//! one file per run written whole, and each check's log under `logs/ID/`. The newest
//! [`KEPT_RUNS`] records and the logs of the newest [`KEPT_LOGS`] runs stay; a run removes the
//! rest once it wrote its own. `--status` reads them and the release tags, and writes nothing.
//! While a run holds `run.lock`, `running.json` says which run it is and when its time ends.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::checks::Ran;
use super::git::short;
use super::tags::Release;
use crate::clock::stamp_at;

/// The records a run keeps.
pub(crate) const KEPT_RUNS: usize = 60;
/// The runs whose logs a run keeps.
pub(crate) const KEPT_LOGS: usize = 14;
/// A night older than this is overdue: the schedule runs once a day.
const OVERDUE_SECONDS: f64 = 36.0 * 3600.0;

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
    /// `green`, `red`, `unchanged` (the candidate was released already) or `error` (no check ran).
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
}

/// The run holding the lock: which, its process, when it started and when its time ends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Running {
    pub(crate) id: String,
    pub(crate) pid: u32,
    pub(crate) started_at: f64,
    pub(crate) until: f64,
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

/// Write `run` whole into `folder`.
pub(crate) fn write(folder: &Path, run: &Run) -> Result<(), String> {
    let runs = folder.join("runs");
    fs::create_dir_all(&runs).map_err(|e| crate::text::os_error(&e, &runs))?;
    let path = runs.join(format!("{}.json", run.id));
    let text = serde_json::to_string_pretty(run).map_err(|e| e.to_string())? + "\n";
    crate::text::write_whole(&path, &text).map_err(|e| crate::text::os_error(&e, &path))
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
/// one was later, and the last green run.
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
            pid: 7,
            started_at: NOW,
            until: NOW + 60.0,
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
}
