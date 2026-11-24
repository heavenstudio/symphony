//! Token usage recorded atomically with the beat of its bound session.
use super::{Pg, listed, rows};

/// `s_` is the beat's session. Choose one working role and add each usage sample.
pub(in crate::store) const SPEND: &str = "DECLARE on_ record; slug_ text; role_ text; round_ bigint; x_ jsonb;
BEGIN
    SELECT r.slug, r.role INTO on_ FROM role r JOIN task t ON t.slug = r.slug
    WHERE r.handle = s_.handle AND r.role <> 'orchestrator' AND t.status NOT IN (SELECT jsonb_array_elements_text(v -> 'finished'))
    ORDER BY CASE r.role WHEN 'executor' THEN 0 WHEN 'reviewer' THEN 1 ELSE 2 END, t.priority, r.slug LIMIT 1;
    slug_ := coalesce(on_.slug, ''); role_ := coalesce(on_.role, s_.kind);
    round_ := CASE WHEN on_.slug IS NULL THEN 0 ELSE (SELECT count(*) + 1 FROM message WHERE slug = on_.slug AND kind = 'system' AND strpos(text, v ->> 'moved') > 0 AND right(text, length(v ->> 'into_review')) = v ->> 'into_review') END;
    FOR x_ IN SELECT jsonb_array_elements(v -> 'spent') LOOP
        INSERT INTO usage (id, handle, slug, role, model, speed, round, input, cache_write_5m, cache_write_1h, cache_read, output, calls, first, last)
        VALUES (jsonb_build_array(s_.handle, slug_, role_, x_ -> 'model', x_ -> 'speed', round_), s_.handle, slug_, role_, x_ ->> 'model', x_ ->> 'speed', round_,
            (x_ #>> '{tokens,input}')::bigint, (x_ #>> '{tokens,cache_write_5m}')::bigint, (x_ #>> '{tokens,cache_write_1h}')::bigint, (x_ #>> '{tokens,cache_read}')::bigint, (x_ #>> '{tokens,output}')::bigint, (x_ #>> '{tokens,calls}')::bigint, (v ->> 'now')::double precision, (v ->> 'now')::double precision)
        ON CONFLICT (id) DO UPDATE SET input = usage.input + EXCLUDED.input, cache_write_5m = usage.cache_write_5m + EXCLUDED.cache_write_5m, cache_write_1h = usage.cache_write_1h + EXCLUDED.cache_write_1h, cache_read = usage.cache_read + EXCLUDED.cache_read, output = usage.output + EXCLUDED.output, calls = usage.calls + EXCLUDED.calls, last = EXCLUDED.last;
    END LOOP;
END;";

pub(in crate::store) fn spent_on() -> Pg<'static> {
    listed(
        "u",
        "usage u WHERE slug IN (SELECT jsonb_array_elements_text(v -> 'slugs'))",
        "u.handle, u.slug, u.role, u.model, u.speed, u.round",
    )
}
/// The rows spent on `slugs` and on their subtasks, and each such subtask's parent.
pub(in crate::store) fn spent_under() -> Pg<'static> {
    let slugs = "(SELECT jsonb_array_elements_text(v -> 'slugs'))";
    Pg::read(format!(
        "(WITH under AS (SELECT slug, parent FROM task WHERE parent IN {slugs}) \
         SELECT jsonb_build_object('rows', {}, 'under', coalesce((SELECT jsonb_agg(\
         jsonb_build_object('slug', slug, 'parent', parent) ORDER BY slug) FROM under), '[]')))",
        rows(
            "u",
            &format!("usage u WHERE u.slug IN {slugs} OR u.slug IN (SELECT slug FROM under)"),
            "u.id"
        )
    ))
}

pub(in crate::store) fn spent_by() -> Pg<'static> {
    listed(
        "u",
        "usage u WHERE handle IN (SELECT jsonb_array_elements_text(v -> 'handles'))",
        "u.id",
    )
}
