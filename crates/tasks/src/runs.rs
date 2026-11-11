//! A project's runs as the board's recorder keeps them (docs/RUNS.md): the day files
//! `symphony/runs/YYYY-MM-DD.jsonl` in the git common dir, one [`symphony_api::RunLine`] a line. A
//! recorded operation that is a run, or a step of one ([`recorded::Run`]), appends a start line when
//! its command starts and an end line when it ends, each to the file of the UTC day it says. Lines
//! are appended whole under the folder's lock, and day files more than [`KEPT_DAYS`] days old go
//! then. `symphony/runs/logs/` is not this module's.

use crate::cli::Env;
use crate::cli::machine_work::recorded::{self, Record, Step};
use crate::sys::{self, FileLock};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Write as _};
use std::os::unix::fs::{FileExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use symphony_api::{End, RUN_SCHEMA, RunLine, run_day};

/// The day files' folder in the git common dir: in the one folder of Symphony's there (DECISIONS:
/// symphony-own-names).
pub const FOLDER: &str = "symphony/runs";
/// Names the day files' folder in place of the common dir's: a test's own.
pub const FOLDER_VAR: &str = "SYMPHONY_RUNS";
/// Held while a line is appended or the folder pruned.
pub const LOCK: &str = "lock";
/// How many days a day file is kept after its day.
pub const KEPT_DAYS: u32 = 90;
/// The longest an append waits for the lock; then it appends without it, in one write still.
const LOCK_WAIT: Duration = Duration::from_secs(2);
const DAY_S: f64 = 86_400.0;

/// Where `env`'s runs are kept: the folder [`FOLDER_VAR`] names; none for a run whose work folder
/// `TASKS_WORK` names, or whose board `TASKS_SERVER` does (a test's), without it; else [`FOLDER`]
/// in the git common dir of its working directory, `symphony/runs.<database>/` for a board
/// database other than the live one.
pub fn folder(env: &Env) -> Option<PathBuf> {
    if let Some(named) = env.var(FOLDER_VAR) {
        return Some(PathBuf::from(named));
    }
    if env.var(recorded::FOLDER_VAR).is_some() || env.var("TASKS_SERVER").is_some() {
        return None;
    }
    let common = crate::git::common_dir(&env.cwd).ok()?;
    Some(common.join(match env.var("TASKS_DATABASE") {
        Some(database) if database != crate::DATABASE => format!("{FOLDER}.{database}"),
        _ => FOLDER.to_string(),
    }))
}

/// The live board's runs in the git common dir `common`: what the Symphony reads.
pub fn in_common(common: &Path) -> PathBuf {
    common.join(FOLDER)
}

/// The line `record` writes, as it stands: none for an operation that is no run or step. Its end
/// is set once it finished.
pub fn line(record: &Record) -> Option<RunLine> {
    let run = record.run.as_ref()?;
    let finished = record.state == Step::Finished;
    Some(RunLine {
        schema: RUN_SCHEMA,
        run: record.operation_id.clone(),
        parent: run.step.as_ref().and(record.parent_id.clone()),
        project: run.project.clone(),
        machine: record.machine_id.clone(),
        pipeline: run.pipeline.clone(),
        step: run.step.clone(),
        label: record.label.clone(),
        task: record.task.clone(),
        handle: record.handle.clone(),
        trigger: run.trigger.clone(),
        branch: record.branch.clone(),
        commit: record.commit.clone(),
        tree: run.tree.clone(),
        dirty: run.dirty,
        queued: record.queued_at,
        started: record.started_at,
        ended: record.ended_at.filter(|_| finished),
        end: finished
            .then(|| End::of(record.started_at.is_some(), record.exit_code, record.signal)),
        exit: record.exit_code,
        signal: record.signal,
        cpu_s: run.cpu_s.filter(|_| finished),
        rerun_of: run.rerun_of.clone(),
        log: None,
        log_bytes: None,
        suites: None,
        failed: None,
    })
}

/// The folder's lock, waited for at most [`LOCK_WAIT`]; none when it stayed held that long.
fn locked(folder: &Path) -> io::Result<Option<FileLock>> {
    let file = regular(
        &folder.join(LOCK),
        OpenOptions::new().read(true).write(true).create(true),
    )?;
    let deadline = Instant::now() + LOCK_WAIT;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(Some(FileLock(file))),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Error(error)) => return Err(error),
        }
    }
}

/// `path` opened as `options` say, never through a link and never waiting on a FIFO, and only when
/// it is a regular file.
fn regular(path: &Path, options: &mut OpenOptions) -> io::Result<File> {
    let file = options
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other(format!(
            "{}: not a regular file",
            path.display()
        )));
    }
    Ok(file)
}

/// Whether `file` is empty or ends a line: else a writer before was cut short in its line.
fn ends_a_line(file: &File) -> io::Result<bool> {
    let length = file.metadata()?.len();
    if length == 0 {
        return Ok(true);
    }
    let mut last = [0];
    file.read_exact_at(&mut last, length - 1)?;
    Ok(last[0] == b'\n')
}

/// `line` appended to the file of its day in `folder` at `now`, made as needed: whole, in one write,
/// under the folder's lock (after a newline when a writer before was cut short, so each line stays
/// its own); then the day files more than [`KEPT_DAYS`] days before `now`'s go. A lock held past
/// [`LOCK_WAIT`] is not waited for: the line is appended in one write, and nothing pruned.
pub fn append(folder: &Path, line: &RunLine, now: f64) -> io::Result<()> {
    fs::create_dir_all(folder)?;
    let mut text = serde_json::to_vec(line).map_err(io::Error::other)?;
    text.push(b'\n');
    let held = locked(folder)?;
    let path = folder.join(format!("{}.jsonl", line.day()));
    let mut file = regular(
        &path,
        OpenOptions::new().read(true).append(true).create(true),
    )?;
    if held.is_some() && !ends_a_line(&file)? {
        text.insert(0, b'\n');
    }
    file.write_all(&text)?;
    if held.is_some() {
        prune(folder, now);
    }
    Ok(())
}

/// Whether `name` is a day file's: `YYYY-MM-DD.jsonl`.
fn day_file(name: &str) -> bool {
    let Some(day) = name.strip_suffix(".jsonl") else {
        return false;
    };
    day.len() == 10
        && day.bytes().enumerate().all(|(at, byte)| match at {
            4 | 7 => byte == b'-',
            _ => byte.is_ascii_digit(),
        })
}

/// Each day file in `folder` of a day more than [`KEPT_DAYS`] days before `now`'s removed; nothing
/// else there, and no link, folder or FIFO under such a name.
fn prune(folder: &Path, now: f64) {
    let oldest = format!("{}.jsonl", run_day(now - f64::from(KEPT_DAYS) * DAY_S));
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if day_file(name)
            && name < oldest.as_str()
            && entry.file_type().is_ok_and(|kind| kind.is_file())
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// The lines of `day`'s file in `folder` that hold `needle`, read in order; a line that is not one
/// (cut short) is passed over.
fn lines_holding(folder: &Path, day: &str, needle: &str) -> Vec<RunLine> {
    if needle.is_empty() {
        return Vec::new();
    }
    let Ok(bytes) = fs::read(folder.join(format!("{day}.jsonl"))) else {
        return Vec::new();
    };
    bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| {
            line.windows(needle.len())
                .any(|window| window == needle.as_bytes())
        })
        .filter_map(|line| serde_json::from_slice(line).ok())
        .collect()
}

/// The run of `pipeline` and `step` on `tree` that a clean run asked for at `before` repeats: the
/// newest one that ended by then, also clean, in the day files of that day and the day before.
pub fn rerun_of(
    folder: &Path,
    pipeline: &str,
    step: Option<&str>,
    tree: &str,
    before: f64,
) -> Option<String> {
    let mut found: Option<RunLine> = None;
    for day in [run_day(before - DAY_S), run_day(before)] {
        for line in lines_holding(folder, &day, tree) {
            let ended = line.ended.filter(|ended| *ended <= before);
            if line.end.is_some()
                && ended.is_some()
                && line.dirty == Some(false)
                && line.tree.as_deref() == Some(tree)
                && line.pipeline == pipeline
                && line.step.as_deref() == step
                && found.as_ref().is_none_or(|newest| newest.ended <= ended)
            {
                found = Some(line);
            }
        }
    }
    found.map(|line| line.run)
}

/// The tree of `commit` and whether the checkout at `cwd` differs from it (a tracked change, or a
/// file git does not ignore), as git says; none for what it cannot say. `checked` names a commit
/// the run checks that is not the checkout's: its tree, and clean.
pub fn tree_and_dirty(cwd: &Path, commit: &str, checked: bool) -> (Option<String>, Option<bool>) {
    let git = |args: &[&str]| {
        let done = sys::output(sys::command("git").arg("-C").arg(cwd).args(args)).ok()?;
        done.status
            .success()
            .then(|| String::from_utf8_lossy(&done.stdout).into_owned())
    };
    let tree = git(&[
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("{commit}^{{tree}}"),
    ])
    .map(|tree| tree.trim().to_string())
    .filter(|tree| matches!(tree.len(), 40 | 64) && tree.bytes().all(|b| b.is_ascii_hexdigit()));
    let dirty = if checked {
        tree.as_ref().map(|_| false)
    } else {
        git(&[
            "--no-optional-locks",
            "status",
            "--porcelain",
            "--untracked-files=normal",
        ])
        .map(|said| !said.is_empty())
    };
    (tree, dirty)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("symphony-runs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn ended(run: &str, at: f64, tree: &str, dirty: bool) -> RunLine {
        RunLine {
            schema: RUN_SCHEMA,
            run: run.into(),
            parent: None,
            project: None,
            machine: None,
            pipeline: "verify".into(),
            step: None,
            label: "just verify".into(),
            task: None,
            handle: None,
            trigger: "owner".into(),
            branch: None,
            commit: None,
            tree: Some(tree.into()),
            dirty: Some(dirty),
            queued: at - 10.0,
            started: Some(at - 9.0),
            ended: Some(at),
            end: Some(End::Red),
            exit: Some(1),
            signal: None,
            cpu_s: Some(1.0),
            rerun_of: None,
            log: None,
            log_bytes: None,
            suites: None,
            failed: None,
        }
    }

    const NOW: f64 = 1_794_400_000.0; // 2026-11-11, 12:26 UTC

    #[test]
    fn lines_go_whole_to_the_file_of_their_day_and_a_line_cut_short_stays_apart() {
        let dir = scratch("append");
        let first = ended("a", NOW, "t", false);
        append(&dir, &first, NOW).unwrap();
        let day = dir.join("2026-11-11.jsonl");
        // A writer cut short in its line.
        let mut cut = OpenOptions::new().append(true).open(&day).unwrap();
        cut.write_all(b"{\"schema\":1,\"ru").unwrap();
        let mut next = ended("b", NOW + DAY_S, "t", false);
        append(&dir, &next, NOW).unwrap();
        next.run = "c".into();
        next.ended = Some(NOW + 1.0);
        append(&dir, &next, NOW).unwrap();
        let text = fs::read_to_string(&day).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3, "{text}");
        assert_eq!(serde_json::from_str::<RunLine>(lines[0]).unwrap(), first);
        assert!(serde_json::from_str::<RunLine>(lines[1]).is_err());
        assert_eq!(serde_json::from_str::<RunLine>(lines[2]).unwrap().run, "c");
        assert!(text.ends_with('\n'));
        let tomorrow = fs::read_to_string(dir.join("2026-11-12.jsonl")).unwrap();
        assert_eq!(
            serde_json::from_str::<RunLine>(tomorrow.trim())
                .unwrap()
                .run,
            "b"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn day_files_older_than_ninety_days_go_and_nothing_else_does() {
        let dir = scratch("prune");
        let kept = [
            "2026-08-13.jsonl", // 90 days before 2026-11-11
            "2026-11-10.jsonl",
            "notes.jsonl",
            "2026-08-1.jsonl",
            "2026-08-11.json",
        ];
        let gone = ["2026-08-12.jsonl", "2025-12-31.jsonl", "1999-01-01.jsonl"];
        for name in kept.iter().chain(&gone) {
            fs::write(dir.join(name), "{}\n").unwrap();
        }
        fs::create_dir_all(dir.join("logs/2026-01-01")).unwrap();
        fs::create_dir_all(dir.join("2026-01-02.jsonl")).unwrap();
        std::os::unix::fs::symlink(dir.join("notes.jsonl"), dir.join("2026-01-03.jsonl")).unwrap();
        append(&dir, &ended("a", NOW, "t", false), NOW).unwrap();
        for name in kept {
            assert!(dir.join(name).exists(), "{name} went");
        }
        for name in gone {
            assert!(!dir.join(name).exists(), "{name} stayed");
        }
        for name in ["logs/2026-01-01", "2026-01-02.jsonl", "2026-01-03.jsonl"] {
            assert!(dir.join(name).symlink_metadata().is_ok(), "{name} went");
        }
        assert!(dir.join("2026-11-11.jsonl").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_lock_held_too_long_is_not_waited_for_and_nothing_is_pruned() {
        let dir = scratch("held");
        fs::write(dir.join("2000-01-01.jsonl"), "{}\n").unwrap();
        let holder = File::create(dir.join(LOCK)).unwrap();
        holder.lock().unwrap();
        let began = Instant::now();
        append(&dir, &ended("a", NOW, "t", false), NOW).unwrap();
        assert!(began.elapsed() >= LOCK_WAIT);
        assert!(
            dir.join("2000-01-01.jsonl").exists(),
            "pruned without the lock"
        );
        let text = fs::read_to_string(dir.join("2026-11-11.jsonl")).unwrap();
        assert_eq!(text.lines().count(), 1);
        holder.unlock().unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn many_writers_at_once_leave_every_line_whole() {
        let dir = scratch("many");
        let label = "x".repeat(4096);
        std::thread::scope(|scope| {
            for writer in 0..16 {
                let (dir, label) = (&dir, &label);
                scope.spawn(move || {
                    for n in 0..25 {
                        let mut line = ended(&format!("{writer}-{n}"), NOW, "t", false);
                        line.label.clone_from(label);
                        append(dir, &line, NOW).unwrap();
                    }
                });
            }
        });
        let text = fs::read_to_string(dir.join("2026-11-11.jsonl")).unwrap();
        let read: Vec<RunLine> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(read.len(), 400);
        let mut runs: Vec<&str> = read.iter().map(|line| line.run.as_str()).collect();
        runs.sort_unstable();
        runs.dedup();
        assert_eq!(runs.len(), 400);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_rerun_is_the_newest_clean_end_of_its_pipeline_and_step_on_its_tree_by_then() {
        let dir = scratch("rerun");
        let tree = "c".repeat(40);
        let mut lines = vec![
            ended("yesterday", NOW - DAY_S + 5.0, &tree, false),
            ended("older", NOW - 100.0, &tree, false),
            ended("newest", NOW - 50.0, &tree, false),
            ended("dirty", NOW - 40.0, &tree, true),
            ended("other-tree", NOW - 30.0, &"d".repeat(40), false),
            ended("after", NOW + 5.0, &tree, false),
        ];
        let mut started = ended("started", NOW - 20.0, &tree, false);
        started.end = None;
        let mut step = ended("a-step", NOW - 10.0, &tree, false);
        step.step = Some("tests".into());
        let mut lint = ended("lint", NOW - 5.0, &tree, false);
        lint.pipeline = "lint".into();
        lines.extend([started, step, lint]);
        for line in &lines {
            append(&dir, line, NOW).unwrap();
        }
        let found = |pipeline, step, before| rerun_of(&dir, pipeline, step, &tree, before);
        assert_eq!(found("verify", None, NOW).as_deref(), Some("newest"));
        assert_eq!(
            found("verify", Some("tests"), NOW).as_deref(),
            Some("a-step")
        );
        assert_eq!(found("lint", None, NOW).as_deref(), Some("lint"));
        assert_eq!(found("test", None, NOW), None);
        assert_eq!(
            found("verify", None, NOW - 60.0).as_deref(),
            Some("older"),
            "a run ended after it began is none it repeats"
        );
        assert_eq!(
            found("verify", None, NOW - 200.0).as_deref(),
            Some("yesterday"),
            "the day before is read too"
        );
        assert_eq!(
            found("verify", None, NOW + 2.0 * DAY_S),
            None,
            "two days on"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn runs_are_kept_in_the_common_dir_or_the_folder_named_and_a_tests_board_keeps_none() {
        let dir = scratch("folder");
        fs::create_dir_all(dir.join(".git")).unwrap();
        let common = dir.join(".git").canonicalize().unwrap();
        let env = |vars: &[(&str, &str)]| Env {
            vars: vars
                .iter()
                .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                .collect(),
            cwd: dir.clone(),
        };
        assert_eq!(folder(&env(&[])), Some(common.join("symphony/runs")));
        assert_eq!(folder(&env(&[])), Some(in_common(&common)));
        assert_eq!(
            folder(&env(&[("TASKS_DATABASE", crate::DATABASE)])),
            Some(common.join("symphony/runs"))
        );
        assert_eq!(
            folder(&env(&[("TASKS_DATABASE", "scratch")])),
            Some(common.join("symphony/runs.scratch"))
        );
        assert_eq!(folder(&env(&[(recorded::FOLDER_VAR, "/w")])), None);
        assert_eq!(folder(&env(&[("TASKS_SERVER", "127.0.0.1:1")])), None);
        assert_eq!(
            folder(&env(&[("TASKS_SERVER", "127.0.0.1:1"), (FOLDER_VAR, "/r")])),
            Some(PathBuf::from("/r"))
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_day_files_name_is_a_date() {
        assert!(day_file("2026-11-11.jsonl"));
        for name in [
            "2026-11-1.jsonl",
            "2026_11_10.jsonl",
            "2026-11-10.json",
            "lock",
            "x.jsonl",
        ] {
            assert!(!day_file(name), "{name}");
        }
    }
}
