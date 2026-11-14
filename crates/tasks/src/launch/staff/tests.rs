//! The staffing pass's own cases on a scratch store: an idle agent reused for a job (`hc send` to
//! its terminal, confirmed only by the turn hc saw start or later work), an idle reviewer picked
//! only when the board would take its verdict, a review dispatch that fails or whose reviewer the
//! board refuses and cancels its instruction, and an ended pass freeing its lock while a forked
//! child still holds the descriptor. hc is a stand-in here (`NoHc`, `ReusedTerminal`): no terminal
//! is started.

use super::store_fixture::{Fixture, Seed};
use super::*;
use std::cell::Cell;

use crate::launch::LaunchError;

struct NoHc;

impl Hc for NoHc {
    fn run(&self, _args: &[&str]) -> Result<String, LaunchError> {
        panic!("failed checkout must not reach hc")
    }

    fn feed(&self, _args: &[&str], _input: &str) -> Result<String, LaunchError> {
        panic!("failed checkout must not reach hc")
    }

    fn app(&self, _args: &[&str]) -> Result<String, LaunchError> {
        panic!("failed checkout must not reach the Herdr app")
    }

    fn caller(&self) -> Option<String> {
        None
    }

    fn board(&self) -> String {
        "fixture/tasks".into()
    }
}

/// The idle agent's terminal `t1`: `outcome` says how it answers once the job is sent.
struct ReusedTerminal<'a> {
    outcome: &'a str,
    sent: Cell<bool>,
    looks: Cell<usize>,
}

impl Hc for ReusedTerminal<'_> {
    fn run(&self, args: &[&str]) -> Result<String, LaunchError> {
        match args {
            ["get", "t1", "--json"] => {
                if self.sent.get() {
                    self.looks.set(self.looks.get() + 1);
                    match self.outcome {
                        "failed get" => return Err(LaunchError("hc get t1: boom".into())),
                        "gone" => {
                            return Err(LaunchError("hc get t1: not_found: none".into()));
                        }
                        _ => {}
                    }
                }
                let status = match (self.sent.get(), self.outcome) {
                    (true, "working later") => "working",
                    (true, "done") => "done",
                    _ => "idle",
                };
                Ok(serde_json::json!({
                    "address": "t1", "state": "live", "status": {"app": "codex", "state": status},
                })
                .to_string())
            }
            ["send", "t1", _, "--json"] => {
                self.sent.set(true);
                let started = self.outcome == "started";
                Ok(
                    serde_json::json!({"started": started, "status": "idle", "turn": 1})
                        .to_string(),
                )
            }
            _ => panic!("reuse must not press keys or close its terminal: {args:?}"),
        }
    }

    fn feed(&self, args: &[&str], _input: &str) -> Result<String, LaunchError> {
        panic!("reuse needs no connection: {args:?}")
    }

    fn app(&self, args: &[&str]) -> Result<String, LaunchError> {
        panic!("reuse must not change the window: {args:?}")
    }

    fn caller(&self) -> Option<String> {
        None
    }

    fn board(&self) -> String {
        "fixture/tasks".into()
    }
}

impl ReusedTerminal<'_> {
    fn answering(outcome: &str) -> ReusedTerminal<'_> {
        ReusedTerminal {
            outcome,
            sent: Cell::new(false),
            looks: Cell::new(0),
        }
    }
}

/// `handle`, an idle `kind` agent at rest in the terminal `t1`, detached in `worktree`.
fn idle(handle: &str, kind: &str, worktree: &str) -> agents::Agent {
    agents::Agent {
        handle: handle.into(),
        kind: kind.into(),
        model: "gpt-6.1-sol".into(),
        parent: None,
        task: None,
        role: None,
        tasks: Vec::new(),
        worktree: worktree.into(),
        branch: String::new(),
        agent: Some("codex".into()),
        session_id: Some("existing-conversation".into()),
        pane: Some("t1".into()),
        callback: None,
        chat: false,
        state: "idle",
        detail: None,
        health: Health::Live,
        last_tool_call: None,
        last_message: None,
        slots: None,
        session_url: None,
        in_process: false,
    }
}

/// An idle reviewer of `handle` in a checkout that is a folder.
fn idle_reviewer(handle: &str) -> agents::Agent {
    idle(handle, "reviewer", env!("CARGO_MANIFEST_DIR"))
}

/// The sessions a review's requests and work name: its filer, its orchestrator before and now, its
/// executor and a fresh one.
const FILER: &str = "coral-hare";
const ORCHESTRATED: &str = "ochre-owl";
const ORCHESTRATES: &str = "amber-heron";
const EXECUTED: &str = "slate-wren";
const FRESH: &str = "fleet-vole";

fn reviewing() -> Fixture {
    let seed = [FILER, ORCHESTRATED, ORCHESTRATES, EXECUTED, FRESH];
    Fixture::new(seed.into_iter().fold(Seed::default(), Seed::session))
}

/// `slug` in review: filed by [`FILER`], orchestrated by [`ORCHESTRATED`], which handed it to
/// [`ORCHESTRATES`], and executed by [`EXECUTED`], which has let its role go; a fast fix when `fast`.
fn in_review(f: &Fixture, slug: &str, fast: bool) {
    let store = &f.store;
    store
        .create(&NewTask {
            fast_fix: fast.then(|| "a one-line fix, its test posted".into()),
            ..NewTask::new(slug, "A review", FILER)
        })
        .unwrap();
    store.take_role(slug, ORCHESTRATED, "orchestrator").unwrap();
    store.hand_over(slug, ORCHESTRATED, ORCHESTRATES).unwrap();
    store.claim(slug, EXECUTED).unwrap();
    store.move_to(slug, "review", EXECUTED).unwrap();
    store.release(slug, EXECUTED, Some("executor")).unwrap();
}

#[test]
fn an_idle_reviewer_is_picked_only_when_the_board_takes_its_verdict() {
    let f = reviewing();
    for slug in ["sim/jump", "sim/fast"] {
        in_review(&f, slug, slug == "sim/fast");
    }
    let board = f.store.board().unwrap();
    let picked = |slug: &str, from: &[&str]| {
        let task = board.tasks.iter().find(|t| t.slug == slug).unwrap();
        let agents: Vec<_> = from.iter().map(|handle| idle_reviewer(handle)).collect();
        let held = f.store.ever_held(slug).unwrap();
        idle_for(&agents, &board, task, "reviewer", &HashSet::new(), &held)
            .map(|a| a.handle.clone())
    };
    // Its executor, its filer and its orchestrator before are passed over for a fresh one.
    let unfresh = [EXECUTED, FILER, ORCHESTRATED];
    assert_eq!(
        picked("sim/jump", &[EXECUTED, FILER, ORCHESTRATED, FRESH]).as_deref(),
        Some(FRESH)
    );
    assert_eq!(picked("sim/jump", &unfresh), None);
    for one in unfresh {
        assert_eq!(picked("sim/jump", &[one]), None, "{one}");
    }
    // On a fast fix only its executor is: who requested it may judge it.
    assert_eq!(picked("sim/fast", &[EXECUTED]), None);
    assert_eq!(
        picked("sim/fast", &[EXECUTED, FILER]).as_deref(),
        Some(FILER)
    );
    assert_eq!(
        picked("sim/fast", &[ORCHESTRATED]).as_deref(),
        Some(ORCHESTRATED)
    );
    // The orchestrator it has now orchestrates work, which no idle pick takes, fast fix or not.
    assert_eq!(picked("sim/fast", &[ORCHESTRATES]), None);
}

/// A machine a reuse whose turn hc saw start runs on: it launches, looks and inspects nothing.
fn reusing() -> Machine {
    Machine {
        root: PathBuf::new(),
        wt_base: PathBuf::new(),
        new_id: Box::new(|| panic!("reuse must not start another conversation")),
        sleep: Box::new(|_| panic!("a started turn needs no look")),
        processes: Box::new(|_| panic!("reuse must not inspect processes")),
        mode: "auto".into(),
        board_env: Vec::new(),
    }
}

/// The review of sim/jump's tip a job is filed for.
fn jump_review() -> ReviewTarget {
    ReviewTarget {
        branch: "sim/jump".into(),
        tip: "0123abc".into(),
    }
}

#[test]
fn a_reused_reviewer_the_board_refuses_is_left_no_job_and_the_job_is_cancelled() {
    let f = reviewing();
    in_review(&f, "sim/jump", false);
    let (machine, target) = (reusing(), jump_review());
    let hc = ReusedTerminal::answering("started");
    let launching = Defaults::default().resolve(None, None);
    let task = f.store.get("sim/jump").unwrap();
    let staff = |agent: &agents::Agent| {
        dispatch(
            &f.store,
            &hc,
            &machine,
            &launching,
            &task,
            ORCHESTRATES,
            Some(&target),
            Some(agent),
        )
    };

    // Its filer, as a pass that read the board before the rule would pick it.
    let error = staff(&idle_reviewer(FILER)).unwrap_err();
    assert!(
        error
            .to_string()
            .contains(&format!("{FILER} filed sim/jump and cannot review it")),
        "{error}"
    );
    assert!(!hc.sent.get(), "no prompt went to the filer");
    let job = f.store.get("sim/jump/staff-review-1").unwrap();
    assert_eq!(job.status, "cancelled");
    assert!(job.holders("reserved").is_empty(), "{:?}", job.roles);
    assert!(f.store.task_sessions(&job.slug).unwrap().is_empty());
    assert!(
        f.store
            .get("sim/jump")
            .unwrap()
            .holders("reviewer")
            .is_empty()
    );

    // The next job goes to a fresh reviewer, reserved for it once it holds the role.
    let line = staff(&idle_reviewer(FRESH)).unwrap();
    assert!(
        line.starts_with(&format!(
            "pinged {FRESH} as reviewer, pane t1, prompt $review-task sim/jump/staff-review-2 {FRESH}"
        )),
        "{line}"
    );
    assert_eq!(
        f.store
            .get("sim/jump/staff-review-2")
            .unwrap()
            .holders("reserved"),
        [FRESH]
    );
    assert_eq!(
        f.store.get("sim/jump").unwrap().holders("reviewer"),
        [FRESH]
    );
}

#[test]
fn a_reused_reviewer_whose_job_is_reserved_first_lets_its_role_go_with_the_job() {
    let f = reviewing();
    in_review(&f, "sim/jump", false);
    // Another session reserves the job once the reused reviewer holds the role, before its own
    // reservation: the role taken, the job is not its to have.
    let (other, taken) = (f.other(), std::sync::atomic::AtomicBool::new(false));
    let store = f.busy(move || {
        use std::sync::atomic::Ordering::SeqCst;
        let holds = |slug| {
            other
                .get(slug)
                .is_ok_and(|t| t.holders("reviewer") == [FRESH])
        };
        if !taken.load(SeqCst)
            && holds("sim/jump")
            && (other.reserve("sim/jump/staff-review-1", ORCHESTRATED, 3600.0)).is_ok()
        {
            taken.store(true, SeqCst);
        }
    });
    let hc = ReusedTerminal::answering("started");
    let task = f.store.get("sim/jump").unwrap();
    let error = dispatch(
        &store,
        &hc,
        &reusing(),
        &Defaults::default().resolve(None, None),
        &task,
        ORCHESTRATES,
        Some(&jump_review()),
        Some(&idle_reviewer(FRESH)),
    )
    .unwrap_err();
    let reserved = format!("sim/jump/staff-review-1 is reserved by {ORCHESTRATED}");
    assert!(error.to_string().contains(&reserved), "{error}");
    assert!(!hc.sent.get(), "no prompt went to a reviewer with no job");
    let job = f.store.get("sim/jump/staff-review-1").unwrap();
    assert_eq!(job.status, "cancelled");
    assert!(f.store.task_sessions(&job.slug).unwrap().is_empty());
    let reviewed = f.store.get("sim/jump").unwrap();
    assert!(
        reviewed.holders("reviewer").is_empty(),
        "{:?}",
        reviewed.roles
    );
    let said = f.said("sim/jump");
    let let_go = format!("{FRESH} released reviewer (released)");
    assert!(said.contains(&let_go), "{said:?}");
}

#[test]
fn reuse_reports_unconfirmed_acceptance_unless_hc_saw_the_turn_start() {
    for outcome in ["started", "working later", "done", "failed get", "gone"] {
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
            board_env: Vec::new(),
        };
        let agent = idle("amber-heron", "worker", "fixture-reuse");
        let hc = ReusedTerminal::answering(outcome);
        let task = f.store.get("sim/jump").unwrap();
        let line = reuse(&f.store, &hc, &machine, &agent, &task, &task, "executor").unwrap();
        let expected = "pinged amber-heron as executor, pane t1, prompt $work sim/jump amber-heron";
        match outcome {
            "started" => {
                assert_eq!(line, expected);
                assert_eq!(hc.looks.get(), 0);
            }
            "working later" => {
                assert_eq!(line, expected);
                assert_eq!(hc.looks.get(), 1);
            }
            _ => {
                assert_eq!(
                    line,
                    format!("{expected} (prompt acceptance unconfirmed)"),
                    "{outcome}"
                );
                assert_eq!(hc.looks.get(), super::super::SEND_LOOKS);
            }
        }
        assert!(hc.sent.get());
        let records = f.store.task_sessions("sim/jump").unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].pane, "t1");
        assert_eq!(records[0].claude_session, "codex:existing-conversation");
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
        board_env: Vec::new(),
    };
    let task = f.store.get("sim/jump").unwrap();
    let error = dispatch(
        &f.store,
        &NoHc,
        &machine,
        &Defaults::default().resolve(None, None),
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
    let root = std::env::temp_dir().join(format!("symphony-staff-release-{}", std::process::id()));
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
