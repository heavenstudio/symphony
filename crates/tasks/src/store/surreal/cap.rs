//! The statements of `store/cap.rs`.

use super::branches::{OF_TASKS, ON_BRANCHES};

/// The open tasks, through `task_status`, each with its red mark ([`crate::red`]).
pub(in crate::store) const OPEN_TASKS: &str = "SELECT slug, parent, status, instruction, fields._board.red AS red FROM task WHERE status IN $slot_statuses";

/// The tasks `$slot_wanted` names, by id.
pub(in crate::store) const BY_ID: &str = "SELECT slug, parent, status, instruction, fields._board.red AS red FROM $slot_wanted.map(|$s| type::record('task', $s))";

/// Their roles, through `role_slug`: a task holds a handful, and naming the two the cap reads would
/// let the server take an index of every row holding one of them instead.
pub(in crate::store) const ROLES_OF: &str =
    "SELECT slug, handle, role FROM role WHERE slug IN $slot_wanted";

/// Every slot counter, a range of record ids.
pub(in crate::store) const COUNTERS: &str =
    "SELECT VALUE [record::id(id)[1], last] FROM counter:['slots', NONE]..['slots', ..]";

/// Every red's counter ([`crate::slots::reds`]), a range of record ids.
pub(in crate::store) const RED_COUNTERS: &str =
    "SELECT VALUE [record::id(id)[1], last] FROM counter:['red', NONE]..['red', ..]";

/// Every hold, its task and the stamp it was made at, a range of record ids.
pub(in crate::store) const HOLDS: &str =
    "SELECT VALUE [record::id(id)[1], ts] FROM counter:['hold', NONE]..['hold', ..]";

/// The tasks `$slot_holds` holds, by id, with the stamp each hold must match.
pub(in crate::store) const HELD_TASKS: &str = "SELECT slug, parent, status, instruction, fields._board.red AS red, updated FROM $slot_holds.map(|$h| type::record('task', $h[0]))";

/// Every statement the cap sends, which `tests/store/slots/cost.rs` has the server plan.
pub const STATEMENTS: [&str; 9] = [
    OPEN_TASKS,
    BY_ID,
    ROLES_OF,
    COUNTERS,
    HOLDS,
    HELD_TASKS,
    RED_COUNTERS,
    OF_TASKS,
    ON_BRANCHES,
];

/// The shared slot inputs before a snapshot's `RETURN`. `wanted` names the task being changed
/// (`[$slug]`) or every task in a whole-board holdings observation (`$slugs`).
pub(in crate::store) fn lets(wanted: &str) -> String {
    format!(
        "LET $slot_open = ({OPEN_TASKS}); LET $slot_holds = ({HOLDS}); LET $slot_held = ({HELD_TASKS});
         LET $slot_wanted = array::union(array::union($slot_open.slug, $slot_held.slug), {wanted});"
    )
}

/// The shared snapshot field for admission and read-only holdings.
pub(in crate::store) fn field() -> String {
    format!(
        "slots: {{ open: $slot_open, holds: $slot_holds, held: $slot_held, roles: ({ROLES_OF}), counted: ({COUNTERS}), reds: ({RED_COUNTERS}) }},"
    )
}

/// The write's hold on each counter it read (`$slots`: its key, `['slots', HANDLE]` or `['red',
/// SLUG]`, and `last`): stale if another write moved it since, then moved on.
pub(in crate::store) const GUARD: &str = "
FOR $s IN $slots {
    IF ((SELECT VALUE last FROM ONLY type::record('counter', $s.key)) ?? 0) != $s.last { THROW 'stale' };
    UPSERT type::record('counter', $s.key) SET last = $s.last + 1 RETURN NONE;
};";

/// The end of `$slug`'s hold, in a write that moves it or changes its parent or dependencies.
pub(in crate::store) const UNHOLD: &str =
    "DELETE type::record('counter', ['hold', $slug]) RETURN NONE;";

/// A rule's write's part in a hold: a move (`$status`) ends one, and the hold's own move (`$hold`)
/// makes one, stamped with the task's `updated` as the write left it (`$updated`).
pub(in crate::store) const HOLDING: &str = "
IF $status { DELETE type::record('counter', ['hold', $slug]) RETURN NONE; };
IF $hold { UPSERT type::record('counter', ['hold', $slug]) SET last = 0, ts = $updated RETURN NONE; };";

/// A write's refusal to go on from a task that moved since the read its counters were judged
/// on (`$version`): a red marked since, whose close then holds counters, among them. The board
/// reads and decides again.
pub(in crate::store) const FENCE: &str = "IF (SELECT VALUE version FROM ONLY type::record('task', $slug)) != $version { THROW 'stale' };\n";

/// A level of tasks above, read by id with their roles and recorded branches: the tasks
/// `$slot_wanted` names and those on the branches it names.
pub(in crate::store) fn level() -> String {
    format!(
        "LET $slot_wanted = array::union($slot_wanted, ({ON_BRANCHES})); RETURN {{ tasks: ({BY_ID}), roles: ({ROLES_OF}), branches: ({OF_TASKS}) }};"
    )
}

/// `body` fenced on the version read ([`FENCE`], `$version`) when `fenced`, and guarded by the
/// counters held ([`GUARD`], `$slots`) when `guarded`.
pub(in crate::store) fn guarding(fenced: bool, body: &str, guarded: bool) -> String {
    let fence = if fenced { FENCE } else { "" };
    let guard = if guarded { GUARD } else { "" };
    format!("{fence}{body}{guard}")
}
