//! The statements of `store/people.rs` on PostgreSQL: a board's people read whole, its log, and a
//! change written fenced on the people revision it read.

use super::Pg;
use std::borrow::Cow;

/// Who is on the board, `{members, groups, revision}`: none, at revision 0, where the board lacks
/// the step that defines the tables.
pub(in crate::store) const PEOPLE: Pg<'static> = Pg::Script(
    Cow::Borrowed(
        "board_answer := jsonb_build_object('members', '[]'::jsonb, 'groups', '[]'::jsonb, 'revision', 0);
IF to_regclass('people_log') IS NOT NULL THEN
    board_answer := jsonb_build_object(
        'members', coalesce((SELECT jsonb_agg(doc ORDER BY doc ->> 'name') FROM people), '[]'::jsonb),
        'groups', coalesce((SELECT jsonb_agg(doc ORDER BY doc ->> 'name') FROM people_group), '[]'::jsonb),
        'revision', coalesce((SELECT (id #>> '{}')::bigint FROM people_log ORDER BY id DESC LIMIT 1), 0));
END IF;",
    ),
    None,
);

/// Every line of the people log, oldest first: none where the board lacks the step.
pub(in crate::store) const LOG: Pg<'static> = Pg::Script(
    Cow::Borrowed(
        "board_answer := '[]'::jsonb;
IF to_regclass('people_log') IS NOT NULL THEN
    board_answer := coalesce((SELECT jsonb_agg(doc ORDER BY id) FROM people_log), '[]'::jsonb);
END IF;",
    ),
    None,
);

/// The change `line` written, with the rows it puts whole (`members`, `groups`) and those it takes
/// away (`members_gone`, `groups_gone`), only while the board's newest change is still the one
/// numbered `expected` (0: none): whether it was. Two changes that read the same revision both
/// write the line numbered after it: the one that commits second meets the first's key, which its
/// snapshot cannot see, and fails to serialize, so it is tried again from a new read; a line its
/// snapshot sees under the key writes nothing and answers that it was not.
pub(in crate::store) fn change() -> Pg<'static> {
    Pg::script(
        "
IF coalesce((SELECT (id #>> '{}')::bigint FROM people_log ORDER BY id DESC LIMIT 1), 0)
        <> (v ->> 'expected')::bigint THEN
    board_answer := 'false'::jsonb;
ELSE
    INSERT INTO people_log (id, doc) VALUES (v -> 'line' -> 'n', v -> 'line')
        ON CONFLICT (id) DO NOTHING;
    IF NOT FOUND THEN
        board_answer := 'false'::jsonb;
    ELSE
        INSERT INTO people (id, doc)
            SELECT r -> 'name', r FROM jsonb_array_elements(v -> 'members') AS r
            ON CONFLICT (id) DO UPDATE SET doc = EXCLUDED.doc;
        DELETE FROM people WHERE id IN (SELECT jsonb_array_elements(v -> 'members_gone'));
        INSERT INTO people_group (id, doc)
            SELECT r -> 'name', r FROM jsonb_array_elements(v -> 'groups') AS r
            ON CONFLICT (id) DO UPDATE SET doc = EXCLUDED.doc;
        DELETE FROM people_group WHERE id IN (SELECT jsonb_array_elements(v -> 'groups_gone'));
        board_answer := 'true'::jsonb;
    END IF;
END IF;
",
    )
}
