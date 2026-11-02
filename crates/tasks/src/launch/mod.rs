//! Launch, restore and clean up board agent sessions in Herdr panes.
//!
//! [`launch`] registers a handle, makes the worktree under `<repo>-wt/` (an executor on its task's
//! branch: its slug; an instruction or reviewer on a detached checkout of main)
//! unless `--worktree` gives one, puts a pane in the herdr tab of its stream (`code` for an executor,
//! `review` for a reviewer, or `--tab`; at most four panes, then `code-2`) and starts Codex by default,
//! or Claude when selected. It sends `$work SLUG HANDLE` (Claude: `/work`), waits for the agent to
//! take it (pressing enter again while the text still sits in its input line), and records on the
//! task: handle, role, conversation id, worktree, branch, pane, model, start time. [`restore`]
//! reopens the recorded conversation in its worktree. [`cleanup`] closes unneeded panes, each closure
//! recorded on its task with the restore line; [`finish`] is the same for one task that just became
//! done or cancelled (its worktree is the worktree sweep's, `tasks sweep --task`). [`reap`] is the
//! periodic sweep of idle agents.
//!
//! A pane is closed only when all of these hold: its agent is idle (or it is a shell with nothing in
//! it) and has no process running under it other than the agent and its helpers (a shell, a monitor,
//! anything); no `verify`, test run, `land` or Symphony (`just symphony`) runs there; it is not the
//! calling pane, the focused pane or an orchestrator's; and the session it belongs to is on finished
//! tasks only or holds nothing live. An agent pane that matches no board session (by recorded pane,
//! Claude session id, `-n HANDLE` or its title) is kept: closing it would be a guess.
//!
//! Every herdr call goes through [`herdr::Herdr`], whose binary is `$HERDR_BIN` (default `herdr`); the
//! tests point it at the crate's `fake-herdr`. Tests: `symphony/crates/tasks/tests/launch/`.

mod cleanup;
pub mod herdr;
pub mod procs;
pub mod reap;
pub mod staff;
mod startup;

pub use cleanup::{Closure, cleanup, finish};

use crate::cli::{Env, probes};
use crate::health::{FINISHED, RESERVE_SECONDS};
use crate::rows::TaskSessionRow;
use crate::{Error, NewLaunch, NewSession, Store};
use herdr::{Herdr, Object, text};
use serde_json::Value;
use std::fmt;
use std::hash::{BuildHasher, RandomState};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// At most this many panes in a stream's tab; the next goes to the stream's next tab.
pub const MAX_PANES: usize = 4;
/// `agent start` is tried this often, every 2 s, while the new pane's shell is not ready.
pub const START_TRIES: u32 = 15;
/// An agent waiting for input; `working`, `blocked` and `unknown` are never closed.
pub const READY_STATES: [&str; 2] = ["idle", "done"];
pub const PERMISSION_MODE: &str = "auto";
/// After a prompt: how long to wait before each look at the agent, and how often enter is pressed
/// again while the text still sits in its input line.
pub const PROMPT_WAIT: f64 = 2.0;
pub const PROMPT_ENTERS: usize = 2;
/// How much of the prompt, and of the bottom of the pane, the look reads.
const PROMPT_HEAD: usize = 24;
const PROMPT_LINES: usize = 12;

/// A herdr or git step failed; whatever it had made is undone as far as it can be. A refusal, at
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

/// The processes running under a pid (`procs::descendants`, or a test's table).
pub type Processes = dyn Fn(i64) -> Result<Vec<String>, LaunchError>;

/// What the launcher runs with besides the board and herdr: the main checkout and the folder of
/// worktrees beside it, the session ids, the waits and the process table.
pub struct Machine {
    /// The main checkout, whichever worktree the command runs in.
    pub root: PathBuf,
    /// Where new worktrees go: `SF_WT_DIR`, else `<repo>-wt/` beside the main checkout.
    pub wt_base: PathBuf,
    pub new_id: Box<dyn Fn() -> String>,
    pub sleep: Box<dyn Fn(f64)>,
    pub processes: Box<Processes>,
    pub mode: String,
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
            processes: Box::new(procs::descendants),
            mode: PERMISSION_MODE.into(),
        }
    }
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

/// A session started in a pane: its record on the task, and whether its prompt still sat in its
/// input line after the last enter.
#[derive(Debug, Clone, PartialEq)]
pub struct Started {
    pub row: TaskSessionRow,
    pub stuck: bool,
}

/// The herdr tab (a stream of work) a role's pane goes to.
fn stream(role: &str) -> &'static str {
    if role == "reviewer" { "review" } else { "code" }
}

/// Start a session for `ask.slug`: handle, worktree, pane, claude, `/work SLUG HANDLE`, and the record on
/// the task.
pub fn launch(store: &Store, herdr: &dyn Herdr, m: &Machine, ask: &Ask) -> Result<Started, Error> {
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
    let mut pane = String::new();
    let tab = ask.tab.clone().filter(|t| !t.is_empty());
    let started = start_in_pane(
        herdr,
        m,
        &mut pane,
        &where_,
        &handle,
        tab.as_deref().unwrap_or(stream(role)),
        &ask.model,
        agent_kind(ask),
        &prompt_text(slug, &handle, &ask.prompt, agent_kind(ask)),
    );
    let (claude_session, stuck) = match started {
        Ok(started) => started,
        Err(problem) => {
            if !pane.is_empty() {
                let _ = herdr.close(&pane);
            }
            store.leave(&handle)?;
            if made_worktree && path.exists() {
                let _ = Command::new("git")
                    .arg("-C")
                    .arg(&m.root)
                    .args(["worktree", "remove", "--force"])
                    .arg(&path)
                    .stdin(Stdio::null())
                    .output();
            }
            return Err(problem.into());
        }
    };
    let row = store.record_launch(&NewLaunch {
        slug: slug.into(),
        handle,
        role: role.into(),
        claude_session,
        worktree: where_,
        branch: use_branch,
        pane,
        model: ask.model.clone(),
        ..NewLaunch::default()
    })?;
    Ok(Started { row, stuck })
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

fn agent_kind(ask: &Ask) -> &str {
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

/// A pane for `handle` in `cwd`, claude started in it with a session id chosen here, and `text` sent:
/// the session id, and whether the text still sat in the input line after the last enter. The pane,
/// once there is one, is left in `pane` for the caller to close if this fails.
#[expect(
    clippy::too_many_arguments,
    reason = "one step of `launch`, named by what it reads"
)]
fn start_in_pane(
    herdr: &dyn Herdr,
    m: &Machine,
    pane: &mut String,
    cwd: &str,
    handle: &str,
    stream: &str,
    model: &str,
    agent: &str,
    text: &str,
) -> Result<(String, bool), LaunchError> {
    *pane = pane_for(herdr, cwd, handle, stream)?;
    let claude_session = (m.new_id)();
    let args = if agent == "codex" {
        codex_args(model, handle)
    } else {
        claude_args(model, handle, &m.mode, ["--session-id", &claude_session])
    };
    start(herdr, handle, pane, agent, &args, &*m.sleep)?;
    herdr.call(&["agent", "prompt", handle, text])?;
    let (taken, session) = taken(herdr, pane, text, &*m.sleep);
    let stuck = !taken;
    let identity = if agent == "codex" {
        format!("codex:{session}")
    } else {
        claude_session
    };
    Ok((identity, stuck))
}

/// Reopen every closed session of `slug` (or `handle`'s) where it was and resume its Claude
/// conversation.
pub fn restore(
    store: &Store,
    herdr: &dyn Herdr,
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
    let mut restored = Vec::new();
    for s in &recorded {
        if s.closed.is_none() || handle.is_some_and(|h| s.handle != h) {
            continue;
        }
        if s.claude_session.is_empty() || s.claude_session == "codex:" {
            return Err(Error::Refused(format!(
                "{} has no recorded conversation id: its conversation cannot be resumed (start it again \
                 with `just tasks launch`)",
                s.handle
            )));
        }
        if let Some(handle) = live_executor_in(store, Path::new(&s.worktree))?
            && handle != s.handle
        {
            return Err(Error::Refused(format!(
                "{} has live executor {handle}: cannot restore another agent there",
                s.worktree
            )));
        }
        if !Path::new(&s.worktree).is_dir() {
            if !s.branch.is_empty() && branch_exists(&m.root, &s.branch) {
                git(&m.root, &["worktree", "add", &s.worktree, &s.branch])?;
            } else {
                git(
                    &m.root,
                    &["worktree", "add", "--detach", &s.worktree, "main"],
                )?;
            }
        }
        let tab = tab.filter(|t| !t.is_empty()).unwrap_or(stream(&s.role));
        let pane = pane_for(herdr, &s.worktree, &s.handle, tab)?;
        let text = format!(
            "You were restored after your pane was closed. First run `just tasks callback --as {}`, then `just tasks inbox --as {}` and continue /work {slug}.",
            s.handle, s.handle
        );
        let (agent, args) = if let Some(id) = s.claude_session.strip_prefix("codex:") {
            let mut args = codex_args(&s.model, &s.handle);
            args.extend(["resume".into(), id.into()]);
            ("codex", args)
        } else {
            (
                "claude",
                claude_args(
                    &s.model,
                    &s.handle,
                    &m.mode,
                    ["--resume", &s.claude_session],
                ),
            )
        };
        let prompted = start(herdr, &s.handle, &pane, agent, &args, &*m.sleep)
            .and_then(|()| herdr.call(&["agent", "prompt", &s.handle, &text]));
        if let Err(problem) = prompted {
            herdr.close(&pane)?;
            return Err(problem.into());
        }
        let stuck = !taken(herdr, &pane, &text, &*m.sleep).0;
        // cleanup leaves handles that closed: the restored agent must be able to use the board
        store.revive(&s.handle)?;
        store.reopen_task_session(slug, &s.handle, &pane)?;
        let row = store
            .task_sessions(slug)?
            .into_iter()
            .find(|x| x.handle == s.handle)
            .ok_or_else(|| {
                Error::Refused(format!("{} has no recorded session on {slug}", s.handle))
            })?;
        restored.push(Started { row, stuck });
    }
    Ok(restored)
}

// -- herdr placement -------------------------------------------------------------------------

/// The tabs of `stream` (`code`, `code-2`, ...) with their numbers, in order.
fn stream_tabs<'t>(tabs: &'t [Object], stream: &str) -> Vec<(u64, &'t Object)> {
    let mut found: Vec<(u64, &Object)> = tabs
        .iter()
        .filter_map(|tab| {
            let label = text(tab.get("label"));
            if label == stream {
                return Some((1, tab));
            }
            let number = label.strip_prefix(stream)?.strip_prefix('-')?;
            if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            Some((number.parse().ok()?, tab))
        })
        .collect();
    found.sort_by_key(|(number, _)| *number);
    found
}

/// Split a stream tab only when the child keeps room for the whole startup chooser. Unknown or
/// cramped geometry uses the next stream tab; the owner's w1:t1 is never a launch destination.
fn pane_for(
    herdr: &dyn Herdr,
    cwd: &str,
    handle: &str,
    stream: &str,
) -> Result<String, LaunchError> {
    let env = format!("TASKS_SESSION={handle}");
    let panes = herdr.panes()?;
    let tabs = herdr.tabs()?;
    let ours = stream_tabs(&tabs, stream);
    for (_, tab) in &ours {
        if tab.get("tab_id").and_then(Value::as_str) == Some("w1:t1") {
            continue;
        }
        let inside: Vec<&Object> = panes
            .iter()
            .filter(|p| p.get("tab_id") == tab.get("tab_id"))
            .collect();
        if !inside.is_empty() && inside.len() < MAX_PANES {
            let Some((target, direction)) = startup_split(herdr, &inside) else {
                continue;
            };
            let made = herdr.call(&[
                "pane",
                "split",
                &target,
                "--direction",
                direction,
                "--cwd",
                cwd,
                "--env",
                &env,
                "--no-focus",
            ])?;
            return pane_id(&made, "pane", "pane split");
        }
    }
    let number = ours.iter().map(|(n, _)| *n).max().unwrap_or(0) + 1;
    let label = if number == 1 {
        stream.to_string()
    } else {
        format!("{stream}-{number}")
    };
    let made = herdr.call(&[
        "tab",
        "create",
        "--label",
        &label,
        "--cwd",
        cwd,
        "--env",
        &env,
        "--no-focus",
    ])?;
    pane_id(&made, "root_pane", "tab create")
}

/// Herdr's layout rectangles use terminal cells. Reserve a normal 80×24 terminal after the split
/// (and one separator cell), so the exact chooser need not navigate hidden options.
fn startup_split(herdr: &dyn Herdr, inside: &[&Object]) -> Option<(String, &'static str)> {
    let first = inside.first()?.get("pane_id")?.as_str()?;
    let answer = herdr.call(&["pane", "layout", "--pane", first]).ok()?;
    let layout = answer.get("layout")?.get("panes")?.as_array()?;
    let directions = if inside.len() == 1 {
        ["right", "down"]
    } else {
        ["down", "right"]
    };
    for pane in inside {
        let id = pane.get("pane_id")?.as_str()?;
        let Some(rect) = layout
            .iter()
            .find(|p| p.get("pane_id") == pane.get("pane_id"))
            .and_then(|p| p.get("rect"))
        else {
            continue;
        };
        let (Some(width), Some(height)) = (
            rect.get("width").and_then(Value::as_u64),
            rect.get("height").and_then(Value::as_u64),
        ) else {
            continue;
        };
        for direction in directions {
            let (columns, rows) = if direction == "right" {
                (width.saturating_sub(1) / 2, height)
            } else {
                (width, height.saturating_sub(1) / 2)
            };
            if columns >= 80 && rows >= 24 {
                return Some((id.into(), direction));
            }
        }
    }
    None
}

/// The id of the pane `made[key]` describes.
fn pane_id(made: &Object, key: &str, call: &str) -> Result<String, LaunchError> {
    let id = made.get(key).and_then(|pane| pane.get("pane_id"));
    match id {
        Some(id) if !id.is_null() => Ok(text(Some(id))),
        _ => Err(LaunchError(format!("herdr {call} answered no pane id"))),
    }
}

/// Start the requested agent, trying again only while its shell is not up (`agent_pane_busy`).
fn start(
    herdr: &dyn Herdr,
    handle: &str,
    pane: &str,
    agent: &str,
    claude_args: &[String],
    sleep: &dyn Fn(f64),
) -> Result<(), LaunchError> {
    let mut call = vec![
        "agent", "start", handle, "--kind", agent, "--pane", pane, "--",
    ];
    call.extend(claude_args.iter().map(String::as_str));
    let mut attempt = 1;
    loop {
        match herdr.call(&call) {
            Ok(_) => return Ok(()),
            Err(problem) if problem.0.contains("agent_pane_busy") && attempt < START_TRIES => {
                attempt += 1;
                sleep(2.0);
            }
            Err(problem) => return Err(problem),
        }
    }
}

fn codex_args(model: &str, handle: &str) -> Vec<String> {
    [
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
        "-c",
        &format!("shell_environment_policy.set.TASKS_SESSION=\"{handle}\""),
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn claude_args(model: &str, handle: &str, mode: &str, identity: [&str; 2]) -> Vec<String> {
    let mut args: Vec<String> = identity.iter().map(|s| s.to_string()).collect();
    args.extend(["-n", handle, "--permission-mode", mode].map(String::from));
    if !model.is_empty() {
        args.extend(["--model".to_string(), model.to_string()]);
    }
    args
}

/// Whether the agent in `pane` took the prompt `text` it was just sent. `agent prompt` sends the text
/// and an enter, which does not prove the agent started a turn, and a new pane's input box has been
/// seen to keep the text unsent (umber-ferret-929, 2026-10-08: twice in a day; `herdr pane send-keys
/// PANE enter` sent it). So: wait, then look. The agent working (or done, or already asking
/// something) took it; else, while the text's head still sits at the bottom of the pane, press enter
/// and look again, up to [`PROMPT_ENTERS`] times. False when it is still there after the last enter;
/// a look herdr cannot answer counts as taken, since the prompt was sent.
fn taken(herdr: &dyn Herdr, pane: &str, text: &str, sleep: &dyn Fn(f64)) -> (bool, String) {
    let head: String = text.chars().take(PROMPT_HEAD).collect();
    let mut session = String::new();
    for round in 0..=PROMPT_ENTERS {
        sleep(PROMPT_WAIT);
        let Ok(agents) = herdr.agents() else {
            return (true, session);
        };
        let Some(agent) = agents.iter().find(|a| text_of(a, "pane_id") == pane) else {
            return (true, session);
        };
        session = cleanup::session_value(agent);
        if matches!(
            text_of(agent, "agent_status").as_str(),
            "working" | "blocked" | "done"
        ) {
            return (true, session);
        }
        let Ok(screen) = herdr.read(pane, PROMPT_LINES) else {
            return (true, session);
        };
        if !screen.contains(&head) {
            return (true, session);
        }
        if round == PROMPT_ENTERS || herdr.enter(pane).is_err() {
            return (false, session);
        }
    }
    (false, session)
}

fn text_of(object: &Object, key: &str) -> String {
    text(object.get(key))
}

// -- git -------------------------------------------------------------------------------------

fn git(root: &Path, args: &[&str]) -> Result<String, LaunchError> {
    let done = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .output()
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
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "rev-parse",
            "--verify",
            "-q",
            &format!("refs/heads/{branch}"),
        ])
        .stdin(Stdio::null())
        .output()
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
