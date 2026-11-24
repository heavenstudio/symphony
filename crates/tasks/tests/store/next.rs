//! `next`: the best work for a session.

use crate::common::{Fixture, GONE, Seed, refused};
use symphony_board::cli::pull::digest;
use symphony_board::health::RESERVE_SECONDS;
use symphony_board::red::FILER;
use symphony_board::{NewSession, NewTask};

fn task(slug: &str, priority: i64) -> NewTask {
    NewTask {
        priority,
        ..NewTask::new(slug, "Jump departure", "owner")
    }
}

fn next(f: &Fixture, handle: &str) -> Option<String> {
    f.store.next_for(handle).unwrap().map(|t| t.slug)
}

#[test]
fn next_prefers_priority_then_affinity_and_skips_others_reservations() {
    let f = Fixture::new(Seed::default());
    let a = f.session_with(NewSession {
        affinity: vec!["view".into()],
        ..NewSession::new("worker")
    });
    let b = f.session("worker");
    f.task_with(task("sim/plain", 2));
    f.task_with(task("view/hud", 2));
    f.task_with(task("sim/urgent", 0));
    f.store.reserve("sim/urgent", &b, RESERVE_SECONDS).unwrap();
    assert_eq!(next(&f, &a).as_deref(), Some("view/hud"));
    assert_eq!(next(&f, &b).as_deref(), Some("sim/urgent"));
}

#[test]
fn next_prefers_the_area_a_session_already_works_in() {
    let f = Fixture::new(Seed::default());
    let a = f.session("worker");
    f.task("sim/one");
    f.task("render/two");
    f.task("render/three");
    f.store.claim("render/two", &a).unwrap();
    assert_eq!(next(&f, &a).as_deref(), Some("render/three"));
}

#[test]
fn next_offers_concept_and_design_work_too() {
    let f = Fixture::new(Seed::default());
    let a = f.session("worker");
    f.task_with(NewTask {
        status: "design".into(),
        ..task("docs/lamp-brief", 2)
    });
    f.task_with(NewTask {
        status: "backlog".into(),
        ..task("sim/later", 2)
    });
    assert_eq!(next(&f, &a).as_deref(), Some("docs/lamp-brief"));
}

#[test]
fn next_skips_claimed_work_and_work_waiting_on_another() {
    let f = Fixture::new(Seed::default());
    let (a, b) = (f.session("worker"), f.session("worker"));
    f.task("sim/one");
    f.task_with(NewTask {
        depends: vec!["sim/one".into()],
        ..task("sim/two", 2)
    });
    f.store.claim("sim/one", &a).unwrap();
    assert_eq!(next(&f, &b), None);
}

#[test]
fn next_breaks_a_tie_by_the_order_made() {
    let f = Fixture::new(Seed::default());
    let a = f.session("worker");
    f.task("sim/zeta");
    f.task("sim/alpha");
    assert_eq!(next(&f, &a).as_deref(), Some("sim/zeta"));
}

#[test]
fn a_red_is_offered_before_other_work_of_its_priority_and_never_before_a_more_urgent_one() {
    let f = Fixture::new(Seed::default());
    let a = f.session_with(NewSession {
        affinity: vec!["sim".into()],
        ..NewSession::new("worker")
    });
    let filer = f.session(FILER);
    // Older, and in the session's own area: what it would be offered without the red.
    f.task_with(task("sim/urgent", 0));
    f.task_with(NewTask {
        priority: 0,
        red: Some("nightly".into()),
        ..NewTask::new("tests/nightly-red-verify", "Nightly red", &filer)
    });
    // Named like a red, but filed as ordinary work: the mark tells, never the slug.
    f.task_with(task("tests/nightly-red-lookalike", 0));
    assert_eq!(next(&f, &a).as_deref(), Some("tests/nightly-red-verify"));
    f.store
        .claim("tests/nightly-red-verify", &f.session("worker"))
        .unwrap();
    assert_eq!(
        next(&f, &a).as_deref(),
        Some("sim/urgent"),
        "the lookalike is ordinary work, behind the session's own area"
    );

    // A red its filer left less urgent waits behind more urgent work.
    let f = Fixture::new(Seed::default());
    let a = f.session("worker");
    let filer = f.session(FILER);
    f.task_with(NewTask {
        priority: 1,
        red: Some("nightly".into()),
        ..NewTask::new("tests/nightly-red-lint", "Nightly red", &filer)
    });
    f.task_with(task("sim/urgent", 0));
    assert_eq!(next(&f, &a).as_deref(), Some("sim/urgent"));

    // Work reserved for the session goes first, a less urgent one than a red too.
    f.task_with(task("sim/booked", 3));
    f.store.reserve("sim/booked", &a, RESERVE_SECONDS).unwrap();
    f.task_with(NewTask {
        priority: 0,
        red: Some("nightly".into()),
        ..NewTask::new("tests/nightly-red-size", "Nightly red", &filer)
    });
    assert_eq!(next(&f, &a).as_deref(), Some("sim/booked"));
}

#[test]
fn next_for_an_unknown_session_is_refused() {
    let f = Fixture::new(Seed::default());
    assert_eq!(
        refused(f.store.next_for("ghost-owl")),
        "no session 'ghost-owl'"
    );
}

#[test]
fn a_cancelled_dependency_blocks_with_a_reason_instead_of_disappearing_from_pull() {
    for actor in ["owner", "amber-heron"] {
        let mut seed = Seed::default()
            .session("amber-heron")
            .task("sim/prerequisite", "ready")
            .task("sim/second-prerequisite", "ready")
            .task("sim/old-prerequisite", "cancelled")
            .task("sim/legacy", "ready")
            .link("sim/legacy", "dep", "sim/old-prerequisite")
            .task("sim/free", "ready");
        for status in symphony_board::STATUSES {
            let slug = format!("sim/{status}").replace('_', "-");
            seed = seed
                .task(&slug, status)
                .link(&slug, "dep", "sim/prerequisite")
                .link(&slug, "dep", "sim/second-prerequisite");
        }
        let f = Fixture::new(seed);
        f.store
            .move_to("sim/prerequisite", "cancelled", actor)
            .unwrap();
        for status in symphony_board::STATUSES {
            let slug = format!("sim/{status}").replace('_', "-");
            let terminal = matches!(status, "done" | "cancelled");
            assert_eq!(
                f.store.get(&slug).unwrap().status,
                if terminal { status } else { "blocked" },
                "{actor} cancellation, dependent was {status}"
            );
            if !terminal {
                assert_eq!(
                    f.said(&slug),
                    [format!(
                        "blocked: cancelled dependencies: sim/prerequisite (was {status})"
                    )]
                );
            }
        }
        f.store
            .move_to("sim/second-prerequisite", "cancelled", actor)
            .unwrap();
        for status in symphony_board::STATUSES {
            let slug = format!("sim/{status}").replace('_', "-");
            let expected = if matches!(status, "done" | "cancelled") {
                Vec::<String>::new()
            } else {
                vec![
                    format!("blocked: cancelled dependencies: sim/prerequisite (was {status})"),
                    "blocked: cancelled dependencies: sim/second-prerequisite (was blocked)".into(),
                ]
            };
            assert_eq!(f.said(&slug), expected, "{actor}, dependent was {status}");
        }
        let later = f.task_with(NewTask {
            depends: vec!["sim/prerequisite".into(), "sim/old-prerequisite".into()],
            ..task("sim/later", 0)
        });
        assert_eq!(later.status, "blocked");
        assert_eq!(
            f.said("sim/later").last().unwrap(),
            "blocked: cancelled dependencies: sim/old-prerequisite, sim/prerequisite (was ready)"
        );
        f.store
            .take_role("sim/later", "amber-heron", "orchestrator")
            .unwrap();
        assert_eq!(
            f.store
                .move_to("sim/later", "ready", "amber-heron")
                .unwrap()
                .status,
            "blocked",
            "moving the task cannot satisfy its cancelled prerequisite"
        );
        assert_eq!(next(&f, "amber-heron").as_deref(), Some("sim/free"));
        let pull = digest(&f.store, &[], None, f.now(), &[]).unwrap();
        assert_eq!(
            pull.items("ready_unstaffed")
                .iter()
                .map(|t| t.slug.as_str())
                .collect::<Vec<_>>(),
            ["sim/free"]
        );
        assert_eq!(pull.count("dependency_blocked"), 9);
        assert_eq!(
            pull.notes["sim/legacy"],
            "cancelled dependencies: sim/old-prerequisite"
        );
        assert!(
            pull.lines()
                .iter()
                .any(|line| line.contains("dependency_blocked p0 sim/later")
                    && line.contains(
                        "cancelled dependencies: sim/old-prerequisite, sim/prerequisite"
                    ))
        );
        assert!(refused(f.store.claim("sim/later", "amber-heron")).contains("is blocked"));
    }
}

#[test]
fn next_offers_work_whose_executor_or_reserver_is_gone() {
    let f = Fixture::new(Seed::default());
    let (gone, other) = (f.session("worker"), f.session("worker"));
    f.task_with(NewTask {
        status: "design".into(),
        ..task("sim/held", 1)
    });
    f.task_with(task("sim/booked", 0));
    f.store.claim("sim/held", &gone).unwrap(); // design work keeps its stage, so it is still pullable
    f.store.reserve("sim/booked", &gone, 10.0 * GONE).unwrap();
    assert_eq!(next(&f, &other), None);
    f.advance(GONE);
    f.store.touch(&other).unwrap();
    assert_eq!(next(&f, &other).as_deref(), Some("sim/booked"));
    f.store.claim("sim/booked", &other).unwrap();
    assert_eq!(next(&f, &other).as_deref(), Some("sim/held"));
}

#[test]
fn a_subtask_made_for_a_handle_is_offered_to_it_alone() {
    let f = Fixture::new(Seed::default());
    let (agent, other) = (f.session("worker"), f.session("worker"));
    f.task("a");
    f.task_with(NewTask {
        parent: Some("a".into()),
        assignee: Some(agent.clone()),
        ..task("a/review", 3)
    });
    f.advance(RESERVE_SECONDS * 3.0);
    f.store.touch(&agent).unwrap();
    f.store.touch(&other).unwrap();
    assert_eq!(next(&f, &agent).as_deref(), Some("a/review"));
    assert_eq!(next(&f, &other).as_deref(), Some("a"));
}
