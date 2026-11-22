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

/// A ready task the board's writers keep starts available.
const READY: &str = r#"
[[workflow_mappings]]
from = { phase = "ready", state = "available" }
to = { phase = "build", state = "available" }
"#;

/// The board's tests' `TWO` (work built, then verified), alone: [`custom`] adds [`READY`] after it.
const TWO: &str = concat!(
    "\n",
    include_str!("../../../../crates/tasks/tests/fixtures/two_workflow.toml")
);

fn custom() -> Custom {
    let (definition, mappings) = written(&format!("{TWO}{READY}")).unwrap();
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
