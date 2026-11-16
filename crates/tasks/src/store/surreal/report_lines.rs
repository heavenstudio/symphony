//! The statements of `store/report_lines.rs`.

/// The tasks open now, whenever they were made, those written since `$since` (a task closed or made
/// since then is one of them), and those with a line since then: a landing writes its verdict on a
/// closed task without writing the task, so an approve before the window and its landing in it are
/// read (the lines by `message_ts`). Each one's thread is read by its own range of keys: a
/// verdict's text only for its first words, a post's not at all. A body is read only for a task
/// marked red, for the commits it names. Nothing is written.
pub(in crate::store) const READ: &str = "
LET $spoken = array::distinct((SELECT VALUE slug FROM message WHERE ts >= $since));
LET $tasks = (SELECT sid, slug, status, created, updated, parent, instruction,
        fields._board.red AS red, IF fields._board.red { body } ELSE { NONE } AS body FROM task
    WHERE status NOT IN ['done', 'cancelled'] OR updated >= $since OR slug IN $spoken
    ORDER BY sid);
RETURN {
    tasks: $tasks,
    lines: $tasks.map(|$task| (SELECT sid, kind, ts, sender,
        IF kind = 'system' { text } ELSE IF kind = 'verdict' { string::slice(text, 0, 16) }
            ELSE { '' } AS text
        FROM message:[$task.slug, NONE]..[$task.slug, ..])),
};";
