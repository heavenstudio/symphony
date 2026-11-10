//! One build/test presentation for task cards and agents; uncertainty never means free capacity.

use dioxus::prelude::*;
use symphony_api::{
    MachineObservation, MachineWork, WorkFreshness, WorkKind, WorkOperation, WorkState,
};

/// Immutable snapshots are shared by every card. A replacement, not a deep comparison,
/// invalidates their props; connectivity is separate so disconnects retain original evidence.
#[derive(Clone, Default)]
pub struct SharedWork(std::rc::Rc<MachineWork>);

impl From<MachineWork> for SharedWork {
    fn from(work: MachineWork) -> Self {
        Self(std::rc::Rc::new(work))
    }
}

impl std::ops::Deref for SharedWork {
    type Target = MachineWork;
    fn deref(&self) -> &MachineWork {
        &self.0
    }
}

impl PartialEq for SharedWork {
    fn eq(&self, other: &Self) -> bool {
        std::rc::Rc::ptr_eq(&self.0, &other.0)
    }
}

pub(crate) enum Scope<'a> {
    Task(&'a str),
    #[cfg(any(not(target_arch = "wasm32"), feature = "page-tasks"))]
    Agent(&'a str),
    Machine(&'a str),
    Unassigned,
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
            Self::Task(slug) => operation.task.as_deref() == Some(*slug),
            #[cfg(any(not(target_arch = "wasm32"), feature = "page-tasks"))]
            Self::Agent(handle) => operation.handle.as_deref() == Some(*handle),
            Self::Machine(id) => operation.machine_id.as_deref() == Some(*id),
            Self::Unassigned => true,
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
    for machine in work
        .machines
        .iter()
        .filter(|_| !matches!(scope, Scope::Unassigned))
    {
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

/// Task and agent views share the same ownership, kind and uncertainty rules.
pub(crate) fn phases_view(work: &MachineWork, scope: &Scope<'_>, connected: bool) -> Element {
    let mut rows = phases(work, scope);
    if !connected {
        for row in &mut rows {
            if row.proven_machine {
                row.freshness = WorkFreshness::Disconnected;
            }
        }
    }
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

fn occupancy(machine: &MachineObservation) -> String {
    if machine.complete
        && machine.freshness == WorkFreshness::Fresh
        && let Some(counts) = &machine.counts
    {
        format!(
            "Builds: {} running, {} queued · Tests: {} running, {} queued",
            counts.running_builds, counts.queued_builds, counts.running_tests, counts.queued_tests,
        )
    } else {
        let observed = if !machine.complete && machine.freshness == WorkFreshness::Fresh {
            WorkFreshness::Incomplete
        } else {
            machine.freshness.clone()
        };
        format!("Occupancy unknown · {}", freshness(&observed))
    }
}

/// Machine occupancy is independent from task lifecycle and is never an admission rule.
pub(crate) fn machines_view(work: &MachineWork, connected: bool) -> Element {
    rsx! {
        section { class: "machine-work card", aria_label: "Machine build and test work",
            h2 { "Build and test work" }
            if work.machines.is_empty() {
                p { class: "muted", "Machine occupancy unknown" }
            }
            for machine in &work.machines {
                div { class: "machine-work-row", "data-machine-id": "{machine.machine_id}",
                    strong { "{machine.label}" }
                    p { if connected { "{occupancy(machine)}" } else { "Occupancy unknown · disconnected" } }
                    {guidance(&machine.machine_id, work.guidance.get(&machine.machine_id))}
                    {phases_view(work, &Scope::Machine(&machine.machine_id), connected)}
                    if !machine.reason.is_empty() { p { class: "muted", "{machine.reason}" } }
                }
            }
            for (id, guidance) in &work.guidance {
                if !work.machines.iter().any(|machine| &machine.machine_id == id) {
                    div { class: "machine-work-row", "data-machine-id": "{id}",
                        strong { "{id}" }
                        p { "Machine observation unknown" }
                        {self::guidance(id, Some(guidance))}
                    }
                }
            }
            if !work.unassigned.is_empty() {
                p { class: "muted", "Some work has no proven machine identity; capacity remains unknown." }
                {phases_view(work, &Scope::Unassigned, connected)}
            }
        }
    }
}

fn guidance(id: &str, guidance: Option<&symphony_api::WorkGuidance>) -> Element {
    let target =
        |value: Option<u32>| value.map_or_else(|| "unknown".into(), |value| value.to_string());
    let build = guidance.and_then(|guidance| guidance.build);
    let test = guidance.and_then(|guidance| guidance.test);
    rsx! { details { class: "machine-guidance",
        summary { "Advisory targets · builds {target(build)} · tests {target(test)}" }
        p { class: "muted", "Orchestrators decide when to start work. These targets do not enforce admission." }
        p { class: "muted", "Machine ID: " code { "{id}" } }
    } }
}

#[cfg(test)]
mod tests;
