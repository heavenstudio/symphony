//! What a read of tasks is handed in each database's language: its variables ([`vars`]), with the
//! bounds of the Codex registrations' presence ([`native_bounds`]), and the health a native
//! registration gives a session ([`native_health`]).

use super::{EXECUTING, PULLABLE, Which, cap};
use crate::health::{self, Health};
use crate::rows::SessionRow;
use serde_json::{Value, json};

/// The variables of a read of `which`, which each database's statements name alike
/// (`surreal::reading::reading`, `postgres::reading::reading`): what it picks its tasks by, the
/// cap's where it reads the slots an orchestrator holds, the statuses that execute and the
/// presence bounds.
pub(super) fn vars(which: &Which) -> Value {
    let mut vars = match which {
        Which::All | Which::SlotHoldings => json!({}),
        Which::Status(status) => json!({"status": status}),
        Which::Slug(slug) | Which::Starting(slug) | Which::Children(slug) => json!({"slug": slug}),
        Which::Slugs(slugs) => json!({"wanted": slugs}),
        Which::Pullable(handle) => json!({"handle": handle, "pullable": PULLABLE}),
        Which::Linked(doc) => json!({"doc": doc}),
    };
    if matches!(which, Which::Starting(_) | Which::SlotHoldings)
        && let (Some(vars), Value::Object(more)) = (vars.as_object_mut(), cap::vars())
    {
        vars.extend(more);
    }
    vars["executing"] = json!(EXECUTING);
    native_bounds(&mut vars);
    vars
}

/// `vars` with the bounds a read of the Codex registrations' presence names: `codex:`, and
/// `codex;`, the character after the colon, so an address starting `codex:` is one from `$codex`
/// up to `$codex_after`.
pub(super) fn native_bounds(vars: &mut Value) {
    vars["codex"] = json!("codex:");
    vars["codex_after"] = json!("codex;");
}

/// A completed sweep either confirms a Codex pane or retires its callback. An expired registration
/// therefore means the sweep did not establish absence, even if ordinary heartbeats have expired.
/// Keep that uncertainty for at most another lost interval; a stalled sweep cannot hold work forever.
pub(super) fn native_health(
    row: &SessionRow,
    health: Health,
    now: f64,
    until: Option<f64>,
) -> Health {
    if row.left_at.is_none()
        && health != Health::Live
        && until.is_some_and(|until| {
            until < now && now - until <= health::LIVE_SECONDS + health::STALE_SECONDS
        })
    {
        Health::Unknown
    } else {
        health
    }
}
