//! The statements of `store/guard.rs`.

/// The approval authorities the active activation commits (`$attesters`), as each statement
/// below reads them first: none on a board with no workflow active.
macro_rules! attesters {
    () => {
        "LET $generation = (SELECT VALUE body.generation FROM ONLY workflow:['active']) ?? NONE;
LET $activation = IF $generation = NONE { NONE } ELSE {
    (SELECT VALUE body FROM ONLY type::record('workflow', ['activation', $generation])) ?? NONE
};
LET $attesters = $activation.authorities ?? [];
"
    };
}

/// The authorities the active activation commits, and the state the board accepted from each.
pub(in crate::store) const AUTHORITIES: &str = concat!(
    attesters!(),
    "RETURN {
    attesters: $attesters,
    accepted: (SELECT VALUE body FROM $attesters.map(|$a| type::record('workflow', ['authority', $a.adapter]))),
};"
);

/// What an observation from `$adapter` is decided on: the active generation, the authorities the
/// activation commits, and the state the board accepted from `$adapter` (NULL for none).
pub(in crate::store) const AUTHORITY: &str = concat!(
    attesters!(),
    "RETURN {
    generation: $generation ?? NULL,
    attesters: $attesters,
    accepted: (SELECT VALUE body FROM ONLY type::record('workflow', ['authority', $adapter])) ?? NULL,
};"
);

/// An observation accepted: `stale` (read and decide again) unless the active generation is still
/// `$generation`, whose activation committed the key it was verified with, and the state accepted
/// from `$adapter` is still `$expected` (NONE for none); then that state is `$body`.
pub(in crate::store) const OBSERVE: &str = "
LET $active = (SELECT VALUE body.generation FROM ONLY workflow:['active']) ?? NONE;
IF $active != $generation { THROW 'stale' };
LET $held = (SELECT VALUE body FROM ONLY type::record('workflow', ['authority', $adapter])) ?? NONE;
IF $held != $expected { THROW 'stale' };
UPSERT type::record('workflow', ['authority', $adapter])
    CONTENT { kind: 'authority', body: $body, at: $now } RETURN NONE;
RETURN NONE;";

/// The admission recorded for transition `$revision` of `$slug`, or NULL.
pub(in crate::store) const ADMISSION_RECORD: &str = "RETURN (SELECT VALUE body FROM ONLY type::record('workflow', ['admission', $slug, $revision])) ?? NULL;";

/// What a dispatch revalidates transition `$revision` of `$slug` on, in one read: the active
/// workflow, the task's position now, the transition and its admission as recorded, the project
/// the activation recorded, and the authorities with the state the board accepted from each.
pub(in crate::store) const DISPATCH: &str = concat!(
    attesters!(),
    "RETURN {
    active: (SELECT VALUE body FROM ONLY workflow:['active']) ?? NULL,
    found: record::exists(type::record('task', $slug)),
    position: (SELECT VALUE fields._board.workflow FROM ONLY type::record('task', $slug)) ?? NULL,
    transition: (SELECT VALUE body FROM ONLY type::record('workflow', ['event', $slug, $revision])) ?? NULL,
    admission: (SELECT VALUE body FROM ONLY type::record('workflow', ['admission', $slug, $revision])) ?? NULL,
    project: $activation.project ?? NULL,
    attesters: $attesters,
    accepted: (SELECT VALUE body FROM $attesters.map(|$a| type::record('workflow', ['authority', $a.adapter]))),
};"
);
