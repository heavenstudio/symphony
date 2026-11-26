//! A read that shows tasks returns only the sessions on them (`Sessions::Shown`), and says of
//! every task what a read of the whole board says: the same roles at the same health, whatever
//! the health and whoever else the board has seen.

use crate::common::{Fixture, GONE, HOST, NOW, Seed, session};
use serde_json::json;
use std::collections::{BTreeSet, HashMap};
use symphony_board::health::LIVE_SECONDS;
use symphony_board::rows::SessionRow;
use symphony_board::{Health, Task};

const DOC: &str = "docs/SHOWN.md";

fn seen(handle: &str, ago: f64) -> SessionRow {
    SessionRow {
        connected: NOW - ago,
        last_seen: NOW - ago,
        ..session(handle)
    }
}

/// A board holding a session of every health on some task, a role nobody holds a session for, an
/// orchestrator holding only the parent above the open work, and sessions on no task at all.
fn board() -> Fixture {
    let mut seed = Seed::default()
        .session_row(SessionRow {
            kind: "orchestrator".into(),
            ..session("above")
        })
        .session_row(SessionRow {
            kind: "orchestrator".into(),
            ..seen("above-gone", GONE)
        })
        .session("live")
        .session_row(seen("stale", LIVE_SECONDS + 1.0))
        .session_row(seen("lost", GONE))
        .session_row(seen("gone", GONE))
        .session_row(SessionRow {
            left_at: Some(NOW - 5.0),
            ..session("left")
        })
        .session_row(seen("unknown", 500.0))
        .session_row(SessionRow {
            pid: Some(41),
            host: HOST.into(),
            ..session("dead-pid")
        })
        .session_row(SessionRow {
            busy_until: Some(NOW + 60.0),
            ..seen("busy", GONE)
        })
        .session_row(SessionRow {
            affinity: "w".into(),
            ..session("asker")
        })
        .task("w", "backlog")
        .role("w", "above", "orchestrator")
        .role("w", "above-gone", "orchestrator");
    for n in 0..20 {
        seed = seed.session_row(seen(&format!("bystander-{n}"), n as f64 * 60.0));
    }
    for (slug, status, holder, role) in [
        ("w/live", "in_progress", "live", "executor"),
        ("w/stale", "in_progress", "stale", "executor"),
        ("w/lost", "in_progress", "lost", "executor"),
        ("w/done", "done", "gone", "executor"),
        ("w/review", "review", "left", "reviewer"),
        ("w/unknown", "in_progress", "unknown", "executor"),
        ("w/missing", "in_progress", "nobody-here", "executor"),
        ("w/pid", "ready", "dead-pid", "executor"),
        ("w/busy", "design", "busy", "executor"),
    ] {
        let mut row = crate::common::task(slug, status);
        row.id = seed.dump.tasks.len() as i64 + 1;
        row.parent = Some("w".into());
        seed = seed.task_row(row).role(slug, holder, role);
    }
    let mut free = crate::common::task("w/free", "ready");
    free.id = seed.dump.tasks.len() as i64 + 1;
    free.parent = Some("w".into());
    let mut deep = crate::common::task("w/live/deep", "concept");
    deep.id = free.id + 1;
    deep.parent = Some("w/live".into());
    let seed = seed
        .task_row(free)
        .task_row(deep)
        .reserved("w/live/deep", "gone", Some(NOW + 600.0))
        .reserved("w/free", "live", Some(NOW - 1.0))
        .reserved("w/pid", "stale", Some(NOW + 600.0))
        .role("w/review", "live", "executor")
        .link("w/lost", "doc", DOC)
        .link("w/missing", "doc", &format!("{DOC}#Heading"))
        .link("w/busy", "dep", "w/live");
    let f = Fixture::new(seed);
    // A native registration whose sweep is overdue: its liveness is unknown.
    (f.store.raw().row("callback_state", "unknown"))
        .put(
            json!({"handle": "unknown", "address": "codex:abcdef01", "host": HOST,
            "confirmed_until": NOW - 1.0, "lease": NOW + 1000.0}),
        )
        .unwrap();
    f
}

/// Every task as the whole board's read gives it, by slug.
fn whole(f: &Fixture) -> HashMap<String, Task> {
    let board = f.store.board().unwrap();
    assert!(board.sessions.len() > 30, "the whole read is every session");
    (board.tasks.into_iter())
        .map(|t| (t.slug.clone(), t))
        .collect()
}

/// `shown`, a scoped read's tasks, are `slugs` in that order, each as the whole read has it.
fn same(whole: &HashMap<String, Task>, shown: &[Task], slugs: &[&str], read: &str) {
    let got: Vec<&str> = shown.iter().map(|t| t.slug.as_str()).collect();
    assert_eq!(got, slugs, "{read}");
    for task in shown {
        assert_eq!(task, &whole[&task.slug], "{read}: {}", task.slug);
    }
}

#[test]
fn a_read_of_the_sessions_shown_gives_each_task_as_the_whole_board_does_at_every_health() {
    let f = board();
    let whole = whole(&f);
    let health = |slug: &str, handle: &str| {
        let role = (whole[slug].roles.iter()).find(|r| r.handle == handle);
        role.unwrap_or_else(|| panic!("{handle} holds a role on {slug}"))
            .health
    };
    // What the board is made to hold: every health, and a holder the board has no session of.
    for (slug, handle, want) in [
        ("w", "above", Health::Live),
        ("w", "above-gone", Health::Left),
        ("w/live", "live", Health::Live),
        ("w/stale", "stale", Health::Stale),
        ("w/lost", "lost", Health::Lost),
        ("w/done", "gone", Health::Left),
        ("w/review", "left", Health::Left),
        ("w/unknown", "unknown", Health::Unknown),
        ("w/missing", "nobody-here", Health::Lost),
        ("w/pid", "dead-pid", Health::Stale),
        ("w/busy", "busy", Health::Live),
        ("w/live/deep", "gone", Health::Left),
    ] {
        assert_eq!(health(slug, handle), want, "{handle} on {slug}");
    }
    let every: Vec<&str> = vec![
        "w",
        "w/live",
        "w/stale",
        "w/lost",
        "w/done",
        "w/review",
        "w/unknown",
        "w/missing",
        "w/pid",
        "w/busy",
        "w/free",
        "w/live/deep",
    ];
    for slug in &every {
        same(&whole, &[f.store.get(slug).unwrap()], &[slug], "get");
    }
    same(&whole, &f.store.tasks(None).unwrap(), &every, "tasks");
    let statuses: BTreeSet<&str> = whole.values().map(|t| t.status.as_str()).collect();
    for status in statuses {
        let slugs: Vec<&str> = (every.iter().copied())
            .filter(|slug| whole[*slug].status == status)
            .collect();
        let shown = f.store.tasks(Some(status)).unwrap();
        same(&whole, &shown, &slugs, status);
    }
    let under = &every[1..11];
    same(&whole, &f.store.children("w").unwrap(), under, "children");
    let below = f.store.descendants("w").unwrap();
    same(&whole, &below, &every[1..], "descendants");
    let linked = f.store.linked_to(DOC).unwrap();
    same(&whole, &linked, &["w/lost", "w/missing"], "linked_to");
}

/// `next` reads the session asking beside the roles' holders: one that holds nothing is still
/// known, with its areas, and one the board never saw is refused as before.
#[test]
fn next_reads_the_session_asking_though_it_holds_nothing() {
    let f = board();
    let whole = whole(&f);
    // The lapsed reservation leaves w/free to anyone; w/live/deep's reserver is gone.
    let next = f.store.next_for("asker").unwrap().expect("work is free");
    assert_eq!(next, whole[&next.slug]);
    assert_eq!(next.slug, "w/free");
    let said = crate::common::refused(f.store.next_for("nobody-here"));
    assert!(said.contains("no session 'nobody-here'"), "{said}");
}

/// A write answers with its task read after it, by the sessions on it alone, and that answer is
/// the task as the whole board has it once the write is done.
#[test]
fn a_writes_answer_is_its_task_as_the_whole_board_has_it_after() {
    let f = board();
    let claimed = f.store.claim("w/free", "asker").unwrap();
    assert_eq!(claimed, whole(&f)["w/free"]);
    assert_eq!(claimed.roles.len(), 1, "the lapsed reservation is gone");
    // A claim over a lost holder decides on the whole read, and answers by the scoped one.
    let taken = f.store.claim("w/lost", "live").unwrap();
    assert_eq!(taken, whole(&f)["w/lost"]);
    let said = crate::common::refused(f.store.claim("w/stale", "asker"));
    assert!(said.contains("stale"), "{said}");
    let moved = f.store.move_to("w/free", "blocked", "asker").unwrap();
    assert_eq!(moved, whole(&f)["w/free"]);
}
