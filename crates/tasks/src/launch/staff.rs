//! One bounded staffing pass. The Symphony already calls `reap` every five minutes; that command
//! runs this first, so an idle agent can take work before the idle sweep closes its pane.

use super::hc::{self, Binary, Hc, text_of};
use super::{Ask, Defaults, Machine, Resolved, launch, prompt_text};
use crate::cli::{Env, agents, probes};
use crate::health::{EXECUTING, FINISHED};
use crate::sys::FileLock;
use crate::workflow::operate::Operating;
use crate::{Board, Error, Health, Held, NewLaunch, Store, Task, git};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use symphony_api::workflow::Role;

/// One config for enablement, capacity and optional subtree selection. Missing means disabled.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    pub max_sessions: usize,
    pub min_free_memory_gib: f64,
    pub only: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            max_sessions: 8,
            min_free_memory_gib: 4.0,
            only: Vec::new(),
        }
    }
}

fn refused(why: impl ToString) -> Error {
    Error::Refused(why.to_string())
}

fn config(env: &Env) -> Result<Config, Error> {
    let root = env
        .cwd
        .ancestors()
        .find(|p| p.join(".git").exists())
        .ok_or_else(|| refused("staff needs a repository"))?;
    let path = root.join("tools/task_staff.json");
    let read = match std::fs::read_to_string(&path) {
        Ok(read) => read,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(e) => return Err(refused(format!("{}: {e}", path.display()))),
    };
    let config: Config = serde_json::from_str(&read).map_err(refused)?;
    if !config.min_free_memory_gib.is_finite() || config.min_free_memory_gib < 0.0 {
        return Err(refused(
            "min_free_memory_gib must be finite and nonnegative",
        ));
    }
    Ok(config)
}

/// A held lock lives until the pass ends, including launches and their prompt confirmation.
fn lock(env: &Env) -> Result<Option<FileLock>, Error> {
    let dir = git::common_dir(&env.cwd)
        .map_err(refused)?
        .join(crate::folders::TASKS);
    std::fs::create_dir_all(&dir).map_err(refused)?;
    let file = File::options()
        .read(true)
        .append(true)
        .create(true)
        .open(dir.join("staff.lock"))
        .map_err(refused)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(FileLock(file))),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(refused(e)),
    }
}

fn owner_only(task: &Task) -> bool {
    let name = format!("{} {}", task.slug, task.title).to_lowercase();
    [
        "owner-only",
        "owner only",
        "owner tier",
        "sign-off",
        "signoff",
        "bless",
    ]
    .iter()
    .any(|s| name.contains(s))
        || task.body.lines().any(|line| {
            let line = line.trim().trim_matches('*').to_lowercase();
            [
                "owner-only:",
                "owner only:",
                "tier: owner",
                "review tier: owner",
            ]
            .iter()
            .any(|prefix| line.starts_with(prefix))
        })
}

fn orchestrator<'a>(task: &'a Task, board: &'a Board) -> Option<&'a str> {
    let mut at = task;
    let mut seen = HashSet::new();
    while seen.insert(at.slug.as_str()) {
        if let Some(role) = at
            .roles
            .iter()
            .find(|r| r.role == "orchestrator" && r.health == Health::Live)
        {
            return Some(&role.handle);
        }
        at = board
            .tasks
            .iter()
            .find(|t| Some(t.slug.as_str()) == at.parent.as_deref())?;
    }
    None
}

/// The role `task` wants staffed, its prerequisites met. On a task a custom workflow holds, the
/// one an edge leaving its position gives (an executor's before a reviewer's) whose dependency
/// predicate is met, never by the status the position projects to; on any other, an executor for
/// ready work and a reviewer for work in review, every prerequisite done.
fn wanted(task: &Task, board: &Board, operating: Option<&Operating>) -> Option<&'static str> {
    if crate::store::workflow::position_held(&task.fields) {
        let operating = operating?;
        let placed = operating.placed(task)?;
        if placed.finished() {
            return None;
        }
        let find = |slug: &str| board.tasks.iter().find(|t| t.slug == slug);
        let mut claims = placed.claims();
        claims.retain(|c| operating.dependencies_met(task, c.edge, find));
        return [Role::Executor, Role::Reviewer]
            .into_iter()
            .find(|role| claims.iter().any(|c| c.role == *role))
            .map(|role| {
                if role == Role::Executor {
                    "executor"
                } else {
                    "reviewer"
                }
            });
    }
    let role = match task.status.as_str() {
        "ready" => "executor",
        "review" => "reviewer",
        _ => return None,
    };
    let done = |slug: &String| (board.tasks.iter()).any(|t| t.slug == *slug && t.status == "done");
    task.depends.iter().all(done).then_some(role)
}

/// The role a staffing pass would start on `task` ([`wanted`]), where nothing stops it: no
/// owner-only mark, a live orchestrator above it, inside `config`'s scope, nobody there holding
/// or reserving that role, and no executor at work on a task under it.
fn eligible(
    task: &Task,
    board: &Board,
    config: &Config,
    operating: Option<&Operating>,
) -> Option<&'static str> {
    let role = wanted(task, board, operating)?;
    if owner_only(task)
        || orchestrator(task, board).is_none()
        || (!config.only.is_empty()
            && !config
                .only
                .iter()
                .any(|s| task.slug == *s || task.slug.starts_with(&format!("{s}/"))))
        || task
            .roles
            .iter()
            .any(|r| (r.role == role || r.role == "reserved") && r.health.holds())
    {
        return None;
    }
    // Work on a child already staffs the parent. Never dispatch an overlapping executor/review.
    let overlapping = board.tasks.iter().any(|child| {
        if !EXECUTING.contains(&child.status.as_str()) {
            return false;
        }
        let mut parent = child.parent.as_deref();
        let mut seen = HashSet::new();
        while let Some(slug) = parent.filter(|s| seen.insert(*s)) {
            if slug == task.slug {
                return child
                    .roles
                    .iter()
                    .any(|r| (r.role == "executor" || r.role == "reserved") && r.health.holds());
            }
            parent = board
                .tasks
                .iter()
                .find(|t| t.slug == slug)
                .and_then(|t| t.parent.as_deref());
        }
        false
    });
    (!overlapping).then_some(role)
}

/// Available memory, including reclaimable inactive pages on macOS (MemAvailable on Linux).
fn free_memory_gib() -> Result<f64, Error> {
    let bytes = if cfg!(target_os = "macos") {
        let output = crate::sys::output(&mut crate::sys::command("vm_stat")).map_err(refused)?;
        if !output.status.success() {
            return Err(refused("vm_stat failed"));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let page = text
            .lines()
            .next()
            .and_then(|l| l.split("page size of ").nth(1))
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.parse::<f64>().ok())
            .ok_or_else(|| refused("vm_stat did not report its page size"))?;
        let mut pages = 0.0;
        for key in ["Pages free:", "Pages inactive:", "Pages speculative:"] {
            pages += text
                .lines()
                .find_map(|l| l.strip_prefix(key))
                .and_then(|s| s.trim().trim_end_matches('.').parse::<f64>().ok())
                .ok_or_else(|| refused(format!("vm_stat did not report {key}")))?;
        }
        page * pages
    } else {
        std::fs::read_to_string("/proc/meminfo")
            .map_err(refused)?
            .lines()
            .find_map(|l| l.strip_prefix("MemAvailable:"))
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.parse::<f64>().ok())
            .ok_or_else(|| refused("/proc/meminfo did not report MemAvailable"))?
            * 1024.0
    };
    Ok(bytes / 1_073_741_824.0)
}

struct ReviewTarget {
    branch: String,
    tip: String,
}

/// Land-request tasks record their target in the opening line, rather than in their own slug.
/// Other tasks use the executor's recorded branch, then their slug when never launched.
fn review_target(store: &Store, task: &Task, machine: &Machine) -> Result<ReviewTarget, Error> {
    let recorded = task
        .body
        .trim_start()
        .strip_prefix("Review `")
        .and_then(|rest| rest.split_once("` before it lands"))
        .map(|(branch, _)| branch.to_string())
        .or_else(|| {
            task.body.lines().find_map(|line| {
                let line = line.trim();
                let branch = line
                    .strip_prefix("Branch:")
                    .or_else(|| line.strip_prefix("branch:"))
                    .or_else(|| {
                        if line.starts_with("Worktree") {
                            line.split_once(", branch ").map(|(_, branch)| branch)
                        } else {
                            None
                        }
                    })?;
                let branch = branch.trim().trim_end_matches('.').trim_matches('`');
                (!branch.is_empty() && !branch.contains(char::is_whitespace))
                    .then(|| branch.to_string())
            })
        });
    let branch = match recorded {
        Some(branch) => branch,
        None => store
            .task_sessions(&task.slug)?
            .into_iter()
            .rev()
            .find(|s| s.role == "executor" && !s.branch.is_empty())
            .map_or_else(|| task.slug.clone(), |s| s.branch),
    };
    let tip = super::git(
        &machine.root,
        &["rev-parse", "--verify", &format!("refs/heads/{branch}")],
    )
    .map_err(|e| refused(format!("cannot resolve review target {branch}: {e}")))?;
    Ok(ReviewTarget { branch, tip })
}

/// Print each proposal/deferral; post only successful dispatches. A dry run changes no board rows.
pub fn pass(store: &Store, env: &Env, dry_run: bool) -> Result<Vec<String>, Error> {
    pass_inner(store, env, dry_run, false)
}

/// The idle sweep has nothing to report when automatic staffing is disabled.
pub fn automatic(store: &Store, env: &Env) -> Result<Vec<String>, Error> {
    pass_inner(store, env, false, true)
}

fn pass_inner(
    store: &Store,
    env: &Env,
    dry_run: bool,
    quiet_disabled: bool,
) -> Result<Vec<String>, Error> {
    let config = config(env)?;
    if !config.enabled && !dry_run {
        if quiet_disabled {
            return Ok(Vec::new());
        }
        return Ok(vec!["staff disabled (tools/task_staff.json)".into()]);
    }
    let Some(_lock) = lock(env)? else {
        return Ok(vec![
            "another staffing pass is running: nothing done".into(),
        ]);
    };
    let hc = Binary::of(env);
    let machine = Machine::of(env);
    // What a launch here starts: the project's `[agents] launch`, on the model `[agents.models]`
    // names for the role, else the board's own.
    let defaults = Defaults::configured_with(&machine.root, &store.stored()).map_err(refused)?;
    let mut candidates = store.board()?.tasks;
    // The custom workflow by whose edges the tasks it holds are staffed, read once for the pass.
    let operating = store.operating_for(&candidates)?;
    candidates.retain(|t| {
        crate::store::workflow::position_held(&t.fields)
            || matches!(t.status.as_str(), "ready" | "review")
    });
    candidates.sort_unstable_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| a.created.total_cmp(&b.created))
            .then_with(|| a.slug.cmp(&b.slug))
    });
    let mut lines = Vec::new();
    let mut used = HashSet::new();
    let mut projected = 0;
    for candidate in candidates {
        let board = store.board()?;
        let Some(task) = board.tasks.iter().find(|t| t.slug == candidate.slug) else {
            continue;
        };
        let Some(role) = eligible(task, &board, &config, operating.as_ref()) else {
            continue;
        };
        let actor = orchestrator(task, &board).expect("eligible has a live orchestrator");
        let review = if role == "reviewer" {
            match review_target(store, task, &machine) {
                Ok(target) => Some(target),
                Err(problem) => {
                    lines.push(format!("staff {}: failed: {problem}", task.slug));
                    continue;
                }
            }
        } else {
            None
        };
        let view = agents::describe(store, &probes::machine(env), store.now(), false)?;
        if !view.herdr || !view.slots {
            return Err(refused(
                "staff needs hc and slot probes; nothing dispatched",
            ));
        }
        // The cap counts every agent of the machine, whoever started it.
        let live = super::running::counted(&view.agents, &hc.live()?) + projected;
        let launching = defaults.resolve_as(Some(role), None, None);
        // Who may not review it, by the rule the verdict and the land hold (`Held::unfresh`).
        let held = if role == "reviewer" {
            store.ever_held(&task.slug)?
        } else {
            Held::default()
        };
        // A review whose recorded reviewer still runs fails here as its launch would, in a dry run
        // too.
        let wanted = Job {
            board: &board,
            task,
            role,
            held: &held,
            operating: operating.as_ref(),
        };
        let job = job_for(store, &hc, &view.agents, &used, &wanted);
        let (resuming, idle) = job
            .as_ref()
            .map_or((None, None), |(resuming, idle)| (resuming.clone(), *idle));
        let memory = free_memory_gib()?;
        let machine_agents = store.limits().machine_agents;
        let blocked = if idle.is_none() && live >= config.max_sessions {
            Some(format!("session cap {live}/{}", config.max_sessions))
        } else if let Some(most) = machine_agents.filter(|most| idle.is_none() && live >= *most) {
            Some(format!(
                "machine agents {live}/{most}, [limits] machine_agents"
            ))
        } else if memory < config.min_free_memory_gib {
            Some(format!(
                "free memory {memory:.1} GiB below {:.1} GiB",
                config.min_free_memory_gib
            ))
        } else {
            None
        };
        let mut dispatched = false;
        let action = if let Err(problem) = &job {
            format!("staff {}: failed: {problem}", task.slug)
        } else if let Some(why) = blocked {
            format!("staff {}: deferred ({why})", task.slug)
        } else if dry_run {
            if let Some(a) = idle {
                used.insert(a.handle.clone());
                format!(
                    "staff {}: would ping {} as {role} in {}",
                    task.slug,
                    a.handle,
                    a.pane.as_deref().unwrap_or_default()
                )
            } else if let Some(handle) = &resuming {
                projected += 1;
                format!("staff {}: would resume {handle} as {role}", task.slug)
            } else {
                projected += 1;
                format!(
                    "staff {}: would launch {} as {role}",
                    task.slug,
                    launching.said()
                )
            }
        } else {
            if let Some(a) = idle {
                used.insert(a.handle.clone());
            }
            let outcome = dispatch(
                store,
                &hc,
                &machine,
                &launching,
                task,
                actor,
                review.as_ref(),
                idle,
            );
            match outcome {
                Ok(line) => {
                    dispatched = true;
                    format!("staff {}: {line}", task.slug)
                }
                Err(e) => format!("staff {}: failed: {e}", task.slug),
            }
        };
        let action = match &review {
            Some(target) => format!("{action} (review {} at {})", target.branch, target.tip),
            None => action,
        };
        if dispatched && let Err(problem) = store.say(&task.slug, actor, &action, None) {
            lines.push(format!(
                "staff {}: dispatched, but thread post failed: {problem}",
                task.slug
            ));
        }
        lines.push(action);
    }
    if lines.is_empty() {
        lines.push("staff: no eligible work".into());
    }
    Ok(lines)
}

/// The idle agent of `agents` a job on `task` as `role` goes to, none of `used`: one at rest in a
/// pane, of the role's kind, with no open work, orchestrating nothing and alone in its checkout;
/// for a review one detached, holding no role on the task and a fresh reviewer of it ([`fresh`],
/// `held` its [`Store::ever_held`]), as the board takes the role and the verdict from no other; for
/// work one detached for an instruction, else on the task's branch.
fn idle_for<'a>(
    agents: &'a [agents::Agent],
    board: &Board,
    task: &Task,
    role: &str,
    used: &HashSet<String>,
    held: &Held,
    operating: Option<&Operating>,
) -> Option<&'a agents::Agent> {
    // Open work by its recorded facts: a task a custom workflow holds until its position is
    // terminal, one the board does not show by its status.
    let open = |t: &agents::Held| match board.tasks.iter().find(|task| task.slug == t.slug) {
        Some(task) => !crate::workflow::operate::finished(operating, task),
        None => !FINISHED.contains(&t.status.as_str()),
    };
    agents.iter().find(|a| {
        !used.contains(&a.handle)
            && a.state == "idle"
            && a.pane.is_some()
            && a.agent.is_some()
            && a.kind
                == if role == "reviewer" {
                    "reviewer"
                } else {
                    "worker"
                }
            && !a.tasks.iter().any(open)
            && !board.tasks.iter().any(|t| {
                t.roles
                    .iter()
                    .any(|r| r.handle == a.handle && r.role == "orchestrator")
            })
            && Path::new(&a.worktree).is_dir()
            && !agents.iter().any(|other| {
                other.health.holds()
                    && other.worktree == a.worktree
                    && other.tasks.iter().any(|t| t.role == "executor" && open(t))
            })
            && if role == "reviewer" {
                a.branch.is_empty()
                    && !task.roles.iter().any(|r| r.handle == a.handle)
                    && fresh(task, held, &a.handle)
            } else if task.instruction {
                a.branch.is_empty()
            } else {
                a.branch == task.slug
            }
    })
}

/// One job of a pass, as the pass read it: the task on its board, the role it is staffed as, who
/// held it ([`Store::ever_held`], for a review) and the custom workflow that holds it, if any.
struct Job<'a> {
    board: &'a Board,
    task: &'a Task,
    role: &'a str,
    held: &'a Held,
    operating: Option<&'a Operating>,
}

/// Who `job` goes to, none of `used`: a review to the task's recorded reviewer that its launch
/// resumes, by the launch's own rule ([`super::restore::reviewer_to_resume`]), never another idle
/// one, and refused, as the launch is, while that reviewer still runs; else the idle agent
/// [`idle_for`] picks, if any. Neither: a launch.
fn job_for<'a>(
    store: &Store,
    hc: &dyn Hc,
    agents: &'a [agents::Agent],
    used: &HashSet<String>,
    job: &Job<'_>,
) -> Result<(Option<String>, Option<&'a agents::Agent>), Error> {
    let Job {
        board,
        task,
        role,
        held,
        operating,
    } = *job;
    if role == "reviewer"
        && let Some(record) = super::restore::reviewer_to_resume(store, hc, &task.slug)?
    {
        return Ok((Some(record.handle), None));
    }
    Ok((
        None,
        idle_for(agents, board, task, role, used, held, operating),
    ))
}

/// Whether `handle` is a fresh reviewer of `task`, `held` its [`Store::ever_held`]: one that
/// executed none of it and neither orchestrates, orchestrated nor filed it, by
/// the one rule the board's verdict and the land hold ([`Held::unfresh`]).
fn fresh(task: &Task, held: &Held, handle: &str) -> bool {
    let orchestrates = task.holders("orchestrator").contains(&handle);
    (held.unfresh(handle, &task.created_by, orchestrates)).is_none()
}

/// Keep task-specific failures (target, checkout, launch and reuse) inside this job. A review is
/// given on the task itself, never a subtask: its reviewer takes the reviewer's role there and its
/// launch is recorded there, and the pass's line on the thread names the tip. A failed reuse lets
/// go of what it took ([`reuse`]), as a launched agent's own rollback does.
#[expect(
    clippy::too_many_arguments,
    reason = "one dispatch of the pass, named by what it reads"
)]
fn dispatch(
    store: &Store,
    hc: &dyn Hc,
    machine: &Machine,
    launching: &Resolved,
    task: &Task,
    actor: &str,
    review: Option<&ReviewTarget>,
    idle: Option<&agents::Agent>,
) -> Result<String, Error> {
    let role = if review.is_some() {
        "reviewer"
    } else {
        "executor"
    };
    if let Some(agent) = idle {
        return reuse(store, hc, machine, agent, task, role);
    }
    let mut ask = Ask::new(&task.slug, actor, &launching.model);
    ask.kind = Some(launching.agent.into());
    ask.effort.clone_from(&launching.effort);
    ask.role = role.into();
    if role == "executor"
        && !task.instruction
        && let Some(path) = super::checked_out_in(&machine.root, &task.slug)?
    {
        ask.worktree = Some(PathBuf::from(path));
    }
    launch(store, hc, machine, &ask).map(|s| {
        let row = &s.row;
        if s.resumed {
            // The task's recorded reviewer, on its own agent: its prompt's `/` or `$` says which.
            let agent = super::agent_of(&row.claude_session);
            return format!(
                "resumed {} as {role}: pane {}, worktree {}, prompt {}",
                row.handle,
                row.pane,
                row.worktree,
                prompt_text(&task.slug, &row.handle, "", agent, role),
            );
        }
        format!(
            "launched {} as {role}: {}, pane {}, worktree {}, prompt {}",
            row.handle,
            launching.said(),
            row.pane,
            row.worktree,
            prompt_text(&task.slug, &row.handle, "", launching.agent, role),
        )
    })
}

/// Give an idle agent the job on `task` in its own terminal (`hc send`), recorded first: an
/// executor's reservation of the task, or a reviewer's role on it. A reviewer whose launch record
/// fails lets go of the role it took, as nothing was sent, so the task waits on no reviewer with no
/// job; once the record is written it keeps the role, as the prompt may reach it. hc saying its
/// turn started, or a look finding it at work, confirms it; else the line says it is unconfirmed.
fn reuse(
    store: &Store,
    hc: &dyn Hc,
    machine: &Machine,
    agent: &agents::Agent,
    task: &Task,
    role: &str,
) -> Result<String, Error> {
    let handle = &agent.handle;
    let pane = agent.pane.as_deref().expect("idle has a pane");
    let current = hc.get(pane)?;
    if !current.as_ref().is_some_and(|t| {
        text_of(t, "state") == "live" && super::READY_STATES.contains(&hc::status(t).as_str())
    }) {
        return Err(refused("idle agent became busy"));
    }
    if let Some(other) = super::live_executor_in(store, Path::new(&agent.worktree))? {
        return Err(refused(format!("checkout has live executor {other}")));
    }
    store.revive(handle)?;
    // A reviewer never reserves the task it judges: it takes the role ([`Store::take_role`] refuses
    // one the board would take no verdict from, and on held work admission does).
    if role == "reviewer" {
        super::take_reviewer(store, &task.slug, handle, None)?;
    } else {
        store.reserve(&task.slug, handle, crate::health::RESERVE_SECONDS)?;
    }
    let kind = agent.agent.as_deref().unwrap_or("codex");
    let prompt = prompt_text(&task.slug, handle, "", kind, role);
    // Record before prompting: once the prompt is sent, a timeout must never free this job for a duplicate.
    let recorded = store.record_launch(&NewLaunch {
        slug: task.slug.clone(),
        handle: handle.clone(),
        role: role.into(),
        pane: pane.into(),
        claude_session: format!(
            "{}{}",
            if kind == "codex" {
                crate::launch::CODEX
            } else {
                ""
            },
            agent.session_id.as_deref().unwrap_or_default()
        ),
        worktree: agent.worktree.clone(),
        branch: agent.branch.clone(),
        model: agent.model.clone(),
        ..NewLaunch::default()
    });
    if let Err(error) = recorded {
        // Picked holding no role on the task ([`idle_for`]): the reviewer's role is the one just taken.
        if role == "reviewer" {
            let _ = store.release(&task.slug, handle, Some("reviewer"));
        }
        return Err(error);
    }
    let started = hc.send(pane, &prompt)?;
    let confirmed = started
        || (0..super::SEND_LOOKS).any(|_| {
            (machine.sleep)(super::PROMPT_WAIT);
            // `done` was its state before the message: only work, or a question, is news.
            hc.get(pane)
                .ok()
                .flatten()
                .is_some_and(|t| matches!(hc::status(&t).as_str(), "working" | "blocked"))
        });
    Ok(format!(
        "pinged {handle} as {role}, pane {pane}, prompt {prompt}{}",
        if confirmed {
            ""
        } else {
            " (prompt acceptance unconfirmed)"
        }
    ))
}

#[cfg(test)]
use crate::store_fixture;

#[cfg(test)]
mod tests;
