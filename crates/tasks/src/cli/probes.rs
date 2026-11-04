//! What the agents API and the monitor ask of the machine: the test runs holding or queued for a
//! slot, herdr's panes (as [`crate::launch::herdr`] asks), the branch checked out in a folder, and
//! whether a task's branch already landed on main.

use super::Env;
use super::agents::{Answer, Pane, Probes, Run};
use crate::git;
use crate::sys::{self, Command};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::{self, File, TryLockError};
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::mpsc;
use std::time::Duration;

/// The slot locks' folder in the git common dir (`test_slots.DIR_NAME`).
pub const SLOTS_DIR: &str = "sf-test-slots";
/// Each pool and the prefix of its files (`test_slots.POOLS`).
const POOLS: [(&str, &str); 2] = [("run", "slot"), ("render", "render")];
/// How long herdr may take to answer.
const HERDR_PATIENCE: Duration = Duration::from_secs(120);

/// The machine's own probes for the board in `env`'s git repository: herdr as `HERDR_BIN` names it,
/// the slot locks in the git common dir, and `git` for branches, each folder asked once.
pub fn machine(env: &Env) -> Probes<'_> {
    machine_with_common(env, git::common_dir(&env.cwd).map_err(|e| e.to_string()))
}

/// A long-lived reader already discovered this repository; ticks need no repository subprocess.
pub fn machine_in<'a>(env: &'a Env, common: &Path) -> Probes<'a> {
    machine_with_common(env, Ok(common.to_path_buf()))
}

fn machine_with_common(env: &Env, common: Result<PathBuf, String>) -> Probes<'_> {
    let branches: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    Probes {
        panes: Box::new(move || herdr_panes(env)),
        runs: Box::new(move || match &common {
            Ok(common) => slot_runs(common),
            Err(why) => Err(why.to_string()),
        }),
        branch_of: Box::new(move |cwd| {
            if let Some(known) = branches.borrow().get(cwd) {
                return Ok(known.clone());
            }
            let found = branch_of(cwd)?;
            branches.borrow_mut().insert(cwd.into(), found.clone());
            Ok(found)
        }),
    }
}

/// The main checkout of the repository `cwd` is in, whichever worktree that is: the common dir's
/// parent.
pub fn repo_root(cwd: &Path) -> Option<PathBuf> {
    git::common_dir(cwd).ok()?.parent().map(Path::to_path_buf)
}

// -- the slot locks --------------------------------------------------------------------

/// Every run holding a slot or queued for one: `holding` (its slot file is locked right now by a live
/// process; the file's last writer is the holder) or `waiting` (a place in the pool's line that its
/// waiter still holds, oldest first). A slot file nobody has locked is no run, though it keeps its
/// last holder's name, and is not locked to find out when its writer is dead (that would show a real
/// `acquire` the slot busy).
pub fn slot_runs(common: &Path) -> Answer<Vec<Run>> {
    let dir = common.join(SLOTS_DIR);
    let mut found = Vec::new();
    if !dir.is_dir() {
        return Ok(found);
    }
    let mut names: Vec<String> = fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .collect();
    names.sort();
    for (pool, prefix) in POOLS {
        for name in names
            .iter()
            .filter(|n| between(n, prefix, ".lock").is_some())
        {
            let path = dir.join(name);
            let Some(pid) = holder(&path).and_then(|info| pid_of(&info)) else {
                continue;
            };
            if !sys::pid_alive(pid) {
                continue;
            }
            let Ok(file) = File::options()
                .read(true)
                .append(true)
                .create(true)
                .open(&path)
            else {
                continue;
            };
            match file.try_lock() {
                Ok(()) => {
                    // A forked child may still hold this description after our file closes.
                    file.unlock().map_err(|e| e.to_string())?;
                    continue;
                }
                Err(TryLockError::WouldBlock) => {}
                Err(TryLockError::Error(problem)) => return Err(problem.to_string()),
            }
            drop(file);
            // Read again: the holder may have changed while we looked.
            if let Some(info) = holder(&path).filter(|info| pid_of(info).is_some()) {
                found.push(run(&info, "holding", pool));
            }
        }
        for path in places(&dir, &names, prefix) {
            if let Some(info) = holder(&path).filter(|info| pid_of(info).is_some())
                && standing(&path)?
            {
                found.push(run(&info, "waiting", pool));
            }
        }
    }
    Ok(found)
}

/// The text of `name` between `prefix-` and `suffix`, if it has both.
fn between<'n>(name: &'n str, prefix: &str, suffix: &str) -> Option<&'n str> {
    name.strip_prefix(prefix)?
        .strip_prefix('-')?
        .strip_suffix(suffix)
}

/// Every place file in `prefix`'s line, standing or not, in arrival order.
fn places(dir: &Path, names: &[String], prefix: &str) -> Vec<PathBuf> {
    let mut found: Vec<(u64, &String)> = names
        .iter()
        .filter_map(|name| {
            let seq = between(name, prefix, ".place")?;
            let digits = !seq.is_empty() && seq.bytes().all(|b| b.is_ascii_digit());
            Some((seq.parse().ok().filter(|_| digits)?, name))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, name)| dir.join(name)).collect()
}

/// Whoever took this place still holds it: its own lock, held until it leaves, dies with its process.
fn standing(path: &Path) -> Answer<bool> {
    let Ok(file) = File::open(path) else {
        return Ok(false);
    };
    match file.try_lock_shared() {
        Ok(()) => {
            file.unlock().map_err(|e| e.to_string())?;
            Ok(false)
        }
        Err(TryLockError::WouldBlock) => Ok(true),
        Err(TryLockError::Error(problem)) => Err(problem.to_string()),
    }
}

/// Who a slot or place file says holds it, if it reads as a JSON object.
fn holder(path: &Path) -> Option<serde_json::Map<String, Value>> {
    let text = fs::read_to_string(path).ok()?;
    match serde_json::from_str(text.trim()).ok()? {
        Value::Object(info) => Some(info),
        _ => None,
    }
}

fn pid_of(info: &serde_json::Map<String, Value>) -> Option<i64> {
    info.get("pid").and_then(Value::as_i64)
}

fn run(info: &serde_json::Map<String, Value>, phase: &str, pool: &str) -> Run {
    let text = |key: &str| info.get(key).and_then(Value::as_str).map(str::to_string);
    Run {
        pid: pid_of(info).unwrap_or_default(),
        worktree: text("worktree").unwrap_or_default(),
        recipe: text("recipe").unwrap_or_default(),
        phase: phase.into(),
        since: info.get("since").and_then(Value::as_f64),
        pool: pool.into(),
        handle: text("handle"),
    }
}

// -- herdr -----------------------------------------------------------------------------

/// herdr's panes: `HERDR_BIN pane list`, in the workspace the caller runs in when it runs in one.
pub fn herdr_panes(env: &Env) -> Answer<Vec<Pane>> {
    let binary = split(env.var("HERDR_BIN").unwrap_or("herdr"))?;
    let Some((program, args)) = binary.split_first() else {
        return Err("HERDR_BIN names no command".into());
    };
    let mut command = Command::new(program);
    command.args(args).args(["pane", "list"]);
    if let Some(workspace) = env.var("HERDR_WORKSPACE_ID") {
        command.args(["--workspace", workspace]);
    }
    command.env_clear().envs(&env.vars).current_dir(&env.cwd);
    let done =
        within(&mut command, HERDR_PATIENCE).map_err(|why| format!("herdr pane list: {why}"))?;
    let said = |bytes: &[u8]| String::from_utf8_lossy(bytes).trim().to_string();
    if !done.status.success() {
        let why = Some(said(&done.stderr))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| said(&done.stdout));
        return Err(format!("herdr pane list failed: {why}"));
    }
    let text = String::from_utf8_lossy(&done.stdout);
    let text = if text.is_empty() { "{}" } else { &text };
    let answer: Value =
        serde_json::from_str(text).map_err(|_| "herdr pane list printed no JSON".to_string())?;
    let no_list = || "herdr pane list answered no list of panes".to_string();
    let result = match answer.as_object().ok_or_else(no_list)?.get("result") {
        Some(result) => result.as_object().ok_or_else(no_list)?,
        None => return Ok(Vec::new()),
    };
    match result.get("panes") {
        Some(Value::Array(panes)) => Ok(panes.iter().filter_map(Pane::from_json).collect()),
        Some(_) => Err(no_list()),
        None => Ok(Vec::new()),
    }
}

/// `command`'s output, if it ends within `limit`; else it is killed. Read through the real pipes
/// [`sys::spawn_output`] makes, so a child another thread spawns meanwhile cannot hold a finished
/// answer past `limit`.
pub(crate) fn within(command: &mut Command, limit: Duration) -> Answer<Output> {
    let child = sys::spawn_output(command).map_err(|e| e.to_string())?;
    let pid = i64::from(child.id());
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = send.send(sys::wait_output(child));
    });
    match receive.recv_timeout(limit) {
        Ok(done) => done.map_err(|e| e.to_string()),
        Err(_) => {
            sys::terminate(pid, true);
            Err(format!("no answer in {} s", limit.as_secs()))
        }
    }
}

/// `text` split into words as a POSIX shell splits them (Python's `shlex.split`): quotes group, a
/// backslash escapes, and nothing is a comment.
pub fn split(text: &str) -> Answer<Vec<String>> {
    let (mut words, mut word, mut in_word) = (Vec::new(), String::new(), false);
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\r' | '\n' => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err("No closing quotation".into()),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\')) => word.push(c),
                            Some(c) => {
                                word.push('\\');
                                word.push(c);
                            }
                            None => return Err("No closing quotation".into()),
                        },
                        Some(c) => word.push(c),
                        None => return Err("No closing quotation".into()),
                    }
                }
            }
            '\\' => match chars.next() {
                Some(c) => {
                    in_word = true;
                    word.push(c);
                }
                None => return Err("No escaped character".into()),
            },
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    Ok(words)
}

// -- git -------------------------------------------------------------------------------

fn git(dir: &Path, args: &[&str]) -> Option<Output> {
    sys::output(Command::new("git").arg("-C").arg(dir).args(args)).ok()
}

/// The branch checked out in `cwd`; "" for a detached head or a folder in no repository.
pub fn branch_of(cwd: &str) -> Answer<String> {
    let done = git(Path::new(cwd), &["symbolic-ref", "--short", "-q", "HEAD"])
        .ok_or("git cannot be run")?;
    Ok(if done.status.success() {
        String::from_utf8_lossy(&done.stdout).trim().to_string()
    } else {
        String::new()
    })
}

/// Whether the branch named for task `slug` has commits of its own and is already on main in the
/// repository at `root`: work that landed under a task still open. A branch cut and not yet used is not
/// one: none of its reflog's entries is a commit.
pub fn landed(root: &Path, slug: &str) -> bool {
    let reference = format!("refs/heads/{slug}");
    let ok = |args: &[&str]| git(root, args).is_some_and(|done| done.status.success());
    if !ok(&["rev-parse", "--verify", "-q", &reference])
        || !ok(&["merge-base", "--is-ancestor", &reference, "main"])
    {
        return false;
    }
    let log = git(root, &["reflog", "show", "--format=%gs", &reference])
        .map(|done| String::from_utf8_lossy(&done.stdout).into_owned())
        .unwrap_or_default();
    let entries: Vec<&str> = log.lines().collect();
    let no_commits = !entries.is_empty() && !entries.iter().any(|e| e.starts_with("commit"));
    !no_commits
}
