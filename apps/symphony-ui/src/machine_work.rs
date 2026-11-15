//! One build/test presentation for a task page's cards; uncertainty never means free capacity. The
//! agents page's machine context is the web client's (apps/symphony-web/src/agents.tsx).

use dioxus::prelude::*;
use symphony_api::{MachineWork, WorkFreshness, WorkKind, WorkOperation, WorkState};

pub(crate) enum Scope<'a> {
    Task(&'a str),
}

struct Phase<'a> {
    operation: &'a WorkOperation,
    machine: &'a str,
    freshness: WorkFreshness,
    proven_machine: bool,
}

impl Scope<'_> {
    fn contains(&self, operation: &WorkOperation) -> bool {
        match self {
            Self::Task(slug) => operation.is_for(slug),
        }
    }
}

fn kind(kind: &WorkKind) -> &'static str {
    if *kind == WorkKind::Build {
        "Build"
    } else {
        "Test"
    }
}

fn state(state: &WorkState) -> &'static str {
    match state {
        WorkState::Queued => "queued",
        WorkState::Running => "running",
    }
}

fn freshness(freshness: &WorkFreshness) -> &'static str {
    match freshness {
        WorkFreshness::Fresh => "fresh",
        WorkFreshness::Stale => "stale",
        WorkFreshness::Disconnected => "disconnected",
        WorkFreshness::Incomplete => "partial inventory",
        WorkFreshness::Conflict => "conflicting evidence",
        WorkFreshness::Unknown => "unknown",
    }
}

fn phases<'a>(work: &'a MachineWork, scope: &Scope<'_>) -> Vec<Phase<'a>> {
    let mut phases: Vec<Phase<'a>> = Vec::new();
    let mut append = |operation: &'a WorkOperation,
                      machine: &'a str,
                      observed: WorkFreshness,
                      proven_machine: bool| {
        if !scope.contains(operation) || !matches!(operation.kind, WorkKind::Build | WorkKind::Test)
        {
            return;
        }
        if proven_machine
            && operation.operation_id.is_some()
            && let Some(previous) = phases.iter_mut().find(|previous| {
                previous.proven_machine
                    && previous.operation.machine_id == operation.machine_id
                    && previous.operation.operation_id == operation.operation_id
            })
        {
            if previous.operation != operation {
                previous.freshness = WorkFreshness::Conflict;
            }
            return;
        }
        phases.push(Phase {
            operation,
            machine,
            freshness: observed,
            proven_machine,
        });
    };
    for machine in &work.machines {
        for operation in &machine.operations {
            if operation.machine_id.as_deref() != Some(machine.machine_id.as_str()) {
                append(operation, "Machine unknown", WorkFreshness::Conflict, false);
            } else {
                let observed = if !machine.complete && machine.freshness == WorkFreshness::Fresh {
                    WorkFreshness::Incomplete
                } else {
                    machine.freshness.clone()
                };
                append(operation, &machine.label, observed, true);
            }
        }
    }
    for operation in &work.unassigned {
        append(operation, "Machine unknown", WorkFreshness::Unknown, false);
    }
    // Keep receiver order in every scope; a second client-side sort adds no evidence.
    phases
}

fn phase_label(phase: &Phase<'_>) -> String {
    let kind = kind(&phase.operation.kind);
    let state = state(&phase.operation.state);
    if phase.freshness == WorkFreshness::Fresh {
        format!("{kind} {state} · {}", phase.machine)
    } else {
        format!(
            "Observed {kind} {state} · {} · {}",
            phase.machine,
            freshness(&phase.freshness)
        )
    }
}

/// A broken browser feed cannot preserve a claim of current machine capacity.
#[cfg(any(test, all(target_arch = "wasm32", feature = "page-tasks")))]
pub(crate) fn connection(mut work: MachineWork, connected: bool) -> MachineWork {
    if !connected {
        for machine in &mut work.machines {
            machine.freshness = WorkFreshness::Disconnected;
            machine.counts = None;
        }
    }
    work
}

/// The phases `scope` shows, a machine's own read as disconnected while the feed is.
fn shown_phases<'a>(work: &'a MachineWork, scope: &Scope<'_>, connected: bool) -> Vec<Phase<'a>> {
    let mut rows = phases(work, scope);
    if !connected {
        for row in &mut rows {
            if row.proven_machine {
                row.freshness = WorkFreshness::Disconnected;
            }
        }
    }
    rows
}

/// Every card shares the same ownership, kind and uncertainty rules.
pub(crate) fn phases_view(work: &MachineWork, scope: &Scope<'_>, connected: bool) -> Element {
    let rows = shown_phases(work, scope, connected);
    rsx! {
        if !rows.is_empty() {
            div { class: "work-phases chips", aria_label: "Build and test work",
                for row in rows {
                    span {
                        class: if row.freshness == WorkFreshness::Fresh && row.operation.state == WorkState::Running { "chip work-phase health-live" } else { "chip work-phase health-stale" },
                        "data-work-kind": if row.operation.kind == WorkKind::Build { "build" } else { "test" },
                        title: "{row.operation.recipe} · {row.operation.resource} · {row.operation.reason}",
                        "{phase_label(&row)}"
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
