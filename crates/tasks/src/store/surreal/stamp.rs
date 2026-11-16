//! The statement of `store/stamp.rs`.

/// The task `$slug`'s row, its links, its roles, the branches its executors' sessions recorded,
/// the tasks right under it and its thread's newest line: each by its own key or index, as the
/// board's snapshot reads them, and one statement, so it is answered outside a transaction. A
/// thread's lines are never rewritten or removed, so its newest line's id says what it holds.
pub(in crate::store) const READ: &str = "RETURN {
    task: (SELECT * OMIT id FROM ONLY type::record('task', $slug)),
    links: (SELECT kind, target FROM link WHERE slug IN [$slug] ORDER BY kind, target),
    roles: (SELECT handle, role, since, expires FROM role WHERE slug IN [$slug] ORDER BY since, handle),
    branches: (SELECT branch, handle, started FROM task_session
        WHERE slug IN [$slug] AND role = 'executor' AND branch != '' ORDER BY started, handle),
    under: (SELECT slug, sid FROM task WHERE parent = $slug ORDER BY sid),
    newest: array::last(SELECT VALUE sid FROM message:[$slug, NONE]..[$slug, ..]),
};";
