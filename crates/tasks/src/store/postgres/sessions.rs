//! The statements of `store/sessions.rs` on PostgreSQL.

use super::Pg;

/// Every session's handle.
pub(in crate::store) const HANDLES: Pg<'static> = Pg::Read(std::borrow::Cow::Borrowed(
    "coalesce((SELECT jsonb_agg(handle) FROM session), '[]'::jsonb)",
));

/// A session, its parent there when it names one, and a handle nobody took meanwhile.
const REGISTER: &str = "IF v ? 'parent' AND NOT EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'parent') THEN
    RAISE EXCEPTION '%', v ->> 'orphan';
END IF;
IF EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'handle') THEN
    RAISE EXCEPTION 'stale';
END IF;
INSERT INTO session
    SELECT r.* FROM jsonb_populate_record(NULL::session, (v -> 'row') || jsonb_build_object('id', v -> 'handle')) AS r
    ON CONFLICT DO NOTHING;
IF NOT FOUND THEN
    RAISE EXCEPTION 'stale';
END IF;";

/// The tasks `v ->> 'handle'` holds that another could take over.
/// Needs `executing`.
pub(in crate::store) const CLAIMABLE: &str =
    "coalesce((SELECT jsonb_agg(r.slug) FROM role r JOIN task t ON t.slug = r.slug
    WHERE r.handle = v ->> 'handle'
    AND ((r.role = 'executor' AND t.status IN (SELECT jsonb_array_elements_text(v -> 'executing')))
        OR (r.role = 'reviewer' AND t.status = 'review'))), '[]'::jsonb)";

/// One session, `v ->> 'handle'`, and whether it holds work another could take over (a `Found`).
/// Needs `executing`.
pub(in crate::store) fn session_read() -> String {
    format!(
        "jsonb_build_object(
    'session', (SELECT {} FROM session s WHERE s.handle = v ->> 'handle'),
    'held', {CLAIMABLE},
    'native_until', (SELECT CASE WHEN starts_with(coalesce(c.doc ->> 'address', ''), 'codex:')
        THEN coalesce(c.doc -> 'confirmed_until', '0'::jsonb) END
        FROM callback_state c WHERE c.id = v -> 'handle'))",
        super::row("s")
    )
}

/// [`REGISTER`], then the session as a read of one gives it ([`session_read`]).
pub(in crate::store) fn register() -> Pg<'static> {
    Pg::script_then(REGISTER, session_read())
}

/// The binding decision, including full-session provenance kept outside the session row.
pub(in crate::store) fn binding() -> Pg<'static> {
    Pg::read(format!("jsonb_build_object('row', (SELECT {} FROM session s WHERE handle = v ->> 'handle'),
'barred', EXISTS (SELECT 1 FROM barred_key WHERE id = v -> 'agent'),
'bound', coalesce((SELECT jsonb_agg(handle) FROM session WHERE agent = v ->> 'agent' AND left_at IS NULL), '[]'),
'in_process', (SELECT doc -> 'in_process' FROM callback_state WHERE id = jsonb_build_array('binding', v -> 'agent')),
'held_in_process', (SELECT c.doc -> 'in_process' FROM session s JOIN callback_state c ON c.id = jsonb_build_array('binding', s.agent) WHERE s.handle = v ->> 'handle'),
'native', EXISTS (SELECT 1 FROM callback_state WHERE id = v -> 'handle' AND doc ? 'address'))", super::row("s")))
}

const BIND_GUARD: &str = "INSERT INTO counter (id, last) VALUES (jsonb_build_array('agent', v -> 'agent'), 1) ON CONFLICT (id) DO UPDATE SET last = counter.last + 1;
IF NOT EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'handle' AND pid IS NOT DISTINCT FROM (v ->> 'pid')::bigint AND host IS NOT DISTINCT FROM v ->> 'host') THEN RAISE EXCEPTION 'stale'; END IF;";

const CHANNEL_BIND: &str = "IF NOT (v ->> 'full')::boolean AND (
    EXISTS (SELECT 1 FROM callback_state WHERE id = v -> 'handle' AND doc ? 'address')
    OR EXISTS (SELECT 1 FROM callback_state WHERE id = jsonb_build_array('binding', v -> 'agent') AND doc -> 'in_process' = 'false')
    OR EXISTS (SELECT 1 FROM session s JOIN callback_state c ON c.id = jsonb_build_array('binding', s.agent) WHERE s.handle = v ->> 'handle' AND c.doc -> 'in_process' = 'false')) THEN RAISE EXCEPTION 'stale'; END IF;
INSERT INTO callback_state (id, doc) VALUES (jsonb_build_array('binding', v -> 'agent'), jsonb_build_object('in_process', NOT (v ->> 'full')::boolean))
ON CONFLICT (id) DO UPDATE SET doc = callback_state.doc || jsonb_build_object('in_process', coalesce((callback_state.doc ->> 'in_process')::boolean, false) OR NOT (v ->> 'full')::boolean);
IF NOT (v ->> 'full')::boolean THEN
    DELETE FROM callback_state WHERE id = v -> 'handle';
    UPDATE callback_event SET doc = doc || '{\"delivered\":true,\"text\":\"\"}' WHERE doc ->> 'handle' = v ->> 'handle' AND starts_with(doc ->> 'key', 'chat-');
END IF;";

pub(in crate::store) fn rebind() -> Pg<'static> {
    Pg::script(format!(
        "{BIND_GUARD}\n{}\nIF (SELECT agent FROM session WHERE handle = v ->> 'handle') IS DISTINCT FROM v ->> 'agent' THEN RAISE EXCEPTION 'stale'; END IF;\n{CHANNEL_BIND}",
        super::fence("v ->> 'handle'")
    ))
}

pub(in crate::store) fn bind() -> Pg<'static> {
    Pg::script(format!("{BIND_GUARD}\n{CHANNEL_BIND}
IF EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'handle' AND left_at IS NOT NULL) THEN RAISE EXCEPTION 'stale'; END IF;
IF (v ->> 'full')::boolean AND (EXISTS (SELECT 1 FROM barred_key WHERE id = v -> 'agent') OR EXISTS (SELECT 1 FROM session WHERE agent = v ->> 'agent' AND left_at IS NULL)) THEN RAISE EXCEPTION 'stale'; END IF;
UPDATE session SET agent = NULL, busy_until = NULL, busy_call = NULL WHERE agent = v ->> 'agent' AND handle <> v ->> 'handle';
UPDATE session SET agent = v ->> 'agent', last_seen = greatest(last_seen, (v ->> 'now')::double precision) WHERE handle = v ->> 'handle';"))
}

pub(in crate::store) fn bar() -> Pg<'static> {
    Pg::script(format!(
        "{BIND_GUARD}
INSERT INTO barred_key (id, key) VALUES (v -> 'agent', v ->> 'agent') ON CONFLICT DO NOTHING;
UPDATE session SET agent = NULL, busy_until = NULL, busy_call = NULL WHERE agent = v ->> 'agent';"
    ))
}

pub(in crate::store) fn touch() -> Pg<'static> {
    Pg::script(super::act())
}
pub(in crate::store) fn reattach() -> Pg<'static> {
    Pg::script(format!(
        "{}\nUPDATE session SET pid = (v ->> 'pid')::bigint, host = v ->> 'host' WHERE handle = v ->> 'actor';",
        super::act()
    ))
}
pub(in crate::store) fn revive() -> Pg<'static> {
    Pg::script("IF NOT EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'handle') THEN RAISE EXCEPTION '%', v ->> 'unknown'; END IF;
UPDATE session SET left_at = NULL, last_seen = greatest(last_seen, (v ->> 'now')::double precision) WHERE handle = v ->> 'handle';")
}

const HELD_ORCHESTRATING: &str = "coalesce((SELECT jsonb_agg(slug ORDER BY slug) FROM role WHERE handle = v ->> 'actor' AND role = 'orchestrator'), '[]')";
pub(in crate::store) fn held_orchestrating() -> Pg<'static> {
    Pg::read(HELD_ORCHESTRATING)
}
pub(in crate::store) fn leave() -> Pg<'static> {
    use crate::stages::{EVERY_ROLE, RELEASED};
    Pg::script(format!("{}\n{}
IF {HELD_ORCHESTRATING} <> v -> 'held' THEN RAISE EXCEPTION 'stale'; END IF;
DECLARE slug_ text; ready_ boolean; held_ boolean;
BEGIN
    FOR slug_ IN SELECT DISTINCT r.slug FROM role r JOIN task t ON t.slug = r.slug
            WHERE r.handle = v ->> 'actor' AND (r.role = 'orchestrator' OR t.fields #> '{{_board,workflow}}' IS NULL)
            ORDER BY r.slug LOOP
        held_ := (SELECT fields #> '{{_board,workflow}}' IS NOT NULL FROM task WHERE slug = slug_);
        DELETE FROM role WHERE slug = slug_ AND handle = v ->> 'actor' AND (NOT held_ OR role = 'orchestrator');
        ready_ := NOT held_ AND (SELECT status = 'in_progress' FROM task WHERE slug = slug_) AND NOT EXISTS (SELECT 1 FROM role WHERE slug = slug_ AND role = 'executor');
        UPDATE task SET version = version + 1, status = CASE WHEN ready_ THEN 'ready' ELSE status END, updated = CASE WHEN ready_ THEN (v ->> 'now')::double precision ELSE updated END WHERE slug = slug_;
        {}
    END LOOP;
END;
{}
UPDATE session SET left_at = (v ->> 'now')::double precision WHERE handle = v ->> 'actor';
DELETE FROM callback_state WHERE id = v -> 'actor' OR id = jsonb_build_array('session_url', v -> 'actor');",
        super::act(), super::work::FENCES,
        super::logged("slug_", "v ->> 'actor'", "'system'", &format!("(v ->> 'actor') || '{RELEASED}{EVERY_ROLE} (left)'"), "NULL"), super::work::gives()))
}

pub(in crate::store) fn sessions() -> Pg<'static> {
    Pg::read(format!(
        "jsonb_build_object('sessions', {}, 'held', coalesce((SELECT jsonb_agg(DISTINCT r.handle) FROM role r JOIN task t ON t.slug = r.slug WHERE (r.role = 'executor' AND t.status IN (SELECT jsonb_array_elements_text(v -> 'executing'))) OR (r.role = 'reviewer' AND t.status = 'review')), '[]'))",
        super::rows("s", "session s", "s.connected, s.handle")
    ))
}
pub(in crate::store) fn handle_for_agent() -> Pg<'static> {
    Pg::read(
        "(SELECT to_jsonb(handle) FROM session WHERE agent = v ->> 'agent' AND left_at IS NULL LIMIT 1)",
    )
}
pub(in crate::store) fn bound_agents() -> Pg<'static> {
    Pg::read(
        "coalesce((SELECT jsonb_agg(jsonb_build_object('handle', handle, 'agent', agent)) FROM session WHERE agent IS NOT NULL AND left_at IS NULL), '[]')",
    )
}
/// The bound sessions that have not left, each with what its binding keeps, an item a row: on a
/// board of many sessions a long list.
pub(in crate::store) fn agent_bindings() -> Pg<'static> {
    super::list_of(
        "jsonb_strip_nulls(jsonb_build_object('handle', s.handle, 'agent', s.agent, 'in_process', coalesce(c.doc -> 'in_process', 'false'), 'worktree', coalesce(c.doc -> 'worktree', '\"\"'), 'branch', coalesce(c.doc -> 'branch', '\"\"'), 'placed_at', coalesce(c.doc -> 'placed_at', '0'), 'stay_from', coalesce(c.doc -> 'stay_from', c.doc -> 'placed_at', '0'), 'placed_boot', c.doc -> 'placed_boot', 'last_boot', c.doc -> 'last_boot', 'boot_uncertain', coalesce(c.doc -> 'boot_uncertain', 'false')))".into(),
        "session s LEFT JOIN callback_state c ON c.id = jsonb_build_array('binding', s.agent) WHERE s.agent IS NOT NULL AND s.left_at IS NULL",
        "s.handle",
    )
}

/// One beat and its optional usage share a transaction and the bound session it found.
pub(in crate::store) fn beat(spends: bool) -> Pg<'static> {
    let spend = if spends { super::spending::SPEND } else { "" };
    Pg::script(format!("DECLARE s_ session; bound_ jsonb; last_ jsonb; boot_ jsonb := v -> 'boot';
BEGIN
    SELECT s.* INTO s_ FROM session s WHERE agent = v ->> 'agent' AND left_at IS NULL LIMIT 1;
    IF FOUND THEN
        UPDATE session SET last_seen = greatest(last_seen, (v ->> 'now')::double precision), last_tool = greatest(coalesce(last_tool, (v ->> 'now')::double precision), (v ->> 'now')::double precision),
            busy_until = CASE WHEN v ? 'busy_for' AND (s_.busy_until IS NULL OR (v ->> 'now')::double precision + (v ->> 'busy_for')::double precision >= s_.busy_until) THEN (v ->> 'now')::double precision + (v ->> 'busy_for')::double precision WHEN (v ->> 'ended')::boolean AND s_.busy_call IS NOT DISTINCT FROM v ->> 'call' THEN NULL ELSE s_.busy_until END,
            busy_call = CASE WHEN v ? 'busy_for' AND (s_.busy_until IS NULL OR (v ->> 'now')::double precision + (v ->> 'busy_for')::double precision >= s_.busy_until) THEN v ->> 'call' WHEN (v ->> 'ended')::boolean AND s_.busy_call IS NOT DISTINCT FROM v ->> 'call' THEN NULL ELSE s_.busy_call END
        WHERE handle = s_.handle;
        SELECT doc INTO bound_ FROM callback_state WHERE id = jsonb_build_array('binding', v -> 'agent');
        IF bound_ -> 'in_process' = 'true' THEN
            IF v ? 'place' AND jsonb_build_array(bound_ -> 'worktree', bound_ -> 'branch') IS DISTINCT FROM jsonb_build_array(v #> '{{place,worktree}}', v #> '{{place,branch}}') THEN
                UPDATE callback_state SET doc = jsonb_strip_nulls(doc || jsonb_build_object('worktree', v #> '{{place,worktree}}', 'branch', v #> '{{place,branch}}', 'placed_at', v -> 'now', 'stay_from', v -> 'now', 'placed_boot', boot_, 'last_boot', boot_, 'boot_order', boot_, 'boot_previous', '[]'::jsonb, 'boot_uncertain', boot_ IS NULL)) WHERE id = jsonb_build_array('binding', v -> 'agent');
            ELSE
                IF v ? 'place' AND (v ->> 'now')::double precision < coalesce((bound_ ->> 'stay_from')::double precision, (bound_ ->> 'placed_at')::double precision) THEN
                    UPDATE callback_state SET doc = doc || jsonb_build_object('stay_from', v -> 'now') WHERE id = jsonb_build_array('binding', v -> 'agent');
                END IF;
                last_ := coalesce(bound_ -> 'boot_order', bound_ -> 'last_boot', bound_ -> 'placed_boot');
                IF boot_ IS NULL AND (bound_ ? 'last_boot' OR bound_ -> 'boot_uncertain' IS DISTINCT FROM 'true'::jsonb) THEN
                    UPDATE callback_state SET doc = jsonb_strip_nulls((doc - 'last_boot') || jsonb_build_object('boot_order', last_, 'boot_uncertain', true)) WHERE id = jsonb_build_array('binding', v -> 'agent');
                ELSIF boot_ IS NOT NULL AND NOT coalesce(bound_ -> 'boot_previous', '[]') @> jsonb_build_array(boot_ -> 'boot') AND (last_ IS NULL OR boot_ -> 'boot' IS DISTINCT FROM last_ -> 'boot' OR (boot_ ->> 'at')::double precision > (last_ ->> 'at')::double precision) THEN
                    UPDATE callback_state SET doc = doc || jsonb_build_object('last_boot', boot_, 'boot_order', boot_, 'boot_previous', CASE WHEN last_ IS NOT NULL AND boot_ -> 'boot' IS DISTINCT FROM last_ -> 'boot' THEN coalesce(doc -> 'boot_previous', '[]') || jsonb_build_array(last_ -> 'boot') ELSE coalesce(doc -> 'boot_previous', '[]') END) WHERE id = jsonb_build_array('binding', v -> 'agent');
                END IF;
            END IF;
        END IF;
        {spend}
        board_answer := to_jsonb(s_.handle);
    END IF;
END;"))
}
