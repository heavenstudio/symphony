//! The statements of `store/callbacks.rs` on PostgreSQL.

use super::Pg;

/// Every channel of a session that has not left, with its session and whether its agent's
/// binding is an in-process one.
pub(in crate::store) fn registrations() -> Pg<'static> {
    Pg::read(format!(
        "coalesce((SELECT jsonb_agg(jsonb_strip_nulls(jsonb_build_object(
    'handle', c.doc -> 'handle', 'address', c.doc -> 'address', 'host', c.doc -> 'host',
    'terminal', c.doc -> 'terminal',
    'registration_revision', coalesce(c.doc -> 'registration_revision', '0'::jsonb),
    'registration_nonce', c.doc -> 'registration_nonce',
    'chat', coalesce(c.doc -> 'chat', 'false'::jsonb),
    'chat_fallback', coalesce(c.doc -> 'chat_fallback', 'false'::jsonb),
    'owner', c.doc -> 'owner',
    'confirmed_until', coalesce(c.doc -> 'confirmed_until', '0'::jsonb),
    'session', {},
    'chat_resume', coalesce(c.doc -> 'chat_resume', 'false'::jsonb),
    'pane_watch', coalesce(c.doc -> 'pane_watch', '{{}}'::jsonb),
    'in_process', CASE WHEN s.agent IS NULL THEN 'false'::jsonb ELSE coalesce(
        (SELECT b.doc -> 'in_process' FROM callback_state b WHERE b.id = jsonb_build_array('binding', s.agent)),
        'true'::jsonb) END)))
    FROM callback_state c LEFT JOIN session s ON s.handle = c.doc ->> 'handle'
    WHERE c.doc ? 'address' AND s.left_at IS NULL), '[]'::jsonb)",
        super::row("s")
    ))
}

/// Whether a queued line is due, with the owner's chat routed only to the current seat.
const CHAT: &str = "(NOT starts_with(e.doc ->> 'key', 'chat-') OR ((v ->> 'chat')::boolean AND NOT EXISTS (SELECT 1 FROM chat_event c WHERE c.id = to_jsonb(substr(e.doc ->> 'key', 6)) AND c.doc -> 'delivered' = 'true')))";
const DUE: &str = "e.doc ->> 'handle' = v ->> 'handle' AND e.doc -> 'delivered' IS DISTINCT FROM 'true'::jsonb AND (e.doc ->> 'attempts')::bigint < 3 AND (NOT e.doc ? 'retry_at' OR (e.doc ->> 'retry_at')::double precision <= (v ->> 'now')::double precision)";
/// Compare each digit run numerically, retaining the text runs between them. Padding the run's
/// length avoids a bounded integer cast: even caller-chosen keys longer than a bigint sort.
fn natural(key: &str) -> String {
    format!(
        "ARRAY(SELECT CASE WHEN part[1] ~ '^[0-9]' THEN '0' || lpad(length(ltrim(part[1], '0'))::text, 10, '0') || ltrim(part[1], '0') ELSE part[1] END FROM regexp_matches({key}, '[0-9]+|[^0-9]+', 'g') part)"
    )
}

fn order() -> String {
    format!(
        "(e.doc ->> 'sequence')::bigint, {}",
        natural("e.doc ->> 'key'")
    )
}

pub(in crate::store) fn send_agent() -> Pg<'static> {
    Pg::script(format!("{}
<<send_>>
DECLARE address_ text; channel_ jsonb; agent_ text; key_ text;
BEGIN
IF NOT EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'handle') THEN RAISE EXCEPTION 'refused: no session %', v ->> 'handle'; END IF;
IF EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'handle' AND left_at IS NOT NULL) THEN RAISE EXCEPTION 'refused: session % has left', v ->> 'handle'; END IF;
{}
IF v -> 'actor' = v -> 'handle' THEN board_answer := '\"\"'; EXIT send_; END IF;
SELECT doc INTO channel_ FROM callback_state WHERE id = v -> 'handle';
address_ := coalesce(channel_ ->> 'address', '');
SELECT agent INTO agent_ FROM session WHERE handle = v ->> 'handle';
IF address_ = '' OR (agent_ IS NOT NULL AND coalesce((SELECT (doc ->> 'in_process')::boolean FROM callback_state WHERE id = jsonb_build_array('binding', agent_)), true)) OR (starts_with(address_, 'uds:') AND NOT channel_ ? 'owner') THEN RAISE EXCEPTION '%', v ->> 'no_channel'; END IF;
INSERT INTO counter (id, last) VALUES (jsonb_build_array('callback-event', v -> 'handle'), 1) ON CONFLICT (id) DO UPDATE SET last = counter.last + 1 RETURNING last INTO sid_;
key_ := 'agent-' || sid_;
INSERT INTO callback_event (id, doc) VALUES (jsonb_build_array(v -> 'handle', key_), jsonb_build_object('key', key_, 'handle', v -> 'handle', 'text', v -> 'text', 'sequence', sid_, 'attempts', 0, 'delivered', false));
board_answer := to_jsonb(key_);
END send_;", super::act(), super::fence("v ->> 'handle'")))
}
pub(in crate::store) fn agent_delivered() -> Pg<'static> {
    Pg::read(
        "NOT EXISTS (SELECT 1 FROM callback_event WHERE id = jsonb_build_array(v -> 'handle', v -> 'key') AND doc -> 'delivered' IS DISTINCT FROM 'true'::jsonb)",
    )
}

pub(in crate::store) fn register() -> Pg<'static> {
    Pg::script(format!("{}
DECLARE agent_ text; binding_ jsonb; other_ text; old_ jsonb; take_ boolean;
BEGIN
INSERT INTO callback_state (id, doc) VALUES ('\"chat\"', '{{\"revision\":1,\"waiting\":[]}}') ON CONFLICT (id) DO UPDATE SET doc = callback_state.doc || jsonb_build_object('revision', coalesce((callback_state.doc ->> 'revision')::bigint, 0) + 1, 'waiting', coalesce((SELECT jsonb_agg(h) FROM jsonb_array_elements(coalesce(callback_state.doc -> 'waiting', '[]')) h WHERE h <> v -> 'actor'), '[]'));
SELECT agent INTO agent_ FROM session WHERE handle = v ->> 'actor';
SELECT doc -> 'in_process' INTO binding_ FROM callback_state WHERE id = jsonb_build_array('binding', agent_);
IF v ? 'address' AND agent_ IS NOT NULL AND binding_ IS NULL THEN RAISE EXCEPTION 'refused: binding provenance is not recorded yet; retry just tasks callback --as % so the current hook can classify the session', v ->> 'actor'; END IF;
IF v ? 'address' AND agent_ IS NOT NULL AND binding_ = 'true' THEN RAISE EXCEPTION 'refused: in-process agent binding cannot register a callback; use just tasks inbox'; END IF;
SELECT c.doc ->> 'handle' INTO other_ FROM callback_state c LEFT JOIN session s ON s.handle = c.doc ->> 'handle' WHERE c.doc -> 'address' = v -> 'address' AND c.doc -> 'handle' <> v -> 'actor' AND s.left_at IS NULL ORDER BY c.doc ->> 'handle' LIMIT 1;
IF v ? 'address' AND other_ IS NOT NULL THEN RAISE EXCEPTION 'refused: callback address already held by %; in-process subagents use just tasks inbox', other_; END IF;
SELECT doc INTO old_ FROM callback_state WHERE id = v -> 'actor';
IF old_ -> 'address' IS DISTINCT FROM v -> 'address' AND (NOT coalesce(old_, '{{}}') ? 'address' OR NOT v ? 'address' OR NOT old_ ? 'owner' OR old_ -> 'owner' IS DISTINCT FROM v -> 'owner') THEN
    UPDATE callback_state SET doc = (doc - ARRAY['pull_entries','pull_said','pull_awaiting','pull_queued']) || '{{\"pull_withheld\":[]}}' WHERE id = v -> 'actor';
    UPDATE callback_event SET doc = doc || '{{\"delivered\":true,\"text\":\"\"}}' WHERE doc -> 'handle' = v -> 'actor' AND doc -> 'delivered' IS DISTINCT FROM 'true'::jsonb AND doc ? 'pull_entries';
END IF;
IF v ? 'address' AND (old_ -> 'address' IS DISTINCT FROM v -> 'address' OR ((v ->> 'chat')::boolean AND NOT (v ->> 'if_free')::boolean)) THEN
    UPDATE callback_event SET doc = doc || '{{\"attempts\":0,\"retry_at\":0,\"capacity_limited\":false}}' WHERE doc -> 'handle' = v -> 'actor' AND doc -> 'delivered' IS DISTINCT FROM 'true'::jsonb;
END IF;
take_ := (v ->> 'chat')::boolean AND (NOT (v ->> 'if_free')::boolean OR NOT EXISTS (SELECT 1 FROM callback_state c LEFT JOIN session s ON s.handle = c.doc ->> 'handle' WHERE c.doc -> 'chat' = 'true' AND c.doc ? 'address' AND c.doc -> 'handle' <> v -> 'actor' AND s.left_at IS NULL));
IF take_ THEN
    UPDATE callback_state SET doc = doc || '{{\"chat\":false}}' WHERE doc -> 'chat' = 'true';
    UPDATE callback_event SET doc = doc || '{{\"delivered\":true,\"text\":\"\"}}' WHERE doc -> 'handle' <> v -> 'actor' AND starts_with(doc ->> 'key', 'chat-');
END IF;
IF NOT v ? 'address' THEN UPDATE callback_event SET doc = doc || '{{\"delivered\":true,\"text\":\"\"}}' WHERE doc -> 'handle' = v -> 'actor' AND starts_with(doc ->> 'key', 'chat-'); END IF;
INSERT INTO callback_state (id, doc) VALUES (v -> 'actor', '{{}}') ON CONFLICT DO NOTHING;
UPDATE callback_state SET doc = jsonb_strip_nulls(doc || jsonb_build_object('handle', v -> 'actor', 'address', v -> 'address', 'host', v -> 'host', 'chat_fallback', (v ->> 'chat')::boolean AND (v ->> 'if_free')::boolean, 'registration_revision', coalesce((doc ->> 'registration_revision')::bigint, 0) + 1, 'registration_nonce', replace(gen_random_uuid()::text, '-', ''), 'owner', v -> 'owner', 'confirmed_until', 0, 'chat_resume', false, 'terminal', v -> 'terminal', 'chat', CASE WHEN NOT v ? 'address' THEN false WHEN take_ THEN true ELSE coalesce((doc ->> 'chat')::boolean, false) END)) WHERE id = v -> 'actor';
IF v ? 'address' AND starts_with(v ->> 'address', 'codex:') THEN UPDATE task_session SET claude_session = v ->> 'address' WHERE handle = v ->> 'actor' AND closed IS NULL AND claude_session = 'codex:'; END IF;
END;", super::act()))
}

pub(in crate::store) fn waiting_orchestrators() -> Pg<'static> {
    super::listed(
        "s",
        "session s WHERE handle IN (SELECT jsonb_array_elements_text(coalesce((SELECT doc -> 'waiting' FROM callback_state WHERE id = '\"chat\"'), '[]'))) AND kind = 'orchestrator' AND left_at IS NULL AND NOT EXISTS (SELECT 1 FROM callback_state c WHERE c.id = to_jsonb(s.handle) AND c.doc ? 'address')",
        "s.handle",
    )
}
pub(in crate::store) fn session_kind() -> Pg<'static> {
    Pg::read("(SELECT to_jsonb(kind) FROM session WHERE handle = v ->> 'handle')")
}
pub(in crate::store) fn unheard_chat() -> Pg<'static> {
    Pg::read(format!(
        "coalesce((SELECT jsonb_agg(jsonb_build_object('key', doc -> 'key', 'text', doc -> 'text') ORDER BY {}) FROM chat_event WHERE doc -> 'delivered' = 'false'), '[]')",
        natural("doc ->> 'key'")
    ))
}
pub(in crate::store) fn prune() -> Pg<'static> {
    Pg::script("DELETE FROM callback_event e WHERE doc -> 'delivered' = 'true' OR (starts_with(doc ->> 'key', 'chat-') AND EXISTS (SELECT 1 FROM chat_event c WHERE c.id = to_jsonb(substr(e.doc ->> 'key', 6)) AND c.doc -> 'delivered' = 'true'));
DELETE FROM callback_state c WHERE doc ? 'in_process' AND NOT EXISTS (SELECT 1 FROM session s WHERE s.agent = c.id ->> 1 AND s.left_at IS NULL);")
}
pub(in crate::store) fn seen() -> Pg<'static> {
    Pg::script("<<seen_>>
DECLARE state_ jsonb; occupied_ boolean; seat_ boolean;
BEGIN
SELECT doc INTO state_ FROM callback_state WHERE id = v -> 'handle';
IF state_ -> 'address' IS DISTINCT FROM v -> 'address' OR state_ -> 'owner' IS DISTINCT FROM v -> 'owner' OR EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'handle' AND left_at IS NOT NULL) THEN board_answer := 'false'; EXIT seen_; END IF;
seat_ := coalesce((state_ ->> 'chat_resume_seat')::boolean, true);
occupied_ := EXISTS (SELECT 1 FROM callback_state c LEFT JOIN session s ON s.handle = c.doc ->> 'handle' WHERE c.doc -> 'chat' = 'true' AND c.doc ? 'address' AND c.doc -> 'handle' <> v -> 'handle' AND s.left_at IS NULL);
IF state_ -> 'chat_resume' = 'true' AND (NOT seat_ OR NOT occupied_) THEN UPDATE callback_state SET doc = doc || jsonb_build_object('chat', seat_, 'chat_fallback', coalesce(doc -> 'chat_resume_fallback', 'false'), 'chat_resume', false) WHERE id = v -> 'handle'; END IF;
UPDATE callback_state SET doc = doc || jsonb_build_object('confirmed_until', v -> 'until', 'blocked', false) WHERE id = v -> 'handle';
IF state_ -> 'blocked' = 'true' THEN UPDATE callback_event SET doc = doc || '{\"attempts\":0,\"retry_at\":0}' WHERE doc -> 'handle' = v -> 'handle' AND doc -> 'delivered' IS DISTINCT FROM 'true'::jsonb AND doc -> 'capacity_limited' IS DISTINCT FROM 'true'::jsonb; END IF;
UPDATE session SET last_seen = greatest(last_seen, (v ->> 'now')::double precision) WHERE handle = v ->> 'handle';
board_answer := 'true';
END seen_;")
}
pub(in crate::store) fn blocked() -> Pg<'static> {
    Pg::script(
        "UPDATE callback_state SET doc = doc || '{\"blocked\":true}' WHERE id = v -> 'handle' AND doc -> 'address' IS NOT DISTINCT FROM v -> 'address' AND doc -> 'owner' IS NOT DISTINCT FROM v -> 'owner' AND doc -> 'blocked' IS DISTINCT FROM 'true'::jsonb;",
    )
}
pub(in crate::store) fn inbox() -> Pg<'static> {
    let order = order();
    Pg::read(format!(
        "coalesce((SELECT jsonb_agg(e.doc ORDER BY {order}) FROM callback_event e WHERE e.doc -> 'handle' = v -> 'handle' AND e.doc -> 'delivered' IS DISTINCT FROM 'true'::jsonb AND NOT e.doc ? 'message' AND {CHAT}), '[]')"
    ))
}
pub(in crate::store) fn reader() -> Pg<'static> {
    Pg::read(format!(
        "jsonb_build_object('session', (SELECT {} FROM session s WHERE handle = v ->> 'handle'), 'state', (SELECT jsonb_strip_nulls(jsonb_build_object('address', doc -> 'address', 'pane_watch', doc -> 'pane_watch')) FROM callback_state WHERE id = v -> 'handle'), 'waiting', coalesce((SELECT doc -> 'waiting' FROM callback_state WHERE id = '\"chat\"'), '[]') @> jsonb_build_array(v -> 'handle'))",
        super::row("s")
    ))
}
pub(in crate::store) fn wait() -> Pg<'static> {
    Pg::script("IF NOT EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'handle' AND left_at IS NOT NULL) AND NOT EXISTS (SELECT 1 FROM callback_state WHERE id = v -> 'handle' AND doc ? 'address') THEN
INSERT INTO callback_state (id, doc) VALUES (v -> 'handle', jsonb_build_object('handle', v -> 'handle')) ON CONFLICT DO NOTHING;
INSERT INTO callback_state (id, doc) VALUES ('\"chat\"', jsonb_build_object('waiting', jsonb_build_array(v -> 'handle'))) ON CONFLICT (id) DO UPDATE SET doc = callback_state.doc || jsonb_build_object('waiting', (SELECT jsonb_agg(DISTINCT h) FROM jsonb_array_elements(coalesce(callback_state.doc -> 'waiting', '[]') || jsonb_build_array(v -> 'handle')) h));
END IF;")
}
pub(in crate::store) fn take_lease() -> Pg<'static> {
    Pg::script("<<lease_>>
DECLARE state_ jsonb; lease_ jsonb; revision_ bigint;
BEGIN
SELECT doc INTO state_ FROM callback_state WHERE id = v -> 'handle';
IF (NOT coalesce(state_, '{}') ? 'address' AND NOT (v ->> 'collect')::boolean) OR EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'handle' AND left_at IS NOT NULL) THEN EXIT lease_; END IF;
INSERT INTO callback_lease (id, doc) VALUES (v -> 'handle', '{}') ON CONFLICT (id) DO UPDATE SET doc = callback_lease.doc;
SELECT doc INTO lease_ FROM callback_lease WHERE id = v -> 'handle';
IF greatest(coalesce((state_ ->> 'lease')::double precision, 0), coalesce((lease_ ->> 'until')::double precision, 0)) > (v ->> 'now')::double precision THEN EXIT lease_; END IF;
revision_ := greatest(coalesce((lease_ ->> 'revision')::bigint, 0), coalesce((state_ ->> 'revision')::bigint, 0)) + 1;
UPDATE callback_lease SET doc = doc || jsonb_build_object('until', (v ->> 'now')::double precision + (v ->> 'lease_seconds')::double precision, 'revision', revision_) WHERE id = v -> 'handle';
board_answer := to_jsonb(revision_);
END lease_;")
}
pub(in crate::store) fn release_lease() -> Pg<'static> {
    Pg::script(
        "UPDATE callback_lease SET doc = doc || '{\"until\":0}' WHERE id = v -> 'handle' AND doc -> 'revision' = v -> 'revision';",
    )
}
pub(in crate::store) fn pull_checked() -> Pg<'static> {
    Pg::script(
        "UPDATE callback_lease SET doc = doc || jsonb_build_object('pull', v -> 'now') WHERE id = v -> 'handle' AND doc -> 'revision' = v -> 'revision';",
    )
}
pub(in crate::store) fn stored() -> Pg<'static> {
    Pg::read(
        "jsonb_build_object('state', (SELECT doc FROM callback_state WHERE id = v -> 'handle'), 'checked_pull', coalesce((SELECT doc -> 'pull' FROM callback_lease WHERE id = v -> 'handle'), '0'), 'queued', coalesce((SELECT jsonb_agg(jsonb_strip_nulls(jsonb_build_object('key', doc -> 'key', 'text', doc -> 'text', 'sequence', doc -> 'sequence', 'pull_entries', doc -> 'pull_entries', 'pull_withheld', doc -> 'pull_withheld', 'pull_said', doc -> 'pull_said'))) FROM callback_event WHERE doc -> 'handle' = v -> 'handle' AND doc -> 'delivered' IS DISTINCT FROM 'true'::jsonb AND (starts_with(doc ->> 'key', 'pull-') OR starts_with(doc ->> 'key', 'event-') OR (starts_with(doc ->> 'key', 'pane-') AND starts_with(doc ->> 'text', 'BLOCKED ')))), '[]'))",
    )
}
pub(in crate::store) fn unqueued_chat() -> Pg<'static> {
    Pg::read(format!(
        "coalesce((SELECT jsonb_agg(jsonb_build_object('key', c.doc -> 'key', 'text', c.doc -> 'text') ORDER BY {}) FROM chat_event c WHERE c.doc -> 'delivered' = 'false' AND NOT EXISTS (SELECT 1 FROM callback_event e WHERE e.id = jsonb_build_array(v -> 'handle', 'chat-' || (c.doc ->> 'key')))), '[]')",
        natural("c.doc ->> 'key'")
    ))
}
pub(in crate::store) fn pending() -> Pg<'static> {
    Pg::read(format!(
        "EXISTS (SELECT 1 FROM callback_event e WHERE {DUE} AND {CHAT})"
    ))
}
pub(in crate::store) fn due() -> Pg<'static> {
    let order = order();
    Pg::script(format!("IF (SELECT doc -> 'revision' FROM callback_lease WHERE id = v -> 'handle') = v -> 'revision' THEN
UPDATE callback_lease SET doc = doc || jsonb_build_object('until', (v ->> 'now')::double precision + (v ->> 'lease_seconds')::double precision) WHERE id = v -> 'handle';
UPDATE callback_event e SET doc = doc || '{{\"delivered\":true}}' WHERE doc -> 'handle' = v -> 'handle' AND doc -> 'delivered' IS DISTINCT FROM 'true'::jsonb AND doc ? 'message' AND EXISTS (SELECT 1 FROM delivery d WHERE d.id = jsonb_build_array(e.doc -> 'message', v -> 'handle'));
DELETE FROM callback_event WHERE doc -> 'handle' = v -> 'handle' AND doc -> 'delivered' = 'true';
SELECT coalesce(jsonb_agg(e.doc ORDER BY {order}), '[]') INTO board_answer FROM callback_event e WHERE {DUE} AND {CHAT};
END IF;"))
}
pub(in crate::store) fn yield_chat_seat() -> Pg<'static> {
    Pg::script(
        "UPDATE callback_state SET doc = doc || jsonb_build_object('chat', false, 'chat_fallback', false, 'chat_resume', true, 'chat_resume_seat', coalesce(doc -> 'chat', 'false'), 'chat_resume_fallback', coalesce(doc -> 'chat_fallback', 'false')) WHERE id = v -> 'handle' AND doc -> 'address' IS NOT DISTINCT FROM v -> 'address' AND doc -> 'owner' IS NOT DISTINCT FROM v -> 'owner' AND doc -> 'chat_resume' IS DISTINCT FROM 'true'::jsonb;",
    )
}
pub(in crate::store) fn retire(held: bool) -> Pg<'static> {
    let guard = if held { super::launches::HOLDS } else { "" };
    Pg::script(format!("{guard}
DELETE FROM callback_state WHERE id = v -> 'handle' AND doc -> 'address' IS NOT DISTINCT FROM v -> 'address' AND doc -> 'owner' IS NOT DISTINCT FROM v -> 'owner' AND doc -> 'terminal' IS NOT DISTINCT FROM v -> 'terminal' AND coalesce(doc -> 'registration_revision', '0') = v -> 'revision' AND doc -> 'registration_nonce' IS NOT DISTINCT FROM v -> 'nonce';
board_answer := to_jsonb(FOUND);
IF FOUND THEN UPDATE callback_event SET doc = doc || '{{\"delivered\":true,\"text\":\"\"}}' WHERE doc -> 'handle' = v -> 'handle' AND starts_with(doc ->> 'key', 'chat-'); END IF;"))
}
pub(in crate::store) fn failure_logged() -> Pg<'static> {
    Pg::script(super::logged(
        "v ->> 'slug'",
        "'callbacks'",
        "'system'",
        "v ->> 'text'",
        "NULL",
    ))
}

/// Record the PULL actually delivered, preserving the first occurrence of each unanswered entry.
fn told(handle: &str) -> String {
    format!("IF pull_ IS NOT NULL THEN
UPDATE callback_state SET doc = jsonb_strip_nulls((doc - ARRAY['pull_awaiting','pull_queued']) || jsonb_build_object(
'pull_unanswered', coalesce((SELECT jsonb_agg(entry ORDER BY first_) FROM (SELECT entry, min(at_) first_ FROM jsonb_array_elements(coalesce(doc -> 'pull_unanswered', '[]') || coalesce((SELECT jsonb_agg(e) FROM jsonb_array_elements(pull_ -> 'pull_entries') e WHERE e ->> 0 = 'unanswered' AND NOT coalesce(pull_ -> 'pull_withheld', '[]') @> jsonb_build_array(e)), '[]')) WITH ORDINALITY x(entry, at_) GROUP BY entry) united_), '[]'),
'pull_entries', pull_ -> 'pull_entries', 'pull_withheld', coalesce(pull_ -> 'pull_withheld', '[]'), 'pull_said', pull_ -> 'pull_said', 'pull_idle', coalesce(pull_ #> '{{pull_said,idle}}', doc -> 'pull_idle'))) WHERE id = {handle};
END IF;")
}
pub(in crate::store) fn ack() -> Pg<'static> {
    Pg::script(format!("{}
DECLARE pull_ jsonb;
BEGIN
SELECT doc INTO pull_ FROM callback_event WHERE doc -> 'handle' = v -> 'actor' AND doc ->> 'key' IN (SELECT jsonb_array_elements_text(v -> 'keys')) AND doc ? 'pull_entries' ORDER BY (doc ->> 'sequence')::bigint DESC LIMIT 1;
{}
UPDATE callback_event SET doc = doc || '{{\"delivered\":true}}' WHERE id IN (SELECT jsonb_build_array(v -> 'actor', k) FROM jsonb_array_elements(v -> 'keys') k);
UPDATE chat_event SET doc = doc || '{{\"delivered\":true,\"text\":\"\"}}' WHERE id IN (SELECT jsonb_array_elements(v -> 'chats'));
UPDATE callback_event SET doc = doc || '{{\"text\":\"\"}}' WHERE doc -> 'handle' = v -> 'actor' AND doc -> 'delivered' = 'true' AND starts_with(doc ->> 'key', 'chat-');
END;", super::act(), told("v -> 'actor'")))
}

pub(in crate::store) fn record_delivery() -> Pg<'static> {
    let marker = super::schema::MARKER;
    Pg::script(format!("DECLARE before_ jsonb; pull_ jsonb := v -> 'pull';
BEGIN
SELECT to_jsonb({marker}) INTO before_;
UPDATE callback_event SET doc = doc || jsonb_build_object('attempts', (doc ->> 'attempts')::bigint + 1, 'delivered', v -> 'success', 'retry_at', (v ->> 'now')::double precision + 5, 'capacity_limited', (v ->> 'capacity')::boolean OR coalesce((doc ->> 'capacity_limited')::boolean, false)) WHERE id IN (SELECT jsonb_build_array(v -> 'handle', k) FROM jsonb_array_elements(v -> 'keys') k);
IF (v ->> 'success')::boolean THEN
{}
INSERT INTO delivery (id, message, handle, acked) SELECT jsonb_build_array(m::bigint, v -> 'handle'), m::bigint, v ->> 'handle', (v ->> 'now')::double precision FROM jsonb_array_elements_text(v -> 'messages') m ON CONFLICT (id) DO UPDATE SET acked = EXCLUDED.acked;
UPDATE chat_event SET doc = doc || '{{\"delivered\":true,\"text\":\"\"}}' WHERE id IN (SELECT jsonb_array_elements(v -> 'chat'));
UPDATE callback_event SET doc = doc || '{{\"text\":\"\"}}' WHERE doc -> 'handle' = v -> 'handle' AND doc -> 'delivered' = 'true' AND starts_with(doc ->> 'key', 'chat-');
END IF;
board_answer := jsonb_build_object('before', before_, 'after', {marker});
END;", told("v -> 'handle'")))
}

pub(in crate::store) fn account() -> Pg<'static> {
    let marker = super::schema::MARKER;
    Pg::script(format!("DECLARE marker_ jsonb; held_ boolean; current_ boolean; before_ jsonb; event_ jsonb; line_ jsonb; prefix_ text;
BEGIN
SELECT to_jsonb({marker}) INTO marker_;
held_ := coalesce((SELECT doc -> 'revision' = v -> 'revision' FROM callback_lease WHERE id = v -> 'handle'), false);
current_ := NOT EXISTS (SELECT 1 FROM jsonb_array_elements(v -> 'waiting') k WHERE NOT EXISTS (SELECT 1 FROM callback_event WHERE id = jsonb_build_array(v -> 'handle', k) AND doc -> 'delivered' IS DISTINCT FROM 'true'::jsonb));
IF held_ AND current_ THEN
    -- Account and takeover must write the same lease even when no PULL was computed.
    UPDATE callback_lease SET doc = doc WHERE id = v -> 'handle';
    IF v ? 'pull_kept' THEN
        DELETE FROM callback_event WHERE doc -> 'handle' = v -> 'handle' AND doc -> 'delivered' IS DISTINCT FROM 'true'::jsonb AND starts_with(doc ->> 'key', 'pull-') AND doc -> 'key' <> v -> 'pull_kept';
        UPDATE callback_event SET doc = jsonb_strip_nulls(doc || jsonb_build_object('text', v -> 'pull_text', 'pull_entries', v #> '{{pull_awaiting,entries}}', 'pull_withheld', v #> '{{pull_awaiting,withheld}}', 'pull_said', v -> 'pull_said')) WHERE id = jsonb_build_array(v -> 'handle', v -> 'pull_kept') AND doc -> 'delivered' IS DISTINCT FROM 'true'::jsonb;
    END IF;
    IF (v ->> 'replace_pull')::boolean THEN DELETE FROM callback_event WHERE doc -> 'handle' = v -> 'handle' AND doc -> 'delivered' IS DISTINCT FROM 'true'::jsonb AND starts_with(doc ->> 'key', 'pull-'); END IF;
    DELETE FROM callback_event WHERE id IN (SELECT jsonb_build_array(v -> 'handle', k) FROM jsonb_array_elements(v #> '{{rescoped,dropped}}') k) AND doc -> 'delivered' IS DISTINCT FROM 'true'::jsonb;
    FOR line_ IN SELECT jsonb_array_elements(v #> '{{rescoped,rewritten}}') LOOP
        UPDATE callback_event SET doc = doc || jsonb_build_object('text', line_ -> 1) WHERE id = jsonb_build_array(v -> 'handle', line_ -> 0) AND doc -> 'delivered' IS DISTINCT FROM 'true'::jsonb;
    END LOOP;
    FOR event_ IN SELECT e FROM jsonb_array_elements(v -> 'events') e WHERE starts_with(e ->> 'key', 'event-') AND starts_with(e ->> 'text', 'NEW ') LOOP
        prefix_ := 'NEW ' || split_part(event_ ->> 'text', ' ', 2) || ' ready — ';
        DELETE FROM callback_event WHERE doc -> 'handle' = v -> 'handle' AND doc -> 'delivered' IS DISTINCT FROM 'true'::jsonb AND starts_with(doc ->> 'key', 'event-') AND starts_with(doc ->> 'text', prefix_);
    END LOOP;
    {}
    IF (v ->> 'computed')::boolean THEN UPDATE callback_lease SET doc = jsonb_strip_nulls(doc || jsonb_build_object('pull', v -> 'pull')) WHERE id = v -> 'handle' AND doc -> 'revision' = v -> 'revision'; END IF;
    SELECT doc INTO before_ FROM callback_state WHERE id = v -> 'handle';
    IF before_ -> 'seen' IS DISTINCT FROM v -> 'seen' OR before_ -> 'narrowing' IS DISTINCT FROM v -> 'narrowing' OR before_ -> 'unknown_children' IS DISTINCT FROM v -> 'unknown_children' OR before_ -> 'statuses' IS DISTINCT FROM v -> 'statuses' OR before_ -> 'pull_queued' IS DISTINCT FROM v -> 'pull_queued' OR before_ -> 'inbox_cursor' IS DISTINCT FROM v -> 'inbox_cursor' OR coalesce((before_ ->> 'message_cursor')::bigint, 0) < (v ->> 'message_cursor')::bigint OR NOT coalesce(before_, '{{}}') ? 'delivery_version' OR coalesce(before_ -> 'registration_revision', '0') <> coalesce(before_ -> 'observed_registration', '0') OR (v ->> 'computed')::boolean THEN
        UPDATE callback_state SET doc = jsonb_strip_nulls(doc || jsonb_build_object('seen', v -> 'seen', 'narrowing', v -> 'narrowing', 'unknown_children', v -> 'unknown_children', 'snapshot_sequence', v -> 'snapshot_sequence', 'statuses', v -> 'statuses', 'revision', v -> 'revision', 'pull', v -> 'pull', 'pull_queued', v -> 'pull_queued', 'observed_registration', coalesce(doc -> 'registration_revision', '0'),
        'pull_entries', CASE WHEN (v ->> 'legacy_upgrade')::boolean THEN coalesce(v #> '{{pull_account,recorded}}', doc -> 'pull_entries', '[]') WHEN (v ->> 'computed')::boolean AND doc ? 'pull_entries' THEN
            coalesce((SELECT jsonb_agg(entry ORDER BY first_) FROM (SELECT entry, min(at_) first_ FROM jsonb_array_elements(coalesce((SELECT jsonb_agg(e) FROM jsonb_array_elements(doc -> 'pull_entries') e WHERE (v -> 'pull_current') @> jsonb_build_array(e) AND NOT (v #> '{{pull_account,untold}}') @> jsonb_build_array(e)), '[]') || (v #> '{{pull_account,unheard}}')) WITH ORDINALITY x(entry, at_) GROUP BY entry) united_), '[]') ELSE doc -> 'pull_entries' END,
        'pull_withheld', CASE WHEN (v ->> 'computed')::boolean THEN v #> '{{pull_account,withheld}}' ELSE doc -> 'pull_withheld' END,
        'pull_awaiting', CASE WHEN (v ->> 'computed')::boolean THEN v -> 'pull_awaiting' ELSE doc -> 'pull_awaiting' END,
        'message_cursor', greatest(coalesce((doc ->> 'message_cursor')::bigint, 0), (v ->> 'message_cursor')::bigint), 'inbox_cursor', v -> 'inbox_cursor', 'delivery_version', coalesce(doc -> 'delivery_version', '1'))) WHERE id = v -> 'handle';
    END IF;
END IF;
board_answer := jsonb_build_object('held', held_, 'current', current_, 'marker', jsonb_build_object('before', marker_, 'after', {marker}));
END;", super::pane_watch::EVENTS))
}
