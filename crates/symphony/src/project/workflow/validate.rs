//! Syntax and graph invariants, without host membership, stored positions or policy evaluation.

use std::collections::BTreeSet;
use symphony_api::workflow::*;

type Result = std::result::Result<(), String>;

fn id(value: &str) -> Result {
    if !value.starts_with(|c: char| c.is_ascii_lowercase())
        || !value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
    {
        return Err(format!("invalid workflow ID: {value:?}"));
    }
    Ok(())
}

fn names<'a>(
    values: impl IntoIterator<Item = &'a str>,
    kind: &str,
) -> std::result::Result<BTreeSet<&'a str>, String> {
    let mut names = BTreeSet::new();
    for value in values {
        id(value)?;
        if !names.insert(value) {
            return Err(format!("duplicate {kind}: {value}"));
        }
    }
    Ok(names)
}

fn reference(value: &str, names: &BTreeSet<&str>, kind: &str) -> Result {
    id(value)?;
    if !names.contains(value) {
        return Err(format!("unknown {kind}: {value}"));
    }
    Ok(())
}

fn pair(at: &Pair, positions: &BTreeSet<&Pair>) -> Result {
    if !positions.contains(at) {
        return Err(format!(
            "invalid phase/state pair: {}/{}",
            at.phase, at.state
        ));
    }
    Ok(())
}

pub(super) fn validate(d: &Definition) -> Result {
    if d.format != FORMAT {
        return Err(format!("unsupported workflow format: {}", d.format));
    }
    if d.task_kinds.is_empty()
        || d.phases.is_empty()
        || d.states.is_empty()
        || d.positions.is_empty()
    {
        return Err("workflow requires task kinds, phases, states and positions".into());
    }
    names(d.task_kinds.iter().map(String::as_str), "task kind")?;
    let phases = names(d.phases.iter().map(|p| p.id.as_str()), "phase")?;
    let states = names(d.states.iter().map(|s| s.id.as_str()), "state")?;
    names(d.edges.iter().map(|e| e.id.as_str()), "edge")?;
    let caps = names(d.capabilities.iter().map(String::as_str), "capability")?;
    let adapters = names(d.adapters.iter().map(String::as_str), "adapter")?;
    let guards = names(d.guards.iter().map(|g| g.id.as_str()), "guard")?;
    for guard in &d.guards {
        reference(&guard.adapter, &adapters, "guard adapter")?;
    }
    let mut order = BTreeSet::new();
    let mut phone = BTreeSet::new();
    for phase in &d.phases {
        if phase.label.trim().is_empty()
            || !order.insert(phase.order)
            || !phone.insert(phase.phone_order)
        {
            return Err("phase labels and unique display/phone order are required".into());
        }
    }
    if d.states.iter().any(|s| s.label.trim().is_empty()) {
        return Err("state label is empty".into());
    }
    let mut positions = BTreeSet::new();
    for p in &d.positions {
        reference(&p.at.phase, &phases, "phase")?;
        reference(&p.at.state, &states, "state")?;
        if !positions.insert(&p.at) {
            return Err(format!("duplicate position: {}/{}", p.at.phase, p.at.state));
        }
        if p.terminal && (p.accepts_work || p.staffing.is_some() || p.outcome.is_none()) {
            return Err("terminal position requires an outcome and no work/staffing".into());
        }
        if p.staffing.is_some() && p.recovery.is_none() {
            return Err(format!(
                "missing holder recovery: {}/{}",
                p.at.phase, p.at.state
            ));
        }
    }
    pair(&d.initial, &positions)?;
    if d.positions.iter().any(|p| p.at == d.initial && p.terminal) {
        return Err("initial position is terminal".into());
    }
    for p in &d.positions {
        if let Some(r) = &p.recovery {
            for action in [
                &r.release,
                &r.lost_holder,
                &r.attempt_failure,
                &r.cancellation_acknowledged,
            ] {
                if let Some(target) = &action.target {
                    pair(target, &positions)?;
                } else if !action.manual {
                    return Err("recovery stay requires explicit manual recovery".into());
                }
            }
        }
    }
    let mut aliases = BTreeSet::new();
    let mut routes = BTreeSet::new();
    for e in &d.edges {
        pair(&e.from, &positions)?;
        pair(&e.to, &positions)?;
        if d.positions.iter().any(|p| p.at == e.from && p.terminal) {
            return Err(format!("ordinary edge from terminal position: {}", e.id));
        }
        if e.label.trim().is_empty() || e.authority.is_empty() {
            return Err(format!("edge {} requires a label and authority", e.id));
        }
        if e.authority.iter().collect::<BTreeSet<_>>().len() != e.authority.len() {
            return Err(format!("duplicate authority: {}", e.id));
        }
        if let Some(alias) = &e.alias {
            id(alias)?;
            if !aliases.insert((&e.from, alias)) {
                return Err(format!("ambiguous legacy alias: {alias}"));
            }
        }
        if !routes.insert((&e.from, e.operation, &e.alias)) {
            return Err(format!("ambiguous operation route: {}", e.id));
        }
        for authority in &e.authority {
            match authority {
                Authority::TaskRole { .. } => {}
                Authority::ProjectCapability { capability }
                | Authority::SystemCapability { capability } => {
                    reference(capability, &caps, "capability")?
                }
            }
        }
        if e.human_wait
            && (e.automatic
                || !e
                    .authority
                    .iter()
                    .any(|a| matches!(a, Authority::ProjectCapability { .. })))
        {
            return Err(format!(
                "human wait requires a project capability and deliberate action: {}",
                e.id
            ));
        }
        names(e.inputs.iter().map(String::as_str), "input")?;
        names(e.guards.iter().map(String::as_str), "edge guard")?;
        for guard in &e.guards {
            reference(guard, &guards, "guard")?;
        }
        if let Some(retry) = &e.retry {
            pair(&retry.exhausted, &positions)?;
            if retry.maximum == 0 || !manual(d, &retry.exhausted) {
                return Err(
                    "retry requires a finite positive maximum and manual exhaustion destination"
                        .into(),
                );
            }
        }
        effects(e, &adapters)?;
        if let Some(destination) = d.positions.iter().find(|p| p.at == e.to && p.terminal)
            && !e.effects.iter().any(|effect| matches!(effect, Effect::Outcome { outcome, satisfies_dependencies } if Some(*outcome) == destination.outcome && *satisfies_dependencies == destination.satisfies_dependencies))
        {
            return Err(format!("terminal route requires matching outcome effect: {}", e.id));
        }
        if let Some(destination) = d.positions.iter().find(|p| p.at == e.to && p.terminal) {
            let completes = match destination.completion {
                Completion::None => true,
                Completion::Landing => e.operation == Operation::ConfirmLand,
                Completion::CoveredClose => e.operation == Operation::CloseCovered,
                Completion::ParentIntegration => e.operation == Operation::CompleteSubtask,
            };
            if !completes {
                return Err(format!(
                    "route does not meet completion requirement: {}",
                    e.id
                ));
            }
        }
        if e.automatic && reachable(d, &e.to, &e.from, true) && e.retry.is_none() {
            return Err(format!(
                "automatic rework cycle requires a finite retry bound: {}",
                e.id
            ));
        }
    }
    for p in &d.positions {
        if !p.terminal
            && !d.positions.iter().any(|end| {
                (end.outcome.is_some() || manual(d, &end.at)) && reachable(d, &p.at, &end.at, false)
            })
        {
            return Err(format!(
                "no outcome or manual recovery reachable: {}/{}",
                p.at.phase, p.at.state
            ));
        }
        if p.terminal && !reachable(d, &d.initial, &p.at, false) {
            return Err(format!(
                "unreachable completion: {}/{}",
                p.at.phase, p.at.state
            ));
        }
    }
    Ok(())
}

fn manual(d: &Definition, at: &Pair) -> bool {
    d.positions
        .iter()
        .find(|p| &p.at == at)
        .and_then(|p| p.recovery.as_ref())
        .is_some_and(|r| {
            [
                &r.release,
                &r.lost_holder,
                &r.attempt_failure,
                &r.cancellation_acknowledged,
            ]
            .iter()
            .any(|a| a.manual)
        })
}

fn reachable(d: &Definition, from: &Pair, to: &Pair, automatic: bool) -> bool {
    let mut pending = vec![from];
    let mut seen = BTreeSet::new();
    while let Some(at) = pending.pop() {
        if at == to {
            return true;
        }
        if seen.insert(at) {
            pending.extend(
                d.edges
                    .iter()
                    .filter(|e| &e.from == at && (!automatic || e.automatic))
                    .map(|e| &e.to),
            );
        }
    }
    false
}

fn effects(e: &Edge, adapters: &BTreeSet<&str>) -> Result {
    use Effect as E;
    use Operation as O;
    let has = |predicate: fn(&Effect) -> bool| e.effects.iter().any(predicate);
    let required = match e.operation {
        O::Claim => has(|x| matches!(x, E::AcquireRole { .. })),
        O::Release => has(|x| matches!(x, E::ReleaseRole { .. })),
        O::Approve => has(|x| matches!(x, E::Review { approved: true })),
        O::Changes => has(|x| matches!(x, E::Review { approved: false })),
        O::Block => has(|x| matches!(x, E::Block { save_resume: true })),
        O::Resume => has(|x| matches!(x, E::ClearBlock)),
        O::Cancel => has(|x| {
            matches!(
                x,
                E::Outcome {
                    outcome: Outcome::Cancellation,
                    ..
                }
            )
        }),
        O::CompleteSubtask => {
            has(|x| matches!(x, E::ParentIntegration))
                && has(|x| {
                    matches!(
                        x,
                        E::Outcome {
                            outcome: Outcome::Success,
                            ..
                        }
                    )
                })
        }
        O::CloseCovered => has(|x| {
            matches!(
                x,
                E::Outcome {
                    outcome: Outcome::Success,
                    ..
                }
            )
        }),
        O::PrepareLand | O::PrepareExternal => has(|x| matches!(x, E::Prepare { .. })),
        O::ConfirmLand | O::ConfirmExternal => has(|x| matches!(x, E::Finalize { .. })),
        _ => true,
    };
    if !required {
        return Err(format!("missing operation effect: {}", e.id));
    }
    for effect in &e.effects {
        let allowed = match effect {
            E::Review { approved: true } => e.operation == O::Approve,
            E::Review { approved: false } => e.operation == O::Changes,
            E::ParentIntegration => e.operation == O::CompleteSubtask,
            E::Block { .. } => e.operation == O::Block,
            E::ClearBlock => e.operation == O::Resume,
            E::Prepare { adapter } => {
                reference(adapter, adapters, "effect adapter")?;
                matches!(e.operation, O::PrepareLand | O::PrepareExternal)
            }
            E::Finalize { adapter } => {
                reference(adapter, adapters, "effect adapter")?;
                matches!(e.operation, O::ConfirmLand | O::ConfirmExternal)
            }
            E::Outcome {
                outcome,
                satisfies_dependencies,
            } => {
                (!satisfies_dependencies || *outcome == Outcome::Success)
                    && matches!(
                        e.operation,
                        O::Cancel
                            | O::CompleteSubtask
                            | O::CloseCovered
                            | O::ConfirmLand
                            | O::ConfirmExternal
                    )
            }
            _ => true,
        };
        if !allowed {
            return Err(format!("operation cannot impersonate effect: {}", e.id));
        }
    }
    // A graph cannot authorize a dependent/self review through a different operation or actor.
    if matches!(e.operation, O::Approve | O::Changes)
        && e.authority.iter().any(|a| {
            !matches!(
                a,
                Authority::TaskRole {
                    role: Role::Reviewer
                }
            )
        })
    {
        return Err(format!(
            "review requires an independent task reviewer: {}",
            e.id
        ));
    }
    if e.effects
        .iter()
        .filter(|x| matches!(x, E::Slot { .. }))
        .count()
        != 1
    {
        return Err(format!(
            "edge requires exactly one explicit slot effect: {}",
            e.id
        ));
    }
    for count in [
        e.effects
            .iter()
            .filter(|x| matches!(x, E::Review { .. }))
            .count(),
        e.effects
            .iter()
            .filter(|x| matches!(x, E::Outcome { .. }))
            .count(),
        e.effects
            .iter()
            .filter(|x| matches!(x, E::Prepare { .. } | E::Finalize { .. }))
            .count(),
    ] {
        if count > 1 {
            return Err(format!("conflicting repeated effect: {}", e.id));
        }
    }
    Ok(())
}
