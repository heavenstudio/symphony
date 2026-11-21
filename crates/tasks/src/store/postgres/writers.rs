//! The statements of `store/writers.rs` on PostgreSQL. The writers' table moves no marker: no
//! trigger is on it. An hourly line refresh also folds ended backends' marker rows into the
//! retained `['board']` row. Deletion and addition commit together, preserving the marker sum;
//! an active backend keeps its own row. Concurrent folds or a reused pid meet on those rows,
//! and a repeatable-read conflict rolls the complete refresh back.

use super::Pg;
use std::borrow::Cow;

/// The opening's one read: the board's schema version, or null before its tables are made, the
/// line of the writer `[host, path]`, or null, or `false` where the table is not there, and how
/// many additive steps on its contract the board has.
pub(in crate::store) const OPENING: Pg<'static> = Pg::Script(
    Cow::Borrowed(
        "board_answer := jsonb_build_array(NULL::jsonb, false, 0);
IF to_regclass('meta') IS NOT NULL THEN
    board_answer := jsonb_set(board_answer, '{0}', coalesce(
        (SELECT to_jsonb(version) FROM meta WHERE id = '\"schema\"'), 'null'::jsonb));
    board_answer := jsonb_set(board_answer, '{2}', coalesce(
        (SELECT to_jsonb(version) FROM meta WHERE id = '\"additions\"'), '0'::jsonb));
END IF;
IF to_regclass('writer') IS NOT NULL THEN
    board_answer := jsonb_set(board_answer, '{1}', coalesce(
        (SELECT jsonb_strip_nulls(jsonb_build_object('schema', doc -> 'schema', 'seen', doc -> 'seen'))
            FROM writer WHERE id = jsonb_build_array(v -> 'host', v -> 'path')), 'null'::jsonb));
END IF;",
    ),
    None,
);

/// Every writer's line, or none where the table is not there.
pub(in crate::store) const LINES: Pg<'static> = Pg::Script(
    Cow::Borrowed(
        "board_answer := '[]'::jsonb;
IF to_regclass('writer') IS NOT NULL THEN
    board_answer := coalesce((SELECT jsonb_agg(jsonb_strip_nulls(jsonb_build_object(
        'program', doc -> 'program', 'path', doc -> 'path', 'host', doc -> 'host',
        'schema', doc -> 'schema', 'seen', doc -> 'seen'))) FROM writer), '[]'::jsonb);
END IF;",
    ),
    None,
);

/// A writer's line, made or replaced, and ended-backend marker rows compacted at the matching
/// schema. The shared schema lock meets migration; readers never send this statement.
pub(in crate::store) const WRITE_LINE: Pg<'static> = Pg::Script(
    Cow::Borrowed(
        "IF (SELECT version FROM meta WHERE id = '\"schema\"' FOR SHARE)
    = (v ->> 'schema')::bigint THEN
    WITH ended AS (
        DELETE FROM counter c WHERE c.id ->> 0 = 'board'
            AND jsonb_typeof(c.id -> 1) = 'number'
            AND NOT EXISTS (SELECT 1 FROM pg_stat_activity a WHERE to_jsonb(a.pid) = c.id -> 1)
            RETURNING c.last
    )
    INSERT INTO counter (id, last)
        SELECT '[\"board\"]'::jsonb, sum(last)::bigint FROM ended HAVING count(*) > 0
        ON CONFLICT (id) DO UPDATE SET last = counter.last + EXCLUDED.last;
END IF;
INSERT INTO writer (id, doc) VALUES (jsonb_build_array(v -> 'host', v -> 'path'),
    jsonb_strip_nulls(jsonb_build_object('program', v -> 'program', 'path', v -> 'path',
        'host', v -> 'host', 'schema', v -> 'schema', 'seen', v -> 'seen')))
    ON CONFLICT (id) DO UPDATE SET doc = EXCLUDED.doc;",
    ),
    None,
);
