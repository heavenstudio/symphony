//! The command a task moving to review starts. `[board] on_review` in the project's `symphony.toml`
//! names it (the default is none), and the board runs it detached from the main checkout with the
//! task's slug, its branch and that branch's tip appended, so the `move` that sent the task to
//! review never waits on it. The branch is the slug, or what follows a `review/` (`review-N/`)
//! prefix; a slug naming no branch starts nothing, and the log says so. Each start appends its lines
//! to `sf-tasks/on-review.log` in the git common dir. A board `TASKS_SERVER` names (a test's) starts
//! none unless `TASKS_ON_REVIEW=on`; `TASKS_ON_REVIEW=off` starts none anywhere.

use crate::cli::Env;
use crate::git::common_dir;
use crate::sweep::git::rev;
use crate::sweep::trigger::{folder, main_checkout, now};
use crate::sys::{self, Stdio};
use serde::Deserialize;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The project file naming the command, read in the main checkout.
pub const MANIFEST: &str = "symphony.toml";
/// What each start said, under a line saying when and for which task.
pub const LOG: &str = "on-review.log";

/// Whether a task moving to review in `env` may start the command: `TASKS_ON_REVIEW=on` or `off`
/// says; else only on the repository's own board, which no `TASKS_SERVER` names.
pub fn enabled(env: &Env) -> bool {
    match env.var("TASKS_ON_REVIEW") {
        Some("off") => false,
        Some("on") => true,
        _ => env.var("TASKS_SERVER").is_none(),
    }
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

/// A task `slug` has moved to review in `env` from `cwd`: start the manifest's command, detached,
/// if `env` lets the move start one and the slug names a branch. Nothing waits on it.
pub fn trigger(env: &Env, cwd: &Path, slug: &str) -> Started {
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
    match spawn(env, &common, root, slug, branch, &tip, &args) {
        Ok(()) => Started::Command(args),
        Err(error) => refuse(&common, slug, &format!("cannot start {}: {error}", args[0])),
    }
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

/// The log, opened to append.
fn open_log(dir: &Path) -> std::io::Result<File> {
    fs::create_dir_all(dir)?;
    OpenOptions::new()
        .append(true)
        .create(true)
        .open(dir.join(LOG))
}

/// The program `args[0]` names, as the command runs it from `root`: a path with a folder in it is
/// the main checkout's own; a bare name is found on `PATH`.
fn program(root: &Path, name: &str) -> PathBuf {
    let path = Path::new(name);
    if path.is_absolute() || path.components().count() < 2 {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

/// `args` in a session of its own, from the main checkout `root`, with `env`'s environment but the
/// mover's session ([`sys::SESSION_VARS`]: the review acts as a board session of its own), its
/// output appended to the log; a thread reaps it when it ends.
fn spawn(
    env: &Env,
    common: &Path,
    root: &Path,
    slug: &str,
    branch: &str,
    tip: &str,
    args: &[String],
) -> std::io::Result<()> {
    let mut log = open_log(&folder(common))?;
    writeln!(
        log,
        "[{}] {slug} is in review: {branch} at {tip}; {}",
        sys::local_minute(now()),
        args.join(" ")
    )?;
    let mut command = sys::command_with(program(root, &args[0]), &env.vars);
    command
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
                "[project]\nname = \"x\"\n[board]\nexport = \"d.md\"\non_review = [\"tools/rs-bin.sh\", \"sf-land\", \"review\"]\n",
                Some(vec![
                    "tools/rs-bin.sh".to_string(),
                    "sf-land".to_string(),
                    "review".to_string(),
                ]),
            ),
        ] {
            fs::write(dir.join(MANIFEST), text).unwrap();
            assert_eq!(command(&dir), Ok(expected), "{text}");
        }
        fs::write(dir.join(MANIFEST), "[board]\non_review = \"sf-land\"\n").unwrap();
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
        assert_eq!(trigger(&env(&[]), &dir, "sim/jump"), Started::None);
        assert_eq!(
            trigger(&env(&[("TASKS_ON_REVIEW", "off")]), &dir, "sim/jump"),
            Started::None
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
