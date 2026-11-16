//! The statements of `store/stage_lines.rs`.

/// The top-level tasks made since `$since` and those done that were written since (a task closed
/// since then is one of them: its close wrote it), who executes them, and each one's thread read
/// by its own range of keys without its posts. A verdict's text is only read for its first word. The
/// roles are read by their tasks' slugs, through `role_slug`, and kept by role after, as the
/// board's snapshot reads them: one condition over both would read every role.
pub(in crate::store) const READ: &str = "
LET $tasks = (SELECT sid, slug, status, created, updated, fields._board.arm AS arm FROM task
    WHERE parent = NONE AND (created >= $since OR (status = 'done' AND updated >= $since))
    ORDER BY sid);
RETURN {
    tasks: $tasks,
    executing: (SELECT slug, role FROM role WHERE slug IN $tasks.slug)
        .filter(|$r| $r.role = 'executor').map(|$r| $r.slug),
    lines: $tasks.map(|$task| (SELECT sid, kind, ts,
        IF kind = 'system' { text } ELSE { string::slice(text, 0, 16) } AS text
        FROM message:[$task.slug, NONE]..[$task.slug, ..] WHERE kind != 'msg')),
};";

/// The status and times of each task `$slugs` names (none for a slug no task has) and the lines of
/// its thread that can change its status, read by its own range of keys: a verdict's text only for
/// its first words, the posts and the board's notices not at all. Nothing is written.
pub(in crate::store) const SINCE_READ: &str = "RETURN {
    tasks: $slugs.map(|$slug| (SELECT status, created, updated FROM type::record('task', $slug))),
    lines: $slugs.map(|$slug| (SELECT sid, kind, ts,
        IF kind = 'system' { text } ELSE { string::slice(text, 0, 16) } AS text
        FROM message:[$slug, NONE]..[$slug, ..] WHERE kind IN ['system', 'verdict'])),
};";
