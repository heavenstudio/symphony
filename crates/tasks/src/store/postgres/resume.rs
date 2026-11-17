//! The statements of `store/resume.rs` on PostgreSQL.

use super::{Pg, cap, work};

pub(in crate::store) fn candidates() -> Pg<'static> {
    Pg::read(
        "coalesce((SELECT jsonb_agg(slug ORDER BY slug) FROM task WHERE status = 'blocked' AND fields #>> '{_board,blocked,on}' LIKE 'task:%'), '[]')",
    )
}

pub(in crate::store) fn prerequisites() -> Pg<'static> {
    Pg::read(
        "coalesce((SELECT jsonb_agg(jsonb_build_object('slug', slug, 'status', status, 'version', version) ORDER BY slug) FROM task WHERE slug IN (SELECT jsonb_array_elements_text(v -> 'required'))), '[]')",
    )
}

pub(in crate::store) fn resume() -> Pg<'static> {
    Pg::script(format!(
        "DECLARE g jsonb; BEGIN
        IF (SELECT version FROM task WHERE slug = v ->> 'slug') IS DISTINCT FROM (v ->> 'version')::bigint THEN RAISE EXCEPTION 'stale'; END IF;
        FOR g IN SELECT jsonb_array_elements(v -> 'watch') LOOP
            IF (SELECT version FROM task WHERE slug = g ->> 'slug') IS DISTINCT FROM (g ->> 'version')::bigint THEN RAISE EXCEPTION 'stale'; END IF;
            UPDATE task SET version = version + 1 WHERE slug = g ->> 'slug';
        END LOOP;
        UPDATE task SET status = v ->> 'status', updated = (v ->> 'now')::double precision, version = version + 1 WHERE slug = v ->> 'slug';
        {}{}{}\n{}\nEND;",
        work::UNBLOCKED, cap::UNHOLD, cap::GUARD,
        super::logged("v ->> 'slug'", "'board'", "'system'", "v ->> 'said'", "NULL"),
    ))
}
