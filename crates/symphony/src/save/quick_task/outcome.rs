//! Confirm an uncertain transaction by its immutable save receipt.

use super::super::Failure;
use serde_json::{Value, json};

/// Keep an unconfirmed outcome pending so the composer retains its draft.
pub(in crate::save) fn pending(error: &str) -> Value {
    json!({"pending": true, "error": error})
}

/// Recover a confirmed task when only its fingerprint remains in the local receipt.
pub(in crate::save) fn recover(
    store: &symphony_board::Store,
    receipt: &str,
) -> Result<Option<Value>, Failure> {
    let slugs = store.owner_saved(receipt)?;
    match slugs.as_slice() {
        [] => Ok(None),
        [slug] => Ok(Some(json!({"slug":slug}))),
        _ => Err(Failure {
            status: 503,
            error: format!("task save {receipt}: ambiguous task list {slugs:?}"),
        }),
    }
}

fn found(
    store: &symphony_board::Store,
    new: &symphony_board::NewTask,
    receipt: &str,
) -> Result<bool, Failure> {
    match store.get(&new.slug) {
        Ok(task) if crate::taskview::is_quick_task(&task, receipt, &new.by) => Ok(true),
        Ok(_) => Err(format!(
            "task {} already belongs to another save; your text is kept",
            new.slug
        )
        .into()),
        Err(symphony_board::Error::Refused(_)) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// Whether this call starts a save or checks an already dispatched intent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::save) enum Attempt {
    First,
    Check,
}

/// Return a confirmed slug or pending outcome, reading an uncertain write back first.
pub(in crate::save) fn create(
    store: &symphony_board::Store,
    new: &symphony_board::NewTask,
    receipt: &str,
    attempt: Attempt,
) -> Result<Value, Failure> {
    let saved = || json!({"slug":new.slug});
    if attempt == Attempt::Check {
        match found(store, new, receipt) {
            Ok(true) => return Ok(saved()),
            Ok(false) => {}
            Err(error) if error.status == 503 => return Ok(pending(&error.error)),
            Err(error) => return Err(error),
        }
    }
    // The slug guard, immutable receipt, task and creation message share one transaction.
    match store.persist_owner_task(new, receipt) {
        Ok(_) => Ok(saved()),
        Err(error) => match found(store, new, receipt) {
            Ok(true) => Ok(saved()),
            Ok(false) if matches!(error, symphony_board::Error::Refused(_)) => Err(error.into()),
            Ok(false) => Ok(pending(&error.to_string())),
            Err(check) if check.status == 503 => Ok(pending(&check.error)),
            Err(check) => Err(check),
        },
    }
}
