//! The statements of `store/writers.rs` on PostgreSQL. The writers' table moves no marker: no
//! trigger is on it.

use super::Pg;
use std::borrow::Cow;

/// The opening's one read: the board's schema version, or null before its tables are made, and
/// the line of the writer `[host, path]`, or null, or `false` where the table is not there.
pub(in crate::store) const OPENING: Pg<'static> = Pg::Script(
    Cow::Borrowed(
        "board_answer := jsonb_build_array(NULL::jsonb, false);
IF to_regclass('meta') IS NOT NULL THEN
    board_answer := jsonb_set(board_answer, '{0}', coalesce(
        (SELECT to_jsonb(version) FROM meta WHERE id = '\"schema\"'), 'null'::jsonb));
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

/// A writer's line, made or replaced.
pub(in crate::store) const WRITE_LINE: Pg<'static> = Pg::Script(
    Cow::Borrowed(
        "INSERT INTO writer (id, doc) VALUES (jsonb_build_array(v -> 'host', v -> 'path'),
    jsonb_strip_nulls(jsonb_build_object('program', v -> 'program', 'path', v -> 'path',
        'host', v -> 'host', 'schema', v -> 'schema', 'seen', v -> 'seen')))
    ON CONFLICT (id) DO UPDATE SET doc = EXCLUDED.doc;",
    ),
    None,
);
