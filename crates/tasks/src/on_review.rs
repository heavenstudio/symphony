//! The command a task moving to review starts. `[board] on_review` in the project's `symphony.toml`
//! names it (the default is none), and the board runs it detached from the main checkout with the
//! task's slug, its branch and that branch's tip appended, so the `move` that sent the task to
//! review never waits on it. The branch is the slug, or what follows a `review/` (`review-N/`)
//! prefix; a slug naming no branch starts nothing, and the log says so. Each start appends its
//! lines to `symphony/tasks/on-review.log` in the git common dir. A board `TASKS_SERVER` names (a
//! test's) starts none unless `TASKS_ON_REVIEW=on`; `TASKS_ON_REVIEW=off` starts none anywhere.
//!
//! The `tasks` that moved the task records it (`tasks run`, a test's `TASKS_BIN` on a test's board):
//! a run of the pipeline `review-callback` started by `on_review`, for the task, on its branch's tip
//! (docs/RUNS.md). With no recorder it runs unrecorded, started by `on_review` still.

use crate::cli::Env;
use crate::cli::machine_work::recorded::{PARENT_VAR, PIPELINE_VAR, TRIGGER_VAR};
use crate::git::common_dir;
use crate::sweep::git::rev;
use crate::sweep::trigger::{folder, main_checkout, now, test_build};
use crate::sys::{self, Stdio};
use serde::Deserialize;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The project file naming the command, read in the main checkout.
pub const MANIFEST: &str = "symphony.toml";
/// What each start said, under a line saying when and for which task.
pub const LOG: &str = "on-review.log";

/// Only a complete top-level feature enters the delivery pipeline.
pub fn delivery_task(parent: Option<&str>, instruction: bool) -> bool {
    parent.is_none() && !instruction
}

/// Whether a task moving to review in `env` may start the command: `TASKS_ON_REVIEW=on` or `off`
/// says; else only on the repository's own board, which no `TASKS_SERVER` names.
pub fn enabled(env: &Env) -> bool {
    env.switch("TASKS_ON_REVIEW")
}

/// The branch a task's `slug` names: the slug itself, or the branch a review task
/// (`review/BRANCH`, `review-N/BRANCH`) was filed for.
pub fn branch_of(slug: &str) -> &str {
    let Some(rest) = slug.strip_prefix("review") else {
        return slug;
    };
    let rest = match rest.strip_prefix('-') {
        Some(numbered) => numbered.trim_start_matches(|c: char| c.is_ascii_digit()),
        None => rest,
    };
    match rest.strip_prefix('/') {
        Some(branch) if !branch.is_empty() => branch,
        _ => slug,
    }
}

#[derive(Default, Deserialize)]
struct Manifest {
    #[serde(default)]
    board: Board,
}

#[derive(Default, Deserialize)]
struct Board {
    #[serde(default)]
    on_review: Vec<String>,
}

/// The command `[board] on_review` names in the manifest at `root`: none when the file, the table
/// or the key is missing or the list is empty. Only that key is read; the rest of the file is the
/// Symphony's.
pub fn command(root: &Path) -> Result<Option<Vec<String>>, String> {
    let path = root.join(MANIFEST);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    let manifest: Manifest = toml::from_str(&text)
        .map_err(|error| format!("{}: [board] on_review: {error}", path.display()))?;
    let command = manifest.board.on_review;
    if command.is_empty() || command[0].is_empty() {
        return Ok(None);
    }
    Ok(Some(command))
}

/// What a task moving to review starts, or why nothing starts.
#[derive(Debug, PartialEq, Eq)]
pub enum Started {
    /// The command, with the slug, branch and tip appended, from the main checkout.
    Command(Vec<String>),
    /// Starting is off in this environment, or the manifest names no command.
    None,
    /// The command is named but cannot start: no main checkout, no branch, or the spawn failed.
    Refused(String),
}

/// What the review command's run is to its recorder: its pipeline, and what started it.
pub const PIPELINE: &str = "review-callback";
/// What started it, in its recorder's [`TRIGGER_VAR`].
pub const TRIGGER: &str = "on_review";

/// A task `slug` has moved to review in `env` from `cwd`: start the manifest's command, detached,
/// if `env` lets the move start one and the slug names a branch, recorded by `recorder` (the
/// `tasks` that moved it), else a test's build. Nothing waits on it.
pub fn trigger(env: &Env, cwd: &Path, slug: &str, recorder: Option<&Path>) -> Started {
    if !enabled(env) {
        return Started::None;
    }
    let Ok(common) = common_dir(cwd) else {
        return Started::None;
    };
    let Some(root) = main_checkout(&common) else {
        return Started::None;
    };
    let command = match command(root) {
        Ok(Some(command)) => command,
        Ok(None) => return Started::None,
        Err(problem) => return refuse(&common, slug, &problem),
    };
    let branch = branch_of(slug);
    let Some(tip) = rev(root, &format!("refs/heads/{branch}")) else {
        return refuse(
            &common,
            slug,
            &format!("no branch {branch} in {}", root.display()),
        );
    };
    let mut args = command;
    args.extend([slug.to_string(), branch.to_string(), tip.clone()]);
    let recorder = recorder.map(Path::to_path_buf).or_else(|| test_build(env));
    let started = Start {
        root,
        slug,
        branch,
        tip: &tip,
        recorder: recorder.as_deref(),
    };
    match spawn(env, &common, &started, &args) {
        Ok(()) => Started::Command(args),
        Err(error) => refuse(&common, slug, &format!("cannot start {}: {error}", args[0])),
    }
}

/// What one start of the review command runs for.
struct Start<'a> {
    /// The main checkout it runs from.
    root: &'a Path,
    slug: &'a str,
    branch: &'a str,
    tip: &'a str,
    /// The `tasks` that records it.
    recorder: Option<&'a Path>,
}

/// Say in the log why nothing started for `slug`.
fn refuse(common: &Path, slug: &str, why: &str) -> Started {
    if let Ok(mut log) = open_log(&folder(common)) {
        let _ = writeln!(
            log,
            "[{}] {slug} is in review\nno command: {why}",
            sys::local_minute(now())
        );
    }
    Started::Refused(why.to_string())
}

/// The log `name` in `dir`, opened to append.
pub(crate) fn open_log_named(dir: &Path, name: &str) -> std::io::Result<File> {
    fs::create_dir_all(dir)?;
    OpenOptions::new()
        .append(true)
        .create(true)
        .open(dir.join(name))
}

/// The log, opened to append.
fn open_log(dir: &Path) -> std::io::Result<File> {
    open_log_named(dir, LOG)
}

/// The program `args[0]` names, as the command runs it from `root`: a path with a folder in it is
/// the main checkout's own; a bare name is found on `PATH`.
pub(crate) fn program(root: &Path, name: &str) -> PathBuf {
    let path = Path::new(name);
    if path.is_absolute() || path.components().count() < 2 {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

/// Whether `program` can start as the command runs it from `root` in `env`: a path names a regular
/// file it may execute, a bare name one along `env`'s `PATH` (`/usr/bin:/bin` without one). Else
/// the error a spawn of it answers: what the lookup said, or that it may not be run. A recorder
/// would start whatever its command is, so this is asked before it is handed one.
pub(crate) fn startable(root: &Path, program: &Path, env: &Env) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt as _;
    let runnable = |path: &Path| -> std::io::Result<()> {
        let found = fs::metadata(path)?;
        let name = std::ffi::CString::new(path.as_os_str().as_bytes())?;
        // SAFETY: a NUL-terminated path, alive through the call, which only reads it.
        if !found.is_file() || unsafe { libc::access(name.as_ptr(), libc::X_OK) } != 0 {
            return Err(std::io::Error::from_raw_os_error(libc::EACCES));
        }
        Ok(())
    };
    if program.components().count() > 1 {
        return runnable(program);
    }
    let mut refused = std::io::Error::from_raw_os_error(libc::ENOENT);
    for dir in std::env::split_paths(env.var("PATH").unwrap_or("/usr/bin:/bin")) {
        match runnable(&root.join(dir).join(program)) {
            Ok(()) => return Ok(()),
            Err(error) if error.raw_os_error() == Some(libc::EACCES) => refused = error,
            Err(_) => {}
        }
    }
    Err(refused)
}

/// `args` in a session of its own, from the main checkout, with `env`'s environment but the
/// mover's session ([`sys::SESSION_VARS`]: the review acts as a board session of its own), the
/// marks of how the move was asked for ([`sys::MARKS`]: the review is the project's command) and
/// any recorded command the mover ran in (it is a run of its own, started by [`TRIGGER`]),
/// recorded as `started` says, its output appended to the log; a thread reaps it when it ends. A
/// program that cannot start starts nothing, recorded or not.
fn spawn(env: &Env, common: &Path, started: &Start, args: &[String]) -> std::io::Result<()> {
    let Start {
        root,
        slug,
        branch,
        tip,
        recorder,
    } = *started;
    let mut log = open_log(&folder(common))?;
    writeln!(
        log,
        "[{}] {slug} is in review: {branch} at {tip}; {}",
        sys::local_minute(now()),
        args.join(" ")
    )?;
    let program = program(root, &args[0]);
    startable(root, &program, env)?;
    let mut command = match recorder {
        Some(recorder) => {
            let mut command = sys::command_with(recorder, &env.vars);
            command
                .arg(crate::cli::recorder::COMMAND)
                .args(["--phase", "test", "--label"])
                .arg(args.join(" "))
                .args(["--pipeline", PIPELINE, "--task", slug, "--commit", tip])
                .args(["--branch", branch, "--"])
                .arg(program);
            command
        }
        None => sys::command_with(program, &env.vars),
    };
    command
        .env(TRIGGER_VAR, TRIGGER)
        .env_remove(PARENT_VAR)
        .env_remove(PIPELINE_VAR)
        .args(&args[1..])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .new_session();
    let mut child = command.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_never_start_their_own_delivery_pipeline() {
        assert!(delivery_task(None, false));
        assert!(!delivery_task(Some("feature"), false));
        assert!(!delivery_task(Some("feature"), true));
        assert!(!delivery_task(None, true));
    }

    fn env(pairs: &[(&str, &str)]) -> Env {
        Env {
            vars: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            cwd: std::env::temp_dir(),
        }
    }

    #[test]
    fn only_the_repositorys_own_board_starts_the_command_unless_told() {
        assert!(enabled(&env(&[])));
        assert!(!enabled(&env(&[("TASKS_SERVER", "127.0.0.1:1")])));
        assert!(enabled(&env(&[
            ("TASKS_SERVER", "127.0.0.1:1"),
            ("TASKS_ON_REVIEW", "on")
        ])));
        assert!(!enabled(&env(&[("TASKS_ON_REVIEW", "off")])));
        assert!(!enabled(&env(&[
            ("TASKS_SERVER", "127.0.0.1:1"),
            ("TASKS_ON_REVIEW", "yes")
        ])));
    }

    #[test]
    fn a_review_tasks_slug_names_the_branch_it_was_filed_for() {
        for (slug, branch) in [
            ("sim/jump", "sim/jump"),
            ("review/sim/jump", "sim/jump"),
            ("review-2/sim/jump", "sim/jump"),
            ("review-10/docs/x", "docs/x"),
            ("reviewer/sim/jump", "reviewer/sim/jump"),
            ("review", "review"),
            ("review/", "review/"),
            ("review-/x", "x"),
            ("sim/review/jump", "sim/review/jump"),
        ] {
            assert_eq!(branch_of(slug), branch, "{slug}");
        }
    }

    #[test]
    fn the_manifest_names_the_command_or_none_and_a_broken_one_is_an_error() {
        let dir = std::env::temp_dir().join(format!("on-review-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(command(&dir), Ok(None), "no manifest");
        for (text, expected) in [
            ("[project]\nname = \"x\"\n", None),
            ("[board]\nexport = \"docs/tasks.md\"\n", None),
            ("[board]\non_review = []\n", None),
            ("[board]\non_review = [\"\"]\n", None),
            (
                "[project]\nname = \"x\"\n[board]\nexport = \"d.md\"\non_review = [\"tools/rs-bin.sh\", \"land\", \"review\"]\n",
                Some(vec![
                    "tools/rs-bin.sh".to_string(),
                    "land".to_string(),
                    "review".to_string(),
                ]),
            ),
        ] {
            fs::write(dir.join(MANIFEST), text).unwrap();
            assert_eq!(command(&dir), Ok(expected), "{text}");
        }
        fs::write(dir.join(MANIFEST), "[board]\non_review = \"land\"\n").unwrap();
        let problem = command(&dir).unwrap_err();
        assert!(
            problem.contains("symphony.toml: [board] on_review:"),
            "{problem}"
        );
        fs::write(dir.join(MANIFEST), "[board\n").unwrap();
        assert!(command(&dir).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_program_with_a_folder_in_its_name_is_the_main_checkouts() {
        let root = Path::new("/repo/main");
        assert_eq!(
            program(root, "tools/rs-bin.sh"),
            PathBuf::from("/repo/main/tools/rs-bin.sh")
        );
        assert_eq!(
            program(root, "./run.sh"),
            PathBuf::from("/repo/main/./run.sh")
        );
        assert_eq!(
            program(root, "/usr/bin/true"),
            PathBuf::from("/usr/bin/true")
        );
        assert_eq!(program(root, "sh"), PathBuf::from("sh"));
    }

    #[test]
    fn outside_a_repository_or_told_off_nothing_starts() {
        let dir = std::env::temp_dir().join(format!("on-review-none-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(trigger(&env(&[]), &dir, "sim/jump", None), Started::None);
        assert_eq!(
            trigger(&env(&[("TASKS_ON_REVIEW", "off")]), &dir, "sim/jump", None),
            Started::None
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
