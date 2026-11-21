//! The statements of `store/workflow.rs`.

/// The active generation and the digest of its definition, or NULL on a board that never
/// activated a custom workflow.
pub(in crate::store) const ACTIVE: &str =
    "RETURN (SELECT VALUE body FROM ONLY workflow:['active']) ?? NULL;";

/// The active generation and the revision its definition was recorded with, or NULL on a board
/// that never activated a custom workflow: what a write's opening compares the committed file with.
pub(in crate::store) const ACTIVE_SOURCE: &str = "
LET $active = (SELECT VALUE body FROM ONLY workflow:['active']) ?? NONE;
RETURN IF $active = NONE { NULL } ELSE { {
    active: $active,
    revision: (SELECT VALUE body.revision FROM ONLY type::record('workflow', ['definition', $active.digest])) ?? NULL,
} };";

/// The transitions `$ids` name (`['event', SLUG, REVISION]`), each read by its id, in their order:
/// none for an id with no record.
pub(in crate::store) const TRANSITIONS: &str =
    "SELECT VALUE body FROM $ids.map(|$i| type::record('workflow', $i));";

/// The definition recorded under `$digest`, or NULL.
pub(in crate::store) const DEFINITION: &str = "RETURN (SELECT VALUE body FROM ONLY type::record('workflow', ['definition', $digest])) ?? NULL;";

/// The mappings activation `$generation` recorded, or NULL.
pub(in crate::store) const MAPPINGS: &str = "RETURN (SELECT VALUE body.mappings FROM ONLY type::record('workflow', ['activation', $generation])) ?? NULL;";

/// The project and approval authorities activation `$generation` recorded, or NULL.
pub(in crate::store) const AUTHORITIES: &str = "RETURN (SELECT VALUE { project: body.project, authorities: body.authorities } FROM ONLY type::record('workflow', ['activation', $generation])) ?? NULL;";

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
#[cfg(test)]
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

/// What an activation plans from, in one read: the active generation, the generation of the last
/// activation recorded (by the range of their ids; 0 with none), and each task a workflow holds
/// or the board's writers keep open, with its row's version, status, position, block record (and
/// its source), the accepted output its last transition left (the event at its workflow
/// revision), its execution attempts whose runtime may still be alive and that no cancel was
/// asked of, oldest first, and the workflow revision a return retired it at.
pub(in crate::store) const STANDINGS: &str = "
RETURN {
    active: (SELECT VALUE body FROM ONLY workflow:['active']) ?? NULL,
    last: array::max(SELECT VALUE body.generation FROM workflow:['activation', NONE]..=['activation', 9223372036854775807]) ?? 0,
    tasks: (SELECT slug, version, status, fields._board.workflow ?? NULL AS position,
        fields._board.blocked.from ?? NULL AS blocked_from,
        fields._board.blocked ?? NULL AS blocked,
        fields._board.workflow_retired.revision ?? 0 AS retired,
        type::record('workflow', ['event', slug, fields._board.workflow.revision ?? 0]).body.accepted ?? NULL AS accepted,
        (SELECT VALUE attempt FROM execution_attempt WHERE task = $parent.slug AND live = true
            AND (state.cancellation ?? 'none') = 'none' ORDER BY created, attempt) AS attempts
        FROM task WHERE status NOT IN ['done', 'cancelled'] OR fields._board.workflow != NONE ORDER BY slug),
};";

/// An activation, whole: refused as stale unless the active generation is still `$expected`, no
/// activation is recorded as `$generation` (after a return none is active, and another activation
/// may have taken it), every task it read is still at its version (and workflow revision) and each
/// holder it read as lost is still unseen (`$seen`); then each kept task written at its version,
/// each moved one recorded and moved, refused as stale where a task the read would select is
/// neither kept nor moved (one filed or reopened since the read, at the old generation's pair or
/// at none), the definition recorded once under its digest, the activation recorded, the board's
/// marker moved (a board read carries the generation, and a relabel moves no task) and the active
/// generation moved to it. Each task the read selected is kept or moved, and stays selected, so a
/// count over theirs is the whole check, and the task is named only where it refuses.
pub(in crate::store) const ACTIVATE: &str = "
LET $active = (SELECT VALUE body FROM ONLY workflow:['active']) ?? NONE;
IF ($active.generation ?? 0) != $expected {
    THROW string::concat('refused: stale_workflow: the active generation is ', <string>($active.generation ?? 0), ', not ', <string>$expected);
};
IF record::exists(type::record('workflow', ['activation', $generation])) {
    THROW string::concat('refused: stale_workflow: generation ', <string>$generation, ' is activated already');
};
FOR $s IN $seen {
    IF (SELECT VALUE last_seen FROM ONLY type::record('session', $s.handle)) != $s.last_seen {
        THROW string::concat('refused: stale_task: ', $s.handle, ' was seen while its workflow activated');
    };
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
LET $standing = (SELECT VALUE slug FROM task
    WHERE status NOT IN ['done', 'cancelled'] OR fields._board.workflow != NONE);
IF array::len($standing) > array::len($kept) + array::len($moved) {
    LET $planned = array::concat($kept.map(|$t| $t.slug), $moved.map(|$m| $m.slug));
    LET $unplanned = array::sort(array::complement($standing, $planned));
    THROW string::concat('refused: stale_task: ', $unplanned[0], ' was filed or reopened while its workflow activated');
};
IF (SELECT VALUE kind FROM ONLY type::record('workflow', ['definition', $digest])) = NONE {
    CREATE type::record('workflow', ['definition', $digest])
        CONTENT { kind: 'definition', body: $definition, at: $at } RETURN NONE;
};
CREATE type::record('workflow', ['activation', $generation])
    CONTENT { kind: 'activation', body: $activation, at: $at } RETURN NONE;
IF record::exists(counter:board_structure) { UPDATE counter:board_structure SET last = (last ?? 0) + 1 RETURN NONE; } ELSE { CREATE counter:board_structure SET last = 1 RETURN NONE; };
UPSERT workflow:['active'] CONTENT { kind: 'active', body: { generation: $generation, digest: $digest }, at: $at } RETURN NONE;
RETURN NONE;";

/// A return to the board's own writers, whole: refused as stale unless the active generation is
/// still `$expected`, every task it read is still at its version (and workflow revision) and each
/// holder it read as lost is still unseen (`$seen`); then the activation recorded as a return,
/// which the schema's fence reads as each position goes, each kept task written at its version,
/// each moved one's transition recorded and the task given its status and blocked record, its
/// position retired with its last revision kept (`workflow_retired`); refused as stale where a
/// task the read would select is neither kept nor moved (one filed or reopened since, which would
/// keep a position with no workflow active), as an activation is, by its slug: a closed task the
/// return moves leaves the selection, so a count would let one task it did not plan pass beside
/// it; then the board's marker moved, as an activation moves it (a board read carries the
/// generation, and a return may move no task), and the active generation deleted: none is active
/// after it.
pub(in crate::store) const RETURN_DEFAULT: &str = "
LET $active = (SELECT VALUE body FROM ONLY workflow:['active']) ?? NONE;
IF ($active.generation ?? 0) != $expected {
    THROW string::concat('refused: stale_workflow: the active generation is ', <string>($active.generation ?? 0), ', not ', <string>$expected);
};
FOR $s IN $seen {
    IF (SELECT VALUE last_seen FROM ONLY type::record('session', $s.handle)) != $s.last_seen {
        THROW string::concat('refused: stale_task: ', $s.handle, ' was seen while its workflow returned');
    };
};
CREATE type::record('workflow', ['activation', $generation])
    CONTENT { kind: 'activation', body: $activation, at: $at } RETURN NONE;
FOR $t IN $kept {
    IF (SELECT VALUE version FROM ONLY type::record('task', $t.slug)) != $t.version {
        THROW string::concat('refused: stale_task: ', $t.slug, ' changed while its workflow returned');
    };
    UPDATE type::record('task', $t.slug) SET version += 1 RETURN NONE;
};
FOR $m IN $moved {
    LET $task = (SELECT version, fields._board.workflow.revision ?? 0 AS revision FROM ONLY type::record('task', $m.slug));
    IF $task = NONE OR $task.version != $m.version OR $task.revision != $m.expected {
        THROW string::concat('refused: stale_task: ', $m.slug, ' changed while its workflow returned');
    };
    CREATE type::record('workflow', ['event', $m.slug, $m.transition.revision])
        CONTENT { kind: 'event', body: $m.transition, at: $at } RETURN NONE;
    UPDATE type::record('task', $m.slug) SET status = $m.transition.status,
        fields._board.workflow = NONE, fields._board.workflow_retired = $m.retired,
        fields._board.blocked = $m.blocked ?? NONE, updated = $at, version += 1 RETURN NONE;
};
LET $standing = (SELECT VALUE slug FROM task
    WHERE status NOT IN ['done', 'cancelled'] OR fields._board.workflow != NONE);
LET $unplanned = array::sort(array::complement($standing,
    array::concat($kept.map(|$t| $t.slug), $moved.map(|$m| $m.slug))));
IF array::len($unplanned) > 0 {
    THROW string::concat('refused: stale_task: ', $unplanned[0], ' was filed or reopened while its workflow returned');
};
IF record::exists(counter:board_structure) { UPDATE counter:board_structure SET last = (last ?? 0) + 1 RETURN NONE; } ELSE { CREATE counter:board_structure SET last = 1 RETURN NONE; };
DELETE workflow:['active'] RETURN NONE;
RETURN NONE;";

/// What admission decides `$slug`'s operation on, in one read: the active generation, the task
/// (NULL when there is none), its role rows, the tasks it depends on and those of them on the
/// board, each task with the accepted output its last transition left (the event at its workflow
/// revision), the receipt `$receipt` names (NULL when no request used its key), whether `$actor`,
/// when the principal is an agent, is a session the board knows that has not left, the approval
/// authorities the active activation commits and the state the board accepted from each.
pub(in crate::store) const ADMISSION: &str = "
LET $deps = (SELECT VALUE target FROM link WHERE slug = $slug AND kind = 'dep');
LET $generation = (SELECT VALUE body.generation FROM ONLY workflow:['active']) ?? NONE;
LET $activation = IF $generation = NONE { NONE } ELSE {
    (SELECT VALUE body FROM ONLY type::record('workflow', ['activation', $generation])) ?? NONE
};
LET $attesters = $activation.authorities ?? [];
RETURN {
    active: (SELECT VALUE body FROM ONLY workflow:['active']) ?? NULL,
    task: (SELECT slug, version, status, created_by, fields._board.workflow ?? NULL AS position,
        type::record('workflow', ['event', slug, fields._board.workflow.revision ?? 0]).body.accepted ?? NULL AS accepted
        FROM ONLY type::record('task', $slug)) ?? NULL,
    roles: (SELECT handle, role, expires FROM role WHERE slug = $slug),
    dependencies: $deps,
    found: (SELECT slug, version, status, fields._board.workflow ?? NULL AS position,
        type::record('workflow', ['event', slug, fields._board.workflow.revision ?? 0]).body.accepted ?? NULL AS accepted
        FROM $deps.map(|$d| type::record('task', $d))),
    receipt: (SELECT VALUE body FROM ONLY type::record('workflow', $receipt)) ?? NULL,
    present: IF $actor = NONE { false } ELSE {
        record::exists(type::record('session', $actor)) AND type::record('session', $actor).left_at = NONE
    },
    project: $activation.project ?? NULL,
    attesters: $attesters,
    accepted: (SELECT VALUE body FROM $attesters.map(|$a| type::record('workflow', ['authority', $a.adapter]))),
};";

/// An admitted operation or a recovery, written whole, or `stale` (read and decide again) when
/// anything it was decided on moved: the active generation and digest, the task's version, each
/// dependency its edge's predicate read, each role it relied on, no other holder or reservation of
/// a role it takes, each lost holder still unseen (`$seen`), its key unused, its agent still on
/// the board, each approval authority its guards were met at still at the state read
/// (`$authorities`). Then its roles, its transition (none for a recovery that moves nothing) and
/// its receipt (none for a recovery), each of those authorities' admissions counted, so an
/// observation and an admission never pass each other, its admission (`$admission`, for a guarded
/// edge), the task at its new position and status, its hold settled when its status moves, its
/// line on the thread. No `RETURN`: `Store::written` reads the task after.
pub(in crate::store) const APPLY: &str = "
LET $active = (SELECT VALUE body FROM ONLY workflow:['active']) ?? NONE;
IF $active = NONE OR $active.generation != $generation OR $active.digest != $digest { THROW 'stale' };
IF (SELECT VALUE version FROM ONLY type::record('task', $slug)) != $version { THROW 'stale' };
FOR $d IN $dependencies {
    IF ((SELECT VALUE version FROM ONLY type::record('task', $d.slug)) ?? -1) != $d.version { THROW 'stale' };
};
FOR $r IN $holding {
    IF !record::exists(type::record('role', [$slug, $r.handle, $r.role])) { THROW 'stale' };
};
FOR $s IN $seen {
    IF (SELECT VALUE last_seen FROM ONLY type::record('session', $s.handle)) != $s.last_seen { THROW 'stale' };
};
IF $receipt != NONE { IF record::exists(type::record('workflow', $receipt)) { THROW 'stale' }; };
IF $actor != NONE AND (!record::exists(type::record('session', $actor)) OR type::record('session', $actor).left_at != NONE) { THROW 'stale' };
FOR $a IN ($authorities ?? []) {
    LET $held = (SELECT VALUE body FROM ONLY type::record('workflow', ['authority', $a.adapter])) ?? NONE;
    IF $held = NONE OR $held.epoch != $a.epoch OR $held.generation != $a.generation
        OR $held.state != $a.state OR ($held.conflicted ?? false) { THROW 'stale' };
};
FOR $r IN $drop { DELETE type::record('role', [$slug, $r.handle, $r.role]) RETURN NONE; };
FOR $r IN $add {
    IF array::len(SELECT VALUE handle FROM role WHERE slug = $slug AND role IN [$r.role, 'reserved'] AND handle != $r.handle) > 0 { THROW 'stale' };
};
FOR $r IN $add { UPSERT type::record('role', [$slug, $r.handle, $r.role]) SET slug = $r.slug, handle = $r.handle, role = $r.role, since = $r.since, expires = $r.expires RETURN NONE; };
IF $transition != NONE {
    CREATE type::record('workflow', ['event', $slug, $transition.revision])
        CONTENT { kind: 'event', body: $transition, at: $transition.at } RETURN NONE;
    UPDATE type::record('task', $slug) SET status = $transition.status, fields._board.workflow = $position,
        updated = $transition.at, version += 1 RETURN NONE;
} ELSE {
    UPDATE type::record('task', $slug) SET updated = $now, version += 1 RETURN NONE;
};
IF $receipt != NONE {
    CREATE type::record('workflow', $receipt)
        CONTENT { kind: 'receipt', body: $receipt_body, at: $transition.at } RETURN NONE;
};
FOR $a IN ($authorities ?? []) {
    UPDATE type::record('workflow', ['authority', $a.adapter]) SET body.admitted = (body.admitted ?? 0) + 1 RETURN NONE;
};
IF ($admission ?? NONE) != NONE {
    CREATE type::record('workflow', ['admission', $slug, $transition.revision])
        CONTENT { kind: 'admission', body: $admission, at: $transition.at } RETURN NONE;
};
IF $moved { DELETE type::record('counter', ['hold', $slug]) RETURN NONE; };
";

/// A task filed while a custom workflow is active, placed at its initial pair in the write that
/// files it: refused as stale when another generation activated since the filing read the
/// workflow, and its first transition (`$placing`) recorded beside its position.
pub(in crate::store) const PLACED: &str = "
IF ((SELECT VALUE body.generation FROM ONLY workflow:['active']) ?? 0) != $placing.generation {
    THROW string::concat('refused: stale_activation: the workflow activated again while ', $slug, ' was filed; file it again');
};
CREATE type::record('workflow', ['event', $slug, 1])
    CONTENT { kind: 'event', body: $placing, at: $placing.at } RETURN NONE;
UPDATE type::record('task', $slug) SET fields._board.workflow = $placed RETURN NONE;";

/// A task with no position placed at `$placed` by `$placing`, its first transition (the next,
/// where a return retired its position), as a board
/// writer's move takes it out of done or cancelled: its status and its position in one write, so
/// the move that follows meets a task the workflow holds already (`workflow_position`). Read
/// again where an activation came between.
pub(in crate::store) const REOPEN_PLACED: &str = "
IF ((SELECT VALUE body.generation FROM ONLY workflow:['active']) ?? 0) != $placing.generation { THROW 'stale' };
CREATE type::record('workflow', ['event', $slug, $placing.revision])
    CONTENT { kind: 'event', body: $placing, at: $placing.at } RETURN NONE;
UPDATE type::record('task', $slug) SET status = $placing.status, fields._board.workflow = $placed RETURN NONE;
";

/// The receipt `$receipt` names, or NULL.
pub(in crate::store) const RECEIPT: &str =
    "RETURN (SELECT VALUE body FROM ONLY type::record('workflow', $receipt)) ?? NULL;";

/// The tasks a custom workflow holds on which `$actor` holds a role but the orchestrator's, sorted:
/// what its leave recovers first.
pub(in crate::store) const LEAVING: &str = "RETURN array::sort(array::distinct(
    (SELECT VALUE slug FROM role WHERE handle = $actor AND role != 'orchestrator'
        AND type::record('task', slug).fields._board.workflow != NONE)));";
