//! The statements of `store/admission/effects.rs`.

/// The intent the prepare of `$slug` at workflow revision `$revision` recorded, or NULL.
pub(in crate::store) const INTENT: &str = "RETURN (SELECT VALUE body FROM ONLY type::record('workflow', ['intent', $slug, $revision])) ?? NULL;";

/// The transition that made workflow revision `$revision` of `$slug`, or NULL.
pub(in crate::store) const TRANSITION: &str = "RETURN (SELECT VALUE body FROM ONLY type::record('workflow', ['event', $slug, $revision])) ?? NULL;";

/// What a dispatch or a finalize decides on, in one read: the intent, the active generation, and
/// the task's version and position, each NULL when it is not there.
pub(in crate::store) const STANDING: &str = "
RETURN {
    intent: (SELECT VALUE body FROM ONLY type::record('workflow', ['intent', $slug, $revision])) ?? NULL,
    active: (SELECT VALUE body FROM ONLY workflow:['active']) ?? NULL,
    task: (SELECT version, fields._board.workflow ?? NULL AS position
        FROM ONLY type::record('task', $slug)) ?? NULL,
};";

/// After `super::workflow::APPLY`, in the write that admits a prepare edge: its intent, recorded
/// once beside its transition.
pub(in crate::store) const PREPARED: &str = "
CREATE type::record('workflow', ['intent', $slug, $transition.revision])
    CONTENT { kind: 'intent', body: $intent, at: $transition.at } RETURN NONE;
";

/// The intent moved on to `$intent` from the stage it was read at (`$stage_was`), `stale` when
/// another write moved it first or took its finalize edge: after `super::workflow::APPLY`, in the
/// write that takes the finalize edge, or in [`moved`].
pub(in crate::store) const SETTLED: &str = "
LET $held = (SELECT VALUE body FROM ONLY type::record('workflow', ['intent', $slug, $intent.revision]));
IF $held = NONE OR $held.stage != $stage_was OR ($held.finalized ?? NONE) != NONE { THROW 'stale' };
UPDATE type::record('workflow', ['intent', $slug, $intent.revision]) SET body = $intent RETURN NONE;
";

/// An intent moved on with no transition of its task ([`SETTLED`]), `stale` unless the task is at
/// the version and the workflow at the generation and digest its decision read (0 and nothing for
/// a board with no workflow active); then `logged`, its line on the thread, if it says one.
pub(in crate::store) fn moved(logged: &str) -> String {
    format!(
        "
IF ((SELECT VALUE version FROM ONLY type::record('task', $slug)) ?? -1) != $version {{ THROW 'stale' }};
LET $active = (SELECT VALUE body FROM ONLY workflow:['active']) ?? NONE;
IF ($active.generation ?? 0) != $generation OR ($active.digest ?? '') != $digest {{ THROW 'stale' }};
{SETTLED}{logged}
RETURN NONE;"
    )
}
