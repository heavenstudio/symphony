//! The statements of `store/work.rs` on PostgreSQL.

use super::cap::{GUARD, UNHOLD, guarding};
use super::write::{SEEN, fenced};
use super::{Pg, act, logged, rows};
use crate::orchestration::WAITING;
use crate::red::FIELDS;
use crate::stages::{ARROW, BLOCK_LINE, MOVED};
use crate::store::surreal::work::Creating;
use std::borrow::Cow;

/// The task, its dependencies and parent there, its assignee a session, and a slug nobody took.
const CREATE: &str = "IF EXISTS (SELECT 1 FROM task WHERE slug = v ->> 'slug') THEN \
    RAISE EXCEPTION '%', v ->> 'taken'; \
END IF; \
IF EXISTS (SELECT 1 FROM jsonb_array_elements(v -> 'needed') AS needed (other) \
        WHERE NOT EXISTS (SELECT 1 FROM task WHERE slug = needed.other ->> 'slug')) THEN \
    RAISE EXCEPTION '%', (SELECT needed.other ->> 'missing' FROM jsonb_array_elements(v -> 'needed') \
        WITH ORDINALITY AS needed (other, at) \
        WHERE NOT EXISTS (SELECT 1 FROM task WHERE slug = needed.other ->> 'slug') \
        ORDER BY needed.at LIMIT 1); \
END IF; \
IF v ? 'assignee' AND NOT EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'assignee') THEN \
    RAISE EXCEPTION '%', v ->> 'unassignable'; \
END IF; \
INSERT INTO counter (id, last) VALUES ('\"task\"', 1) \
    ON CONFLICT (id) DO UPDATE SET last = counter.last + 1 \
    RETURNING last INTO sid_; \
INSERT INTO task (id, sid, slug, title, body, status, priority, parent, created_by, created, updated, instruction) \
    VALUES (v -> 'slug', sid_, v ->> 'slug', v ->> 'title', v ->> 'body', v ->> 'status', \
        (v ->> 'priority')::bigint, v ->> 'parent', v ->> 'actor', (v ->> 'now')::double precision, \
        (v ->> 'now')::double precision, (v ->> 'instruction')::boolean) \
    ON CONFLICT DO NOTHING; \
IF NOT FOUND THEN \
    RAISE EXCEPTION 'stale'; \
END IF; \
IF v ? 'assignee' THEN \
    INSERT INTO role (id, slug, handle, role, since) \
        VALUES (jsonb_build_array(v -> 'slug', v -> 'assignee', 'reserved'), v ->> 'slug', \
            v ->> 'assignee', 'reserved', (v ->> 'now')::double precision); \
END IF; \
INSERT INTO link (id, slug, kind, target) \
    SELECT DISTINCT jsonb_build_array(v -> 'slug', l.link -> 'kind', l.link -> 'target'), v ->> 'slug', \
        l.link ->> 'kind', l.link ->> 'target' \
    FROM jsonb_array_elements(v -> 'links') AS l (link) \
    ON CONFLICT (id) DO NOTHING;";

/// `v ->> 'slug'`'s task with `path` of its fields set to `value`, a `jsonb` expression.
pub(in crate::store) fn field_set(path: &str, value: &str) -> String {
    format!(
        "UPDATE task SET fields = board_fields(id, fields, '{{{path}}}', {value}) WHERE slug = v ->> 'slug';"
    )
}

/// What a rule's write sets the mark of a task approved and not landed to: when its approve was
/// given, or nothing, which lifts it, where the change's `unlanded` is null.
pub(in crate::store) const UNLANDED: &str = "nullif(v -> 'unlanded', 'null'::jsonb)";

/// A filing as the board's own, refused (`unplaced`) while a custom workflow is active: it is
/// filed again at the workflow's initial pair (`super::workflow::PLACED`).
const UNPLACED: &str = "IF EXISTS (SELECT 1 FROM workflow WHERE id = '[\"active\"]'::jsonb) THEN \
    RAISE EXCEPTION '%', v ->> 'unplaced'; \
END IF;";

/// The write's refusal unless its actor is a red's filer: `filer` and `unfiled`.
pub(in crate::store) const FILED: &str = "IF (SELECT kind FROM session WHERE handle = v ->> 'actor') IS DISTINCT FROM v ->> 'filer' THEN \
    RAISE EXCEPTION '%', v ->> 'unfiled'; \
END IF;";

/// A new task ([`CREATE`]) with the marks `new` names, its line on the thread (`said`) and, where
/// a task it waits on is cancelled, its block (`super::super::surreal::work::create`).
pub(in crate::store) fn create(new: &Creating) -> String {
    let mut block = String::new();
    if new.acts {
        block.push_str(&act());
        block.push('\n');
    }
    if !new.placed {
        block.push_str(UNPLACED);
        block.push('\n');
    }
    block.push_str(CREATE);
    block.push('\n');
    if new.placed {
        block.push_str(super::workflow::PLACED);
        block.push('\n');
    }
    if new.receipt {
        block.push_str(&field_set("_symphony,owner_save", "v -> 'owner_save'"));
        block.push('\n');
    }
    if new.red {
        block.push_str(FILED);
        block.push('\n');
        block.push_str(&field_set("_board,red", "v -> 'red'"));
        block.push('\n');
    }
    if new.arm {
        block.push_str(&field_set("_board,arm", "v -> 'arm'"));
        block.push('\n');
    }
    if new.kind {
        block.push_str(&field_set("_board,kind", "v -> 'kind'"));
        block.push('\n');
    }
    block.push_str(&said("v ->> 'said'"));
    block.push('\n');
    block.push_str(&block_cancelled_dependencies(false));
    block
}

/// Cancellation does not satisfy a prerequisite: the unfinished dependents of a cancelled task
/// blocked, with the line that says so and the status each held (`super::super::surreal::work`'s
/// own). `cancelling`: the task cancelled is `v ->> 'slug'`; else it is the one created or moved.
pub(in crate::store) fn block_cancelled_dependencies(cancelling: bool) -> String {
    let scope = if cancelling {
        "l.target = v ->> 'slug'"
    } else {
        "l.slug = v ->> 'slug'"
    };
    format!(
        "DECLARE \
    dependent_ record; \
    was_ text; \
BEGIN \
    FOR dependent_ IN SELECT l.slug, string_agg(l.target, ', ' ORDER BY l.target) AS cancelled \
        FROM link l JOIN task t ON t.slug = l.target JOIN task d ON d.slug = l.slug \
        WHERE l.kind = 'dep' AND {scope} AND t.status = 'cancelled' \
            AND d.status NOT IN ('done', 'cancelled') AND d.fields #> '{{_board,workflow}}' IS NULL \
        GROUP BY l.slug ORDER BY l.slug \
    LOOP \
        SELECT status INTO was_ FROM task WHERE slug = dependent_.slug; \
        UPDATE task SET status = 'blocked', updated = (v ->> 'now')::double precision, \
            version = version + 1 WHERE slug = dependent_.slug; \
        DELETE FROM counter WHERE id = jsonb_build_array('hold', dependent_.slug); \
        {} \
    END LOOP; \
END;",
        logged(
            "dependent_.slug",
            "v ->> 'actor'",
            "'system'",
            &format!("'{BLOCK_LINE}' || dependent_.cancelled || ' (was ' || was_ || ')'"),
            "NULL",
        )
    )
}

/// `v ->> 'slug'`'s priority set to `priority` by `actor`, a session's write when `acts`, with the
/// line that names both values (`missing` refuses a task not there).
pub(in crate::store) fn set_priority(acts: bool) -> String {
    let act = if acts { act() } else { String::new() };
    format!(
        "{act} \
DECLARE \
    was_ bigint; \
BEGIN \
    SELECT priority INTO was_ FROM task WHERE slug = v ->> 'slug'; \
    IF NOT FOUND THEN \
        RAISE EXCEPTION '%', v ->> 'missing'; \
    END IF; \
    UPDATE task SET priority = (v ->> 'priority')::bigint, version = version + 1 \
        WHERE slug = v ->> 'slug'; \
    {} \
END;",
        said("(v ->> 'actor') || ' changed priority p' || was_ || ' → p' || (v ->> 'priority')")
    )
}

/// `v ->> 'slug'` given the kind `kind` by `actor`, a session's write when `acts`, with the line
/// that names it and the one it replaced, unless it is of that kind already (`missing` refuses a
/// task not there, `closed` one done or cancelled).
pub(in crate::store) fn set_kind(acts: bool) -> String {
    let act = if acts { act() } else { String::new() };
    format!(
        "{act} \
DECLARE \
    task_ record; \
BEGIN \
    SELECT status, fields #>> '{{_board,kind}}' AS kind INTO task_ FROM task \
        WHERE slug = v ->> 'slug'; \
    IF NOT FOUND THEN \
        RAISE EXCEPTION '%', v ->> 'missing'; \
    END IF; \
    IF task_.status IN ('done', 'cancelled') THEN \
        RAISE EXCEPTION '%', (v ->> 'closed') || task_.status || ': only an open task is given a kind'; \
    END IF; \
    IF task_.kind IS DISTINCT FROM v ->> 'kind' THEN \
        UPDATE task SET fields = board_fields(id, fields, '{{_board,kind}}', v -> 'kind'), \
            version = version + 1 WHERE slug = v ->> 'slug'; \
        {} \
    END IF; \
END;",
        said(
            "CASE WHEN task_.kind IS NULL THEN (v ->> 'actor') || ' gave it the kind ' || (v ->> 'kind') \
             ELSE (v ->> 'actor') || ' changed its kind ' || task_.kind || ' → ' || (v ->> 'kind') END"
        )
    )
}

/// The actor's line `text` (a text expression) on `v ->> 'slug'`'s thread, the board's own record.
pub(in crate::store) fn said(text: &str) -> String {
    logged("v ->> 'slug'", "v ->> 'actor'", "'system'", text, "NULL")
}

/// A move's write of its reason's line (`move_reason`) by the actor, after the move's own line.
pub(in crate::store) fn reason_written() -> String {
    let reason = "v ->> 'move_reason'";
    logged("v ->> 'slug'", "v ->> 'actor'", "'msg'", reason, "NULL")
}

/// What a block waited on removed from `v ->> 'slug'` once a write took it out of blocked.
pub(in crate::store) const UNBLOCKED: &str = "UPDATE task SET fields = board_fields(id, fields, '{_board,blocked}', NULL) \
    WHERE slug = v ->> 'slug' AND status <> 'blocked' AND fields #> '{_board,blocked}' IS NOT NULL;";

/// Every task's slug and parent: the tree, read whole.
const TREE: &str =
    "coalesce((SELECT jsonb_agg(jsonb_build_array(slug, parent) ORDER BY slug) FROM task), '[]')";

/// The tree ([`TREE`]), as a read of its own.
pub(in crate::store) const TREE_ONLY: Pg<'static> = Pg::Read(Cow::Borrowed(TREE));

/// The tree as `set_parent` decides on it, the count of moves it was read at, and the actor.
pub(in crate::store) fn tree_read() -> Pg<'static> {
    Pg::read(format!(
        "jsonb_build_object('tree', {TREE}, \
    'moves', coalesce((SELECT last FROM counter WHERE id = '\"tree\"'), 0), \
    'me', (SELECT jsonb_strip_nulls(jsonb_build_object('left_at', left_at)) FROM session \
        WHERE handle = v ->> 'actor'), \
    'known', EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'actor'))"
    ))
}

/// No task moved under another since the tree was read; then the task under `v ->> 'parent'`.
const MOVE_UNDER: &str = "IF coalesce((SELECT last FROM counter WHERE id = '\"tree\"'), 0) \
        <> (v ->> 'moves')::bigint THEN \
    RAISE EXCEPTION 'stale'; \
END IF; \
INSERT INTO counter (id, last) VALUES ('\"tree\"', 1) \
    ON CONFLICT (id) DO UPDATE SET last = counter.last + 1; \
UPDATE task SET parent = v ->> 'parent', updated = (v ->> 'now')::double precision, \
    version = version + 1 WHERE slug = v ->> 'slug';";

/// What `Store::run_for` reads, in one statement (`super::super::surreal::work::run_for_read`).
pub(in crate::store) fn run_for_read() -> Pg<'static> {
    const SEEN_AS: &str =
        "SELECT s.handle, s.last_seen, s.busy_until, s.pid, s.host FROM session s";
    const BINDING: &str = "callback_state c ON c.id = jsonb_build_array('binding', s.agent)";
    Pg::read(format!(
        "(WITH here AS ({SEEN_AS} JOIN {BINDING} \
        WHERE v ? 'checkout' AND s.left_at IS NULL \
            AND c.doc -> 'in_process' = 'true' AND c.doc ->> 'worktree' = v ->> 'checkout'), \
    called AS ({SEEN_AS} LEFT JOIN {BINDING} \
        WHERE v ? 'claude' AND s.left_at IS NULL AND s.agent = v ->> 'claude' \
            AND coalesce(c.doc -> 'in_process', 'false') = 'false'), \
    who AS (SELECT v ->> 'asked' AS handle UNION SELECT handle FROM here \
        UNION SELECT handle FROM called) \
SELECT jsonb_build_object( \
    'here', {}, \
    'called', {}, \
    'held', coalesce((SELECT jsonb_agg(jsonb_build_object('handle', r.handle, 'slug', r.slug)) \
        FROM role r JOIN task t ON t.slug = r.slug \
        WHERE r.handle IN (SELECT handle FROM who) \
        AND ((r.role = 'executor' AND t.status IN (SELECT jsonb_array_elements_text(v -> 'executing'))) \
            OR (r.role = 'reviewer' AND t.status = 'review'))), '[]'), \
    'tree', {TREE}, \
    'branch', (SELECT to_jsonb(status) FROM task WHERE slug = v ->> 'branch')))",
        rows("h", "here h", "h.handle"),
        rows("h", "called h", "h.handle"),
    ))
}

/// The checks a pass's write makes first, before any statement of its own moves a version or a
/// `last_seen`: each task as read (`fenced`), then each dropped holder as read ([`SEEN`]).
pub(in crate::store) const FENCES: &str = "IF EXISTS (SELECT 1 FROM jsonb_array_elements(v -> 'fenced') AS g \
        WHERE (SELECT version FROM task WHERE slug = g ->> 'slug') \
            IS DISTINCT FROM (g ->> 'version')::bigint) THEN \
    RAISE EXCEPTION 'stale'; \
END IF;";

/// Whether a receiver `v -> 'receivers'` names has no session that has not left: the receivers'
/// sessions read by `session_handle`, all of them by one condition (`= ANY`), whatever number the
/// server guesses a list holds.
pub(in crate::store) const RECEIVER_GONE: &str = "EXISTS (SELECT 1 \
        FROM jsonb_array_elements_text(v -> 'receivers') AS h \
        LEFT JOIN (SELECT handle FROM session WHERE handle = ANY \
            (ARRAY(SELECT jsonb_array_elements_text(v -> 'receivers'))) AND left_at IS NULL) AS s \
        ON s.handle = h WHERE s.handle IS NULL)";

/// A pass's writes, after its fences: each receiver still there and fenced, its slot counter where
/// it was read; each give's role, version, slot counter and line, sent by `sender`; each mark set
/// or lifted (`super::super::surreal::work::gives`).
pub(in crate::store) fn gives() -> String {
    format!(
        "IF {RECEIVER_GONE} THEN \
    RAISE EXCEPTION 'stale'; \
END IF; \
{} \
IF EXISTS (SELECT 1 FROM jsonb_array_elements(v -> 'counters') AS c \
        WHERE coalesce((SELECT last FROM counter WHERE id = c -> 'key'), 0) \
            <> (c ->> 'last')::bigint) THEN \
    RAISE EXCEPTION 'stale'; \
END IF; \
DECLARE \
    give_ jsonb; \
BEGIN \
    FOR give_ IN SELECT jsonb_array_elements(v -> 'gives') LOOP \
        DELETE FROM role WHERE id IN (SELECT jsonb_build_array(give_ ->> 'slug', h, 'orchestrator') \
            FROM jsonb_array_elements_text(give_ -> 'drop') AS h); \
        INSERT INTO role (id, slug, handle, role, since) \
            VALUES (jsonb_build_array(give_ ->> 'slug', give_ ->> 'to', 'orchestrator'), \
                give_ ->> 'slug', give_ ->> 'to', 'orchestrator', (v ->> 'now')::double precision) \
            ON CONFLICT (id) DO UPDATE SET since = EXCLUDED.since, expires = NULL; \
        UPDATE task SET version = version + 1 WHERE slug = give_ ->> 'slug'; \
        INSERT INTO counter (id, last) VALUES (jsonb_build_array('slots', give_ ->> 'to'), 1) \
            ON CONFLICT (id) DO UPDATE SET last = counter.last + 1; \
        {} \
    END LOOP; \
END; \
UPDATE task SET version = version + 1, \
    fields = board_fields(id, fields, '{{{FIELDS},{WAITING}}}', v -> 'now') \
    WHERE slug IN (SELECT jsonb_array_elements_text(v -> 'marks')); \
UPDATE task SET version = version + 1, fields = board_fields(id, fields, '{{{FIELDS},{WAITING}}}', NULL) \
    WHERE slug IN (SELECT jsonb_array_elements_text(v -> 'unmarks'));",
        fenced("SELECT jsonb_array_elements_text(v -> 'receivers')"),
        logged(
            "give_ ->> 'slug'",
            "v ->> 'sender'",
            "'system'",
            "give_ ->> 'line'",
            "NULL"
        )
    )
}

/// What `Store::gaining` reads of `v ->> 'handle'`: its slot counter, and the status and red mark
/// of each task it holds the orchestrator role on.
pub(in crate::store) const GAINING: Pg<'static> = Pg::Read(Cow::Borrowed(
    "jsonb_build_object( \
    'last', coalesce((SELECT last FROM counter WHERE id = jsonb_build_array('slots', v ->> 'handle')), 0), \
    'tasks', coalesce((SELECT jsonb_agg(jsonb_build_object('status', t.status, \
            'red', t.fields #> '{_board,red}') ORDER BY t.slug) \
        FROM role r JOIN task t ON t.slug = r.slug \
        WHERE r.handle = v ->> 'handle' AND r.role = 'orchestrator'), '[]'))",
));

/// Of `candidates`, the sessions that listen: a channel registered, or a reader of `tasks wait`;
/// the channels by their ids, all of them by one condition (`= ANY`).
pub(in crate::store) const LISTENING: Pg<'static> = Pg::Read(Cow::Borrowed(
    "coalesce((SELECT jsonb_agg(DISTINCT l.handle) FROM ( \
    SELECT c.doc ->> 'handle' AS handle FROM callback_state c \
        WHERE c.id = ANY (ARRAY(SELECT to_jsonb(h) FROM jsonb_array_elements_text(v -> 'candidates') AS h)) \
            AND c.doc ->> 'address' IS NOT NULL \
    UNION SELECT w FROM jsonb_array_elements_text(coalesce( \
            (SELECT doc -> 'waiting' FROM callback_state WHERE id = '\"chat\"'), '[]')) AS w \
        WHERE w IN (SELECT jsonb_array_elements_text(v -> 'candidates'))) AS l \
    WHERE l.handle IS NOT NULL), '[]')",
));

/// A pass of gives as one write: its fences, then its writes ([`gives`]), by a session (`actor`)
/// when `acts`, else the board's own.
pub(in crate::store) fn give_pass(acts: bool) -> Pg<'static> {
    let act = if acts { act() } else { String::new() };
    Pg::script(format!("{act}\n{FENCES}\n{SEEN}\n{}", gives()))
}

/// [`MOVE_UNDER`] with its line on the thread (`said`), by the actor: a session's write when
/// `acts`, else the owner's; with the slot counters it holds (`slots`) when `guarded`; and, when
/// `gives`, the give the move makes in the same write, its fences first and its writes last.
pub(in crate::store) fn set_parent(acts: bool, guarded: bool, gives: bool) -> Pg<'static> {
    let act = if acts { act() } else { String::new() };
    let guard = if guarded { GUARD } else { "" };
    let (fences, seen, given) = match gives {
        true => (FENCES, SEEN, self::gives()),
        false => ("", "", String::new()),
    };
    Pg::script(format!(
        "{act}\n{fences}\n{seen}\n{MOVE_UNDER}\n{UNHOLD}\n{}\n{guard}\n{given}",
        said("v ->> 'said'")
    ))
}

/// Retitles `v ->> 'slug'`, links it to each of `links` and says a changed title on its thread
/// (`missing` refuses a task not there), by the actor: a session's write when `acts`, else the
/// owner's.
pub(in crate::store) fn amend(acts: bool) -> Pg<'static> {
    let act = if acts { act() } else { String::new() };
    Pg::script(format!(
        "{act} \
DECLARE \
    title_ text; \
BEGIN \
    SELECT title INTO title_ FROM task WHERE slug = v ->> 'slug'; \
    IF NOT FOUND THEN \
        RAISE EXCEPTION '%', v ->> 'missing'; \
    END IF; \
    INSERT INTO link (id, slug, kind, target) \
        SELECT DISTINCT jsonb_build_array(v ->> 'slug', 'doc', l), v ->> 'slug', 'doc', l \
        FROM jsonb_array_elements_text(v -> 'links') AS l \
        ON CONFLICT (id) DO NOTHING; \
    UPDATE task SET title = v ->> 'title', version = version + 1 WHERE slug = v ->> 'slug'; \
    IF title_ IS DISTINCT FROM v ->> 'title' THEN \
        {} \
    END IF; \
END;",
        said("(v ->> 'actor') || ' retitled it: ' || title_ || ' → ' || (v ->> 'title')")
    ))
}

/// The owner's move of `v ->> 'slug'` from one of `from` to `status`, its line on the thread, its
/// reason's (`move_reason`) when `reason`, and its dependants blocked (`cancelling`: the move
/// cancels it); fenced on `version` when `fenced` and holding `slots` when `guarded`. Refuses
/// with `missing`, `cannot` and `instruction`.
pub(in crate::store) fn move_as_owner(
    reason: bool,
    cancelling: bool,
    fenced: bool,
    guarded: bool,
) -> Pg<'static> {
    let reason = if reason {
        reason_written()
    } else {
        String::new()
    };
    let body = format!(
        "DECLARE \
    task_ record; \
BEGIN \
    SELECT status, instruction, parent INTO task_ FROM task WHERE slug = v ->> 'slug'; \
    IF NOT FOUND THEN \
        RAISE EXCEPTION '%', v ->> 'missing'; \
    END IF; \
    IF task_.status NOT IN (SELECT jsonb_array_elements_text(v -> 'from')) THEN \
        RAISE EXCEPTION '%', (v ->> 'cannot') || task_.status || ' to ' || (v ->> 'status'); \
    END IF; \
    IF v ->> 'status' = 'review' AND (task_.instruction OR task_.parent IS NOT NULL) THEN \
        RAISE EXCEPTION '%', v ->> 'instruction'; \
    END IF; \
    UPDATE task SET status = v ->> 'status', updated = (v ->> 'now')::double precision, \
        version = version + 1 WHERE slug = v ->> 'slug'; \
    {UNBLOCKED} \
    {UNHOLD} \
    {} \
END; \
{reason} \
{}",
        said(&format!(
            "(v ->> 'actor') || '{MOVED}' || task_.status || '{ARROW}' || (v ->> 'status')"
        )),
        block_cancelled_dependencies(cancelling)
    );
    Pg::script(guarding(fenced, &body, guarded))
}

/// Whether `v ->> 'slug'` is a task, the tasks right under it, the lines of its thread that record
/// an executor or an orchestrator, and who holds those roles now.
pub(in crate::store) const EVER_HELD: Pg<'static> = Pg::Read(Cow::Borrowed(
    "jsonb_build_object( \
    'known', EXISTS (SELECT 1 FROM task WHERE slug = v ->> 'slug'), \
    'below', coalesce((SELECT jsonb_agg(jsonb_build_object('slug', slug, 'instruction', instruction) \
        ORDER BY slug) FROM task WHERE parent = v ->> 'slug'), '[]'), \
    'lines', coalesce((SELECT jsonb_agg(jsonb_build_object('sender', m.sender, 'text', m.text) \
            ORDER BY m.sid) \
        FROM message m WHERE m.slug = v ->> 'slug' AND m.kind = 'system' \
            AND (starts_with(m.text, m.sender || ' claimed it') \
                OR m.text = m.sender || ' joined as executor' \
                OR strpos(m.text, ' orchestrator') > 0)), '[]'), \
    'holders', coalesce((SELECT jsonb_agg(jsonb_build_object('handle', handle, 'role', role) \
        ORDER BY handle, role) \
        FROM role WHERE slug = v ->> 'slug' AND role IN ('executor', 'orchestrator')), '[]'))",
));

/// The task `v ->> 'slug'` as a red's mark reads it, into `task_`: refused with `missing` when it
/// is not there.
const RED_READ: &str = "SELECT status, priority, fields #> '{_board,red}' AS red INTO task_ FROM task \
        WHERE slug = v ->> 'slug'; \
    IF NOT FOUND THEN \
        RAISE EXCEPTION '%', v ->> 'missing'; \
    END IF;";

/// A red's filer ([`FILED`]) marking `v ->> 'slug'` red (`red`) and most urgent, said on its
/// thread, unless it is both already; refused for a task closed (`closed`).
pub(in crate::store) fn mark_red() -> Pg<'static> {
    Pg::script(format!(
        "{} \
DECLARE \
    task_ record; \
BEGIN \
    {RED_READ} \
    IF task_.status IN ('done', 'cancelled') THEN \
        RAISE EXCEPTION '%', (v ->> 'closed') || task_.status || ': only an open task is marked red'; \
    END IF; \
    {FILED} \
    IF task_.red IS NULL OR task_.priority > 0 THEN \
        UPDATE task SET fields = board_fields(id, fields, '{{_board,red}}', v -> 'red'), priority = 0, \
            version = version + 1 WHERE slug = v ->> 'slug'; \
        {} \
    END IF; \
END;",
        act(),
        said(
            "(v ->> 'actor') || ' marked it red (' || (v ->> 'red') || ') and most urgent, as a red is filed'"
        )
    ))
}

/// A red's filer ([`FILED`]) lifting `v ->> 'slug'`'s red mark, said on its thread, unless it has
/// none or is closed; fenced on `version` when `fenced` and holding `slots` when `guarded`.
pub(in crate::store) fn unmark_red(fenced: bool, guarded: bool) -> Pg<'static> {
    let body = format!(
        "{} \
DECLARE \
    task_ record; \
BEGIN \
    {RED_READ} \
    {FILED} \
    IF task_.red IS NOT NULL AND task_.status NOT IN ('done', 'cancelled') THEN \
        UPDATE task SET fields = board_fields(id, fields, '{{_board,red}}', NULL), version = version + 1 \
            WHERE slug = v ->> 'slug'; \
        {} \
    END IF; \
END;",
        act(),
        said(
            "(v ->> 'actor') || ' lifted its red mark: red no more, it and the open work under it take a slot as any work does'"
        )
    );
    Pg::script(guarding(fenced, &body, guarded))
}

/// The owner's tasks saved under the receipt `receipt`, two at most.
pub(in crate::store) const OWNER_SAVED: Pg<'static> = Pg::Read(Cow::Borrowed(
    "coalesce((SELECT jsonb_agg(o.slug) FROM (SELECT slug FROM task WHERE created_by = 'owner' \
    AND fields #>> '{_symphony,owner_save}' = v ->> 'receipt' ORDER BY slug LIMIT 2) AS o), '[]')",
));

#[cfg(test)]
mod tests {
    use super::*;

    /// `statement`'s block, the shape every write here has.
    fn block(statement: Pg<'static>) -> String {
        match statement {
            Pg::Script(block, None) => block.into_owned(),
            other => panic!("a write is a block answering its own: {other:?}"),
        }
    }

    /// Where each of `parts` is in `text`, each after the one before it.
    fn in_order(text: &str, parts: &[&str]) {
        let at: Vec<usize> = (parts.iter())
            .map(|part| {
                text.find(part)
                    .unwrap_or_else(|| panic!("{part} in {text}"))
            })
            .collect();
        assert!(at.windows(2).all(|pair| pair[0] < pair[1]), "{at:?}");
    }

    #[test]
    fn a_new_task_carries_each_mark_it_is_made_with_and_no_other() {
        let new = |acts, receipt, red, arm| Creating {
            acts,
            receipt,
            red,
            arm,
            kind: false,
            placed: false,
        };
        let placed = |acts| Creating {
            placed: true,
            ..new(acts, false, false, false)
        };
        let marks = [
            field_set("_symphony,owner_save", "v -> 'owner_save'"),
            field_set("_board,red", "v -> 'red'"),
            field_set("_board,arm", "v -> 'arm'"),
            field_set("_board,kind", "v -> 'kind'"),
        ];
        // Filed as the board's own, refused in the write while a custom workflow is active.
        let bare = create(&new(false, false, false, false));
        assert!(
            bare.starts_with(&format!("{UNPLACED}\n{CREATE}")),
            "the owner's task is no session's act"
        );
        assert!(!bare.contains(super::super::workflow::PLACED));
        // Filed again at the workflow's initial pair: placed, never refused for it.
        let at_initial = create(&placed(false));
        assert!(at_initial.starts_with(&format!("{CREATE}\n{}", super::super::workflow::PLACED)));
        assert!(!at_initial.contains(UNPLACED));
        in_order(
            &create(&placed(true)),
            &[&act(), CREATE, super::super::workflow::PLACED],
        );
        assert!(marks.iter().all(|mark| !bare.contains(mark.as_str())));
        assert!(!bare.contains(FILED));
        let all = create(&Creating {
            kind: true,
            ..new(true, true, true, true)
        });
        let act = act();
        let said = said("v ->> 'said'");
        let blocks = block_cancelled_dependencies(false);
        in_order(
            &all,
            &[
                &act, UNPLACED, CREATE, &marks[0], FILED, &marks[1], &marks[2], &marks[3], &said,
                &blocks,
            ],
        );
        let red = create(&new(true, false, true, false));
        assert!(red.contains(FILED) && red.contains(marks[1].as_str()));
        assert!(!red.contains(marks[2].as_str()) && !red.contains(marks[3].as_str()));
        let kind = create(&Creating {
            kind: true,
            ..new(false, false, false, false)
        });
        assert!(kind.contains(marks[3].as_str()) && !kind.contains(FILED));
    }

    #[test]
    fn a_move_under_another_checks_before_it_writes_and_gives_after() {
        let bare = block(set_parent(false, false, false));
        assert!(bare.contains(MOVE_UNDER) && bare.contains(UNHOLD));
        for part in [FENCES, SEEN, GUARD, "v -> 'gives'", "unknown_actor"] {
            assert!(!bare.contains(part), "{part}");
        }
        let whole = block(set_parent(true, true, true));
        let (act, said, gives) = (act(), said("v ->> 'said'"), gives());
        in_order(
            &whole,
            &[&act, FENCES, SEEN, MOVE_UNDER, UNHOLD, &said, GUARD, &gives],
        );
        assert_eq!(
            block(give_pass(false)),
            format!("\n{FENCES}\n{SEEN}\n{gives}")
        );
        assert!(block(give_pass(true)).starts_with(&act));
    }

    #[test]
    fn an_owners_move_is_fenced_guarded_reasoned_and_blocks_as_asked() {
        let bare = block(move_as_owner(false, false, false, false));
        assert!(!bare.contains("move_reason") && !bare.contains(GUARD));
        assert!(
            !bare.contains("RAISE EXCEPTION 'stale'"),
            "no fence: {bare}"
        );
        assert!(bare.contains(&block_cancelled_dependencies(false)));
        let whole = block(move_as_owner(true, true, true, true));
        let (reason, blocks) = (reason_written(), block_cancelled_dependencies(true));
        in_order(
            &whole,
            &[
                super::super::cap::FENCE,
                UNBLOCKED,
                UNHOLD,
                &reason,
                &blocks,
                GUARD,
            ],
        );
        let unmarked = block(unmark_red(true, false));
        assert!(unmarked.starts_with(super::super::cap::FENCE) && !unmarked.contains(GUARD));
    }

    #[test]
    fn a_session_amends_as_an_act_and_the_owner_with_none() {
        let act = act();
        assert!(block(amend(true)).starts_with(&act));
        let owners = block(amend(false));
        assert!(!owners.contains(&act) && owners.contains("v ->> 'missing'"));
        assert!(block(mark_red()).starts_with(&act));
    }
}
