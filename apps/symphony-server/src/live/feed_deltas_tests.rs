//! What the feed sends for one change (perf/feed-deltas): a task that changed once, as the fields
//! that changed; a holder's health once, as its session, never again in every task it holds; and
//! for an agent's tool call nothing, as no page shows when an agent last called a tool. Each case
//! counts the changes and bytes one board observation publishes, never a time.

use super::Hub;
use crate::testing::{
    Tree,
    board::{Fixture, NOW, Seed},
    repository,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use symphony_board::rows::SessionRow;

/// The tasks one agent held and finished: each says its holder's health on the board.
const HELD: usize = 24;
/// The session that goes quiet, seen 80 s before the others: 20 s on, it is stale.
const QUIET: &str = "amber-heron";
/// The session that stays live, and holds the one open task.
const LIVE: &str = "birch-wren";
const OPEN: &str = "work/open";

/// A board where [`QUIET`] executed [`HELD`] tasks, each orchestrated by [`LIVE`], who also
/// executes [`OPEN`].
fn board() -> Fixture {
    let mut seed = Seed::default()
        .session_row(SessionRow {
            handle: QUIET.into(),
            kind: "worker".into(),
            connected: NOW - 80.0,
            last_seen: NOW - 80.0,
            ..Default::default()
        })
        .session(LIVE)
        .task(OPEN, "in_progress")
        .role(OPEN, LIVE, "executor");
    for n in 0..HELD {
        let slug = format!("work/done-{n}");
        seed = seed
            .task(&slug, "done")
            .role(&slug, QUIET, "executor")
            .role(&slug, LIVE, "orchestrator");
    }
    Fixture::new(seed)
}

fn agent(handle: &str, state: &str, tool: f64) -> symphony_api::Agent {
    serde_json::from_value(json!({
        "handle": handle, "kind": "worker", "model": "", "parent": null, "task": null,
        "role": null, "tasks": [], "worktree": "", "branch": "", "agent": null,
        "session_id": null, "pane": null, "state": state, "state_detail": null,
        "health": "live", "last_tool_call": tool, "last_message": null,
    }))
    .unwrap()
}

/// What one observation of the board, with `agents` as the agents view read them, published on
/// the main page: its changes, and the bytes of the events that carried them.
struct Published {
    changes: Vec<Value>,
    bytes: usize,
}

impl Published {
    fn of(hub: &Hub, store: &symphony_board::Store, agents: Vec<symphony_api::Agent>) -> Self {
        let last = |hub: &Hub| {
            hub.pages.read().unwrap()[""]
                .history
                .back()
                .map_or(0, |event| event.version)
        };
        let before = last(hub);
        hub.update_board(
            store,
            store.board().unwrap(),
            symphony_api::Agents {
                machine_work: symphony_api::MachineWork::default(),
                generated: store.now(),
                probes: symphony_api::AgentProbes {
                    panes: "ok".into(),
                    slots: "ok".into(),
                },
                agents,
            },
        )
        .unwrap();
        let pages = hub.pages.read().unwrap();
        let events: Vec<_> = pages[""]
            .history
            .iter()
            .filter(|event| event.version > before)
            .collect();
        let changes = events
            .iter()
            .flat_map(|event| {
                let value: Value = serde_json::from_str(&event.json).unwrap();
                value["changes"].as_array().unwrap().clone()
            })
            .collect();
        let bytes = events.iter().map(|event| event.json.len()).sum();
        Self { changes, bytes }
    }

    /// How many changes of each region it holds.
    fn regions(&self) -> BTreeMap<String, usize> {
        let mut regions = BTreeMap::new();
        for change in &self.changes {
            *regions
                .entry(change["region"].as_str().unwrap().to_string())
                .or_default() += 1;
        }
        regions
    }

    /// The changes that name the task `slug`, of any region.
    fn naming(&self, slug: &str) -> Vec<&Value> {
        self.changes
            .iter()
            .filter(|change| change["data"]["key"] == slug)
            .collect()
    }

    /// Its figures, as the thread quotes them.
    fn said(&self, case: &str) -> String {
        format!(
            "feed-deltas {case}: {} changes, {} B: {:?}",
            self.changes.len(),
            self.bytes,
            self.regions()
        )
    }
}

/// The hub over [`board`], its first observation made, with both agents at their first tool call.
fn observed(f: &Fixture, tree: &Tree) -> Hub {
    let root = repository(tree);
    let hub = Hub::new(&root, &root.join("build/client"));
    hub.refresh_page("", &root).unwrap();
    Published::of(&hub, &f.store, both("working", 1.0));
    hub
}

fn both(state: &str, tool: f64) -> Vec<symphony_api::Agent> {
    vec![agent(QUIET, state, tool), agent(LIVE, state, tool)]
}

#[test]
fn a_holders_health_is_sent_once_as_its_session_not_in_every_task_it_held() {
    let tree = Tree::new();
    let f = board();
    let hub = observed(&f, &tree);
    f.advance(20.0);
    let published = Published::of(&hub, &f.store, both("working", 1.0));
    eprintln!("{}", published.said("health sweep"));
    let snapshot = hub.task_snapshot().unwrap();
    let held = snapshot
        .tasks
        .iter()
        .filter(|task| task.roles.iter().any(|role| role.handle == QUIET))
        .collect::<Vec<_>>();
    assert_eq!(held.len(), HELD);
    assert!(
        held.iter().all(|task| task
            .roles
            .iter()
            .any(|r| r.handle == QUIET && r.health == "stale")),
        "a read of the board still says each holder's health"
    );
    // The one session, and the home page's count of live agents, which fell from two to one.
    assert_eq!(
        published.regions(),
        BTreeMap::from([("counts".to_string(), 1), ("session".to_string(), 1)]),
        "{}",
        published.said("health sweep")
    );
    let session = published
        .changes
        .iter()
        .find(|change| change["region"] == "session")
        .unwrap();
    assert_eq!(session["data"]["key"], QUIET);
    assert_eq!(session["data"]["value"]["health"], "stale");
    let counts = published
        .changes
        .iter()
        .find(|change| change["region"] == "counts")
        .unwrap();
    assert_eq!(counts["data"]["tasks"]["agents_live"], 1);
}

#[test]
fn a_changed_task_is_sent_once_as_the_fields_that_changed() {
    let tree = Tree::new();
    let f = board();
    let hub = observed(&f, &tree);
    f.store
        .client()
        .query(
            "UPDATE type::record('task', $slug) SET title = 'Renamed' RETURN NONE;",
            json!({"slug": OPEN}),
        )
        .unwrap();
    let published = Published::of(&hub, &f.store, both("working", 1.0));
    eprintln!("{}", published.said("one title"));
    let naming = published.naming(OPEN);
    assert_eq!(naming.len(), 1, "{}", published.said("one title"));
    assert_eq!(naming[0]["region"], "task-fields");
    assert_eq!(naming[0]["data"]["fields"], json!({"title": "Renamed"}));
}

#[test]
fn a_new_holder_is_sent_with_the_roles_it_joined_and_their_health() {
    let tree = Tree::new();
    let f = board();
    let hub = observed(&f, &tree);
    f.advance(20.0);
    // Written as the store writes a role, without the claim's beat, which would make it live.
    f.store
        .client()
        .query(
            "UPSERT type::record('role', [$slug, $handle, 'reviewer']) \
             CONTENT {slug: $slug, handle: $handle, role: 'reviewer', since: $now} RETURN NONE;",
            json!({"slug": OPEN, "handle": QUIET, "now": NOW + 20.0}),
        )
        .unwrap();
    let published = Published::of(&hub, &f.store, both("working", 1.0));
    eprintln!("{}", published.said("health sweep and a role"));
    let naming = published.naming(OPEN);
    assert_eq!(
        naming.len(),
        1,
        "{}",
        published.said("health sweep and a role")
    );
    assert_eq!(naming[0]["region"], "task-fields");
    let roles = &naming[0]["data"]["fields"]["roles"];
    let quiet = roles
        .as_array()
        .unwrap()
        .iter()
        .find(|role| role["handle"] == QUIET)
        .unwrap_or_else(|| panic!("{roles}"));
    assert_eq!(
        quiet["health"], "stale",
        "a role sent says its holder's health now"
    );
    assert_eq!(
        published.regions().get("task-fields"),
        Some(&1),
        "the tasks it finished are not sent again: {}",
        published.said("health sweep and a role")
    );
}

#[test]
fn an_agents_tool_call_sends_nothing_and_a_change_it_shows_sends_its_record() {
    let tree = Tree::new();
    let f = board();
    let hub = observed(&f, &tree);
    let published = Published::of(&hub, &f.store, both("working", 2.0));
    eprintln!("{}", published.said("tool calls"));
    assert_eq!(
        published.changes,
        Vec::<Value>::new(),
        "{}",
        published.said("tool calls")
    );

    let published = Published::of(
        &hub,
        &f.store,
        vec![agent(QUIET, "idle", 3.0), agent(LIVE, "working", 3.0)],
    );
    eprintln!("{}", published.said("one state"));
    assert_eq!(
        published.regions(),
        BTreeMap::from([("agent".to_string(), 1)])
    );
    let record = &published.changes[0]["data"];
    assert_eq!(record["key"], QUIET);
    assert_eq!(record["value"]["state"], "idle");
    assert_eq!(
        record["value"]["last_tool_call"], 3.0,
        "the record sent is the agent's as it is now"
    );
}

/// Two tasks are alike, as the feed compares them, when they differ in their holders' health
/// alone: a change of any other field, a role's among them, is the task's own.
#[test]
fn a_task_is_alike_another_only_in_its_holders_health() {
    let task: symphony_api::Task = serde_json::from_value(json!({
        "slug": OPEN, "title": "Open", "body": "The brief", "status": "in_progress", "priority": 2,
        "parent": null, "created_by": "owner", "created": 1.0, "updated": 2.0, "links": [],
        "depends": [], "instruction": false,
        "roles": [{"handle": LIVE, "role": "executor", "since": 1.0, "expires": null, "health": "live"}],
    }))
    .unwrap();
    let mut stale = task.clone();
    stale.roles[0].health = "stale".into();
    assert!(super::shown_alike(&task, &stale));
    assert!(super::tasks_alike(
        std::slice::from_ref(&task),
        std::slice::from_ref(&stale)
    ));
    type Change = fn(&mut symphony_api::Task);
    let changes: [(&str, Change); 9] = [
        ("body", |t| t.body.push('.')),
        ("status", |t| t.status = "review".into()),
        ("updated", |t| t.updated += 1.0),
        ("depends", |t| t.depends.push("work/other".into())),
        ("role", |t| t.roles[0].role = "reviewer".into()),
        ("since", |t| t.roles[0].since += 1.0),
        ("expires", |t| t.roles[0].expires = Some(9.0)),
        ("roles", |t| t.roles.clear()),
        ("fields", |t| {
            let mut value = serde_json::to_value(&*t).unwrap();
            value["fields"] = json!({"plugin": {"lane": "fast"}});
            *t = serde_json::from_value(value).unwrap();
        }),
    ];
    for (field, change) in changes {
        let mut other = stale.clone();
        change(&mut other);
        assert!(!super::shown_alike(&task, &other), "{field}");
    }
}
