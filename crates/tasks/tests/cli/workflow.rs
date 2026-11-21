//! `tasks workflow`: the active workflow shown, a task's next actions read and an edge taken
//! through the board's one admission, and the board's own commands refusing a task it holds.

use super::Case;
use symphony_api::workflow::Source;
use symphony_board::workflow::{Custom, Host, written};
use symphony_board::{NewLaunch, NewSession, NewTask};

/// Work built (available, then worked by its executor), then accepted by the owner.
pub(crate) const FLOW: &str = r#"
[workflow]
format = 1
initial = { phase = "build", state = "available" }
task_kinds = ["task"]
phases = [
  { id = "build", label = "Build", order = 0, phone_order = 0, column = true },
  { id = "verify", label = "Verify", order = 1, phone_order = 1, column = true },
]
states = [
  { id = "available", label = "Available" }, { id = "working", label = "Working" },
  { id = "awaiting", label = "Awaiting" }, { id = "accepted", label = "Accepted" },
]
capabilities = ["owner"]
adapters = []
guards = []
positions = [
  { at = { phase = "build", state = "available" }, slot = "release", dependencies = "none", unsuccessful_dependency = "block", accepts_work = true, terminal = false, legacy = "ready", satisfies_dependencies = false, completion = "none" },
  { at = { phase = "build", state = "working" }, staffing = "executor", slot = "acquire", dependencies = "none", unsuccessful_dependency = "block", accepts_work = true, terminal = false, legacy = "in_progress", satisfies_dependencies = false, completion = "none", recovery = { release = { target = { phase = "build", state = "available" }, manual = false, slot = "release", release_role = true, release_reservation = false }, lost_holder = { target = { phase = "build", state = "available" }, manual = false, slot = "release", release_role = true, release_reservation = false }, attempt_failure = { manual = true, slot = "retain", release_role = false, release_reservation = false }, cancellation_acknowledged = { manual = true, slot = "retain", release_role = false, release_reservation = false } } },
  { at = { phase = "verify", state = "awaiting" }, slot = "release", dependencies = "none", unsuccessful_dependency = "block", accepts_work = false, terminal = false, legacy = "review", satisfies_dependencies = false, completion = "none" },
  { at = { phase = "verify", state = "accepted" }, slot = "release", dependencies = "none", unsuccessful_dependency = "block", accepts_work = false, terminal = true, legacy = "done", outcome = "success", satisfies_dependencies = true, completion = "covered_close" },
]
edges = [
  { id = "claim", label = "Claim", from = { phase = "build", state = "available" }, to = { phase = "build", state = "working" }, operation = "claim", authority = [{ kind = "task_role", role = "executor" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "acquire_role", role = "executor" }, { kind = "slot", policy = "acquire" }], automatic = false, human_wait = false },
  { id = "submit", label = "Submit", from = { phase = "build", state = "working" }, to = { phase = "verify", state = "awaiting" }, operation = "request_review", authority = [{ kind = "task_role", role = "executor" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "slot", policy = "release" }], automatic = false, human_wait = false },
  { id = "accept", label = "Accept", from = { phase = "verify", state = "awaiting" }, to = { phase = "verify", state = "accepted" }, operation = "close_covered", authority = [{ kind = "project_capability", capability = "owner" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "outcome", outcome = "success", satisfies_dependencies = true }, { kind = "slot", policy = "release" }], automatic = false, human_wait = true },
]
"#;

/// The reviewer's position FLOW lacks: `verify/reviewing`, staffed by the reviewer the `review`
/// edge gives from `verify/awaiting`.
const REVIEWING: &str = r#"{ at = { phase = "verify", state = "reviewing" }, staffing = "reviewer", slot = "retain", dependencies = "none", unsuccessful_dependency = "block", accepts_work = false, terminal = false, legacy = "review", satisfies_dependencies = false, completion = "none", recovery = { release = { target = { phase = "verify", state = "awaiting" }, manual = false, slot = "retain", release_role = true, release_reservation = false }, lost_holder = { manual = true, slot = "retain", release_role = false, release_reservation = true }, attempt_failure = { manual = true, slot = "retain", release_role = false, release_reservation = false }, cancellation_acknowledged = { manual = true, slot = "retain", release_role = false, release_reservation = false } } },
  "#;
const REVIEW: &str = r#"{ id = "review", label = "Review", from = { phase = "verify", state = "awaiting" }, to = { phase = "verify", state = "reviewing" }, operation = "claim", authority = [{ kind = "task_role", role = "reviewer" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "acquire_role", role = "reviewer" }, { kind = "slot", policy = "retain" }], automatic = false, human_wait = false },
  "#;

pub(crate) fn flow() -> Custom {
    custom(FLOW)
}

/// FLOW with a reviewer's position and the edge that staffs it.
fn reviewed() -> Custom {
    custom(
        &(FLOW.replace(
            r#"{ id = "accepted", label = "Accepted" },"#,
            r#"{ id = "accepted", label = "Accepted" }, { id = "reviewing", label = "Reviewing" },"#,
        ))
        .replace(
            r#"  { at = { phase = "verify", state = "accepted" }"#,
            &format!(r#"  {REVIEWING}{{ at = {{ phase = "verify", state = "accepted" }}"#),
        )
        .replace(
            r#"  { id = "accept","#,
            &format!(r#"  {REVIEW}{{ id = "accept","#),
        ),
    )
}

fn custom(text: &str) -> Custom {
    let definition = written(text).unwrap().0.unwrap();
    let revision = definition
        .revision(Source {
            commit: "1".repeat(40),
            project_blob: "2".repeat(40),
        })
        .unwrap();
    Custom {
        definition,
        revision,
        mappings: Vec::new(),
        authorities: Vec::new(),
        project: None,
    }
}

#[test]
fn a_session_reads_a_tasks_actions_and_takes_an_edge_from_the_revision_it_read() {
    let c = Case::new();
    let amber = c.register("worker");
    assert_eq!(
        c.ok(&amber, &["workflow", "show"]),
        "no custom workflow is active: the board's own writers keep its tasks\n"
    );
    let custom = flow();
    c.store()
        .activate_workflow(&custom, &Host::local())
        .unwrap();
    c.ok(&amber, &["create", "sim/jump", "Jump departure"]);
    let shown = c.ok(&amber, &["workflow", "show"]);
    assert!(
        shown.starts_with(&format!(
            "workflow {} (generation 1), committed at {}, starting at build/available\n",
            custom.revision.digest,
            "1".repeat(40)
        )),
        "{shown}"
    );
    assert!(shown.contains(
        "  accept: verify/awaiting → verify/accepted  close_covered, by owner, a person's\n"
    ));
    assert_eq!(
        c.ok(&amber, &["workflow", "actions", "sim/jump"]),
        "sim/jump at build/available, workflow revision 1 (generation 1)\n  claim → build/working  Claim, by its executor\n"
    );
    let stale = c.run_as(
        &amber,
        &["workflow", "apply", "sim/jump", "claim", "--revision", "0"],
        "",
    );
    assert_eq!(stale.code, 1);
    assert!(
        stale.err.starts_with("✖ stale_task: sim/jump is at build/available (workflow revision 1), not build/available (revision 0)"),
        "{}",
        stale.err
    );
    let took = c.ok(
        &amber,
        &["workflow", "apply", "sim/jump", "claim", "--revision", "1"],
    );
    assert_eq!(
        took,
        "took claim (sim/jump@2)\nsim/jump at build/working, workflow revision 2 (generation 1)\n  submit → verify/awaiting  Submit, by its executor\n"
    );
    // The same command again answers the first result: its key is the task, revision and edge.
    assert_eq!(
        c.ok(
            &amber,
            &["workflow", "apply", "sim/jump", "claim", "--revision", "1"]
        ),
        took
    );
    let moved = c.run_as(&amber, &["move", "sim/jump", "review"], "");
    assert_eq!(moved.code, 1);
    assert!(
        moved
            .err
            .starts_with("✖ explicit_action_required: a custom workflow holds sim/jump"),
        "{}",
        moved.err
    );
    let json = c.ok(&amber, &["workflow", "actions", "sim/jump", "--json"]);
    let next: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(next["snapshot"]["task_revision"], 2);
    assert_eq!(next["actions"][0]["edge"], "submit");
    let usage = c.run_as(&amber, &["workflow", "apply", "sim/jump", "submit"], "");
    assert_eq!(usage.code, 1);
    assert!(
        usage
            .err
            .contains("--revision is the task's workflow revision you read"),
        "{}",
        usage.err
    );
}

#[test]
fn an_agent_reads_and_takes_an_edge_through_the_api_as_the_session_it_names() {
    let c = Case::new();
    let amber = c.register("worker");
    let call = |call: serde_json::Value| {
        let ran = super::run(&c.env, &["api"], &call.to_string());
        assert_eq!(ran.code, 0, "{}", ran.err);
        serde_json::from_str::<serde_json::Value>(&ran.out).unwrap()
    };
    assert_eq!(
        call(serde_json::json!({"method": "workflow_read"})),
        serde_json::json!({"ok": null})
    );
    let custom = flow();
    c.store()
        .activate_workflow(&custom, &Host::local())
        .unwrap();
    c.ok(&amber, &["create", "sim/jump", "Jump departure"]);
    let read = call(serde_json::json!({"method": "workflow_read"}));
    assert_eq!(read["ok"]["revision"]["digest"], custom.revision.digest);
    assert_eq!(read["ok"]["activation_generation"], 1);
    let next = call(serde_json::json!({"method": "workflow_actions",
        "args": {"slug": "sim/jump", "handle": amber}}));
    let snapshot = next["ok"]["snapshot"].clone();
    assert_eq!(snapshot["task_revision"], 1);
    let project = c.store().database().database().to_string();
    let request = serde_json::json!({
        "project": project, "task": "sim/jump", "edge": "claim", "expected": snapshot,
        "input_revisions": {}, "idempotency_key": "api-1", "inputs": {},
    });
    let applied = call(serde_json::json!({"method": "workflow_apply",
        "args": {"handle": amber, "request": request}}));
    assert_eq!(applied["ok"]["transition"], "sim/jump@2");
    assert_eq!(applied["ok"]["next"]["actions"][0]["edge"], "submit");
    let owner = call(serde_json::json!({"method": "workflow_apply",
        "args": {"handle": "owner", "request": request}}));
    assert!(
        owner["refused"]
            .as_str()
            .unwrap()
            .contains("the owner acts from the Symphony")
    );
    let mut again = request.clone();
    again["idempotency_key"] = "api-2".into();
    let stale = call(serde_json::json!({"method": "workflow_apply",
        "args": {"handle": amber, "request": again}}));
    assert!(
        stale["refused"]
            .as_str()
            .unwrap()
            .starts_with("stale_task: sim/jump is at build/working"),
        "{stale}"
    );
}

#[test]
fn a_session_recovers_a_lost_holders_task_which_pull_and_next_then_offer_to_its_claim() {
    let mut c = Case::new();
    let amber = c.register("worker");
    let birch = c.register("worker");
    c.store()
        .activate_workflow(&flow(), &Host::local())
        .unwrap();
    c.ok(&amber, &["create", "sim/jump", "Jump departure"]);
    c.ok(
        &amber,
        &["workflow", "apply", "sim/jump", "claim", "--revision", "1"],
    );
    let live = c.run_as(&birch, &["workflow", "recover", "sim/jump", &amber], "");
    assert_eq!(live.code, 1, "{}", live.out);
    assert!(
        live.err.starts_with(&format!(
            "✖ explicit_action_required: {amber} is live on the board: only a lost holder's \
             work is recovered"
        )),
        "{}",
        live.err
    );
    let now: f64 = c.env.vars["TASKS_NOW"].parse().unwrap();
    let lost = now + symphony_board::health::LIVE_SECONDS + symphony_board::health::STALE_SECONDS;
    c.env
        .vars
        .insert("TASKS_NOW".into(), (lost + 1.0).to_string());
    c.store().touch(&birch).unwrap();
    assert_eq!(
        c.ok(&birch, &["workflow", "recover", "sim/jump", &amber]),
        format!(
            "recovered {amber} on sim/jump (sim/jump@3): build/working → build/available\n\
             sim/jump at build/available, workflow revision 3 (generation 1)\n  \
             claim → build/working  Claim, by its executor\n"
        )
    );
    // Ready and unstaffed: PULL lists it, `next` offers it, and the board's claim takes its edge.
    let pulled = c.ok(&birch, &["pull"]);
    assert!(pulled.contains("ready_unstaffed=1"), "{pulled}");
    assert!(pulled.contains("sim/jump"), "{pulled}");
    assert!(c.ok(&birch, &["next"]).starts_with("sim/jump  p"));
    assert_eq!(
        c.ok(&birch, &["claim", "sim/jump"]),
        "claimed sim/jump (sim/jump@4): it is at build/working, workflow revision 4; its edges: \
         `just tasks workflow actions sim/jump`\n"
    );
    assert!(c.ok(&amber, &["next"]).starts_with("nothing to take"));
    let again = c.run_as(&amber, &["claim", "sim/jump"], "");
    assert_eq!(again.code, 1);
    assert!(
        again.err.starts_with(
            "✖ explicit_action_required: no edge leaving sim/jump's build/working gives the \
             executor role"
        ),
        "{}",
        again.err
    );
}

#[test]
fn a_pull_recovers_a_lost_holder_by_its_positions_action_and_lists_the_work_again() {
    let mut c = Case::new();
    let amber = c.register("worker");
    let orch = c.register("orchestrator");
    c.store()
        .activate_workflow(&flow(), &Host::local())
        .unwrap();
    c.ok(&amber, &["create", "sim/jump", "Jump departure"]);
    c.ok(&amber, &["claim", "sim/jump"]);
    let pulled = c.ok(&orch, &["pull"]);
    assert!(pulled.contains("ready_unstaffed=0"), "{pulled}");
    let now: f64 = c.env.vars["TASKS_NOW"].parse().unwrap();
    let lost = now + symphony_board::health::LIVE_SECONDS + symphony_board::health::STALE_SECONDS;
    c.env
        .vars
        .insert("TASKS_NOW".into(), (lost + 1.0).to_string());
    c.store().touch(&orch).unwrap();
    let pulled = c.ok(&orch, &["pull"]);
    assert!(pulled.contains("ready_unstaffed=1"), "{pulled}");
    assert!(pulled.contains("sim/jump"), "{pulled}");
    let shown = c.ok(&orch, &["show", "sim/jump"]);
    assert!(
        shown.contains(&format!(
            "board (system): the board's recovery (sim/jump@3): {amber}'s executor lost; \
             build/working → build/available, workflow revision 3"
        )),
        "{shown}"
    );
}

#[test]
fn a_launch_starts_the_role_a_held_tasks_edges_give_its_executor_reserving_and_its_reviewer_claiming()
 {
    let p = super::launch::Terminals::new("held");
    let board = p.board();
    let orch = (board.register(&NewSession::new("orchestrator")).unwrap())
        .row
        .handle;
    board
        .activate_workflow(&reviewed(), &Host::local())
        .unwrap();
    board
        .create(&NewTask::new("sim/jump", "Jump", &orch))
        .unwrap();
    let folder = |name: &str| {
        let path = p.dir.join(name);
        std::fs::create_dir_all(&path).unwrap();
        path.display().to_string()
    };
    let (built, looked) = (folder("built"), folder("looked"));
    let launch = |role: &str, worktree: &str, more: &[&str]| {
        let args = [
            "launch",
            "sim/jump",
            "--kind",
            "claude",
            "--role",
            role,
            "--worktree",
            worktree,
        ];
        p.as_(&orch, &[&args[..], more].concat())
    };
    // No edge leaving build/available gives the reviewer's role: nothing starts.
    let early = launch("reviewer", &looked, &[]);
    assert_eq!(early.code, 1, "{}", early.out);
    assert!(
        early.err.starts_with(
            "✖ explicit_action_required: a custom workflow holds sim/jump, at build/available: no \
             edge leaving it gives the reviewer role"
        ),
        "{}",
        early.err
    );
    assert!(board.task_sessions("sim/jump").unwrap().is_empty());
    // The executor's launch reserves it; its claim takes the edge and the reservation with it.
    let ran = launch("executor", &built, &[]);
    assert_eq!((ran.code, ran.err.as_str()), (0, ""));
    let [executor] = <[_; 1]>::try_from(board.task_sessions("sim/jump").unwrap()).unwrap();
    let task = board.get("sim/jump").unwrap();
    assert_eq!(task.status, "ready");
    assert!(
        (task.roles.iter()).any(|r| r.handle == executor.handle && r.role == "reserved"),
        "{:?}",
        task.roles
    );
    let claimed = p.as_(&executor.handle, &["claim", "sim/jump"]);
    assert!(
        claimed.out.starts_with("claimed sim/jump (sim/jump@2)"),
        "{}{}",
        claimed.out,
        claimed.err
    );
    let submitted = p.as_(
        &executor.handle,
        &["workflow", "apply", "sim/jump", "submit", "--revision", "2"],
    );
    assert_eq!(submitted.code, 0, "{}", submitted.err);
    // A second reviewer beside another is no edge's: refused before anything starts.
    let beside = launch("reviewer", &looked, &["--beside", "a second look"]);
    assert_eq!(beside.code, 1);
    assert!(
        beside
            .err
            .contains("its edges give the reviewer's role to one session"),
        "{}",
        beside.err
    );
    // The reviewer's launch takes the review edge as the new session.
    let reviewed = launch("reviewer", &looked, &[]);
    assert_eq!((reviewed.code, reviewed.err.as_str()), (0, ""));
    let sessions = board.task_sessions("sim/jump").unwrap();
    let reviewer = (sessions.iter())
        .find(|s| s.handle != executor.handle)
        .unwrap();
    let position = board.workflow_position("sim/jump").unwrap().unwrap();
    assert_eq!(
        (
            position.phase.as_str(),
            position.state.as_str(),
            position.revision
        ),
        ("verify", "reviewing", 4)
    );
    assert!(
        (board.get("sim/jump").unwrap().roles.iter())
            .any(|r| r.handle == reviewer.handle && r.role == "reviewer")
    );
}

#[test]
fn an_edge_to_a_terminal_position_finishes_the_task_closing_what_was_launched_on_it() {
    const SHIP: &str = r#"{ id = "ship", label = "Ship", from = { phase = "build", state = "working" }, to = { phase = "verify", state = "accepted" }, operation = "close_covered", authority = [{ kind = "task_role", role = "executor" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "outcome", outcome = "success", satisfies_dependencies = true }, { kind = "slot", policy = "release" }], automatic = false, human_wait = false },
  "#;
    let p = super::launch::Terminals::new("held-finish");
    let board = p.board();
    let orch = (board.register(&NewSession::new("orchestrator")).unwrap())
        .row
        .handle;
    let shipping = FLOW.replace(
        r#"  { id = "accept","#,
        &format!(r#"  {SHIP}{{ id = "accept","#),
    );
    board
        .activate_workflow(&custom(&shipping), &Host::local())
        .unwrap();
    board
        .create(&NewTask::new("sim/jump", "Jump", &orch))
        .unwrap();
    let worker = (board.register(&NewSession::new("worker")).unwrap())
        .row
        .handle;
    board
        .record_launch(&NewLaunch {
            slug: "sim/jump".into(),
            handle: worker.clone(),
            role: "executor".into(),
            claude_session: "c".into(),
            pane: "t1".into(),
            ..NewLaunch::default()
        })
        .unwrap();
    p.idle_terminal(&worker);
    // An edge short of a terminal position closes nothing launched on it.
    let claimed = p.as_(
        &worker,
        &["workflow", "apply", "sim/jump", "claim", "--revision", "1"],
    );
    assert!(
        claimed.out.starts_with("took claim (sim/jump@2)\n")
            && !claimed.out.contains("closed pane"),
        "{}{}",
        claimed.out,
        claimed.err
    );
    assert_eq!(p.addresses(), ["t1"]);
    let shipped = p.as_(
        &worker,
        &["workflow", "apply", "sim/jump", "ship", "--revision", "2"],
    );
    assert_eq!(
        (shipped.code, shipped.out.as_str(), shipped.err.as_str()),
        (
            0,
            format!(
                "took ship (sim/jump@3)\nsim/jump at verify/accepted, workflow revision 3 \
                 (generation 1)\n  no edge leaves it\n→ closed pane t1 of {worker} (its tasks are \
                 finished); restore: just tasks restore sim/jump {worker}\n"
            )
            .as_str(),
            ""
        )
    );
    assert_eq!(p.addresses(), Vec::<String>::new());
    assert_eq!(board.get("sim/jump").unwrap().status, "done");
}

#[test]
fn a_staffing_pass_starts_the_role_a_held_tasks_open_claim_edge_gives_and_none_while_it_is_held() {
    let p = super::launch::Terminals::new("held-staff");
    p.manifest("[agents]\nlaunch = { default = \"claude\", model = \"opus\" }\n");
    let board = p.board();
    board
        .activate_workflow(&reviewed(), &Host::local())
        .unwrap();
    p.staffed();
    let staff = || {
        let ran = p.run(&["staff", "--dry-run"]);
        assert_eq!((ran.code, ran.err.as_str()), (0, ""), "{}", ran.out);
        ran.out
    };
    // At build/available its claim gives the executor's role.
    assert_eq!(
        staff(),
        "staff sim/jump: would launch claude on opus as executor\n"
    );
    // Claimed, the role is held: no edge leaving build/working gives one.
    let amber = (board.register(&NewSession::new("worker")).unwrap())
        .row
        .handle;
    let claimed = p.as_(&amber, &["claim", "sim/jump"]);
    assert_eq!(claimed.code, 0, "{}", claimed.err);
    assert_eq!(staff(), "staff: no eligible work\n");
    // Submitted, its review edge gives the reviewer's role, on the branch its slug names.
    let submitted = p.as_(
        &amber,
        &["workflow", "apply", "sim/jump", "submit", "--revision", "2"],
    );
    assert_eq!(submitted.code, 0, "{}", submitted.err);
    super::launch::git(&p.env.cwd, &["branch", "sim/jump"]);
    // The tip the branch was made at, as `git branch` writes it.
    let tip = std::fs::read_to_string(p.env.cwd.join(".git/refs/heads/sim/jump")).unwrap();
    assert_eq!(
        staff(),
        format!(
            "staff sim/jump: would launch claude on opus as reviewer (review sim/jump at {})\n",
            tip.trim()
        )
    );
}

#[test]
fn a_wait_recovers_a_lost_holder_as_a_pull_does() {
    let mut c = Case::new();
    let amber = c.register("worker");
    let orch = c.register("orchestrator");
    c.store()
        .activate_workflow(&flow(), &Host::local())
        .unwrap();
    c.ok(&amber, &["create", "sim/jump", "Jump departure"]);
    c.ok(&amber, &["claim", "sim/jump"]);
    let now: f64 = c.env.vars["TASKS_NOW"].parse().unwrap();
    let lost = now + symphony_board::health::LIVE_SECONDS + symphony_board::health::STALE_SECONDS;
    c.env
        .vars
        .insert("TASKS_NOW".into(), (lost + 1.0).to_string());
    c.store().touch(&orch).unwrap();
    c.ok(&orch, &["wait", "--timeout", "0"]);
    let position = c.store().workflow_position("sim/jump").unwrap().unwrap();
    assert_eq!(
        (
            position.phase.as_str(),
            position.state.as_str(),
            position.revision
        ),
        ("build", "available", 3)
    );
}

#[test]
fn a_session_reads_the_accepted_output_a_claim_takes_and_names_it_to_take_the_edge() {
    let c = Case::new();
    let amber = c.register("worker");
    let text = FLOW.replacen(
        "inputs = [], dependencies = \"none\"",
        "inputs = [], dependencies = \"accepted_output\"",
        1,
    );
    let definition = written(&text).unwrap().0.unwrap();
    let revision = (definition.revision(Source {
        commit: "1".repeat(40),
        project_blob: "2".repeat(40),
    }))
    .unwrap();
    let custom = Custom {
        definition,
        revision,
        mappings: Vec::new(),
        authorities: Vec::new(),
        project: None,
    };
    c.store()
        .activate_workflow(&custom, &Host::local())
        .unwrap();
    c.ok(&amber, &["create", "sim/up", "Upstream"]);
    c.ok(
        &amber,
        &["create", "sim/down", "Downstream", "--depends", "sim/up"],
    );
    assert_eq!(
        c.ok(&amber, &["workflow", "actions", "sim/down"]),
        "sim/down at build/available, workflow revision 1 (generation 1)\n  claim → build/working  \
         Claim, by its executor  (not now: dependency)\n  takes sim/up's accepted output: none \
         stands\n"
    );
    c.ok(
        &amber,
        &["workflow", "apply", "sim/up", "claim", "--revision", "1"],
    );
    c.ok(
        &amber,
        &["workflow", "apply", "sim/up", "submit", "--revision", "2"],
    );
    // The owner accepts it from the Symphony, as the local owner.
    let owner = symphony_board::workflow::local_owner();
    let snapshot = (c.store().workflow_actions("sim/up", &owner, &Host::local()))
        .unwrap()
        .snapshot;
    let accept = symphony_api::workflow::OperationRequest {
        project: c.store().database().database().to_string(),
        task: "sim/up".into(),
        edge: "accept".into(),
        expected: snapshot,
        input_revisions: Default::default(),
        idempotency_key: "owner-accept".into(),
        inputs: Default::default(),
    };
    let accepted = c.store().apply_operation(&owner, &Host::local(), &accept);
    assert_eq!(accepted.unwrap().transition, "sim/up@4");
    assert_eq!(
        c.ok(&amber, &["workflow", "actions", "sim/up"]),
        "sim/up at verify/accepted, workflow revision 4 (generation 1)\n  no edge leaves it\n  its \
         accepted output: sim/up@4\n"
    );
    assert_eq!(
        c.ok(&amber, &["workflow", "actions", "sim/down"]),
        "sim/down at build/available, workflow revision 1 (generation 1)\n  claim → build/working  \
         Claim, by its executor\n  takes sim/up's accepted output sim/up@4 (--accept \
         sim/up=sim/up@4)\n"
    );
    let apply = |extra: &[&str]| {
        let mut args = vec!["workflow", "apply", "sim/down", "claim", "--revision", "1"];
        args.extend_from_slice(extra);
        c.run_as(&amber, &args, "")
    };
    for (extra, refused) in [
        (
            &[][..],
            "✖ explicit_action_required: edge claim takes the accepted output of sim/up: name the \
             one it takes, sim/up = sim/up@4",
        ),
        (
            &["--accept", "sim/up=sim/up@3"][..],
            "✖ stale_input: sim/down takes sim/up@3 of sim/up, whose accepted output standing is \
             sim/up@4",
        ),
        (
            &["--accept", "sim/up"][..],
            "✖ --accept is SLUG=OUTPUT, not \"sim/up\"",
        ),
    ] {
        let ran = apply(extra);
        assert_eq!(ran.code, 1, "{}", ran.out);
        assert!(ran.err.starts_with(refused), "{}", ran.err);
    }
    let took = apply(&["--accept", "sim/up=sim/up@4"]);
    assert_eq!(took.code, 0, "{}", took.err);
    assert!(
        took.out.starts_with("took claim (sim/down@2)\n"),
        "{}",
        took.out
    );
    // The same command again answers the first result, the output it named included; another
    // output under its key is another request, and refused.
    assert_eq!(apply(&["--accept", "sim/up=sim/up@4"]).out, took.out);
    let other = apply(&["--accept", "sim/up=sim/up@9"]);
    assert_eq!(other.code, 1, "{}", other.out);
    assert!(
        other.err.starts_with(
            "✖ explicit_action_required: the key sim/down@1:claim was used for another request"
        ),
        "{}",
        other.err
    );
}

/// FLOW with its submit edge guarded by `approved`, which the `approvals` authority attests
/// with the key of seed 7, in the project `sim`.
mod guarded {
    use super::super::{Case, scratch};
    use super::{FLOW, flow};
    use ed25519_dalek::{Signer, SigningKey};
    use std::collections::BTreeMap;
    use symphony_api::workflow::{
        ATTESTATION_CONTRACT, Attestation, Attested, Observation, Pair, Signed,
    };
    use symphony_board::workflow::guard::{MAX_VALIDITY, attestee, signed_message};
    use symphony_board::workflow::{Attester, Custom, Host, agent_session, written};

    const NOW: f64 = 1_800_000_000.0;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn signing(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn signed(seed: u8, observation: bool, payload: String) -> Signed {
        let signature = signing(seed).sign(&signed_message(observation, &payload));
        Signed {
            payload,
            signature: hex(&signature.to_bytes()),
        }
    }

    fn guarded_flow() -> Custom {
        let text = FLOW
            .replacen("adapters = []", "adapters = [\"approvals\"]", 1)
            .replacen(
                "guards = []",
                "guards = [{ id = \"approved\", adapter = \"approvals\" }]",
                1,
            )
            .replacen(
                "operation = \"request_review\", authority = [{ kind = \"task_role\", role = \"executor\" }], inputs = [], dependencies = \"none\", guards = []",
                "operation = \"request_review\", authority = [{ kind = \"task_role\", role = \"executor\" }], inputs = [], dependencies = \"none\", guards = [\"approved\"]",
                1,
            );
        let definition = written(&text).unwrap().0.unwrap();
        Custom {
            definition,
            authorities: vec![Attester {
                adapter: "approvals".into(),
                issuer: "ledger".into(),
                contract: ATTESTATION_CONTRACT,
                key: format!("ed25519:{}", hex(signing(7).verifying_key().as_bytes())),
            }],
            project: Some("sim".into()),
            ..flow()
        }
    }

    fn observation(seed: u8, generation: u64) -> Signed {
        observation_of(seed, generation, &format!("s{generation}"))
    }

    /// The first observation of `approvals` at `generation` in epoch e1, in `state`.
    fn observation_of(seed: u8, generation: u64, state: &str) -> Signed {
        let observed = Observation {
            contract: ATTESTATION_CONTRACT,
            adapter: "approvals".into(),
            issuer: "ledger".into(),
            epoch: "e1".into(),
            generation,
            state: state.into(),
            previous: None,
            at: NOW,
        };
        signed(seed, true, serde_json::to_string(&observed).unwrap())
    }

    /// This machine's time, which the board holds an attestation to whatever `TASKS_NOW` pins.
    fn machine_now() -> f64 {
        (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH))
            .unwrap()
            .as_secs_f64()
    }

    /// amber's submit of sim/jump from its workflow revision 2, passed at generation 3, on the
    /// input revision design=r7, issued a second ago by this machine's clock for as long as the
    /// board takes one ([`MAX_VALIDITY`]).
    fn submit_attestation(custom: &Custom, amber: &str) -> Signed {
        submit_attested_at(custom, amber, machine_now() - 1.0)
    }

    /// [`submit_attestation`], issued at `issued`.
    fn submit_attested_at(custom: &Custom, amber: &str, issued: f64) -> Signed {
        let attestation = Attestation {
            contract: ATTESTATION_CONTRACT,
            adapter: "approvals".into(),
            issuer: "ledger".into(),
            epoch: "e1".into(),
            generation: 3,
            project: "sim".into(),
            task: "sim/jump".into(),
            guard: "approved".into(),
            edge: "submit".into(),
            workflow: custom.revision.digest.clone(),
            activation_generation: 1,
            task_revision: 2,
            at: Pair {
                phase: "build".into(),
                state: "working".into(),
            },
            input_revisions: BTreeMap::from([("design".to_string(), "r7".to_string())]),
            principal: attestee(&agent_session(amber)),
            result: Attested::Pass,
            facts: vec!["ledger:decision/41".into()],
            reasons: Vec::new(),
            issued_at: issued,
            expires_at: issued + MAX_VALIDITY,
        };
        signed(7, false, serde_json::to_string(&attestation).unwrap())
    }

    /// A board with `guarded_flow` active and sim/jump claimed by a new worker session.
    fn claimed() -> (Case, String, Custom) {
        let c = Case::new();
        let amber = c.register("worker");
        let custom = guarded_flow();
        c.store()
            .activate_workflow(&custom, &Host::local())
            .unwrap();
        c.ok(&amber, &["create", "sim/jump", "Jump departure"]);
        c.ok(
            &amber,
            &["workflow", "apply", "sim/jump", "claim", "--revision", "1"],
        );
        (c, amber, custom)
    }

    #[test]
    fn a_session_takes_a_guarded_edge_with_the_authoritys_signed_word_and_no_other() {
        let (mut c, amber, custom) = claimed();
        let dir = scratch("workflow-guarded-cli");
        let file = |name: &str, signed: &Signed| {
            let path = dir.join(name);
            std::fs::write(&path, serde_json::to_string(signed).unwrap()).unwrap();
            path.to_string_lossy().into_owned()
        };
        assert!(c.ok(&amber, &["workflow", "actions", "sim/jump"]).contains(
            "submit → verify/awaiting  Submit, by its executor  (not now: guard_unavailable)"
        ));
        let submit = ["workflow", "apply", "sim/jump", "submit", "--revision", "2"];
        let unobserved = c.run_as(&amber, &submit, "");
        assert!(
            unobserved.err.starts_with(
                "✖ guard_unavailable: edge submit is guarded by approved, and the board has \
                 accepted no observation from approvals"
            ),
            "{}",
            unobserved.err
        );
        let observed = file("observed.json", &observation(7, 3));
        assert_eq!(
            c.ok(&amber, &["workflow", "observe", &observed]),
            "approvals: accepted generation 3 of e1 (s3)\n"
        );
        assert_eq!(
            c.ok(&amber, &["workflow", "observe", &observed]),
            "approvals: already at generation 3 of e1 (s3)\n"
        );
        let json: serde_json::Value =
            serde_json::from_str(&c.ok(&amber, &["workflow", "observe", &observed, "--json"]))
                .unwrap();
        assert_eq!(json["observed"], "unchanged");
        assert_eq!(json["authority"]["generation"], 3);
        assert_eq!(json["authority"]["state"], "s3");
        let forged = file("forged.json", &observation(8, 4));
        let refused = c.run_as(&amber, &["workflow", "observe", &forged], "");
        assert!(refused.err.contains("committed key"), "{}", refused.err);

        let attested = file("attested.json", &submit_attestation(&custom, &amber));
        let mut with = submit.to_vec();
        with.extend(["--attestation", &attested]);
        let drifted = c.run_as(&amber, &with, "");
        assert!(drifted.err.starts_with("✖ stale_input:"), "{}", drifted.err);
        let missing = c.run_as(
            &amber,
            &[
                &submit[..],
                &["--attestation", "/nonexistent/attested.json"],
            ]
            .concat(),
            "",
        );
        assert!(
            missing
                .err
                .contains("--attestation /nonexistent/attested.json"),
            "{}",
            missing.err
        );
        std::fs::write(dir.join("garbage.json"), "{}").unwrap();
        let garbage = dir.join("garbage.json").to_string_lossy().into_owned();
        let unread = c.run_as(
            &amber,
            &[&submit[..], &["--attestation", &garbage]].concat(),
            "",
        );
        assert!(
            unread.err.contains("not a signed statement"),
            "{}",
            unread.err
        );
        let bad_revision = c.run_as(
            &amber,
            &[&with[..], &["--input-revision", "design"]].concat(),
            "",
        );
        assert!(
            bad_revision
                .err
                .contains("--input-revision is NAME=REVISION"),
            "{}",
            bad_revision.err
        );
        let twice = c.run_as(
            &amber,
            &[
                &with[..],
                &["--accept", "design=r1", "--input-revision", "design=r7"],
            ]
            .concat(),
            "",
        );
        assert!(twice.err.contains("design is given twice"), "{}", twice.err);
        with.extend(["--input-revision", "design=r7"]);
        // An attestation expired by this machine's clock is refused, though TASKS_NOW pins the
        // command's clock inside its time: the caller sets that clock, never the guard's.
        let issued = machine_now() - MAX_VALIDITY - 100.0;
        let expired = file("expired.json", &submit_attested_at(&custom, &amber, issued));
        let pinned = c.env.vars["TASKS_NOW"].clone();
        c.env
            .vars
            .insert("TASKS_NOW".into(), (issued + 1.0).to_string());
        let late = c.run_as(
            &amber,
            &[
                &submit[..],
                &["--attestation", &expired, "--input-revision", "design=r7"],
            ]
            .concat(),
            "",
        );
        c.env.vars.insert("TASKS_NOW".into(), pinned);
        assert!(
            late.err.starts_with("✖ guard_unavailable:") && late.err.contains("expired"),
            "{}",
            late.err
        );
        assert_eq!(
            c.ok(&amber, &with),
            "took submit (sim/jump@3)\nsim/jump at verify/awaiting, workflow revision 3 (generation 1)\n  accept → verify/accepted  Accept, by owner  (not now: human_wait, unauthorized)\n"
        );
        // The same command again answers the first result.
        assert!(c.ok(&amber, &with).starts_with("took submit (sim/jump@3)"));
        let admission = c
            .store()
            .workflow_admission("sim/jump", 3)
            .unwrap()
            .unwrap();
        assert_eq!(admission.guards[0].guard, "approved");
        // Another decision at the generation accepted leaves the authority unknown: the state
        // accepted stays, marked in conflict.
        let other = file("other.json", &observation_of(7, 3, "x3"));
        assert_eq!(
            c.ok(&amber, &["workflow", "observe", &other]),
            "approvals: in conflict at generation 3 of e1 (s3); nothing it attests is admitted \
             until it reports a state following this one\n"
        );
        let third = file("third.json", &observation_of(7, 3, "y3"));
        let json: serde_json::Value =
            serde_json::from_str(&c.ok(&amber, &["workflow", "observe", &third, "--json"]))
                .unwrap();
        assert_eq!(json["observed"], "conflicted");
        assert_eq!(json["authority"]["conflicted"], true);
        assert_eq!(json["authority"]["state"], "s3");
    }

    #[test]
    fn an_agent_takes_a_guarded_edge_through_the_api_with_the_attestation_it_carries() {
        let (c, amber, custom) = claimed();
        let call = |call: serde_json::Value| {
            let ran = super::super::run(&c.env, &["api"], &call.to_string());
            serde_json::from_str::<serde_json::Value>(&ran.out).unwrap()
        };
        let observed = call(serde_json::json!({"method": "workflow_observe",
            "args": {"signed": observation(7, 3)}}));
        assert_eq!(observed["ok"]["observed"], "accepted", "{observed}");
        assert_eq!(observed["ok"]["authority"]["generation"], 3);
        let next = call(serde_json::json!({"method": "workflow_actions",
            "args": {"slug": "sim/jump", "handle": amber}}));
        let request = serde_json::json!({
            "project": c.store().database().database(), "task": "sim/jump", "edge": "submit",
            "expected": next["ok"]["snapshot"], "input_revisions": {"design": "r7"},
            "idempotency_key": "api-submit", "inputs": {},
        });
        let bare = call(serde_json::json!({"method": "workflow_apply",
            "args": {"handle": amber, "request": request}}));
        assert!(
            bare["refused"]
                .as_str()
                .unwrap()
                .starts_with("guard_unavailable:"),
            "{bare}"
        );
        let applied = call(serde_json::json!({"method": "workflow_apply",
            "args": {"handle": amber, "request": request,
                "attestations": [submit_attestation(&custom, &amber)]}}));
        assert_eq!(applied["ok"]["transition"], "sim/jump@3", "{applied}");
        let forged = call(serde_json::json!({"method": "workflow_observe",
            "args": {"signed": observation(8, 4)}}));
        assert!(
            forged["refused"]
                .as_str()
                .unwrap()
                .contains("committed key"),
            "{forged}"
        );
    }
}

#[test]
fn after_a_return_the_boards_own_commands_move_the_task_again() {
    let c = Case::new();
    let amber = c.register("worker");
    c.store()
        .activate_workflow(&flow(), &Host::local())
        .unwrap();
    c.ok(&amber, &["create", "sim/jump", "Jump departure"]);
    c.ok(
        &amber,
        &["workflow", "apply", "sim/jump", "claim", "--revision", "1"],
    );
    // The default branch commits a file with no [workflow], its mappings onto the board's pairs.
    let back = "[[workflow_mappings]]\nfrom = { phase = \"build\", state = \"working\" }\nto = { phase = \"in_progress\", state = \"working\" }\n";
    let committed = symphony_board::workflow::Committed {
        commit: "3".repeat(40),
        custom: None,
        returning: written(back).unwrap().1,
    };
    assert_eq!(
        c.store().follow_committed(&committed, &Host::local()),
        Ok(symphony_board::workflow::Followed::Returned { generation: 2 })
    );
    assert_eq!(
        c.ok(&amber, &["workflow", "show"]),
        "no custom workflow is active: the board's own writers keep its tasks\n"
    );
    let shown = c.ok(&amber, &["show", "sim/jump"]);
    assert!(shown.contains("[in_progress"), "{shown}");
    // Its executor came back with it, and sends it to review by the board's own move.
    c.ok(&amber, &["move", "sim/jump", "review"]);
    let edge = c.run_as(
        &amber,
        &["workflow", "apply", "sim/jump", "submit", "--revision", "3"],
        "",
    );
    assert_eq!(edge.code, 1, "{}", edge.out);
    assert!(!edge.err.is_empty());
}
