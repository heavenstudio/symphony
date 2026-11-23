//! Launch, restore and clean up board agent sessions in hc terminals, shown in the Herdr app.
//!
//! [`launch`] registers a handle, makes the worktree under `<repo>-wt/` (an executor on its task's
//! branch: its slug; an instruction or reviewer on a detached checkout of main) unless
//! `--worktree` gives one, and starts the agent, model and effort the caller asks for, or else the
//! project's `[agents] launch` on the model `[agents.models]` names for its role ([`defaults`]:
//! Codex on its board model when the project names none), in an hc terminal of its own (`hc run`),
//! then gives it its first message (`$work SLUG
//! HANDLE`, a reviewer's `$review-task SLUG HANDLE`; Claude: `/work`, `/review-task`) once it is
//! ready. The terminal outlives whoever launched it
//! (`--outlive-parent`), is kept (`--keep`), and shows in the stream tab of its role (`code` for an
//! executor, `review` for a reviewer, or `--tab`; at most four terminals, then `code-2`) of the
//! project's space in the Herdr app, found by name and made when missing (the labels `herdr.space`
//! and `herdr.tab`): never one of the owner's tabs. The launch is confirmed only by hc's receipt of that message (`startup`); one
//! that cannot be confirmed fails, naming the terminal, which is killed and removed. The launch is
//! recorded on the task: handle, role, conversation id, worktree, branch, terminal, model, start
//! time. [`restore`] reopens the recorded conversation in its worktree, in a new terminal, the same
//! way, and so does a reviewer launch for the task's recorded reviewer, refused while it still runs
//! (a new reviewer starts only where none can be resumed). [`cleanup`] closes unneeded terminals
//! (killed, then removed), each closure recorded on its task with the restore line; [`finish`] is
//! the same for one task that just became done or
//! cancelled (its worktree is the worktree sweep's, `tasks sweep --task`). [`reap`] is the periodic
//! sweep of idle agents (`just tasks clean` runs one). [`close`] closes one agent's terminal on its
//! orchestrator's word, whatever it holds, recorded the same way, under the rules its module gives.
//! Each launch, restore and close is recorded as an execution attempt as well, hc its provider
//! ([`attempts`]), beside the records above, which no attempt replaces.
//! For a command an agent ran unasked (`just tasks`), a launch and a restore start an agent only in
//! the project, and a launch's first message is its skill's alone ([`Bounds`]). Neither starts an
//! agent turned off (`[agents.launch.on]`), nor one whose program this machine lacks ([`off`]).
//!
//! A terminal is closed only when all of these hold: it is this board's (`symphony.board`); its
//! agent is idle (or it is a shell with nothing in it, or its program ended), its foreground is the
//! program it started (an agent, or for no agent a shell: a program a shell became is no idle
//! shell) and has no process running under it other than the agent and its helpers (a shell, a
//! monitor, anything); the last `hc get` before the kill finds that same program, still finished;
//! no `verify`, test run, `land` or Symphony (`just symphony`) runs there; it is not the calling
//! terminal (`HC_TERMINAL_ID`) or the caller's, an orchestrator's, a chat's, or on screen in the
//! Herdr app ([`screen`]); and the session it belongs to is on finished tasks only or holds nothing
//! live. An agent terminal that matches no board session (by recorded terminal, `symphony.handle`,
//! `-n HANDLE` or its name) is kept: closing it would be a guess. A recorded pane hc does not run
//! (a herdr pane id from before hc, `w1:p…`) is gone: cleanup records it closed, with its restore
//! line.
//!
//! Every hc call goes through [`hc::Hc`], whose binaries are `$HC_BIN` (default `hc`) and
//! `$HERDR_APP_BIN` (default `herdr-app`); the tests (`tests/launch/`) point both at `fake-hc`.

pub mod attempts;
mod bounds;
mod checkout;
mod cleanup;
mod close;
pub mod defaults;
pub mod go;
pub mod hc;
pub mod off;
mod place;
mod recover;
mod restore;
mod rollback;
pub mod running;
pub use recover::restore_unanswered;
pub use restore::restore;
pub mod procs;
pub mod reap;
pub mod screen;
pub mod staff;
mod startup;
mod uuid;

pub use bounds::{Bounds, may_bind, own_thread};
pub use cleanup::{Closure, NOT_LANDED, approved, cleanup, cleanup_as, finish, landed};
pub use close::close;
pub use defaults::{Defaults, Resolved};
use place::{name_place, place, stream};
pub use uuid::new_uuid;

use crate::cli::{Env, probes};
use crate::health::RESERVE_SECONDS;
use crate::rows::TaskSessionRow;
use crate::sys;
use crate::{Error, NewLaunch, NewSession, Store};
use hc::Hc;
use serde_json::Value;
use std::collections::HashSet;
use std::fmt;
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
    /// The main checkout, whichever worktree the command runs in ([`root_of`]). Its folder's name
    /// is the Herdr app space's.
    pub root: PathBuf,
    /// Where new worktrees go: `SYMPHONY_WT_DIR`, else `<repo>-wt/` beside the main checkout.
    pub wt_base: PathBuf,
    pub new_id: Box<dyn Fn() -> String>,
    pub sleep: Box<dyn Fn(f64)>,
    pub processes: Box<Processes>,
    pub mode: String,
    /// What a launched agent is given besides `TASKS_SESSION` to reach this board: the caller's
    /// `TASKS_DATABASE` and `TASKS_SERVER`, when it has them. hc starts programs in the login
    /// session's environment, not the caller's.
    pub board_env: Vec<(String, String)>,
    /// For a command run under the unasked recipes' marker, where the agents it starts may run
    /// ([`Bounds`]); none for the binary run by itself, which starts them anywhere.
    pub bounds: Option<Bounds>,
    /// The search path an agent's program is looked for in before it starts ([`off`]): the
    /// caller's `PATH`.
    pub path: Option<std::ffi::OsString>,
}

impl Machine {
    /// This machine's, for a command run in `env`'s working directory.
    #[expect(
        clippy::disallowed_methods,
        reason = "no-timed-waits baseline: the machine's pause behind the clock the launch's tests replace"
    )]
    pub fn of(env: &Env) -> Machine {
        let root = root_of(env);
        let wt_base = worktree_base(&root, env);
        Machine {
            bounds: Bounds::of(env, &root, &wt_base),
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
            path: env.var("PATH").map(std::ffi::OsString::from),
        }
    }

    /// Why `agent` may not start here now ([`off::refusing`]), under the settings in force read
    /// as a launch reads them, refused with why where they cannot be read.
    pub(crate) fn refusing(&self, store: &Store, agent: &str) -> Result<Option<String>, Error> {
        let defaults =
            Defaults::configured_with(&self.root, &store.stored()).map_err(Error::Refused)?;
        Ok(off::refusing(&defaults, self.path.as_deref(), agent))
    }

    /// The Herdr app space the board's terminals show in: the main checkout's folder name.
    fn project(&self) -> String {
        self.root
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    }
}

/// The main checkout of the repository `env`'s working directory is in, whichever worktree that is:
/// the folder holding the common dir when that is a `.git` folder. A repository with none (a
/// submodule, a `--separate-git-dir` checkout, a bare repository's worktree), whose common dir's
/// folder is no checkout of it, has the checkout the command runs in, from which git reaches the
/// same repository; the working directory itself outside a repository. The launcher's git commands
/// run there, so a worktree it makes is of this repository, never the common dir's neighbour's.
pub fn root_of(env: &Env) -> PathBuf {
    let Ok(common) = crate::git::common_dir(&env.cwd) else {
        return env.cwd.clone();
    };
    match crate::sweep::trigger::main_checkout(&common) {
        Some(main) => main.to_path_buf(),
        None => crate::git::checkout(&env.cwd).map_or_else(|| env.cwd.clone(), |(top, _)| top),
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

/// `SYMPHONY_WT_DIR`, else `<repo>-wt/` beside the main checkout.
pub fn worktree_base(root: &Path, env: &Env) -> PathBuf {
    match env.read(symphony_env::WT_DIR) {
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
    /// The agent's effort; `None` is Codex's [`defaults::CODEX_EFFORT`] or Claude's own.
    pub effort: Option<String>,
    pub kind: Option<String>,
    pub role: String,
    pub branch: Option<String>,
    pub detached: bool,
    /// A worktree to use as it is, rather than one made under `<repo>-wt/`.
    pub worktree: Option<PathBuf>,
    pub prompt: String,
    pub tab: Option<String>,
    /// A reviewer's: why it joins beside one still there ([`Store::take_role_beside`]).
    pub beside: Option<String>,
}

impl Ask {
    pub fn new(slug: &str, actor: &str, model: &str) -> Ask {
        Ask {
            slug: slug.into(),
            actor: actor.into(),
            model: model.into(),
            effort: None,
            kind: None,
            role: "executor".into(),
            branch: None,
            detached: false,
            worktree: None,
            prompt: String::new(),
            tab: None,
            beside: None,
        }
    }
}

/// A session whose first message was confirmed in its terminal: an unconfirmed launch is an error,
/// never a session record.
#[derive(Debug, Clone, PartialEq)]
pub struct Started {
    pub row: TaskSessionRow,
    /// A recorded conversation resumed (a restore, or a reviewer launch going back to the task's
    /// reviewer), not a new one started.
    pub resumed: bool,
}

/// Start a session for `ask.slug`: handle, worktree, terminal with its agent and `/work SLUG
/// HANDLE` (a reviewer's `/review-task SLUG HANDLE`), and the record on the task. A reviewer goes
/// to the task itself, never an instruction subtask, and a reviewer launch goes back to the task's
/// recorded reviewer, resumed as a restore resumes it, refused while it still runs
/// ([`restore::reviewer_to_resume`]); a new reviewer starts only where none can be resumed, or
/// beside it when the launch says why (`ask.beside`).
pub fn launch(store: &Store, hc: &dyn Hc, m: &Machine, ask: &Ask) -> Result<Started, Error> {
    let (slug, role) = (ask.slug.as_str(), ask.role.as_str());
    if role != "executor" && role != "reviewer" {
        return Err(Error::Refused(format!(
            "a launched session is an executor or a reviewer, not {}",
            crate::quote::repr(role)
        )));
    }
    if ask.beside.is_some() && role != "reviewer" {
        return Err(Error::Refused(format!(
            "only a reviewer joins beside another, not {}",
            crate::quote::repr(role)
        )));
    }
    // Through a recipe an agent runs unasked: no word of the launcher's in the first message, and
    // no folder outside the project ([`Bounds`]). A given folder is used as the check resolved it,
    // not resolved again.
    let mut admitted = None;
    if let Some(bounds) = &m.bounds {
        bounds
            .prompt(slug, role, &ask.prompt)
            .map_err(Error::Refused)?;
        if let Some(given) = &ask.worktree {
            admitted = Some(bounds.admit("--worktree", given).map_err(Error::Refused)?);
        }
    }
    store.touch(&ask.actor)?;
    let task = store.get(slug)?;
    // A task a custom workflow holds is finished by its position, and staffed by the edges
    // leaving it: a launch starts only a role one of them gives.
    let operating = store.operating_for(std::slice::from_ref(&task))?;
    if crate::workflow::operate::finished(operating.as_ref(), &task) {
        return Err(Error::Refused(format!("{slug} is {}", task.status)));
    }
    if let Some(placed) = operating.as_ref().and_then(|o| o.placed(&task)) {
        by_its_edges(&placed, slug, role, ask.beside.is_some())?;
    }
    if role == "reviewer" {
        if task.instruction {
            let parent = task.parent.as_deref().unwrap_or("its parent");
            return Err(Error::Refused(format!(
                "{slug} is an instruction subtask: a reviewer judges the task itself \
                 (/review-task takes no subtask), so launch it there: just tasks launch {parent} \
                 --role reviewer"
            )));
        }
        // A reviewer beside one still there (`--beside WHY`) is a second on purpose: no resume.
        if ask.beside.is_none()
            && let Some(record) = restore::reviewer_to_resume(store, hc, slug)?
        {
            return restore::resume_reviewer(
                store,
                hc,
                m,
                &record,
                &ask.prompt,
                ask.tab.as_deref(),
            );
        }
    }
    // An agent turned off, or whose program this machine lacks, starts nowhere: refused before
    // anything is made for it.
    let agent = agent_kind(ask);
    if let Some(why) = m.refusing(store, agent)? {
        return Err(Error::Refused(format!("no launch: {why}")));
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
            if let Some(bounds) = &m.bounds {
                bounds
                    .admit("its worktree", &path)
                    .map_err(Error::Refused)?;
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
            let path =
                admitted.unwrap_or_else(|| given.canonicalize().unwrap_or_else(|_| given.clone()));
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
    let actor = if ask.actor.is_empty() {
        crate::workflow::board_service("launch")
    } else {
        crate::workflow::agent_session(&ask.actor)
    };
    let local = attempts::Local::new(store, hc, actor);
    let mut terminal = String::new();
    let mut checkout_made = false;
    let mut attempt = None;
    let started = (|| {
        take_up(store, ask, &handle)?;
        if made_worktree {
            let base = checkout::base(store, m, ask, &task)?;
            checkout::make(m, &path, &use_branch, detach, &base, &mut checkout_made)?;
        }
        let tab = ask.tab.clone().filter(|t| !t.is_empty());
        let agent = agent_kind(ask);
        let conversation = (m.new_id)();
        let args = if agent == "codex" {
            codex_args(&ask.model, ask.effort.as_deref(), &handle, m)
        } else {
            claude_args(
                &ask.model,
                ask.effort.as_deref(),
                &handle,
                &m.mode,
                ["--session-id", &conversation],
            )
        };
        let text = prompt_text(slug, &handle, &ask.prompt, agent, role);
        // Admitted, its start owed, before hc is started.
        let admitted = local.admit(&task, role, &handle, &where_, &text)?.id;
        attempt = Some(admitted.clone());
        start_confirmed(
            hc,
            m,
            &mut terminal,
            &Start {
                cwd: &where_,
                handle: &handle,
                stream: tab.as_deref().unwrap_or(stream(role)),
                agent,
                resumed_codex: false,
                args,
                text: &text,
            },
            (&local, &admitted),
        )?;
        local.running(
            &admitted,
            (agent != "codex").then_some(conversation.as_str()),
        )?;
        let claude_session = if agent == "codex" {
            // The thread is known once the agent registers its callback (`just tasks callback`).
            CODEX.to_string()
        } else {
            conversation
        };
        let row = store.record_launch(&NewLaunch {
            slug: slug.into(),
            handle: handle.clone(),
            role: role.into(),
            claude_session,
            worktree: where_,
            branch: use_branch,
            pane: terminal.clone(),
            model: ask.model.clone(),
            ..NewLaunch::default()
        })?;
        Ok(Started {
            row,
            resumed: false,
        })
    })();
    started.map_err(|problem| {
        let made = rollback::Made {
            handle: &handle,
            terminal: &terminal,
            checkout: checkout_made.then_some(path.as_path()),
            attempt: attempt.as_deref().map(|id| (&local, id)),
        };
        rollback::failed(store, hc, m, &made, problem)
    })
}

/// A checkout with unfinished work held by a present executor cannot host another agent.
fn live_executor_in(store: &Store, path: &Path) -> Result<Option<String>, Error> {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let board = store.board()?;
    let records = store.all_task_sessions()?;
    // Finished by its recorded facts: a held task by its position, never its projected status.
    let operating = store.operating_for(&board.tasks)?;
    let finished = |t| crate::workflow::operate::finished(operating.as_ref(), t);
    for task in board.tasks.iter().filter(|t| !finished(t)) {
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

/// The skill a session of `role` works by: a reviewer's is the review column's own, never `work`.
pub(super) fn skill(role: &str) -> &'static str {
    if role == "reviewer" {
        "review-task"
    } else {
        "work"
    }
}

/// `/work SLUG HANDLE` (a reviewer's `/review-task SLUG HANDLE`), then the prompt given, if any.
/// That is the whole prompt (the skill's rule): the skill carries the rest, subagents and forks
/// forbidden among it.
pub(super) fn prompt_text(
    slug: &str,
    handle: &str,
    prompt: &str,
    agent: &str,
    role: &str,
) -> String {
    let skill = skill(role);
    let work = if agent == "codex" {
        format!("${skill}")
    } else {
        format!("/{skill}")
    };
    match prompt.trim() {
        "" => format!("{work} {slug} {handle}"),
        more => format!("{work} {slug} {handle} {more}"),
    }
}

/// What starts a Codex agent's conversation in a launch record, and its callback's address, before
/// its thread (`codex:ID`); alone, a Codex agent whose thread is not known yet, as it is from its
/// launch until it registers its callback.
pub(crate) const CODEX: &str = "codex:";

/// Whether a launch record's `conversation` is one an agent can be found or resumed by: any but
/// none, and but a Codex agent's whose thread was never recorded.
pub(crate) fn resumable(conversation: &str) -> bool {
    !conversation.is_empty() && conversation != CODEX
}

/// The agent whose conversation a record names: Codex's ids are kept as `codex:ID`.
pub(super) fn agent_of(conversation: &str) -> &'static str {
    if conversation.starts_with(CODEX) {
        "codex"
    } else {
        "claude"
    }
}

fn agent_kind(ask: &Ask) -> &'static str {
    match ask.kind.as_deref() {
        Some("claude" | "worker" | "reviewer") => "claude",
        _ => "codex",
    }
}

/// The new session takes its part on the task: an executor reserves it, a reviewer takes the
/// reviewer role ([`take_reviewer`]), beside one still there only when the launch says why. A
/// refusal naming how one joins beside it names the launch's own `--beside`, not `tasks role`'s.
/// An executor of a task a custom workflow holds reserves it as on any task, and claims its edge
/// when it starts.
fn take_up(store: &Store, ask: &Ask, handle: &str) -> Result<(), Error> {
    let slug = ask.slug.as_str();
    if ask.role == "executor" {
        store.reserve(slug, handle, RESERVE_SECONDS)?;
        return Ok(());
    }
    let board = store.board_command();
    take_reviewer(store, slug, handle, ask.beside.as_deref()).map_err(|problem| match problem {
        Error::Refused(text) => Error::Refused(text.replace(
            &format!("{board} role {slug} reviewer --beside WHY"),
            &format!("{board} launch {slug} --role reviewer --beside WHY"),
        )),
        problem => problem,
    })
}

/// `handle` takes the reviewer's role on `slug`: on a task a custom workflow holds, by the one edge
/// leaving its position that gives it, as that session, through admission, which judges it fresh
/// ([`Store::claim_by_workflow`]); on any other, as the board's role, beside one still there only
/// when `beside` says why ([`Store::take_role_beside`]).
pub(super) fn take_reviewer(
    store: &Store,
    slug: &str,
    handle: &str,
    beside: Option<&str>,
) -> Result<(), Error> {
    if crate::store::workflow::position_held(&store.get(slug)?.fields) {
        let principal = crate::workflow::agent_session(handle);
        let role = symphony_api::workflow::Role::Reviewer;
        store.claim_by_workflow(slug, &principal, &crate::workflow::Host::local(), role)?;
    } else {
        store.take_role_beside(slug, handle, "reviewer", beside)?;
    }
    Ok(())
}

/// Whether a launch of `role` on `slug`, standing at `placed`, starts a role the edges leaving its
/// position give: refused before anything starts where none does, or where a reviewer would join
/// beside another, as an edge gives the reviewer's role to one session.
fn by_its_edges(
    placed: &crate::workflow::operate::Placed<'_>,
    slug: &str,
    role: &str,
    beside: bool,
) -> Result<(), Error> {
    let at = placed.at();
    let explicit = |why: String| {
        Error::Refused(format!(
            "explicit_action_required: a custom workflow holds {slug}, at {}/{}: {why}; its \
             edges: `just tasks workflow actions {slug}`",
            at.phase, at.state
        ))
    };
    if beside {
        return Err(explicit(
            "its edges give the reviewer's role to one session, so no reviewer joins beside \
             another"
                .into(),
        ));
    }
    let claims = placed.claims();
    if !(claims.iter()).any(|c| crate::workflow::admit::word(&c.role) == role) {
        return Err(explicit(format!(
            "no edge leaving it gives the {role} role"
        )));
    }
    Ok(())
}

/// The terminal and its first message, borrowed from the launch or restore.
struct Start<'launch> {
    cwd: &'launch str,
    handle: &'launch str,
    stream: &'launch str,
    agent: &'static str,
    resumed_codex: bool,
    args: Vec<String>,
    text: &'launch str,
}

/// Start `start`'s agent in its stream's tab, in a terminal of its own: its address, and where it
/// shows.
fn open(
    hc: &dyn Hc,
    m: &Machine,
    start: &Start<'_>,
) -> Result<(String, place::Place), LaunchError> {
    let place = place(hc, &m.project(), start.stream)?;
    let mut argv = vec!["env".to_string(), format!("TASKS_SESSION={}", start.handle)];
    argv.extend(m.board_env.iter().map(|(k, v)| format!("{k}={v}")));
    if start.agent == "codex" {
        // Its shell sees what `codex_args` sets, and the terminal, known only once it runs.
        argv.extend(hc::IN_ITS_TERMINAL.map(String::from));
    }
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
    let address = made
        .get("address")
        .and_then(Value::as_str)
        .filter(|a| !a.is_empty())
        .ok_or_else(|| LaunchError("hc run answered no terminal address".into()))?
        .to_string();
    Ok((address, place))
}

/// Start in the stream's tab, as the start of `attempt` (the local provider's, and the attempt's
/// id), and confirm the message (`startup`); leave the terminal for the caller.
fn start_confirmed(
    hc: &dyn Hc,
    m: &Machine,
    terminal: &mut String,
    start: &Start<'_>,
    attempt: (&attempts::Local<'_>, &str),
) -> Result<startup::Message, Error> {
    let (local, id) = attempt;
    let mut placed = None;
    local.start(id, &mut || {
        let (address, place) = open(hc, m, start)?;
        terminal.clone_from(&address);
        placed = Some(place);
        Ok(address)
    })?;
    let Some(mut place) = placed else {
        return Err(LaunchError("hc run answered no terminal address".into()).into());
    };
    let message = startup::first_message(
        hc,
        terminal,
        start.agent,
        start.text,
        start.resumed_codex,
        &mut || name_place(hc, &mut place),
        &*m.sleep,
    )?;
    Ok(message)
}

/// Whether a recorded pane is gone: a herdr pane id (`w1:p3`: herdr is no more), or a terminal of
/// this board hc no longer runs. A record naming no pane says nothing either way.
pub(crate) fn gone(pane: &str, running: &HashSet<String>) -> bool {
    !pane.is_empty() && (pane.contains(':') || !running.contains(pane))
}

fn codex_args(model: &str, effort: Option<&str>, handle: &str, m: &Machine) -> Vec<String> {
    let model = if model.is_empty() {
        defaults::CODEX_MODEL
    } else {
        model
    };
    let effort = effort
        .filter(|e| !e.is_empty())
        .unwrap_or(defaults::CODEX_EFFORT);
    let mut args: Vec<String> = [
        "--no-daemon".to_string(),
        "-s".into(),
        "workspace-write".into(),
        "-m".into(),
        model.into(),
        "-c".into(),
        format!("model_reasoning_effort=\"{effort}\""),
        "-c".into(),
        "sandbox_workspace_write.network_access=true".into(),
    ]
    .into();
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

fn claude_args(
    model: &str,
    effort: Option<&str>,
    handle: &str,
    mode: &str,
    identity: [&str; 2],
) -> Vec<String> {
    let mut args: Vec<String> = identity.iter().map(|s| s.to_string()).collect();
    args.extend(["-n", handle, "--permission-mode", mode].map(String::from));
    if !model.is_empty() {
        args.extend(["--model".to_string(), model.to_string()]);
    }
    if let Some(effort) = effort.filter(|e| !e.is_empty()) {
        args.extend(["--effort".to_string(), effort.to_string()]);
    }
    args
}

// -- git -------------------------------------------------------------------------------------

fn git(root: &Path, args: &[&str]) -> Result<String, LaunchError> {
    let done = sys::output(sys::git().arg("-C").arg(root).args(args))
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
    sys::output(sys::git().arg("-C").arg(root).args([
        "rev-parse",
        "--verify",
        "-q",
        &format!("refs/heads/{branch}"),
    ]))
    .is_ok_and(|done| done.status.success())
}

/// The worktree that has `branch` checked out, if any.
fn checked_out_in(root: &Path, branch: &str) -> Result<Option<String>, LaunchError> {
    let wanted = format!("refs/heads/{branch}");
    let listed = crate::sweep::git::worktrees(root).map_err(LaunchError)?;
    let held = listed
        .into_iter()
        .find(|tree| tree.branch.as_ref() == Some(&wanted));
    Ok(held.map(|tree| tree.path.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::{CODEX, agent_of, resumable};

    /// A record's conversation is one to resume by unless it is none or a Codex agent's whose
    /// thread was never recorded; a Codex thread and a Claude session both are.
    #[test]
    fn a_conversation_is_resumable_unless_empty_or_a_codex_agent_without_its_thread() {
        assert!(!resumable(""));
        assert!(!resumable(CODEX));
        assert!(resumable("codex:0abc-12"));
        assert!(resumable("3f2a-claude-session"));
        // Only the prefix itself is a thread not known yet: a word that merely looks like it is
        // a conversation of its own.
        assert!(resumable("codex"));
        assert!(resumable("codex: "));
        assert_eq!(agent_of(CODEX), "codex");
        assert_eq!(agent_of("codex:0abc-12"), "codex");
        assert_eq!(agent_of("codex"), "claude");
        assert_eq!(agent_of(""), "claude");
    }
}
