//! Saved pane observations and their generated outbox lines.
use super::Pg;

/// The pane lines still queued for the recipient `of` names (its `handle`).
fn queued_of(of: &str) -> String {
    format!(
        "coalesce((SELECT jsonb_agg(jsonb_build_object('key', doc -> 'key', 'text', doc -> 'text')) FROM callback_event WHERE doc ->> 'handle' = {of} ->> 'handle' AND doc -> 'delivered' IS DISTINCT FROM 'true'::jsonb AND NOT doc ? 'message' AND starts_with(doc ->> 'key', 'pane-')), '[]')"
    )
}
pub(in crate::store) fn queued() -> Pg<'static> {
    Pg::read(super::callbacks::each_recipient(&format!(
        "'queued', {}",
        queued_of("r")
    )))
}
pub(in crate::store) fn saved() -> Pg<'static> {
    Pg::read(format!(
        "jsonb_build_object('watch', coalesce((SELECT doc -> 'pane_watch' FROM callback_state WHERE id = v -> 'handle'), '{{}}'), 'queued', {})",
        queued_of("v")
    ))
}

/// Queue each event once; the per-recipient counter serializes new sequence numbers.
pub(in crate::store) const EVENTS: &str = "FOR event_ IN SELECT jsonb_array_elements(v -> 'events') LOOP
    IF NOT EXISTS (SELECT 1 FROM callback_event WHERE id = jsonb_build_array(v -> 'handle', event_ -> 'key')) THEN
        INSERT INTO counter (id, last) VALUES (jsonb_build_array('callback-event', v -> 'handle'), 1) ON CONFLICT (id) DO UPDATE SET last = counter.last + 1 RETURNING last INTO sid_;
        INSERT INTO callback_event (id, doc) VALUES (jsonb_build_array(v -> 'handle', event_ -> 'key'), event_ || jsonb_build_object('sequence', sid_)) ON CONFLICT (id) DO NOTHING;
    END IF;
END LOOP;";
pub(in crate::store) fn record() -> Pg<'static> {
    Pg::script(format!("DELETE FROM callback_event WHERE doc ->> 'handle' = v ->> 'handle' AND doc -> 'delivered' IS DISTINCT FROM 'true'::jsonb AND NOT doc ? 'message' AND doc ->> 'key' IN (SELECT jsonb_array_elements_text(v -> 'retired'));
DECLARE event_ jsonb;
BEGIN {EVENTS} END;
UPDATE callback_state SET doc = doc || jsonb_build_object('pane_watch', v -> 'after') WHERE id = v -> 'handle';"))
}
