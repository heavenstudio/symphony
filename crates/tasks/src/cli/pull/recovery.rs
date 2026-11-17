//! PULL advice follows today's executor without reassigning or answering a historical post.
//! A replacement still holding work gets a follow-up; once lost, its role needs recovery again.
//! Each target/action has its own delivery identity, so a previous recovery or follow-up cannot
//! swallow the next handoff's advice. Ordinary posts keep their existing once-per-post identity.

use crate::store::unanswered::Pending;

#[derive(Debug, Clone)]
pub(super) struct Advice {
    pub slug: String,
    pub post: i64,
    pub handle: String,
    pub command: String,
    pub key: String,
}

impl Advice {
    pub(super) fn of(pending: Pending) -> Self {
        let mut key = format!("{}#{}", pending.slug, pending.post.id);
        let replacement = pending
            .executor
            .as_ref()
            .filter(|(handle, _)| pending.role == "executor" && *handle != pending.handle);
        let mut handle = pending.handle.clone();
        let command = if let Some((current, health)) = replacement {
            handle = format!("{} (executor now {current})", pending.handle);
            let action = if health.holds() {
                "followup"
            } else {
                "recover"
            };
            key.push_str(&format!(":executor:{current}:{action}"));
            if health.holds() {
                format!(
                    "just tasks say {} 'Reconcile unanswered post #{} addressed to {}' --to {}",
                    pending.slug, pending.post.id, pending.handle, current
                )
            } else {
                // The predecessor's restorable conversation is not today's executor. Recover
                // the task's vacant role rather than suggesting that old conversation again.
                format!("just tasks launch {} --role executor", pending.slug)
            }
        } else {
            pending.command()
        };
        Self {
            slug: pending.slug,
            post: pending.post.id,
            handle,
            command,
            key,
        }
    }
}
