//! The statements of `store/waits.rs` on PostgreSQL.

use super::work::said;
use super::{Pg, act};
use crate::alarm::{ASSESSED, FIXED, WAITS};
use crate::red::FIELDS;

/// Task `slug`'s status and version, as one row; none for a slug no task has.
pub(in crate::store) fn assessment_state() -> Pg<'static> {
    Pg::read(
        "coalesce((SELECT jsonb_agg(jsonb_build_object('slug', slug, 'status', status, \
    'version', version)) FROM task WHERE slug = v ->> 'slug'), '[]')",
    )
}

/// The time of the line just written (`ts_`) as a JSON float, a time of a whole second too
/// (`1800000000.0`), as the reader of a task's fields compares it.
const WRITTEN_AT: &str =
    "CASE WHEN ts_ = trunc(ts_) THEN to_jsonb(round(ts_::numeric, 1)) ELSE to_jsonb(ts_) END";

/// What task `slug`'s landing waits on, `record` of the assessment `assessment`, written with its
/// line unless that pair was committed before or stands: the pair standing is kept among the
/// task's assessments first. `acting`, the actor is a session, checked and its activity recorded.
pub(in crate::store) fn record(acting: bool) -> Pg<'static> {
    let act = if acting { act() } else { String::new() };
    let kept = |id: &str, record: &str, at: &str, sid: &str| {
        format!(
            "UPDATE task SET fields = board_fields(id, fields, ARRAY['{FIELDS}', 'assessments', {id}, {record}], \
            jsonb_build_object('at', {at}, 'sid', {sid})) WHERE slug = v ->> 'slug';"
        )
    };
    Pg::script(format!(
        "DECLARE \
    board_ jsonb; \
BEGIN \
    SELECT fields -> '{FIELDS}' INTO board_ FROM task WHERE slug = v ->> 'slug'; \
    IF NOT FOUND THEN \
        RAISE EXCEPTION '%', v ->> 'missing'; \
    END IF; \
    IF board_ -> 'assessments' -> (v ->> 'assessment') -> (v ->> 'record') IS NULL \
        AND NOT coalesce(board_ ->> '{WAITS}' = v ->> 'record' \
            AND board_ -> '{ASSESSED}' ->> 'id' = v ->> 'assessment', false) THEN \
        {act} \
        IF v ? 'source_error' THEN \
            RAISE EXCEPTION '%', v ->> 'source_error'; \
        END IF; \
        IF NOT (v ->> 'source_current')::boolean THEN \
            RAISE EXCEPTION '%', v ->> 'source_changed'; \
        END IF; \
        IF (v ->> 'checked')::boolean AND EXISTS (SELECT 1 FROM task WHERE slug = v ->> 'slug' \
                AND (status IS DISTINCT FROM v ->> 'expected_status' \
                    OR version IS DISTINCT FROM (v ->> 'expected_version')::bigint)) THEN \
            RAISE EXCEPTION '%', v ->> 'changed'; \
        END IF; \
        IF board_ -> '{ASSESSED}' -> 'id' IS NOT NULL AND board_ -> '{WAITS}' IS NOT NULL THEN \
            {} \
        END IF; \
        UPDATE task SET fields = board_fields(id, fields, '{{{FIELDS},{WAITS}}}', v -> 'record'), \
            version = version + 1 WHERE slug = v ->> 'slug'; \
        {} \
        UPDATE task SET fields = board_fields(id, fields, '{{{FIELDS},{ASSESSED}}}', \
            jsonb_build_object('id', v -> 'assessment', 'at', {WRITTEN_AT}, 'sid', sid_)) \
            WHERE slug = v ->> 'slug'; \
        {} \
    END IF; \
END;",
        kept(
            &format!("board_ -> '{ASSESSED}' ->> 'id'"),
            &format!("board_ ->> '{WAITS}'"),
            &format!("board_ -> '{ASSESSED}' -> 'at'"),
            &format!("board_ -> '{ASSESSED}' -> 'sid'"),
        ),
        said("(v ->> 'actor') || (v ->> 'set') || (v ->> 'record')"),
        kept("v ->> 'assessment'", "v ->> 'record'", WRITTEN_AT, "sid_"),
    ))
}

/// Where the rework the record `written` of the tip `judged` asked for ended, kept on task `slug`
/// (`patch`) unless one is kept for that record, answering what is kept after.
pub(in crate::store) fn fixed() -> Pg<'static> {
    let kept = format!("ARRAY['{FIELDS}', '{FIXED}', v ->> 'judged', v ->> 'written']");
    Pg::script_then(
        format!(
            "UPDATE task SET fields = board_fields(id, fields, {kept}, v -> 'patch' #> {kept}) \
    WHERE slug = v ->> 'slug' AND fields #> {kept} IS NULL;"
        ),
        format!("(SELECT fields #> {kept} FROM task WHERE slug = v ->> 'slug')"),
    )
}
