//! When the sweep runs by itself: work finishing starts it, detached and quiet, so the command that
//! finished the work never waits on it. A task going to done or cancelled (`done`, `move`, `verdict`),
//! a session leaving (`leave`) and a reap (`reap`) each start one; `inbox`, which every session runs
//! between steps, starts one at most once an hour, by a stamp file. Each runs from the main checkout
//! as `tasks sweep --background=WHY`, its lines appended to `symphony/tasks/sweep.log` in the git
//! common dir, one sweep at a time (`symphony/tasks/sweep.lock`). The build that runs it is the main
//! checkout's ([`crate::builds::of_main`]: its `.target/bin/tasks` link, else the machine's release's,
//! else `tasks` on `PATH`),
//! never the caller's: a worktree holding sweep code that has not landed would run it for real.
//! When the main checkout has no build, the log says so and nothing sweeps; a test's `TASKS_BIN`
//! stands in for it on a test's board. A board `TASKS_SERVER` names (a test's) starts none, unless
//! `TASKS_SWEEP=on`: such a board does not know who works in this repository's worktrees.
//! `TASKS_SWEEP=off` starts none anywhere.

use crate::cli::Env;
use crate::git::common_dir;
use crate::server::FOLDER;
use crate::sys::{self, Stdio};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// When the last sweep started, in seconds since the epoch, as text.
pub const STAMP: &str = "sweep.stamp";
/// Held by the sweep that runs; another caller records its request and never waits.
pub const LOCK: &str = "sweep.lock";
/// What the triggered sweeps said, each under a line saying when and why.
pub const LOG: &str = "sweep.log";
/// How often `inbox` may start one.
pub const HOURLY_SECONDS: f64 = 3600.0;
/// The log's size past which it is moved to `sweep.log.1`, the one before it dropped: the bound of
/// every log [`append_log`] opens.
pub const LOG_LIMIT: u64 = 1 << 20;

/// The machine's time, in seconds since the epoch.
pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// Whether work finishing in `env` may start a sweep: `TASKS_SWEEP=on` or `off` says; else only on
/// the repository's own board, which no `TASKS_SERVER` names.
pub fn enabled(env: &Env) -> bool {
    env.switch("TASKS_SWEEP")
}

/// The sweep's folder in the git common dir `common`, beside the board's server.
pub fn folder(common: &Path) -> PathBuf {
    common.join(FOLDER)
}

/// Start a sweep for `why`, detached, if `env` lets work finishing start one: `task`, a task just
/// done or cancelled, whose closed sessions' worktrees go without the window. Whether one started;
/// nothing it meets is said, and nothing waits on it.
pub fn trigger(env: &Env, cwd: &Path, why: &str, task: Option<&str>) -> bool {
    if !enabled(env) {
        return false;
    }
    let Ok(common) = common_dir(cwd) else {
        return false;
    };
    spawn(env, &common, why, task).is_ok()
}

/// Start the hourly sweep if `env` lets work start one and none started in the last hour: the first
/// caller to find the stamp old claims it, so callers at once start one.
pub fn hourly(env: &Env, cwd: &Path) -> bool {
    if !enabled(env) {
        return false;
    }
    let Ok(common) = common_dir(cwd) else {
        return false;
    };
    let at = now();
    let stamp = folder(&common).join(STAMP);
    if fresh(&fs::read_to_string(&stamp).unwrap_or_default(), at) {
        return false; // the common case, one read
    }
    claim(&stamp, at).unwrap_or(false) && spawn(env, &common, "hourly", None).is_ok()
}

/// Whether a stamp reading `text` was written within the hour before `now`.
fn fresh(text: &str, now: f64) -> bool {
    text.trim()
        .parse::<f64>()
        .is_ok_and(|then| (now - then).abs() < HOURLY_SECONDS)
}

/// The stamp at `path`, locked, not waited for: None while another caller holds it. Without the
/// explicit unlock a [`sys::FileLock`] gives, the next claim under load found the stamp locked by
/// nobody's claim (the test of two claims below, red at a load of 260 on 16 cores).
fn lock_stamp(path: &Path) -> std::io::Result<Option<sys::FileLock>> {
    let file = open_stamp(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(sys::FileLock(file))),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(problem)) => Err(problem),
    }
}

/// Write `now` into the stamp at `path` if it is not fresh, under its lock, not waited for: whether
/// this caller did.
fn claim(path: &Path, now: f64) -> std::io::Result<bool> {
    let Some(mut held) = lock_stamp(path)? else {
        return Ok(false);
    };
    let mut text = String::new();
    held.0.read_to_string(&mut text)?;
    if fresh(&text, now) {
        return Ok(false);
    }
    write_stamp(&mut held.0, now)?;
    Ok(true)
}

fn open_stamp(path: &Path) -> std::io::Result<File> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

fn write_stamp(file: &mut File, now: f64) -> std::io::Result<()> {
    file.set_len(0)?;
    file.rewind()?;
    file.write_all(now.to_string().as_bytes())
}

/// Try the sweep's lock without waiting. Only a holder writes the start stamp.
pub fn hold(common: &Path) -> Result<Option<sys::FileLock>, String> {
    let dir = folder(common);
    let path = dir.join(LOCK);
    let lock = fs::create_dir_all(&dir)
        .and_then(|()| {
            OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)
        })
        .map_err(|e| format!("cannot take the sweep's lock {}: {e}", path.display()))?;
    match lock.try_lock() {
        Err(TryLockError::WouldBlock) => return Ok(None),
        Err(TryLockError::Error(e)) => {
            return Err(format!(
                "cannot take the sweep's lock {}: {e}",
                path.display()
            ));
        }
        Ok(()) => {}
    }
    let lock = sys::FileLock(lock);
    let stamp = dir.join(STAMP);
    let _ = lock_stamp(&stamp).and_then(|held| {
        if let Some(mut held) = held {
            write_stamp(&mut held.0, now())?;
        }
        Ok(())
    });
    Ok(Some(lock))
}

/// The sweep's log, opened to append, moved aside first when it has grown past `LOG_LIMIT`.
fn open_log(dir: &Path) -> std::io::Result<File> {
    append_log(dir, LOG)
}

/// The log `name` in `dir`, opened to append, moved aside to `name.1` first when it has grown past
/// `LOG_LIMIT`.
pub(crate) fn append_log(dir: &Path, name: &str) -> std::io::Result<File> {
    let log = dir.join(name);
    if fs::metadata(&log).is_ok_and(|m| m.len() > LOG_LIMIT) {
        let _ = fs::rename(&log, dir.join(format!("{name}.1")));
    }
    OpenOptions::new().append(true).create(true).open(&log)
}

/// The build a test names in `TASKS_BIN` to sweep in its place (and to record a review command,
/// [`crate::on_review`]): only on a test's board, which a `TASKS_SERVER` names, so a `TASKS_BIN` a
/// launcher left in a session's environment is never taken for it.
pub(crate) fn test_build(env: &Env) -> Option<PathBuf> {
    env.var("TASKS_SERVER")?;
    env.var("TASKS_BIN").map(PathBuf::from)
}

/// The build that sweeps for the main checkout `root`: a test's `TASKS_BIN` on a test's board, else
/// the main checkout's own ([`crate::builds::of_main`]: its `.target/bin/tasks` link, else the machine's
/// release's, else `tasks` on the caller's `PATH`). Never the caller's: a worktree's `tasks` carries that worktree's sweep,
/// which has not landed.
fn sweeper(env: &Env, root: &Path) -> Option<PathBuf> {
    test_build(env).or_else(|| crate::builds::of_main(root, "tasks", &beyond(env)))
}

/// What a search for a build reads past the main checkout's link, in `env`: its machine folder and
/// its `PATH` ([`crate::builds::Beyond`]).
pub(crate) fn beyond(env: &Env) -> crate::builds::Beyond {
    crate::builds::Beyond::of(|name| env.var(name).map(std::ffi::OsString::from))
}

/// The main checkout of the repository whose git common dir is `common`: the folder holding it, when
/// it is that checkout's `.git`. A bare repository, or a common dir kept elsewhere, has none.
pub(crate) fn main_checkout(common: &Path) -> Option<&Path> {
    if common.file_name().is_some_and(|name| name == ".git") {
        common.parent().filter(|root| !root.as_os_str().is_empty())
    } else {
        None
    }
}

/// `tasks sweep --background=WHY [--task=TASK]` in a session of its own, from the main checkout, by
/// the main checkout's build of `tasks` ([`sweeper`]), with `env`'s environment but its session
/// ([`sys::SESSION_VARS`]: the sweep acts as no one, in no terminal), its output appended to the log;
/// a thread reaps it when it ends. With no main checkout, or no build of it, the log says so and
/// nothing starts.
fn spawn(env: &Env, common: &Path, why: &str, task: Option<&str>) -> std::io::Result<()> {
    let dir = folder(common);
    fs::create_dir_all(&dir)?;
    let mut log = open_log(&dir)?;
    let unswept = |log: &mut File, why_not: String| -> std::io::Result<()> {
        writeln!(
            log,
            "[{}] {why}\nno sweep: {why_not}",
            sys::local_minute(now())
        )?;
        Err(std::io::Error::other(why_not))
    };
    // The main checkout, which is never swept, so the sweep's own working directory keeps nothing.
    let Some(root) = main_checkout(common) else {
        return unswept(
            &mut log,
            format!(
                "the git common dir {} is no main checkout's, so no build of its sources is known",
                common.display()
            ),
        );
    };
    let Some(binary) = sweeper(env, root) else {
        return unswept(&mut log, crate::builds::missing(root, "tasks"));
    };
    let mut command = sys::command_with(binary, &env.vars);
    command.arg("sweep").arg(format!("--background={why}"));
    if let Some(task) = task {
        command.arg(format!("--task={task}"));
    }
    command
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
    fn only_the_repositorys_own_board_starts_a_sweep_unless_told() {
        assert!(enabled(&env(&[])));
        assert!(!enabled(&env(&[("TASKS_SERVER", "127.0.0.1:1")])));
        assert!(enabled(&env(&[
            ("TASKS_SERVER", "127.0.0.1:1"),
            ("TASKS_SWEEP", "on")
        ])));
        assert!(!enabled(&env(&[("TASKS_SWEEP", "off")])));
        assert!(!enabled(&env(&[
            ("TASKS_SERVER", "127.0.0.1:1"),
            ("TASKS_SWEEP", "yes")
        ])));
    }

    #[test]
    fn a_tests_build_stands_in_only_on_a_tests_board() {
        assert_eq!(test_build(&env(&[])), None);
        assert_eq!(test_build(&env(&[("TASKS_BIN", "/b/tasks")])), None); // a launcher's leftover
        assert_eq!(test_build(&env(&[("TASKS_SERVER", "127.0.0.1:1")])), None);
        assert_eq!(
            test_build(&env(&[
                ("TASKS_SERVER", "127.0.0.1:1"),
                ("TASKS_BIN", "/b/tasks")
            ])),
            Some(PathBuf::from("/b/tasks"))
        );
    }

    #[test]
    fn the_sweeper_is_a_tests_build_else_the_main_checkouts_link_else_the_search_paths() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("sweeper");
        let program = |rel: &str| {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "#!/bin/sh\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        let path = root.join("path").display().to_string();
        assert_eq!(sweeper(&env(&[("PATH", &path)]), &root), None);
        let installed = program("path/tasks");
        assert_eq!(sweeper(&env(&[("PATH", &path)]), &root), Some(installed));
        let linked = program(".target/bin/tasks");
        assert_eq!(sweeper(&env(&[("PATH", &path)]), &root), Some(linked));
        assert_eq!(
            sweeper(
                &env(&[
                    ("TASKS_SERVER", "127.0.0.1:1"),
                    ("TASKS_BIN", "/b/tasks"),
                    ("PATH", &path)
                ]),
                &root
            ),
            Some(PathBuf::from("/b/tasks"))
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_main_checkout_is_the_folder_holding_the_common_dir_named_dot_git() {
        assert_eq!(
            main_checkout(Path::new("/r/star/.git")),
            Some(Path::new("/r/star"))
        );
        for none in ["/r/star.git", "/r/star/.git/worktrees/a", "/", ".git"] {
            assert_eq!(main_checkout(Path::new(none)), None, "{none}");
        }
    }

    #[test]
    fn a_stamp_is_fresh_for_an_hour_and_one_that_cannot_be_read_is_old() {
        let now = 1_800_000_000.0;
        assert!(fresh("1800000000.0", now));
        assert!(fresh(" 1799996400.5\n", now));
        assert!(!fresh("1799996400.0", now));
        assert!(!fresh("1800003600.0", now)); // a clock that went back: the stamp is no guide
        for text in ["", "soon", "nan"] {
            assert!(!fresh(text, now), "{text}");
        }
    }

    #[test]
    fn of_two_claims_on_an_old_stamp_one_wins() {
        let dir = std::env::temp_dir().join(format!("symphony-sweep-stamp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let stamp = dir.join(STAMP);
        assert!(claim(&stamp, 1000.0).unwrap()); // none yet: old
        assert!(!claim(&stamp, 1001.0).unwrap()); // written just now
        assert!(claim(&stamp, 1000.0 + HOURLY_SECONDS).unwrap());
        let held = open_stamp(&stamp).unwrap();
        held.lock().unwrap();
        assert!(!claim(&stamp, 9.0e9).unwrap()); // another holds it: not waited for
        drop(held);
        fs::remove_dir_all(&dir).unwrap();
    }

    /// The holder that made the test above red under load: a copy of a claim's descriptor, which a
    /// child another thread forked keeps until it execs. A clone of the file is that copy: one open
    /// file description, two descriptors.
    #[test]
    fn a_claim_lets_go_of_the_stamp_whatever_copy_of_its_descriptor_a_fork_still_holds() {
        let dir = std::env::temp_dir().join(format!("symphony-sweep-fork-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let stamp = dir.join(STAMP);
        let held = lock_stamp(&stamp).unwrap().unwrap();
        let fork = held.0.try_clone().unwrap();
        drop(held);
        assert!(
            claim(&stamp, 1000.0).unwrap(),
            "the stamp stayed locked once its claim ended, by a copy of its descriptor"
        );
        drop(fork);
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A folder of its own under the temporary one, for test `name`.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("symphony-sweep-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_sweep_never_waits_for_the_one_that_runs_and_only_the_holder_writes_the_stamp() {
        let common = scratch("hold");
        let first = hold(&common).unwrap().unwrap();
        let stamp = folder(&common).join(STAMP);
        assert!(fresh(&fs::read_to_string(&stamp).unwrap(), now()));
        let written = fs::read_to_string(&stamp).unwrap();
        assert!(hold(&common).unwrap().is_none());
        assert_eq!(fs::read_to_string(&stamp).unwrap(), written);
        drop(first);
        assert!(hold(&common).unwrap().is_some());
        let written = fs::read_to_string(&stamp).unwrap();
        let stamped = lock_stamp(&stamp).unwrap().unwrap();
        assert!(hold(&common).unwrap().is_some());
        assert_eq!(fs::read_to_string(&stamp).unwrap(), written);
        drop(stamped);
        fs::remove_dir_all(&common).unwrap();
    }

    #[test]
    fn the_log_is_moved_aside_past_its_limit_and_appended_to_below_it() {
        let dir = scratch("log");
        fs::write(dir.join(LOG), "small\n").unwrap();
        writeln!(open_log(&dir).unwrap(), "more").unwrap();
        assert_eq!(fs::read_to_string(dir.join(LOG)).unwrap(), "small\nmore\n");
        assert!(!dir.join(format!("{LOG}.1")).exists());
        let big = "x".repeat(LOG_LIMIT as usize + 1);
        fs::write(dir.join(LOG), &big).unwrap();
        writeln!(open_log(&dir).unwrap(), "fresh").unwrap();
        assert_eq!(fs::read_to_string(dir.join(LOG)).unwrap(), "fresh\n");
        assert_eq!(
            fs::read_to_string(dir.join(format!("{LOG}.1"))).unwrap(),
            big
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
