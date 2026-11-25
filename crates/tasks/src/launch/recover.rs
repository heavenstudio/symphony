//! Resume a missing conversation once per overdue addressed post, from the periodic reap command.

use super::hc::{self, Hc, Object, text_of};
use super::{Machine, cleanup, restore};
use crate::cli::{Env, agents, probes};
use crate::rows::TaskSessionRow;
use crate::store::unanswered::{Asked, restorable};
use crate::{Error, Store};
use std::collections::BTreeSet;
use std::io::Write;

pub(super) fn present(launch: &TaskSessionRow, agents: &[Object]) -> bool {
    agents.iter().any(|agent| {
        text_of(agent, "address") == launch.pane
            || text_of(agent, "name") == launch.handle
            || hc::label(agent, hc::HANDLE_LABEL) == launch.handle
            || cleanup::stored_session(agent) == launch.claude_session
    })
}

/// The timer's recovery pass. Missing conversations remain in PULL for a person to launch.
/// The durable reservation precedes process creation, so even an interrupted sweep cannot resume
/// the same post twice. An unsuccessful attempt stays flagged and is not retried automatically.
pub fn restore_unanswered(
    store: &Store,
    hc: &dyn Hc,
    machine: &Machine,
    warn: &mut dyn Write,
) -> Result<Vec<String>, Error> {
    let board = store.board()?;
    let callbacks = store.callbacks()?;
    let env = Env {
        cwd: machine.root.clone(),
        ..Env::of_process()
    };
    // Recovery already reads hc through its injected owner. The agents view needs only the
    // existing run/process probes here; it must not start a second, uninjected hc inventory.
    let probes = agents::Probes {
        panes: Box::new(|| Ok(Vec::new())),
        ..probes::machine(&env)
    };
    let verifying = || {
        agents::running_tests(store, &probes).map(|runs| {
            let mut handles = BTreeSet::new();
            handles.extend(runs.into_keys());
            handles
        })
    };
    let mut restored = Vec::new();
    for pending in store.pending_posts(&board, Asked::Overdue(&board.tasks, store.now()))? {
        if pending.health.holds() && callbacks.contains_key(&pending.handle) {
            continue; // a native agent may be alive without an hc terminal
        }
        let Some(launch) = pending.launch.as_ref().filter(|s| restorable(s)) else {
            continue;
        };
        let agents = hc.live()?;
        if agents.iter().any(|a| text_of(a, "address").is_empty()) || present(launch, &agents) {
            continue;
        }
        // Re-read before reserving: an answer, a return or a task-status change can make recovery
        // obsolete.
        let fresh = store.board()?;
        let task = store.get(&pending.slug)?;
        let again = Asked::Overdue(std::slice::from_ref(&task), store.now());
        if !store
            .pending_posts(&fresh, again)?
            .iter()
            .any(|p| p.handle == pending.handle && p.post.id == pending.post.id)
            || !store.claim_unanswered_restore_checking(&pending, &verifying)?
        {
            continue;
        }
        match restore(
            store,
            hc,
            machine,
            &launch.slug,
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
            (true, json!({"address":"old-pane"})),
            (true, json!({"address":"new-pane", "name":"worker"})),
            (
                true,
                json!({"address":"new-pane", "agent":"codex", "argv":["codex","resume","thread"]}),
            ),
            (
                false,
                json!({"address":"new-pane", "agent":"codex", "argv":["codex","resume","another-thread"]}),
            ),
            (
                false,
                json!({"address":"new-pane", "agent":"claude", "argv":["claude","--resume","thread"]}),
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
