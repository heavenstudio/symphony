//! Version 1 describes the existing operations. `legacy_board` denotes the current board's
//! predicates (claim confinement, roles, stamps, holds, subtask/covered/landing rules), not a
//! new policy implementation. Omitted configuration still executes those existing methods.

use symphony_api::workflow::*;

/// What a transition or an activation back to the board's own writers names where a custom
/// definition has its digest: built-in v1 is this build's, committed by no project file.
pub const DEFAULT: &str = "builtin-v1";

const STATUSES: [&str; 9] = [
    "backlog",
    "concept",
    "design",
    "ready",
    "in_progress",
    "review",
    "done",
    "blocked",
    "cancelled",
];
const MOVES: [&[&str]; 9] = [
    &["concept", "design", "ready", "cancelled"],
    &["backlog", "design", "ready", "blocked", "cancelled"],
    &["backlog", "concept", "ready", "blocked", "cancelled"],
    &[
        "backlog",
        "concept",
        "design",
        "in_progress",
        "blocked",
        "cancelled",
    ],
    &["ready", "review", "blocked", "cancelled"],
    &["in_progress", "blocked", "cancelled"],
    &[],
    &[
        "concept",
        "design",
        "ready",
        "in_progress",
        "review",
        "cancelled",
    ],
    &[],
];

/// The pair of built-in v1 a task of a board status stands at when nobody holds it: what a task
/// the board's existing writers kept starts from when a custom workflow activates. None for a
/// word that is no status.
pub fn legacy_pair(status: &str) -> Option<Pair> {
    STATUSES.contains(&status).then(|| at(status))
}

fn at(status: &str) -> Pair {
    Pair {
        phase: status.into(),
        state: match status {
            "in_progress" => "working",
            "review" => "awaiting_review",
            "done" => "approved",
            "blocked" => "waiting",
            "cancelled" => "cancelled",
            _ => "available",
        }
        .into(),
    }
}

fn manual() -> RecoveryAction {
    RecoveryAction {
        target: None,
        manual: true,
        slot: Slot::Retain,
        release_role: false,
        release_reservation: false,
    }
}

fn recovery(target: Option<Pair>) -> Recovery {
    let release = target.map_or_else(manual, |target| RecoveryAction {
        target: Some(target),
        manual: false,
        slot: Slot::Release,
        release_role: true,
        release_reservation: false,
    });
    Recovery {
        release: release.clone(),
        lost_holder: release,
        attempt_failure: manual(),
        cancellation_acknowledged: manual(),
    }
}

fn edge(id: &str, from: Pair, to: Pair, operation: Operation, effects: Vec<Effect>) -> Edge {
    let mut effects = effects;
    effects.push(Effect::Slot {
        policy: match (
            ["in_progress", "review"].contains(&from.phase.as_str()),
            ["in_progress", "review"].contains(&to.phase.as_str()),
        ) {
            (false, true) => Slot::Acquire,
            (true, true) => Slot::Retain,
            _ => Slot::Release,
        },
    });
    Edge {
        id: id.into(),
        label: id.replace('_', " "),
        from,
        to,
        operation,
        alias: Some(id.into()),
        authority: vec![Authority::ProjectCapability {
            capability: "legacy_actor".into(),
        }],
        inputs: vec![],
        dependencies: Dependency::None,
        guards: vec!["legacy_board".into()],
        effects,
        automatic: false,
        retry: None,
        human_wait: false,
    }
}

/// The statuses whose position a role holder holds at a `held` pair of its own.
const HELD: [&str; 3] = ["concept", "design", "review"];

/// The position of built-in v1 at `status`, the `index`th of [`STATUSES`].
fn status_position(index: usize, status: &str) -> Position {
    Position {
        at: at(status),
        staffing: match status {
            "concept" | "design" | "ready" | "in_progress" => Some(Role::Executor),
            "review" => Some(Role::Reviewer),
            _ => None,
        },
        slot: if ["in_progress", "review"].contains(&status) {
            Slot::Retain
        } else {
            Slot::Release
        },
        dependencies: Dependency::Done,
        unsuccessful_dependency: UnsuccessfulDependency::Cancel,
        accepts_work: !["done", "blocked", "cancelled"].contains(&status),
        terminal: status == "cancelled",
        legacy: [
            LegacyStatus::Backlog,
            LegacyStatus::Concept,
            LegacyStatus::Design,
            LegacyStatus::Ready,
            LegacyStatus::InProgress,
            LegacyStatus::Review,
            LegacyStatus::Done,
            LegacyStatus::Blocked,
            LegacyStatus::Cancelled,
        ][index],
        outcome: (status == "cancelled").then_some(Outcome::Cancellation),
        satisfies_dependencies: status == "done",
        completion: Completion::None,
        recovery: (status != "cancelled")
            .then(|| recovery((status == "in_progress").then(|| at("ready")))),
    }
}

/// Where built-in v1 closes a task its landing completed.
fn closed() -> Pair {
    Pair {
        phase: "done".into(),
        state: "completed".into(),
    }
}

fn closed_position() -> Position {
    Position {
        at: closed(),
        staffing: None,
        slot: Slot::Release,
        dependencies: Dependency::None,
        unsuccessful_dependency: UnsuccessfulDependency::Block,
        accepts_work: false,
        terminal: true,
        legacy: LegacyStatus::Done,
        outcome: Some(Outcome::Success),
        satisfies_dependencies: true,
        completion: Completion::None,
        recovery: None,
    }
}

fn held_pair(status: &str) -> Pair {
    Pair {
        phase: status.into(),
        state: "held".into(),
    }
}

/// The position of `status` held by its role holder: the status's own, at its `held` pair, which
/// recovers to the status's own pair.
fn held_position(status: &str) -> Position {
    let index = (STATUSES.iter())
        .position(|known| *known == status)
        .expect("a held status is a status");
    Position {
        at: held_pair(status),
        recovery: Some(recovery(Some(at(status)))),
        ..status_position(index, status)
    }
}

/// The position built-in v1 declares at `pair`, built alone, or none for a pair it does not
/// declare: what a reopening reads of the pair a task leaves, without building the whole of
/// [`builtin`], which declares the same positions by the same functions.
pub(crate) fn position_at(pair: &Pair) -> Option<Position> {
    if *pair == closed() {
        return Some(closed_position());
    }
    let index = STATUSES.iter().position(|status| *status == pair.phase)?;
    let status = STATUSES[index];
    if *pair == at(status) {
        return Some(status_position(index, status));
    }
    (pair.state == "held" && HELD.contains(&status)).then(|| held_position(status))
}

/// A fresh copy of immutable built-in v1, in the same format as custom candidates. It records
/// the default's legacy status projection and predicates without replacing any current writer.
pub fn builtin() -> Definition {
    let mut d = Definition {
        format: FORMAT,
        initial: at("ready"),
        task_kinds: vec!["task".into(), "subtask".into()],
        phases: STATUSES
            .iter()
            .enumerate()
            .map(|(i, id)| Phase {
                id: (*id).into(),
                label: id.replace('_', " "),
                order: i as u32,
                phone_order: [6, 5, 4, 2, 1, 0, 3, 7, 8][i],
                column: i > 0 && i < 7,
            })
            .collect(),
        states: [
            "available",
            "working",
            "awaiting_review",
            "approved",
            "waiting",
            "cancelled",
            "completed",
            "held",
        ]
        .iter()
        .map(|id| State {
            id: (*id).into(),
            label: id.replace('_', " "),
        })
        .collect(),
        positions: vec![],
        edges: vec![],
        capabilities: vec!["legacy_actor".into()],
        adapters: vec!["legacy_board".into()],
        guards: vec![Guard {
            id: "legacy_board".into(),
            adapter: "legacy_board".into(),
        }],
    };
    for (index, status) in STATUSES.iter().enumerate() {
        d.positions.push(status_position(index, status));
        for target in MOVES[index] {
            let (operation, effects) = match *target {
                "cancelled" => (
                    Operation::Cancel,
                    vec![Effect::Outcome {
                        outcome: Outcome::Cancellation,
                        satisfies_dependencies: false,
                    }],
                ),
                "blocked" => (Operation::Block, vec![Effect::Block { save_resume: true }]),
                "review" if *status == "in_progress" => (Operation::RequestReview, vec![]),
                _ if *status == "blocked" => (Operation::Resume, vec![Effect::ClearBlock]),
                _ => (Operation::Advance, vec![]),
            };
            let mut e = edge(
                &format!("move_{target}"),
                at(status),
                at(target),
                operation,
                effects,
            );
            e.id = format!("{status}_to_{target}");
            d.edges.push(e);
        }
    }
    let closed = closed();
    d.positions.push(closed_position());
    for status in HELD {
        let held = held_pair(status);
        d.positions.push(held_position(status));
        let role = if status == "review" {
            Role::Reviewer
        } else {
            Role::Executor
        };
        d.edges.push(edge(
            &format!("claim_{status}"),
            at(status),
            held.clone(),
            Operation::Claim,
            vec![Effect::AcquireRole { role }],
        ));
        d.edges.push(edge(
            &format!("release_{status}"),
            held.clone(),
            at(status),
            Operation::Release,
            vec![Effect::ReleaseRole { role }],
        ));
        // A held preparation/review position retains all moves available before the claim.
        let outgoing: Vec<_> = d
            .edges
            .iter()
            .filter(|e| e.from == at(status) && e.operation != Operation::Claim)
            .cloned()
            .collect();
        for mut e in outgoing {
            e.id = format!("held_{}", e.id);
            e.from = held.clone();
            d.edges.push(e);
        }
    }
    for status in ["backlog", "concept", "design", "ready"] {
        d.edges.push(edge(
            &format!("create_{status}"),
            at("ready"),
            at(status),
            Operation::Create,
            vec![],
        ));
    }
    for status in ["ready", "in_progress"] {
        let mut claim = edge(
            &format!("claim_{status}"),
            at(status),
            at("in_progress"),
            Operation::Claim,
            vec![Effect::AcquireRole {
                role: Role::Executor,
            }],
        );
        claim.dependencies = Dependency::Done;
        d.edges.push(claim);
    }
    d.edges.push(edge(
        "release_work",
        at("in_progress"),
        at("ready"),
        Operation::Release,
        vec![Effect::ReleaseRole {
            role: Role::Executor,
        }],
    ));
    for from in [
        at("review"),
        Pair {
            phase: "review".into(),
            state: "held".into(),
        },
    ] {
        for (name, approved, to) in [
            ("approve", true, at("done")),
            ("changes", false, at("in_progress")),
        ] {
            let mut e = edge(
                &format!("{name}_{}", from.state),
                from.clone(),
                to,
                if approved {
                    Operation::Approve
                } else {
                    Operation::Changes
                },
                vec![Effect::Review { approved }],
            );
            e.authority = vec![Authority::TaskRole {
                role: Role::Reviewer,
            }];
            d.edges.push(e);
        }
    }
    d.edges.push(edge(
        "reopen_approved",
        at("done"),
        at("review"),
        Operation::Advance,
        vec![],
    ));
    for status in ["in_progress", "review"] {
        let mut hold = edge(
            &format!("hold_{status}"),
            at(status),
            at("blocked"),
            Operation::Block,
            vec![Effect::Block { save_resume: true }],
        );
        hold.effects.retain(|e| !matches!(e, Effect::Slot { .. }));
        hold.effects.push(Effect::Slot {
            policy: Slot::Retain,
        });
        d.edges.push(hold);
    }
    // The current covered-close rule accepts every non-finished legacy status, including
    // preparation, blocks and held roles. Its legacy predicate still checks the holder and land.
    for position in &d.positions {
        if matches!(
            position.legacy,
            LegacyStatus::Done | LegacyStatus::Cancelled
        ) {
            continue;
        }
        let suffix = if position.at.state == "held" {
            format!("{}_held", position.at.phase)
        } else {
            position.at.phase.clone()
        };
        d.edges.push(edge(
            &format!("covered_{suffix}"),
            position.at.clone(),
            closed.clone(),
            Operation::CloseCovered,
            vec![Effect::Outcome {
                outcome: Outcome::Success,
                satisfies_dependencies: true,
            }],
        ));
    }
    d.edges.push(edge(
        "complete_subtask",
        at("in_progress"),
        closed.clone(),
        Operation::CompleteSubtask,
        vec![
            Effect::ParentIntegration,
            Effect::Outcome {
                outcome: Outcome::Success,
                satisfies_dependencies: true,
            },
        ],
    ));
    for status in ["review", "blocked", "in_progress", "done"] {
        d.edges.push(edge(
            &format!("prepare_land_{status}"),
            at(status),
            at(status),
            Operation::PrepareLand,
            vec![Effect::Prepare {
                adapter: "legacy_board".into(),
            }],
        ));
        d.edges.push(edge(
            &format!("confirm_land_{status}"),
            at(status),
            closed.clone(),
            Operation::ConfirmLand,
            vec![
                Effect::Finalize {
                    adapter: "legacy_board".into(),
                },
                Effect::Outcome {
                    outcome: Outcome::Success,
                    satisfies_dependencies: true,
                },
            ],
        ));
    }
    d
}
