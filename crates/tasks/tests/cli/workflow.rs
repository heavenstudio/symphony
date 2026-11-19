//! `tasks workflow`: the active workflow shown, a task's next actions read and an edge taken
//! through the board's one admission, and the board's own commands refusing a task it holds.

use super::Case;
use symphony_api::workflow::Source;
use symphony_board::workflow::{Custom, Host, written};

/// Work built (available, then worked by its executor), then accepted by the owner.
const FLOW: &str = r#"
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

pub(crate) fn flow() -> Custom {
    let definition = written(FLOW).unwrap().0.unwrap();
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
fn a_session_recovers_a_lost_holders_task_which_pull_and_next_offer_to_no_claim() {
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
    // Ready and unstaffed, yet no claim but its edge takes it.
    let pulled = c.ok(&birch, &["pull"]);
    assert!(pulled.contains("ready_unstaffed=0"), "{pulled}");
    assert!(!pulled.contains("sim/jump"), "{pulled}");
    assert!(c.ok(&birch, &["next"]).starts_with("nothing to take"));
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
