//! The statements of `store/spending.rs`.

/// After [`super::sessions`]' beat, which found `$s`, the session bound to the agent: `$spent`
/// added to its row. The task is the one the agents view shows it working (an open task it holds
/// a role on, bar orchestrating: an executor's before a reviewer's before a reservation, then by
/// priority and slug); with none, no task, and its kind for the role. The round is 1 and the moves
/// into review the task's thread holds (`HANDLE moved it FROM → review`), read through the
/// thread's range of keys; 0 with no task. Needs `$finished`, `$moved` and `$into_review` beside
/// the beat's own variables.
pub(in crate::store) const SPEND: &str = "
IF $s != NONE {
    LET $working = (SELECT slug, role,
            (IF role = 'executor' { 0 } ELSE IF role = 'reviewer' { 1 } ELSE { 2 }) AS rank,
            (type::record('task', slug).priority ?? 2) AS priority,
            type::record('task', slug).status AS status
        FROM role WHERE handle = $s.handle AND role != 'orchestrator');
    LET $on = (SELECT slug, role, rank, priority FROM $working
        WHERE status != NONE AND status NOT IN $finished ORDER BY rank, priority, slug LIMIT 1)[0];
    LET $slug = $on.slug ?? '';
    LET $role = $on.role ?? $s.kind;
    LET $round = IF $on = NONE { 0 } ELSE {
        array::len(SELECT VALUE sid FROM message:[$slug, NONE]..[$slug, ..]
            WHERE kind = 'system' AND string::contains(text, $moved) AND string::ends_with(text, $into_review)) + 1
    };
    FOR $x IN $spent {
        UPSERT type::record('usage', [$s.handle, $slug, $role, $x.model, $x.speed, $round]) SET
            handle = $s.handle, slug = $slug, role = $role, model = $x.model, speed = $x.speed,
            round = $round,
            input = (input ?? 0) + $x.tokens.input,
            cache_write_5m = (cache_write_5m ?? 0) + $x.tokens.cache_write_5m,
            cache_write_1h = (cache_write_1h ?? 0) + $x.tokens.cache_write_1h,
            cache_read = (cache_read ?? 0) + $x.tokens.cache_read,
            output = (output ?? 0) + $x.tokens.output,
            calls = (calls ?? 0) + $x.tokens.calls,
            first = first ?? $now, last = $now
        RETURN NONE;
    };
};
";

/// The rows of the tokens spent on `$slugs`, by handle, task, role, model, speed and round.
pub(in crate::store) const SPENT_ON: &str = "SELECT * OMIT id FROM usage WHERE slug IN $slugs
    ORDER BY handle, slug, role, model, speed, round;";

/// The rows of the tokens spent on `$slugs` and on their subtasks (`rows`, by the table's index of
/// slugs), with each such subtask's parent (`under`).
pub(in crate::store) const SPENT_UNDER: &str = "
LET $under = (SELECT slug, parent FROM task WHERE parent IN $slugs);
RETURN {
    rows: (SELECT * OMIT id FROM usage WHERE slug IN array::concat($slugs, $under.slug)),
    under: $under,
};";

/// The rows of the tokens `$handles` spent.
pub(in crate::store) const SPENT_BY: &str = "SELECT * OMIT id FROM usage WHERE handle IN $handles;";
