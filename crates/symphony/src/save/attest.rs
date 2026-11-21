//! The attestations the owner's workflow operation is taken with when its payload gives none: each
//! guard of its edge asked of the approval authority a plugin the project selects answers for that
//! guard's adapter ([`symphony_plugin::Authority`], docs/WORKFLOW.md, "Guards"), for the
//! operation's own snapshot and the principal the host bound. What an authority answers goes to
//! the board unchanged, which verifies it as it verifies a pushed one (the committed key, the
//! operation and principal it names, the accepted generation, its time): nothing here decides a
//! guard or keeps an answer, and each operation asks again. An authority that errs, panics or does
//! not answer in [`WITHIN`] refuses the operation as `guard_unavailable`; a guard no plugin answers
//! for is left to the board, which refuses it as it refuses one taken with no attestation.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use symphony_api::workflow::{OperationRequest, RefusalCode, Signed};
use symphony_board::workflow::admit::Refusal;
use symphony_plugin::Authority;
use symphony_plugin::authority::{Operation, Snapshot};
use symphony_plugin::execution::Principal;

use super::Failure;
use crate::plugins::Plugins;

/// How long an authority is given to answer: as long as any plugin's call
/// ([`crate::plugins::process::WAIT`]).
pub const WITHIN: Duration = crate::plugins::process::WAIT;

/// The attestations `request` is taken with by `principal`, each of its edge's guards asked of the
/// authority `plugins` hold for its adapter, in the project at `root`: none on a board with no
/// workflow active, for an edge it does not define or one no plugin's authority guards, as the
/// board decides those. Refused, `guard_unavailable` first, naming the guard and why, when an
/// authority asked gave no answer.
pub(super) fn fetched(
    store: &symphony_board::Store,
    plugins: &Plugins,
    root: &Path,
    request: &OperationRequest,
    principal: &Principal,
) -> Result<Vec<Signed>, Failure> {
    fetched_within(store, plugins, root, request, principal, WITHIN)
}

fn fetched_within(
    store: &symphony_board::Store,
    plugins: &Plugins,
    root: &Path,
    request: &OperationRequest,
    principal: &Principal,
    within: Duration,
) -> Result<Vec<Signed>, Failure> {
    let Some(read) = store.workflow_read()? else {
        return Ok(Vec::new());
    };
    let definition = &read.definition;
    let Some(edge) = definition.edges.iter().find(|e| e.id == request.edge) else {
        return Ok(Vec::new());
    };
    let asked: Vec<(&str, &'static dyn Authority)> = (edge.guards.iter())
        .filter_map(|id| {
            let guard = definition.guards.iter().find(|g| &g.id == id)?;
            Some((id.as_str(), plugins.authority(&guard.adapter)?))
        })
        .collect();
    if asked.is_empty() {
        return Ok(Vec::new());
    }
    // The project an attestation names is the one the active activation recorded, its committed
    // `[project] id`; with none, no authority is committed and the board refuses the guard.
    let Some(project) = store.workflow_authorities_of(read.activation_generation)?.0 else {
        return Ok(Vec::new());
    };
    (asked.into_iter())
        .map(|(guard, authority)| {
            let operation = operation(&project, guard, request, principal);
            let signed = answer(authority, root, operation, within).map_err(|why| {
                let said = format!(
                    "edge {}'s guard {guard}: its authority {} gave no attestation: {why}",
                    request.edge,
                    authority.adapter()
                );
                Failure::from(Refusal::new(RefusalCode::GuardUnavailable, said).to_string())
            })?;
            Ok(Signed {
                payload: signed.payload,
                signature: signed.signature,
            })
        })
        .collect()
}

/// `guard` of `request` taken by `principal` in `project`, as its attestation must name it.
fn operation(
    project: &str,
    guard: &str,
    request: &OperationRequest,
    principal: &Principal,
) -> Operation {
    let expected = &request.expected;
    Operation::new(
        project.into(),
        request.task.clone(),
        request.edge.clone(),
        guard.into(),
        Snapshot::new(
            expected.workflow.digest.clone(),
            expected.activation_generation,
            expected.task_revision,
            expected.at.phase.clone(),
            expected.at.state.clone(),
        ),
        request.input_revisions.clone(),
        principal.clone(),
    )
}

/// What `authority` answers for `operation` in the project at `root`, asked on a thread of its own
/// and waited for `within`; a panic its error. A call that never returns holds its thread, as a
/// linked plugin's call cannot be stopped.
fn answer(
    authority: &'static dyn Authority,
    root: &Path,
    operation: Operation,
    within: Duration,
) -> Result<symphony_plugin::authority::Signed, String> {
    let (said, heard) = mpsc::channel();
    let root = root.to_path_buf();
    std::thread::Builder::new()
        .name(format!("authority {}", authority.adapter()))
        .spawn(move || {
            let answer = catch_unwind(AssertUnwindSafe(|| {
                authority.attest(&root, &operation, within)
            }))
            .unwrap_or_else(|panic| {
                Err(crate::gates::panicked(
                    authority.adapter(),
                    "attesting",
                    &*panic,
                ))
            });
            // Nobody hears an answer that came after the wait.
            let _ = said.send(answer);
        })
        .map_err(|e| format!("it could not be asked: {e}"))?;
    match heard.recv_timeout(within) {
        Ok(answer) => answer,
        Err(RecvTimeoutError::Timeout) => Err(format!("it did not answer within {within:?}")),
        Err(RecvTimeoutError::Disconnected) => Err("it ended without answering".into()),
    }
}

#[cfg(test)]
#[path = "attest_tests.rs"]
mod tests;
