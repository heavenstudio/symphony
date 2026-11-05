//! Confirm an uncertain transaction by its immutable save receipt.

use super::super::Failure;
use serde_json::{Value, json};

/// Keep an unconfirmed outcome pending so the composer retains its draft.
pub(in crate::save) fn pending(error: &str) -> Value {
    json!({"pending": true, "error": error})
}

fn found(store: &tasks::Store, new: &tasks::NewTask, receipt: &str) -> Result<bool, Failure> {
    match store.get(&new.slug) {
        Ok(task) if crate::taskview::is_quick_task(&task, receipt) => Ok(true),
        Ok(_) => Err(format!(
            "task {} already belongs to another save; your text is kept",
            new.slug
        )
        .into()),
        Err(tasks::Error::Refused(_)) => Ok(false),
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
    store: &tasks::Store,
    new: &tasks::NewTask,
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
    match store.create_owner_task(new, receipt) {
        Ok(_) => Ok(saved()),
        Err(error) => match found(store, new, receipt) {
            Ok(true) => Ok(saved()),
            Ok(false) if matches!(error, tasks::Error::Refused(_)) => Err(error.into()),
            Ok(false) => Ok(pending(&error.to_string())),
            Err(check) if check.status == 503 => Ok(pending(&check.error)),
            Err(check) => Err(check),
        },
    }
}
