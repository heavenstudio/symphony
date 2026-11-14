//! `tasks pull --as HANDLE`: the whole digest as that orchestrator hears it, whatever a callback
//! carried before, read on demand and changing nothing.

use crate::{Case, repository, scratch};
use symphony_board::NewTask;

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
fn pull_and_inbox_show_every_orchestrator_what_the_owner_said_to_nobody() {
    use super::wait::OWNER;
    let c = case("owner");
    let (orch, other) = (c.register("orchestrator"), c.register("orchestrator"));
    // No channel and nobody waiting: nobody holds the chat seat.
    c.store().owner_callback("asks-1", OWNER).unwrap();
    for handle in [&orch, &other] {
        let whole = c.ok(handle, &["pull"]);
        assert!(whole.ends_with(&format!("\n{OWNER}\n")), "{whole}");
    }
    // An orchestrator's inbox takes it; read, it is shown to nobody after.
    assert!(c.ok(&orch, &["inbox"]).contains(OWNER));
    assert!(!c.ok(&other, &["pull"]).contains(OWNER));
    assert!(!c.ok(&other, &["inbox"]).contains(OWNER));
}

#[test]
fn pull_prints_the_whole_digest_its_actor_hears_every_time_it_asks() {
    let c = case("whole");
    let orch = c.register("orchestrator");
    let other = c.register("orchestrator");
    // Filed through the store, which gives no orchestrator: sim/free stays nobody's, as a task is
    // until the board gives it one.
    for (slug, title) in [
        ("sim/free", "Free"),
        ("sim/mine", "Mine"),
        ("sim/theirs", "Theirs"),
    ] {
        c.store().create(&NewTask::new(slug, title, &orch)).unwrap();
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
fn pull_lists_idle_no_child_it_is_told_went_quiet_as_the_dispatch_does() {
    let mut c = case("quiet-child");
    let orch = c.register("orchestrator");
    let child = c.register_with("worker", &["--parent", &orch]);
    let stranger = c.register("worker");
    c.ok(&orch, &["create", "sim/kid", "Kid"]);
    c.ok(&orch, &["role", "sim/kid", "orchestrator"]);
    c.ok(&child, &["claim", "sim/kid"]);
    let first = c.ok(&orch, &["wait", "--timeout", "0"]);
    assert!(first.contains(" idle=0"), "{first}");
    // Both workers go stale with no terminal, which the agents view calls idle.
    let later = crate::common::NOW + symphony_board::health::LIVE_SECONDS + 1.0;
    c.env.vars.insert("TASKS_NOW".into(), later.to_string());
    let waited = c.ok(&orch, &["wait", "--timeout", "0"]);
    let pulled = c.ok(&orch, &["pull"]);
    // The child is said STALE, not listed idle; the stranger, of whom it is not told, is idle.
    assert!(waited.contains(&format!("STALE {child}")), "{waited}");
    assert!(waited.contains("PULL idle=1 (was 0)\n"), "{waited}");
    let header = pulled.lines().next().unwrap();
    assert!(header.ends_with(" idle=1"), "{pulled}");
    for said in [&waited, &pulled] {
        assert!(!said.contains(&format!("idle {child}")), "{said}");
        assert!(said.contains(&format!("idle {stranger}")), "{said}");
    }
}

/// Past its wait (`TASKS_UNANSWERED_WAIT`), the unanswered lookup leaves its posts out of the PULL
/// and the rest of the PULL stands; within it, the same pull tells them. A microsecond is a wait no
/// answer meets, however quiet the machine.
#[test]
fn pull_past_the_unanswered_wait_leaves_out_the_posts_alone() {
    use symphony_board::UNANSWERED_WAIT_VAR;
    let mut c = case("unanswered-wait");
    let orch = c.register("orchestrator");
    let worker = c.register("worker");
    for slug in ["sim/asked", "sim/free"] {
        c.ok(&orch, &["create", slug, "Work"]);
        c.ok(&orch, &["role", slug, "orchestrator"]);
    }
    c.ok(&worker, &["claim", "sim/asked"]);
    let post = (c.store())
        .say("sim/asked", "owner", "continue", Some("@executor"))
        .unwrap();
    // Half an hour on, its executor gone: the post is overdue.
    let later = crate::common::NOW + 31.0 * 60.0 + crate::common::GONE;
    c.env.vars.insert("TASKS_NOW".into(), later.to_string());
    let told = c.ok(&orch, &["pull"]);
    assert!(told.contains(&format!("post #{} —", post.id)), "{told}");
    assert!(told.contains("ready_unstaffed=1"), "{told}");

    c.env
        .vars
        .insert(UNANSWERED_WAIT_VAR.into(), "0.000001".into());
    let skipped = c.ok(&orch, &["pull"]);
    let without: Vec<String> = (told.lines())
        .map(|line| line.replace(" unanswered=1", ""))
        .filter(|line| !line.trim_start().starts_with("unanswered "))
        .collect();
    assert_eq!(skipped.lines().collect::<Vec<_>>(), without, "{told}");

    let backstop = crate::common::LOOKUP_BACKSTOP.to_string();
    c.env.vars.insert(UNANSWERED_WAIT_VAR.into(), backstop);
    assert_eq!(c.ok(&orch, &["pull"]), told);
}

#[test]
fn pull_needs_an_actor() {
    let c = case("actor");
    let ran = c.run_as("", &["pull"], "");
    assert_eq!(ran.code, 2, "{}", ran.out);
    assert!(ran.err.contains("TASKS_SESSION"), "{}", ran.err);
}
