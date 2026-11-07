//! Launch, restore and clean up board agent sessions in hc terminals, shown in the Herdr app.
//!
//! [`launch`] registers a handle, makes the worktree under `<repo>-wt/` (an executor on its task's
//! branch: its slug; an instruction or reviewer on a detached checkout of main) unless
//! `--worktree` gives one, and starts Codex by default, or Claude when selected, in an hc terminal
//! of its own (`hc run`), then gives it its first message (`$work SLUG HANDLE`; Claude: `/work`)
//! once it is ready. The terminal outlives whoever launched it (`--outlive-parent`), is kept
//! (`--keep`), and shows in the stream tab of its role (`code` for an executor, `review` for a
//! reviewer, or `--tab`; at most four terminals, then `code-2`) of the project's space in the Herdr
//! app, found by name and made when missing (the labels `herdr.space` and `herdr.tab`): never one of
//! the owner's tabs. The launch is confirmed only by hc's receipt of that message (`startup`); one
//! that cannot be confirmed fails, naming the terminal, which is killed and removed. The launch is
//! recorded on the task: handle, role, conversation id, worktree, branch, terminal, model, start
//! time. [`restore`] reopens the recorded conversation in its worktree, in a new terminal, the same
//! way. [`cleanup`] closes unneeded terminals (killed, then removed), each closure recorded on its
//! task with the restore line; [`finish`] is the same for one task that just became done or
//! cancelled (its worktree is the worktree sweep's, `tasks sweep --task`). [`reap`] is the periodic
//! sweep of idle agents. [`close`] closes one agent's terminal on its orchestrator's word, whatever
//! it holds, recorded the same way, under the rules its module gives.
//!
//! A terminal is closed only when all of these hold: it is this board's (`sf.board`); its agent is
//! idle (or it is a shell with nothing in it, or its program ended), its foreground is the program it
//! started (an agent, or for no agent a shell: a program a shell became is no idle shell) and has no
//! process running under it other than the agent and its helpers (a shell, a monitor, anything);
//! the last `hc get` before the kill finds that same program, still finished; no `verify`, test run,
//! `land` or Symphony (`just symphony`) runs there; it is not the calling terminal
//! (`HC_TERMINAL_ID`), an orchestrator's, or on screen in the Herdr app ([`screen`]); and the
//! session it belongs to is on finished tasks only or holds nothing live. An agent terminal that
//! matches no board session (by recorded terminal, `sf.handle`, `-n HANDLE` or its name) is kept:
//! closing it would be a guess. A recorded pane hc does not run (a herdr pane id from before hc,
//! `w1:p…`) is gone: cleanup records it closed, with its restore line.
//!
//! Every hc call goes through [`hc::Hc`], whose binaries are `$HC_BIN` (default `hc`) and
//! `$HERDR_APP_BIN` (default `herdr-app`); the tests (`tests/launch/`) point both at `fake-hc`.

mod cleanup;
mod close;
pub mod hc;
mod place;
pub mod procs;
pub mod reap;
pub mod screen;
pub mod staff;
mod startup;

pub use cleanup::{Closure, cleanup, finish};
pub use close::close;
use place::{name_place, place, stream};

use crate::cli::{Env, probes};
use crate::health::{FINISHED, RESERVE_SECONDS};
use crate::rows::TaskSessionRow;
use crate::sys;
use crate::{Error, NewLaunch, NewSession, Store};
use hc::{Hc, text_of};
use serde_json::Value;
use std::collections::HashSet;
use std::fmt;
use std::hash::{BuildHasher, RandomState};
use std::io::Read;
use std::path::{Path, PathBuf};

/// At most this many terminals in a stream's tab; the next goes to the stream's next tab.
pub const MAX_PANES: usize = 4;
/// An agent waiting for input; `working`, `blocked` and `error` are never closed.
pub const READY_STATES: [&str; 2] = ["idle", "done"];
pub const PERMISSION_MODE: &str = "auto";
/// After a launch: how long to wait before each look at hc's record of the terminal, and how many
/// looks the launch is given (a minute) before it fails.
pub const PROMPT_WAIT: f64 = 2.0;
pub const CONFIRM_LOOKS: usize = 30;
/// The looks a message sent to an idle agent is given before it is reported unconfirmed.
pub const SEND_LOOKS: usize = 3;
/// Why a record whose pane is gone is closed.
pub const GONE: &str = "its pane is gone";

/// An hc or git step failed; whatever it had made is undone as far as it can be. A refusal, at
/// the command line: exit 1.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchError(pub String);

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<LaunchError> for Error {
    fn from(error: LaunchError) -> Error {
        Error::Refused(error.0)
    }
}

/// The processes running under a pid (`procs::descendants`, `procs::table`'s, or a test's table).
pub type Processes = dyn Fn(i64) -> Result<Vec<String>, LaunchError>;

/// What the launcher runs with besides the board and hc: the main checkout and the folder of
/// worktrees beside it, the session ids, the waits, the process table and the board a launched
/// agent is to reach.
pub struct Machine {
    /// The main checkout, whichever worktree the command runs in. Its folder's name is the Herdr
    /// app space's.
    pub root: PathBuf,
    /// Where new worktrees go: `SF_WT_DIR`, else `<repo>-wt/` beside the main checkout.
    pub wt_base: PathBuf,
    pub new_id: Box<dyn Fn() -> String>,
    pub sleep: Box<dyn Fn(f64)>,
    pub processes: Box<Processes>,
    pub mode: String,
    /// What a launched agent is given besides `TASKS_SESSION` to reach this board: the caller's
    /// `TASKS_DATABASE` and `TASKS_SERVER`, when it has them. hc starts programs in the login
    /// session's environment, not the caller's.
    pub board_env: Vec<(String, String)>,
}

impl Machine {
    /// This machine's, for a command run in `env`'s working directory.
    pub fn of(env: &Env) -> Machine {
        let root = probes::repo_root(&env.cwd).unwrap_or_else(|| env.cwd.clone());
        let wt_base = worktree_base(&root, env);
        Machine {
            root,
            wt_base,
            new_id: Box::new(new_uuid),
            sleep: Box::new(|seconds| {
                std::thread::sleep(
                    std::time::Duration::try_from_secs_f64(seconds).unwrap_or_default(),
                )
            }),
            processes: procs::table(env),
            mode: PERMISSION_MODE.into(),
            board_env: board_env(env),
        }
    }

    /// The Herdr app space the board's terminals show in: the main checkout's folder name.
    fn project(&self) -> String {
        self.root
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    }
}

/// The caller's `TASKS_DATABASE` and `TASKS_SERVER`, those it has: what a launched agent needs to
/// reach the same board.
pub fn board_env(env: &Env) -> Vec<(String, String)> {
    ["TASKS_DATABASE", "TASKS_SERVER"]
        .into_iter()
        .filter_map(|key| Some((key.to_string(), env.var(key)?.to_string())))
        .collect()
}

/// `SF_WT_DIR`, else `<repo>-wt/` beside the main checkout.
pub fn worktree_base(root: &Path, env: &Env) -> PathBuf {
    match env.var("SF_WT_DIR") {
        Some(named) => PathBuf::from(named),
        None => {
            let name = root.file_name().map(|n| n.to_string_lossy().into_owned());
            root.parent()
                .unwrap_or(root)
                .join(format!("{}-wt", name.unwrap_or_default()))
        }
    }
}

/// What `just tasks launch` asks for, as its parser read it.
#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    pub slug: String,
    /// Who launches it: the launched session's parent.
    pub actor: String,
    pub model: String,
    pub kind: Option<String>,
    pub role: String,
    pub branch: Option<String>,
    pub detached: bool,
    /// A worktree to use as it is, rather than one made under `<repo>-wt/`.
    pub worktree: Option<PathBuf>,
    pub prompt: String,
    pub tab: Option<String>,
}

impl Ask {
    pub fn new(slug: &str, actor: &str, model: &str) -> Ask {
        Ask {
            slug: slug.into(),
            actor: actor.into(),
            model: model.into(),
            kind: None,
            role: "executor".into(),
            branch: None,
            detached: false,
            worktree: None,
            prompt: String::new(),
            tab: None,
        }
    }
}

/// A session whose first message was confirmed in its terminal: an unconfirmed launch is an error,
/// never a session record.
#[derive(Debug, Clone, PartialEq)]
pub struct Started {
    pub row: TaskSessionRow,
}

/// Start a session for `ask.slug`: handle, worktree, terminal with its agent and `/work SLUG
/// HANDLE`, and the record on the task.
pub fn launch(store: &Store, hc: &dyn Hc, m: &Machine, ask: &Ask) -> Result<Started, Error> {
    let (slug, role) = (ask.slug.as_str(), ask.role.as_str());
    if role != "executor" && role != "reviewer" {
        return Err(Error::Refused(format!(
            "a launched session is an executor or a reviewer, not {}",
            crate::pyjson::repr(role)
        )));
    }
    store.touch(&ask.actor)?;
    let task = store.get(slug)?;
    if FINISHED.contains(&task.status.as_str()) {
        return Err(Error::Refused(format!("{slug} is {}", task.status)));
    }
    let dashed = slug.replace('/', "-");
    let made_worktree = ask.worktree.is_none();
    let detach = ask.detached || ((role == "reviewer" || task.instruction) && ask.branch.is_none());
    let (path, use_branch) = match &ask.worktree {
        None => {
            let mut path = m.wt_base.join(&dashed);
            if role == "reviewer" {
                // beside the executor's, never in it
                path = m.wt_base.join(format!("{dashed}-review"));
                for n in 2..100 {
                    if !path.exists() {
                        break;
                    }
                    path = m.wt_base.join(format!("{dashed}-review-{n}"));
                }
            }
            if path.exists() {
                return Err(LaunchError(format!(
                    "{} exists: give it with --worktree, or remove it",
                    path.display()
                ))
                .into());
            }
            // Each dispatched task owns its checkout; instructions and reviews are detached.
            let use_branch = if detach {
                String::new()
            } else {
                match ask.branch.as_deref().filter(|b| !b.is_empty()) {
                    Some(branch) => branch.to_string(),
                    None => slug.to_string(),
                }
            };
            if !use_branch.is_empty()
                && let Some(held) = checked_out_in(&m.root, &use_branch)?
            {
                return Err(LaunchError(format!(
                    "branch {use_branch} is checked out in {held}: to continue it, launch with --worktree {held}; \
                     --branch <other> starts another branch; --detached is for read-only work"
                ))
                .into());
            }
            (path, use_branch)
        }
        Some(given) => {
            if !given.is_dir() {
                return Err(LaunchError(format!(
                    "--worktree {} is not a directory",
                    given.display()
                ))
                .into());
            }
            let path = given.canonicalize().unwrap_or_else(|_| given.clone());
            if let Some(handle) = live_executor_in(store, &path)? {
                return Err(Error::Refused(format!(
                    "{} has live executor {handle}: launch in a separate worktree",
                    path.display()
                )));
            }
            let branch = probes::branch_of(&path.to_string_lossy()).unwrap_or_default();
            (path, branch)
        }
    };
    let where_ = path.to_string_lossy().into_owned();
    let kind = ask
        .kind
        .clone()
        .filter(|k| k == "worker" || k == "reviewer")
        .unwrap_or_else(|| {
            (if role == "reviewer" {
                "reviewer"
            } else {
                "worker"
            })
            .into()
        });
    let session = store.register(&NewSession {
        kind,
        parent: Some(ask.actor.clone()),
        worktree: where_.clone(),
        branch: use_branch.clone(),
        model: ask.model.clone(),
        ..NewSession::default()
    })?;
    let handle = session.row.handle;
    if let Err(problem) = take_up(
        store,
        m,
        ask,
        &task,
        &handle,
        made_worktree.then_some(&path),
        detach,
        &use_branch,
    ) {
        if matches!(problem, Error::Refused(_)) {
            store.leave(&handle)?;
        }
        return Err(problem);
    }
    let mut terminal = String::new();
    let tab = ask.tab.clone().filter(|t| !t.is_empty());
    let agent = agent_kind(ask);
    let conversation = (m.new_id)();
    let args = if agent == "codex" {
        codex_args(&ask.model, &handle, m)
    } else {
        claude_args(
            &ask.model,
            &handle,
            &m.mode,
            ["--session-id", &conversation],
        )
    };
    let started = start_confirmed(
        hc,
        m,
        &mut terminal,
        &Start {
            cwd: &where_,
            handle: &handle,
            stream: tab.as_deref().unwrap_or(stream(role)),
            agent,
            args,
            text: &prompt_text(slug, &handle, &ask.prompt, agent),
        },
    );
    if let Err(problem) = started {
        if !terminal.is_empty() {
            let _ = hc.close(&terminal);
        }
        store.leave(&handle)?;
        if made_worktree && path.exists() {
            let _ = sys::output(
                sys::command("git")
                    .arg("-C")
                    .arg(&m.root)
                    .args(["worktree", "remove", "--force"])
                    .arg(&path),
            );
        }
        return Err(problem.into());
    }
    let claude_session = if agent == "codex" {
        // The thread is known once the agent registers its callback (`just tasks callback`).
        "codex:".to_string()
    } else {
        conversation
    };
    let row = store.record_launch(&NewLaunch {
        slug: slug.into(),
        handle,
        role: role.into(),
        claude_session,
        worktree: where_,
        branch: use_branch,
        pane: terminal,
        model: ask.model.clone(),
        ..NewLaunch::default()
    })?;
    Ok(Started { row })
}

/// A checkout with unfinished work held by a present executor cannot host another agent.
fn live_executor_in(store: &Store, path: &Path) -> Result<Option<String>, Error> {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let board = store.board()?;
    let records = store.all_task_sessions()?;
    for task in board
        .tasks
        .iter()
        .filter(|t| !FINISHED.contains(&t.status.as_str()))
    {
        for role in task
            .roles
            .iter()
            .filter(|r| r.role == "executor" && r.health.holds())
        {
            let worktree = records
                .iter()
                .rev()
                .find(|r| r.slug == task.slug && r.handle == role.handle)
                .map(|r| r.worktree.as_str())
                .or_else(|| {
                    board
                        .sessions
                        .iter()
                        .find(|s| s.row.handle == role.handle)
                        .map(|s| s.row.worktree.as_str())
                });
            if let Some(worktree) = worktree.filter(|w| !w.is_empty())
                && Path::new(worktree).canonicalize().is_ok_and(|w| w == path)
            {
                return Ok(Some(role.handle.clone()));
            }
        }
    }
    Ok(None)
}

/// `/work SLUG HANDLE`, then the prompt given, if any. That is the whole prompt (the work skill's
/// rule): the skill carries the rest, subagents and forks forbidden among it.
pub(super) fn prompt_text(slug: &str, handle: &str, prompt: &str, agent: &str) -> String {
    let work = if agent == "codex" { "$work" } else { "/work" };
    match prompt.trim() {
        "" => format!("{work} {slug} {handle}"),
        more => format!("{work} {slug} {handle} {more}"),
    }
}

fn agent_kind(ask: &Ask) -> &'static str {
    match ask.kind.as_deref() {
        Some("claude" | "worker" | "reviewer") => "claude",
        _ => "codex",
    }
}

/// The new session takes its part on the task (an executor reserves it; a review subtask's reviewer
/// reserves the subtask and judges its parent; any other reviewer takes the reviewer role), and its
/// worktree is made when the launch makes one.
#[expect(
    clippy::too_many_arguments,
    reason = "one step of `launch`, named by what it reads"
)]
fn take_up(
    store: &Store,
    m: &Machine,
    ask: &Ask,
    task: &crate::Task,
    handle: &str,
    made: Option<&PathBuf>,
    detach: bool,
    use_branch: &str,
) -> Result<(), Error> {
    let slug = ask.slug.as_str();
    if ask.role == "executor" {
        store.reserve(slug, handle, RESERVE_SECONDS)?;
    } else if let Some(parent) = task.parent.as_deref().filter(|_| task.instruction) {
        // A review subtask: its reviewer works it as an executor (`/work` claims it) and judges the
        // task it reviews, so the reviewer role goes on the parent, never on the subtask it must claim.
        store.reserve(slug, handle, RESERVE_SECONDS)?;
        match store.take_role(parent, handle, "reviewer") {
            Ok(_) | Err(Error::Refused(_)) => {} // the parent is not open for review: the reviewer says so
            Err(problem) => return Err(problem),
        }
    } else {
        store.take_role(slug, handle, "reviewer")?;
    }
    let Some(path) = made else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| LaunchError(format!("cannot make {}: {e}", parent.display())))?;
    }
    let path = path.to_string_lossy();
    if detach {
        git(&m.root, &["worktree", "add", "--detach", &path, "main"])?;
    } else if branch_exists(&m.root, use_branch) {
        git(&m.root, &["worktree", "add", &path, use_branch])?;
    } else {
        git(
            &m.root,
            &["worktree", "add", "-b", use_branch, &path, "main"],
        )?;
    }
    Ok(())
}

/// What one terminal starts: in `cwd`, for `handle`, in the tab of `stream`, the `agent` (`claude`
/// or `codex`) with its `args`, and `text` its first message. It borrows them from the launch or
/// restore that fills it.
struct Start<'launch> {
    cwd: &'launch str,
    handle: &'launch str,
    stream: &'launch str,
    agent: &'static str,
    args: Vec<String>,
    text: &'launch str,
}

/// A terminal for `start.handle`, placed in its stream's tab, its agent started and given its first
/// message once ready, and hc's receipt of that message taken (`startup`). The terminal, once there
/// is one, is left in `terminal` for the caller to close if this fails.
fn start_confirmed(
    hc: &dyn Hc,
    m: &Machine,
    terminal: &mut String,
    start: &Start<'_>,
) -> Result<(), LaunchError> {
    let mut place = place(hc, &m.project(), start.stream)?;
    let mut argv = vec!["env".to_string(), format!("TASKS_SESSION={}", start.handle)];
    argv.extend(m.board_env.iter().map(|(k, v)| format!("{k}={v}")));
    argv.push(start.agent.into());
    argv.extend(start.args.iter().cloned());
    let labels = [
        format!("herdr.space={}", place.space),
        format!("herdr.tab={}", place.tab),
        format!("{}={}", hc::BOARD_LABEL, hc.board()),
        format!("{}={}", hc::HANDLE_LABEL, start.handle),
    ];
    let mut call = vec!["run", "--bg", "--name", start.handle, "--cwd", start.cwd];
    for label in &labels {
        call.extend(["--label", label]);
    }
    // The board's, not the caller's: a child of the caller's terminal would show under it, never
    // in the stream's tab.
    call.extend([
        "--no-parent",
        "--outlive-parent",
        "--keep",
        "--no-notify",
        "--",
    ]);
    call.extend(argv.iter().map(String::as_str));
    let made = hc.json(&call)?;
    *terminal = made
        .get("address")
        .and_then(Value::as_str)
        .filter(|a| !a.is_empty())
        .ok_or_else(|| LaunchError("hc run answered no terminal address".into()))?
        .to_string();
    startup::first_message(
        hc,
        terminal,
        start.agent,
        start.text,
        &mut || name_place(hc, &mut place),
        &*m.sleep,
    )
}

/// Reopen every closed session of `slug` (or `handle`'s), and every open one whose pane is gone,
/// where it was, in a new terminal, resuming its conversation.
pub fn restore(
    store: &Store,
    hc: &dyn Hc,
    m: &Machine,
    slug: &str,
    handle: Option<&str>,
    tab: Option<&str>,
) -> Result<Vec<Started>, Error> {
    let recorded = store.task_sessions(slug)?;
    if let Some(handle) = handle
        && !recorded.iter().any(|s| s.handle == handle)
    {
        return Err(Error::Refused(format!(
            "{handle} has no recorded session on {slug}"
        )));
    }
    if recorded.is_empty() {
        return Err(Error::Refused(format!(
            "{slug} has no recorded sessions: it was not launched with `just tasks launch`"
        )));
    }
    let running: HashSet<String> = if recorded.iter().any(|s| s.closed.is_none()) {
        live_addresses(hc)?
    } else {
        HashSet::new()
    };
    let mut restored = Vec::new();
    for s in &recorded {
        if handle.is_some_and(|h| s.handle != h) || (s.closed.is_none() && !gone(&s.pane, &running))
        {
            continue;
        }
        // Held for the whole restore, as a close holds it: neither starts while the other runs,
        // and each of its writes lands only while it still holds it.
        let hold = store.take_session(&s.handle, "a restore")?;
        let started = restore_one(store, hc, m, slug, s, tab, hold);
        let released = store.release_session(&s.handle, hold);
        restored.push(started?);
        released?;
    }
    Ok(restored)
}

/// Restore `session` of `slug`, closed or in a pane that is gone, while it holds it.
fn restore_one(
    store: &Store,
    hc: &dyn Hc,
    machine: &Machine,
    slug: &str,
    session: &TaskSessionRow,
    tab: Option<&str>,
    hold: u64,
) -> Result<Started, Error> {
    if session.claude_session.is_empty() || session.claude_session == "codex:" {
        return Err(Error::Refused(format!(
            "{} has no recorded conversation id: its conversation cannot be resumed (start it again \
             with `just tasks launch`)",
            session.handle
        )));
    }
    if let Some(terminal) = running_as(hc, &session.handle)? {
        return Err(Error::Refused(format!(
            "{} still runs in terminal {terminal}: close it before restoring",
            session.handle
        )));
    }
    if let Some(handle) = live_executor_in(store, Path::new(&session.worktree))?
        && handle != session.handle
    {
        return Err(Error::Refused(format!(
            "{} has live executor {handle}: cannot restore another agent there",
            session.worktree
        )));
    }
    if session.closed.is_none() {
        // A pane that is gone (a herdr pane from before hc): its closure is said first, once:
        // a close that recorded it meanwhile said it already.
        store.close_task_session_as(
            session,
            GONE,
            &cleanup::restore_line(slug, &session.handle),
            hold,
        )?;
    }
    if !Path::new(&session.worktree).is_dir() {
        if !session.branch.is_empty() && branch_exists(&machine.root, &session.branch) {
            git(
                &machine.root,
                &["worktree", "add", &session.worktree, &session.branch],
            )?;
        } else {
            git(
                &machine.root,
                &["worktree", "add", "--detach", &session.worktree, "main"],
            )?;
        }
    }
    let text = format!(
        "You were restored after your pane was closed. First run `just tasks callback --as {}`, then `just tasks inbox --as {}` and continue /work {slug}.",
        session.handle, session.handle
    );
    let (agent, args) = if let Some(id) = session.claude_session.strip_prefix("codex:") {
        let mut args = codex_args(&session.model, &session.handle, machine);
        args.extend(["resume".into(), id.into()]);
        ("codex", args)
    } else {
        (
            "claude",
            claude_args(
                &session.model,
                &session.handle,
                &machine.mode,
                ["--resume", &session.claude_session],
            ),
        )
    };
    if !store.renew_session(&session.handle, hold)? {
        return Err(Error::Refused(format!(
            "another close or a restore of {} took its session over, so the restore starts no \
             terminal",
            session.handle
        )));
    }
    let mut terminal = String::new();
    let started = start_confirmed(
        hc,
        machine,
        &mut terminal,
        &Start {
            cwd: &session.worktree,
            handle: &session.handle,
            stream: tab
                .filter(|t| !t.is_empty())
                .unwrap_or(stream(&session.role)),
            agent,
            args,
            text: &text,
        },
    );
    if let Err(problem) = started {
        if !terminal.is_empty() {
            let _ = hc.close(&terminal); // the problem is what the caller must hear
        }
        return Err(problem.into());
    }
    // Reopened and revived only while the restore still holds the session and the record is the
    // one read: else the record says what holds, the terminal this one started goes, and it
    // records nothing.
    let reopened = store.reopen_task_session_as(session, &terminal, hold);
    if !matches!(reopened, Ok(true)) {
        let _ = hc.close(&terminal); // the problem is what the caller must hear
        let why = match reopened {
            Err(Error::Refused(why)) => why,
            Err(problem) => return Err(problem),
            Ok(_) => format!("{}'s record changed", session.handle),
        };
        return Err(Error::Refused(format!(
            "{why} while it was restored, so the restore records nothing and terminal \
             {terminal}, which it started, is killed"
        )));
    }
    let row = store
        .task_sessions(slug)?
        .into_iter()
        .find(|x| x.handle == session.handle)
        .ok_or_else(|| {
            Error::Refused(format!(
                "{} has no recorded session on {slug}",
                session.handle
            ))
        })?;
    Ok(Started { row })
}

/// The addresses of this board's live terminals.
fn live_addresses(hc: &dyn Hc) -> Result<HashSet<String>, LaunchError> {
    Ok(hc
        .terminals()?
        .iter()
        .filter(|t| text_of(t, "state") == "live")
        .map(|t| text_of(t, "address"))
        .collect())
}

/// Whether a recorded pane is gone: a herdr pane id (`w1:p3`: herdr is no more), or a terminal of
/// this board hc no longer runs. A record naming no pane says nothing either way.
pub(crate) fn gone(pane: &str, running: &HashSet<String>) -> bool {
    !pane.is_empty() && (pane.contains(':') || !running.contains(pane))
}

/// The live terminal `handle` still runs in, by its name or its `sf.handle` label, if any.
fn running_as(hc: &dyn Hc, handle: &str) -> Result<Option<String>, LaunchError> {
    let labelled = hc.listed(&[format!("{}={handle}", hc::HANDLE_LABEL)], false)?;
    let named = hc.live()?;
    Ok(labelled
        .iter()
        .chain(named.iter().filter(|t| text_of(t, "name") == handle))
        .map(|t| text_of(t, "address"))
        .next())
}

fn codex_args(model: &str, handle: &str, m: &Machine) -> Vec<String> {
    let mut args: Vec<String> = [
        "--no-daemon",
        "-s",
        "workspace-write",
        "-m",
        if model.is_empty() {
            "gpt-6.1-sol"
        } else {
            model
        },
        "-c",
        "model_reasoning_effort=\"xhigh\"",
        "-c",
        "sandbox_workspace_write.network_access=true",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let set = std::iter::once(("TASKS_SESSION".to_string(), handle.to_string()))
        .chain(m.board_env.iter().cloned());
    for (key, value) in set {
        args.extend([
            "-c".into(),
            format!("shell_environment_policy.set.{key}=\"{value}\""),
        ]);
    }
    args
}

fn claude_args(model: &str, handle: &str, mode: &str, identity: [&str; 2]) -> Vec<String> {
    let mut args: Vec<String> = identity.iter().map(|s| s.to_string()).collect();
    args.extend(["-n", handle, "--permission-mode", mode].map(String::from));
    if !model.is_empty() {
        args.extend(["--model".to_string(), model.to_string()]);
    }
    args
}

// -- git -------------------------------------------------------------------------------------

fn git(root: &Path, args: &[&str]) -> Result<String, LaunchError> {
    let done = sys::output(sys::command("git").arg("-C").arg(root).args(args))
        .map_err(|e| LaunchError(format!("git {}: {e}", args[..2.min(args.len())].join(" "))))?;
    if !done.status.success() {
        return Err(LaunchError(format!(
            "git {}: {}",
            args[..2.min(args.len())].join(" "),
            String::from_utf8_lossy(&done.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&done.stdout).trim().to_string())
}

fn branch_exists(root: &Path, branch: &str) -> bool {
    sys::output(sys::command("git").arg("-C").arg(root).args([
        "rev-parse",
        "--verify",
        "-q",
        &format!("refs/heads/{branch}"),
    ]))
    .is_ok_and(|done| done.status.success())
}

/// The worktree that has `branch` checked out, if any.
fn checked_out_in(root: &Path, branch: &str) -> Result<Option<String>, LaunchError> {
    let mut at = "";
    let listed = git(root, &["worktree", "list", "--porcelain"])?;
    let wanted = format!("branch refs/heads/{branch}");
    for line in listed.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            at = path;
        } else if line == wanted {
            return Ok(Some(at.to_string()));
        }
    }
    Ok(None)
}

/// A random (version 4) UUID: the id a new Claude session is started with.
pub fn new_uuid() -> String {
    let mut bytes = [0u8; 16];
    let read = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes));
    if read.is_err() {
        let state = RandomState::new();
        let (a, b) = (
            state.hash_one(std::process::id()),
            state.hash_one(std::time::SystemTime::now()),
        );
        bytes[..8].copy_from_slice(&a.to_le_bytes());
        bytes[8..].copy_from_slice(&b.to_le_bytes());
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}
