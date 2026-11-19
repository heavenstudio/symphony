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

#[test]
fn test_presence_changes_the_waits_line_even_when_the_board_did_not_change() {
    let tree = Tree::new();
    let root = repository(&tree);
    let f = Fixture::new(
        Seed::default()
            .session(QUIET)
            .task(OPEN, "in_progress")
            .role(OPEN, QUIET, "executor"),
    );
    f.advance(700.0);
    let hub = Hub::new(&root, &root.join("build/client"));
    hub.refresh_page("", &root).unwrap();
    let raw = f.store.board().unwrap();
    assert_eq!(raw.tasks[0].roles[0].health, symphony_board::Health::Lost);
    let waits = || {
        hub.task_snapshot().unwrap().tasks[0]
            .waits_on
            .clone()
            .unwrap()
    };
    Published::of(&hub, &f.store, vec![agent(QUIET, "gone", NOW)]);
    assert_eq!(waits().kind, "nobody");
    let began = Published::of(&hub, &f.store, vec![agent(QUIET, "running_test", NOW)]);
    assert_eq!(
        (waits().kind.as_str(), waits().who.as_str()),
        ("working", QUIET)
    );
    assert_eq!(began.regions().get("task-fields"), Some(&1));
    assert!(!began.regions().contains_key("task-counts"));
    let same = Published::of(&hub, &f.store, vec![agent(QUIET, "running_test", NOW)]);
    assert!(same.changes.is_empty());
    let ended = Published::of(&hub, &f.store, vec![agent(QUIET, "gone", NOW)]);
    assert_eq!(waits().kind, "nobody");
    assert_eq!(ended.regions().get("task-fields"), Some(&1));
    assert_eq!(f.store.board().unwrap(), raw);
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
        .raw()
        .row("task", OPEN)
        .set(json!({"title": "Renamed"}))
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
        .raw()
        .row("role", json!([OPEN, QUIET, "reviewer"]))
        .put(json!({"slug": OPEN, "handle": QUIET, "role": "reviewer", "since": NOW + 20.0}))
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

#[test]
fn changed_review_counts_are_sent_and_an_unavailable_summary_replaces_the_task() {
    let task: symphony_api::Task = serde_json::from_value(json!({
        "slug": OPEN, "title": "Open", "body": "The brief", "status": "review", "priority": 2,
        "parent": null, "created_by": "owner", "created": 1.0, "updated": 2.0, "links": [],
        "depends": [], "instruction": false, "roles": [], "review_rounds": 1,
    }))
    .unwrap();
    for count in [Some(0), Some(2), None] {
        let mut after = task.clone();
        after.review_rounds = count;
        assert!(!super::tasks_alike(
            std::slice::from_ref(&task),
            std::slice::from_ref(&after)
        ));
        let mut changes = Vec::new();
        super::task_changes(
            &mut changes,
            std::slice::from_ref(&task),
            std::slice::from_ref(&after),
        );
        assert_eq!(changes.len(), 1);
        if let Some(count) = count {
            assert_eq!(changes[0]["region"], "task-fields");
            assert_eq!(
                changes[0]["data"]["fields"],
                json!({"review_rounds": count})
            );
        } else {
            assert_eq!(changes[0]["region"], "task");
            assert_eq!(
                changes[0]["data"]["value"],
                serde_json::to_value(&after).unwrap()
            );
        }
        let mut unchanged = Vec::new();
        super::task_changes(
            &mut unchanged,
            std::slice::from_ref(&after),
            std::slice::from_ref(&after),
        );
        assert!(unchanged.is_empty());
    }
}

#[test]
fn owned_json_publication_keeps_exact_tokens_key_order_versions_history_and_notifications() {
    let tree = Tree::new();
    let root = repository(&tree);
    let hub = Hub::new(&root, &root.join("build/client"));
    hub.refresh_page("", &root).unwrap();
    let payload: Value = serde_json::from_str(r#"{"z":"quote \" slash \\ newline\n null\u0000 猫","a":[null,true,false,-0.0,1.50,18446744073709551616,1.234567890123456789123456789,{"last":0,"first":1}]}"#).unwrap();
    assert_eq!(payload.as_object().unwrap().keys().next().unwrap(), "z");
    let old_message = super::region_event("message", json!(payload));
    let new_message = super::region_event("message", payload.clone());
    assert_eq!(old_message.to_string(), new_message.to_string());
    let mut notice = hub.changed.subscribe();
    let mut pages = hub.pages.write().unwrap();
    let page = pages.get_mut("").unwrap();
    let initial = page.board.version;
    let before_history: Vec<_> = page
        .history
        .iter()
        .map(|event| event.json.clone())
        .collect();
    let mut before_notice = *notice.borrow_and_update();
    hub.publish(page, vec![]);
    assert_eq!(page.board.version, initial);
    assert_eq!(
        page.history
            .iter()
            .map(|event| event.json.clone())
            .collect::<Vec<_>>(),
        before_history
    );
    assert!(!notice.has_changed().unwrap());
    for i in 1..=(super::HISTORY + 3) {
        let changes = vec![
            super::region_event("message", payload.clone()),
            json!({"region":"counts","data":{"n":i}}),
        ];
        let version = initial + u64::try_from(i).unwrap();
        let expected = json!({"version":version,"changes":changes}).to_string();
        hub.publish(page, changes);
        assert_eq!(page.board.version, version);
        let event = page.history.back().unwrap();
        assert_eq!(
            (event.version, event.json.as_bytes()),
            (version, expected.as_bytes())
        );
        assert!(notice.has_changed().unwrap());
        before_notice += 1;
        assert_eq!(*notice.borrow_and_update(), before_notice);
    }
    assert_eq!(page.history.len(), super::HISTORY);
    assert_eq!(page.history.front().unwrap().version, initial + 4);
    page.board.version = u64::MAX - 1;
    let changes = vec![new_message];
    let expected = json!({"version":u64::MAX,"changes":changes}).to_string();
    hub.publish(page, changes);
    assert_eq!(page.history.back().unwrap().json, expected);
    assert_eq!(page.board.version, u64::MAX);
    hub.publish(page, vec![]);
    assert_eq!(page.board.version, u64::MAX);
}
