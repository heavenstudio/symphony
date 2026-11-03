//! One bounded staffing pass. The Symphony already calls `reap` every five minutes; that command
//! runs this first, so an idle agent can take work before the idle sweep closes its pane.

use super::herdr::{Binary, Herdr};
use super::{Ask, Machine, launch, prompt_text};
use crate::cli::{Env, agents, probes};
use crate::health::{EXECUTING, FINISHED};
use crate::sys::{Command, FileLock};
use crate::{Board, Error, Health, NewLaunch, NewTask, Store, Task, git};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};

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
    let dir = git::common_dir(&env.cwd).map_err(refused)?.join("sf-tasks");
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

fn eligible(task: &Task, board: &Board, config: &Config) -> bool {
    let role = match task.status.as_str() {
        "ready" => "executor",
        "review" => "reviewer",
        _ => return false,
    };
    if owner_only(task)
        || orchestrator(task, board).is_none()
        || (!config.only.is_empty()
            && !config
                .only
                .iter()
                .any(|s| task.slug == *s || task.slug.starts_with(&format!("{s}/"))))
        || !task.depends.iter().all(|d| {
            board
                .tasks
                .iter()
                .any(|t| t.slug == *d && t.status == "done")
        })
        || task
            .roles
            .iter()
            .any(|r| (r.role == role || r.role == "reserved") && r.health.holds())
    {
        return false;
    }
    // Work on a child already staffs the parent. Never dispatch an overlapping executor/review.
    !board.tasks.iter().any(|child| {
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
    })
}

/// Available memory, including reclaimable inactive pages on macOS (MemAvailable on Linux).
fn free_memory_gib() -> Result<f64, Error> {
    let bytes = if cfg!(target_os = "macos") {
        let output = crate::sys::output(&mut Command::new("vm_stat")).map_err(refused)?;
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

/// A review is an instruction child: `/work` can claim it and the parent keeps its reviewer role.
fn review_job(
    store: &Store,
    task: &Task,
    actor: &str,
    target: &ReviewTarget,
) -> Result<Task, Error> {
    let ReviewTarget { branch, tip } = target;
    let tasks = store.tasks(None)?;
    let slug = (1..)
        .map(|n| format!("{}/staff-review-{n}", task.slug))
        .find(|slug| !tasks.iter().any(|t| t.slug == *slug))
        .expect("unbounded review names");
    store.create(&NewTask {
        slug, title: format!("Review {}", task.slug), parent: Some(task.slug.clone()),
        instruction: true, priority: task.priority, by: actor.into(),
        body: format!("Review {branch} at tip {tip}. In your detached checkout, check out {tip}. Read for breakage (a hang, a wrong exit, a lost record, or a command that stopped working) and for tests that cannot fail. Follow the interim bar in AGENTS.md: the executor posts verify, check and lint-rs green on a tip containing main; a clean merge of main reruns check and lint-rs only, and the orchestrator merges. Do not run another verify or request a stamp. Post findings on {}. Finish with just tasks verdict {} approve|changes followed by the reason, then close this instruction with just tasks done and a complete result note.", task.slug, task.slug),
        ..NewTask::default()
    })
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
    let herdr = Binary::of(env);
    let mut global = env.clone();
    global.vars.remove("HERDR_WORKSPACE_ID");
    let global_herdr = Binary::of(&global);
    let machine = Machine::of(env);
    let mut candidates = store.board()?.tasks;
    candidates.retain(|t| matches!(t.status.as_str(), "ready" | "review"));
    candidates.sort_by(|a, b| {
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
        if !eligible(task, &board, &config) {
            continue;
        }
        let actor = orchestrator(task, &board).expect("eligible has a live orchestrator");
        let review = if task.status == "review" {
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
                "staff needs herdr and slot probes; nothing dispatched",
            ));
        }
        let global_agents = global_herdr.agents()?;
        let live = global_agents.len()
            + view
                .agents
                .iter()
                .filter(|a| {
                    a.health == Health::Live
                        && a.agent.is_none()
                        && matches!(a.kind.as_str(), "worker" | "reviewer" | "orchestrator")
                        && !global_agents.iter().any(|p| {
                            super::text_of(p, "name") == a.handle
                                || a.pane.as_deref() == Some(super::text_of(p, "pane_id").as_str())
                        })
                })
                .count()
            + projected;
        let role = if task.status == "review" {
            "reviewer"
        } else {
            "executor"
        };
        let idle = view.agents.iter().find(|a| {
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
                && !a
                    .tasks
                    .iter()
                    .any(|t| !FINISHED.contains(&t.status.as_str()))
                && !board.tasks.iter().any(|t| {
                    t.roles
                        .iter()
                        .any(|r| r.handle == a.handle && r.role == "orchestrator")
                })
                && Path::new(&a.worktree).is_dir()
                && !view.agents.iter().any(|other| {
                    other.health.holds()
                        && other.worktree == a.worktree
                        && other
                            .tasks
                            .iter()
                            .any(|t| t.role == "executor" && !FINISHED.contains(&t.status.as_str()))
                })
                && if role == "reviewer" {
                    a.branch.is_empty() && !task.roles.iter().any(|r| r.handle == a.handle)
                } else if task.instruction {
                    a.branch.is_empty()
                } else {
                    a.branch == task.slug
                }
        });
        let memory = free_memory_gib()?;
        let blocked = if idle.is_none() && live >= config.max_sessions {
            Some(format!("session cap {live}/{}", config.max_sessions))
        } else if memory < config.min_free_memory_gib {
            Some(format!(
                "free memory {memory:.1} GiB below {:.1} GiB",
                config.min_free_memory_gib
            ))
        } else {
            None
        };
        let mut dispatched = false;
        let action = if let Some(why) = blocked {
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
            } else {
                projected += 1;
                format!("staff {}: would launch codex as {role}", task.slug)
            }
        } else {
            if let Some(a) = idle {
                used.insert(a.handle.clone());
            }
            let outcome = dispatch(store, &herdr, &machine, task, actor, review.as_ref(), idle);
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

/// Keep task-specific failures (target, child creation, checkout, launch and reuse) inside this job.
fn dispatch(
    store: &Store,
    herdr: &dyn Herdr,
    machine: &Machine,
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
    let job = match review {
        Some(target) => review_job(store, task, actor, target)?,
        None => task.clone(),
    };
    let outcome = (|| {
        if let Some(agent) = idle {
            return reuse(store, herdr, machine, agent, task, &job, role);
        }
        let mut ask = Ask::new(&job.slug, actor, "gpt-6.1-sol");
        ask.role = role.into();
        if role == "executor"
            && !task.instruction
            && let Some(path) = super::checked_out_in(&machine.root, &task.slug)?
        {
            ask.worktree = Some(PathBuf::from(path));
        }
        launch(store, herdr, machine, &ask).map(|s| {
            format!(
                "launched {} as {role}, pane {}, worktree {}, prompt {}{}",
                s.row.handle,
                s.row.pane,
                s.row.worktree,
                prompt_text(&job.slug, &s.row.handle, "", "codex"),
                if s.stuck {
                    " (prompt still in input)"
                } else {
                    ""
                },
            )
        })
    })();
    if outcome.is_err() && review.is_some() && idle.is_none() {
        let _ = store.move_to(&job.slug, "cancelled", actor);
    }
    outcome
}

fn reuse(
    store: &Store,
    herdr: &dyn Herdr,
    machine: &Machine,
    agent: &agents::Agent,
    target: &Task,
    job: &Task,
    role: &str,
) -> Result<String, Error> {
    let handle = &agent.handle;
    let pane = agent.pane.as_deref().expect("idle has a pane");
    let current = herdr.agents()?;
    if !current.iter().any(|a| {
        super::text_of(a, "pane_id") == pane
            && super::READY_STATES.contains(&super::text_of(a, "agent_status").as_str())
    }) {
        return Err(refused("idle agent became busy"));
    }
    if let Some(other) = super::live_executor_in(store, Path::new(&agent.worktree))? {
        return Err(refused(format!("checkout has live executor {other}")));
    }
    store.revive(handle)?;
    store.reserve(&job.slug, handle, crate::health::RESERVE_SECONDS)?;
    if role == "reviewer" {
        store.take_role(&target.slug, handle, "reviewer")?;
    }
    let kind = agent.agent.as_deref().unwrap_or("codex");
    let prompt = prompt_text(&job.slug, handle, "", kind);
    // Record before prompting: once the prompt is sent, a timeout must never free this job for a duplicate.
    store.record_launch(&NewLaunch {
        slug: job.slug.clone(),
        handle: handle.clone(),
        role: role.into(),
        pane: pane.into(),
        claude_session: format!(
            "{}{}",
            if kind == "codex" { "codex:" } else { "" },
            agent.session_id.as_deref().unwrap_or_default()
        ),
        worktree: agent.worktree.clone(),
        branch: agent.branch.clone(),
        model: agent.model.clone(),
        ..NewLaunch::default()
    })?;
    herdr.call(&["agent", "prompt", pane, &prompt])?;
    let confirmed = super::taken(herdr, pane, &prompt, &*machine.sleep).0;
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
#[path = "../../tests/store/common.rs"]
#[expect(
    dead_code,
    reason = "reuse the scratch store harness; this regression needs only part of it"
)]
mod store_fixture;

#[cfg(test)]
mod tests {
    use super::store_fixture::{Fixture, Seed};
    use super::*;
    use std::cell::Cell;

    struct NoHerdr;

    impl Herdr for NoHerdr {
        fn run(&self, _args: &[&str]) -> Result<String, super::super::LaunchError> {
            panic!("failed checkout must not reach herdr")
        }

        fn workspace(&self) -> Option<String> {
            None
        }
    }

    struct ReusedPane<'a> {
        outcome: &'a str,
        prompted: Cell<bool>,
        looks: Cell<usize>,
    }

    impl Herdr for ReusedPane<'_> {
        fn run(&self, args: &[&str]) -> Result<String, crate::launch::LaunchError> {
            match args {
                ["agent", "list"] => {
                    if self.prompted.get() {
                        self.looks.set(self.looks.get() + 1);
                        match self.outcome {
                            "agent list" => {
                                return Err(crate::launch::LaunchError("failed list".into()));
                            }
                            "missing agent" => return Ok(r#"{"result":{"agents":[]}}"#.into()),
                            _ => {}
                        }
                    }
                    let status = if self.prompted.get() && self.outcome == "working" {
                        "working"
                    } else {
                        "idle"
                    };
                    Ok(serde_json::json!({
                        "result": {"agents": [{"pane_id": "w1:p1", "agent_status": status}]}
                    })
                    .to_string())
                }
                ["agent", "prompt", "w1:p1", _] => {
                    self.prompted.set(true);
                    Ok("{}".into())
                }
                ["pane", "read", "w1:p1", "--lines", "12"] => {
                    if self.outcome == "failed read" {
                        Err(crate::launch::LaunchError("failed read".into()))
                    } else {
                        Ok(String::new())
                    }
                }
                _ => panic!("reuse must not send keys or close its pane: {args:?}"),
            }
        }

        fn workspace(&self) -> Option<String> {
            None
        }
    }

    #[test]
    fn reuse_reports_unconfirmed_acceptance_without_claiming_the_prompt_is_visible() {
        for outcome in [
            "working",
            "absent head",
            "failed read",
            "agent list",
            "missing agent",
        ] {
            let f = Fixture::new(
                Seed::default()
                    .session("amber-heron")
                    .task("sim/jump", "ready"),
            );
            let machine = Machine {
                root: PathBuf::new(),
                wt_base: PathBuf::new(),
                new_id: Box::new(|| panic!("reuse must not start another conversation")),
                sleep: Box::new(|seconds| assert_eq!(seconds, super::super::PROMPT_WAIT)),
                processes: Box::new(|_| panic!("reuse must not inspect processes")),
                mode: "auto".into(),
            };
            let agent = agents::Agent {
                handle: "amber-heron".into(),
                kind: "worker".into(),
                model: "gpt-6.1-sol".into(),
                parent: None,
                task: None,
                role: None,
                tasks: Vec::new(),
                worktree: "fixture-reuse".into(),
                branch: String::new(),
                agent: Some("codex".into()),
                session_id: Some("existing-conversation".into()),
                pane: Some("w1:p1".into()),
                callback: None,
                chat: false,
                state: "idle",
                detail: None,
                health: Health::Live,
                last_tool_call: None,
                last_message: None,
            };
            let herdr = ReusedPane {
                outcome,
                prompted: Cell::new(false),
                looks: Cell::new(0),
            };
            let task = f.store.get("sim/jump").unwrap();
            let line = reuse(&f.store, &herdr, &machine, &agent, &task, &task, "executor").unwrap();
            let expected =
                "pinged amber-heron as executor, pane w1:p1, prompt $work sim/jump amber-heron";
            if outcome == "working" {
                assert_eq!(line, expected);
                assert_eq!(herdr.looks.get(), 1);
            } else {
                assert_eq!(
                    line,
                    format!("{expected} (prompt acceptance unconfirmed)"),
                    "{outcome}"
                );
                assert_eq!(herdr.looks.get(), super::super::PROMPT_ENTERS + 1);
            }
            assert!(herdr.prompted.get());
            let records = f.store.task_sessions("sim/jump").unwrap();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].pane, "w1:p1");
            assert!(records[0].closed.is_none());
            assert_eq!(
                f.store.get("sim/jump").unwrap().holders("reserved"),
                ["amber-heron"]
            );
        }
    }

    #[test]
    fn a_failed_staff_review_dispatch_cancels_its_instruction_on_the_store() {
        let f = Fixture::new(
            Seed::default()
                .session("amber-heron")
                .task("sim/jump", "review")
                .role("sim/jump", "amber-heron", "orchestrator"),
        );
        // A path below a regular file cannot be a checkout or create a worktree. No repository,
        // agent process or staffing configuration is touched by this failed dispatch.
        let machine = Machine {
            root: std::env::temp_dir(),
            wt_base: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml/not-a-folder"),
            new_id: Box::new(|| "unused".into()),
            sleep: Box::new(|_| panic!("failed checkout must not wait")),
            processes: Box::new(|_| panic!("failed checkout must not inspect processes")),
            mode: "auto".into(),
        };
        let task = f.store.get("sim/jump").unwrap();
        let error = dispatch(
            &f.store,
            &NoHerdr,
            &machine,
            &task,
            "amber-heron",
            Some(&ReviewTarget {
                branch: "sim/jump".into(),
                tip: "unused".into(),
            }),
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("not-a-folder"), "{error}");
        let job = f.store.get("sim/jump/staff-review-1").unwrap();
        assert!(job.instruction);
        assert_eq!(job.parent.as_deref(), Some("sim/jump"));
        assert_eq!(job.status, "cancelled");
        assert_eq!(f.store.get("sim/jump").unwrap().status, "review");
        assert_eq!(
            f.said(&job.slug).last().unwrap(),
            "amber-heron moved it ready → cancelled"
        );
    }

    #[test]
    fn an_ended_pass_frees_its_lock_while_a_forked_child_still_holds_the_descriptor() {
        let root = std::env::temp_dir().join(format!("sf-staff-release-{}", std::process::id()));
        // An empty `.git` folder is a common dir to `git::common_dir`, which runs no git for it.
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let env = Env {
            vars: std::collections::BTreeMap::new(),
            cwd: root.clone(),
        };
        let held = lock(&env).unwrap().unwrap();
        // What fork gives a child until it execs: the same open file, so the same lock.
        let inherited = held.0.try_clone().unwrap();
        drop(held);
        let next = lock(&env).unwrap();
        assert!(next.is_some(), "the ended pass still holds its lock");
        assert!(lock(&env).unwrap().is_none());
        drop((inherited, next));
        std::fs::remove_dir_all(root).unwrap();
    }
}
