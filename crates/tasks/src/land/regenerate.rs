//! The regenerator: what the project's `[land] generators` make of the default branch, made after a
//! land and never on its path, so a land is its merge, its move and its push alone ([`super`]).
//!
//! A land that moved the default branch starts it, detached ([`start`]), and waits for nothing it
//! does. At most one runs ([`LOCK`], taken without waiting): one that finds it held ends at once,
//! and the one holding it reads the default branch again once it lets go, and goes round while it
//! moved, so no land's start is lost. Each round ([`run`]) takes the default branch's newest tip
//! into a worktree of its own that it keeps between rounds (`regenerate` in the project's
//! `[agents] worktrees`, else `sf-tasks/land/regenerate-worktree`), so what its builds leave there
//! stays warm, and runs the generators there in order, a `when_changed` one only after an earlier
//! one changed something. A generator with a `key` prints it first: a key and outputs it passed on
//! before ([`Passed`], in `sf-tasks/land/`) need nothing made. What the generators change, inside
//! their outputs alone, is one commit, `Regenerate NAMES for BRANCH at TIP`, landed in seconds
//! under the land's lock: the default branch's tree as it is then with those outputs replaced,
//! made only while the default branch holds them as the tip did (else the round goes round), then
//! the fast-forward of the main checkout and the push, as a land makes them. It needs no
//! reviewer's approve: it changes the generators' outputs alone, made by them from the default
//! branch's own tip.
//!
//! A red round (a generator failed, or wrote outside its outputs) files the task [`RED`] on the
//! board, marked red (`tests/main-red-regenerate-2` and on once one was closed), or adds a line
//! to the one open; a round that passes closes it. What the last round came to is kept in
//! [`STATE`] and said by [`status`]. Its lines go to [`LOG`].

use super::{
    FOLDER, GIT_VARS, Generator, Landing, Move, RECIPE, Ran, Settings, TRIES, and, at, changed,
    clear, hold, on_paths, owns, run_git, same, short, signal_name, started, worktree,
};
use crate::cli::Env;
use crate::sweep::git::{rev, worktrees};
use crate::sweep::trigger;
use crate::sys::{self, Stdio};
use crate::{NewSession, NewTask};
use crate::{Store, red};
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fs::{self, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The lock one regenerator holds while it runs, in the lands' folder.
pub const LOCK: &str = "regenerate.lock";
/// What its last round came to ([`State`]), in the lands' folder.
pub const STATE: &str = "regenerate.json";
/// The keys and outputs each generator passed on ([`Passed`]), in the lands' folder.
pub const PASSED: &str = "regenerated.json";
/// Its log, in the lands' folder.
pub const LOG: &str = "regenerate.log";
/// The file naming the group a round runs while it runs, in the lands' folder: its own, so a land
/// never takes a regenerator's build for a group an earlier land left.
pub const RUNNING: &str = "regenerate.running";
/// The red task a red round files.
pub const RED: &str = "tests/main-red-regenerate";
/// What a red round marks its task with ([`red::MARK`]).
const MARK: &str = "the regenerator: [land] generators fail on the default branch";
/// The start of the reason its worktree is locked with, which keeps it from the sweep.
const REASON: &str = "symphony regenerate";
/// What it is called in its lines, where a land names its task.
const NAME: &str = "regenerate";
/// How many pairs [`Passed`] keeps, the newest first.
const KEPT: usize = 64;

/// Whether a land in `env` starts the regenerator: `TASKS_REGENERATE=on` or `off` says; else only
/// on the repository's own board, which no `TASKS_SERVER` names (a test's starts none).
pub fn enabled(env: &Env) -> bool {
    match env.var("TASKS_REGENERATE") {
        Some("off") => false,
        Some("on") => true,
        _ => env.var("TASKS_SERVER").is_none(),
    }
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
            .and_then(|text| serde_json::from_str(&text).ok())
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
        serde_json::from_str(&text).ok()
    }

    fn new(state: &str, tip: &str, landed: Option<&str>, line: &str) -> State {
        let pid = i64::from(std::process::id());
        State {
            state: state.into(),
            tip: tip.into(),
            landed: landed.map(str::to_string),
            at: trigger::now(),
            pid,
            start: started(pid),
            line: line.into(),
        }
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

/// Start the regenerator after a land moved the default branch of the main checkout `main`, whose
/// committed `settings` name its generators, detached: `tasks regenerate` from the main checkout,
/// by a test's `TASKS_BIN` on a test's board, else by the land's own build, else the main
/// checkout's build of `tasks` ([`crate::builds::of_main`]), with `env`'s environment but its
/// session, its output appended to [`LOG`]; a thread reaps it. None when there are no generators or `env` starts none
/// ([`enabled`]); else the line saying it started, or why it did not.
pub fn start(env: &Env, main: &Path, settings: &Settings) -> Option<String> {
    if settings.generators.is_empty() || !enabled(env) {
        return None;
    }
    let what = names(&settings.generators);
    let started = (|| -> Result<PathBuf, String> {
        let common = crate::git::common_dir(main).map_err(|e| e.to_string())?;
        let folder = trigger::folder(&common).join(FOLDER);
        fs::create_dir_all(&folder).map_err(|e| at(&folder, &e))?;
        // The land's own build, which holds the regenerator it starts: a link to the main
        // checkout's may be older than it.
        let binary = trigger::test_build(env)
            .or_else(|| std::env::current_exe().ok())
            .or_else(|| crate::builds::of_main(main, "tasks", env.var("PATH").map(OsStr::new)))
            .ok_or_else(|| crate::builds::missing(main, "tasks"))?;
        let path = folder.join(LOG);
        let log = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .map_err(|e| at(&path, &e))?;
        let mut command = sys::command_with(&binary, &env.vars);
        sys::sessionless(&mut command).env_remove("TASKS_TASK");
        for name in GIT_VARS {
            command.env_remove(name);
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
        Ok(path)
    })();
    Some(match started {
        Ok(log) => format!(
            "the regenerator makes {what} in the background (`just tasks regenerate --status`; its \
             log {})",
            log.display()
        ),
        Err(why) => format!("the regenerator of {what} did not start: {why}"),
    })
}

/// The regenerator's lock in the lands' `folder`, taken without waiting: none while another holds
/// it.
fn try_hold(folder: &Path) -> Result<Option<sys::FileLock>, String> {
    let path = folder.join(LOCK);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| at(&path, &e))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(sys::FileLock(file))),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(at(&path, &e)),
    }
}

/// What one round came to.
#[derive(Debug, Clone, PartialEq)]
enum Round {
    /// The generators' outputs are what they make of `tip`: nothing to land.
    Current { tip: String, line: String },
    /// The commit `commit`, made on `tip`, landed on the default branch, whose tip was then
    /// `onto`.
    Landed {
        tip: String,
        onto: String,
        commit: String,
        line: String,
    },
    /// The default branch changed the generators' outputs since `tip`: what was made of it is
    /// dropped, and the next round makes them again.
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
    let common = crate::git::common_dir(&env.cwd).map_err(|e| e.to_string())?;
    let main = trigger::main_checkout(&common)
        .ok_or_else(|| format!("{} has no main checkout to regenerate on", common.display()))?
        .to_path_buf();
    let folder = trigger::folder(&common).join(FOLDER);
    fs::create_dir_all(&folder).map_err(|e| at(&folder, &e))?;
    let interrupts = sys::Interrupts::listen().ok();
    let mut said: Option<String> = None;
    loop {
        let settings = Settings::of(&main)?;
        if settings.generators.is_empty() {
            return Ok("no [land] generators: nothing to regenerate".into());
        }
        let Some(held) = try_hold(&folder)? else {
            return Ok(said.unwrap_or_else(|| {
                "the regenerator runs already: it goes round once it ends, and makes the newest \
                 tip"
                .into()
            }));
        };
        let log = folder.join(LOG);
        let landing = Landing {
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
            recorder: None,
            out: &mut *out,
        };
        let mut regenerator = Regenerator {
            landing,
            common: &common,
        };
        let round = regenerator.round();
        regenerator.settle(store, &round);
        let branch = regenerator.landing.branch();
        drop(regenerator);
        // Read once it lets go: a land that moved the default branch while it held its lock started
        // a regenerator that found it held and ended, and this one goes round for it.
        drop(held);
        let now = rev(&main, &branch);
        // What the round left the default branch at: the commit it landed, or the tip it took.
        let (left, line, last) = match &round {
            Round::Current { tip, line } | Round::Red { tip, line } => (tip.clone(), line, true),
            Round::Landed {
                tip,
                onto,
                commit,
                line,
            } => (commit.clone(), line, tip == onto),
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
    landing: Landing<'a>,
    common: &'b Path,
}

impl Regenerator<'_, '_> {
    fn say(&mut self, line: &str) {
        self.landing.say(&format!("{NAME}: {line}"));
    }

    fn state(&self, state: &str, tip: &str, landed: Option<&str>, line: &str) {
        let _ = write_whole(
            &self.landing.folder,
            STATE,
            &State::new(state, tip, landed, line),
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
            self.state("stopped", "", None, &line);
            return stopped("", line);
        };
        let round = self.make(&tip);
        let (state, landed, line) = match &round {
            Round::Current { line, .. } => ("current", None, line),
            Round::Landed { commit, line, .. } => ("current", Some(commit.as_str()), line),
            Round::Dropped { line, .. } => ("dropped", None, line),
            Round::Red { line, .. } => ("red", None, line),
            Round::Stopped { line, .. } => ("stopped", None, line),
        };
        self.state(state, &tip, landed, line);
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
        self.state("building", tip, None, &format!("making {}", short(tip)));
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
            let key = match self.key(generator, &path) {
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
        for (generator, key) in ran {
            let Some(key) = key else { continue };
            if self.key(generator, &path).ok().flatten().as_deref() == Some(key.as_str()) {
                passed.add(Pair {
                    generator: generator.name.clone(),
                    key,
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
        let outputs: Vec<String> = made_by.iter().map(|g| g.outputs.clone()).collect();
        self.land(tip, &built, &outputs, &message)
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

    /// The key `generator` prints in `dir`, its last line, or none when it has no key, or its key
    /// failed or printed nothing (said, and the generator runs); the line saying it took a signal.
    fn key(&mut self, generator: &Generator, dir: &Path) -> Result<Option<String>, String> {
        if generator.key.is_empty() {
            return Ok(None);
        }
        let read = self.landing.logged_reading(&generator.key, dir, true);
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
            "{}'s key ({}) {why}: making it",
            generator.name,
            generator.key.join(" ")
        ));
        Ok(None)
    }

    /// Land `built`, made on `tip` in the regenerator's worktree, its generators' `outputs` changed,
    /// under the land's lock: on the default branch's tip as it is then, as `message`, only while
    /// it holds those outputs as `tip` did; then the push.
    fn land(&mut self, tip: &str, built: &str, outputs: &[String], message: &str) -> Round {
        let stopped = |line: String| Round::Stopped {
            tip: tip.into(),
            line,
        };
        let default = self.landing.settings.default_branch.clone();
        let held = {
            let landing = &mut self.landing;
            hold(self.common, NAME, landing.interrupts, &mut *landing.out)
        };
        let _held = match held {
            Ok(held) => held,
            Err(line) => return stopped(line),
        };
        let running = std::mem::replace(
            &mut self.landing.running,
            self.landing.folder.join(super::RUNNING),
        );
        let landed = self.land_held(tip, built, outputs, message);
        self.landing.running = running;
        match landed {
            Ok(round) => round,
            Err(line) => stopped(format!(
                "{} made on {default} at {} did not land: {line}",
                and(&outputs.iter().map(String::as_str).collect::<Vec<_>>()),
                short(tip)
            )),
        }
    }

    fn land_held(
        &mut self,
        tip: &str,
        built: &str,
        outputs: &[String],
        message: &str,
    ) -> Result<Round, String> {
        let landing = &mut self.landing;
        landing.leftover_group()?;
        landing.recover()?;
        let default = landing.settings.default_branch.clone();
        for _ in 0..TRIES {
            landing.go_on(None)?;
            landing.clean()?;
            let base = landing.base()?;
            if let Some(output) = outputs.iter().find(|output| {
                outputs_at(&landing.main, &base, output) != outputs_at(&landing.main, tip, output)
            }) {
                return Ok(Round::Dropped {
                    tip: tip.into(),
                    line: format!(
                        "{default} changed {output} since {}, at {}: what was made of it is \
                         dropped, and made again of the newest tip",
                        short(tip),
                        short(&base)
                    ),
                });
            }
            let commit = if base == tip {
                built.to_string()
            } else {
                replaced(
                    &landing.main,
                    &landing.folder,
                    &base,
                    built,
                    outputs,
                    message,
                )?
            };
            if rev(&landing.main, &format!("{commit}^{{tree}}"))
                == rev(&landing.main, &format!("{base}^{{tree}}"))
            {
                return Ok(Round::Current {
                    tip: base.clone(),
                    line: format!(
                        "{}: current on {default} at {}",
                        names(&landing.settings.generators),
                        short(&base)
                    ),
                });
            }
            match landing.fast_forward(&base, &commit, false)? {
                Move::Done => {}
                Move::Raced(_) => continue,
                Move::Not { why, restored } => {
                    let mut line = format!(
                        "{default} could not move to {} in the main checkout: {why}",
                        short(&commit)
                    );
                    match restored {
                        Ok(restored) => {
                            if let Some(said) = restored.said(&default, &base) {
                                line.push_str(&format!("; {said}"));
                            }
                        }
                        Err(why) => line.push_str(&format!(
                            "; what the move half-wrote in the main checkout could not be \
                             restored ({why}): the next land tries again"
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
            return Ok(Round::Landed {
                tip: tip.into(),
                onto: base.clone(),
                commit: commit.clone(),
                line: format!(
                    "{message}: landed on {default} at {}{pushed}",
                    &commit[..commit.len().min(12)]
                ),
            });
        }
        Err(format!("{default} moved {TRIES} times while it landed"))
    }

    /// The red task, filed, added to or closed as `round` says: filed or added to when red, closed
    /// when it made the outputs of the tip it took.
    fn settle(&mut self, store: &Store, round: &Round) {
        let said = match round {
            Round::Red { tip, line } => file_red(store, tip, line, &self.landing.log),
            Round::Current { tip, .. } | Round::Landed { tip, .. } => clear_red(store, tip),
            Round::Dropped { .. } | Round::Stopped { .. } => Ok(None),
        };
        match said {
            Ok(Some(line)) => self.say(&line),
            Ok(None) => {}
            Err(why) => self.say(&format!("the board did not take its red: {why}")),
        }
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
        let mut command = sys::command("git");
        for name in GIT_VARS {
            command.env_remove(name);
        }
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

/// Add a line to the open red task of the regenerator, or file one, for `tip`, red as `line` says,
/// its output in `log`: what it said.
fn file_red(store: &Store, tip: &str, line: &str, log: &Path) -> Result<Option<String>, String> {
    let (open, next) = reds(store)?;
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
            .map(|_| format!("red again: a line added to {slug}")),
        None => store
            .create(&NewTask {
                slug: next.clone(),
                title: "The regenerator is red on the default branch".into(),
                by: filer.clone(),
                body: format!(
                    "**What is red.** The regenerator (`just tasks regenerate`, started after each \
                     land) runs the project's `[land] generators` on the default branch's newest \
                     tip, and one failed there: {line}\n\nIts log is {}; `just tasks regenerate \
                     --status` says where it stands. Until a round passes, the committed outputs \
                     are behind the sources they are made from.\n\n**Done when.** A commit on the \
                     default branch makes the generators pass; the next round, which each land \
                     starts, closes this task.",
                    log.display()
                ),
                priority: 0,
                red: Some(MARK.into()),
                ..NewTask::default()
            })
            .map(|_| format!("red: filed {next}")),
    };
    let _ = store.leave(&filer);
    said.map(Some).map_err(|e| e.to_string())
}

/// Close the open red task of the regenerator, if one is: a round passed on `tip`.
fn clear_red(store: &Store, tip: &str) -> Result<Option<String>, String> {
    let (Some(open), _) = reds(store)? else {
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
                "The regenerator passed on main at {}: closing this red",
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

/// The regenerator's red task that is open, if one is, and the slug the next one is filed as:
/// [`RED`], then `RED-2`, `RED-3` and on once each is closed.
fn reds(store: &Store) -> Result<(Option<String>, String), String> {
    let mut open = None;
    let mut n = 1;
    loop {
        let slug = if n == 1 {
            RED.to_string()
        } else {
            format!("{RED}-{n}")
        };
        match store.get(&slug) {
            Ok(task) => {
                if red::open(&task.status) {
                    open = Some(slug);
                }
            }
            Err(crate::Error::Refused(why)) if why.starts_with("no task ") => {
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
    let folder = trigger::folder(&common).join(FOLDER);
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
    let making = match &state.start {
        Some(start) => started(state.pid).as_deref() == Some(start.as_str()),
        None => sys::pid_alive(state.pid),
    };
    Ok(match state.state.as_str() {
        "building" | "dropped" if making => standing(
            format!(
                "{what}: making them of {default} at {} since {when} (pid {}); {default} is {} past \
                 it",
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
                 without a word; the next land starts another",
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
        "red" => standing(
            format!(
                "{what}: red on {default} at {} since {when}: {} ({RED})",
                short(&state.tip),
                state.line
            ),
            false,
            making,
        ),
        _ => standing(
            format!(
                "{what}: stopped on {default} at {} ({when}): {}",
                short(&state.tip),
                state.line
            ),
            false,
            false,
        ),
    })
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
