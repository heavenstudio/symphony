//! The statements of `store/work.rs`.

use super::cap::{GUARD, UNHOLD, guarding};
use super::logged;
use super::write::{ACT, FENCE_RECIPIENT};
use crate::orchestration::WAITING;
use crate::red::FIELDS;
use crate::stages::{ARROW, BLOCK_LINE, MOVED};

/// A move's write of its reason's line (`$move_reason`, [`reason_line`]) by `$actor` on `$slug`,
/// after the move's own line.
pub(in crate::store) fn reason_written() -> String {
    logged("$slug", "$actor", "'msg'", "$move_reason", "NONE")
}

/// The task, its dependencies and parent there, its assignee a session, and a slug nobody took.
pub(in crate::store) const CREATE: &str = "
IF record::exists(type::record('task', $slug)) { THROW $taken };
FOR $other IN $needed { IF !record::exists(type::record('task', $other.slug)) { THROW $other.missing }; };
IF $assignee != NONE AND !record::exists(type::record('session', $assignee)) { THROW $unassignable };
LET $sid = (UPSERT ONLY counter:task SET last += 1).last;
CREATE type::record('task', $slug) CONTENT {
    sid: $sid, slug: $slug, title: $title, body: $body, status: $status, priority: $priority, parent: $parent,
    created_by: $actor, created: $now, updated: $now, instruction: $instruction,
} RETURN NONE;
IF $assignee != NONE {
    CREATE type::record('role', [$slug, $assignee, 'reserved'])
        CONTENT { slug: $slug, handle: $assignee, role: 'reserved', since: $now } RETURN NONE;
};
FOR $link IN $links {
    UPSERT type::record('link', [$slug, $link.kind, $link.target])
        SET slug = $slug, kind = $link.kind, target = $link.target RETURN NONE;
};";

/// Every task's slug and parent: the tree, read whole.
pub(in crate::store) const TREE: &str = "SELECT VALUE [slug, parent] FROM task";

/// The tree as `set_parent` decides on it, and the count of moves it was read at.
pub(in crate::store) const TREE_READ: &str = "RETURN {
    tree: (SELECT VALUE [slug, parent] FROM task),
    moves: (SELECT VALUE last FROM ONLY counter:tree) ?? 0,
    me: (SELECT left_at FROM ONLY type::record('session', $actor)),
    known: record::exists(type::record('session', $actor)),
};";

/// No task moved under another since the tree was read.
pub(in crate::store) const MOVE_UNDER: &str = "
IF ((SELECT VALUE last FROM ONLY counter:tree) ?? 0) != $moves { THROW 'stale' };
UPSERT counter:tree SET last += 1 RETURN NONE;
UPDATE type::record('task', $slug) SET parent = $parent, updated = $now, version += 1 RETURN NONE;
DELETE type::record('counter', ['hold', $slug]) RETURN NONE;";

/// What [`Store::run_for`] reads, in one statement: the sessions (not left) whose in-process
/// binding places their last tool call in the run's checkout (`here`), the full session its Claude
/// Code session binds (`called`), each with what its health is judged on, the roles any of them or
/// the handle asked holds on work another session could take over, the tree, and the branch's task's
/// status.
pub(in crate::store) fn run_for_read() -> String {
    format!(
        "
LET $here = IF $checkout = NONE {{ [] }} ELSE {{
    (SELECT handle, last_seen, busy_until, pid, host FROM session WHERE left_at = NONE AND agent != NONE
        AND type::record('callback_state', ['binding', agent]).in_process = true
        AND type::record('callback_state', ['binding', agent]).worktree = $checkout)
}};
LET $called = IF $claude = NONE {{ [] }} ELSE {{
    (SELECT handle, last_seen, busy_until, pid, host FROM session WHERE left_at = NONE AND agent = $claude
        AND (type::record('callback_state', ['binding', agent]).in_process ?? false) = false)
}};
LET $who = array::union(array::union(IF $asked = NONE {{ [] }} ELSE {{ [$asked] }}, $here.handle),
    $called.handle);
RETURN {{
    here: $here,
    called: $called,
    held: (SELECT handle, slug FROM role WHERE handle IN $who AND {}),
    tree: ({TREE}),
    branch: IF $branch = NONE {{ NONE }} ELSE {{ type::record('task', $branch).status }},
}};",
        super::holding!()
    )
}

/// Cancellation does not satisfy a prerequisite. Block its unfinished dependents, or the task
/// being created/moved if one of its prerequisites was cancelled earlier, in the same transaction.
/// Already blocked work still needs each new cancellation's reason on its thread. The line ends with
/// the status the dependent held, `(was ready)`, which `depends` gives it back. A dependent a
/// custom workflow holds is its workflow's: only its edges move it, whose dependency predicates
/// refuse it while the prerequisite is cancelled (docs/workflow/WORKFLOW.md).
pub(in crate::store) fn block_cancelled_dependencies(cancelling: bool) -> String {
    // Ordinary moves/creates use link_slug's index; only cancellation needs the reverse lookup.
    let scope = if cancelling {
        "target = $slug"
    } else {
        "slug = $slug"
    };
    format!(
        "LET $blocked_links = SELECT slug, target FROM link WHERE kind = 'dep'
            AND {scope}
            AND type::record('task', target).status = 'cancelled'
            AND type::record('task', slug).status NOT IN ['done', 'cancelled']
            AND type::record('task', slug).fields._board.workflow = NONE
            ORDER BY slug, target;
FOR $dependent IN array::distinct($blocked_links.slug) {{
    LET $cancelled = $blocked_links.filter(|$link| $link.slug = $dependent).target;
    LET $was = (SELECT VALUE status FROM ONLY type::record('task', $dependent));
    UPDATE type::record('task', $dependent) SET status = 'blocked', updated = $now, version += 1 RETURN NONE;
    DELETE type::record('counter', ['hold', $dependent]) RETURN NONE;
    LET $reason = '{BLOCK_LINE}' + array::join($cancelled, ', ') + ' (was ' + $was + ')';
    {}
}};",
        logged("$dependent", "$actor", "'system'", "$reason", "NONE")
    )
}

/// The write of [`Change::needs`] after the rule's own ([`super::super::WRITE`]).
pub(in crate::store) const NEEDS_WRITTEN: &str =
    "\nUPDATE type::record('task', $slug) SET fields._board.needs = $needs RETURN NONE;";

/// The write of [`Change::gates`] after the rule's own ([`super::super::WRITE`]).
pub(in crate::store) const GATES_WRITTEN: &str =
    "\nUPDATE type::record('task', $slug) SET fields._board.gates = $gates RETURN NONE;";

/// The write of [`Change::unlanded`] after the rule's own: the mark of a task approved and not
/// landed set to when its approve was given, or lifted where `$unlanded` is null; a task that
/// carries none is left as it is.
pub(in crate::store) const UNLANDED_WRITTEN: &str = "\nUPDATE type::record('task', $slug) SET fields._board.unlanded = $unlanded ?? NONE WHERE ($unlanded ?? NONE) != NONE OR fields._board.unlanded != NONE RETURN NONE;";

/// The write of what a block waits on (`$blocked`, [`crate::waits_on::Blocked`]) after the rule's
/// own, which moved the task to blocked or found it there.
pub(in crate::store) const BLOCKED_WRITTEN: &str =
    "\nUPDATE type::record('task', $slug) SET fields._board.blocked = $blocked RETURN NONE;";

/// What a block waited on removed from `$slug` once a write took it out of blocked: after every
/// write of its status but one that blocks it.
pub(in crate::store) const UNBLOCKED: &str = "\nUPDATE type::record('task', $slug) SET fields._board.blocked = NONE WHERE status != 'blocked' AND fields._board.blocked != NONE RETURN NONE;";

/// The write's refusal unless its actor is a red's filer: `$filer` and `$unfiled` ([`filing`]).
pub(in crate::store) const FILED: &str = "IF (SELECT VALUE kind FROM ONLY type::record('session', $actor)) != $filer { THROW $unfiled };";

/// [`Store::mark_red`]'s write: what it judges is read in the transaction that writes, so two marks
/// at once post one line.
pub(in crate::store) const MARK_RED: &str = "
LET $task = (SELECT status, priority, fields._board.red AS red FROM ONLY type::record('task', $slug));
IF $task = NONE { THROW $missing };
IF $task.status IN ['done', 'cancelled'] {
    LET $refusal = $closed + $task.status + ': only an open task is marked red';
    THROW $refusal;
};";

/// A red task's mark, written with the task by its filer alone ([`super::red`]).
pub(in crate::store) const RED: &str =
    "UPDATE type::record('task', $slug) SET fields._board.red = $red RETURN NONE;";

/// The arm a task is filed on, written with the task ([`crate::arm`]).
pub(in crate::store) const ARM: &str =
    "UPDATE type::record('task', $slug) SET fields._board.arm = $arm RETURN NONE;";

/// The checks a pass's write makes first, before any statement of its own moves a version or a
/// `last_seen`: each task as read, each dropped holder as read. Needs `$fenced` and `$seen`.
pub(in crate::store) const FENCES: &str = "
FOR $g IN $fenced {
    IF (SELECT VALUE version FROM ONLY type::record('task', $g.slug)) != $g.version { THROW 'stale' };
};
FOR $s IN $seen {
    IF (SELECT VALUE last_seen FROM ONLY type::record('session', $s.handle)) != $s.last_seen { THROW 'stale' };
};";

/// A pass's writes, after its [`FENCES`]: each receiver still there, fenced as a recipient is, its
/// slot counter where it was read (`Pass::guard`); each give's role, version, slot counter and
/// line, sent by `$sender`; each mark set or lifted. Needs `$receivers`, `$counters`, `$gives`,
/// `$marks`, `$unmarks`, `$sender` and `$now`.
pub(in crate::store) fn gives() -> String {
    format!(
        "
FOR $handle IN $receivers {{
    IF !record::exists(type::record('session', $handle)) OR type::record('session', $handle).left_at != NONE {{ THROW 'stale' }};
    {FENCE_RECIPIENT}
}};
FOR $c IN $counters {{
    IF ((SELECT VALUE last FROM ONLY type::record('counter', $c.key)) ?? 0) != $c.last {{ THROW 'stale' }};
}};
FOR $g IN $gives {{
    FOR $h IN $g.drop {{ DELETE type::record('role', [$g.slug, $h, 'orchestrator']) RETURN NONE; }};
    UPSERT type::record('role', [$g.slug, $g.to, 'orchestrator'])
        SET slug = $g.slug, handle = $g.to, role = 'orchestrator', since = $now, expires = NONE RETURN NONE;
    UPDATE type::record('task', $g.slug) SET version += 1 RETURN NONE;
    UPSERT type::record('counter', ['slots', $g.to]) SET last += 1 RETURN NONE;
    {}
}};
FOR $slug IN $marks {{
    UPDATE type::record('task', $slug) SET version += 1, fields.{FIELDS}.{WAITING} = $now RETURN NONE;
}};
FOR $slug IN $unmarks {{
    UPDATE type::record('task', $slug) SET version += 1, fields.{FIELDS}.{WAITING} = NONE RETURN NONE;
}};",
        logged("$g.slug", "$sender", "'system'", "$g.line", "NONE")
    )
}

/// What `Store::gaining` reads of `$handle`: its slot counter, and the status and red mark of each
/// task it holds the orchestrator role on: its roles read through the index of a role's holder
/// alone and narrowed after, as a leave reads them (a condition on the role's kind as well has the
/// server read `role_role` beside it, every orchestrator role on the board), then each task by id.
pub(in crate::store) const GAINING: &str =
    "LET $mine = (SELECT slug, role FROM role WHERE handle = $handle)
    .filter(|$r| $r.role = 'orchestrator').map(|$r| $r.slug);
RETURN {
    last: (SELECT VALUE last FROM ONLY type::record('counter', ['slots', $handle])) ?? 0,
    tasks: (SELECT status, fields._board.red AS red FROM $mine.map(|$s| type::record('task', $s))),
};";

/// Sets `$slug`'s priority to `$priority` and says so, with the one it replaced, on its thread
/// (`$missing` refuses a task not there), by `$actor`: a session's write when `acts`, else the
/// owner's.
pub(in crate::store) fn set_priority(acts: bool) -> String {
    let act = if acts { ACT } else { "" };
    format!(
        "{act}
LET $task = (SELECT priority FROM ONLY type::record('task', $slug));
IF $task = NONE {{ THROW $missing }};
UPDATE type::record('task', $slug) SET priority = $priority, version += 1 RETURN NONE;
LET $said = $actor + ' changed priority p' + <string>$task.priority + ' → p' + <string>$priority;
{}",
        logged("$slug", "$actor", "'system'", "$said", "NONE")
    )
}

/// Retitles `$slug` (`$title`), links it to each of `$links` and says a changed title on its thread
/// (`$missing` refuses a task not there), by `$actor`: a session's write when `acts`, else the
/// owner's.
pub(in crate::store) fn amend(acts: bool) -> String {
    let act = if acts { ACT } else { "" };
    format!(
        "{act}
LET $task = (SELECT title FROM ONLY type::record('task', $slug));
IF $task = NONE {{ THROW $missing }};
FOR $link IN $links {{
    UPSERT type::record('link', [$slug, 'doc', $link])
        SET slug = $slug, kind = 'doc', target = $link RETURN NONE;
}};
UPDATE type::record('task', $slug) SET title = $title, version += 1 RETURN NONE;
IF $task.title != $title {{
    LET $said = $actor + ' retitled it: ' + $task.title + ' → ' + $title;
    {}
}};",
        logged("$slug", "$actor", "'system'", "$said", "NONE")
    )
}

/// [`MOVE_UNDER`] with its line on the thread (`$said`), by `$actor`: a session's write when
/// `acts`, else the owner's; with the slot counters it holds (`$slots`) when `guarded`; and, when
/// `gives`, the give the move makes in the same write, its [`FENCES`] first and its writes
/// ([`gives`]) last.
pub(in crate::store) fn set_parent(acts: bool, guarded: bool, gives: bool) -> String {
    let act = if acts { ACT } else { "" };
    let guard = if guarded { GUARD } else { "" };
    if gives {
        format!(
            "{act}{FENCES}{MOVE_UNDER}\n{}{guard}{}",
            logged("$slug", "$actor", "'system'", "$said", "NONE"),
            self::gives()
        )
    } else {
        format!(
            "{act}{MOVE_UNDER}\n{}{guard}",
            logged("$slug", "$actor", "'system'", "$said", "NONE")
        )
    }
}

/// The tree ([`TREE`]), as a read of its own.
pub(in crate::store) fn tree() -> String {
    format!("RETURN {TREE};")
}

/// The owner's move of `$slug` from one of `$from` to `$status`, its line on the thread, its
/// reason's (`$move_reason`) when `reason`, and its dependants blocked (`cancelling`: the move
/// cancels it); fenced on `$version` when `fenced` and holding `$slots` when `guarded`. Refuses
/// with `$missing`, `$cannot` and `$instruction`.
pub(in crate::store) fn move_as_owner(
    reason: bool,
    cancelling: bool,
    fenced: bool,
    guarded: bool,
) -> String {
    let body = format!(
        "LET $task = (SELECT status, instruction, parent FROM ONLY type::record('task', $slug));
IF $task = NONE {{ THROW $missing }};
IF $task.status NOT IN $from {{
    LET $refusal = $cannot + $task.status + ' to ' + $status;
    THROW $refusal;
}};
IF $status = 'review' AND ($task.instruction OR $task.parent != NONE) {{ THROW $instruction }};
UPDATE type::record('task', $slug) SET status = $status, updated = $now, version += 1 RETURN NONE;{UNBLOCKED}
{UNHOLD}
LET $said = $actor + '{MOVED}' + $task.status + '{ARROW}' + $status;
{}\n{}\n{}",
        logged("$slug", "$actor", "'system'", "$said", "NONE"),
        if reason {
            reason_written()
        } else {
            String::new()
        },
        block_cancelled_dependencies(cancelling)
    );
    guarding(fenced, &body, guarded)
}

/// Whether `$slug` is a task, the tasks right under it, the lines of its thread that record an
/// executor or an orchestrator, and who holds those roles now.
pub(in crate::store) const EVER_HELD: &str = "RETURN {
            known: record::exists(type::record('task', $slug)),
            below: (SELECT slug, instruction FROM task WHERE parent = $slug),
            lines: (SELECT sender, text FROM message:[$slug, NONE]..[$slug, ..]
                WHERE kind = 'system' AND (string::starts_with(text, sender + ' claimed it')
                    OR text = sender + ' joined as executor'
                    OR string::contains(text, ' orchestrator'))),
            holders: (SELECT handle, role FROM role
                WHERE slug = $slug AND role IN ['executor', 'orchestrator']),
        };";

/// [`MARK_RED`] by a red's filer ([`FILED`]): `$slug` marked red (`$red`) and most urgent, said on
/// its thread, unless it is both already.
pub(in crate::store) fn mark_red() -> String {
    format!(
        "{ACT}{MARK_RED}
{FILED}
IF !$task.red OR $task.priority > 0 {{
    UPDATE type::record('task', $slug) SET fields._board.red = $red, priority = 0, version += 1 RETURN NONE;
    LET $said = $actor + ' marked it red (' + $red + ') and most urgent, as a red is filed';
    {}
}};",
        logged("$slug", "$actor", "'system'", "$said", "NONE")
    )
}

/// A red's filer ([`FILED`]) lifting `$slug`'s red mark, said on its thread, unless it has none or
/// is closed (`$missing` refuses a task not there); fenced on `$version` when `fenced` and holding
/// `$slots` when `guarded`.
pub(in crate::store) fn unmark_red(fenced: bool, guarded: bool) -> String {
    let body = format!(
        "{ACT}
LET $task = (SELECT status, fields._board.red AS red FROM ONLY type::record('task', $slug));
IF $task = NONE {{ THROW $missing }};
{FILED}
IF $task.red AND $task.status NOT IN ['done', 'cancelled'] {{
    UPDATE type::record('task', $slug) SET fields._board.red = NONE, version += 1 RETURN NONE;
    LET $said = $actor + ' lifted its red mark: red no more, it and the open work under it take a slot as any work does';
    {}
}};",
        logged("$slug", "$actor", "'system'", "$said", "NONE")
    );
    guarding(fenced, &body, guarded)
}

/// An owner task's save receipt (`$owner_save`), written with the task.
pub(in crate::store) const OWNER_SAVE: &str =
    "UPDATE type::record('task', $slug) SET fields._symphony.owner_save = $owner_save RETURN NONE;";

/// The owner's tasks saved under the receipt `$receipt`, two at most: one is the save, two say it
/// is not known which.
pub(in crate::store) const OWNER_SAVED: &str = "SELECT VALUE slug FROM task WHERE created_by = 'owner' AND fields._symphony.owner_save = $receipt LIMIT 2;";

/// A filing as the board's own, refused (`$unplaced`) while a custom workflow is active: it is
/// filed again at the workflow's initial pair (`super::workflow::PLACED`).
const UNPLACED: &str = "IF record::exists(workflow:['active']) { THROW $unplaced };\n";

/// What a new task's write turns on.
pub(in crate::store) struct Creating {
    /// A session files it, not the owner.
    pub(in crate::store) acts: bool,
    /// It keeps an owner's save receipt ([`OWNER_SAVE`]).
    pub(in crate::store) receipt: bool,
    /// Its filer marks it red ([`FILED`], [`RED`]).
    pub(in crate::store) red: bool,
    /// It is filed on an arm ([`ARM`]).
    pub(in crate::store) arm: bool,
    /// A custom workflow is active, and it starts at the initial pair (`super::workflow::PLACED`).
    pub(in crate::store) placed: bool,
}

/// A new task ([`CREATE`]) with the marks `new` names, its line on the thread (`$said`) and, where
/// a task it waits on is cancelled, its block.
pub(in crate::store) fn create(new: &Creating) -> String {
    let act = if new.acts { ACT } else { "" };
    let stamp = if new.receipt { OWNER_SAVE } else { "" };
    let marked = if new.red {
        format!("{FILED}\n{RED}")
    } else {
        String::new()
    };
    let arm_mark = if new.arm { ARM } else { "" };
    let (unplaced, placed) = if new.placed {
        ("", super::workflow::PLACED)
    } else {
        (UNPLACED, "")
    };
    format!(
        "{act}{unplaced}{CREATE}\n{placed}\n{stamp}\n{marked}\n{arm_mark}\n{}\n{}",
        logged("$slug", "$actor", "'system'", "$said", "NONE"),
        block_cancelled_dependencies(false)
    )
}

/// Of `$candidates`, the sessions that listen: a channel registered, or a reader of `tasks wait`.
pub(in crate::store) const LISTENING: &str = "RETURN array::union(
                    (SELECT VALUE handle FROM $candidates.map(|$h| type::record('callback_state', $h)) WHERE address != NONE),
                    ((SELECT VALUE waiting FROM ONLY callback_state:chat) ?? []).filter(|$h| $h IN $candidates)
                );";

/// A pass of gives as one write: its [`FENCES`], then its writes ([`gives`]), by a session
/// (`$actor`) when `acts`, else the board's own.
pub(in crate::store) fn give_pass(acts: bool) -> String {
    let act = if acts { ACT } else { "" };
    format!("{act}{FENCES}{}\nRETURN NONE;", gives())
}
