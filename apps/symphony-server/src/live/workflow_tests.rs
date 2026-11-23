//! A custom workflow through the Symphony's routes, on a fixture project's own board: the active
//! definition and a task's place read, the local owner's next actions previewed, an edge taken,
//! repeated under its key and refused as stale from the revision it left, and the feed saying the
//! board activated a workflow so a page reads the definition again (docs/workflow/WORKFLOW.md, "The app").

use super::boards_tests::{Project, answer, asked, env, writer};
use crate::front::Shared;
use crate::testing::Tree;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::Duration;
use symphony_api::workflow::{OperationRequest, Source};
use symphony_board::workflow::{Custom, Host, agent_session, local_owner, written};
use symphony_board::{NewSession, NewTask};
use symphony_plugin::execution::Principal;

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
    committed(&format!("{TWO}{READY}"), '1')
}

/// The workflow `text` writes, as committed at a commit of `digit`s in a project file of its own:
/// another text is another file, and the board takes a revision from the active one's file as the
/// active workflow, whatever it says.
fn committed(text: &str, digit: char) -> Custom {
    let (definition, mappings) = written(text).unwrap();
    let definition = definition.unwrap();
    let revision = definition
        .revision(Source {
            commit: digit.to_string().repeat(40),
            project_blob: digit.to_string().repeat(40),
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

/// Wait for the project's page to hear the generations `expected`, in order; only a stall
/// reaches the backstop. The hub publishes a read a step after it keeps it.
fn hears(project: &Project, expected: &[u64]) {
    let said: Vec<Value> = (expected.iter())
        .map(|generation| json!({"region": "workflow", "data": {"generation": generation}}))
        .collect();
    let heard = test_time::until_true("the page's feed to say the generations", BACKSTOP, || {
        generations(project).len() >= said.len()
    });
    if let Err(stall) = heard {
        panic!("{stall}; the feed said {:?}", published(project));
    }
    assert_eq!(generations(project), said);
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
    hears(&project, &[1]);

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
    // No edge leaving its pair takes an accepted output: it names none.
    assert_eq!(next["takes"], json!([]), "{next}");
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

/// [`TWO`] and [`READY`] with its claim taking the accepted output of each task a task depends on.
fn taking() -> Custom {
    let claim = "operation = \"claim\", authority = [{ kind = \"task_role\", role = \"executor\" }], \
                 inputs = [], dependencies = \"none\"";
    assert!(TWO.contains(claim));
    let text = TWO.replacen(claim, &claim.replace("\"none\"", "\"accepted_output\""), 1);
    committed(&format!("{text}{READY}"), '4')
}

/// `edge` on `slug` taken by `principal` from where it reads the task now: the transition.
fn took(board: &symphony_board::Store, slug: &str, principal: &Principal, edge: &str) -> String {
    let host = Host::local();
    let next = board.workflow_actions(slug, principal, &host).unwrap();
    let request = OperationRequest {
        project: board.database().database().to_string(),
        task: slug.into(),
        edge: edge.into(),
        expected: next.snapshot,
        input_revisions: BTreeMap::new(),
        idempotency_key: format!("{slug}:{edge}"),
        inputs: BTreeMap::new(),
    };
    let result = board.apply_operation(principal, &host, &request);
    result
        .unwrap_or_else(|e| panic!("{edge} on {slug}: {e}"))
        .transition
}

/// A take in flight through the actions route: a dependant whose claim takes the accepted output
/// of the task it depends on names that task with none standing, then with the output its
/// acceptance minted; a task whose edges take none names none.
#[test]
fn the_owners_actions_carry_the_output_a_dependants_claim_takes() {
    let tree = Tree::new();
    let project = Project::made_by(&tree, "flow", writer);
    let board = writer(&project.root);
    board
        .create(&NewTask {
            depends: vec!["flow/one".into()],
            ..NewTask::new("flow/two", "Made second", "owner")
        })
        .unwrap();
    project.reads(&["flow/one", "flow/two"]);
    let shared = Shared::new(1, 1234, &project.root, None, crate::testing::open_gate());
    assert!(shared.api.set(project.hub.clone()).is_ok());
    board.activate_workflow(&taking(), &Host::local()).unwrap();
    reads_at(&project, "flow/two", "available");
    let actions = |slug: &str| {
        let (status, read) = answer(
            &shared,
            asked("GET", &format!("/api/workflow-actions?slug={slug}"), None),
        );
        assert_eq!(status, 200, "{read}");
        read
    };

    let read = actions("flow/two");
    assert_eq!(
        read["takes"],
        json!([{"task": "flow/one", "output": null}]),
        "{read}"
    );
    // flow/one depends on nothing, so its claim takes nothing.
    assert_eq!(actions("flow/one")["takes"], json!([]));

    let amber = board
        .register(&NewSession::new("worker"))
        .unwrap()
        .row
        .handle;
    let amber = agent_session(&amber);
    took(&board, "flow/one", &amber, "claim");
    took(&board, "flow/one", &amber, "submit");
    let output = took(&board, "flow/one", &local_owner(), "accept");
    assert_eq!(output, "flow/one@4");
    let read = actions("flow/two");
    assert_eq!(
        read["takes"],
        json!([{"task": "flow/one", "output": output}]),
        "{read}"
    );
    // The page sends it with the claim it previews, which nothing else stops now.
    let claim = (read["actions"].as_array().unwrap().iter())
        .find(|a| a["edge"] == "claim")
        .unwrap();
    assert!(
        !(claim["unavailable"].as_array().unwrap()).contains(&json!("dependency")),
        "{claim}"
    );
}

/// A relabel moves no task, and the page still hears its generation: the board's own read carries
/// it, and the activation moved the board's marker, so the hub's kept read is read again.
#[test]
fn a_relabel_that_moves_no_task_reaches_the_page() {
    let tree = Tree::new();
    let project = Project::made_by(&tree, "flow", writer);
    project.reads(&["flow/one"]);
    let board = writer(&project.root);
    board.activate_workflow(&custom(), &Host::local()).unwrap();
    reads_at(&project, "flow/one", "available");
    hears(&project, &[1]);
    let placed = place(&project, "flow/one");

    let relabelled = committed(
        &format!("{TWO}{READY}").replace(r#"label = "Build""#, r#"label = "Make""#),
        '3',
    );
    assert_eq!(
        board.activate_workflow(&relabelled, &Host::local()),
        Ok(symphony_board::workflow::Followed::Activated(
            symphony_board::workflow::Active {
                generation: 2,
                digest: relabelled.revision.digest.clone(),
            }
        ))
    );
    hears(&project, &[1, 2]);
    assert_eq!(
        place(&project, "flow/one"),
        placed,
        "the relabel moved no task"
    );
    assert_eq!(project.hub.workflow_generation(), Some(2));
}

/// [`TWO`] and [`READY`] with the owner's Block guarded by `approved`, which the `approvals`
/// authority attests by a key the project commits (the curve's base point, which signs nothing
/// here).
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
    let (definition, mappings) = written(&format!("{text}{READY}")).unwrap();
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
    let shared = Shared::new(1, 1234, &project.root, None, crate::testing::open_gate());
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
