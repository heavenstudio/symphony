//! `change_depends`: what a task waits on after it was made, and leaving a cancellation's block.

use crate::common::{Fixture, NOW, Seed, refused};
use symphony_board::rows::MessageRow;

const HANDLE: &str = "amber-heron";

fn change(f: &Fixture, slug: &str, add: &[&str], remove: &[&str]) -> symphony_board::Task {
    f.store
        .change_depends(slug, &owned(add), &owned(remove), HANDLE)
        .unwrap()
}

fn owned(slugs: &[&str]) -> Vec<String> {
    slugs.iter().map(|s| (*s).to_string()).collect()
}

/// `sim/old` and `sim/new`, ready, and `sim/dep` in `status` waiting on `sim/old`.
fn superseded(status: &str) -> Seed {
    Seed::default()
        .session(HANDLE)
        .task("sim/old", "ready")
        .task("sim/new", "ready")
        .task("sim/dep", status)
        .link("sim/dep", "dep", "sim/old")
}

#[test]
fn a_dependent_follows_the_successor_and_goes_back_to_the_status_it_held() {
    for (status, executor, back) in [
        ("backlog", false, "backlog"),
        ("concept", false, "concept"),
        ("design", false, "design"),
        ("ready", false, "ready"),
        ("in_progress", true, "in_progress"),
        ("in_progress", false, "ready"),
        ("review", false, "review"),
    ] {
        let mut seed = superseded(status).session("brisk-finch");
        if executor {
            seed = seed.role("sim/dep", "brisk-finch", "executor");
        }
        let f = Fixture::new(seed);
        f.store.move_to("sim/old", "cancelled", "owner").unwrap();
        assert_eq!(f.store.get("sim/dep").unwrap().status, "blocked");
        let t = change(&f, "sim/dep", &["sim/new"], &["sim/old"]);
        assert_eq!(
            (t.status.as_str(), t.depends.as_slice()),
            (back, ["sim/new".to_string()].as_slice()),
            "dependent was {status}, executor {executor}"
        );
        assert_eq!(
            f.said("sim/dep"),
            [
                format!("blocked: cancelled dependencies: sim/old (was {status})"),
                format!("{HANDLE} changed its dependencies: removed sim/old; added sim/new"),
                format!("unblocked: no cancelled dependency left; back to {back}"),
            ]
        );
    }
}

#[test]
fn a_second_cancelled_prerequisite_keeps_it_blocked_until_it_goes_too() {
    let f = Fixture::new(
        superseded("review")
            .task("sim/other", "ready")
            .task("sim/other-new", "done")
            .link("sim/dep", "dep", "sim/other"),
    );
    f.store.move_to("sim/old", "cancelled", "owner").unwrap();
    f.store.move_to("sim/other", "cancelled", "owner").unwrap();
    let t = change(&f, "sim/dep", &["sim/new"], &["sim/old"]);
    assert_eq!(
        (t.status.as_str(), t.depends.as_slice()),
        (
            "blocked",
            ["sim/new".to_string(), "sim/other".to_string()].as_slice()
        )
    );
    assert!(
        !f.said("sim/dep").iter().any(|l| l.starts_with("unblocked")),
        "{:?}",
        f.said("sim/dep")
    );
    let t = change(&f, "sim/dep", &["sim/other-new"], &["sim/other"]);
    assert_eq!(t.status, "review", "the status before the first block");
    assert_eq!(
        f.said("sim/dep"),
        [
            "blocked: cancelled dependencies: sim/old (was review)",
            "blocked: cancelled dependencies: sim/other (was blocked)",
            "amber-heron changed its dependencies: removed sim/old; added sim/new",
            "amber-heron changed its dependencies: removed sim/other; added sim/other-new",
            "unblocked: no cancelled dependency left; back to review",
        ]
    );
}

#[test]
fn work_blocked_by_hand_stays_blocked() {
    let f = Fixture::new(superseded("ready").role("sim/dep", HANDLE, "orchestrator"));
    f.store.move_to("sim/dep", "blocked", HANDLE).unwrap();
    f.store.move_to("sim/old", "cancelled", "owner").unwrap();
    assert_eq!(
        f.said("sim/dep").last().unwrap(),
        "blocked: cancelled dependencies: sim/old (was blocked)"
    );
    let t = change(&f, "sim/dep", &["sim/new"], &["sim/old"]);
    assert_eq!(t.status, "blocked");
    let lines = f.said("sim/dep");
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(
        lines.last().unwrap(),
        "amber-heron changed its dependencies: removed sim/old; added sim/new"
    );
}

/// `sim/dep` blocked by `sim/old`'s cancellation before the board recorded the status it held.
fn blocked_before_the_record(executor: bool) -> Fixture {
    let mut seed = superseded("blocked").session("brisk-finch");
    seed.dump.tasks[0].status = "cancelled".into();
    seed.dump.messages.push(MessageRow {
        id: 1,
        slug: "sim/dep".into(),
        sender: "owner".into(),
        recipient: None,
        kind: "system".into(),
        text: "blocked: cancelled dependencies: sim/old".into(),
        ts: NOW,
        person: false,
    });
    if executor {
        seed = seed.role("sim/dep", "brisk-finch", "executor");
    }
    Fixture::new(seed)
}

#[test]
fn a_block_line_that_recorded_no_status_sends_it_to_ready_or_to_its_executor() {
    for (executor, back) in [(false, "ready"), (true, "in_progress")] {
        let f = blocked_before_the_record(executor);
        let t = change(&f, "sim/dep", &["sim/new"], &["sim/old"]);
        assert_eq!(t.status, back);
        assert_eq!(
            f.said("sim/dep").last().unwrap(),
            &format!(
                "unblocked: no cancelled dependency left; back to {back} (its block line recorded no earlier status)"
            )
        );
    }
}

#[test]
fn adding_or_removing_alone_changes_only_what_it_waits_on() {
    let f = Fixture::new(superseded("ready"));
    let t = change(&f, "sim/dep", &["sim/new"], &[]);
    assert_eq!(
        (t.status.as_str(), t.depends.as_slice()),
        (
            "ready",
            ["sim/new".to_string(), "sim/old".to_string()].as_slice()
        )
    );
    let t = f
        .store
        .change_depends("sim/dep", &[], &owned(&["sim/old", "sim/new"]), "owner")
        .unwrap();
    assert!(t.depends.is_empty());
    assert_eq!(
        f.said("sim/dep"),
        [
            "amber-heron changed its dependencies: added sim/new",
            "owner changed its dependencies: removed sim/new, sim/old",
        ]
    );
}

#[test]
fn unknown_tasks_cycles_and_changes_that_change_nothing_are_refused() {
    let f = Fixture::new(
        superseded("ready")
            .task("sim/top", "ready")
            .task("sim/gone", "cancelled")
            .task("sim/finished", "done")
            .link("sim/top", "dep", "sim/dep")
            .link("sim/new", "dep", "sim/top"),
    );
    let refusal = |slug: &str, add: &[&str], remove: &[&str]| {
        refused(
            f.store
                .change_depends(slug, &owned(add), &owned(remove), HANDLE),
        )
    };
    let cases: [(&str, &[&str], &[&str], &str); 11] = [
        ("sim/nope", &["sim/new"], &[], "no task 'sim/nope'"),
        ("sim/dep", &["sim/nope"], &[], "no task 'sim/nope'"),
        (
            "sim/dep",
            &[],
            &["sim/new"],
            "sim/dep does not depend on sim/new",
        ),
        (
            "sim/dep",
            &["sim/old"],
            &[],
            "sim/dep already depends on sim/old",
        ),
        (
            "sim/dep",
            &["sim/gone"],
            &[],
            "sim/gone is cancelled: nothing waiting on it can start",
        ),
        (
            "sim/dep",
            &["sim/dep"],
            &[],
            "sim/dep cannot wait on itself",
        ),
        (
            "sim/dep",
            &["sim/new"],
            &[],
            "sim/dep cannot wait on sim/new: sim/new already waits on sim/dep",
        ),
        (
            "sim/dep",
            &["sim/old"],
            &["sim/old"],
            "sim/old cannot be both added and removed",
        ),
        (
            "sim/dep",
            &[],
            &[],
            "name a dependency to add, remove or replace",
        ),
        ("sim/finished", &["sim/new"], &[], "sim/finished is done"),
        ("sim/gone", &["sim/new"], &[], "sim/gone is cancelled"),
    ];
    for (slug, add, remove, why) in cases {
        assert_eq!(
            refusal(slug, add, remove),
            why,
            "{slug} +{add:?} -{remove:?}"
        );
    }
    assert_eq!(
        refused(
            f.store
                .change_depends("sim/dep", &owned(&["sim/finished"]), &[], "nobody-here")
        ),
        f.no_session("nobody-here")
    );
    assert_eq!(f.store.get("sim/dep").unwrap().depends, ["sim/old"]);
    assert!(f.said("sim/dep").is_empty());
}

#[test]
fn a_dependency_changed_meanwhile_is_read_again_before_a_loop_is_made() {
    let f = Fixture::new(
        Seed::default()
            .session(HANDLE)
            .task("sim/a", "ready")
            .task("sim/b", "ready"),
    );
    let other = f.other();
    let late = f.late(move || {
        other
            .change_depends("sim/b", &owned(&["sim/a"]), &[], HANDLE)
            .unwrap();
    });
    assert_eq!(
        refused(late.change_depends("sim/a", &owned(&["sim/b"]), &[], HANDLE)),
        "sim/a cannot wait on sim/b: sim/b already waits on sim/a"
    );
    assert!(f.store.get("sim/a").unwrap().depends.is_empty());
}

#[test]
fn a_prerequisite_cancelled_meanwhile_keeps_the_dependent_blocked() {
    let f = Fixture::new(superseded("ready"));
    f.store.move_to("sim/old", "cancelled", "owner").unwrap();
    let other = f.other();
    let late = f.late(move || {
        other.move_to("sim/new", "cancelled", "owner").unwrap();
    });
    assert_eq!(
        refused(late.change_depends(
            "sim/dep",
            &owned(&["sim/new"]),
            &owned(&["sim/old"]),
            HANDLE
        )),
        "sim/new is cancelled: nothing waiting on it can start"
    );
    let t = f.store.get("sim/dep").unwrap();
    assert_eq!(
        (t.status.as_str(), t.depends.as_slice()),
        ("blocked", ["sim/old".to_string()].as_slice())
    );
}
