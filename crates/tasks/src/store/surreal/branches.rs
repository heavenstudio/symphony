//! The statements of `store/branches.rs`.

pub(in crate::store) const OF_TASKS: &str = "SELECT slug, branch, handle, started FROM task_session WHERE slug IN $slot_wanted AND role = 'executor' AND branch != ''";

pub(in crate::store) const ON_BRANCHES: &str =
    "SELECT VALUE slug FROM task_session WHERE branch IN $slot_wanted AND role = 'executor'";

/// A branch and the tasks recorded on it: `$branch`, with `$slot_wanted` holding it alone.
pub(in crate::store) fn on_branch() -> String {
    format!("RETURN array::union([$branch], ({ON_BRANCHES}));")
}
