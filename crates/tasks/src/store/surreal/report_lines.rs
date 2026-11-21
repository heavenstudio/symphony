//! The statements of `store/report_lines.rs`.

/// The tasks open now, whenever they were made, those written since `$since` (a task closed or made
/// since then is one of them), and those with a line since then: a landing writes its verdict on a
/// closed task without writing the task, so an approve before the window and its landing in it are
/// read (the lines by `message_ts`). Each one's thread is read by its own range of keys: a
/// verdict's text only for its first words, a post's not at all. A body is read only for a task
/// marked red, for the commits it names. With them, of the tokens the board recorded
/// (`crate::usage`): the whole table summed by role, model and speed (`usage`), and the rows of
/// each top-level task of the read that is done, with its subtasks' (`spent`, by the table's
/// index of slugs; `under` names each such subtask's parent). Whether the board runs a custom
/// workflow is the active one's record, read by its key. Nothing is written.
pub(in crate::store) const READ: &str = "
LET $spoken = array::distinct((SELECT VALUE slug FROM message WHERE ts >= $since));
LET $tasks = (SELECT sid, slug, status, created, updated, parent, instruction,
        fields._board.red AS red, IF fields._board.red { body } ELSE { NONE } AS body FROM task
    WHERE status NOT IN ['done', 'cancelled'] OR updated >= $since OR slug IN $spoken
    ORDER BY sid);
LET $landed = (SELECT VALUE slug FROM $tasks WHERE status = 'done' AND !parent);
LET $under = (SELECT slug, parent FROM task WHERE parent IN $landed);
RETURN {
    usage: (SELECT role, model, speed, math::sum(input) AS input,
            math::sum(cache_write_5m) AS cache_write_5m, math::sum(cache_write_1h) AS cache_write_1h,
            math::sum(cache_read) AS cache_read, math::sum(output) AS output,
            math::sum(calls) AS calls
        FROM usage GROUP BY role, model, speed),
    spent: (SELECT slug, role, model, speed, round, input, cache_write_5m, cache_write_1h,
            cache_read, output, calls
        FROM usage WHERE slug IN array::concat($landed, $under.slug)),
    under: $under,
    tasks: $tasks,
    workflow: (SELECT VALUE body FROM ONLY workflow:['active']) ?? NULL,
    lines: $tasks.map(|$task| (SELECT sid, kind, ts, sender,
        IF kind = 'system' { text } ELSE IF kind = 'verdict' { string::slice(text, 0, 16) }
            ELSE { '' } AS text
        FROM message:[$task.slug, NONE]..[$task.slug, ..])),
};";
