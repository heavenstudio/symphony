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
            pane: Some("t1".into()),
            callback: None,
            chat: false,
            state: "idle",
            detail: None,
            health: Health::Live,
            last_tool_call: None,
            last_message: None,
        };
        let hc = ReusedTerminal {
            outcome,
            sent: Cell::new(false),
            looks: Cell::new(0),
        };
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
