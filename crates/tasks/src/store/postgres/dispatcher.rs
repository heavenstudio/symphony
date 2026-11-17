//! Host dispatch leases: asks, completed passes and lease revisions.
use super::Pg;

const KEY: &str = "jsonb_build_array('dispatch', v -> 'host')";

pub(in crate::store) fn take() -> Pg<'static> {
    Pg::script(format!("DECLARE lease_ jsonb; revision_ bigint; ask_ bigint;
BEGIN
    INSERT INTO callback_lease (id, doc) VALUES ({KEY}, '{{}}') ON CONFLICT (id) DO UPDATE SET doc = callback_lease.doc;
    SELECT doc INTO lease_ FROM callback_lease WHERE id = {KEY};
    IF coalesce((lease_ ->> 'until')::double precision, 0) > (v ->> 'now')::double precision THEN
        ask_ := coalesce((lease_ ->> 'asked')::bigint, 0) + 1;
        UPDATE callback_lease SET doc = doc || jsonb_build_object('asked', ask_) WHERE id = {KEY};
        board_answer := jsonb_build_object('ask', ask_);
    ELSE
        revision_ := coalesce((lease_ ->> 'revision')::bigint, 0) + 1;
        UPDATE callback_lease SET doc = doc || jsonb_build_object('until', (v ->> 'now')::double precision + (v ->> 'lease_seconds')::double precision, 'revision', revision_, 'asked', coalesce(lease_ -> 'asked', '0'), 'served', coalesce(lease_ -> 'asked', '0'), 'completed', coalesce(lease_ -> 'completed', '0')) WHERE id = {KEY};
        board_answer := jsonb_build_object('taken', revision_, 'began', coalesce(lease_ -> 'asked', '0'));
    END IF;
END;"))
}

pub(in crate::store) fn lease() -> Pg<'static> {
    Pg::read(format!(
        "jsonb_build_object('completed', coalesce((SELECT doc -> 'completed' FROM callback_lease WHERE id = {KEY}), '0'), 'until', coalesce((SELECT doc -> 'until' FROM callback_lease WHERE id = {KEY}), '0'))"
    ))
}

pub(in crate::store) fn let_go() -> Pg<'static> {
    Pg::script(format!(
        "UPDATE callback_lease SET doc = doc || '{{\"until\":0}}' WHERE id = {KEY} AND doc -> 'revision' = v -> 'revision';"
    ))
}

pub(in crate::store) fn passed() -> Pg<'static> {
    Pg::script(format!("DECLARE lease_ jsonb;
BEGIN
    SELECT doc INTO lease_ FROM callback_lease WHERE id = {KEY};
    UPDATE callback_lease SET doc = doc || jsonb_build_object('completed', greatest(coalesce((doc ->> 'completed')::bigint, 0), (v ->> 'began')::bigint)) WHERE id = {KEY};
    IF lease_ -> 'revision' IS DISTINCT FROM v -> 'revision' THEN
        board_answer := '{{\"next\":\"lapsed\"}}';
    ELSIF coalesce((lease_ ->> 'asked')::bigint, 0) > coalesce((lease_ ->> 'served')::bigint, 0) OR (v ->> 'more')::boolean THEN
        UPDATE callback_lease SET doc = doc || jsonb_build_object('served', coalesce(doc -> 'asked', '0'), 'until', (v ->> 'now')::double precision + (v ->> 'lease_seconds')::double precision) WHERE id = {KEY};
        board_answer := jsonb_build_object('next', 'again', 'began', coalesce(lease_ -> 'asked', '0'));
    ELSE
        UPDATE callback_lease SET doc = doc || '{{\"until\":0}}' WHERE id = {KEY};
        board_answer := '{{\"next\":\"done\"}}';
    END IF;
END;"))
}
