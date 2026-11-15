//! Explicit attribution, nonce deduplication and unknown capacity on a task page's cards. The
//! agents' and the machines' scopes, and the machine context's occupancy, are the web client's
//! (apps/symphony-web/src/work.test.ts, agents.test.tsx).

use super::*;
use serde_json::json;

fn operation(machine: &str, nonce: &str, kind: &str, state: &str) -> WorkOperation {
    serde_json::from_value(json!({
        "machine_id":machine, "operation_id":nonce, "parent_id":null,
        "handle":"worker-a", "task":"sim/a", "checkout":"/opaque/checkout",
        "recipe":"fixture", "kind":kind, "state":state,
        "resource":"run_slot", "reason":"", "phase_since":null,
    }))
    .unwrap()
}

fn machine(id: &str, operations: Vec<WorkOperation>) -> symphony_api::MachineObservation {
    serde_json::from_value(json!({
        "machine_id":id, "label":id, "producer_id":"fixture", "sequence":1,
        "age_ms":0, "freshness":"fresh", "complete":true, "reason":"",
        "counts":{"running_builds":0,"queued_builds":0,"running_tests":0,"queued_tests":0},
        "operations":operations,
    }))
    .unwrap()
}

#[test]
fn identities_deduplicate_wrappers_on_one_machine_without_merging_other_machines() {
    let build = operation("alpha", "same-nonce", "build", "running");
    let work = MachineWork {
        machines: vec![
            machine("alpha", vec![build.clone(), build]),
            machine(
                "beta",
                vec![operation("beta", "same-nonce", "test", "queued")],
            ),
        ],
        ..MachineWork::default()
    };
    let rows = phases(&work, &Scope::Task("sim/a"));
    assert_eq!(rows.len(), 2);
    assert_eq!(phase_label(&rows[0]), "Build running · alpha");
    assert_eq!(phase_label(&rows[1]), "Test queued · beta");
}

#[test]
fn resource_badges_use_explicit_ownership_and_exclude_setup_commands_and_reservations() {
    let mut no_task = operation("alpha", "native", "test", "running");
    no_task.task = None;
    let work = MachineWork {
        machines: vec![machine(
            "alpha",
            vec![
                operation("alpha", "build", "build", "queued"),
                no_task,
                operation("alpha", "setup", "setup", "running"),
                operation("alpha", "command", "command", "running"),
                operation("alpha", "reservation", "reservation", "queued"),
            ],
        )],
        ..MachineWork::default()
    };
    let task = phases(&work, &Scope::Task("sim/a"));
    assert_eq!(task.len(), 1);
    assert_eq!(phase_label(&task[0]), "Build queued · alpha");
    assert!(phases(&work, &Scope::Task("sim/other")).is_empty());
    // The card of a task the run's task is under carries its chip too, once the server says so.
    assert!(phases(&work, &Scope::Task("sim")).is_empty());
    let mut placed = work.clone();
    placed.place_under(|slug| (slug == "sim/a").then_some("sim"));
    let parent = phases(&placed, &Scope::Task("sim"));
    assert_eq!(parent.len(), 1);
    assert_eq!(phase_label(&parent[0]), "Build queued · alpha");
    assert_eq!(phases(&placed, &Scope::Task("sim/a")).len(), 1);
}

#[test]
fn partial_stale_and_conflicting_rows_retain_observation_without_claiming_current_execution() {
    let mut alpha = machine("alpha", vec![operation("alpha", "test", "test", "running")]);
    alpha.complete = false;
    let mut work = MachineWork {
        machines: vec![alpha],
        ..MachineWork::default()
    };
    let target = Scope::Task("sim/a");
    assert_eq!(
        phase_label(&phases(&work, &target)[0]),
        "Observed Test running · alpha · partial inventory"
    );
    work.machines[0].freshness = WorkFreshness::Stale;
    assert_eq!(
        phase_label(&phases(&work, &target)[0]),
        "Observed Test running · alpha · stale"
    );
    // A newly observed sibling shares the machine's conservative oldest-phase freshness.
    // The original task's card keeps its stale qualification.
    let mut sibling = operation("alpha", "new-sibling", "build", "queued");
    sibling.task = Some("sim/new".into());
    work.machines[0].operations.push(sibling);
    work.machines[0].age_ms = Some(10_002);
    let html = dioxus_ssr::render_element(phases_view(&work, &target, true));
    assert!(
        html.contains("Observed Test running · alpha · stale"),
        "{html}"
    );
    work.machines[0].operations[0].machine_id = Some("foreign".into());
    assert_eq!(
        phase_label(&phases(&work, &target)[0]),
        "Observed Test running · Machine unknown · conflicting evidence"
    );
    work.machines.clear();
    let mut legacy = operation("alpha", "legacy", "build", "queued");
    legacy.machine_id = None;
    legacy.operation_id = None;
    work.unassigned = vec![legacy.clone(), legacy];
    let rows = phases(&work, &target);
    assert_eq!(
        rows.len(),
        2,
        "unproven legacy rows cannot be deduplicated by a path or label"
    );
    assert_eq!(
        phase_label(&rows[0]),
        "Observed Build queued · Machine unknown · unknown"
    );
    for operation in &mut work.unassigned {
        operation.operation_id = Some("same-untrusted-nonce".into());
        operation.machine_id = Some("claimed-alpha".into());
    }
    assert_eq!(
        phases(&work, &target).len(),
        2,
        "unassigned identity strings are not a trusted machine envelope"
    );
}

#[test]
fn a_card_shows_a_disconnected_machines_phase_as_observed() {
    let mut alpha = machine("alpha", vec![operation("alpha", "test", "test", "running")]);
    alpha.freshness = WorkFreshness::Disconnected;
    let work = MachineWork {
        machines: vec![alpha],
        ..MachineWork::default()
    };
    let html = dioxus_ssr::render_element(phases_view(&work, &Scope::Task("sim/a"), true));
    assert!(
        html.contains("Observed Test running · alpha · disconnected"),
        "{html}"
    );
    assert!(html.contains("data-work-kind=\"test\""), "{html}");
}

#[test]
fn feed_loss_drops_the_counts_and_retains_the_exact_last_observed_phase() {
    let work = MachineWork {
        machines: vec![machine(
            "alpha",
            vec![operation("alpha", "test", "test", "running")],
        )],
        ..MachineWork::default()
    };
    assert_eq!(connection(work.clone(), true), work);
    let lost = connection(work.clone(), false);
    assert_eq!(lost.machines[0].operations, work.machines[0].operations);
    assert_eq!(lost.machines[0].counts, None);
    assert_eq!(lost.machines[0].freshness, WorkFreshness::Disconnected);
    assert_eq!(
        phase_label(&phases(&lost, &Scope::Task("sim/a"))[0]),
        "Observed Test running · alpha · disconnected"
    );
}

/// A card's phase chips, in their order, read as disconnected while the feed is; a task with no
/// work has none.
#[test]
fn a_cards_phase_chips_read_as_disconnected_while_the_feed_is() {
    let work = MachineWork {
        machines: vec![machine(
            "alpha",
            vec![
                operation("alpha", "build", "build", "running"),
                operation("alpha", "test", "test", "queued"),
            ],
        )],
        ..MachineWork::default()
    };
    let chips = |slug: &str, connected: bool| {
        let html = dioxus_ssr::render_element(phases_view(&work, &Scope::Task(slug), connected));
        html.split("data-work-kind")
            .skip(1)
            .map(|chip| chip.split('>').nth(1).unwrap().split('<').next().unwrap())
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        chips("sim/a", true),
        ["Build running · alpha", "Test queued · alpha"]
    );
    assert_eq!(
        chips("sim/a", false),
        [
            "Observed Build running · alpha · disconnected",
            "Observed Test queued · alpha · disconnected"
        ]
    );
    assert!(chips("sim/b", true).is_empty());
}
