//! What a land requires of a task at its tip, and the refusal naming what it lacks and who gives it.

use super::*;
use crate::cli::machine_work::recorded::{Phase, Step};
use serde_json::json;
use std::collections::BTreeMap;

const SLUG: &str = "tools/lamp";
const TIP: &str = "4be91c2aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER: &str = "7c0ffee1bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn task(fields: Vec<(&str, serde_json::Value)>) -> Task {
    Task {
        branch: None,
        id: 1,
        slug: SLUG.into(),
        title: "A lamp".into(),
        body: String::new(),
        status: "review".into(),
        priority: 2,
        parent: None,
        created_by: "owner".into(),
        created: 0.0,
        updated: 0.0,
        links: Vec::new(),
        depends: Vec::new(),
        roles: Vec::new(),
        instruction: false,
        fields: BTreeMap::from([(
            crate::red::FIELDS.to_string(),
            (fields.into_iter())
                .map(|(name, value)| (name.to_string(), value))
                .collect(),
        )]),
    }
}

fn assessed(stamps: &[&str], tip: Option<&str>) -> serde_json::Value {
    json!({"stamps": stamps, "tip": tip, "by": "slate-owl", "why": "a store query and its test"})
}

/// A folder outside every repository: the catalogue there is the Symphony's own.
fn nowhere() -> std::path::PathBuf {
    std::env::temp_dir()
}

fn needed(stamps: &[&str], tip: Option<&str>) -> Needed {
    let task = task(vec![(crate::stamps::NEEDS, assessed(stamps, tip))]);
    Needed::of(&task, &nowhere(), Some(TIP)).unwrap()
}

#[test]
fn a_task_needs_its_assessment_even_with_a_legacy_mark_and_the_defaults_with_none() {
    let unassessed = Needed::of(&task(Vec::new()), &nowhere(), Some(TIP)).unwrap();
    assert_eq!(unassessed.stamps, ["review"]);
    assert!(unassessed.review() && unassessed.needs.is_none());
    assert!(
        !unassessed.told(),
        "a land says nothing new of a task as before stamps"
    );
    assert_eq!(unassessed.stale(TIP), None);
    let tests = needed(&["tests"], Some(TIP));
    assert_eq!(tests.stamps, ["tests"]);
    assert!(!tests.review() && tests.told());
    let none = needed(&[], Some(TIP));
    assert!(none.stamps.is_empty() && none.told());
    // On another tip, or from the brief, the defaults stand beside what it names.
    assert_eq!(needed(&["tests"], Some(OTHER)).stamps, ["tests", "review"]);
    assert_eq!(needed(&[], None).stamps, ["review"]);
    // A legacy mark never bypasses the assessment.
    let fast = task(vec![
        ("fast_fix", json!("a one-line fix")),
        (crate::stamps::NEEDS, assessed(&["tests"], Some(TIP))),
    ]);
    let fast = Needed::of(&fast, &nowhere(), Some(TIP)).unwrap();
    assert_eq!(fast.stamps, ["tests"]);
    assert!(fast.needs.is_some() && fast.told());
}

#[test]
fn an_assessment_of_another_tip_or_the_brief_is_stale_saying_whose_and_what_it_read() {
    assert_eq!(needed(&["tests"], Some(TIP)).stale(TIP), None);
    assert_eq!(
        needed(&["tests"], Some(OTHER)).stale(TIP).unwrap(),
        "its assessment by slate-owl read 7c0ffee, not the tip 4be91c2, so the catalogue's \
         defaults stand beside the stamps it names: an assessment lowers what a task needs only \
         on the tip it read"
    );
    assert!(
        (needed(&[], None).stale(TIP).unwrap())
            .starts_with("its assessment by slate-owl read the brief alone, not the tip 4be91c2,")
    );
}

#[test]
fn the_lands_line_says_what_it_needs_from_where_and_each_stamp_standing() {
    let met = [(
        "tests".to_string(),
        "Green at 4be91c2: `cargo test`".to_string(),
    )];
    assert_eq!(
        needed(&["tests"], Some(TIP)).line(SLUG, TIP, &met),
        "land tools/lamp: it needs tests at 4be91c2 (its assessment by slate-owl); tests: Green \
         at 4be91c2: `cargo test`"
    );
    assert_eq!(
        needed(&[], Some(TIP)).line(SLUG, TIP, &[]),
        "land tools/lamp: it needs none at 4be91c2 (its assessment by slate-owl)"
    );
    assert_eq!(
        needed(&["tests"], Some(OTHER)).line(SLUG, TIP, &[]),
        "land tools/lamp: it needs tests, review at 4be91c2 (its assessment by slate-owl at \
         7c0ffee and the catalogue's defaults)"
    );
    let unassessed = Needed::of(&task(Vec::new()), &nowhere(), Some(TIP)).unwrap();
    assert_eq!(
        unassessed.line(SLUG, TIP, &[]),
        "land tools/lamp: it needs review at 4be91c2 (the catalogue's defaults, no assessment)"
    );
}

#[test]
fn a_stamp_required_beside_the_needs_is_added_once() {
    let mut unassessed = Needed::of(&task(Vec::new()), &nowhere(), Some(TIP)).unwrap();
    unassessed.require(TESTS);
    unassessed.require(TESTS);
    assert_eq!(unassessed.stamps, ["review", "tests"]);
    assert!(unassessed.told(), "the land says what it needs");
    let mut named = needed(&["tests", "review"], Some(TIP));
    named.require(TESTS);
    assert_eq!(named.stamps, ["tests", "review"]);
}

#[test]
fn a_landing_by_its_needed_stamps_names_them_and_the_whole_tip_they_stood_on() {
    let stood = needed(&["tests"], Some(TIP)).stood(TIP);
    assert_eq!(stood, format!("; its needs (tests) stood at {TIP}"));
    assert_eq!(
        needed(&[], Some(TIP)).stood(TIP),
        format!("; its needs (none) stood at {TIP}")
    );
    let line = format!("landed {SLUG} on main at 0123456789ab{stood}; pushed to origin");
    assert_eq!(stood_at(&line), Some(TIP));
    for other in [
        "landed tools/lamp on main at 0123456789ab; pushed to origin",
        "approve: 4be91c2 reads right",
        "its needs (tests) stood at nothing",
    ] {
        assert_eq!(stood_at(other), None, "{other}");
    }
}

#[test]
fn a_task_needing_no_review_is_ready_in_review_blocked_or_done_and_told_to_move_otherwise() {
    for status in ["review", "blocked", "done"] {
        assert_eq!(not_ready(SLUG, status), None, "{status}");
    }
    assert_eq!(
        not_ready(SLUG, "in_progress"),
        Some((
            "tools/lamp is in_progress: it needs no review, so it lands once its executor says \
             the work is ready by moving it to review (`just tasks move tools/lamp review`)"
                .into(),
            Some("@executor")
        ))
    );
    assert_eq!(
        not_ready(SLUG, "cancelled"),
        Some(("tools/lamp is cancelled: nothing of it lands".into(), None))
    );
}

fn run(id: u8, label: &str, phase: Phase, exit: i32) -> Record {
    let started = 1000.0 * f64::from(id);
    Record {
        version: 1,
        operation_id: format!("{id:032x}"),
        parent_id: None,
        phase,
        label: label.into(),
        state: Step::Finished,
        handle: Some("amber".into()),
        task: Some(SLUG.into()),
        checkout: "/checkout".into(),
        branch: Some(SLUG.into()),
        commit: Some(TIP.into()),
        machine_id: None,
        host: "host".into(),
        pid: 10,
        child_pid: Some(11),
        queued_at: started - 5.0,
        started_at: Some(started),
        ended_at: Some(started + 60.0),
        beat_at: started,
        exit_code: Some(exit),
        signal: None,
        reason: String::new(),
        run: None,
        reading: None,
    }
}

fn evidence<'a>(thread: &'a [Message], runs: Vec<Record>) -> Evidence<'a> {
    Evidence {
        thread,
        runs,
        ours: vec![SLUG.into()],
        folder: crate::stamps::read::folder("artifacts", SLUG),
        main: std::path::PathBuf::from("/nowhere"),
    }
}

fn post(id: i64, sender: &str, text: &str) -> Message {
    Message {
        id,
        slug: SLUG.into(),
        sender: sender.into(),
        recipient: None,
        kind: "msg".into(),
        text: text.into(),
        ts: 100.0 * f64::from(u16::try_from(id).expect("a small id")),
    }
}

#[test]
fn each_stamp_but_review_is_read_at_the_tip_by_its_reader() {
    let all = needed(&["tests", "review", "pictures", "owner"], Some(TIP));
    let thread = [
        post(1, "amber", &format!("{TIP}: artifacts/{SLUG}/wide.png")),
        post(2, "owner", &format!("approve {TIP}")),
    ];
    let green = vec![run(1, "cargo test -p x", Phase::Test, 0)];
    let (met, missing) = standing(&all, TIP, &evidence(&thread, green));
    assert!(missing.is_empty(), "{missing:?}");
    let names: Vec<&str> = met.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        ["tests", "pictures", "owner"],
        "review is the approval's"
    );
    // Nothing at the tip: each is missing, its reader's words its detail.
    let (met, missing) = standing(&all, TIP, &evidence(&[], Vec::new()));
    assert!(met.is_empty());
    assert_eq!(
        missing,
        [
            Missing {
                name: "tests".into(),
                detail: "No build or test recorded for it at 4be91c2".into()
            },
            Missing {
                name: "pictures".into(),
                detail: "No picture posted at 4be91c2".into()
            },
            Missing {
                name: "owner".into(),
                detail: "Waits on the owner's approve of 4be91c2".into()
            },
        ]
    );
}

#[test]
fn a_stray_red_run_is_named_with_what_clears_it_and_a_green_rerun_of_it_does() {
    let tests = needed(&["tests"], Some(TIP));
    let mut runs = vec![
        run(1, "cargo test -p x --test typo", Phase::Test, 101),
        run(2, "cargo test -p x", Phase::Test, 0),
        run(3, "cargo build -p x", Phase::Build, 1),
    ];
    let (_, missing) = standing(&tests, TIP, &evidence(&[], runs.clone()));
    assert_eq!(
        missing,
        [Missing {
            name: "tests".into(),
            detail: "red at 4be91c2: `cargo test -p x --test typo` (exit 101), `cargo build -p x` \
                     (exit 1); the newest run of each command at the tip is what counts, so \
                     running that command again, green, at 4be91c2 clears it; one that cannot \
                     pass (a command that names what is not there) is cleared only by a new \
                     commit, where the stamps the task needs are given again"
                .into()
        }]
    );
    // The same commands run again, green, at the tip: the newest of each counts.
    runs.push(run(4, "cargo test -p x --test typo", Phase::Test, 0));
    runs.push(run(5, "cargo build -p x", Phase::Build, 0));
    let (met, missing) = standing(&tests, TIP, &evidence(&[], runs));
    assert!(missing.is_empty(), "{missing:?}");
    assert_eq!(met[0].0, "tests");
}

#[test]
fn a_refusal_names_each_missing_stamp_and_who_gives_it() {
    let missing = |names: &[&str]| -> Vec<Missing> {
        (names.iter())
            .map(|name| Missing {
                name: name.to_string(),
                detail: format!("no {name}"),
            })
            .collect()
    };
    assert_eq!(
        refusal(SLUG, TIP, &missing(&["tests", "pictures", "owner"])),
        (
            "tools/lamp lacks at its tip 4be91c2 what it needs: tests (no tests) and pictures (no \
             pictures), which its executor gives at that tip; owner (no owner), which the owner \
             gives on the task's page"
                .into(),
            "@executor"
        )
    );
    assert_eq!(
        refusal(SLUG, TIP, &missing(&["owner"])),
        (
            "tools/lamp lacks at its tip 4be91c2 what it needs: owner (no owner), which the owner \
             gives on the task's page"
                .into(),
            "@orchestrator"
        )
    );
    assert!(owner_alone(&missing(&["owner"])));
    for names in [&["tests", "owner"][..], &["pictures"], &[]] {
        assert!(!owner_alone(&missing(names)), "{names:?}");
    }
}
