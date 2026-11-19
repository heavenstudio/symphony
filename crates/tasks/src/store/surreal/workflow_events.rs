//! SurrealQL of `store/workflow_events.rs`: a task's recorded workflow transitions.

/// The transitions of the task `$slug`, by revision, each its recorded body: an empty list for a
/// task no workflow ever held, or no task at all.
pub(in crate::store) const TRANSITIONS: &str =
    "RETURN (SELECT VALUE body FROM workflow:['event', $slug, NONE]..['event', $slug, ..]);";
