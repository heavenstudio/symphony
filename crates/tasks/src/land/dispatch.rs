//! A land dispatched at review: where the project delivers by pull request (`[land] delivery`,
//! [`super::delivery`]), the host merges a task's head once its evidence stands, and no land here
//! waits for it (docs/workflow/TASKS.md, "A land dispatched at review").
//!
//! A task's move to review, and every board write after it that can change its evidence (an
//! assessment, a verdict, a gate decision, a move), starts `tasks land SLUG --dispatch` detached
//! from the main checkout ([`start`], [`on_move`], [`poke`]), and the write returns at once; a
//! task the regenerator files in review is dispatched as one moved there once it lets go of its
//! lock ([`on_filed`]). A
//! dispatch ([`Landing::dispatched`]) does what a land does up to the publication
//! ([`Landing::publish_head`]): the refusals at the head, the protection the program reads, the
//! head pushed, never forced, its pull request opened ready and the checks written from the
//! evidence at that head, whatever it says; then the host's own merge of that head armed
//! ([`Landing::arm`]). The check the default branch requires, `symphony/eligible`, is pending while
//! a stamp is missing, so the host merges once the last one stands, seconds after the write that
//! made it stand published it, with nobody running a land. A dispatch whose head and evidence are
//! as it published them asks the host nothing, and neither does one that runs once the task
//! landed at that head: it says so in its log alone. One whose publication the host refuses
//! because it merged that head meanwhile ([`Landing::merged_meanwhile`]) stops with no word on
//! the thread, the merge being the watcher's to settle. One that finds its task approved, not
//! landed and at a head the approve does not name moves it back to review first
//! ([`Landing::reviewed_again`]), where the fresh approve that head waits on is given.
//!
//! Whether the host merged is the one thing the board cannot see, and it is the watcher's
//! ([`watch`]): one per project (`tasks land --watch`, detached, [`WATCH_LOCK`]), started by each
//! dispatch that arms a merge, which reads each armed task every `[land] watch_seconds` and ends
//! once none is left. No webhook: nothing on this machine answers one. A round of one task
//! ([`Landing::watched`]) settles a merge as a land settles one, the task closed from review or
//! done alike; publishes again a task whose head or evidence changed since (a write no command
//! dispatched for: the owner's, through the Symphony); and, once the evidence it published stood,
//! takes a red check that is not the program's, a conflict with the default branch, or the pull
//! request closed unmerged as a failure: the merge disarmed first, so a later green cannot merge
//! behind the board, and the task back in progress for its executor ([`Landing::undelivered`]). One
//! neither merged nor red once `[land] merge_within_minutes` passed since its evidence stood is
//! disarmed and left where it is, for `just land SLUG` to arm again. A task that cannot go back
//! (cancelled, closed, or one the regenerator or a document save remakes) has a task filed for it
//! instead ([`FAILED`]). A task cancelled while its pull request is open has it closed unmerged,
//! its branch left.
//!
//! Both run as a land does, under the land's lock and with its mark ([`super::RECIPE`]), which
//! the board sets on what it starts: a move is not a land, but what it starts publishes and arms
//! only what the land would.

use super::delivery::{self, Binding, Journal, Receipt, Stage, Transport};
use super::{FOLDER, GIT_VARS, Landing, Mode, RECIPE, Refusal, Stop, short};
use crate::cli::Env;
use crate::sweep::git::rev;
use crate::sweep::trigger;
use crate::sys::{self, Stdio};
use crate::{NewSession, NewTask, Store, Task, red};
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The watcher's lock in the lands' folder, which names its process while it runs.
pub const WATCH_LOCK: &str = "watch.lock";
/// Where the watcher's lines go, in the lands' folder.
pub const WATCH_LOG: &str = "watch.log";
/// Where each dispatch a write started says its lines, in the lands' folder.
pub const DISPATCH_LOG: &str = "dispatch.log";
/// The slug a failed merge's task is filed under, the task's slug after it, `/` as `-`.
pub const FAILED: &str = "tools/delivery-failed-";
/// The statuses a dispatched task closes from once the host merged it: review, where a task that
/// needs no reviewer stands; done, where an approve left it; blocked, where a land held it.
pub const CLOSES: [&str; 3] = ["review", "blocked", "done"];

/// The watcher's rest between two rounds: what no event tells this machine, whether the host
/// merged, read again after it.
pub fn rest(wait: Duration) {
    #[expect(
        clippy::disallowed_methods,
        reason = "the named helper of RULES.md code/no-timed-waits for a check for an event its watcher can drop: the delivery watcher's round, as no webhook tells this machine the host merged"
    )]
    std::thread::sleep(wait);
}

/// Now, in whole seconds since 1970: the wall clock `[land] merge_within_minutes` is counted on.
pub fn seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
        })
}

/// Whether a write in `env` may start a dispatch or the watcher: `TASKS_DISPATCH=on` or `off` says;
/// else only on the repository's own board, which no `TASKS_SERVER` names, as for `on_review`.
pub fn enabled(env: &Env) -> bool {
    env.switch("TASKS_DISPATCH")
}

/// The lands' folder of the repository `main` is the main checkout of.
fn lands(main: &Path) -> Result<PathBuf, String> {
    let common = crate::git::common_dir(main).map_err(|e| e.to_string())?;
    Ok(trigger::folder(&common).join(FOLDER))
}

/// The main checkout of the repository `cwd` is in, and whether its committed settings deliver by
/// pull request.
fn delivering(cwd: &Path) -> Option<PathBuf> {
    let common = crate::git::common_dir(cwd).ok()?;
    let main = trigger::main_checkout(&common)?.to_path_buf();
    let command = delivery::command(&main).ok()?;
    (!command.is_empty()).then_some(main)
}

/// Where the project `cwd` is in delivers by pull request ([`delivering`]): the pull request
/// `slug` is bound to, as its journal names it, none before a dispatch bound one. None where the
/// land merges here.
pub fn bound(cwd: &Path, slug: &str) -> Option<Option<Binding>> {
    let main = delivering(cwd)?;
    let journal = lands(&main)
        .ok()
        .and_then(|folder| Journal::read(&folder, slug));
    Some(journal.and_then(|journal| journal.binding))
}

/// The `tasks` a detached child of `env` runs: a test's `TASKS_BIN`, else this one, else the main
/// checkout's build.
fn binary(env: &Env, main: &Path) -> Result<PathBuf, String> {
    trigger::test_build(env)
        .or_else(|| std::env::current_exe().ok())
        .or_else(|| crate::builds::of_main(main, "tasks", &trigger::beyond(env)))
        .ok_or_else(|| crate::builds::missing(main, "tasks"))
}

/// `tasks land ARGS` started detached from `main`, with `env`'s environment but its session, the
/// task the recorder names and git's own, and the land's mark ([`RECIPE`]); its output appended to
/// `log` in the lands' folder, and a thread reaping it.
fn detached(env: &Env, main: &Path, args: &[&str], log: &str) -> Result<PathBuf, String> {
    let folder = lands(main)?;
    fs::create_dir_all(&folder).map_err(|e| super::at(&folder, &e))?;
    let path = folder.join(log);
    let file = trigger::append_log(&folder, log).map_err(|e| super::at(&path, &e))?;
    let program = binary(env, main)?;
    let mut command = sys::command_with(&program, &env.vars);
    sys::sessionless(&mut command).env_remove("TASKS_TASK");
    for name in GIT_VARS {
        command.env_remove(name);
    }
    command
        .env(RECIPE, "dispatch")
        .arg("land")
        .args(args)
        .current_dir(main)
        .stdin(Stdio::null())
        .stdout(file.try_clone().map_err(|e| super::at(&path, &e))?)
        .stderr(file)
        .new_session();
    let mut child = command
        .spawn()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(path)
}

/// The dispatch of `slug` as `actor`, started detached from the main checkout `main`: the log its
/// lines go to, or why it did not start.
fn dispatching(env: &Env, main: &Path, slug: &str, actor: &str) -> Result<PathBuf, String> {
    detached(
        env,
        main,
        &[slug, "--dispatch", "--as", actor],
        DISPATCH_LOG,
    )
}

/// The line saying a dispatch started, by `start`, or why it did not.
fn started(start: &Result<PathBuf, String>) -> String {
    match start {
        Ok(log) => format!(
            "its dispatch to the host started detached (its lines go to {})",
            log.display()
        ),
        Err(why) => format!("its dispatch to the host did not start: {why}"),
    }
}

/// The dispatch of `slug` as `actor`, started detached from the main checkout of the repository
/// `cwd` is in: none where its settings deliver nothing or `env` starts none ([`enabled`]); else the
/// line saying it started, or why it did not.
pub fn start(env: &Env, cwd: &Path, slug: &str, actor: &str) -> Option<String> {
    if !enabled(env) {
        return None;
    }
    let main = delivering(cwd)?;
    Some(started(&dispatching(env, &main, slug, actor)))
}

/// What a move of `slug` to review by `mover` starts, where the project `cwd` is in delivers by
/// pull request and `env` starts one ([`enabled`]): its mark left, naming `mover` ([`mark`]), then
/// its dispatch as `actor` ([`dispatching`]). None where nothing starts.
fn to_review(
    env: &Env,
    cwd: &Path,
    slug: &str,
    mover: &str,
    actor: &str,
) -> Option<Result<PathBuf, String>> {
    if !enabled(env) {
        return None;
    }
    let main = delivering(cwd)?;
    if let Ok(folder) = lands(&main)
        && fs::create_dir_all(&folder).is_ok()
    {
        let _ = fs::write(mark(&folder, slug), mover);
    }
    Some(dispatching(env, &main, slug, actor))
}

/// What a move of `task` by `actor` starts: its dispatch, where it moved to review (its mark left
/// first, [`mark`]) or was dispatched before ([`poke`]), for a task the delivery pipeline takes;
/// else none.
pub fn on_move(env: &Env, cwd: &Path, task: &Task, actor: &str) -> Option<String> {
    if !crate::on_review::delivery_task(task.parent.as_deref(), task.instruction) {
        return None;
    }
    if task.status == "review" {
        return to_review(env, cwd, &task.slug, actor, actor).map(|start| started(&start));
    }
    poke(env, cwd, &task.slug, actor)
}

/// What `task`, which one of the board's own makers (the regenerator) filed in review, starts once
/// the maker let go of the lock its dispatch would be refused under: what a move to review starts
/// on the command line ([`on_move`]), which the store's own move does not, where nothing
/// dispatched it yet (no [`mark`], no pull request bound) and it stands where a merge closes it
/// ([`CLOSES`]). Its mark names the session that filed it, which left at once; its dispatch acts
/// as its orchestrator, who stays, where one holds the role ([`acting`]), as a session that left
/// posts nothing. A dispatch that does not start is said on the thread to `@orchestrator`, once,
/// as the board's own line: nobody reads the maker's log, and the next write to the task
/// dispatches it by its mark. The line saying what started; none where nothing starts.
pub fn on_filed(env: &Env, store: &Store, cwd: &Path, task: &Task) -> Option<String> {
    let slug = task.slug.as_str();
    if !CLOSES.contains(&task.status.as_str()) {
        return None;
    }
    let main = delivering(cwd)?;
    let folder = lands(&main).ok()?;
    let bound = Journal::read(&folder, slug).is_some_and(|journal| journal.binding.is_some());
    if bound || mark(&folder, slug).exists() {
        return None;
    }
    let actor = acting(Some(task), &task.created_by);
    let start = to_review(env, &main, slug, &task.created_by, &actor)?;
    let line = started(&start);
    if start.is_err() {
        let _ = store.tell_orchestrator_once(
            slug,
            &format!(
                "land {slug}: {line}; `just land {slug} --dispatch` dispatches it, as a later \
                 write to it does"
            ),
            NOT_STARTED,
        );
    }
    Some(line)
}

/// The key a filed task's dispatch that did not start is said under, once on its thread
/// ([`on_filed`]).
const NOT_STARTED: &str = "dispatch-not-started";

/// The dispatch of `slug`, started as [`start`] does, only where it was dispatched before: a
/// write that can change its evidence (an assessment, a verdict, a gate decision) after its pull
/// request was published, or after a move to review whose dispatch has not bound one yet (its
/// [`mark`]), so a first dispatch that stopped before it published is made again. None for a
/// task never dispatched, or delivered and closed.
pub fn poke(env: &Env, cwd: &Path, slug: &str, actor: &str) -> Option<String> {
    if !enabled(env) {
        return None;
    }
    let main = delivering(cwd)?;
    let folder = lands(&main).ok()?;
    let bound = Journal::read(&folder, slug).is_some_and(|journal| journal.binding.is_some());
    if !(bound || mark(&folder, slug).exists()) {
        return None;
    }
    start(env, &main, slug, actor)
}

/// The mark a move to review leaves in the lands' `folder` for `slug` until a dispatch binds its
/// pull request: what [`poke`] starts a dispatch by before there is a binding to start it by.
/// It names who moved it.
fn mark(folder: &Path, slug: &str) -> PathBuf {
    folder.join(format!("{}{MARK}", super::file_stem(slug)))
}

/// A mark's ending in the lands' folder ([`mark`]).
const MARK: &str = ".dispatching";

/// `slug`'s [`mark`] taken away: its pull request bound, or its dispatch due no more (cancelled,
/// or out of review before it bound one).
pub(super) fn unmark(folder: &Path, slug: &str) {
    let _ = fs::remove_file(mark(folder, slug));
}

/// Whether `said`, a refusal and the head it was made at, is new for `slug`: not the last one said
/// on its thread, which it is from now on. Kept in the lands' `folder` beside its journal and
/// beyond it, as `SLUG.said`, so what holds a journal in hand writes nothing over it. Asked only by
/// a land delivered by a pull request, whose end forgets it ([`unsaid`]).
pub(super) fn said(folder: &Path, slug: &str, said: &str) -> bool {
    let path = folder.join(format!("{}{SAID}", super::file_stem(slug)));
    if fs::read_to_string(&path).is_ok_and(|last| last == said) {
        return false;
    }
    let _ = fs::write(&path, said);
    true
}

/// `slug`'s last refusal said forgotten ([`said`]): its delivery ended, landed or cancelled.
pub(super) fn unsaid(folder: &Path, slug: &str) {
    let _ = fs::remove_file(folder.join(format!("{}{SAID}", super::file_stem(slug))));
}

/// The ending of the file a task's last refusal said is kept in ([`said`]).
const SAID: &str = ".said";

/// When a round of the watcher first could not finish `slug`'s land, in whole seconds since 1970:
/// `clock` where none is kept yet, which it is from now on. Kept in the lands' `folder` beside its
/// journal, as `SLUG.stopped`, so a run that writes the journal whole writes nothing over it.
pub(super) fn stopped(folder: &Path, slug: &str, clock: i64) -> i64 {
    let path = folder.join(format!("{}{STOPPED}", super::file_stem(slug)));
    let kept = fs::read_to_string(&path)
        .ok()
        .and_then(|text| text.trim().parse().ok());
    kept.unwrap_or_else(|| {
        let _ = fs::write(&path, clock.to_string());
        clock
    })
}

/// When `slug`'s land first could not be finished forgotten ([`stopped`]): it ended, or it is a
/// round's to finish no more.
pub(super) fn unstopped(folder: &Path, slug: &str) {
    let _ = fs::remove_file(folder.join(format!("{}{STOPPED}", super::file_stem(slug))));
}

/// The ending of the file that keeps when a task's land first could not be finished
/// ([`stopped`]).
const STOPPED: &str = ".stopped";

/// The watcher started detached from the main checkout `main` where none runs and `env` starts one
/// ([`enabled`]): the line saying it started, or none where one runs already or none starts.
pub fn watch_start(env: &Env, main: &Path) -> Option<String> {
    if !enabled(env) {
        return None;
    }
    let folder = lands(main).ok()?;
    if holder(&folder).is_some() {
        return None;
    }
    Some(match detached(env, main, &["--watch"], WATCH_LOG) {
        Ok(log) => format!(
            "the watcher reads what the host made of it every round (its lines go to {})",
            log.display()
        ),
        Err(why) => format!("the watcher did not start: {why}"),
    })
}

/// Who holds the watcher's lock in `folder`, by the name it wrote: none while it is free.
fn holder(folder: &Path) -> Option<String> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(folder.join(WATCH_LOCK))
        .ok()?;
    match file.try_lock() {
        Ok(()) => {
            let _ = file.unlock();
            None
        }
        Err(_) => {
            let mut name = String::new();
            let _ = file.seek(SeekFrom::Start(0));
            let _ = file.read_to_string(&mut name);
            Some(match name.trim() {
                "" => "a watcher".to_string(),
                name => name.to_string(),
            })
        }
    }
}

/// The watcher's lock `file`, taken without waiting and named by this process in one write, as a
/// land names its lock: none while another holds it.
fn taken(file: std::fs::File) -> Option<sys::FileLock> {
    file.try_lock().ok()?;
    let held = sys::FileLock(file);
    let mut named = &held.0;
    let _ = named.set_len(0);
    let _ = named.seek(SeekFrom::Start(0));
    let _ = named.write_all(format!("the watcher (pid {})", std::process::id()).as_bytes());
    Some(held)
}

/// Each task the lands' folder `folder` holds a journal of that the watcher reads
/// ([`Journal::watched`]), by slug: armed at the host, or merged there with its land not ended.
pub fn armed(folder: &Path) -> Vec<Journal> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut armed: Vec<Journal> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".delivery"))
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .filter_map(|text| serde_json::from_str::<Journal>(&text).ok())
        .filter(Journal::watched)
        .collect();
    armed.sort_by(|a, b| a.slug.cmp(&b.slug));
    armed
}

/// Who the delivery moves `task` as: its orchestrator, who holds a role on it and may move it;
/// else `by`, who dispatched it. A round of the watcher acts so throughout, and a dispatch so for
/// its moves back to review ([`Landing::reviewed_again`]) and to work
/// ([`Landing::back_to_work`]), whoever's write started it.
fn acting(task: Option<&Task>, by: &str) -> String {
    (task.and_then(|task| task.holders("orchestrator").first().copied()))
        .unwrap_or(by)
        .to_string()
}

/// What the watcher's last round said of each armed task: a round that comes to the same says
/// nothing, so a task that waits days for its approve costs the log one line and not one a round,
/// and what the log gains follows what changed, never how long the watcher ran.
#[derive(Default)]
pub(super) struct Told(std::collections::BTreeMap<String, Vec<u8>>);

impl Told {
    /// Only the tasks in `armed` kept: one armed no more is forgotten, so armed again it is said
    /// again, and what is kept is no more than what is armed.
    pub(super) fn keep(&mut self, armed: &[&str]) {
        self.0.retain(|slug, _| armed.contains(&slug.as_str()));
    }

    /// Whether `said`, all a round wrote of `slug`, differs from its round before: kept as its
    /// last from now on.
    pub(super) fn news(&mut self, slug: &str, said: Vec<u8>) -> bool {
        if self.0.get(slug) == Some(&said) {
            return false;
        }
        self.0.insert(slug.to_string(), said);
        true
    }

    /// What the last round that said anything of `slug` said.
    pub(super) fn last(&self, slug: &str) -> &[u8] {
        self.0.get(slug).map_or(&[], Vec::as_slice)
    }
}

/// How a watcher runs: its clock, its rest and, for a test, how many rounds it makes at most.
pub struct Watching<'a> {
    /// What answers the delivery's verbs in place of the command `[land] delivery` names: a test's.
    pub transport: Option<&'a dyn Transport>,
    /// The wall clock, in whole seconds since 1970, that `[land] merge_within_minutes` is
    /// counted on ([`seconds`]).
    pub clock: &'a dyn Fn() -> i64,
    /// The rest between two rounds ([`rest`]).
    pub rest: &'a dyn Fn(Duration),
    /// The rounds it makes at most; none, until nothing is armed.
    pub rounds: Option<u32>,
}

/// `tasks land --watch`: while any task is armed at the host, or merged there with its land not
/// ended, a round of each ([`Landing::watched`]) every `[land] watch_seconds`, ended once none is;
/// the line it ends with. One runs per project:
/// a watcher that finds another running ends at once. A round writes to `out` what it came to for
/// a task only where that differs from its round before ([`Told`]): `out` is the watcher's log,
/// which is set aside past its limit only when a watcher starts.
///
/// # Errors
///
/// No checkout, or a lock that cannot be opened.
pub fn watch(
    store: &Store,
    env: &Env,
    out: &mut dyn Write,
    watching: &Watching<'_>,
) -> Result<String, String> {
    let common = crate::git::common_dir(&env.cwd).map_err(|e| e.to_string())?;
    let main = trigger::main_checkout(&common)
        .ok_or_else(|| format!("{} has no main checkout", common.display()))?
        .to_path_buf();
    let folder = trigger::folder(&common).join(FOLDER);
    fs::create_dir_all(&folder).map_err(|e| super::at(&folder, &e))?;
    let path = folder.join(WATCH_LOCK);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| super::at(&path, &e))?;
    let Some(mut held) = taken(file) else {
        let running = holder(&folder).unwrap_or_else(|| "a watcher".into());
        return Ok(format!("{running} runs already: nothing started"));
    };
    let mut round = 0;
    let mut told = Told::default();
    loop {
        let mut journals = armed(&folder);
        if journals.is_empty() {
            // Let go first, then look once more: a dispatch that armed a task meanwhile, and
            // found the lock still held, started no watcher, so this one takes it back.
            drop(held);
            journals = armed(&folder);
            let again = (!journals.is_empty())
                .then(|| OpenOptions::new().read(true).write(true).open(&path).ok())
                .flatten()
                .and_then(taken);
            match again {
                Some(lock) => held = lock,
                None => return Ok("nothing is armed at the host: the watcher ends".into()),
            }
        }
        if watching.rounds.is_some_and(|rounds| round >= rounds) {
            let unfinished = journals.iter().filter(|j| j.settling()).count();
            let armed = journals.len() - unfinished;
            return Ok(match unfinished {
                0 => format!("{armed} armed at the host after {round} round(s)"),
                _ => format!(
                    "{armed} armed at the host and {unfinished} merged with its land not ended \
                     after {round} round(s)"
                ),
            });
        }
        round += 1;
        let slugs: Vec<&str> = journals
            .iter()
            .map(|journal| journal.slug.as_str())
            .collect();
        told.keep(&slugs);
        for journal in journals {
            let task = store.get(&journal.slug).ok();
            let actor = acting(task.as_ref(), &journal.by);
            let mut said = Vec::new();
            let ended = super::watch_round(
                store,
                env,
                &journal.slug,
                &actor,
                &mut said,
                watching.transport,
                (watching.clock)(),
            );
            let line = match ended {
                Ok(line) => line,
                Err(Stop::Refused(line)) => line,
                Err(Stop::Board(error)) => format!("{}: the board failed: {error}", journal.slug),
            };
            said.extend_from_slice(line.as_bytes());
            said.push(b'\n');
            if told.news(&journal.slug, said) {
                let _ = out.write_all(told.last(&journal.slug));
                let _ = out.flush();
            }
        }
        let settings = super::Settings::of(&main).unwrap_or_default();
        (watching.rest)(settings.watch);
    }
}

/// What turning a task's merge off as it left review came to ([`Landing::left_review`]).
enum Left {
    /// Off, the journal written so.
    Off,
    /// Ended there, merged or still armed: the line it ends with.
    Ended(String),
}

/// The line a task's merge turned off as it left review is said by: `slug` at `status`, its
/// checks published at `head`.
fn off(slug: &str, status: &str, head: &str) -> String {
    format!(
        "land {slug}: its auto-merge is off while {slug} is {status}, its checks as its evidence \
         says at {}; it is armed again once it moves to review",
        short(head)
    )
}

/// What a disarm came to.
enum Disarm {
    /// Off: no check that passes later merges it.
    Off,
    /// The host merged it first.
    Merged,
    /// Not turned off: the line saying so.
    Not(String),
}

/// Why a merge armed at the host did not come.
pub(super) enum Failure {
    /// Checks on its head that are not the program's are red: each name and conclusion.
    Red(Vec<String>),
    /// The host reports the pull request in conflict with the default branch.
    Conflict,
    /// Closed without a merge, by someone.
    Closed,
    /// Neither merged nor red within `[land] merge_within_minutes` of its evidence standing.
    Bound(u64),
}

impl Landing<'_> {
    /// `tasks land SLUG --dispatch`: the task's head published and the host's merge of it armed,
    /// its evidence whatever it says, as a land does with evidence that stands
    /// ([`Landing::publish_head`], [`Landing::arm`]); the line it ends with. A task not in review,
    /// done or blocked is dispatched only where it was before (a new head after a changes verdict,
    /// say); a cancelled one has its pull request closed; one whose head and evidence are as last
    /// published asks the host nothing.
    pub(super) fn dispatched(mut self) -> Result<String, Stop> {
        let slug = self.slug;
        self.leftover_group().map_err(Stop::Refused)?;
        self.recover().map_err(Stop::Refused)?;
        if !self.delivers() {
            return Ok(format!(
                "{slug}: [land] delivery is not set, so nothing is dispatched"
            ));
        }
        if self.held()?.is_some() {
            return Ok(format!(
                "{slug} lands by the workflow that holds it: nothing is dispatched"
            ));
        }
        if let Some(done) = self.resumed() {
            return done;
        }
        let task = self.store.get(slug)?;
        let journal = Journal::read(&self.folder, slug);
        if task.status == "cancelled" {
            return self.cancelled(journal);
        }
        let bound = (journal.as_ref()).is_some_and(|journal| journal.binding.is_some());
        let closes = CLOSES.contains(&task.status.as_str());
        if bound || !closes {
            // Bound, a later write dispatches it by its binding; out of review unbound, its next
            // move to review marks it again.
            unmark(&self.folder, slug);
        }
        if !(closes || bound) {
            return Ok(format!(
                "{slug} is {}: it is dispatched once it moves to review",
                task.status
            ));
        }
        self.repeated()?;
        self.unchecked().map_err(|refusal| self.refuse(refusal))?;
        let tip = rev(&self.main, &format!("refs/heads/{slug}")).ok_or_else(|| {
            self.refuse(Refusal::thread(format!(
                "{slug} has no branch in the main checkout to dispatch"
            )))
        })?;
        // Landed already at this head, by its own receipt ([`Landing::repeated`] refused any
        // other): a write's dispatch that ran once the land of its merge had closed the task and
        // dropped its journal. Nothing is published to a pull request merged, and nothing stopped.
        if let Some(receipt) = Receipt::read(&self.folder, slug).filter(|kept| kept.head == tip) {
            unmark(&self.folder, slug);
            return Ok(format!(
                "{slug} landed already as {} from {} by pull request #{} of {}: nothing is \
                 dispatched",
                short(&receipt.squash),
                short(&receipt.head),
                receipt.pull,
                receipt.name
            ));
        }
        // A pull request bound before and merged or closed since is settled first.
        if let Some(journal) = journal.clone().filter(|journal| journal.binding.is_some())
            && let Some(done) = self.settled_before(journal)
        {
            return done;
        }
        let task = self.reviewed_again(task, &tip)?;
        // Out of review with its merge armed: turned off before anything is published, as a
        // publication that keeps its evidence standing would leave the host seconds to merge a
        // task moved back to work.
        if !closes && let Some(earlier) = journal.clone().filter(|journal| journal.armed) {
            if let Left::Ended(line) = self.left_review(earlier, &task.status)? {
                return Ok(line);
            }
            self.publish_head(&task, &tip, &CLOSES, String::new(), false)?;
            let line = off(slug, &task.status, &tip);
            self.post(&line, None);
            return Ok(line);
        }
        let snapshot = self.read_evidence(&task, &tip, false)?;
        if let Some(journal) = journal.filter(|journal| {
            closes
                && journal.stage == Stage::Armed
                && journal.head == tip
                && journal.revision == snapshot.revision
        }) {
            if let Some(said) = watch_start(self.env, &self.main) {
                self.say(&format!("land {slug}: {said}"));
            }
            return Ok(format!(
                "{slug}: armed at {} by pull request #{}, its evidence as published",
                short(&journal.head),
                journal.binding.map_or(0, |binding| binding.pull)
            ));
        }
        let (journal, eligible) = self.publish_head(&task, &tip, &CLOSES, String::new(), false)?;
        if closes {
            return self.arm(journal, eligible);
        }
        // Published for its checks to say where it stands; armed only once it is back in review.
        Ok(format!(
            "{slug} is {}: its checks say its evidence at {}, and it is armed once it moves to \
             review",
            task.status,
            short(&tip)
        ))
    }

    /// `task` as a dispatch goes on with it at `tip`, its branch's head: back in review, by the
    /// board's own move from done ([`Store::move_to`]), where its reviewer approved it, no landing
    /// has closed it ([`crate::orchestration::unlanded`]) and the approve does not name `tip`; any
    /// other task as it is. A delivery takes an approve naming the head alone
    /// ([`super::evidence`]), so a head moved under an approved task (a merge of the default
    /// branch that settles a conflict, a fix) waits on a fresh one, which only a task in review is
    /// given: said once to its orchestrator, which starts its assessor and resumes its reviewer as
    /// at any move to review. The move is its orchestrator's ([`acting`]), whoever's write started
    /// the dispatch: an assessor's holds no role that may send it back. One that does not go back
    /// (no orchestrator, and who dispatched it may not move it) is refused, naming the move that
    /// does.
    fn reviewed_again(&mut self, task: Task, tip: &str) -> Result<Task, Stop> {
        let slug = self.slug;
        if crate::orchestration::unlanded(&task).is_none() {
            return Ok(task);
        }
        let thread = self.store.thread(slug)?;
        let Ok(approval) = super::approval(&task, &thread) else {
            return Ok(task);
        };
        let approved: Vec<String> = (super::named(&approval.text).into_iter())
            .filter_map(|word| rev(&self.main, &format!("{word}^{{commit}}")))
            .collect();
        if approved.iter().any(|sha| sha == tip) {
            return Ok(task);
        }
        let moved = format!(
            "{slug}'s head is {}, which its approve by {} does not name ({})",
            short(tip),
            approval.by,
            approved.first().map_or_else(
                || "it names no commit".to_string(),
                |sha| format!("it names {}", short(sha))
            )
        );
        let mover = acting(Some(&task), self.actor);
        match self.store.move_to(slug, "review", &mover) {
            Ok(task) => {
                let verify = self.verifying(&task);
                self.post(
                    &format!(
                        "land {slug}: {moved}; {slug} is back in review{verify}. Its assessor \
                         reads {head} and its reviewer approves it naming it; its pull \
                         request shows {head} with `symphony/eligible` pending until then",
                        head = short(tip)
                    ),
                    Some("@orchestrator"),
                );
                Ok(task)
            }
            Err(error) => Err(self.refuse(Refusal::to(
                format!("{moved}, and it does not go back to review ({error})"),
                "@orchestrator",
                format!("`just tasks move {slug} review` does"),
            ))),
        }
    }

    /// The pull request `journal` is bound to, read: merged since, settled as a land settles a
    /// merge; closed unmerged, a failure ([`Landing::undelivered`]); open, or not read, none, and the
    /// dispatch goes on.
    fn settled_before(&mut self, journal: Journal) -> Option<Result<String, Stop>> {
        let binding = journal.binding.clone()?;
        let request = json!({
            "verb": "result", "slug": self.slug,
            "repository": binding.repository, "pull": binding.pull,
        });
        let result = self.ask("result", &request).ok()?;
        match delivery::state(&result).0 {
            "merged" => Some(self.merged(journal, None)),
            "closed" => Some(self.undelivered(journal, Failure::Closed)),
            _ => None,
        }
    }

    /// The line a dispatch ends with, saying nothing on the thread, where the host refused its
    /// publication because it merged the head meanwhile: the pull request `journal` binds, read
    /// as open when the dispatch began, reads merged at `tip`, the head asked to be published,
    /// and the watcher reads `journal` ([`Journal::watched`]), so its next round settles the
    /// merge and closes the task. None for a land or a round, where the pull request does not
    /// read, and where it was merged at another head or closed: the refusal stands as it is.
    pub(super) fn merged_meanwhile(&self, journal: &Journal, tip: &str) -> Option<String> {
        let slug = self.slug;
        if !matches!(self.mode, Mode::Dispatch) || !journal.watched() {
            return None;
        }
        let binding = journal.binding.as_ref()?;
        let request = json!({
            "verb": "result", "slug": slug,
            "repository": binding.repository, "pull": binding.pull,
        });
        let result = self.ask("result", &request).ok()?;
        let merged = delivery::state(&result).0 == "merged" && result["head"].as_str() == Some(tip);
        merged.then(|| {
            // One that ended, or that a reboot ended, is started for it.
            let _ = watch_start(self.env, &self.main);
            format!(
                "{slug}: pull request #{} of {} merged {} while it was dispatched: nothing more \
                 is published to it, and the watcher's next round closes the task",
                binding.pull,
                binding.name,
                short(tip)
            )
        })
    }

    /// A round of the watcher for this task, armed at the host, at `clock` (whole seconds since
    /// 1970). Its evidence published standing, the pull request is read first: merged, settled
    /// as a land settles a merge (held for a person where the branch moved past the head merged);
    /// closed, a failure. Then a head or evidence changed since it was published (a write no
    /// command dispatched for: the owner's, through the Symphony) is dispatched again; and,
    /// standing, a red check that is not the Symphony's, a conflict or the bound passed is a
    /// failure ([`Failure`]). Evidence not standing yet asks the host nothing: it merges nothing
    /// while `symphony/eligible` is pending.
    pub(super) fn watched(mut self, clock: i64) -> Result<String, Stop> {
        let slug = self.slug;
        let Some(journal) = Journal::read(&self.folder, slug).filter(Journal::watched) else {
            return Ok(format!("{slug} is not armed at the host"));
        };
        if journal.settling() {
            return self.finished(clock);
        }
        let task = self.store.get(slug)?;
        if task.status == "cancelled" {
            return self.cancelled(Some(journal));
        }
        let Some(binding) = journal.binding.clone() else {
            return Ok(format!("{slug} is armed and bound to no pull request"));
        };
        let tip = rev(&self.main, &format!("refs/heads/{slug}"));
        // A publication that stopped before the host answered its arming again: whatever its
        // checks said when it stopped, the host may merge it, so it is read every round.
        let unfinished = journal.stage != Stage::Armed;
        let mut read = Value::Null;
        if journal.green.is_some() || unfinished {
            let request = json!({
                "verb": "result", "slug": slug,
                "repository": binding.repository, "pull": binding.pull,
            });
            let unread = |why: &str| {
                format!(
                    "{slug}: pull request #{} was not read ({why}): the next round reads it",
                    binding.pull
                )
            };
            read = match self.ask("result", &request) {
                Ok(result) => result,
                Err(why) => return Ok(unread(&why)),
            };
            match delivery::state(&read) {
                ("merged", _) if tip.as_ref().is_none_or(|tip| *tip == journal.head) => {
                    return self.merged(journal, Some(&task.status));
                }
                ("merged", _) => {
                    let what = format!(
                        "pull request #{} was merged at {} and {slug}'s branch is at {}",
                        binding.pull,
                        short(&journal.head),
                        short(tip.as_deref().unwrap_or_default())
                    );
                    return Err(self.hold(journal, what));
                }
                ("closed", _) => return self.undelivered(journal, Failure::Closed),
                ("open", _) => {}
                (said, why) => return Ok(unread(if why.is_empty() { said } else { why })),
            }
        }
        if !CLOSES.contains(&task.status.as_str()) {
            let head = journal.head.clone();
            return Ok(match self.left_review(journal, &task.status)? {
                Left::Ended(line) => line,
                Left::Off => {
                    let line = off(slug, &task.status, &head);
                    self.post(&line, None);
                    line
                }
            });
        }
        let task = match &tip {
            Some(tip) => self.reviewed_again(task, tip)?,
            None => task,
        };
        // A head or evidence that changed since it was published, or a publication that did not
        // end: dispatched again.
        if let Some(tip) = tip {
            let snapshot = self.read_evidence(&task, &tip, false)?;
            if unfinished || tip != journal.head || snapshot.revision != journal.revision {
                self.say(&format!(
                    "land {slug}: {}: publishing it again",
                    if tip != journal.head {
                        "its head changed since it was published"
                    } else if unfinished {
                        "its publication did not end while its merge is armed"
                    } else {
                        "its evidence changed since it was published"
                    }
                ));
                let (journal, eligible) =
                    self.publish_head(&task, &tip, &CLOSES, String::new(), false)?;
                return self.arm(journal, eligible);
            }
        }
        let Some(green) = journal.green else {
            return Ok(format!(
                "{slug}: armed at {}, its evidence not standing yet",
                short(&journal.head)
            ));
        };
        let foreign: Vec<String> = (read["foreign"].as_array().into_iter().flatten())
            .map(|check| {
                format!(
                    "{} ({})",
                    check["name"].as_str().unwrap_or("a check"),
                    check["conclusion"].as_str().unwrap_or("red")
                )
            })
            .collect();
        if !foreign.is_empty() {
            return self.undelivered(journal, Failure::Red(foreign));
        }
        if read["mergeable_state"] == "dirty" {
            return self.undelivered(journal, Failure::Conflict);
        }
        let within = self.settings.merge_within.as_secs();
        let stood = u64::try_from(clock.saturating_sub(green)).unwrap_or(0);
        if stood > within {
            return self.undelivered(journal, Failure::Bound(within / 60));
        }
        Ok(format!(
            "{slug}: armed at {}, its evidence standing for {} min: waiting for {} to merge it",
            short(&journal.head),
            stood / 60,
            binding.name
        ))
    }

    /// A round for this task, its land past the merge asked and not ended ([`Journal::settling`]):
    /// the land's tail gone on with from where its journal stands, as a land run again goes on
    /// ([`Landing::resumed`]): the pull request read, the commit proved, the checkout
    /// fast-forwarded, the task closed once and what follows a land started once. A round that
    /// cannot finish it says why and leaves it for the next; one a person now holds, or whose pull
    /// request is open after all, is this round's no more. A stop that lasts past
    /// `[land] merge_within_minutes` of the first round it stopped is said once on the thread.
    fn finished(&mut self, clock: i64) -> Result<String, Stop> {
        let slug = self.slug;
        let stop = match self.resumed() {
            Some(Ok(line)) => return Ok(line),
            Some(Err(stop)) => stop,
            None => return Ok(format!("{slug} is not armed at the host")),
        };
        let Some(left) = Journal::read(&self.folder, slug).filter(Journal::settling) else {
            unstopped(&self.folder, slug);
            return Err(stop);
        };
        let why = match stop {
            Stop::Refused(line) => line,
            Stop::Board(error) => format!("the board failed: {error}"),
        };
        let within = self.settings.merge_within.as_secs();
        let since = stopped(&self.folder, slug, clock);
        if u64::try_from(clock.saturating_sub(since)).unwrap_or(0) > within {
            let said = format!(
                "land {slug}: the land of {} has not ended",
                short(&left.head)
            );
            // Said once, however many rounds stop: a thread that does not read says nothing.
            let told = (self.store.thread(slug))
                .map(|thread| thread.iter().any(|post| post.text.starts_with(&said)));
            if told.is_ok_and(|told| !told) {
                self.post(
                    &format!(
                        "{said} ({}) within {} min of the watcher's first round that could not \
                         finish it (`[land] merge_within_minutes`): {why}. The watcher tries again \
                         every round; put right what it names, or run `just land {slug}`, which \
                         goes on from it",
                        left.said(),
                        within / 60
                    ),
                    Some("@orchestrator"),
                );
            }
        }
        Err(Stop::Refused(format!(
            "{slug}: its land did not end ({why}): the next round tries again"
        )))
    }

    /// The merge armed for `journal` that did not come, for `failure`: disarmed first (one the
    /// host made meanwhile is settled instead), then said to the orchestrator. Past its bound the
    /// task stays where it is, its work not at fault; otherwise it goes back to in_progress for
    /// its executor, or, where it cannot, a task is filed for it ([`FAILED`]).
    fn undelivered(&mut self, mut journal: Journal, failure: Failure) -> Result<String, Stop> {
        let slug = self.slug;
        let default = self.settings.default_branch.clone();
        let Some(binding) = journal.binding.clone() else {
            return Ok(format!("{slug} is bound to no pull request"));
        };
        if !matches!(failure, Failure::Closed) {
            match self.disarm(&journal) {
                Disarm::Off => {}
                Disarm::Merged => return self.merged(journal, None),
                Disarm::Not(line) => return Ok(line),
            }
        }
        let why = match &failure {
            Failure::Red(checks) => format!(
                "{} on its head {}, which {} merges past none of: {}",
                if checks.len() == 1 {
                    "a check that is not the Symphony's is red"
                } else {
                    "checks that are not the Symphony's are red"
                },
                short(&journal.head),
                binding.name,
                checks.join(", ")
            ),
            Failure::Conflict => format!(
                "{} reports pull request #{} in conflict with {default}",
                binding.name, binding.pull
            ),
            Failure::Closed => format!("pull request #{} was closed without a merge", binding.pull),
            Failure::Bound(minutes) => format!(
                "{} neither merged pull request #{} nor reported it red within {minutes} min of \
                 its evidence standing (`[land] merge_within_minutes`)",
                binding.name, binding.pull
            ),
        };
        if matches!(failure, Failure::Closed) {
            // A pull request closed is one no dispatch publishes to again: the next opens another.
            Journal::drop_in(&self.folder, slug);
        } else {
            journal.stage = Stage::Eligible;
            journal.green = None;
            journal.armed = false;
            journal.write(&self.folder).map_err(Stop::Refused)?;
        }
        let line = format!("land {slug}: not merged: {why}; nothing merged, its auto-merge off");
        if matches!(failure, Failure::Bound(_)) {
            let status = self.store.get(slug)?.status;
            self.post(
                &format!(
                    "{line}. The work is not at fault: {slug} stays {status}, and `just land \
                     {slug}` arms it again"
                ),
                Some("@orchestrator"),
            );
            return Ok(line);
        }
        match self.back_to_work() {
            Ok(()) => {
                self.post(
                    &format!(
                        "{line}; {slug} is back in progress. Its executor merges {default} into \
                         the branch once, or puts the check right, then moves it to review, which \
                         dispatches it again"
                    ),
                    Some("@orchestrator"),
                );
                Ok(line)
            }
            Err(cannot) => {
                let filed = self.file_failed(&line, &cannot);
                Ok(format!("{line}; {filed}"))
            }
        }
    }

    /// The host's merge of the pull request `journal` binds, armed, turned off.
    fn disarm(&mut self, journal: &Journal) -> Disarm {
        let slug = self.slug;
        let Some(binding) = journal.binding.clone() else {
            return Disarm::Off;
        };
        let request = json!({
            "verb": "disarm", "slug": slug,
            "repository": binding.repository, "pull": binding.pull,
        });
        let not = |why: &str| {
            Disarm::Not(format!(
                "{slug}: pull request #{} was not disarmed ({why}): the next round tries again",
                binding.pull
            ))
        };
        match self.ask("disarm", &request) {
            Ok(answer) => match delivery::state(&answer) {
                ("disarmed", _) => Disarm::Off,
                ("merged", _) => Disarm::Merged,
                (said, "") => not(said),
                (_, why) => not(why),
            },
            Err(why) => not(&why),
        }
    }

    /// The pull request `journal` binds, merged by the host: settled as a land settles a merge,
    /// the task closed from where it stands, `also` among them.
    fn merged(&mut self, mut journal: Journal, also: Option<&str>) -> Result<String, Stop> {
        if let Some(status) = also.filter(|status| !journal.closes.iter().any(|s| s == status)) {
            journal.closes.push(status.to_string());
        }
        journal.stage = Stage::MergeRequested;
        journal.write(&self.folder).map_err(Stop::Refused)?;
        self.settle(journal, None)
    }

    /// The host's merge of this task, armed, turned off as it moved back to `status`, where no
    /// merge closes it: armed again once it moves to review. [`Left::Off`] once it is, which the
    /// caller says ([`off`]); [`Left::Ended`] where it ends there: merged by the host meanwhile,
    /// settled, the task closed from `status` as well; or not turned off, still armed and read
    /// again by the watcher's next round.
    fn left_review(&mut self, mut journal: Journal, status: &str) -> Result<Left, Stop> {
        match self.disarm(&journal) {
            Disarm::Off => {}
            Disarm::Merged => return self.merged(journal, Some(status)).map(Left::Ended),
            Disarm::Not(line) => {
                if let Some(said) = watch_start(self.env, &self.main) {
                    self.say(&format!("land {}: {said}", self.slug));
                }
                return Ok(Left::Ended(line));
            }
        }
        journal.stage = Stage::Eligible;
        journal.green = None;
        journal.armed = false;
        journal.write(&self.folder).map_err(Stop::Refused)?;
        Ok(Left::Off)
    }

    /// The task back in progress, from review, blocked or done (by review, as a done task goes
    /// only there), moved as its orchestrator ([`acting`]), whoever's write started a dispatch
    /// that found its pull request closed: why it cannot, for a task that cannot go back or that
    /// one of the project's own makers remakes.
    fn back_to_work(&self) -> Result<(), String> {
        let slug = self.slug;
        if slug.starts_with(GENERATED) {
            return Err(format!(
                "{slug} is the regenerator's, which cancels and makes it again"
            ));
        }
        if slug.starts_with(OWNER_SAVES) {
            return Err(format!(
                "{slug} is a document save, which stays pending and shown"
            ));
        }
        let task = self.store.get(slug).map_err(|error| error.to_string())?;
        let mover = acting(Some(&task), self.actor);
        let mut status = task.status;
        if status == "done" {
            (self.store.move_to(slug, "review", &mover))
                .map_err(|error| format!("{slug} is done and does not go back: {error}"))?;
            status = "review".into();
        }
        match status.as_str() {
            "in_progress" => Ok(()),
            "review" | "blocked" => (self.store.move_to(slug, "in_progress", &mover))
                .map(drop)
                .map_err(|error| format!("{slug} does not go back to in_progress: {error}")),
            other => Err(format!("{slug} is {other}, which does not go back to work")),
        }
    }

    /// A task filed for the merge of this one that failed with `line`, as it cannot go back
    /// (`cannot`): the line that says so. Filed once: a second failure adds a line to the one
    /// open.
    fn file_failed(&self, line: &str, cannot: &str) -> String {
        let slug = self.slug;
        let filed = format!("{FAILED}{}", slug.replace('/', "-"));
        let filer = match self.store.register(&NewSession::new(red::FILER)) {
            Ok(session) => session.row.handle,
            Err(error) => return format!("no task was filed for it: {error}"),
        };
        let said = match self.store.get(&filed) {
            Ok(open) if red::open(&open.status) => self
                .store
                .say(&filed, &filer, line, Some("@orchestrator"))
                .map(|_| format!("a line added to {filed}")),
            _ => self
                .store
                .create(&NewTask {
                    slug: filed.clone(),
                    title: format!("The merge of {slug} by pull request failed"),
                    by: filer.clone(),
                    body: format!(
                        "**What failed.** {line}.\n\n**Why a task of its own.** {cannot}.\n\n\
                         **Done when.** What the line names is put right and {slug}'s work is \
                         delivered, or it is said why it need not be."
                    ),
                    priority: 1,
                    ..NewTask::default()
                })
                .map(|_| {
                    let _ = self.store.give_filed(&filed, &filer, None);
                    format!("filed {filed}")
                }),
        };
        let _ = self.store.leave(&filer);
        match said {
            Ok(said) => {
                self.post(&format!("{line}; {cannot}: {said}"), Some("@orchestrator"));
                said
            }
            Err(error) => format!("no task was filed for it: {error}"),
        }
    }

    /// The task, cancelled: the pull request `journal` binds it to closed unmerged and its merge
    /// disarmed, the branch left; one the host merged meanwhile is held for a person, as nothing
    /// closes a cancelled task. Nothing with no pull request.
    fn cancelled(&mut self, journal: Option<Journal>) -> Result<String, Stop> {
        let slug = self.slug;
        unmark(&self.folder, slug);
        unsaid(&self.folder, slug);
        let Some(journal) = journal else {
            return Ok(format!("{slug} is cancelled: nothing to close"));
        };
        let Some(binding) = journal.binding.clone() else {
            Journal::drop_in(&self.folder, slug);
            return Ok(format!("{slug} is cancelled: nothing to close"));
        };
        let request = json!({
            "verb": "close", "slug": slug,
            "repository": binding.repository, "pull": binding.pull,
        });
        match self.ask("close", &request) {
            Ok(answer) if delivery::state(&answer).0 == "closed" => {
                Journal::drop_in(&self.folder, slug);
                let line = format!(
                    "land {slug}: pull request #{} of {} closed without a merge, as {slug} was \
                     cancelled; its branch is left",
                    binding.pull, binding.name
                );
                self.post(&line, None);
                Ok(line)
            }
            Ok(answer) if delivery::state(&answer).0 == "merged" => Err(self.hold(
                journal,
                format!(
                    "pull request #{} was merged while {slug} was cancelled",
                    binding.pull
                ),
            )),
            Ok(answer) => Ok(self.not_closed(&journal, delivery::state(&answer).1)),
            Err(why) => Ok(self.not_closed(&journal, &why)),
        }
    }

    /// The pull request of a cancelled task the host did not close, for `why`: armed, the
    /// watcher's next round asks again, a watcher started where none runs; not armed, nothing
    /// merges it and no round reads it, so the orchestrator is told how to close it.
    fn not_closed(&mut self, journal: &Journal, why: &str) -> String {
        let slug = self.slug;
        let pull = journal.binding.as_ref().map_or(0, |binding| binding.pull);
        let line = format!("{slug} is cancelled and pull request #{pull} was not closed ({why})");
        if journal.watched() {
            if let Some(said) = watch_start(self.env, &self.main) {
                self.say(&format!("land {slug}: {said}"));
            }
            return format!("{line}: the next round tries again");
        }
        let line = format!(
            "land {slug}: {line}; it is not armed, so nothing merges it, and `just land {slug} \
             --dispatch` closes it"
        );
        self.post(&line, Some("@orchestrator"));
        line
    }
}

/// The slugs a document save is delivered under, each a task of its own (the Symphony's
/// `commit::pending::BRANCHES`, which this crate does not link).
const OWNER_SAVES: &str = "docs/owner-save-";
/// The slugs the regenerator files what it made under ([`super::regenerate`]).
const GENERATED: &str = "generated/outputs-at-";
