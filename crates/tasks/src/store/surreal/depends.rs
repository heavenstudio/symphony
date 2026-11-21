//! The statements of `store/depends.rs`.

use super::cap::GUARD;
use super::logged;
use super::work::UNBLOCKED;
use super::write::ACT;

/// Every task's status, every dependency, the count of changes to them, and the task's own version,
/// thread and executor, as `change_depends` decides on them.
pub(in crate::store) const READ: &str = "RETURN {
    tasks: (SELECT VALUE [slug, status] FROM task),
    deps: (SELECT VALUE [slug, target] FROM link WHERE kind = 'dep'),
    edits: (SELECT VALUE last FROM ONLY counter:depends) ?? 0,
    version: (SELECT VALUE version FROM ONLY type::record('task', $slug)),
    lines: (SELECT VALUE text FROM message:[$slug, NONE]..[$slug, ..] WHERE kind = 'system'),
    executing: array::len(SELECT VALUE handle FROM role WHERE slug = $slug AND role = 'executor') > 0,
};";

/// Stale unless no dependency changed anywhere, the task is the version read and what it still
/// waits on has the status read; then the links change, and the task with them.
pub(in crate::store) const CHANGE: &str = "
IF ((SELECT VALUE last FROM ONLY counter:depends) ?? 0) != $edits { THROW 'stale' };
IF (SELECT VALUE version FROM ONLY type::record('task', $slug)) != $version { THROW 'stale' };
FOR $dep IN $watch {
    IF (SELECT VALUE status FROM ONLY type::record('task', $dep.slug)) != $dep.status { THROW 'stale' };
};
UPSERT counter:depends SET last += 1 RETURN NONE;
FOR $target IN $drop { DELETE type::record('link', [$slug, 'dep', $target]); };
FOR $target IN $add {
    UPSERT type::record('link', [$slug, 'dep', $target])
        SET slug = $slug, kind = 'dep', target = $target RETURN NONE;
};
UPDATE type::record('task', $slug) SET status = $status ?? status, updated = $now, version += 1 RETURN NONE;
DELETE type::record('counter', ['hold', $slug]) RETURN NONE;";

/// [`CHANGE`] with its line on the thread (`$said`) and, when the task goes back to a status
/// (`$back`), that line too, by `$actor`, and what its block waited on removed
/// ([`UNBLOCKED`]): a session's write when `acts`, else the owner's; with the slot counters it
/// holds (`$slots`) when `guarded`.
pub(in crate::store) fn change(acts: bool, guarded: bool) -> String {
    let act = if acts { ACT } else { "" };
    let guard = if guarded { GUARD } else { "" };
    let body = format!(
        "{act}{CHANGE}{UNBLOCKED}\n{}\nIF $back != NONE {{\n{}\n}};",
        logged("$slug", "$actor", "'system'", "$said", "NONE"),
        logged("$slug", "$actor", "'system'", "$back", "NONE")
    );
    format!("{body}{guard}")
}
