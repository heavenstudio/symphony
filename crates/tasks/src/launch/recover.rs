//! Resume a missing conversation once per overdue addressed post, from the periodic reap command.

use super::herdr::{Herdr, Object, text};
use super::{Machine, cleanup, reap, restore};
use crate::rows::TaskSessionRow;
use crate::store::unanswered::restorable;
use crate::{Error, Store};
use std::io::Write;

pub(super) fn present(launch: &TaskSessionRow, agents: &[Object]) -> bool {
    agents.iter().any(|agent| {
        text(agent.get("pane_id")) == launch.pane
            || text(agent.get("name")) == launch.handle
            || cleanup::stored_session(agent) == launch.claude_session
    })
}

/// The timer's recovery pass. Missing conversations remain in PULL for a person to launch.
/// The durable reservation precedes process creation, so even an interrupted sweep cannot resume
/// the same post twice. An unsuccessful attempt stays flagged and is not retried automatically.
pub fn restore_unanswered(
    store: &Store,
    herdr: &dyn Herdr,
    machine: &Machine,
    warn: &mut dyn Write,
) -> Result<Vec<String>, Error> {
    let tasks = store.tasks(None)?;
    let callbacks = store.callbacks()?;
    let mut restored = Vec::new();
    for pending in store.pending_posts(&tasks)? {
        if !pending.overdue(store.now()) {
            continue;
        }
        if pending.health.holds() && callbacks.contains_key(&pending.handle) {
            continue; // a native agent may be alive without a Herdr pane
        }
        let Some(launch) = pending.launch.as_ref().filter(|s| restorable(s)) else {
            continue;
        };
        let agents = herdr.all_agents()?;
        if !reap::native_sessions(&agents).1 || present(launch, &agents) {
            continue;
        }
        // Re-read immediately before reserving: an answer or replacement makes recovery obsolete.
        let task = store.get(&pending.slug)?;
        if !store.pending_posts(&[task])?.iter().any(|p| {
            p.handle == pending.handle && p.post.id == pending.post.id && p.overdue(store.now())
        }) || !store.claim_unanswered_restore(&pending)?
        {
            continue;
        }
        match restore(
            store,
            herdr,
            machine,
            &pending.slug,
            Some(&pending.handle),
            None,
        ) {
            Ok(started) if !started.is_empty() => {
                let line = format!(
                    "restored {} for unanswered post #{}; {}",
                    pending.handle,
                    pending.post.id,
                    pending.command()
                );
                store.record_unanswered_restore(&pending.slug, &line)?;
                restored.push(line);
            }
            Ok(_) => {}
            Err(problem) => {
                let _ = writeln!(
                    warn,
                    "could not restore {} for post #{}: {problem}; {}",
                    pending.handle,
                    pending.post.id,
                    pending.command()
                );
            }
        }
    }
    Ok(restored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_existing_conversation_is_present_after_its_pane_moves() {
        let launch = TaskSessionRow {
            pane: "old-pane".into(),
            handle: "worker".into(),
            claude_session: "codex:thread".into(),
            ..TaskSessionRow::default()
        };
        assert!(!present(&launch, &[]));
        for (expected, agent) in [
            (true, json!({"pane_id":"old-pane"})),
            (true, json!({"pane_id":"new-pane", "name":"worker"})),
            (
                true,
                json!({"pane_id":"new-pane", "agent":"codex", "agent_session":{"value":"thread"}}),
            ),
            (
                false,
                json!({"pane_id":"new-pane", "agent":"codex", "agent_session":{"value":"another-thread"}}),
            ),
            (
                false,
                json!({"pane_id":"new-pane", "agent":"claude", "agent_session":{"value":"thread"}}),
            ),
        ] {
            assert_eq!(
                present(&launch, &[agent.as_object().unwrap().clone()]),
                expected,
                "{agent}"
            );
        }
    }
}
