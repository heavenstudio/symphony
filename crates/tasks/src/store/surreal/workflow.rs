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

/// What an activation plans from, in one read: the active generation, and each task a workflow
/// holds or the board's writers keep open, with its row's version, status, position and blocked
/// source.
pub(in crate::store) const STANDINGS: &str = "
RETURN {
    active: (SELECT VALUE body FROM ONLY workflow:['active']) ?? NULL,
    tasks: (SELECT slug, version, status, fields._board.workflow ?? NULL AS position,
        fields._board.blocked.from ?? NULL AS blocked_from FROM task
        WHERE status NOT IN ['done', 'cancelled'] OR fields._board.workflow != NONE ORDER BY slug),
};";

/// An activation, whole: refused as stale unless the active generation is still `$expected` and
/// every task it read is still at its version (and workflow revision); then each kept task
/// written at its version, each moved one recorded and moved, the definition recorded once under
/// its digest, the activation recorded, and the active generation moved to it.
pub(in crate::store) const ACTIVATE: &str = "
LET $active = (SELECT VALUE body FROM ONLY workflow:['active']) ?? NONE;
IF ($active.generation ?? 0) != $expected {
    THROW string::concat('refused: stale_workflow: the active generation is ', <string>($active.generation ?? 0), ', not ', <string>$expected);
};
FOR $t IN $kept {
    IF (SELECT VALUE version FROM ONLY type::record('task', $t.slug)) != $t.version {
        THROW string::concat('refused: stale_task: ', $t.slug, ' changed while its workflow activated');
    };
    UPDATE type::record('task', $t.slug) SET version += 1 RETURN NONE;
};
FOR $m IN $moved {
    LET $task = (SELECT version, fields._board.workflow.revision ?? 0 AS revision FROM ONLY type::record('task', $m.slug));
    IF $task = NONE OR $task.version != $m.version OR $task.revision != $m.expected {
        THROW string::concat('refused: stale_task: ', $m.slug, ' changed while its workflow activated');
    };
    CREATE type::record('workflow', ['event', $m.slug, $m.transition.revision])
        CONTENT { kind: 'event', body: $m.transition, at: $at } RETURN NONE;
    UPDATE type::record('task', $m.slug) SET status = $m.transition.status,
        fields._board.workflow = $m.position, updated = $at, version += 1 RETURN NONE;
};
IF (SELECT VALUE kind FROM ONLY type::record('workflow', ['definition', $digest])) = NONE {
    CREATE type::record('workflow', ['definition', $digest])
        CONTENT { kind: 'definition', body: $definition, at: $at } RETURN NONE;
};
CREATE type::record('workflow', ['activation', $generation])
    CONTENT { kind: 'activation', body: $activation, at: $at } RETURN NONE;
UPSERT workflow:['active'] CONTENT { kind: 'active', body: { generation: $generation, digest: $digest }, at: $at } RETURN NONE;
RETURN NONE;";
