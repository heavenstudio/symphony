//! Dispatchers built before one orchestrator per task and after it, alternating on one
//! orchestrator's callback state: the fields older ones read keep their whole-board meaning, and
//! neither repeats a line the other said.

use super::common::Fixture;
use super::orchestrators::{go_quiet_but, heard};
use super::quiet_callbacks::{listen, with_volume};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use symphony_board::health::{EXECUTING, FINISHED};
use symphony_board::{Health, NewSession, NewTask, Task};

/// What a dispatcher built before one orchestrator per task records of `handle` once it has looked
/// and told: its look of the whole board (every ready task, every task below what it orchestrates
/// and the whole board's staffing; its children by parent), every open task's status, and every
/// entry of the whole digest as delivered. Reckoned here by those rules, from the board as it is.
pub(super) fn older_record(fixture: &Fixture, handle: &str) -> Value {
    let board = fixture.store.board().unwrap();
    let open = |slug: &str| {
        (board.tasks.iter()).any(|t| t.slug == slug && !FINISHED.contains(&t.status.as_str()))
    };
    let holds = |t: &&Task, roles: &[&str]| {
        (t.roles.iter()).any(|r| r.handle == handle && roles.contains(&r.role.as_str()))
    };
    let mine: Vec<&Task> = (board.tasks.iter())
        .filter(|t| t.roles.iter().any(|r| r.handle == handle))
        .collect();
    let mut below = BTreeSet::new();
    let mut todo: Vec<String> = (mine.iter())
        .filter(|t| holds(t, &["orchestrator"]))
        .map(|t| t.slug.clone())
        .collect();
    while let Some(slug) = todo.pop() {
        let children = (board.tasks.iter()).filter(|t| t.parent.as_deref() == Some(slug.as_str()));
        for child in children {
            if below.insert(child.slug.clone()) {
                todo.push(child.slug.clone());
            }
        }
    }
    let status = |slug: &str| {
        let task = (board.tasks.iter()).find(|t| t.slug == slug);
        task.unwrap().status.clone()
    };
    let statuses: BTreeMap<String, String> = (mine.iter().map(|t| t.slug.clone()))
        .chain(below.iter().cloned())
        .filter(|slug| open(slug))
        .map(|slug| (slug.clone(), status(&slug)))
        .collect();
    let verdict_tasks: BTreeSet<String> = (mine.iter())
        .filter(|t| holds(t, &["executor", "orchestrator"]))
        .map(|t| t.slug.clone())
        .chain(below.iter().cloned())
        .filter(|slug| open(slug))
        .collect();
    let roles: BTreeSet<(String, String, String)> = (mine.iter())
        .filter(|t| open(&t.slug))
        .flat_map(|t| (t.roles.iter()).map(|r| (t.slug.clone(), r.handle.clone(), r.role.clone())))
        .collect();
    let ready: BTreeMap<String, String> = (board.tasks.iter())
        .filter(|t| t.status == "ready")
        .map(|t| (t.slug.clone(), t.title.clone()))
        .collect();
    let active = |t: &&Task| {
        matches!(t.status.as_str(), "ready" | "review") || EXECUTING.contains(&t.status.as_str())
    };
    let staffing: BTreeSet<(&str, &str, &str, bool)> = (board.tasks.iter())
        .filter(active)
        .flat_map(|t| {
            (t.roles.iter())
                .filter(|r| matches!(r.role.as_str(), "executor" | "reviewer" | "reserved"))
                .map(|r| {
                    (
                        t.slug.as_str(),
                        r.handle.as_str(),
                        r.role.as_str(),
                        r.health.holds(),
                    )
                })
        })
        .collect();
    let children: BTreeMap<String, Health> = (board.sessions.iter())
        .filter(|s| s.row.parent.as_deref() == Some(handle))
        .map(|s| (s.row.handle.clone(), s.health))
        .collect();
    let every: BTreeMap<String, String> = (board.tasks.iter())
        .filter(|t| open(&t.slug))
        .map(|t| (t.slug.clone(), t.status.clone()))
        .collect();
    // With no pane to read, the agents view calls a stale worker idle.
    let idle: Vec<String> = (board.sessions.iter())
        .filter(|s| {
            s.health == Health::Stale && !matches!(s.row.kind.as_str(), "orchestrator" | "chat")
        })
        .map(|s| s.row.handle.clone())
        .collect();
    let digest =
        symphony_board::cli::pull::digest(&fixture.store, &idle, None, fixture.now(), &[]).unwrap();
    json!({
        "seen": {
            "latest_message": board.latest_message, "verdict_tasks": verdict_tasks,
            "staffing": serde_json::to_string(&staffing).unwrap(), "roles": roles,
            "statuses": statuses, "ready": ready, "children": children,
        },
        "statuses": every,
        "pull_entries": digest.entries(),
    })
}

/// The fields of `handle`'s callback state older dispatchers read: its record, every open status
/// and the delivered digest entries (as a set: their order is the store's).
pub(super) fn older_fields(fixture: &Fixture, handle: &str) -> Value {
    let state = fixture
        .store
        .raw()
        .row("callback_state", handle)
        .fields(&["seen", "statuses", "pull_entries"])
        .unwrap();
    let entries: BTreeSet<(String, String)> =
        serde_json::from_value(state["pull_entries"].clone()).unwrap();
    json!({"seen": state["seen"], "statuses": state["statuses"], "pull_entries": entries})
}

/// An older dispatcher's look and write: what it told is in what it records.
fn written_by_an_older_dispatcher(fixture: &Fixture, handle: &str) {
    let record = older_record(fixture, handle);
    fixture
        .store
        .raw()
        .row("callback_state", handle)
        .set(json!({
            "seen": record["seen"], "statuses": record["statuses"],
            "pull_entries": record["pull_entries"], "pull_queued": null,
        }))
        .unwrap();
}

/// `slug`, below `parent`.
fn subtask(fixture: &Fixture, slug: &str, parent: &str) {
    fixture.task_with(NewTask {
        parent: Some(parent.into()),
        ..NewTask::new(slug, "Jump departure", "owner")
    });
}

/// The board two orchestrators share: `orchestrator` holds `scratch/a`, which `hand` executes, and
/// `other` holds `scratch/a/b` below it and `scratch/b`, whose `scratch/b/1` is ready and whose
/// `scratch/b/rev` the worker has in review, reviewed by `kid`. The worker also works on
/// `scratch/free`, nobody's. `kid` is the orchestrator's child, `hand` the other's.
struct TwoOrchestrators {
    fixture: Fixture,
    orchestrator: String,
    other: String,
    hand: String,
    worker: String,
}

impl TwoOrchestrators {
    fn new() -> TwoOrchestrators {
        let fixture = Fixture::new(crate::common::Seed::default());
        let orchestrator = fixture.session("orchestrator");
        let other = fixture.session("orchestrator");
        let child = |kind: &str, parent: &str| {
            fixture.session_with(NewSession {
                parent: Some(parent.to_string()),
                ..NewSession::new(kind)
            })
        };
        let (kid, hand) = (child("reviewer", &orchestrator), child("worker", &other));
        let worker = fixture.session("worker");
        let role = |slug: &str, handle: &str, role: &str| {
            fixture.store.take_role(slug, handle, role).unwrap();
        };
        fixture.task("scratch/a");
        role("scratch/a", &orchestrator, "orchestrator");
        fixture.store.claim("scratch/a", &hand).unwrap();
        subtask(&fixture, "scratch/a/b", "scratch/a");
        role("scratch/a/b", &other, "orchestrator");
        fixture.task("scratch/b");
        role("scratch/b", &other, "orchestrator");
        subtask(&fixture, "scratch/b/1", "scratch/b");
        subtask(&fixture, "scratch/b/rev", "scratch/b");
        fixture.store.claim("scratch/b/rev", &worker).unwrap();
        fixture.historical_review("scratch/b/rev");
        role("scratch/b/rev", &kid, "reviewer");
        fixture.task("scratch/free");
        fixture.store.claim("scratch/free", &worker).unwrap();
        TwoOrchestrators {
            fixture,
            orchestrator,
            other,
            hand,
            worker,
        }
    }
}

#[test]
fn an_older_dispatcher_between_newer_ones_repeats_no_line_and_widens_none() {
    let TwoOrchestrators {
        fixture,
        orchestrator,
        other,
        hand,
        worker,
    } = TwoOrchestrators::new();
    let listener = listen(&fixture, &orchestrator);
    let ears = [(&listener, orchestrator.as_str())];
    let pull = |executing: u8, orphaned: u8| {
        format!(
            "PULL executing={executing} ready_unstaffed=0 in_progress_orphaned={orphaned} review_unreviewed=0 landed_open=0 idle=0"
        )
    };
    assert_eq!(
        heard(&fixture, &worker, &ears),
        [with_volume(&orchestrator, 1, &pull(2, 0))
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>()],
        "the other's ready and unreviewed work is its own"
    );
    // The newer dispatcher looks first as kid and hand go quiet: the orchestrator hears of
    // hand only.
    go_quiet_but(&fixture, &[&orchestrator, &other, &worker]);
    let mut rounds = vec![heard(&fixture, &worker, &ears).remove(0)];
    let mut joined = String::new();
    // Writes on another host, which leave this host's recipient to the next dispatch here.
    let elsewhere = fixture.elsewhere();
    for round in 1..=6 {
        assert_eq!(
            older_fields(&fixture, &orchestrator),
            older_record(&fixture, &orchestrator),
            "the fields older dispatchers read keep their meaning, round {round}"
        );
        // An older dispatcher looks first, and what it says is its own: this listener hears the
        // newer dispatcher alone.
        if round == 2 {
            elsewhere
                .move_to("scratch/free", "blocked", &worker)
                .unwrap();
        } else if round == 3 {
            let next = NewTask {
                parent: Some("scratch/a".into()),
                ..NewTask::new("scratch/a/next", "Jump departure", "owner")
            };
            elsewhere.create(&next).unwrap();
            let r2 = elsewhere.register(&NewSession::new("reviewer")).unwrap();
            (elsewhere.take_role("scratch/a", &r2.row.handle, "reviewer")).unwrap();
        }
        written_by_an_older_dispatcher(&fixture, &orchestrator);
        if round == 4 {
            // The newer dispatcher looks first. The other's new row in the whole digest is
            // recorded as withheld, so an older dispatcher never sends it.
            subtask(&fixture, "scratch/b/2", "scratch/b");
            joined = fixture.session("reviewer");
            fixture
                .store
                .take_role_beside("scratch/a", &joined, "reviewer", Some("a second look"))
                .unwrap();
        }
        rounds.push(heard(&fixture, &worker, &ears).remove(0));
    }
    let none = Vec::<String>::new;
    assert_eq!(
        rounds,
        [
            vec![
                format!("LOST {hand} — its work is claimable: scratch/a"),
                // What changed since its first PULL: one running task fewer, one orphaned.
                "PULL executing=1 (was 2) in_progress_orphaned=1 (was 0)".to_string(),
                "in_progress_orphaned p2 scratch/a — Jump departure".to_string(),
            ],
            none(),
            // Nobody's work blocking is heard; the older dispatcher does not follow it.
            vec!["STATUS scratch/free in_progress → blocked".to_string()],
            // NEW scratch/a/next, its PULL row and the reviewer's ROLE: the older one said them.
            none(),
            vec![format!("ROLE scratch/a {joined} reviewer, in_progress")],
            none(),
            none(),
        ],
        "never kid, the other's work, or a line twice"
    );
}
