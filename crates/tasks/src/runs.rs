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
/// The longest a recorder apart from its command's caller (on macOS) waits for the lock before it
/// appends a line without it, in one write still.
pub const LOCK_WAIT: Duration = Duration::from_secs(2);
/// The longest an append waits for the lock where the command's caller waits on it (the recorder
/// is the command's parent, or sweeps before it begins): a held lock never holds up the status.
pub const BRIEF_WAIT: Duration = Duration::from_millis(50);
/// The most of a day file the rerun reader reads: its last bytes, the newest lines.
const READ_BYTES: u64 = 16 * 1024 * 1024;
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
        cpu_partial: run.cpu_partial.filter(|_| finished && run.cpu_s.is_some()),
        rerun_of: run.rerun_of.clone(),
        log: None,
        log_bytes: None,
        suites: None,
        failed: None,
    })
}

/// The folder's lock, waited for at most `wait`; none when it stayed held that long.
fn locked(folder: &Path, wait: Duration) -> io::Result<Option<FileLock>> {
    let file = regular(
        &folder.join(LOCK),
        OpenOptions::new().read(true).write(true).create(true),
    )?;
    lock_within(file, wait)
}

/// `file` locked, tried again every 5 ms for at most `wait`; none when it stayed held that long.
pub(crate) fn lock_within(file: File, wait: Duration) -> io::Result<Option<FileLock>> {
    let deadline = Instant::now() + wait;
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
/// under the folder's lock, after a newline when a writer before was cut short, so each line stays
/// its own; then the day files more than [`KEPT_DAYS`] days before `now`'s go. A lock held past
/// `wait` is not waited for: the line is appended so all the same, and nothing pruned.
pub fn append(folder: &Path, line: &RunLine, now: f64, wait: Duration) -> io::Result<()> {
    fs::create_dir_all(folder)?;
    let mut text = serde_json::to_vec(line).map_err(io::Error::other)?;
    text.push(b'\n');
    let held = locked(folder, wait)?;
    let path = folder.join(format!("{}.jsonl", line.day()));
    let mut file = regular(
        &path,
        OpenOptions::new().read(true).append(true).create(true),
    )?;
    // Without the lock too: two writers that both find a line cut short leave an empty line between
    // theirs, which a reader passes over as it passes over any line that does not parse.
    if !ends_a_line(&file)? {
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

/// The lines of `day`'s file in `folder` that hold `needle`, read in order: of its last
/// [`READ_BYTES`], read only from a regular file (never through a link, never waiting on a FIFO). A
/// line that is not one (cut short, or begun before what is read) is passed over.
fn lines_holding(folder: &Path, day: &str, needle: &str) -> Vec<RunLine> {
    if needle.is_empty() {
        return Vec::new();
    }
    let Ok(file) = regular(
        &folder.join(format!("{day}.jsonl")),
        OpenOptions::new().read(true),
    ) else {
        return Vec::new();
    };
    let Ok(length) = file.metadata().map(|metadata| metadata.len()) else {
        return Vec::new();
    };
    let from = length.saturating_sub(READ_BYTES);
    let mut bytes = vec![0; usize::try_from(length - from).unwrap_or(0)];
    if file.read_exact_at(&mut bytes, from).is_err() {
        return Vec::new();
    }
    bytes
        .split(|byte| *byte == b'\n')
        .skip(usize::from(from > 0))
        .filter(|line| {
            line.windows(needle.len())
                .any(|window| window == needle.as_bytes())
        })
        .filter_map(|line| serde_json::from_slice(line).ok())
        .collect()
}

/// What a run is the same as an earlier one by: its pipeline, its step and its label (the recipe or
/// the command line), on its tree.
#[derive(Clone, Copy, Debug)]
pub struct Same<'a> {
    pub pipeline: &'a str,
    pub step: Option<&'a str>,
    pub label: &'a str,
    pub tree: &'a str,
}

impl Same<'_> {
    /// Whether `pipeline`, `step` and `label` on `tree` say the same.
    pub fn of(&self, pipeline: &str, step: Option<&str>, label: &str, tree: Option<&str>) -> bool {
        pipeline == self.pipeline
            && step == self.step
            && label == self.label
            && tree == Some(self.tree)
    }
}

/// The run `same` that a clean run asked for at `before` repeats: the newest one that ended by
/// then, also clean, in the day files of that day and the day before.
pub fn rerun_of(folder: &Path, same: Same, before: f64) -> Option<String> {
    let mut found: Option<RunLine> = None;
    for day in [run_day(before - DAY_S), run_day(before)] {
        for line in lines_holding(folder, &day, same.tree) {
            let ended = line.ended.filter(|ended| *ended <= before);
            if line.end.is_some()
                && ended.is_some()
                && line.dirty == Some(false)
                && same.of(
                    &line.pipeline,
                    line.step.as_deref(),
                    &line.label,
                    line.tree.as_deref(),
                )
                && found.as_ref().is_none_or(|newest| newest.ended <= ended)
            {
                found = Some(line);
            }
        }
    }
    found.map(|line| line.run)
}

/// The tree of `commit` and whether the checkout at `cwd` differs from it (a tracked change, or a
/// file git does not ignore), as git says within `within`; none for what it cannot say by then.
/// `checked` names a commit the run checks that is not the checkout's: its tree, and clean. A git
/// still running then is killed and reaped.
pub fn tree_and_dirty(
    cwd: &Path,
    commit: &str,
    checked: bool,
    within: Duration,
) -> (Option<String>, Option<bool>) {
    let deadline = Instant::now() + within;
    let git = |args: &[&str]| {
        let said = said_by(sys::command("git").arg("-C").arg(cwd).args(args), deadline)?;
        Some(String::from_utf8_lossy(&said).into_owned())
    };
    let tree = git(&[
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("{commit}^{{tree}}"),
    ])
    .map(|tree| tree.trim().to_string())
    .filter(|tree| matches!(tree.len(), 40 | 64) && tree.bytes().all(|b| b.is_ascii_hexdigit()));
    if checked {
        let dirty = tree.as_ref().map(|_| false);
        return (tree, dirty);
    }
    let dirty = git(&[
        "--no-optional-locks",
        "status",
        "--porcelain",
        "--untracked-files=normal",
    ])
    .map(|said| !said.is_empty());
    (tree, dirty)
}

/// What `command` printed, when it ended with success by `deadline`; none otherwise, and a command
/// still running then is killed and reaped. Its output is read on a thread of its own, so a command
/// that prints much never waits on its pipe.
fn said_by(command: &mut sys::Command, deadline: Instant) -> Option<Vec<u8>> {
    use std::io::Read as _;
    if Instant::now() >= deadline {
        return None;
    }
    let mut child = command
        .stdin(sys::Stdio::Null)
        .stdout(sys::Stdio::Piped)
        .stderr(sys::Stdio::Null)
        .spawn()
        .ok()?;
    let (tell, said) = std::sync::mpsc::channel();
    if let Some(mut out) = child.stdout.take() {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = tell.send(out.read_to_end(&mut bytes).map(|_| bytes));
        });
    }
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(2)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let bytes = said
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .ok()?
        .ok()?;
    status?.success().then_some(bytes)
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
            cpu_partial: None,
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
        append(&dir, &first, NOW, LOCK_WAIT).unwrap();
        let day = dir.join("2026-11-11.jsonl");
        // A writer cut short in its line.
        let mut cut = OpenOptions::new().append(true).open(&day).unwrap();
        cut.write_all(b"{\"schema\":1,\"ru").unwrap();
        let mut next = ended("b", NOW + DAY_S, "t", false);
        append(&dir, &next, NOW, LOCK_WAIT).unwrap();
        next.run = "c".into();
        next.ended = Some(NOW + 1.0);
        append(&dir, &next, NOW, LOCK_WAIT).unwrap();
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
        append(&dir, &ended("a", NOW, "t", false), NOW, LOCK_WAIT).unwrap();
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
        append(&dir, &ended("a", NOW, "t", false), NOW, LOCK_WAIT).unwrap();
        assert!(began.elapsed() >= LOCK_WAIT);
        // Where the command's caller waits, hardly at all; a line cut short before stays apart.
        use std::io::Write as _;
        let day = dir.join("2026-11-11.jsonl");
        OpenOptions::new()
            .append(true)
            .open(&day)
            .unwrap()
            .write_all(b"{\"schema\":1,\"ru")
            .unwrap();
        let began = Instant::now();
        append(&dir, &ended("b", NOW, "t", false), NOW, BRIEF_WAIT).unwrap();
        let waited = began.elapsed();
        assert!(
            BRIEF_WAIT <= waited && waited < Duration::from_millis(500),
            "{waited:?}"
        );
        assert!(
            dir.join("2000-01-01.jsonl").exists(),
            "pruned without the lock"
        );
        let text = fs::read_to_string(&day).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3, "{text}");
        assert_eq!(lines[1], "{\"schema\":1,\"ru");
        let [first, _, last] = [lines[0], lines[1], lines[2]]
            .map(|line| serde_json::from_str::<RunLine>(line).map(|line| line.run).ok());
        assert_eq!((first.as_deref(), last.as_deref()), (Some("a"), Some("b")));
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
                        append(dir, &line, NOW, LOCK_WAIT).unwrap();
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
    fn a_rerun_is_the_newest_clean_end_of_its_pipeline_step_and_label_on_its_tree_by_then() {
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
        // Another command of the same pipeline: `just test-rs -p A` is not `just test-rs -p B`.
        let mut other = ended("other-label", NOW - 2.0, &tree, false);
        other.label = "just verify --quick".into();
        lines.extend([started, step, lint, other]);
        for line in &lines {
            append(&dir, line, NOW, LOCK_WAIT).unwrap();
        }
        let found = |pipeline, step, before| {
            let same = Same {
                pipeline,
                step,
                label: "just verify",
                tree: &tree,
            };
            rerun_of(&dir, same, before)
        };
        assert_eq!(found("verify", None, NOW).as_deref(), Some("newest"));
        assert_eq!(
            found("verify", Some("tests"), NOW).as_deref(),
            Some("a-step")
        );
        assert_eq!(found("lint", None, NOW).as_deref(), Some("lint"));
        assert_eq!(found("test", None, NOW), None);
        let labelled = |label| {
            let same = Same {
                pipeline: "verify",
                step: None,
                label,
                tree: &tree,
            };
            rerun_of(&dir, same, NOW)
        };
        assert_eq!(
            labelled("just verify --quick").as_deref(),
            Some("other-label")
        );
        assert_eq!(labelled("just verify -p x"), None);
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
    fn the_rerun_reader_reads_a_regular_files_newest_bytes_and_never_a_fifo_or_a_link() {
        let dir = scratch("rerun-read");
        let tree = "c".repeat(40);
        let same = Same {
            pipeline: "verify",
            step: None,
            label: "just verify",
            tree: &tree,
        };
        let day = dir.join("2026-11-11.jsonl");
        // A FIFO named as the day's file: read at once as nothing, with no writer to wait for.
        let fifo = std::ffi::CString::new(day.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: a NUL-terminated path, alive through the call.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let began = Instant::now();
        assert_eq!(rerun_of(&dir, same, NOW), None);
        assert!(began.elapsed() < Duration::from_secs(1));
        fs::remove_file(&day).unwrap();
        // A link, to an endless device or to a day file, is not read.
        std::os::unix::fs::symlink("/dev/zero", &day).unwrap();
        assert_eq!(rerun_of(&dir, same, NOW), None);
        fs::remove_file(&day).unwrap();
        let elsewhere = scratch("rerun-read-elsewhere");
        append(
            &elsewhere,
            &ended("linked", NOW - 5.0, &tree, false),
            NOW,
            LOCK_WAIT,
        )
        .unwrap();
        std::os::unix::fs::symlink(elsewhere.join("2026-11-11.jsonl"), &day).unwrap();
        assert_eq!(rerun_of(&dir, same, NOW), None);
        fs::remove_file(&day).unwrap();
        // Of a file longer than it reads, its newest lines: a line begun before them is passed over.
        append(
            &dir,
            &ended("first", NOW - 9.0, &tree, false),
            NOW,
            LOCK_WAIT,
        )
        .unwrap();
        let mut file = OpenOptions::new().append(true).open(&day).unwrap();
        let filler = format!("{}\n", "x".repeat(1023));
        for _ in 0..usize::try_from(READ_BYTES / 1024).unwrap() {
            file.write_all(filler.as_bytes()).unwrap();
        }
        assert_eq!(rerun_of(&dir, same, NOW), None, "the oldest line was read");
        append(
            &dir,
            &ended("last", NOW - 1.0, &tree, false),
            NOW,
            LOCK_WAIT,
        )
        .unwrap();
        assert_eq!(rerun_of(&dir, same, NOW).as_deref(), Some("last"));
        fs::remove_dir_all(&dir).unwrap();
        fs::remove_dir_all(&elsewhere).unwrap();
    }

    /// A step's record, running.
    fn running_step() -> Record {
        Record {
            version: 1,
            operation_id: "a".repeat(32),
            parent_id: Some("b".repeat(32)),
            phase: recorded::Phase::Build,
            label: "just lint-rs".into(),
            state: Step::Running,
            handle: Some("ember-osprey".into()),
            task: Some("tools/run-record".into()),
            checkout: "/repo".into(),
            branch: Some("main".into()),
            commit: Some("c".repeat(40)),
            machine_id: Some("d".repeat(32)),
            host: "host".into(),
            pid: 1,
            child_pid: Some(2),
            queued_at: NOW - 3.0,
            started_at: Some(NOW - 2.0),
            ended_at: None,
            beat_at: NOW - 1.0,
            exit_code: None,
            signal: None,
            reason: String::new(),
            run: Some(recorded::Run {
                pipeline: "verify".into(),
                step: Some("lint".into()),
                trigger: "ember-osprey".into(),
                project: Some("symphony".into()),
                tree: Some("e".repeat(40)),
                dirty: Some(false),
                rerun_of: Some("f".repeat(32)),
                cpu_s: Some(1.5),
                cpu_partial: Some(true),
                runs: Some("/repo/.git/symphony/runs".into()),
            }),
        }
    }

    #[test]
    fn a_record_says_its_start_line_while_it_runs_and_its_end_line_once_finished() {
        let mut record = running_step();
        let start = line(&record).unwrap();
        assert!(start.is_start());
        assert_eq!(
            (start.ended, start.end, start.cpu_s, start.cpu_partial),
            (None, None, None, None),
            "a start line says no end, nor its CPU yet"
        );
        assert_eq!(start.run, record.operation_id);
        assert_eq!(start.parent, record.parent_id);
        assert_eq!(
            (start.pipeline.as_str(), start.step.as_deref()),
            ("verify", Some("lint"))
        );
        assert_eq!(
            (start.machine.as_deref(), start.project.as_deref()),
            (Some("d".repeat(32).as_str()), Some("symphony"))
        );
        assert_eq!(start.rerun_of.as_deref(), Some("f".repeat(32).as_str()));
        record.state = Step::Finished;
        record.ended_at = Some(NOW);
        record.exit_code = Some(101);
        let end = line(&record).unwrap();
        assert_eq!(
            (end.ended, end.end, end.exit, end.cpu_s, end.cpu_partial),
            (Some(NOW), Some(End::Red), Some(101), Some(1.5), Some(true))
        );
        assert_eq!(end.day(), "2026-11-11");
        // No CPU read, no word on whether it is whole.
        record.run.as_mut().unwrap().cpu_s = None;
        assert_eq!(line(&record).unwrap().cpu_partial, None);
        record.run.as_mut().unwrap().cpu_s = Some(1.5);
        // A run has no parent, though it ran inside a recorded command.
        record.run.as_mut().unwrap().step = None;
        assert_eq!(line(&record).unwrap().parent, None);
        // One that never started ended incomplete, whatever its exit.
        record.started_at = None;
        assert_eq!(line(&record).unwrap().end, Some(End::Incomplete));
        record.run = None;
        assert_eq!(line(&record), None, "an operation of the work folder alone");
    }

    #[test]
    fn a_tree_is_its_commits_and_a_checkout_is_dirty_with_a_change_or_a_file_git_does_not_ignore() {
        let dir = scratch("tree");
        let git = |args: &[&str]| {
            let done = sys::output(
                sys::command("git")
                    .args(["-c", "user.email=t@t", "-c", "user.name=t", "-C"])
                    .arg(&dir)
                    .args(args),
            )
            .unwrap();
            assert!(done.status.success(), "{args:?}: {done:?}");
            String::from_utf8(done.stdout).unwrap().trim().to_string()
        };
        git(&["init", "-q", "-b", "main"]);
        fs::write(dir.join(".gitignore"), "ignored\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "base"]);
        let commit = git(&["rev-parse", "HEAD"]);
        let tree = git(&["rev-parse", "HEAD^{tree}"]);
        let said =
            |commit: &str, checked| tree_and_dirty(&dir, commit, checked, Duration::from_secs(30));
        assert_eq!(said(&commit, false), (Some(tree.clone()), Some(false)));
        fs::write(dir.join("ignored"), "x").unwrap();
        assert_eq!(said(&commit, false).1, Some(false));
        fs::write(dir.join("untracked"), "x").unwrap();
        assert_eq!(said(&commit, false), (Some(tree.clone()), Some(true)));
        // A commit named, not the checkout's: clean, whatever the checkout holds.
        assert_eq!(said(&commit, true), (Some(tree), Some(false)));
        assert_eq!(said(&"0".repeat(40), true), (None, None));
        let elsewhere = scratch("tree-none");
        assert_eq!(
            tree_and_dirty(&elsewhere, &commit, false, Duration::from_secs(30)),
            (None, None)
        );
        // What git has not said in the time given is not known.
        assert_eq!(
            tree_and_dirty(&dir, &commit, false, Duration::ZERO),
            (None, None)
        );
        fs::remove_dir_all(&dir).unwrap();
        fs::remove_dir_all(&elsewhere).unwrap();
    }

    #[test]
    fn a_command_asked_by_a_deadline_says_its_output_or_is_killed_and_reaped() {
        let dir = scratch("said-by");
        let sh = |script: &str, within: Duration| {
            said_by(
                sys::command("/bin/sh").args(["-c", script]),
                Instant::now() + within,
            )
        };
        let long = Duration::from_secs(30);
        assert_eq!(sh("printf 'a tree'", long), Some(b"a tree".to_vec()));
        assert_eq!(sh("printf said; exit 3", long), None);
        // More than a pipe holds is read as it comes.
        let much = sh("i=0; while [ $i -lt 2000 ]; do printf '%0100d\\n' $i; i=$((i+1)); done", long);
        assert_eq!(much.map(|said| said.len()), Some(2000 * 101));
        // One still running at the deadline is killed and reaped: gone, no zombie left.
        let pid = dir.join("pid");
        let began = Instant::now();
        let script = format!("echo $$ > '{}'; exec sleep 30", pid.display());
        assert_eq!(sh(&script, Duration::from_millis(300)), None);
        assert!(began.elapsed() < Duration::from_secs(10), "{:?}", began.elapsed());
        let pid: libc::pid_t = fs::read_to_string(&pid).unwrap().trim().parse().unwrap();
        // SAFETY: signal 0 only asks whether the process is there.
        let there = unsafe { libc::kill(pid, 0) };
        assert_eq!(
            (there, io::Error::last_os_error().raw_os_error()),
            (-1, Some(libc::ESRCH))
        );
        // Asked when the time is up, it is not started.
        assert_eq!(sh("printf late", Duration::ZERO), None);
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
