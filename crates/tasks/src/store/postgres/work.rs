//! The statements of `store/work.rs` on PostgreSQL.

use super::{act, logged};
use crate::stages::BLOCK_LINE;
use crate::store::surreal::work::Creating;

/// The task, its dependencies and parent there, its assignee a session, and a slug nobody took.
const CREATE: &str = "IF EXISTS (SELECT 1 FROM task WHERE slug = v ->> 'slug') THEN
    RAISE EXCEPTION '%', v ->> 'taken';
END IF;
IF EXISTS (SELECT 1 FROM jsonb_array_elements(v -> 'needed') AS needed (other)
        WHERE NOT EXISTS (SELECT 1 FROM task WHERE slug = needed.other ->> 'slug')) THEN
    RAISE EXCEPTION '%', (SELECT needed.other ->> 'missing' FROM jsonb_array_elements(v -> 'needed')
        WITH ORDINALITY AS needed (other, at)
        WHERE NOT EXISTS (SELECT 1 FROM task WHERE slug = needed.other ->> 'slug')
        ORDER BY needed.at LIMIT 1);
END IF;
IF v ? 'assignee' AND NOT EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'assignee') THEN
    RAISE EXCEPTION '%', v ->> 'unassignable';
END IF;
INSERT INTO counter (id, last) VALUES ('\"task\"', 1)
    ON CONFLICT (id) DO UPDATE SET last = counter.last + 1
    RETURNING last INTO sid_;
INSERT INTO task (id, sid, slug, title, body, status, priority, parent, created_by, created, updated, instruction)
    VALUES (v -> 'slug', sid_, v ->> 'slug', v ->> 'title', v ->> 'body', v ->> 'status',
        (v ->> 'priority')::bigint, v ->> 'parent', v ->> 'actor', (v ->> 'now')::double precision,
        (v ->> 'now')::double precision, (v ->> 'instruction')::boolean)
    ON CONFLICT DO NOTHING;
IF NOT FOUND THEN
    RAISE EXCEPTION 'stale';
END IF;
IF v ? 'assignee' THEN
    INSERT INTO role (id, slug, handle, role, since)
        VALUES (jsonb_build_array(v -> 'slug', v -> 'assignee', 'reserved'), v ->> 'slug',
            v ->> 'assignee', 'reserved', (v ->> 'now')::double precision);
END IF;
INSERT INTO link (id, slug, kind, target)
    SELECT jsonb_build_array(v -> 'slug', l.link -> 'kind', l.link -> 'target'), v ->> 'slug',
        l.link ->> 'kind', l.link ->> 'target'
    FROM jsonb_array_elements(v -> 'links') AS l (link)
    ON CONFLICT (id) DO UPDATE SET slug = EXCLUDED.slug, kind = EXCLUDED.kind, target = EXCLUDED.target;";

/// `v ->> 'slug'`'s task with `path` of its fields set to `value`, a `jsonb` expression.
fn field_set(path: &str, value: &str) -> String {
    format!(
        "UPDATE task SET fields = board_set(fields, '{{{path}}}', {value}) WHERE slug = v ->> 'slug';"
    )
}

/// The write's refusal unless its actor is a red's filer: `filer` and `unfiled`.
const FILED: &str =
    "IF (SELECT kind FROM session WHERE handle = v ->> 'actor') IS DISTINCT FROM v ->> 'filer' THEN
    RAISE EXCEPTION '%', v ->> 'unfiled';
END IF;";

/// A new task ([`CREATE`]) with the marks `new` names, its line on the thread (`said`) and, where
/// a task it waits on is cancelled, its block (`super::super::surreal::work::create`).
pub(in crate::store) fn create(new: &Creating) -> String {
    let mut block = String::new();
    if new.acts {
        block.push_str(&act());
        block.push('\n');
    }
    block.push_str(CREATE);
    block.push('\n');
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
    if new.fast_fix {
        block.push_str(&field_set("_board,fast_fix", "v -> 'fast_fix'"));
        block.push('\n');
    }
    if new.arm {
        block.push_str(&field_set("_board,arm", "v -> 'arm'"));
        block.push('\n');
    }
    block.push_str(&logged(
        "v ->> 'slug'",
        "v ->> 'actor'",
        "'system'",
        "v ->> 'said'",
        "NULL",
    ));
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
        "DECLARE
    dependent_ record;
    was_ text;
BEGIN
    FOR dependent_ IN SELECT l.slug, string_agg(l.target, ', ' ORDER BY l.target) AS cancelled
        FROM link l JOIN task t ON t.slug = l.target JOIN task d ON d.slug = l.slug
        WHERE l.kind = 'dep' AND {scope} AND t.status = 'cancelled'
            AND d.status NOT IN ('done', 'cancelled')
        GROUP BY l.slug ORDER BY l.slug
    LOOP
        SELECT status INTO was_ FROM task WHERE slug = dependent_.slug;
        UPDATE task SET status = 'blocked', updated = (v ->> 'now')::double precision,
            version = version + 1 WHERE slug = dependent_.slug;
        DELETE FROM counter WHERE id = jsonb_build_array('hold', dependent_.slug);
        {}
    END LOOP;
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
        "{act}
DECLARE
    was_ bigint;
BEGIN
    SELECT priority INTO was_ FROM task WHERE slug = v ->> 'slug';
    IF NOT FOUND THEN
        RAISE EXCEPTION '%', v ->> 'missing';
    END IF;
    UPDATE task SET priority = (v ->> 'priority')::bigint, version = version + 1
        WHERE slug = v ->> 'slug';
    {}
END;",
        logged(
            "v ->> 'slug'",
            "v ->> 'actor'",
            "'system'",
            "(v ->> 'actor') || ' changed priority p' || was_ || ' → p' || (v ->> 'priority')",
            "NULL",
        )
    )
}
