//! The statements of `store/workflow.rs`.

/// The active generation and the digest of its definition, or NULL on a board that never
/// activated a custom workflow.
pub(in crate::store) const ACTIVE: &str =
    "RETURN (SELECT VALUE body FROM ONLY workflow:['active']) ?? NULL;";

/// The definition recorded under `$digest`, or NULL.
pub(in crate::store) const DEFINITION: &str = "RETURN (SELECT VALUE body FROM ONLY type::record('workflow', ['definition', $digest])) ?? NULL;";

/// The task `$slug`'s workflow position, refused when there is no such task: NULL for a task the
/// board's own writers keep.
pub(in crate::store) const POSITION: &str = "
LET $task = (SELECT fields._board.workflow AS w FROM ONLY type::record('task', $slug));
IF $task = NONE { THROW string::concat('refused: no task ', $slug); };
RETURN $task.w ?? NULL;";

/// The transition `$transition` of the task `$slug`, recorded, and the task put at `$position`
/// with the status it projects to, in one write, from the workflow revision `$expected` (0 for a
/// task no workflow held): refused as stale from any other, and the record made once, so of two
/// transitions from one revision one is written. The schema's fence checks the two agree.
pub(in crate::store) const RECORD: &str = "
LET $task = (SELECT fields._board.workflow AS w FROM ONLY type::record('task', $slug));
IF $task = NONE { THROW string::concat('refused: no task ', $slug); };
IF ($task.w.revision ?? 0) != $expected {
    THROW string::concat('refused: stale_task: ', $slug, ' is at workflow revision ', <string>($task.w.revision ?? 0), ', not ', <string>$expected);
};
CREATE type::record('workflow', ['event', $slug, $transition.revision])
    CONTENT { kind: 'event', body: $transition, at: $transition.at } RETURN NONE;
UPDATE type::record('task', $slug) SET status = $transition.status, fields._board.workflow = $position,
    updated = $transition.at, version += 1 RETURN NONE;
RETURN NONE;";
