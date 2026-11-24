//! The statements of `store/cap.rs` on PostgreSQL: the cap's reads and the writes that hold its
//! counters.

use super::Pg;
use super::branches::{ON_BRANCHES, of_tasks};

/// A task `t` as the cap reads it, with its red mark ([`crate::red`]).
const ROW: &str = "jsonb_build_object('slug', t.slug, 'parent', t.parent, 'status', t.status, \
    'instruction', t.instruction, 'red', t.fields #> '{_board,red}', \
    'position', t.fields #> '{_board,workflow}')";

/// A task `t` in a status a task holding a slot may be in (`slot_statuses`, [`lets`]).
const OPEN: &str = "t.status IN (SELECT status FROM slot_statuses)";

/// The CTEs `slot_positions`, the positions of the active custom workflow, and `slot_statuses`,
/// the statuses a task holding a slot may be in.
const SLOT_POSITIONS: &str = "slot_positions AS (SELECT p -> 'at' ->> 'phase' AS phase, \
        p -> 'at' ->> 'state' AS state, p ->> 'slot' AS slot, p ->> 'legacy' AS legacy \
        FROM workflow d CROSS JOIN LATERAL jsonb_array_elements(d.body -> 'definition' -> 'positions') p \
        WHERE d.id = jsonb_build_array('definition', \
            (SELECT body ->> 'digest' FROM workflow WHERE id = '[\"active\"]'::jsonb))), \
    slot_statuses AS (SELECT jsonb_array_elements_text(v -> 'slot_statuses') AS status \
        UNION SELECT legacy FROM slot_positions WHERE slot <> 'release')";

/// The tasks the holds name, `counter:['hold', SLUG]`, as rows of one text.
const HELD: &str = "SELECT c.id ->> 1 FROM counter c WHERE c.id ->> 0 = 'hold'";

/// The roles of the tasks `slot_wanted` names (a CTE of `slug`s), as a list.
const ROLES_OF: &str = "coalesce((SELECT jsonb_agg(jsonb_build_object('slug', r.slug, 'handle', r.handle, \
    'role', r.role) ORDER BY r.slug, r.handle, r.role) FROM role r \
    WHERE r.slug IN (SELECT slug FROM slot_wanted)), '[]')";

/// Every counter of `kind`, `counter:[KIND, NAME]`, as a list of `[NAME, column]`.
fn counters(kind: &str, column: &str) -> String {
    format!(
        "coalesce((SELECT jsonb_agg(jsonb_build_array(c.id -> 1, c.{column}) ORDER BY c.id ->> 1) \
    FROM counter c WHERE c.id ->> 0 = '{kind}'), '[]')"
    )
}

/// Every slot counter, as a read of its own.
pub(in crate::store) fn counted() -> Pg<'static> {
    Pg::read(counters("slots", "last"))
}

/// The CTE `slot_wanted`: the open tasks, the held ones and `wanted` (rows of `slug`).
pub(in crate::store) fn lets(wanted: &str) -> String {
    format!(
        "{SLOT_POSITIONS}, \
    slot_wanted AS (SELECT t.slug FROM task t WHERE {OPEN} OR t.slug IN ({HELD}) UNION {wanted})"
    )
}

/// The snapshot's `slots` field, after [`lets`].
pub(in crate::store) fn field() -> String {
    format!(
        "'slots', jsonb_build_object( \
        'open', coalesce((SELECT jsonb_agg({ROW} ORDER BY t.slug) FROM task t WHERE {OPEN}), '[]'), \
        'holds', {}, \
        'held', coalesce((SELECT jsonb_agg({ROW} || jsonb_build_object('updated', t.updated) \
            ORDER BY t.slug) FROM task t WHERE t.slug IN ({HELD})), '[]'), \
        'roles', {ROLES_OF}, \
        'counted', {}, \
        'reds', {}, \
        'positions', coalesce((SELECT jsonb_agg(jsonb_build_object('phase', phase, 'state', state, \
            'slot', slot, 'legacy', legacy)) FROM slot_positions), '[]')),",
        counters("hold", "ts"),
        counters("slots", "last"),
        counters("red", "last")
    )
}

/// A level of tasks above, by slug, with their roles and recorded branches: the tasks
/// `v -> 'slot_wanted'` names and those on the branches it names.
pub(in crate::store) fn level() -> Pg<'static> {
    Pg::read(format!(
        "(WITH slot_wanted AS (SELECT jsonb_array_elements_text(v -> 'slot_wanted') AS slug \
    UNION {ON_BRANCHES}) \
SELECT jsonb_build_object( \
    'tasks', coalesce((SELECT jsonb_agg({ROW} ORDER BY t.slug) FROM task t \
        WHERE t.slug IN (SELECT slug FROM slot_wanted)), '[]'), \
    'roles', {ROLES_OF}, \
    'branches', {}))",
        of_tasks("b.slug IN (SELECT slug FROM slot_wanted)")
    ))
}

/// The write's hold on each counter it read (`v -> 'slots'`: its key and `last`): stale if another
/// write moved it since, then moved on.
pub(in crate::store) const GUARD: &str = "DECLARE \
    slot_ jsonb; \
BEGIN \
    FOR slot_ IN SELECT jsonb_array_elements(v -> 'slots') LOOP \
        IF coalesce((SELECT last FROM counter WHERE id = slot_ -> 'key'), 0) \
            <> (slot_ ->> 'last')::bigint THEN \
            RAISE EXCEPTION 'stale'; \
        END IF; \
        INSERT INTO counter (id, last) VALUES (slot_ -> 'key', (slot_ ->> 'last')::bigint + 1) \
            ON CONFLICT (id) DO UPDATE SET last = EXCLUDED.last; \
    END LOOP; \
END;";

/// The end of `v ->> 'slug'`'s hold.
pub(in crate::store) const UNHOLD: &str =
    "DELETE FROM counter WHERE id = jsonb_build_array('hold', v ->> 'slug');";

/// A rule's write's part in a hold: a move (`status`) ends one, and the hold's own move (`hold`)
/// makes one, stamped with the task's `updated` as the write left it.
pub(in crate::store) const HOLDING: &str = "IF v ? 'status' THEN \
    DELETE FROM counter WHERE id = jsonb_build_array('hold', v ->> 'slug'); \
END IF; \
IF coalesce((v ->> 'hold')::boolean, false) THEN \
    INSERT INTO counter (id, last, ts) \
        VALUES (jsonb_build_array('hold', v ->> 'slug'), 0, (v ->> 'updated')::double precision) \
        ON CONFLICT (id) DO UPDATE SET last = 0, ts = EXCLUDED.ts; \
END IF;";

/// A write's refusal to go on from a task that moved since the read its counters were judged on
/// (`version`).
pub(in crate::store) const FENCE: &str = "IF (SELECT version FROM task WHERE slug = v ->> 'slug') \
        IS DISTINCT FROM (v ->> 'version')::bigint THEN \
    RAISE EXCEPTION 'stale'; \
END IF;";

/// `body` fenced on the version read ([`FENCE`]) when `fenced`, and guarded by the counters held
/// ([`GUARD`]) when `guarded`.
pub(in crate::store) fn guarding(fenced: bool, body: &str, guarded: bool) -> String {
    let fence = if fenced { FENCE } else { "" };
    let guard = if guarded { GUARD } else { "" };
    format!("{fence}\n{body}\n{guard}")
}
