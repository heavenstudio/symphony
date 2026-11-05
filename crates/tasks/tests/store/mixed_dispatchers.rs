//! Dispatchers built before one orchestrator per task and after it, alternating on one
//! orchestrator's callback state: the fields older ones read keep their whole-board meaning, and
//! neither repeats a line the other said.

use super::common::Fixture;
use super::orchestrators::{go_quiet_but, heard};
use super::quiet_callbacks::listen;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use symphony_board::health::{EXECUTING, FINISHED};
use symphony_board::{Health, NewSession, NewTask, Task};

/// What a dispatcher built before one orchestrator per task records of `handle` once it has looked
/// and told: its look of the whole board (every ready task, every task below what it orchestrates
/// and the whole board's staffing; its children by parent), every open task's status, and every
/// entry of the whole digest as delivered. Reckoned here by those rules, from the board as it is.
fn older_record(f: &Fixture, handle: &str) -> Value {
    let board = f.store.board().unwrap();
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
        for child in (board.tasks.iter()).filter(|t| t.parent.as_deref() == Some(slug.as_str())) {
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
    let digest = symphony_board::cli::pull::digest(&f.store, &[], None, f.now(), &[]).unwrap();
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
fn older_fields(f: &Fixture, handle: &str) -> Value {
    let state = f
        .store
        .client()
        .query(
            "RETURN SELECT seen, statuses, pull_entries FROM ONLY type::record('callback_state', $handle);",
            json!({"handle":handle}),
        )
        .unwrap()
        .remove(0);
    let entries: BTreeSet<(String, String)> =
        serde_json::from_value(state["pull_entries"].clone()).unwrap();
    json!({"seen": state["seen"], "statuses": state["statuses"], "pull_entries": entries})
}

/// An older dispatcher's look and write: what it told is in what it records.
fn written_by_an_older_dispatcher(f: &Fixture, handle: &str) {
    f.store
        .client()
        .query(
            "UPDATE type::record('callback_state', $handle) SET seen = $record.seen, statuses = $record.statuses,
                pull_entries = $record.pull_entries, pull_queued = NONE RETURN NONE;",
            json!({"handle":handle, "record":older_record(f, handle)}),
        )
        .unwrap();
}

#[test]
fn an_older_dispatcher_between_newer_ones_repeats_no_line_and_widens_none() {
    let f = Fixture::empty();
    let (a, b) = (f.session("orchestrator"), f.session("orchestrator"));
    let child = |kind: &str, parent: &str| {
        f.session_with(NewSession {
            parent: Some(parent.to_string()),
            ..NewSession::new(kind)
        })
    };
    // kid is a's, reviewing b's work; hand is b's, executing a's; w works on nobody's.
    let (kid, hand) = (child("reviewer", &a), child("worker", &b));
    let w = f.session("worker");
    let sub = |slug: &str, parent: &str| {
        f.task_with(NewTask {
            parent: Some(parent.into()),
            ..NewTask::new(slug, "Jump departure", "owner")
        })
    };
    f.task("scratch/a");
    f.store.take_role("scratch/a", &a, "orchestrator").unwrap();
    f.store.claim("scratch/a", &hand).unwrap();
    sub("scratch/a/b", "scratch/a");
    f.store
        .take_role("scratch/a/b", &b, "orchestrator")
        .unwrap();
    f.task("scratch/b");
    f.store.take_role("scratch/b", &b, "orchestrator").unwrap();
    sub("scratch/b/1", "scratch/b");
    sub("scratch/b/rev", "scratch/b");
    f.store.claim("scratch/b/rev", &w).unwrap();
    f.store.move_to("scratch/b/rev", "review", &w).unwrap();
    f.store
        .take_role("scratch/b/rev", &kid, "reviewer")
        .unwrap();
    f.task("scratch/free");
    f.store.claim("scratch/free", &w).unwrap();
    let la = listen(&f, &a);
    let ears = [(&la, a.as_str())];
    let pull = |executing: u8, orphaned: u8| {
        format!(
            "PULL executing={executing} ready_unstaffed=0 in_progress_orphaned={orphaned} review_unreviewed=0 landed_open=0 idle=0"
        )
    };
    assert_eq!(
        heard(&f, &w, &ears),
        [vec![pull(2, 0)]],
        "b's ready and unreviewed work is b's"
    );
    // The newer dispatcher looks first as kid and hand go quiet: a hears of hand only.
    go_quiet_but(&f, &[&a, &b, &w]);
    let mut rounds = vec![heard(&f, &w, &ears).remove(0)];
    let mut joined = String::new();
    // Writes on another host, which leave this host's recipient to the next dispatch here.
    let elsewhere = f.elsewhere();
    for round in 1..=6 {
        assert_eq!(
            older_fields(&f, &a),
            older_record(&f, &a),
            "the fields older dispatchers read keep their meaning, round {round}"
        );
        // An older dispatcher looks first, and what it says is its own: this listener hears the
        // newer dispatcher alone.
        if round == 2 {
            elsewhere.move_to("scratch/free", "blocked", &w).unwrap();
        } else if round == 3 {
            let next = NewTask {
                parent: Some("scratch/a".into()),
                ..NewTask::new("scratch/a/next", "Jump departure", "owner")
            };
            elsewhere.create(&next).unwrap();
            let r2 = elsewhere.register(&NewSession::new("reviewer")).unwrap();
            (elsewhere.take_role("scratch/a", &r2.row.handle, "reviewer")).unwrap();
        }
        written_by_an_older_dispatcher(&f, &a);
        if round == 4 {
            // The newer dispatcher looks first. b's new row in the whole digest is recorded as
            // withheld, so an older dispatcher never sends it.
            sub("scratch/b/2", "scratch/b");
            joined = f.session("reviewer");
            f.store.take_role("scratch/a", &joined, "reviewer").unwrap();
        }
        rounds.push(heard(&f, &w, &ears).remove(0));
    }
    let none = Vec::<String>::new;
    assert_eq!(
        rounds,
        [
            vec![
                format!("LOST {hand} — its work is claimable: scratch/a"),
                pull(1, 1),
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
        "never kid, b's work, or a line twice"
    );
}
