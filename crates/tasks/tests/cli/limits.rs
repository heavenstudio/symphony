//! The project's `[limits]` as the command line holds to them, read from the main checkout's
//! `symphony.toml` as it is at each command: the cap written there over `[board]`'s, the
//! low-volume threshold PULL says, `tasks limits` naming where each came from, and a store kept
//! open (as the Symphony's server keeps its own) following an edit with no restart.

use crate::slots::{RESERVE_HINT, holding, holds_two, in_project};
use crate::{Case, run};
use std::fs;
use std::path::{Path, PathBuf};

/// The project file of the checkout at `root`, as the board names it.
fn file(root: &Path) -> PathBuf {
    root.join("symphony.toml").canonicalize().unwrap()
}

/// What PULL tells an orchestrator holding no open task under a low-volume threshold of `low`.
fn low_volume(orch: &str, low: usize) -> String {
    format!(
        "  low_volume {orch} open=0 threshold={low} cap=none — pick disjoint ready work or resolve owned blockers using actual resources and ownership"
    )
}

#[test]
fn a_cap_written_in_limits_holds_and_a_different_one_in_board_is_said_and_left_out() {
    let (c, root) = in_project(
        "limits-cap",
        "[limits]\norchestrator_slots = 2\n\n[board]\norchestrator_slots = 9\n",
    );
    let (orch, worker) = holding(&c, 2);
    let ran = c.run_as(&worker, &["claim", "work/2"], "");
    assert_eq!(ran.code, 1, "{}", ran.err);
    assert_eq!(
        ran.err,
        format!(
            "⚠ {}: [board] orchestrator_slots (line 5) is 9, left out: [limits] orchestrator_slots (line 2) is 2, which holds\n✖ work/2 cannot start: {}{RESERVE_HINT}\n",
            file(&root).display(),
            holds_two(&orch)
        )
    );
}

#[test]
fn an_edit_to_the_file_holds_from_the_next_command() {
    let (c, root) = in_project("limits-next", "[limits]\norchestrator_slots = 2\n");
    let (_, worker) = holding(&c, 2);
    assert_eq!(c.run_as(&worker, &["claim", "work/2"], "").code, 1);
    // Edited in the main checkout, not committed: the file as it stands is the one read.
    fs::write(
        root.join("symphony.toml"),
        "[limits]\norchestrator_slots = 3\n",
    )
    .unwrap();
    c.ok(&worker, &["claim", "work/2"]);
}

#[test]
fn a_store_kept_open_follows_an_edit_to_the_file_with_no_restart() {
    let (c, root) = in_project("limits-live", "[limits]\norchestrator_slots = 2\n");
    let (_, worker) = holding(&c, 2);
    // Opened once and kept, as the Symphony's server keeps its reader and its data worker's store.
    let store = c.store();
    assert_eq!(store.slots(), Some(2));
    assert!(store.claim("work/2", &worker).is_err());
    fs::write(
        root.join("symphony.toml"),
        "[limits]\norchestrator_slots = 10\nlow_volume = 1\nmachine_agents = 6\n\n[board]\nreview_alarm_ratio = 3\n",
    )
    .unwrap();
    let limits = store.limits();
    assert_eq!(
        (
            limits.orchestrator_slots,
            limits.low_volume,
            limits.machine_agents
        ),
        (Some(10), 1, Some(6))
    );
    assert_eq!(store.alarm().ratio, 3);
    store.claim("work/2", &worker).unwrap();
    // The table taken out again: every default, no cap.
    fs::write(root.join("symphony.toml"), "[project]\nname = \"slots\"\n").unwrap();
    assert_eq!(store.limits(), symphony_board::limits::Limits::default());
}

#[test]
fn pull_says_low_volume_at_the_threshold_the_file_writes_and_none_at_zero() {
    let (c, root) = in_project("limits-low", "[limits]\nlow_volume = 1\n");
    let orch = c.register("orchestrator");
    let pulled = c.ok(&orch, &["pull"]);
    assert!(pulled.contains(&low_volume(&orch, 1)), "{pulled}");
    fs::write(root.join("symphony.toml"), "[limits]\nlow_volume = 0\n").unwrap();
    let pulled = c.ok(&orch, &["pull"]);
    assert!(!pulled.contains("  low_volume "), "{pulled}");
    // With no table, the default of 4.
    fs::write(root.join("symphony.toml"), "[project]\nname = \"low\"\n").unwrap();
    let pulled = c.ok(&orch, &["pull"]);
    assert!(pulled.contains(&low_volume(&orch, 4)), "{pulled}");
}

#[test]
fn limits_prints_each_limit_and_model_in_force_and_where_it_came_from() {
    let manifest = "\
[agents]
launch = { default = \"claude\", model = \"opus\" }

[agents.models]
reviewer = \"sonnet\"

[limits]
low_volume = 2
machine_agents = 7
staff = 3

[board]
orchestrator_slots = 8
";
    let (mut c, root) = in_project("limits-shown", manifest);
    // An hc that is not there: the machine's agents cannot be counted, never this machine's own.
    c.env
        .vars
        .insert("HC_BIN".into(), root.join("no-hc").display().to_string());
    let orch = c.register("orchestrator");
    let ran = c.run_as(&orch, &["limits"], "");
    assert_eq!(ran.code, 0, "{}", ran.err);
    let lines: Vec<&str> = ran.out.lines().collect();
    assert_eq!(lines[0], format!("LIMITS from {}", file(&root).display()));
    assert_eq!(
        lines[1..3],
        [
            "orchestrator_slots       8       [board] orchestrator_slots, line 13",
            "low_volume               2       [limits] low_volume, line 8",
        ]
    );
    assert_eq!(
        lines[3..],
        [
            "machine_agents           7       [limits] machine_agents, line 9; hc cannot be asked, so the agents this machine runs cannot be counted",
            "review_alarm_ratio       2       default",
            "review_alarm_hours       4       default",
            "review_alarm_clear_hours 1       default",
            "MODELS of claude",
            "orchestrator             -       whoever starts one chooses",
            "executor                 opus    [agents] launch model, line 2",
            "reviewer                 sonnet  [agents.models] reviewer, line 5",
            "LEFT OUT [limits] staff (line 10): this build does not know it",
        ]
    );
}

#[test]
fn limits_says_a_value_it_cannot_read_and_shows_the_default_in_force() {
    let (c, root) = in_project("limits-bad", "[limits]\nlow_volume = \"many\"\n");
    let orch = c.register("orchestrator");
    let ran = c.run_as(&orch, &["limits"], "");
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(
        ran.err,
        format!(
            "⚠ {}: [limits] low_volume (line 2) is \"many\"; not a whole number; it takes its default, 4\n",
            file(&root).display()
        )
    );
    assert!(
        ran.out
            .contains("\nlow_volume               4       default\n"),
        "{}",
        ran.out
    );
}

#[test]
fn limits_says_a_launch_table_it_cannot_read_and_still_prints_the_limits() {
    let (c, root) = in_project(
        "limits-launch-broken",
        "[agents.launch]\ndefault = \"gemini\"\n\n[limits]\nlow_volume = 2\n",
    );
    let orch = c.register("orchestrator");
    let ran = c.run_as(&orch, &["limits"], "");
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(
        ran.err,
        format!(
            "⚠ {}: [agents] launch default \"gemini\" is not one of codex, claude\n",
            file(&root).display()
        )
    );
    let lines: Vec<&str> = ran.out.lines().collect();
    assert_eq!(
        lines[2],
        "low_volume               2       [limits] low_volume, line 5"
    );
    assert_eq!(
        lines.last(),
        Some(&"MODELS: the project file cannot be read for them")
    );
}

#[test]
fn outside_a_repository_every_limit_is_its_default() {
    let c = Case::new();
    let ran = run(&c.env, &["limits"], "");
    assert_eq!(ran.code, 0, "{}", ran.err);
    let lines: Vec<&str> = ran.out.lines().collect();
    assert_eq!(
        lines[..7],
        [
            "LIMITS: no project file outside a repository, so every default",
            "orchestrator_slots       none    default",
            "low_volume               4       default",
            "machine_agents           none    default",
            "review_alarm_ratio       2       default",
            "review_alarm_hours       4       default",
            "review_alarm_clear_hours 1       default",
        ]
    );
}
