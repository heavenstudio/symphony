//! The regenerator: what the project's `[land] generators` make of the default branch, made after a
//! land and never on its path, so a land is its merge, its move and its push alone ([`super`]).
//!
//! Only one version is built for main at a time. Once a version is built, the next build takes the
//! most recent one; a build in progress is never cancelled, but left to finish. So there is no
//! queue, only a limit of one at a time, which may skip some merges from building entirely.
//!
//! A land that moved the default branch starts it, detached ([`start`]), and waits for nothing it
//! does; while one runs ([`LOCK`], which names its process and start) a land starts nothing and
//! signals nothing, and the default branch's ref is all it leaves: there is no list of builds to
//! make anywhere. One that finds the lock held ends at once, and a lock its holder died with is
//! free for the next. Each round ([`run`]) takes the default branch's newest tip into a worktree
//! of its own that it keeps between rounds (`regenerate` in the project's `[agents] worktrees`,
//! else `symphony/tasks/land/regenerate-worktree`), so what its builds leave there stays warm, and
//! runs the generators there in order, a `when_changed` one only after an earlier one changed
//! something. A generator with a key (`[land] keys`) prints it first: a key and outputs it passed
//! on before ([`Passed`], in `symphony/tasks/land/`) need nothing made. A round is never cut short
//! or begun again because the default branch moved. What the generators change, inside their
//! outputs alone, is one commit, `Regenerate NAMES for BRANCH at TIP`, landed in seconds under the
//! land's lock: the default branch's tree as it is then with those outputs replaced, made only when
//! what the generators are made of is the same there as at the tip (each one's key, else every path
//! outside their outputs), else discarded as stale and said; then the fast-forward of the main
//! checkout and the push, as a land makes them. It needs no reviewer's approve: it changes the
//! generators' outputs alone, made by them of what the default branch holds. Once it lets go of its
//! lock it reads the default branch once: on another tip than the one it finished with, it makes
//! that tip, the newest, whatever lands came in between, else it ends.
//!
//! Where the project delivers by pull request (`[land] delivery`, [`super::delivery`]) a round
//! commits nothing to the default branch and pushes nothing. What it made is kept as [`Pending`],
//! then pointed at by a branch and filed as a task in review ([`Regenerator::propose`]), which an
//! assessor reads and a land delivers as any task, checked first as the regenerator's own
//! ([`Landing::generated`]). While that record's inputs are the tip's, a round makes nothing
//! ([`Regenerator::waiting`]); once they are not, its task is cancelled, a pull request it was
//! bound to is told ([`Regenerator::retract`]), and the newest tip is made. Nothing else is kept
//! of what is to make: a start that never happened is made up for by any later one.
//!
//! A red round (a generator failed, or wrote outside its outputs) files the task [`RED`] on the
//! board, marked red (`tests/main-red-regenerate-2` and on once one was closed), or adds a line
//! to the one open; a round that passes closes it. What the last round came to is kept in
//! [`STATE`] and said by [`status`]. Its lines go to [`LOG`].
//!
//! Where `[land] check = false` keeps `[verdicts] check` out of the land, each round runs it in
//! its worktree on what it left the default branch holding (the commit it landed, with whatever
//! the branch gained during the build, else the tip it took), once that commit has landed, so it
//! never holds back the generators' commit or a land ([`Regenerator::check`]): part of the round,
//! under its lock, never cut short because the branch moved, ended only by its time limit or a
//! signal. A round discarded as stale, or stopped, is not checked: the one after it checks the
//! newer tip. A red files [`RED_CHECK`] the same way, a task of its own beside the generators'
//! (one open at a time, a line added while it stays red), its text saying what is known of a
//! check that ran past its time or could not run; a later round that passes closes it. What it
//! came to, or that it runs, is kept in [`CHECKED`] and said by [`status`], whose exit stays the
//! outputs' alone.

use super::delivery::{self, Journal, Receipt, Stage, Transport};
use super::{
    FOLDER, GIT_VARS, Generator, Landing, Move, RECIPE, Ran, Refusal, Settings, TRIES, and, at,
    changed, clear, hold, on_paths, owns, run_git, same, short, signal_name, started, worktree,
};
use crate::cli::Env;
use crate::sweep::git::{ancestor, rev, worktrees};
use crate::sweep::trigger;
use crate::sys::{self, Stdio};
use crate::{NewSession, NewTask};
use crate::{Store, red};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The lock one regenerator holds while it runs, in the lands' folder.
pub const LOCK: &str = "regenerate.lock";
/// What its last round came to ([`State`]), in the lands' folder.
pub const STATE: &str = "regenerate.json";
/// The keys and outputs each generator passed on ([`Passed`]), in the lands' folder.
pub const PASSED: &str = "regenerated.json";
/// Its log, in the lands' folder: set aside as `regenerate.log.1` by a start that finds it past the
/// sweep's bound ([`trigger::LOG_LIMIT`]), the one before dropped.
pub const LOG: &str = "regenerate.log";
/// The file naming the group a round runs while it runs, in the lands' folder: its own, so a land
/// never takes a regenerator's build for a group an earlier land left.
pub const RUNNING: &str = "regenerate.running";
/// What a round made that waits for its pull request ([`Pending`]), in the lands' folder.
pub const PENDING: &str = "regenerate.pending";
/// The tasks the regenerator cancelled whose pull request is still to be told its evidence no
/// longer stands ([`Regenerator::retract`]), in the lands' folder.
pub const WITHDRAWN: &str = "regenerate.withdrawn";
/// The red task a red round files.
pub const RED: &str = "tests/main-red-regenerate";
/// What a red round marks its task with ([`red::MARK`]).
const MARK: &str = "the regenerator: [land] generators fail on the default branch";
/// The red task a round whose check is red files ([`Regenerator::check`]).
pub const RED_CHECK: &str = "tests/main-red-check";
/// What the check last came to ([`Checked`]), in the lands' folder.
pub const CHECKED: &str = "regenerate-check.json";
/// The start of the reason its worktree is locked with, which keeps it from the sweep.
const REASON: &str = "symphony regenerate";
/// What it is called in its lines, where a land names its task.
const NAME: &str = "regenerate";
/// What follows a key it cannot read before it makes the outputs: it makes them.
const MAKING: &str = "making it";
/// What follows a key it cannot read on a default branch that moved: what it made is stale.
const STALE: &str = "what was made is stale";
/// How many pairs [`Passed`] keeps, the newest first.
const KEPT: usize = 64;

/// Whether a land in `env` starts the regenerator: `TASKS_REGENERATE=on` or `off` says; else only
/// on the repository's own board, which no `TASKS_SERVER` names (a test's starts none).
pub fn enabled(env: &Env) -> bool {
    env.switch("TASKS_REGENERATE")
}

/// A generator's key and the object its outputs were at when it passed on that key: a tip with
/// both needs nothing made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pair {
    pub generator: String,
    pub key: String,
    pub outputs: String,
}

/// The pairs the generators passed on, newest first, [`KEPT`] at most.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Passed {
    pub pairs: Vec<Pair>,
}

impl Passed {
    /// As the lands' `folder` keeps them; none when it keeps none or cannot be read.
    pub fn read(folder: &Path) -> Passed {
        fs::read_to_string(folder.join(PASSED))
            .ok()
            .and_then(crate::objects::parse)
            .unwrap_or_default()
    }

    /// Whether `pair` is one of them.
    pub fn holds(&self, pair: &Pair) -> bool {
        self.pairs.contains(pair)
    }

    /// `pair` first, once.
    pub fn add(&mut self, pair: Pair) {
        self.pairs.retain(|kept| *kept != pair);
        self.pairs.insert(0, pair);
        self.pairs.truncate(KEPT);
    }

    /// Kept in `folder`, whole or not at all.
    pub fn write(&self, folder: &Path) -> Result<(), String> {
        write_whole(folder, PASSED, self)
    }
}

/// A generator whose outputs a commit holds, with the key it printed on the tip they were made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Made {
    pub generator: String,
    pub key: Option<String>,
}

/// What a round made where the project delivers by pull request, until the host lands it: the one
/// record a restart, a later round and the land of its task go by, written before the branch and
/// the task it names are made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// The task it is delivered as, and its branch.
    pub slug: String,
    /// The default branch's tip it was made of: the one parent of `head`.
    pub tip: String,
    /// The commit made: `tip` with the generators' outputs as they made them.
    pub head: String,
    pub made: Vec<Made>,
}

impl Pending {
    /// As the lands' `folder` keeps it: a line each for the task, the tip and the commit, then
    /// one for each generator, its key after a tab where it has one.
    pub fn read(folder: &Path) -> Option<Pending> {
        let text = fs::read_to_string(folder.join(PENDING)).ok()?;
        let mut lines = text.lines();
        let mut next = || lines.next().map(str::to_string);
        let (slug, tip, head) = (next()?, next()?, next()?);
        let made = (lines)
            .map(|line| {
                let (generator, key) = match line.split_once('\t') {
                    Some((generator, key)) => (generator, Some(key.to_string())),
                    None => (line, None),
                };
                Made {
                    generator: generator.to_string(),
                    key,
                }
            })
            .collect();
        Some(Pending {
            slug,
            tip,
            head,
            made,
        })
    }

    /// Kept in `folder`, whole or not at all.
    pub fn write(&self, folder: &Path) -> Result<(), String> {
        let mut text = format!("{}\n{}\n{}\n", self.slug, self.tip, self.head);
        for made in &self.made {
            text.push_str(&made.generator);
            if let Some(key) = &made.key {
                text.push('\t');
                text.push_str(key);
            }
            text.push('\n');
        }
        write_lines(folder, PENDING, &text)
    }

    /// The generators of `settings` it names, each with its key: none when one is gone.
    fn made_by(&self, settings: &Settings) -> Option<Vec<(Generator, Option<String>)>> {
        (self.made.iter())
            .map(|made| {
                (settings.generators.iter())
                    .find(|generator| generator.name == made.generator)
                    .map(|generator| (generator.clone(), made.key.clone()))
            })
            .collect()
    }
}

/// What a round came to, kept in [`STATE`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// `building`, `current`, `red` or `stopped`.
    pub state: String,
    /// The default branch's tip the round took.
    pub tip: String,
    /// The commit it landed, when it made one.
    #[serde(default)]
    pub landed: Option<String>,
    /// The newest commit of the default branch whose committed outputs were found to be what the
    /// generators make of it, by this round or one before: none before the first.
    #[serde(default)]
    pub current: Option<String>,
    /// When it was written, in seconds since the epoch.
    pub at: f64,
    /// The regenerator that wrote it, and when that process started ([`started`]).
    pub pid: i64,
    #[serde(default)]
    pub start: Option<String>,
    /// What it came to, in words.
    pub line: String,
}

impl State {
    /// As the lands' `folder` keeps it.
    pub fn read(folder: &Path) -> Option<State> {
        let text = fs::read_to_string(folder.join(STATE)).ok()?;
        crate::objects::parse(&text)
    }

    /// Whether a round finished with the default branch at `tip`: its outputs found current there,
    /// landed there, or red there.
    fn finished(&self, tip: &str) -> bool {
        match self.state.as_str() {
            "current" => self.tip == tip || self.landed.as_deref() == Some(tip),
            "red" => self.tip == tip,
            _ => false,
        }
    }

    fn new(
        state: &str,
        tip: &str,
        landed: Option<&str>,
        current: Option<String>,
        line: &str,
    ) -> State {
        let pid = i64::from(std::process::id());
        State {
            state: state.into(),
            tip: tip.into(),
            landed: landed.map(str::to_string),
            current,
            at: trigger::now(),
            pid,
            start: started(pid),
            line: line.into(),
        }
    }
}

/// What the check no land runs ([`Settings::main_check`]) came to on the last round that ran it, or
/// that it runs, kept in [`CHECKED`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Checked {
    /// `checking` while it runs, then `passed` or `red`.
    pub state: String,
    /// The commit of the default branch it checks: what the round left the branch holding.
    pub tip: String,
    /// The red task it is filed in, while red and the board took it.
    #[serde(default)]
    pub task: Option<String>,
    /// When it began or ended, in seconds since the epoch.
    pub at: f64,
    /// What it came to, in words.
    pub line: String,
    /// The regenerator that wrote it, and when that process started ([`started`]).
    #[serde(default)]
    pub pid: i64,
    #[serde(default)]
    pub start: Option<String>,
}

impl Checked {
    /// As the lands' `folder` keeps it.
    pub fn read(folder: &Path) -> Option<Checked> {
        let text = fs::read_to_string(folder.join(CHECKED)).ok()?;
        crate::objects::parse(&text)
    }

    /// `state` on `tip` as this process writes it now, filed in `task`, said by `line`.
    fn new(state: &str, tip: &str, task: Option<String>, line: &str) -> Checked {
        let pid = i64::from(std::process::id());
        Checked {
            state: state.into(),
            tip: tip.into(),
            task,
            at: trigger::now(),
            line: line.into(),
            pid,
            start: started(pid),
        }
    }
}

/// Whether the process `pid` that wrote a record still runs: the one of `start` when it was read,
/// else any by that number.
fn lives(pid: i64, start: Option<&str>) -> bool {
    match start {
        Some(start) => started(pid).as_deref() == Some(start),
        None => sys::pid_alive(pid),
    }
}

/// `value` as JSON in `folder`'s `name`, whole or not at all.
fn write_whole<T: Serialize>(folder: &Path, name: &str, value: &T) -> Result<(), String> {
    let path = folder.join(name);
    let staged = folder.join(format!(".{name}.{}", std::process::id()));
    let text = serde_json::to_string(value).map_err(|e| e.to_string())?;
    fs::write(&staged, text).map_err(|e| at(&staged, &e))?;
    fs::rename(&staged, &path).map_err(|e| at(&path, &e))
}

/// The generators' names, as words.
fn names(generators: &[Generator]) -> String {
    and(&generators
        .iter()
        .map(|g| g.name.as_str())
        .collect::<Vec<_>>())
}

/// How a start came out.
enum Started {
    /// The log it writes to.
    Log(PathBuf),
    /// Who runs already, in words.
    Running(String),
}

/// Start the regenerator after a land moved the default branch of the main checkout `main`, whose
/// committed `settings` name its generators, detached: `tasks regenerate` from the main checkout,
/// by a test's `TASKS_BIN` on a test's board, else by the land's own build, else the main
/// checkout's build of `tasks` ([`crate::builds::of_main`]), with `env`'s environment but its
/// session and the unasked recipes' mark, the land's kept, its output appended to [`LOG`]; a
/// thread reaps it. While one runs it starts nothing and signals nothing: the default branch's ref
/// is all a land leaves it, and the one running reads it once it ends. None when there are no
/// generators or `env` starts none ([`enabled`]); else the line saying it started, that one runs,
/// or why it did not start.
pub fn start(env: &Env, main: &Path, settings: &Settings) -> Option<String> {
    if settings.generators.is_empty() || !enabled(env) {
        return None;
    }
    let what = names(&settings.generators);
    let default = &settings.default_branch;
    let started = (|| -> Result<Started, String> {
        let common = crate::git::common_dir(main).map_err(|e| e.to_string())?;
        let folder = lands(&common);
        fs::create_dir_all(&folder).map_err(|e| at(&folder, &e))?;
        if let Some(running) = holder(&folder)? {
            return Ok(Started::Running(running));
        }
        // The land's own build, which holds the regenerator it starts: a link to the main
        // checkout's may be older than it.
        let binary = trigger::test_build(env)
            .or_else(|| std::env::current_exe().ok())
            .or_else(|| crate::builds::of_main(main, "tasks", &trigger::beyond(env)))
            .ok_or_else(|| crate::builds::missing(main, "tasks"))?;
        // Set aside past its bound as the sweep's log is, only here: no regenerator runs, so none
        // writes on into the file set aside.
        let path = folder.join(LOG);
        let log = trigger::append_log(&folder, LOG).map_err(|e| at(&path, &e))?;
        let mut command = sys::command_with(&binary, &env.vars);
        sys::sessionless(&mut command).env_remove("TASKS_TASK");
        for name in GIT_VARS {
            command.env_remove(name);
        }
        // The land's mark, which no other child keeps (`sys::MARKS`): the regenerator lands what
        // it makes, so it runs only as a land started it ([`admit`]). The unasked recipes' mark it
        // does not keep: its generators and check write where the project says.
        if let Some(asked) = env.var(RECIPE) {
            command.env(RECIPE, asked);
        }
        command
            .arg("regenerate")
            .current_dir(main)
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(|e| at(&path, &e))?)
            .stderr(log)
            .new_session();
        let mut child = command
            .spawn()
            .map_err(|e| format!("{}: {e}", binary.display()))?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(Started::Log(path))
    })();
    Some(match started {
        Ok(Started::Log(log)) => format!(
            "the regenerator makes {what} in the background (`just tasks regenerate --status`; its \
             log {})",
            log.display()
        ),
        Ok(Started::Running(running)) => {
            format!("{running}: it makes {default}'s newest tip once it ends; nothing started")
        }
        Err(why) => format!("the regenerator of {what} did not start: {why}"),
    })
}

/// The lands' folder in the git common dir `common`, where the regenerator keeps its lock, log and
/// state.
fn lands(common: &Path) -> PathBuf {
    trigger::folder(common).join(FOLDER)
}

/// The regenerator's lock file in the lands' `folder`, opened.
fn lock_file(folder: &Path) -> Result<File, String> {
    let path = folder.join(LOCK);
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| at(&path, &e))
}

/// The regenerator's lock in the lands' `folder`, taken without waiting and named by this process
/// and its start, as a land names its lock: none while another holds it. A regenerator that died
/// leaves the file naming it, and the lock free. The name is written in place, as the lock is the
/// file's, in one write: a land reading it meanwhile finds it empty ([`holder`]) or whole.
fn try_hold(folder: &Path) -> Result<Option<sys::FileLock>, String> {
    let file = lock_file(folder)?;
    match file.try_lock() {
        Ok(()) => {
            let pid = std::process::id();
            let name = match started(i64::from(pid)) {
                Some(start) => format!("the regenerator (pid {pid}, started {start})"),
                None => format!("the regenerator (pid {pid})"),
            };
            let mut named = &file;
            let _ = named.set_len(0);
            let _ = named.seek(SeekFrom::Start(0));
            let _ = named.write_all(name.as_bytes());
            Ok(Some(sys::FileLock(file)))
        }
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(at(&folder.join(LOCK), &e)),
    }
}

/// Who holds the regenerator's lock in the lands' `folder`, in words, and what it makes since when;
/// none when no process holds it, one that died included. A lock taken and not yet named is
/// another regenerator's.
fn holder(folder: &Path) -> Result<Option<String>, String> {
    let mut file = lock_file(folder)?;
    match file.try_lock() {
        // Closing the file lets the lock go at once.
        Ok(()) => Ok(None),
        Err(TryLockError::WouldBlock) => {
            let mut named = String::new();
            let _ = file.read_to_string(&mut named);
            let named = match named.trim() {
                "" => "another regenerator".to_string(),
                named => named.to_string(),
            };
            let making = State::read(folder)
                .filter(|state| state.state == "building")
                .map(|state| {
                    format!(
                        ", making them of {} since {}",
                        short(&state.tip),
                        sys::local_minute(state.at)
                    )
                })
                .unwrap_or_default();
            Ok(Some(format!("{named}{making}")))
        }
        Err(TryLockError::Error(e)) => Err(at(&folder.join(LOCK), &e)),
    }
}

/// What one round came to.
#[derive(Debug, Clone, PartialEq)]
enum Round {
    /// The generators' outputs are what they make of `tip`: nothing to land.
    Current { tip: String, line: String },
    /// The commit `commit`, made on `tip`, landed on the default branch.
    Landed {
        tip: String,
        commit: String,
        line: String,
    },
    /// What was made of the default branch waits for its pull request, on `tip`: nothing landed
    /// here, and nothing is made again while its inputs are the tip's.
    Pending { tip: String, line: String },
    /// The default branch moved since `tip` and changed what the generators are made of: what was
    /// made of it is stale and discarded, and the next round makes the newest tip.
    Dropped { tip: String, line: String },
    /// A generator failed on `tip`.
    Red { tip: String, line: String },
    /// It could not get through, or took a signal.
    Stopped { tip: String, line: String },
}

/// Run the regenerator from where `env` runs: round after round while the default branch moves,
/// each said on `out`; the line saying what the last came to, or why it stopped. One that finds
/// another running ends at once, saying so. `store` takes the red task a red round files.
pub fn run(store: &Store, env: &Env, out: &mut dyn Write) -> Result<String, String> {
    run_by(store, env, out, None)
}

/// [`run`] with `transport` answering a delivery's verbs in place of the command `[land]
/// delivery` names: how a test stands a host in.
pub fn run_by(
    store: &Store,
    env: &Env,
    out: &mut dyn Write,
    transport: Option<&dyn Transport>,
) -> Result<String, String> {
    run_with_clock(store, env, out, transport, &std::time::Instant::now)
}

/// [`run_by`] with the monotonic clock its command timeouts read, as [`super::land_with_clock`]
/// gives a land: a test holds time still through the round's git steps and advances it once the
/// command it means to expire runs.
pub fn run_with_clock(
    store: &Store,
    env: &Env,
    out: &mut dyn Write,
    transport: Option<&dyn Transport>,
    now: &(dyn Fn() -> std::time::Instant + Sync),
) -> Result<String, String> {
    let common = crate::git::common_dir(&env.cwd).map_err(|e| e.to_string())?;
    let main = trigger::main_checkout(&common)
        .ok_or_else(|| format!("{} has no main checkout to regenerate on", common.display()))?
        .to_path_buf();
    let folder = lands(&common);
    fs::create_dir_all(&folder).map_err(|e| at(&folder, &e))?;
    let interrupts = sys::Interrupts::listen().ok();
    let mut said: Option<String> = None;
    loop {
        let settings = Settings::of(&main)?;
        if settings.generators.is_empty() {
            return Ok("no [land] generators: nothing to regenerate".into());
        }
        // Where the project delivers by pull request a round lands nothing: what it makes is a
        // task's branch, which the host lands ([`Regenerator::propose`]).
        let delivery = delivery::command(&main)?;
        let Some(held) = try_hold(&folder)? else {
            return Ok(said.unwrap_or_else(|| {
                "the regenerator runs already: it goes round once it ends, and makes the newest \
                 tip"
                .into()
            }));
        };
        // A tip a round finished with, current or red, is made again only once the default branch
        // moves on from it: two starts at once make it once.
        let default = settings.default_branch.clone();
        let finished = rev(&main, &format!("refs/heads/{default}")).and_then(|tip| {
            State::read(&folder)
                .filter(|state| state.finished(&tip))
                .map(|state| (tip, state))
        });
        if let Some((tip, state)) = finished {
            let what = names(&settings.generators);
            return Ok(said.unwrap_or_else(|| match state.state.as_str() {
                "red" => format!(
                    "{what}: red on {default} at {}: {}",
                    short(&tip),
                    state.line
                ),
                _ => format!("{what}: current on {default} at {}", short(&tip)),
            }));
        }
        let log = folder.join(LOG);
        let mut landing = Landing {
            store,
            env,
            slug: NAME,
            actor: NAME,
            tree: worktree(&main, &settings, &folder, super::FOLDER, "worktree")?,
            running: folder.join(RUNNING),
            task: false,
            main: main.clone(),
            settings,
            folder: folder.clone(),
            log,
            interrupts: interrupts.as_ref(),
            now,
            recorder: None,
            named: false,
            transport,
            delivery,
            mode: super::Mode::Land,
            out: &mut *out,
        };
        let mut regenerator = Regenerator {
            landing: &mut landing,
            common: &common,
            worktree: None,
        };
        let round = regenerator.round();
        regenerator.settle(store, &round);
        let halted = regenerator.check(store, &round);
        let branch = regenerator.landing.branch();
        drop(landing);
        // Read once, after it lets go: a land that moved the default branch while it held its lock
        // started nothing and left it that ref alone, which this one reads now and makes, the
        // newest tip only, however many lands came in between; a land after it lets go finds the
        // lock free and starts one, which makes it.
        drop(held);
        if let Some(line) = halted {
            return Err(line);
        }
        let now = rev(&main, &branch);
        // What the round left the default branch at: the tip it took, or the commit it landed,
        // whose outputs are what the generators make of it as its inputs are the tip's.
        let (left, line, last) = match &round {
            Round::Current { tip, line }
            | Round::Red { tip, line }
            | Round::Pending { tip, line } => (tip.clone(), line, true),
            Round::Landed { commit, line, .. } => (commit.clone(), line, true),
            Round::Dropped { tip, line } => (tip.clone(), line, false),
            Round::Stopped { line, .. } => return Err(line.clone()),
        };
        if last && now.as_deref() == Some(left.as_str()) {
            return Ok(line.clone());
        }
        said = Some(line.clone());
        let _ = writeln!(out, "regenerate: the default branch moved; going round");
    }
}

/// A regenerator holding its lock: the land machinery it runs its generators and lands its commit
/// with, for no task.
struct Regenerator<'a, 'b> {
    landing: &'b mut Landing<'a>,
    common: &'b Path,
    /// Its round's worktree, for the check ([`Self::check`]): none until the round has one.
    worktree: Option<PathBuf>,
}

impl Regenerator<'_, '_> {
    fn say(&mut self, line: &str) {
        self.landing.say(&format!("{NAME}: {line}"));
    }

    /// The round's `state` kept, the commit main's outputs were last current at as `current` says,
    /// else as the state before it said.
    fn state(
        &self,
        state: &str,
        tip: &str,
        landed: Option<&str>,
        current: Option<&str>,
        line: &str,
    ) {
        let current = current
            .map(str::to_string)
            .or_else(|| State::read(&self.landing.folder).and_then(|before| before.current));
        let _ = write_whole(
            &self.landing.folder,
            STATE,
            &State::new(state, tip, landed, current, line),
        );
    }

    /// One round, on the default branch's tip as the main checkout has it now.
    fn round(&mut self) -> Round {
        let stopped = |tip: &str, line: String| Round::Stopped {
            tip: tip.into(),
            line,
        };
        let Ok(tip) = self.landing.base() else {
            let line = format!(
                "no branch {} in {}",
                self.landing.settings.default_branch,
                self.landing.main.display()
            );
            self.state("stopped", "", None, None, &line);
            return stopped("", line);
        };
        let round = self.make(&tip);
        // A round found current on a tip the default branch moved to keeps that tip.
        let (state, took, landed, current, line) = match &round {
            Round::Current { tip, line } => ("current", tip, None, Some(tip.as_str()), line),
            Round::Landed { commit, line, .. } => (
                "current",
                &tip,
                Some(commit.as_str()),
                Some(commit.as_str()),
                line,
            ),
            Round::Pending { tip, line } => ("pending", tip, None, None, line),
            Round::Dropped { line, .. } => ("dropped", &tip, None, None, line),
            Round::Red { line, .. } => ("red", &tip, None, None, line),
            Round::Stopped { line, .. } => ("stopped", &tip, None, None, line),
        };
        self.state(state, took, landed, current, line);
        self.say(line);
        round
    }

    /// Make the generators' outputs of `tip` and land them.
    fn make(&mut self, tip: &str) -> Round {
        let stopped = |line: String| Round::Stopped {
            tip: tip.into(),
            line,
        };
        if let Err(line) = self.landing.leftover_group() {
            return stopped(line);
        }
        self.state(
            "building",
            tip,
            None,
            None,
            &format!("making {}", short(tip)),
        );
        let path = match worktree(
            &self.landing.main,
            &self.landing.settings,
            &self.landing.folder,
            NAME,
            "regenerate-worktree",
        )
        .and_then(|path| self.checkout(&path, tip).map(|()| path))
        {
            Ok(path) => path,
            Err(line) => return stopped(line),
        };
        self.worktree = Some(path.clone());
        if !self.landing.delivery.is_empty() {
            self.retract();
        }
        if !self.landing.delivery.is_empty()
            && let Some(pending) = Pending::read(&self.landing.folder)
        {
            match self.waiting(&pending, tip, &path) {
                Ok(Some(round)) => return round,
                Ok(None) => {}
                Err(line) => return stopped(line),
            }
        }
        let default = self.landing.settings.default_branch.clone();
        let generators = self.landing.settings.generators.clone();
        let mut passed = Passed::read(&self.landing.folder);
        let mut wrote = false;
        let mut ran = Vec::new();
        for generator in &generators {
            if generator.when_changed && !wrote {
                continue;
            }
            let outputs = outputs_at(&path, "HEAD", &generator.outputs);
            let key = match self.key(generator, &path, MAKING) {
                Ok(key) => key,
                Err(line) => return stopped(line),
            };
            if let Some(key) = &key {
                let pair = Pair {
                    generator: generator.name.clone(),
                    key: key.clone(),
                    outputs: outputs.clone(),
                };
                if passed.holds(&pair) {
                    self.say(&format!(
                        "{} passed before on what {default} at {} holds (its key {key}): nothing \
                         to make",
                        generator.name,
                        short(tip)
                    ));
                    continue;
                }
            }
            self.say(&format!(
                "making {} for {default} at {} ({})",
                generator.name,
                short(tip),
                generator.command.join(" ")
            ));
            match self.landing.logged(&generator.command, &path) {
                Ok(Ran::Passed) => {}
                Ok(Ran::Interrupted(signal)) => {
                    return stopped(format!(
                        "interrupted by {} while it made {}: nothing landed",
                        signal_name(signal),
                        generator.name
                    ));
                }
                Ok(done) => {
                    return Round::Red {
                        tip: tip.into(),
                        line: format!(
                            "the generator {} {} on {default} at {}; its output is in {}",
                            generator.name,
                            self.landing.failed(done),
                            short(tip),
                            self.landing.log.display()
                        ),
                    };
                }
                Err(why) => return stopped(why),
            }
            match changed(&path) {
                Ok(made) => wrote |= made.iter().any(|name| owns(&generator.outputs, name)),
                Err(why) => return stopped(why),
            }
            ran.push((generator, key));
        }
        let made = match changed(&path) {
            Ok(made) => made,
            Err(why) => return stopped(why),
        };
        if let Some(stray) = made
            .iter()
            .find(|name| !generators.iter().any(|g| owns(&g.outputs, name)))
        {
            return Round::Red {
                tip: tip.into(),
                line: format!(
                    "the generators changed {stray}, outside their outputs, on {default} at {}",
                    short(tip)
                ),
            };
        }
        let made_by: Vec<&Generator> = generators
            .iter()
            .filter(|g| made.iter().any(|name| owns(&g.outputs, name)))
            .collect();
        let message = format!(
            "Regenerate {} for {default} at {}",
            and(&made_by.iter().map(|g| g.name.as_str()).collect::<Vec<_>>()),
            short(tip)
        );
        let built = if made.is_empty() {
            tip.to_string()
        } else {
            let committed = on_paths(&path, &["add", "-A"], &made)
                .and_then(|()| {
                    run_git(&path, &["commit", "--quiet", "--no-verify", "-m", &message])
                })
                .and_then(|_| rev(&path, "HEAD").ok_or_else(|| "the commit left no HEAD".into()));
            match committed {
                Ok(built) => built,
                Err(why) => return stopped(why),
            }
        };
        // A pair is kept only once its generator passed, on the key it printed before and prints
        // still: what a later tip of that key needs is what this one made.
        for (generator, key) in &ran {
            let Some(key) = key else { continue };
            if self.key(generator, &path, MAKING).ok().flatten().as_deref() == Some(key.as_str()) {
                passed.add(Pair {
                    generator: generator.name.clone(),
                    key: key.clone(),
                    outputs: outputs_at(&path, "HEAD", &generator.outputs),
                });
            }
        }
        if let Err(why) = passed.write(&self.landing.folder) {
            self.say(&format!("its passed keys were not kept: {why}"));
        }
        if made.is_empty() {
            return Round::Current {
                tip: tip.into(),
                line: format!(
                    "{}: current on {default} at {}",
                    names(&generators),
                    short(tip)
                ),
            };
        }
        // Each generator whose outputs the commit holds, with the key it printed on `tip`.
        let made_by: Vec<(Generator, Option<String>)> = made_by
            .into_iter()
            .map(|generator| {
                let key = ran
                    .iter()
                    .find(|(ran, _)| ran.name == generator.name)
                    .and_then(|(_, key)| key.clone());
                (generator.clone(), key)
            })
            .collect();
        if self.landing.delivery.is_empty() {
            self.land(tip, &built, &made_by, &path, &message)
        } else {
            (self.propose(tip, &built, &made_by, &message)).unwrap_or_else(stopped)
        }
    }

    /// What `pending`, made by a round before, comes to on `tip`, in the worktree at `path`: none
    /// once it is no longer in the way of making `tip` (delivered and held here, or stale and
    /// withdrawn); else the round, which makes nothing.
    ///
    /// - Delivered, by its receipt, and `tip` holds the commit the host made: forgotten. The
    ///   round goes on, and makes again only what the host's commit does not hold.
    /// - Its inputs are the tip's ([`Self::stale`]): one generation for one input key. Its branch
    ///   and task are made where a run cut short left them unmade, and it waits.
    /// - Its inputs changed: its task is cancelled, saying why, a pull request it is bound to is
    ///   told its evidence no longer stands, and the newest tip is made; with no task on the board,
    ///   its branch goes with the record. Not once its merge was asked for: the host may hold its
    ///   commit, which the land settles.
    fn waiting(
        &mut self,
        pending: &Pending,
        tip: &str,
        path: &Path,
    ) -> Result<Option<Round>, String> {
        let slug = &pending.slug;
        let default = self.landing.settings.default_branch.clone();
        let folder = self.landing.folder.clone();
        let waits = |line: String| {
            Ok(Some(Round::Pending {
                tip: tip.into(),
                line,
            }))
        };
        let made = format!(
            "{}: made of {default} at {}",
            names(&self.landing.settings.generators),
            short(&pending.tip)
        );
        let receipt = Receipt::read(&folder, slug).filter(|receipt| receipt.head == pending.head);
        if let Some(receipt) = receipt {
            if ancestor(&self.landing.main, &receipt.squash, tip) {
                // The host merged it onto the base it had then, which no check here pins: where
                // that base held other inputs, what the default branch holds of them is stale.
                let stale = match pending.made_by(&self.landing.settings) {
                    Some(made_by) => self.stale(&pending.tip, &receipt.parent, &made_by, path)?,
                    None => None,
                };
                self.checkout(path, tip)?;
                let delivered = format!("{slug} was delivered as {}", short(&receipt.squash));
                self.say(&match stale {
                    None => delivered,
                    Some(why) => format!(
                        "{delivered}, onto {} where {why}: what {default} holds of them is \
                         stale, and is made again of the newest tip",
                        short(&receipt.parent)
                    ),
                });
                let _ = fs::remove_file(folder.join(PENDING));
                return Ok(None);
            }
            return waits(format!(
                "{made}, delivered as {slug}, which {default} here does not hold yet: the land \
                 of {slug} run again brings it"
            ));
        }
        let status = self.status(slug)?;
        let asked = Journal::read(&folder, slug)
            .is_some_and(|journal| journal.stage >= Stage::MergeRequested);
        let why = if asked {
            None
        } else {
            match pending.made_by(&self.landing.settings) {
                Some(made_by) => self.stale(&pending.tip, tip, &made_by, path)?,
                None => Some("the project's generators are not the ones that made it".into()),
            }
        };
        let Some(why) = why else {
            if status.as_deref() == Some("cancelled") {
                return waits(format!(
                    "{made}, and {slug}, the task delivering it, was cancelled: nothing is made \
                     again until what the generators are made of changes"
                ));
            }
            self.file(pending)?;
            return waits(format!(
                "{made}, awaiting delivery by pull request as {slug} (`just land {slug}`, once \
                 its stamps stand): nothing is made again of the same inputs"
            ));
        };
        self.say(&format!(
            "{slug}, made of {default} at {}, is stale: {default} is at {} and {why}",
            short(&pending.tip),
            short(tip)
        ));
        if status
            .as_deref()
            .is_some_and(|status| status != "cancelled")
        {
            let store = self.landing.store;
            // Named before it is cancelled: a pull request it is bound to is told, by this round
            // or a later one, however this one ends.
            let mut withdrawn = withdrawn(&folder);
            if !withdrawn.contains(slug) {
                withdrawn.push(slug.clone());
                write_lines(&folder, WITHDRAWN, &withdrawn.join("\n"))?;
            }
            let filer = filer(store)?;
            let cancelled = {
                let _ = store.take_role(slug, &filer, "reviewer");
                store.move_with(
                    slug,
                    "cancelled",
                    &filer,
                    Some(&format!(
                        "what it delivers is stale: {default} is at {} and {why}; the regenerator \
                         makes the newest tip",
                        short(tip)
                    )),
                )
            };
            let _ = store.leave(&filer);
            cancelled.map_err(|error| format!("{slug} could not be cancelled: {error}"))?;
        } else if status.is_none() {
            // A task the board never took: nothing but the record names its branch, which goes
            // first, and only where it is still at the commit made.
            let branch = format!("refs/heads/{slug}");
            let _ = run_git(
                &self.landing.main,
                &["update-ref", "-d", &branch, &pending.head],
            );
        }
        let _ = fs::remove_file(folder.join(PENDING));
        self.retract();
        Ok(None)
    }

    /// The status of the task `slug`, or none where the board has none.
    fn status(&self, slug: &str) -> Result<Option<String>, String> {
        match self.landing.store.get(slug) {
            Ok(task) => Ok(Some(task.status)),
            Err(error) if error.is_no_task() => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    /// Tell the pull request of each task named in [`WITHDRAWN`] that its evidence no longer
    /// stands ([`Landing::retract`]): one told, held or bound to none is forgotten, and one that
    /// could not be told is said and tried again by the next round. Until then its checks may
    /// still pass, which a cancelled task alone never changes.
    fn retract(&mut self) {
        let folder = self.landing.folder.clone();
        let named = withdrawn(&folder);
        if named.is_empty() {
            return;
        }
        let mut left = Vec::new();
        for slug in named {
            if let Err(why) = self.landing.retract(&slug) {
                self.say(&format!(
                    "the pull request of {slug}, which was cancelled, was not told its evidence \
                     no longer stands ({why}): its checks may still pass; the next round tells it"
                ));
                left.push(slug);
            }
        }
        let kept = if left.is_empty() {
            fs::remove_file(folder.join(WITHDRAWN)).map_err(|e| e.to_string())
        } else {
            write_lines(&folder, WITHDRAWN, &left.join("\n"))
        };
        if let Err(why) = kept {
            self.say(&format!("{WITHDRAWN} was not kept: {why}"));
        }
    }

    /// The branch and the task `pending` names, each made only where it is not there: the branch
    /// at the commit made, the task filed as the regenerator's reds are and moved to review, where
    /// its assessor reads the head. It names no stamp and waives none.
    fn file(&mut self, pending: &Pending) -> Result<(), String> {
        let slug = &pending.slug;
        let default = self.landing.settings.default_branch.clone();
        let branch = format!("refs/heads/{slug}");
        match rev(&self.landing.main, &branch) {
            Some(at) if at == pending.head => {}
            Some(at) => {
                return Err(format!(
                    "the branch {slug} is at {}, not the commit the regenerator made ({}): move \
                     it away, and the next round files it",
                    short(&at),
                    short(&pending.head)
                ));
            }
            None => {
                run_git(&self.landing.main, &["update-ref", &branch, &pending.head])?;
            }
        }
        let status = self.status(slug)?;
        let store = self.landing.store;
        if status.as_deref() == Some("in_progress") {
            // Claimed by a run cut short before it said the work is ready: said for it, as the
            // session that filed and claimed it, which then leaves. Anybody else's is theirs.
            let task = store.get(slug).map_err(|error| error.to_string())?;
            let by = (task.holders("executor").first().copied())
                .filter(|executor| *executor == task.created_by);
            if let Some(by) = by {
                store
                    .move_to(slug, "review", by)
                    .map_err(|error| format!("the board did not take {slug}: {error}"))?;
                let _ = store.leave(by);
            }
            return Ok(());
        }
        if !matches!(status.as_deref(), None | Some("ready")) {
            return Ok(());
        }
        let filer = filer(store)?;
        let filed = (|| {
            if status.is_none() {
                let generators = and(&(pending.made.iter())
                    .map(|made| made.generator.as_str())
                    .collect::<Vec<_>>());
                store.create(&NewTask {
                    slug: slug.clone(),
                    title: format!(
                        "Regenerate {generators} for {default} at {}",
                        short(&pending.tip)
                    ),
                    by: filer.clone(),
                    body: format!(
                        "What the project's `[land] generators` make of {default} at {}: the \
                         commit {}, their outputs alone. It lands by `just land {slug}` once \
                         assessed, or the regenerator cancels it for a newer tip.",
                        pending.tip, pending.head
                    ),
                    priority: 1,
                    ..NewTask::default()
                })?;
                // As a red filed here: an orchestrator not given now, the board's next read gives.
                let _ = store.give_filed(slug, &filer, None);
            }
            // Its executor is the regenerator, which made it: claimed, then ready to be read.
            store.claim(slug, &filer)?;
            store.move_to(slug, "review", &filer)
        })();
        let _ = store.leave(&filer);
        filed.map(drop).map_err(|error| {
            format!("the board did not take {slug}: {error}; the next round files it")
        })
    }

    /// Where the project delivers by pull request: `built`, made on `tip` by the generators
    /// `made_by` as `message`, kept as [`Pending`] before anything else, then its branch and its
    /// task ([`Self::file`]). Nothing is committed to the default branch and nothing pushed.
    fn propose(
        &mut self,
        tip: &str,
        built: &str,
        made_by: &[(Generator, Option<String>)],
        message: &str,
    ) -> Result<Round, String> {
        // Named by the tip and by the commit made of it, so no two generations share a name.
        let slug = format!(
            "generated/outputs-at-{}-{}",
            &tip[..tip.len().min(12)],
            short(built)
        );
        let pending = Pending {
            slug,
            tip: tip.into(),
            head: built.into(),
            made: (made_by.iter())
                .map(|(generator, key)| Made {
                    generator: generator.name.clone(),
                    key: key.clone(),
                })
                .collect(),
        };
        pending.write(&self.landing.folder)?;
        self.file(&pending)?;
        Ok(Round::Pending {
            tip: tip.into(),
            line: format!(
                "{message}: made as {}, awaiting delivery by pull request as {} (`just land {}`, \
                 once its stamps stand); nothing committed to {}",
                short(built),
                pending.slug,
                pending.slug,
                self.landing.settings.default_branch
            ),
        })
    }

    /// The regenerator's worktree at `path`, made when missing, checked out at `tip` with nothing
    /// a round before left in it but what git ignores (its builds' output). One in its place that
    /// is not the regenerator's is never touched.
    fn checkout(&self, path: &Path, tip: &str) -> Result<(), String> {
        let main = &self.landing.main;
        let shown = path.to_string_lossy().into_owned();
        let listed = worktrees(main)?;
        match listed.iter().find(|entry| same(&entry.path, path)) {
            Some(entry)
                if entry
                    .locked
                    .as_deref()
                    .is_some_and(|reason| reason.starts_with(REASON)) =>
            {
                if path.exists() {
                    run_git(path, &["reset", "--quiet", "--hard"])?;
                    run_git(path, &["checkout", "--quiet", "--force", "--detach", tip])?;
                    run_git(path, &["clean", "--quiet", "-f", "-d"])?;
                    return Ok(());
                }
                clear(main, path);
            }
            Some(_) => {
                return Err(format!(
                    "{shown} is another worktree, where the regenerator makes its own: move it, \
                     or name another [agents] worktrees folder"
                ));
            }
            None if path.exists() => {
                return Err(format!(
                    "{shown} is in the way of the regenerator's worktree: move it, or name another \
                     [agents] worktrees folder"
                ));
            }
            None => {}
        }
        run_git(
            main,
            &[
                "worktree", "add", "--quiet", "--detach", "--lock", "--reason", REASON, &shown, tip,
            ],
        )?;
        Ok(())
    }

    /// The key `generator` prints in `dir` (its command in `[land] keys`, [`Settings::keys`]), its
    /// last line, or none when it has no key, or its key failed or printed nothing (said, with what
    /// follows, `then`); the line saying it took a signal.
    fn key(
        &mut self,
        generator: &Generator,
        dir: &Path,
        then: &str,
    ) -> Result<Option<String>, String> {
        let Some(key) = self.landing.settings.keys.get(&generator.name).cloned() else {
            return Ok(None);
        };
        let read = self.landing.logged_reading(&key, dir, true);
        let why = match read {
            Ok((Ran::Passed, printed)) => {
                match printed.lines().map(str::trim).rfind(|l| !l.is_empty()) {
                    Some(key) => return Ok(Some(key.to_string())),
                    None => "printed nothing".to_string(),
                }
            }
            Ok((Ran::Interrupted(signal), _)) => {
                return Err(format!(
                    "interrupted by {} while it read {}'s key: nothing landed",
                    signal_name(signal),
                    generator.name
                ));
            }
            Ok((ran, _)) => self.landing.failed(ran),
            Err(why) => why,
        };
        self.say(&format!(
            "{}'s key ({}) {why}: {then}",
            generator.name,
            key.join(" ")
        ));
        Ok(None)
    }

    /// Land `built`, made on `tip` in the regenerator's worktree at `path` by the generators
    /// `made_by` (each with the key it printed on `tip`), as `message`: on the default branch's tip
    /// as it is then, only when what they are made of there is what it was at `tip`
    /// ([`Self::stale`]), else discarded as stale and said, nothing committed; the commit landed
    /// under the land's lock ([`Self::land_on`]), then the push. A build is never cut short or
    /// begun again because the default branch moved: this is where it learns that it did.
    fn land(
        &mut self,
        tip: &str,
        built: &str,
        made_by: &[(Generator, Option<String>)],
        path: &Path,
        message: &str,
    ) -> Round {
        let stopped = |line: String| Round::Stopped {
            tip: tip.into(),
            line,
        };
        let default = self.landing.settings.default_branch.clone();
        let outputs: Vec<String> = made_by.iter().map(|(g, _)| g.outputs.clone()).collect();
        let named = and(&outputs.iter().map(String::as_str).collect::<Vec<_>>());
        let not_landed = |line: String| {
            stopped(format!(
                "{named} made on {default} at {} did not land: {line}",
                short(tip)
            ))
        };
        for _ in 0..TRIES {
            let base = match self.landing.base() {
                Ok(base) => base,
                Err(line) => return stopped(line),
            };
            if base != tip {
                match self.stale(tip, &base, made_by, path) {
                    Ok(None) => {}
                    Ok(Some(why)) => {
                        return Round::Dropped {
                            tip: tip.into(),
                            line: format!(
                                "{default} moved to {} since {}, and {why}: what was made of {} is \
                                 stale, discarded and not committed; the newest tip is made next",
                                short(&base),
                                short(tip),
                                short(tip)
                            ),
                        };
                    }
                    Err(line) => return stopped(line),
                }
            }
            match self.land_on(tip, &base, built, &outputs, message) {
                Ok(Some(round)) => return round,
                // The default branch moved again since it was judged: judged again.
                Ok(None) => continue,
                Err(line) => return not_landed(line),
            }
        }
        not_landed(format!("{default} moved {TRIES} times while it landed"))
    }

    /// Why what the generators `made_by` made of `tip` is not theirs to land on `base`, the
    /// default branch's tip now: none when what they are made of is the same on both. A path
    /// inside the generators' outputs is theirs to make; any other the default branch changed may
    /// be one of their inputs, unless each one's key, read again on `base` in its worktree at
    /// `path`, is the one it printed on `tip`. A generator with no key reads every such path.
    fn stale(
        &mut self,
        tip: &str,
        base: &str,
        made_by: &[(Generator, Option<String>)],
        path: &Path,
    ) -> Result<Option<String>, String> {
        let generators = self.landing.settings.generators.clone();
        let changed = run_git(
            &self.landing.main,
            &["diff", "--name-only", "--no-renames", tip, base],
        )?;
        let Some(first) = changed
            .lines()
            .find(|name| !generators.iter().any(|g| owns(&g.outputs, name)))
        else {
            return Ok(None);
        };
        let mut there = false;
        for (generator, key) in made_by {
            let Some(key) = key else {
                return Ok(Some(format!(
                    "changed {first}, which {} may read (it has no key)",
                    generator.name
                )));
            };
            if !there {
                self.checkout(path, base)?;
                there = true;
            }
            match self.key(generator, path, STALE)? {
                Some(now) if now == *key => {}
                Some(now) => {
                    return Ok(Some(format!(
                        "{}'s key there is {now}, not {key}",
                        generator.name
                    )));
                }
                None => {
                    return Ok(Some(format!(
                        "{}'s key could not be read there",
                        generator.name
                    )));
                }
            }
        }
        Ok(None)
    }

    /// `built`, made on `tip`, landed on the default branch at `base` under the land's lock: as
    /// it is when `base` is `tip`, else `base`'s tree with `outputs` as `built` has them; then the
    /// fast-forward of the main checkout and the push, as a land makes them. None when the default
    /// branch is no longer at `base`, for [`Self::land`] to judge its newer tip.
    fn land_on(
        &mut self,
        tip: &str,
        base: &str,
        built: &str,
        outputs: &[String],
        message: &str,
    ) -> Result<Option<Round>, String> {
        let held = {
            let landing = &mut self.landing;
            hold(self.common, NAME, landing.interrupts, &mut *landing.out)
        }?;
        let running = std::mem::replace(
            &mut self.landing.running,
            self.landing.folder.join(super::RUNNING),
        );
        let landed = self.land_held(tip, base, built, outputs, message);
        self.landing.running = running;
        drop(held);
        landed
    }

    fn land_held(
        &mut self,
        tip: &str,
        base: &str,
        built: &str,
        outputs: &[String],
        message: &str,
    ) -> Result<Option<Round>, String> {
        let landing = &mut self.landing;
        landing.leftover_group()?;
        landing.recover()?;
        let default = landing.settings.default_branch.clone();
        landing.go_on(None)?;
        landing.clean()?;
        if landing.base()? != base {
            return Ok(None);
        }
        let commit = if base == tip {
            built.to_string()
        } else {
            replaced(
                &landing.main,
                &landing.folder,
                base,
                built,
                outputs,
                message,
            )?
        };
        if rev(&landing.main, &format!("{commit}^{{tree}}"))
            == rev(&landing.main, &format!("{base}^{{tree}}"))
        {
            return Ok(Some(Round::Current {
                tip: base.to_string(),
                line: format!(
                    "{}: current on {default} at {}",
                    names(&landing.settings.generators),
                    short(base)
                ),
            }));
        }
        match landing.fast_forward(base, &commit, false)? {
            Move::Done => {}
            Move::Raced(_) => return Ok(None),
            Move::Not { why, restored } => {
                let mut line = format!(
                    "{default} could not move to {} in the main checkout: {why}",
                    short(&commit)
                );
                match restored {
                    Ok(restored) => {
                        if let Some(said) = restored.said(&default, base) {
                            line.push_str(&format!("; {said}"));
                        }
                    }
                    Err(why) => line.push_str(&format!(
                        "; what the move half-wrote in the main checkout could not be restored \
                         ({why}): the next land tries again"
                    )),
                }
                return Err(line);
            }
        }
        let pushed = match landing.push(&commit) {
            super::Pushed::Done => format!("; pushed to {}", super::REMOTE),
            super::Pushed::Off => "; not pushed: [land] push is off".into(),
            super::Pushed::NoRemote => format!("; not pushed: no {} remote", super::REMOTE),
            super::Pushed::Failed(why, retry) => {
                format!("; NOT PUSHED, landed here only: {why}. Push it with: {retry}")
            }
        };
        Ok(Some(Round::Landed {
            tip: tip.into(),
            commit: commit.clone(),
            line: format!(
                "{message}: landed on {default} at {}{pushed}",
                &commit[..commit.len().min(12)]
            ),
        }))
    }

    /// The red task, filed, added to or closed as `round` says: filed or added to when red, closed
    /// when it made the outputs of the tip it took.
    fn settle(&mut self, store: &Store, round: &Round) {
        let said = match round {
            Round::Red { tip, line } => file_red(
                store,
                &GENERATORS,
                &GENERATOR_FAILED,
                tip,
                line,
                &self.landing.log,
            )
            .map(|(_, said)| Some(said)),
            Round::Current { tip, .. } | Round::Landed { tip, .. } | Round::Pending { tip, .. } => {
                clear_red(store, &GENERATORS, tip)
            }
            Round::Dropped { .. } | Round::Stopped { .. } => Ok(None),
        };
        match said {
            Ok(Some(line)) => self.say(&line),
            Ok(None) => {}
            Err(why) => self.say(&format!("the board did not take its red: {why}")),
        }
    }

    /// The check no land runs ([`Settings::main_check`]), once `round` has landed what it made, so
    /// it never holds back the generators' commit: in its worktree, within the land's timeout, on
    /// what the round left the default branch holding. That is the commit it landed, which holds
    /// whatever the branch gained during the build beside the outputs, so a land then is checked
    /// too; else the tip it took (a red round's as committed). Kept in [`CHECKED`] while it runs and
    /// once it ends, said, and its red filed, added to or closed ([`CHECK`]), the task saying no
    /// more than is known of one past its time or one that could not run. A round discarded as
    /// stale, or stopped, is not checked: the round after it checks the newer tip. Nor is a commit
    /// the same check passed on last, which a pending round takes again at each start while its
    /// task waits. None but when it took a signal: the line the regenerator stops with, nothing
    /// judged.
    fn check(&mut self, store: &Store, round: &Round) -> Option<String> {
        let command = self.landing.settings.main_check.clone();
        let path = self.worktree.take()?;
        let commit = match round {
            Round::Landed { commit, .. } => commit.clone(),
            Round::Current { tip, .. } | Round::Red { tip, .. } | Round::Pending { tip, .. } => {
                tip.clone()
            }
            Round::Dropped { .. } | Round::Stopped { .. } => return None,
        };
        if command.is_empty() {
            return None;
        }
        let default = self.landing.settings.default_branch.clone();
        let shown = command.join(" ");
        let at = short(&commit);
        let passed = format!("the check `{shown}` passed on {default} at {at}");
        // A pending round takes the same tip at each start while its task waits: what passed on
        // that commit is not run again.
        if Checked::read(&self.landing.folder).is_some_and(|before| {
            before.state == "passed" && before.tip == commit && before.line == passed
        }) {
            self.say(&format!("{passed} before: not run again"));
            return None;
        }
        let checking = format!("checking {default} at {at} ({shown})");
        self.keep_check(&Checked::new("checking", &commit, None, &checking));
        let ran = match self.checkout(&path, &commit) {
            Ok(()) => {
                self.say(&checking);
                self.landing.logged(&command, &path)
            }
            Err(why) => Err(why),
        };
        let (filed, line) = match ran {
            Ok(Ran::Passed) => (None, passed),
            Ok(Ran::Interrupted(signal)) => {
                return Some(format!(
                    "interrupted by {} while it checked {default} at {at}: what the round made \
                     stands, the check not judged",
                    signal_name(signal)
                ));
            }
            Ok(ran) => (
                Some(if ran == Ran::Late {
                    &CHECK_LATE
                } else {
                    &CHECK_FAILED
                }),
                format!(
                    "the check `{shown}` {} on {default} at {at}; its output is in {}",
                    self.landing.failed(ran),
                    self.landing.log.display()
                ),
            ),
            // A check that cannot run cannot pass: filed, so it is seen, as no more than that.
            Err(why) => (
                Some(&CHECK_UNRUN),
                format!("the check `{shown}` could not run on {default} at {at}: {why}"),
            ),
        };
        self.say(&line);
        let (task, said) = match filed {
            None => (None, clear_red(store, &CHECK, &commit)),
            Some(filed) => {
                match file_red(store, &CHECK, filed, &commit, &line, &self.landing.log) {
                    Ok((slug, said)) => (Some(slug), Ok(Some(said))),
                    Err(why) => (None, Err(why)),
                }
            }
        };
        match said {
            Ok(Some(said)) => self.say(&said),
            Ok(None) => {}
            Err(why) => self.say(&format!("the board did not take the check's red: {why}")),
        }
        let state = if filed.is_none() { "passed" } else { "red" };
        self.keep_check(&Checked::new(state, &commit, task, &line));
        None
    }

    /// `checked` kept in [`CHECKED`], or said when it cannot be.
    fn keep_check(&mut self, checked: &Checked) {
        if let Err(why) = write_whole(&self.landing.folder, CHECKED, checked) {
            self.say(&format!("what the check came to was not kept: {why}"));
        }
    }
}

/// The tasks named in [`WITHDRAWN`] in the lands' `folder`, a line each.
fn withdrawn(folder: &Path) -> Vec<String> {
    let text = fs::read_to_string(folder.join(WITHDRAWN)).unwrap_or_default();
    text.lines().map(str::to_string).collect()
}

/// `text` as `folder`'s `name`, whole or not at all.
fn write_lines(folder: &Path, name: &str, text: &str) -> Result<(), String> {
    let path = folder.join(name);
    let staged = folder.join(format!(".{name}.{}", std::process::id()));
    fs::write(&staged, text).map_err(|e| at(&staged, &e))?;
    fs::rename(&staged, &path).map_err(|e| at(&path, &e))
}

/// A session of the kind that files the regenerator's tasks ([`red::FILER`]): its handle.
fn filer(store: &Store) -> Result<String, String> {
    store
        .register(&NewSession::new(red::FILER))
        .map(|session| session.row.handle)
        .map_err(|error| error.to_string())
}

impl Landing<'_> {
    /// What the regenerator made that this task delivers, if it delivers that.
    fn pending(&self) -> Option<Pending> {
        Pending::read(&self.folder).filter(|pending| pending.slug == self.slug)
    }

    /// The regenerator started, as a land's end starts it, by a land of a task the host delivered
    /// already (its receipt, of the head `tip` its branch is at) that is run again: the retry of
    /// a start a run cut short never made, with no later land to make it. A start makes nothing
    /// twice. What the land answers is as it was.
    pub(super) fn delivered_before(&mut self) {
        let slug = self.slug;
        let tip = rev(&self.main, &format!("refs/heads/{slug}"));
        let delivered = Receipt::read(&self.folder, slug)
            .is_some_and(|receipt| Some(&receipt.head) == tip.as_ref());
        if delivered && let Some(said) = start(self.env, &self.main, &self.settings) {
            self.say(&format!("land {slug}: delivered already; {said}"));
        }
    }

    /// Whether the land of this task, whose branch is at `tip`, delivers what the regenerator
    /// made ([`Pending`]) to the fetched remote default branch at `base`: none when the task is
    /// no such delivery. The operation is the regenerator's record and the graph's to say, never
    /// a title's: the head is the commit it made, of one parent the remote default holds,
    /// changing generators' outputs alone, and what the generators are made of at `base` is what
    /// it was at that parent, each key read again there under the regenerator's lock
    /// ([`Regenerator::stale`]). A stale one is refused before anything is published, and the
    /// regenerator started, which cancels it and makes the newest tip.
    pub(super) fn generated(&mut self, base: &str, tip: &str) -> Option<Result<(), Refusal>> {
        let slug = self.slug;
        let pending = self.pending()?;
        let default = self.settings.default_branch.clone();
        Some((|| {
            let here = |line: String| Refusal::here(line);
            let parents =
                run_git(&self.main, &["rev-list", "--parents", "-n", "1", tip]).map_err(here)?;
            let parents: Vec<&str> = parents.split(' ').skip(1).collect();
            let changed = run_git(
                &self.main,
                &["diff", "--name-only", "--no-renames", &pending.tip, tip],
            )
            .map_err(here)?;
            let stray = (changed.lines()).find(|name| {
                !self
                    .settings
                    .generators
                    .iter()
                    .any(|g| owns(&g.outputs, name))
            });
            let wrong = if pending.head != tip {
                Some(format!(
                    "is not the commit the regenerator made ({})",
                    short(&pending.head)
                ))
            } else if parents != [pending.tip.as_str()] || !ancestor(&self.main, &pending.tip, base)
            {
                Some(format!(
                    "is not one commit on {}, which {}/{default} holds",
                    short(&pending.tip),
                    super::REMOTE
                ))
            } else {
                stray.map(|stray| format!("changes {stray}, outside the generators' outputs"))
            };
            if let Some(wrong) = wrong {
                return Err(Refusal::to(
                    format!("{slug}'s head {} {wrong}", short(tip)),
                    "@orchestrator",
                    "A generated update is delivered as the regenerator made it, or not at all: \
                     nothing was published"
                        .into(),
                ));
            }
            let common = crate::git::common_dir(&self.main).map_err(|e| here(e.to_string()))?;
            let Some(held) = try_hold(&self.folder).map_err(here)? else {
                return Err(here(format!(
                    "the regenerator runs, and judges what {slug} was made of: nothing published; \
                     run the land again once it ends (`just tasks regenerate --status`)"
                )));
            };
            let stale = match pending.made_by(&self.settings) {
                None => Ok(Some(
                    "the project's generators are not the ones that made it".to_string(),
                )),
                Some(made_by) => worktree(
                    &self.main,
                    &self.settings,
                    &self.folder,
                    NAME,
                    "regenerate-worktree",
                )
                .and_then(|path| {
                    let mut regenerator = Regenerator {
                        landing: &mut *self,
                        common: &common,
                        worktree: None,
                    };
                    regenerator.stale(&pending.tip, base, &made_by, &path)
                }),
            };
            drop(held);
            let Some(why) = stale.map_err(here)? else {
                return Ok(());
            };
            let started = super::regenerate::start(self.env, &self.main, &self.settings)
                .map_or_else(String::new, |said| format!("; {said}"));
            Err(Refusal::to(
                format!(
                    "{slug} is stale: it was made of {default} at {}, {}/{default} is at {} and \
                     {why}; nothing published{started}",
                    short(&pending.tip),
                    super::REMOTE,
                    short(base)
                ),
                "@orchestrator",
                "The regenerator cancels it and makes the newest tip, which is delivered as a \
                 task of its own"
                    .into(),
            ))
        })())
    }
}

/// What the commit or tree `commit` of the repository at `dir` holds at `output` (a generator's
/// outputs): its object, or `none`.
fn outputs_at(dir: &Path, commit: &str, output: &str) -> String {
    rev(dir, &format!("{commit}:{}", output.trim_end_matches('/'))).unwrap_or_else(|| "none".into())
}

/// A commit on `base` of `main`'s repository, as `message`: `base`'s tree with each of `outputs`
/// as `built` has it (gone where it has none), made through an index of its own beside the lands'
/// `folder`, so neither the main checkout nor its index changes.
fn replaced(
    main: &Path,
    folder: &Path,
    base: &str,
    built: &str,
    outputs: &[String],
    message: &str,
) -> Result<String, String> {
    let index = folder.join(format!(".regenerate-index.{}", std::process::id()));
    let indexed = |args: &[&str]| -> Result<String, String> {
        let mut command = sys::git();
        command
            .args(args)
            .current_dir(main)
            .env("GIT_INDEX_FILE", &index)
            .stdin(Stdio::null());
        let done = sys::output(&mut command).map_err(|e| format!("git {}: {e}", args[0]))?;
        if !done.status.success() {
            return Err(format!(
                "git {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&done.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&done.stdout).trim().to_string())
    };
    let made = (|| {
        indexed(&["read-tree", base])?;
        for output in outputs {
            let output = output.trim_end_matches('/');
            indexed(&[
                "--literal-pathspecs",
                "rm",
                "-r",
                "-q",
                "-f",
                "--cached",
                "--ignore-unmatch",
                "--",
                output,
            ])?;
            let object = format!("{built}:{output}");
            match indexed(&["cat-file", "-t", &object]).as_deref() {
                Ok("tree") => {
                    indexed(&["read-tree", &format!("--prefix={output}/"), &object])?;
                }
                Ok("blob") => {
                    let listed = indexed(&["ls-tree", built, "--", output])?;
                    let mode = listed.split(' ').next().unwrap_or("100644").to_string();
                    let sha = indexed(&["rev-parse", &object])?;
                    indexed(&[
                        "update-index",
                        "--add",
                        "--cacheinfo",
                        &format!("{mode},{sha},{output}"),
                    ])?;
                }
                // Gone where it built none.
                _ => {}
            }
        }
        let tree = indexed(&["write-tree"])?;
        indexed(&["commit-tree", &tree, "-p", base, "-m", message])
    })();
    let _ = fs::remove_file(&index);
    made
}

/// A series of red tasks the regenerator files, one open at a time: the generators', and the
/// check's.
struct Series {
    /// Its first task's slug; once one is closed, the next takes `-2`, then `-3` and on.
    slug: &'static str,
    /// What passed, in the line that closes it.
    passed: &'static str,
}

/// The generators' reds ([`RED`]).
const GENERATORS: Series = Series {
    slug: RED,
    passed: "The regenerator passed",
};

/// The check's reds ([`RED_CHECK`]).
const CHECK: Series = Series {
    slug: RED_CHECK,
    passed: "The check passed",
};

/// What a red task a series files says of its red: no more than is known of it.
struct Filed {
    /// What it is marked with ([`red::MARK`]).
    mark: &'static str,
    title: &'static str,
    /// What ran on the default branch and what it came to, before the line saying how.
    what: &'static str,
    /// What holds while it stays red.
    meanwhile: &'static str,
    /// When it is done, and what closes it.
    done: &'static str,
}

/// A generator that failed or wrote outside its outputs.
const GENERATOR_FAILED: Filed = Filed {
    mark: MARK,
    title: "The regenerator is red on the default branch",
    what: "runs the project's `[land] generators` on the default branch's newest tip, and one \
           failed there",
    meanwhile: "Until a round passes, the committed outputs are behind the sources they are made \
                from.",
    done: "A commit on the default branch makes the generators pass; the next round, which each \
           land starts, closes this task.",
};

/// A check that ran and failed.
const CHECK_FAILED: Filed = Filed {
    mark: "the regenerator: [verdicts] check fails on the default branch",
    title: "The check is red on the default branch",
    what: "runs the project's `[verdicts] check` on what each round leaves the default branch \
           holding, as no land does there (`[land] check = false`), and it failed",
    meanwhile: "Until it passes, whatever runs that check on the default branch is refused, the \
                Symphony's commit of the owner's verdicts among them.",
    done: "A commit on the default branch makes the check pass; the next round, which each land \
           starts, closes this task.",
};

/// A check ended past `[land] timeout_seconds`: it neither passed nor failed.
const CHECK_LATE: Filed = Filed {
    mark: "the regenerator: [verdicts] check runs past its time on the default branch",
    title: "The check runs past its time on the default branch",
    what: "runs the project's `[verdicts] check` on what each round leaves the default branch \
           holding, as no land does there (`[land] check = false`), and it ran past its time and \
           was ended, so it neither passed nor failed",
    meanwhile: "Whatever else runs that check on the default branch may hang on what held it up, \
                the Symphony's commit of the owner's verdicts among them.",
    done: "The check ends within `[land] timeout_seconds` on the default branch and passes; the \
           next round that checks it, which each land starts, closes this task.",
};

/// A check that could not be started or run: it neither passed nor failed.
const CHECK_UNRUN: Filed = Filed {
    mark: "the regenerator: [verdicts] check cannot run on the default branch",
    title: "The check cannot run on the default branch",
    what: "runs the project's `[verdicts] check` on what each round leaves the default branch \
           holding, as no land does there (`[land] check = false`), and could not run it, so it \
           neither passed nor failed",
    meanwhile: "The line above says why: trouble with the command or with the regenerator's \
                worktree, not a judgement of what the default branch holds.",
    done: "The check runs in the regenerator's worktree again and passes; the next round that \
           checks it, which each land starts, closes this task.",
};

/// Add a line to the open red task of `series`, or file one as `filed` says, for `tip`, red as
/// `line` says, its output in `log`: that task's slug, and what it said.
fn file_red(
    store: &Store,
    series: &Series,
    filed: &Filed,
    tip: &str,
    line: &str,
    log: &Path,
) -> Result<(String, String), String> {
    let (open, next) = reds(store, series)?;
    let filer = store
        .register(&NewSession::new(red::FILER))
        .map_err(|e| e.to_string())?
        .row
        .handle;
    let said = match open {
        Some(slug) => store
            .say(
                &slug,
                &filer,
                &format!("Red again on main at {}: {line}", short(tip)),
                None,
            )
            .map(|_| (slug.clone(), format!("red again: a line added to {slug}"))),
        None => store
            .create(&NewTask {
                slug: next.clone(),
                title: filed.title.into(),
                by: filer.clone(),
                body: format!(
                    "**What is red.** The regenerator (`just tasks regenerate`, started after each \
                     land) {}: {line}\n\nIts log is {}; `just tasks regenerate --status` says \
                     where it stands. {}\n\n**Done when.** {}",
                    filed.what,
                    log.display(),
                    filed.meanwhile,
                    filed.done
                ),
                priority: 0,
                red: Some(filed.mark.into()),
                ..NewTask::default()
            })
            .map(|_| {
                // Every open task has an orchestrator; one not given here, the board's next read
                // of the whole board gives ([`Store::give_if_due`]).
                let _ = store.give_filed(&next, &filer, None);
                (next.clone(), format!("red: filed {next}"))
            }),
    };
    let _ = store.leave(&filer);
    said.map_err(|e| e.to_string())
}

/// Close the open red task of `series`, if one is: what it names passed on `tip`.
fn clear_red(store: &Store, series: &Series, tip: &str) -> Result<Option<String>, String> {
    let (Some(open), _) = reds(store, series)? else {
        return Ok(None);
    };
    let filer = store
        .register(&NewSession::new(red::FILER))
        .map_err(|e| e.to_string())?
        .row
        .handle;
    let closed = (|| {
        let _ = store.take_role(&open, &filer, "reviewer");
        store.say(
            &open,
            &filer,
            &format!(
                "{} on main at {}: closing this red",
                series.passed,
                short(tip)
            ),
            None,
        )?;
        store.move_to(&open, "cancelled", &filer)
    })();
    let _ = store.leave(&filer);
    closed
        .map(|_| Some(format!("passed: closed {open}")))
        .map_err(|e| e.to_string())
}

/// The red task of `series` that is open, if one is, and the slug the next one is filed as: its
/// slug, then `-2`, `-3` and on once each is closed.
fn reds(store: &Store, series: &Series) -> Result<(Option<String>, String), String> {
    let mut open = None;
    let mut n = 1;
    loop {
        let slug = if n == 1 {
            series.slug.to_string()
        } else {
            format!("{}-{n}", series.slug)
        };
        match store.get(&slug) {
            Ok(task) => {
                if red::open(&task.status) {
                    open = Some(slug);
                }
            }
            Err(error) if error.is_no_task() => {
                return Ok((open, slug));
            }
            Err(error) => return Err(error.to_string()),
        }
        n += 1;
    }
}

/// Where the regenerator stands ([`status`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Standing {
    /// In words.
    pub line: String,
    /// Whether the generators' outputs on the default branch are what they make of its tip.
    pub current: bool,
    /// Whether the regenerator that last wrote its state still runs: outputs not current may be
    /// soon.
    pub making: bool,
}

/// Where the regenerator stands, from where `env` runs.
pub fn status(env: &Env) -> Result<Standing, String> {
    let common = crate::git::common_dir(&env.cwd).map_err(|e| e.to_string())?;
    let main = trigger::main_checkout(&common)
        .ok_or_else(|| format!("{} has no main checkout", common.display()))?;
    let settings = Settings::of(main)?;
    let standing = |line: String, current: bool, making: bool| Standing {
        line,
        current,
        making,
    };
    if settings.generators.is_empty() {
        return Ok(standing(
            "no [land] generators: nothing to regenerate".into(),
            true,
            false,
        ));
    }
    let what = names(&settings.generators);
    let default = &settings.default_branch;
    let tip = rev(main, &format!("refs/heads/{default}"))
        .ok_or_else(|| format!("no branch {default} in {}", main.display()))?;
    let folder = lands(&common);
    let Some(state) = State::read(&folder) else {
        return Ok(standing(
            format!(
                "{what}: never regenerated here; the next land starts the regenerator, which \
                 makes them of {default} at {}",
                short(&tip)
            ),
            false,
            false,
        ));
    };
    let when = sys::local_minute(state.at);
    let since =
        |from: &str| match run_git(main, &["rev-list", "--count", &format!("{from}..{tip}")])
            .as_deref()
        {
            Ok("1") => "1 commit".to_string(),
            Ok(count) => format!("{count} commits"),
            Err(_) => "an unknown number of commits".into(),
        };
    let making = lives(state.pid, state.start.as_deref());
    // How far behind the outputs the default branch holds are: from the newest commit they were
    // found current at.
    let held = match &state.current {
        Some(current) => format!(
            "{default} holds them as of {}, {} behind",
            short(current),
            since(current)
        ),
        None => format!("{default} holds none found current here yet"),
    };
    let mut said = match state.state.as_str() {
        "building" | "dropped" if making => standing(
            format!(
                "{what}: making them of {default} at {} since {when} (pid {}); {default} is {} past \
                 it; {held}",
                short(&state.tip),
                state.pid,
                since(&state.tip)
            ),
            false,
            true,
        ),
        "building" | "dropped" => standing(
            format!(
                "{what}: the regenerator making them of {default} at {} since {when} ended \
                 without a word; the next land starts another; {held}",
                short(&state.tip)
            ),
            false,
            false,
        ),
        "current" if state.landed.as_deref() == Some(tip.as_str()) || state.tip == tip => standing(
            format!(
                "{what}: current on {default} at {} (judged {when})",
                short(&tip)
            ),
            true,
            false,
        ),
        "current" => {
            let from = state.landed.as_deref().unwrap_or(&state.tip);
            standing(
                format!(
                    "{what}: behind, {default} is {} past {}, which they were last made of \
                     ({when}); {}",
                    since(from),
                    short(from),
                    if making {
                        "the regenerator goes round for it"
                    } else {
                        "the regenerator runs after each land"
                    }
                ),
                false,
                making,
            )
        }
        "pending" => standing(format!("{}; {held}", state.line), false, false),
        "red" => standing(
            format!(
                "{what}: red on {default} at {} since {when}: {} ({RED}); {held}",
                short(&state.tip),
                state.line
            ),
            false,
            making,
        ),
        _ => standing(
            format!(
                "{what}: stopped on {default} at {} ({when}): {}; {held}",
                short(&state.tip),
                state.line
            ),
            false,
            false,
        ),
    };
    said.line.push_str(&checked(&settings, &folder));
    Ok(said)
}

/// What the check no land runs ([`Settings::main_check`]) last came to, or that it runs, as
/// [`status`] says it after the outputs' standing, which it leaves as it is: nothing where the
/// regenerator runs no check, or none has run.
fn checked(settings: &Settings, folder: &Path) -> String {
    let Some(checked) = Checked::read(folder).filter(|_| !settings.main_check.is_empty()) else {
        return String::new();
    };
    let command = settings.main_check.join(" ");
    let (default, tip) = (&settings.default_branch, short(&checked.tip));
    let when = sys::local_minute(checked.at);
    match (checked.state.as_str(), &checked.task) {
        ("checking", _) if lives(checked.pid, checked.start.as_deref()) => {
            format!(
                "; checking {default} at {tip} since {when} (pid {}): the check `{command}`",
                checked.pid
            )
        }
        ("checking", _) => format!(
            "; the check `{command}` of {default} at {tip}, begun {when}, ended unjudged; the \
             next round checks again"
        ),
        ("passed", _) => format!("; the check `{command}` passed on {default} at {tip} ({when})"),
        (_, Some(task)) => {
            format!("; the check `{command}` is red on {default} at {tip} ({when}, {task})")
        }
        (_, None) => format!("; the check `{command}` is red on {default} at {tip} ({when})"),
    }
}

/// How often [`settled`] reads where the regenerator stands while it waits.
pub const WAITING: Duration = Duration::from_secs(2);

/// Where the regenerator stands from where `env` runs, once its outputs on the default branch are
/// current or none makes them; while one makes them it reads [`status`] again every `every`,
/// `waiting` told each time it waits (it says whether to wait on). A release or a pin waits so:
/// it goes on only once they are current.
pub fn settled(
    env: &Env,
    every: Duration,
    waiting: &mut dyn FnMut(&Standing) -> bool,
) -> Result<Standing, String> {
    loop {
        let standing = status(env)?;
        if standing.current || !standing.making || !waiting(&standing) {
            return Ok(standing);
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: looks again for the regenerator's status each period, which no event announces"
        )]
        std::thread::sleep(every);
    }
}

/// Whether the regenerator may run where `env` runs: as the land does, only by the `land` recipe
/// that started it, or the one asked for, since it moves and pushes the default branch.
pub fn admit(env: &Env) -> Result<(), String> {
    if env.var(RECIPE).is_none() {
        return Err(
            "`tasks regenerate` moves and pushes the default branch: a land starts it (`just \
             land SLUG`); `just tasks regenerate --status` says where it stands"
                .into(),
        );
    }
    Ok(())
}
