//! The staffing pass's own cases on a scratch store: an idle agent reused for a job (`hc send` to
//! its terminal, confirmed only by the turn hc saw start or later work), an idle reviewer picked
//! only when the board would take its verdict, a review given on the task itself and never as a
//! subtask, whether it is handed, refused or fails, and an ended pass freeing its lock while a
//! forked child still holds the descriptor. hc is a stand-in here (`NoHc`, `ReusedTerminal`): no
//! terminal is started.

use super::store_fixture::{Fixture, Seed};
use super::*;
use crate::NewTask;
use crate::rows::TaskSessionRow;
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

/// A machine whose only live terminal, `t9`, runs `handle` (none for `None`): what a launch asks
/// to tell whether a recorded reviewer still runs.
struct Running(Option<&'static str>);

impl Hc for Running {
    fn run(&self, args: &[&str]) -> Result<String, LaunchError> {
        assert_eq!(args.first(), Some(&"ls"), "only listed: {args:?}");
        let terminals = self.0.map_or_else(Vec::new, |handle| {
            vec![serde_json::json!({"address": "t9", "state": "live", "name": handle})]
        });
        Ok(serde_json::json!({ "terminals": terminals }).to_string())
    }

    fn feed(&self, args: &[&str], _input: &str) -> Result<String, LaunchError> {
        panic!("a job's choice feeds nothing: {args:?}")
    }

    fn app(&self, args: &[&str]) -> Result<String, LaunchError> {
        panic!("a job's choice changes no window: {args:?}")
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
            ["send", "t1", _, "--json"] if self.outcome == "unsent" => {
                Err(LaunchError("hc send t1: boom".into()))
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
        following: 0,
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
        confirmed: false,
        last_tool_call: None,
        last_message: None,
        slots: None,
        session_url: None,
        in_process: false,
        tokens: None,
        cost: None,
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
/// Another fresh one.
const SECOND: &str = "dune-lark";

fn reviewing() -> Fixture {
    let seed = [FILER, ORCHESTRATED, ORCHESTRATES, EXECUTED, FRESH, SECOND];
    Fixture::new(seed.into_iter().fold(Seed::default(), Seed::session))
}

/// `slug` in review: filed by [`FILER`], orchestrated by [`ORCHESTRATED`], which handed it to
/// [`ORCHESTRATES`], and executed by [`EXECUTED`], which has let its role go; a fast fix when `fast`.
fn in_review(f: &Fixture, slug: &str, fast: bool) {
    let store = &f.store;
    store
        .create(&NewTask::new(slug, "A review", FILER))
        .unwrap();
    if fast {
        store
            .raw()
            .row("task", slug)
            .set(serde_json::json!({
                "fields._board.fast_fix": "a one-line fix, its test posted"
            }))
            .unwrap();
    }
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
        idle_for(
            &agents,
            &board,
            task,
            "reviewer",
            &HashSet::new(),
            &held,
            None,
            &Defaults::default(),
        )
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
    // A legacy mark gives no requester a reviewer's role.
    assert_eq!(
        picked("sim/fast", &[EXECUTED, FILER, ORCHESTRATED, FRESH]).as_deref(),
        Some(FRESH)
    );
    for one in [EXECUTED, FILER, ORCHESTRATED, ORCHESTRATES] {
        assert_eq!(picked("sim/fast", &[one]), None, "{one}");
    }
}

#[test]
fn a_review_goes_back_to_the_tasks_recorded_reviewer_never_to_another_idle_one() {
    let f = reviewing();
    in_review(&f, "sim/jump", false);
    let idle = [idle_reviewer(SECOND)];
    let job_in = |hc: &dyn Hc| {
        let board = f.store.board().unwrap();
        let task = board.tasks.iter().find(|t| t.slug == "sim/jump").unwrap();
        let held = f.store.ever_held("sim/jump").unwrap();
        let wanted = Job {
            board: &board,
            task,
            role: "reviewer",
            held: &held,
            operating: None,
        };
        let defaults = Defaults::default();
        let (resumed, picked) = job_for(&f.store, hc, &idle, &HashSet::new(), &wanted, &defaults)?;
        Ok::<_, Error>((
            resumed.map(|(handle, _)| handle),
            picked.map(|a| a.handle.clone()),
        ))
    };
    let job = || job_in(&Running(None)).unwrap();
    let record = |conversation: &str| {
        f.store
            .record_launch(&NewLaunch {
                claude_session: conversation.into(),
                worktree: env!("CARGO_MANIFEST_DIR").into(),
                ..NewLaunch::new("sim/jump", FRESH, "reviewer")
            })
            .unwrap();
    };
    // No reviewer recorded: the idle fresh one.
    let another = (None, Some(SECOND.to_string()));
    assert_eq!(job(), another);
    // One recorded with no conversation, or a Codex thread that never arrived: none to go back to.
    for none in ["", "codex:"] {
        record(none);
        assert_eq!(job(), another, "{none:?}");
    }
    // One recorded with a conversation: back to it, and the idle one is passed over.
    record("codex:0abc-def");
    assert_eq!(job(), (Some(FRESH.to_string()), None));
    // While it still runs, the job fails as its launch would, the idle one passed over all the same.
    let still = job_in(&Running(Some(FRESH))).unwrap_err().to_string();
    assert!(
        still.starts_with(&format!(
            "{FRESH} reviews sim/jump and still runs in terminal t9"
        )),
        "{still}"
    );
    // An executor's record is no reviewer's.
    f.store
        .record_launch(&NewLaunch {
            claude_session: "work".into(),
            worktree: env!("CARGO_MANIFEST_DIR").into(),
            ..NewLaunch::new("sim/jump", EXECUTED, "executor")
        })
        .unwrap();
    assert_eq!(job(), (Some(FRESH.to_string()), None));
    // Once it left, another takes the review.
    f.store.leave(FRESH).unwrap();
    assert_eq!(job(), another);
}

/// A folder of this process's named `name`, holding a runnable stand-in for each of `agents`:
/// the search path a launch finds their programs on.
fn programs(name: &str, agents: &[&str]) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("staff-programs-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for agent in agents {
        let program = dir.join(agent);
        std::fs::write(&program, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    dir
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
        bounds: None,
        path: None,
    }
}

/// The review of sim/jump's tip a job is filed for.
fn jump_review() -> ReviewTarget {
    ReviewTarget {
        branch: "sim/jump".into(),
        tip: "0123abc".into(),
    }
}

/// The subtasks under `slug`: a review is given on the task itself, so it files none.
fn subtasks(f: &Fixture, slug: &str) -> Vec<String> {
    let under = format!("{slug}/");
    let tasks = f.store.tasks(None).unwrap();
    tasks
        .into_iter()
        .filter(|t| t.slug.starts_with(&under))
        .map(|t| t.slug)
        .collect()
}

/// `handle`'s launch recorded on `slug`, if any.
fn recorded(f: &Fixture, slug: &str, handle: &str) -> Option<TaskSessionRow> {
    let ran = f.store.task_sessions(slug).unwrap();
    ran.into_iter().find(|s| s.handle == handle)
}

#[test]
fn a_reused_reviewer_the_board_refuses_is_handed_nothing_and_a_fresh_one_reviews_the_task_itself() {
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
    assert_eq!(subtasks(&f, "sim/jump"), Vec::<String>::new());
    assert_eq!(recorded(&f, "sim/jump", FILER), None);
    let jump = f.store.get("sim/jump").unwrap();
    assert!(jump.holders("reviewer").is_empty(), "{:?}", jump.roles);

    // The next goes to a fresh reviewer, on the task it reviews: its role and its launch are
    // recorded there, it reserves nothing (a reviewer never reserves the task it judges), and no
    // subtask is filed.
    let line = staff(&idle_reviewer(FRESH)).unwrap();
    assert_eq!(
        line,
        format!("pinged {FRESH} as reviewer, pane t1, prompt $review-task sim/jump {FRESH}")
    );
    assert!(hc.sent.get());
    assert_eq!(subtasks(&f, "sim/jump"), Vec::<String>::new());
    let jump = f.store.get("sim/jump").unwrap();
    assert_eq!(jump.holders("reviewer"), [FRESH]);
    assert!(jump.holders("reserved").is_empty(), "{:?}", jump.roles);
    let launch = recorded(&f, "sim/jump", FRESH).expect("its launch is recorded on the task");
    assert_eq!(launch.role, "reviewer");
    assert_eq!(launch.pane, "t1");
    assert_eq!(launch.claude_session, "codex:existing-conversation");
}

#[test]
fn a_reused_reviewer_its_prompt_may_have_reached_keeps_its_role() {
    let f = reviewing();
    in_review(&f, "sim/jump", false);
    let hc = ReusedTerminal::answering("unsent");
    let task = f.store.get("sim/jump").unwrap();
    let error = dispatch(
        &f.store,
        &hc,
        &reusing(),
        &Defaults::default().resolve(None, None),
        &task,
        ORCHESTRATES,
        Some(&jump_review()),
        Some(&idle_reviewer(FRESH)),
    )
    .unwrap_err();
    assert!(error.to_string().contains("hc send t1: boom"), "{error}");
    // Recorded before the prompt went, so it may have the review: it keeps the role, and its
    // record says where it is.
    assert_eq!(
        f.store.get("sim/jump").unwrap().holders("reviewer"),
        [FRESH]
    );
    let launch = recorded(&f, "sim/jump", FRESH).expect("recorded before the prompt");
    assert_eq!(launch.role, "reviewer");
    assert_eq!(subtasks(&f, "sim/jump"), Vec::<String>::new());
}

#[test]
fn a_reused_reviewer_whose_launch_is_not_recorded_lets_its_role_go() {
    use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
    let f = reviewing();
    in_review(&f, "sim/jump", false);
    // Once the reused reviewer holds the role, before its launch is recorded, the board starts
    // refusing a record of its terminal.
    let (probe, client, refusing) = (f.other(), f.other_client(), AtomicBool::new(false));
    let store = f.busy(move || {
        let holds = probe
            .get("sim/jump")
            .is_ok_and(|t| t.holders("reviewer") == [FRESH]);
        if holds && !refusing.swap(true, SeqCst) {
            let refuse = "DEFINE FIELD OVERWRITE pane ON task_session TYPE string \
                ASSERT $value != 't1';";
            client.query(refuse, serde_json::json!({})).unwrap();
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
    assert!(error.to_string().contains("pane"), "{error}");
    // Nothing was sent and nothing recorded, so the task waits on no reviewer with no job.
    assert!(
        !hc.sent.get(),
        "no prompt went to a reviewer with no record"
    );
    assert_eq!(recorded(&f, "sim/jump", FRESH), None);
    let jump = f.store.get("sim/jump").unwrap();
    assert!(jump.holders("reviewer").is_empty(), "{:?}", jump.roles);
    let said = f.said("sim/jump");
    let let_go = format!("{FRESH} released reviewer (released)");
    assert!(said.contains(&let_go), "{said:?}");
    assert_eq!(subtasks(&f, "sim/jump"), Vec::<String>::new());
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
            bounds: None,
            path: None,
        };
        let agent = idle("amber-heron", "worker", "fixture-reuse");
        let hc = ReusedTerminal::answering(outcome);
        let task = f.store.get("sim/jump").unwrap();
        let line = reuse(&f.store, &hc, &machine, &agent, &task, "executor").unwrap();
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
fn a_failed_staff_review_launch_files_no_subtask_and_leaves_the_task_waiting_on_no_reviewer() {
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
        bounds: None,
        // The agent's program found, so the launch goes as far as its checkout.
        path: Some(programs("failed-review", &["codex"]).into_os_string()),
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
    assert_eq!(subtasks(&f, "sim/jump"), Vec::<String>::new());
    let jump = f.store.get("sim/jump").unwrap();
    assert_eq!(jump.status, "review");
    // What the launch took on the task its rollback let go: no reviewer that has no job.
    assert!(jump.holders("reviewer").is_empty(), "{:?}", jump.roles);
    assert!(f.store.task_sessions("sim/jump").unwrap().is_empty());
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

/// The review workflow of [`crate::workflow::operate::tests::REVIEWED`], as a board activates it.
fn reviewed_workflow() -> crate::workflow::Custom {
    use crate::workflow::activate::tests::{MAPPED, custom};
    custom(&format!(
        "{}{MAPPED}",
        crate::workflow::operate::tests::REVIEWED
    ))
}

#[test]
fn a_held_task_wants_the_role_its_open_claim_edges_give_never_its_projected_status() {
    use crate::workflow::operate::tests::{held, holding, legacy, reviewed};
    let w = reviewed();
    let orchestrated = |task| holding(task, ORCHESTRATES, "orchestrator", Health::Live);
    let wants = |task: Task, others: Vec<Task>| {
        let task = orchestrated(task);
        let mut tasks = others;
        tasks.push(task.clone());
        let board = Board {
            tasks,
            sessions: Vec::new(),
            latest_message: 0,
            workflow_generation: None,
        };
        eligible(&task, &board, &Config::default(), Some(&w))
    };
    assert_eq!(
        wants(held("sim/a", "build", "available"), Vec::new()),
        Some("executor")
    );
    assert_eq!(
        wants(held("sim/a", "verify", "awaiting"), Vec::new()),
        Some("reviewer")
    );
    // In review by its projection, yet no edge leaving it gives a reviewer: a launch would fail.
    let reviewing = held("sim/a", "verify", "reviewing");
    assert_eq!(reviewing.status, "review");
    assert_eq!(wants(reviewing, Vec::new()), None);
    assert_eq!(
        wants(legacy("sim/a", "review"), Vec::new()),
        Some("reviewer")
    );
    // Work lost at a position no claim leaves is its recovery's, never a new executor's.
    let lost = holding(
        held("sim/a", "build", "working"),
        EXECUTED,
        "executor",
        Health::Lost,
    );
    assert_eq!(wants(lost, Vec::new()), None);
    assert_eq!(wants(held("sim/a", "verify", "accepted"), Vec::new()), None);
    // The claim's dependency predicate, read from the prerequisite's position.
    let needing = || {
        let mut task = held("sim/a", "build", "available");
        task.depends = vec!["sim/b".into()];
        task
    };
    assert_eq!(
        wants(needing(), vec![held("sim/b", "build", "working")]),
        None
    );
    assert_eq!(
        wants(needing(), vec![held("sim/b", "verify", "approved")]),
        Some("executor")
    );
    // Reserved for a session there: nobody else is started.
    let reserved = holding(
        held("sim/a", "build", "available"),
        FRESH,
        "reserved",
        Health::Live,
    );
    assert_eq!(wants(reserved, Vec::new()), None);
    // No workflow read: a held task is never staffed by its status.
    let task = orchestrated(held("sim/a", "build", "available"));
    let board = Board {
        tasks: vec![task.clone()],
        sessions: Vec::new(),
        latest_message: 0,
        workflow_generation: None,
    };
    assert_eq!(eligible(&task, &board, &Config::default(), None), None);
}

#[test]
fn an_idle_fresh_reviewer_takes_a_held_tasks_review_edge_and_its_executor_is_passed_over() {
    use crate::workflow::{Host, agent_session};
    let f = reviewing();
    let host = Host::local();
    f.store
        .activate_workflow(&reviewed_workflow(), &host)
        .unwrap();
    f.store
        .create(&NewTask::new("sim/jump", "A review", FILER))
        .unwrap();
    f.store
        .take_role("sim/jump", ORCHESTRATES, "orchestrator")
        .unwrap();
    let executed = agent_session(EXECUTED);
    (f.store)
        .claim_by_workflow("sim/jump", &executed, &host, Role::Executor)
        .unwrap();
    let next = f
        .store
        .workflow_actions("sim/jump", &executed, &host)
        .unwrap();
    let request = symphony_api::workflow::OperationRequest {
        project: f.store.database().database().to_string(),
        task: "sim/jump".into(),
        edge: "submit".into(),
        input_revisions: Default::default(),
        idempotency_key: "submit".into(),
        inputs: Default::default(),
        expected: next.snapshot,
    };
    f.store.apply_operation(&executed, &host, &request).unwrap();
    let board = f.store.board().unwrap();
    let task = board.tasks.iter().find(|t| t.slug == "sim/jump").unwrap();
    let operating = f.store.operating_for(&board.tasks).unwrap();
    assert_eq!(
        eligible(task, &board, &Config::default(), operating.as_ref()),
        Some("reviewer")
    );
    // Its executor let the role go with its submit, yet executed it: a fresh one is picked.
    let held = f.store.ever_held("sim/jump").unwrap();
    let agents = [
        idle_reviewer(EXECUTED),
        idle_reviewer(FILER),
        idle_reviewer(FRESH),
    ];
    let picked = idle_for(
        &agents,
        &board,
        task,
        "reviewer",
        &HashSet::new(),
        &held,
        operating.as_ref(),
        &Defaults::default(),
    );
    assert_eq!(picked.map(|a| a.handle.as_str()), Some(FRESH));
    let hc = ReusedTerminal::answering("started");
    let line = dispatch(
        &f.store,
        &hc,
        &reusing(),
        &Defaults::default().resolve(None, None),
        task,
        ORCHESTRATES,
        Some(&jump_review()),
        picked,
    )
    .unwrap();
    assert_eq!(
        line,
        format!("pinged {FRESH} as reviewer, pane t1, prompt $review-task sim/jump {FRESH}")
    );
    let position = f.store.workflow_position("sim/jump").unwrap().unwrap();
    assert_eq!(
        (position.phase.as_str(), position.state.as_str()),
        ("verify", "reviewing")
    );
    assert_eq!(
        f.store.get("sim/jump").unwrap().holders("reviewer"),
        [FRESH]
    );
    // Taken: no claim leaves it, so the next pass starts nobody there.
    let board = f.store.board().unwrap();
    let task = board.tasks.iter().find(|t| t.slug == "sim/jump").unwrap();
    assert_eq!(
        eligible(task, &board, &Config::default(), operating.as_ref()),
        None
    );
}

/// Defaults whose launch default is claude and whose codex is turned off in the board's store.
fn codex_off() -> Defaults {
    let mut defaults = Defaults {
        agent: Some("claude"),
        ..Defaults::default()
    };
    defaults.on[0] = Some(crate::launch::defaults::Switch {
        on: false,
        at: crate::limits::Written {
            at: "the board's store [agents.launch.on] codex".into(),
            line: None,
        },
    });
    defaults
}

#[test]
fn an_idle_agent_turned_off_is_given_no_new_work() {
    let f = reviewing();
    in_review(&f, "sim/jump", false);
    let board = f.store.board().unwrap();
    let task = board.tasks.iter().find(|t| t.slug == "sim/jump").unwrap();
    let held = f.store.ever_held("sim/jump").unwrap();
    let mut claude = idle_reviewer(SECOND);
    claude.agent = Some("claude".into());
    let agents = [idle_reviewer(FRESH), claude];
    let pick = |defaults: &Defaults| {
        idle_for(
            &agents,
            &board,
            task,
            "reviewer",
            &HashSet::new(),
            &held,
            None,
            defaults,
        )
        .map(|a| a.handle.clone())
    };
    // Codex on: the first idle fresh one, a Codex agent.
    assert_eq!(pick(&Defaults::default()).as_deref(), Some(FRESH));
    // Codex off: passed over for the Claude one, and none when it is alone.
    assert_eq!(pick(&codex_off()).as_deref(), Some(SECOND));
    let alone = [idle_reviewer(FRESH)];
    let none = idle_for(
        &alone,
        &board,
        task,
        "reviewer",
        &HashSet::new(),
        &held,
        None,
        &codex_off(),
    );
    assert!(none.is_none());
}

#[test]
fn a_job_that_would_start_an_agent_turned_off_or_missing_is_refused_and_a_ping_never() {
    let path = programs("starting", &["claude", "codex"]).into_os_string();
    let launching = Defaults {
        agent: Some("claude"),
        ..Defaults::default()
    }
    .resolve(None, None);
    let off = codex_off();
    let resuming = ("fleet-vole".to_string(), "codex");
    let said = "codex is turned off (the board's store [agents.launch.on] codex): the board starts \
                none while it is, and turning it on again is that switch alone";
    // A resume of a Codex reviewer turned off.
    assert_eq!(
        starting_refused((Some(&resuming), None), &launching, &off, Some(&path)).as_deref(),
        Some(said)
    );
    // A launch of the default, claude, goes; a ping of an idle agent is never refused.
    assert_eq!(
        starting_refused((None, None), &launching, &off, Some(&path)),
        None
    );
    let idle_one = idle("fleet-vole", "worker", "/nowhere");
    assert_eq!(
        starting_refused(
            (Some(&resuming), Some(&idle_one)),
            &launching,
            &off,
            Some(&path)
        ),
        None
    );
    // The default's program missing: the launch is refused with that reason.
    let only_codex = programs("starting-codex", &["codex"]).into_os_string();
    assert_eq!(
        starting_refused(
            (None, None),
            &launching,
            &Defaults::default(),
            Some(&only_codex)
        )
        .as_deref(),
        Some("this machine has no claude program on its search path, so it cannot start here")
    );
    // Nothing turned off and every program here: nothing refused.
    assert_eq!(
        starting_refused(
            (Some(&resuming), None),
            &launching,
            &Defaults::default(),
            Some(&path)
        ),
        None
    );
}
