//! What the agents API and the monitor ask of the machine: the test runs holding or queued for a
//! slot, hc's terminals (as [`crate::launch::hc`] asks), the branch checked out in a folder, the
//! working directory of every process, and whether a task's branch already landed on main.

use super::Env;
use super::agents::{Answer, Began, Pane, Probes, Process, ProcessScan, Run};
use crate::channel::channel;
use crate::git;
use crate::launch::hc::{Binary, Hc};
use crate::sys::{self, Command};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::{self, File, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;

/// The project file's key naming the folder of the project's test-slot locks (`[board] slot_locks`,
/// [`crate::project::board_path`]): a project that names none has no slots to probe.
pub const SLOT_LOCKS: &str = "slot_locks";
/// Each pool and the prefix of its files (`test_slots.POOLS`).
const POOLS: [(&str, &str); 2] = [("run", "slot"), ("render", "render")];

/// The machine's own probes for the board in `env`'s git repository: hc as `HC_BIN` names it, the
/// slot locks in the folder the project names ([`SLOT_LOCKS`]), and `git` for branches, each folder
/// asked once.
pub fn machine(env: &Env) -> Probes<'_> {
    machine_with_common(env, git::common_dir(&env.cwd).map_err(|e| e.to_string()))
}

/// A long-lived reader already discovered this repository; ticks need no repository subprocess.
pub fn machine_in<'a>(env: &'a Env, common: &Path) -> Probes<'a> {
    machine_with_common(env, Ok(common.to_path_buf()))
}

/// How long the Symphony's tick gives a native inventory reader to answer ([`native_inventory`]),
/// so a reader that hangs never holds the tick: past it the reader is killed and its scan unknown.
pub const NATIVE_READER_WAIT: Duration = Duration::from_secs(2);

/// One read-only native scan through the existing bounded child/pipe owner, its answer given up and
/// the reader killed once `limit` has passed. The reader is an already installed local executable;
/// this never builds a launcher or forwards caller capabilities.
pub fn native_inventory(
    env: &Env,
    reader: &Path,
    common: &Path,
    limit: Duration,
) -> Answer<Vec<u8>> {
    let vars = env
        .vars
        .iter()
        .filter(|(key, _)| {
            matches!(
                key.as_str(),
                "PATH" | "LANG" | "LC_ALL" | "LLVM_PROFILE_FILE"
            )
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let mut command = sys::command_with(reader, &vars);
    command
        .args(["machine-work", "--common-dir"])
        .arg(common)
        .current_dir(&env.cwd);
    let output = within(&mut command, limit)?;
    if !output.status.success() || output.stdout.len() > super::machine_work::native::MAX_BYTES {
        return Err("native inventory reader failed or exceeded its output bound".into());
    }
    Ok(output.stdout)
}

fn machine_with_common(env: &Env, common: Result<PathBuf, String>) -> Probes<'_> {
    let branches: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    Probes {
        panes: Box::new(move || hc_terminals(env)),
        runs: Box::new(move || match &common {
            Ok(common) => crate::project::board_path(common, SLOT_LOCKS)
                .map_or_else(|| Ok(Vec::new()), |folder| slot_runs(&folder)),
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
        processes: Box::new(processes),
    }
}

/// Every process `lsof` lists, where it works as the worktree sweep reads it (a sessionless `lsof`,
/// [`process_cwd::pid_cwds_with`]) and when it started as the kernel stamped it, on the clock it
/// dates starts by ([`sys::process_starts`]), none when that cannot be read; one that ended since it
/// was listed is left out. Readable rows can prove presence even when a partial listing cannot
/// prove absence.
pub fn processes() -> Answer<ProcessScan> {
    let scan = process_cwd::pid_cwds_with(sys::command("lsof"))
        .ok_or_else(|| "lsof cannot list the processes' working directories".to_string())?;
    let pids: Vec<i64> = scan.listed.iter().map(|(pid, _)| *pid).collect();
    let starts = sys::process_starts(&pids);
    let complete = scan.complete;
    let listed = scan
        .listed
        .into_iter()
        .zip(starts)
        .filter_map(|((_, cwd), start)| {
            let started = match start {
                sys::Start::At(at) => Some(Began::At(at)),
                sys::Start::AfterBoot(read) => Some(Began::AfterBoot(read)),
                sys::Start::Unreadable => None,
                sys::Start::Ended => return None,
            };
            Some(Process { cwd, started })
        })
        .collect();
    Ok(ProcessScan { listed, complete })
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
/// `acquire` the slot busy). `dir` is the slot locks' folder; none there is no run.
pub fn slot_runs(dir: &Path) -> Answer<Vec<Run>> {
    let mut found = Vec::new();
    if !dir.is_dir() {
        return Ok(found);
    }
    let mut names: Vec<String> = fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .collect();
    names.sort_unstable();
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
        for path in places(dir, &names, prefix) {
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
    found.sort_unstable();
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
    match serde_json::from_slice(text.trim().as_bytes()).ok()? {
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

// -- hc ---------------------------------------------------------------------------------

/// The machine's live hc terminals (`HC_BIN ls`), whoever started them, as panes: the agents view
/// finds a session's by its recorded terminal or its name, and touches none.
pub fn hc_terminals(env: &Env) -> Answer<Vec<Pane>> {
    let live = Binary::of(env).live().map_err(|e| e.0)?;
    Ok(live.iter().filter_map(Pane::from_hc).collect())
}

impl Pane {
    /// A terminal from hc's JSON (`ls` or `get`), whatever else hc says of it; None for one with
    /// no address.
    pub fn from_hc(terminal: &serde_json::Map<String, Value>) -> Option<Pane> {
        use crate::launch::hc;
        let text = |key: &str| {
            terminal
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_string)
        };
        let id = text("address").filter(|id| !id.is_empty())?;
        let foreground = terminal
            .get("foreground")
            .and_then(|f| f.get("cwd"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let cwd = [foreground, text("cwd")]
            .into_iter()
            .flatten()
            .find(|cwd| !cwd.is_empty())
            .unwrap_or_default();
        let session = hc::conversation(&hc::argv(terminal)).map(|id| {
            id.strip_prefix(crate::launch::CODEX)
                .unwrap_or(&id)
                .to_string()
        });
        Some(Pane {
            id,
            agent: hc::agent(terminal).map(str::to_string),
            status: Some(hc::status(terminal)).filter(|s| !s.is_empty()),
            turn: terminal.get("turn").and_then(Value::as_u64),
            session,
            cwd,
            title: text("title"),
            name: text("name"),
        })
    }
}

/// `command`'s output, if it ends within `limit`; else it is killed. Read through the real pipes
/// [`sys::spawn_output`] makes, so a child another thread spawns meanwhile cannot hold a finished
/// answer past `limit`.
pub(crate) fn within(command: &mut Command, limit: Duration) -> Answer<Output> {
    within_fed(command, &[], limit)
}

/// [`within`], with `input` written to the child's stdin, which then ends. The child stays in its
/// caller's process group, so a signal to that group (a server ending a `tasks reap`, Ctrl-C in a
/// terminal) ends it with its caller; the kill once `limit` runs out reaches the child alone. A
/// process the child started that still holds its output keeps the reader, and the child
/// unreaped, until it ends; the answer is given up at `limit` all the same.
pub(crate) fn within_fed(command: &mut Command, input: &[u8], limit: Duration) -> Answer<Output> {
    let mut child = if input.is_empty() {
        sys::spawn_output(command)
    } else {
        sys::spawn_piped(command)
    }
    .map_err(|e| e.to_string())?;
    let pid = i64::from(child.id());
    let (send, receive) = channel();
    let input = input.to_vec();
    std::thread::spawn(move || {
        if let Some(mut stdin) = child.stdin.take() {
            // A child that ends without reading its input answers all the same.
            let _ = stdin.write_all(&input);
        }
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
    sys::output(sys::git().arg("-C").arg(dir).args(args)).ok()
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

/// The tip of the branch task `slug` names in the repository at `root`, and when it was committed;
/// none when there is no such branch. With `after` (a commit a landing's record judged, and when
/// the record was written), the commit the branch first moved to from that one after then, when
/// the executor answered the changes the record asked for, and when it moved, as the branch's
/// reflog tells ([`moved_from`]): the next commit made on it, or what replaced it, amended or
/// rebased away; for a record of a tip the branch came back to, the move after it came back. None
/// while the branch has not moved from it since. Where the reflog does not name it, the first
/// commit after it on the branch's line of first parents, when it was made; else the tip stands in
/// (also for a commit that reads as no commit id, never passed to git). The pipeline alarm judges
/// a landing's record of what a task waits for against it (`cli::pull`), and the board keeps the
/// first answer of each record (`Store::record_fixed`).
pub fn tip(
    root: &Path,
    slug: &str,
    after: Option<crate::alarm::Written<'_>>,
) -> Option<crate::alarm::Tip> {
    let reference = format!("refs/heads/{}", crate::on_review::branch_of(slug));
    let now = commit(
        root,
        &["log", "-1", "--format=%H %ct", &reference, "--"],
        None,
    )?;
    let Some(crate::alarm::Written {
        tip: judged,
        at: written,
        ..
    }) = after
    else {
        return Some(now);
    };
    if judged.is_empty() || !judged.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Some(now);
    }
    if let Some(moved) = moved_from(root, &reference, judged, written) {
        return Some(moved);
    }
    if judged == now.commit {
        return None;
    }
    let range = format!("{judged}..{reference}");
    let line = [
        "log",
        "--first-parent",
        "--ancestry-path",
        "--reverse",
        "--format=%H %ct %P",
        &range,
        "--",
    ];
    // Only a commit whose first parent is the one judged follows it on the branch's own line: the
    // ancestry path also reaches it through a merge's other parent, past a line that never held it.
    Some(commit(root, &line, Some(judged)).unwrap_or(now))
}

/// The commit `reference`'s reflog says it first moved to from `judged` after a record written at
/// `written` judged it, and when it moved: from the newest entry naming `judged` by then (else the
/// oldest naming it), the next that names another. What replaced a commit amended or rebased away,
/// which later moves never change; none when the reflog names no move from it, or not `judged`.
fn moved_from(
    root: &Path,
    reference: &str,
    judged: &str,
    written: f64,
) -> Option<crate::alarm::Tip> {
    let moves = [
        "log",
        "--walk-reflogs",
        "--date=unix",
        "--format=%H %gd",
        reference,
        "--",
    ];
    let done = git(root, &moves).filter(|done| done.status.success())?;
    let text = String::from_utf8_lossy(&done.stdout);
    let newest_first: Vec<(&str, f64)> = (text.lines())
        .filter_map(|line| {
            let (commit, selector) = line.split_once(' ')?;
            let at = selector.rsplit_once("@{")?.1.strip_suffix('}')?;
            Some((commit, at.parse().ok()?))
        })
        .collect();
    let named = |(commit, _): &(&str, f64)| *commit == judged;
    let by_then = (newest_first.iter()).position(|entry| named(entry) && entry.1 <= written);
    let became = by_then.or_else(|| newest_first.iter().rposition(named))?;
    let mut newer = newest_first[..became].iter().rev();
    let (commit, at) = newer.find(|entry| !named(entry))?;
    Some(crate::alarm::Tip {
        commit: commit.to_string(),
        at: *at,
    })
}

/// The reader of the tips of the branches in the repository at `root`, none without one ([`tip`]).
pub fn tips(
    root: Option<&Path>,
) -> impl Fn(&crate::Task, Option<crate::alarm::Written<'_>>) -> Option<crate::alarm::Tip> {
    move |task, after| tip(root?, &task.slug, after)
}

/// The first commit git's `args` list, as `%H %ct`; with `parent`, as `%H %ct %P`, and none unless
/// `parent` is its first parent.
fn commit(root: &Path, args: &[&str], parent: Option<&str>) -> Option<crate::alarm::Tip> {
    let done = git(root, args).filter(|done| done.status.success())?;
    let text = String::from_utf8_lossy(&done.stdout);
    let mut fields = text.lines().next()?.split_whitespace();
    let (commit, at) = (fields.next()?, fields.next()?);
    if parent.is_some_and(|parent| fields.next() != Some(parent)) {
        return None;
    }
    Some(crate::alarm::Tip {
        commit: commit.to_string(),
        at: at.parse().ok()?,
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
