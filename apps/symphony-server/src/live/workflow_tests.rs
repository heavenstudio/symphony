//! A custom workflow through the Symphony's routes, on a fixture project's own board: the active
//! definition and a task's place read, the local owner's next actions previewed, an edge taken,
//! repeated under its key and refused as stale from the revision it left, and the feed saying the
//! board activated a workflow so a page reads the definition again (docs/WORKFLOW.md, "The app").

use super::boards_tests::{Project, answer, asked, env, writer};
use crate::front::Shared;
use crate::testing::Tree;
use serde_json::{Value, json};
use std::time::Duration;
use symphony_api::workflow::Source;
use symphony_board::workflow::{Custom, Host, written};

/// The wall-clock backstop of a wait for the hub's read: only a stall reaches it.
const BACKSTOP: Duration = test_time::backstop(Duration::from_secs(30));

/// Work built (available, worked by a held executor, or blocked by the owner), then verified
/// (awaiting, accepted by a person); a ready task the board's writers keep starts available.
const TWO: &str = r#"
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
  { id = "blocked", label = "Blocked" }, { id = "awaiting", label = "Awaiting" },
  { id = "accepted", label = "Accepted" },
]
capabilities = ["owner"]
adapters = []
guards = []
positions = [
  { at = { phase = "build", state = "available" }, slot = "release", dependencies = "none", unsuccessful_dependency = "block", accepts_work = true, terminal = false, legacy = "ready", satisfies_dependencies = false, completion = "none" },
  { at = { phase = "build", state = "working" }, staffing = "executor", slot = "acquire", dependencies = "none", unsuccessful_dependency = "block", accepts_work = true, terminal = false, legacy = "in_progress", satisfies_dependencies = false, completion = "none", recovery = { release = { target = { phase = "build", state = "available" }, manual = false, slot = "release", release_role = true, release_reservation = false }, lost_holder = { target = { phase = "build", state = "available" }, manual = false, slot = "release", release_role = true, release_reservation = false }, attempt_failure = { manual = true, slot = "retain", release_role = false, release_reservation = false }, cancellation_acknowledged = { manual = true, slot = "retain", release_role = false, release_reservation = false } } },
  { at = { phase = "build", state = "blocked" }, slot = "release", dependencies = "none", unsuccessful_dependency = "block", accepts_work = false, terminal = false, legacy = "blocked", satisfies_dependencies = false, completion = "none" },
  { at = { phase = "verify", state = "awaiting" }, slot = "release", dependencies = "none", unsuccessful_dependency = "block", accepts_work = false, terminal = false, legacy = "review", satisfies_dependencies = false, completion = "none" },
  { at = { phase = "verify", state = "accepted" }, slot = "release", dependencies = "none", unsuccessful_dependency = "block", accepts_work = false, terminal = true, legacy = "done", outcome = "success", satisfies_dependencies = true, completion = "covered_close" },
]
edges = [
  { id = "claim", label = "Claim", from = { phase = "build", state = "available" }, to = { phase = "build", state = "working" }, operation = "claim", authority = [{ kind = "task_role", role = "executor" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "acquire_role", role = "executor" }, { kind = "slot", policy = "acquire" }], automatic = false, human_wait = false },
  { id = "submit", label = "Submit", from = { phase = "build", state = "working" }, to = { phase = "verify", state = "awaiting" }, operation = "request_review", authority = [{ kind = "task_role", role = "executor" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "slot", policy = "release" }], automatic = false, human_wait = false },
  { id = "block", label = "Block", from = { phase = "build", state = "available" }, to = { phase = "build", state = "blocked" }, operation = "block", authority = [{ kind = "project_capability", capability = "owner" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "block", save_resume = true }, { kind = "slot", policy = "release" }], automatic = false, human_wait = false },
  { id = "resume", label = "Resume", from = { phase = "build", state = "blocked" }, to = { phase = "build", state = "available" }, operation = "resume", authority = [{ kind = "project_capability", capability = "owner" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "clear_block" }, { kind = "slot", policy = "release" }], automatic = false, human_wait = false },
  { id = "accept", label = "Accept", from = { phase = "verify", state = "awaiting" }, to = { phase = "verify", state = "accepted" }, operation = "close_covered", authority = [{ kind = "project_capability", capability = "owner" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "outcome", outcome = "success", satisfies_dependencies = true }, { kind = "slot", policy = "release" }], automatic = false, human_wait = true },
]

[[workflow_mappings]]
from = { phase = "ready", state = "available" }
to = { phase = "build", state = "available" }
"#;

fn custom() -> Custom {
    let (definition, mappings) = written(TWO).unwrap();
    let definition = definition.unwrap();
    let revision = definition
        .revision(Source {
            commit: "1".repeat(40),
            project_blob: "2".repeat(40),
        })
        .unwrap();
    Custom {
        definition,
        revision,
        mappings,
        authorities: Vec::new(),
        project: None,
    }
}

/// The hub's last read of `slug`'s place on the wire, if it read the task.
fn place(project: &Project, slug: &str) -> Option<symphony_api::workflow::TaskPlace> {
    let read = project.hub.tasks_read().ok()?;
    read.tasks.iter().find(|t| t.slug == slug)?.workflow.clone()
}

/// Wait for the hub to read `slug` at `state`; only a stall reaches the backstop.
fn reads_at(project: &Project, slug: &str, state: &str) {
    let waited = test_time::until_true(
        format!("the hub to read {slug} at {state}"),
        BACKSTOP,
        || place(project, slug).is_some_and(|p| p.state == state),
    );
    if let Err(stall) = waited {
        panic!("{stall}; it read {:?}", place(project, slug));
    }
}

/// The workflow changes among the feed events the project's page published.
fn generations(project: &Project) -> Vec<Value> {
    published(project)
        .into_iter()
        .filter(|change| change["region"] == "workflow")
        .collect()
}

/// The feed events the project's page published, as JSON.
fn published(project: &Project) -> Vec<Value> {
    let pages = project.hub.pages.read().unwrap();
    let Some(page) = pages.get("") else {
        return Vec::new();
    };
    page.history
        .iter()
        .flat_map(|event| {
            let event: Value = serde_json::from_str(&event.json).unwrap();
            event["changes"].as_array().cloned().unwrap_or_default()
        })
        .collect()
}

#[test]
fn the_owner_reads_a_workflow_and_takes_an_edge_through_the_pages_routes() {
    let tree = Tree::new();
    let project = Project::made_by(&tree, "flow", writer);
    project.reads(&["flow/one"]);
    let shared = Shared::new(1, 1234, &project.root, None, crate::testing::open_gate());
    assert!(shared.api.set(project.hub.clone()).is_ok());
    let database = symphony::taskview::connect_in(&project.root, env(&project.root))
        .unwrap()
        .database()
        .database()
        .to_string();

    // A board whose own writers keep its tasks has no workflow, and its tasks no place.
    let (status, none) = answer(&shared, asked("GET", "/api/workflow", None));
    assert_eq!(status, 200, "{none}");
    assert_eq!(none, json!({"project": database, "workflow": null}));
    let (_, tasks) = answer(&shared, asked("GET", "/api/tasks", None));
    assert!(tasks["tasks"][0].get("workflow").is_none(), "{tasks}");
    let board = "/api/task-board?view=waiting&counts=1";
    let (_, listing) = answer(&shared, asked("GET", board, None));
    assert!(listing["waiting"].get("workflow").is_none(), "{listing}");
    let (status, unheld) = answer(
        &shared,
        asked("GET", "/api/workflow-actions?slug=flow/one", None),
    );
    assert_eq!(status, 404, "{unheld}");

    let custom = custom();
    writer(&project.root)
        .activate_workflow(&custom, &Host::local())
        .unwrap();
    reads_at(&project, "flow/one", "available");
    // The activation is said on the feed once the hub reads it, so a page reads the definition.
    // The hub publishes a read a step after it keeps it, so the wait is for the feed's own event.
    let said = test_time::until_true("the page's feed to say the activation", BACKSTOP, || {
        !generations(&project).is_empty()
    });
    if let Err(stall) = said {
        panic!("{stall}; the feed said {:?}", published(&project));
    }
    assert_eq!(
        generations(&project),
        [json!({"region": "workflow", "data": {"generation": 1}})]
    );

    let (status, read) = answer(&shared, asked("GET", "/api/workflow", None));
    assert_eq!(status, 200, "{read}");
    assert_eq!(read["project"], database);
    assert_eq!(read["workflow"]["activation_generation"], 1);
    assert_eq!(
        read["workflow"]["revision"]["digest"],
        custom.revision.digest
    );
    assert_eq!(
        read["workflow"]["definition"]["phases"][1]["label"],
        "Verify"
    );
    let (_, tasks) = answer(&shared, asked("GET", "/api/tasks", None));
    assert_eq!(
        tasks["tasks"][0]["workflow"],
        json!({"phase": "build", "state": "available", "revision": 1}),
        "{tasks}"
    );
    // The board's listing says the generation a page reads the definition by, and the phase.
    let (_, listing) = answer(&shared, asked("GET", board, None));
    assert_eq!(listing["waiting"]["workflow"], 1, "{listing}");
    assert_eq!(listing["waiting"]["states"], json!([["available", 1]]));
    assert_eq!(listing["counts"], json!([["phase:build", 1]]));

    // The owner's preview: Block is theirs, Claim an agent's.
    let (status, next) = answer(
        &shared,
        asked("GET", "/api/workflow-actions?slug=flow/one", None),
    );
    assert_eq!(status, 200, "{next}");
    assert_eq!(next["snapshot"]["task_revision"], 1);
    let action = |next: &Value, edge: &str| {
        (next["actions"].as_array().unwrap().iter())
            .find(|a| a["edge"] == edge)
            .cloned()
            .unwrap_or_else(|| panic!("no {edge} in {next}"))
    };
    assert_eq!(action(&next, "block")["unavailable"], json!([]));
    let claim = action(&next, "claim");
    assert!(
        (claim["unavailable"].as_array().unwrap()).contains(&json!("unauthorized")),
        "{claim}"
    );

    let request = |key: &str, expected: &Value| {
        json!({"slug": "flow/one", "request": {
            "project": database, "task": "flow/one", "edge": "block", "expected": expected,
            "input_revisions": {}, "idempotency_key": key, "inputs": {}}})
    };
    let applied = request("page:flow/one:1:block:a", &next["snapshot"]);
    let (status, taken) = answer(
        &shared,
        asked("POST", "/api/workflow-apply", Some(&applied)),
    );
    assert_eq!(status, 200, "{taken}");
    assert_eq!(taken["ok"], true);
    assert_eq!(taken["result"]["transition"], "flow/one@2");
    assert_eq!(
        taken["result"]["next"]["snapshot"]["at"]["state"],
        "blocked"
    );
    // The same request again answers what the first did; it moves nothing.
    let (status, again) = answer(
        &shared,
        asked("POST", "/api/workflow-apply", Some(&applied)),
    );
    assert_eq!((status, &again), (200, &taken));
    // Another from the revision it left is stale: refused by its code, never replayed.
    let (status, stale) = answer(
        &shared,
        asked(
            "POST",
            "/api/workflow-apply",
            Some(&request("page:flow/one:1:block:b", &next["snapshot"])),
        ),
    );
    assert_ne!(status, 200, "{stale}");
    let said = stale["error"].as_str().unwrap_or_default();
    assert!(said.contains("stale_task"), "{stale}");
    assert!(stale.get("guard").is_none(), "no guard refused it: {stale}");

    reads_at(&project, "flow/one", "blocked");
    let (_, next) = answer(
        &shared,
        asked("GET", "/api/workflow-actions?slug=flow/one", None),
    );
    assert_eq!(next["snapshot"]["task_revision"], 2);
    assert_eq!(action(&next, "resume")["unavailable"], json!([]));
    let (_, tasks) = answer(&shared, asked("GET", "/api/tasks", None));
    assert_eq!(
        tasks["tasks"][0]["workflow"],
        json!({"phase": "build", "state": "blocked", "revision": 2,
               "resume": {"phase": "build", "state": "available"}}),
        "{tasks}"
    );
    let (status, missing) = answer(
        &shared,
        asked("GET", "/api/workflow-actions?slug=flow/none", None),
    );
    assert_eq!(status, 404, "{missing}");
}

/// [`TWO`] with the owner's Block guarded by `approved`, which the `approvals` authority attests
/// by a key the project commits (the curve's base point, which signs nothing here).
fn guarded() -> Custom {
    let text = TWO
        .replacen("adapters = []", "adapters = [\"approvals\"]", 1)
        .replacen(
            "guards = []",
            "guards = [{ id = \"approved\", adapter = \"approvals\" }]",
            1,
        )
        .replacen(
            "operation = \"block\", authority = [{ kind = \"project_capability\", capability = \"owner\" }], inputs = [], dependencies = \"none\", guards = []",
            "operation = \"block\", authority = [{ kind = \"project_capability\", capability = \"owner\" }], inputs = [], dependencies = \"none\", guards = [\"approved\"]",
            1,
        );
    assert!(text.contains("guards = [\"approved\"]"), "{text}");
    let (definition, mappings) = written(&text).unwrap();
    let definition = definition.unwrap();
    let revision = definition
        .revision(Source {
            commit: "1".repeat(40),
            project_blob: "2".repeat(40),
        })
        .unwrap();
    Custom {
        definition,
        revision,
        mappings,
        authorities: vec![symphony_board::workflow::Attester {
            adapter: "approvals".into(),
            issuer: "ledger".into(),
            contract: symphony_api::workflow::ATTESTATION_CONTRACT,
            key: format!("ed25519:58{}", "66".repeat(31)),
        }],
        project: Some("flow".into()),
    }
}

/// A task's guards through the page's routes: nothing on a board with no workflow, then the
/// authority the activation commits, the guard and the edge it holds; and the owner's take of
/// that edge refused with the guard that refused it beside the refusal's words.
#[test]
fn the_owner_reads_a_tasks_guards_and_a_guards_refusal_names_it_beside_its_words() {
    let tree = Tree::new();
    let project = Project::made_by(&tree, "flow", writer);
    project.reads(&["flow/one"]);
    let shared = Shared::new(
        1,
        1234,
        &project.root,
        None,
        Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
    );
    assert!(shared.api.set(project.hub.clone()).is_ok());
    let database = symphony::taskview::connect_in(&project.root, env(&project.root))
        .unwrap()
        .database()
        .database()
        .to_string();
    let guards = "/api/workflow-guards?slug=flow/one";
    let (status, none) = answer(&shared, asked("GET", guards, None));
    assert_eq!(status, 200, "{none}");
    assert_eq!(none, json!({"authorities": [], "guards": [], "edges": []}));

    writer(&project.root)
        .activate_workflow(&guarded(), &Host::local())
        .unwrap();
    reads_at(&project, "flow/one", "available");
    let (status, read) = answer(&shared, asked("GET", guards, None));
    assert_eq!(status, 200, "{read}");
    assert_eq!(
        read,
        json!({
            // The authority has reported no state, and no plugin of this project answers for it.
            "authorities": [{"adapter": "approvals", "issuer": "ledger", "conflicted": false,
                             "admitted": 0, "asked_here": false}],
            "guards": [{"guard": "approved", "adapter": "approvals"}],
            "edges": [{"edge": "block", "guards": ["approved"]}],
        })
    );

    let (_, next) = answer(
        &shared,
        asked("GET", "/api/workflow-actions?slug=flow/one", None),
    );
    let applied = json!({"slug": "flow/one", "request": {
        "project": database, "task": "flow/one", "edge": "block", "expected": next["snapshot"],
        "input_revisions": {}, "idempotency_key": "page:flow/one:1:block:a", "inputs": {}}});
    let (status, refused) = answer(
        &shared,
        asked("POST", "/api/workflow-apply", Some(&applied)),
    );
    assert_eq!(status, 400, "{refused}");
    let said = refused["error"].as_str().unwrap_or_default();
    assert!(
        said.starts_with("guard_unavailable: edge block is guarded by approved"),
        "{refused}"
    );
    assert_eq!(
        refused["guard"],
        json!({"guard": "approved", "adapter": "approvals", "why": "unreported"})
    );
    reads_at(&project, "flow/one", "available");
}

/// The hub says a generation once it changes after its first read, and nothing for one it knows.
#[test]
fn the_hub_says_a_new_active_generation_once() {
    let tree = Tree::new();
    let root = crate::testing::repository(&tree);
    let hub = super::Hub::new(&root, &root.join("build/client"));
    assert_eq!(hub.workflow_read(None), None);
    assert_eq!(hub.workflow_read(None), None);
    assert_eq!(
        hub.workflow_read(Some(1)),
        Some(json!({"region": "workflow", "data": {"generation": 1}}))
    );
    assert_eq!(hub.workflow_read(Some(1)), None);
    assert_eq!(
        hub.workflow_read(Some(2)),
        Some(json!({"region": "workflow", "data": {"generation": 2}}))
    );
    // A board read first with a workflow active says nothing: its pages read the definition then.
    let fresh = super::Hub::new(&root, &root.join("build/client"));
    assert_eq!(fresh.workflow_read(Some(3)), None);
}
