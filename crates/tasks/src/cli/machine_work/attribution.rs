//! Retain every local reservation and enrich it only from positively observed ownership.

use super::{Kind, Operation, Snapshot, State};
use crate::cli::agents::Run;
use crate::rows::TaskSessionRow;
use std::collections::HashMap;

/// Attribute from an explicit owner or one active, positively local checkout. Never use the
/// agent's selected display task: it can be a historical review in another checkout.
pub(crate) fn attribute(
    work: &mut Snapshot,
    runs: &[Run],
    records: &[TaskSessionRow],
    fallback_owners: &HashMap<String, &str>,
    real: &dyn Fn(&str) -> String,
) {
    for run in runs {
        let mut operation = Operation {
            machine_id: None,
            operation_id: None,
            parent_id: None,
            handle: run.handle.clone(),
            task: None,
            checkout: run.worktree.clone(),
            recipe: run.recipe.clone(),
            kind: Kind::Reservation,
            state: if run.phase == "waiting" {
                State::Queued
            } else {
                State::Running
            },
            resource: format!("{}_slot", run.pool),
            reason: "local slot reservation; execution and stable machine identity unknown".into(),
            phase_since: run.since,
        };
        assign(&mut operation, records, fallback_owners, real);
        work.unassigned.push(operation);
    }
    // Native operations already carry receiver-validated attribution. Their foreign paths
    // are opaque, and current board roles cannot rewrite work that started under an earlier role.
}

fn assign(
    operation: &mut Operation,
    records: &[TaskSessionRow],
    fallback_owners: &HashMap<String, &str>,
    real: &dyn Fn(&str) -> String,
) {
    // An explicit handle remains valid, but two missing paths cannot prove its task context.
    if operation.checkout.is_empty() {
        return;
    }
    if operation.handle.is_none() {
        operation.handle = fallback_owners
            .get(&real(&operation.checkout))
            .map(|handle| (*handle).to_string());
    }
    let Some(handle) = operation.handle.as_deref() else {
        return;
    };
    // A handle came from this local run or a positively observed owner, never a hostname match.
    let mut tasks: Vec<_> = records
        .iter()
        .filter(|r| r.closed.is_none() && r.handle == handle && r.worktree == operation.checkout)
        .map(|r| &r.slug)
        .collect();
    tasks.sort();
    tasks.dedup();
    if tasks.len() == 1 {
        operation.task = Some(tasks[0].clone());
    }
}

pub(crate) fn json(work: &Snapshot) -> crate::pyjson::Py {
    fn convert(value: serde_json::Value) -> crate::pyjson::Py {
        use crate::pyjson::Py;
        use serde_json::Value;
        match value {
            Value::Null => Py::Null,
            Value::Bool(b) => Py::Bool(b),
            Value::Number(n) => {
                if let Some(signed) = n.as_i64() {
                    Py::Int(signed)
                } else if let Some(unsigned) = n.as_u64() {
                    Py::UInt(unsigned)
                } else {
                    Py::Float(n.as_f64().unwrap_or_default())
                }
            }
            Value::String(s) => Py::Str(s),
            Value::Array(a) => Py::List(a.into_iter().map(convert).collect()),
            Value::Object(o) => Py::Obj(o.into_iter().map(|(k, v)| (k, convert(v))).collect()),
        }
    }
    convert(serde_json::to_value(work).expect("machine work records serialize"))
}
