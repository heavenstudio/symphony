//! `tasks pull --as HANDLE`: the whole digest as that orchestrator hears it, whatever a callback
//! carried before, read on demand and changing nothing.

use crate::{Case, repository, scratch};

const FAKE: &str = env!("CARGO_BIN_EXE_fake-hc");

/// A board run from a repository of its own, hc the crate's `fake-hc` with no terminal.
pub(super) fn case(name: &str) -> Case {
    let dir = scratch(&format!("pull-{name}"));
    let mut c = Case::new();
    c.env.cwd = repository(&dir.join("repo"));
    let state = dir.join("hc.json");
    std::fs::write(&state, r#"{"terminals": []}"#).unwrap();
    c.env.vars.insert("HC_BIN".into(), FAKE.into());
    (c.env.vars).insert("FAKE_HC_STATE".into(), state.display().to_string());
    c
}

#[test]
fn pull_prints_the_whole_digest_its_actor_hears_every_time_it_asks() {
    let c = case("whole");
    let orch = c.register("orchestrator");
    let other = c.register("orchestrator");
    for (slug, title) in [
        ("sim/free", "Free"),
        ("sim/mine", "Mine"),
        ("sim/theirs", "Theirs"),
    ] {
        c.ok(&orch, &["create", slug, title]);
    }
    c.ok(&orch, &["role", "sim/mine", "orchestrator"]);
    c.ok(&other, &["role", "sim/theirs", "orchestrator"]);
    let whole = c.ok(&orch, &["pull"]);
    let lines: Vec<&str> = whole.lines().collect();
    // Every count, unchanged or not, after the local minute ("PULL HH:MM ").
    assert!(lines[0].starts_with("PULL "), "{whole}");
    assert_eq!(
        lines[0][11..],
        *"executing=0 ready_unstaffed=2 in_progress_orphaned=0 review_unreviewed=0 landed_open=0 idle=0"
    );
    assert_eq!(
        lines[1..],
        [
            format!(
                "  low_volume {orch} open=0 threshold=4 cap=none — pick disjoint ready work or resolve owned blockers using actual resources and ownership"
            ),
            "  ready_unstaffed p2 sim/free — Free".into(),
            "  ready_unstaffed p2 sim/mine — Mine".into(),
        ]
    );
    // Asked again, with nothing changed, it says it all again: reading is not being told.
    assert_eq!(c.ok(&orch, &["pull"]), whole);
    // The other orchestrator hears its own task and the free one, not this one's.
    let theirs = c.ok(&other, &["pull"]);
    assert!(theirs.contains("sim/theirs"), "{theirs}");
    assert!(!theirs.contains("sim/mine"), "{theirs}");
}

#[test]
fn pull_needs_an_actor() {
    let c = case("actor");
    let ran = c.run_as("", &["pull"], "");
    assert_eq!(ran.code, 2, "{}", ran.out);
    assert!(ran.err.contains("TASKS_SESSION"), "{}", ran.err);
}
