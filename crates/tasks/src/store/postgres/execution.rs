//! The statements of `store/execution.rs` on PostgreSQL. Every time they compare or write is the
//! server's own clock (`clock_timestamp()`, in milliseconds), never a caller's. A row is held whole
//! in `doc`, its record without its id. A write that two transactions race on meets the other at a
//! row both write: a fence is made by
//! `INSERT … ON CONFLICT DO UPDATE`, which under REPEATABLE READ fails the later one as a conflict,
//! tried again, rather than as a duplicate key.

use super::Pg;

/// The database's clock in milliseconds.
const NOW: &str = "(extract(epoch from clock_timestamp()) * 1000)::bigint";

/// The attempt `v ->> 'attempt'`'s row id.
const ATTEMPT_ID: &str = "to_jsonb(v ->> 'attempt')";

pub(in crate::store) fn admit() -> Pg<'static> {
    Pg::script(format!(
        "<<admit>>
DECLARE
    now_ bigint := {NOW};
    known_ jsonb;
    current_ jsonb;
    held_ jsonb := '[]';
    handed_ boolean := false;
    before_ jsonb;
    fact_ jsonb;
BEGIN
    IF v ? 'request' THEN
        SELECT doc INTO known_ FROM execution_fence WHERE id = jsonb_build_array('request', v -> 'request');
        IF known_ IS NOT NULL THEN
            board_answer := jsonb_build_object('outcome', 'existing', 'attempt', known_ -> 'attempt', 'digest', known_ -> 'digest');
            EXIT admit;
        END IF;
    END IF;
    SELECT doc -> 'attempt' INTO current_ FROM execution_fence
        WHERE id = jsonb_build_array('lane', v -> 'task', v -> 'lane');
    IF v ? 'predecessor' AND current_ IS DISTINCT FROM v -> 'predecessor' THEN
        board_answer := jsonb_build_object('outcome', 'busy', 'attempt', current_);
        EXIT admit;
    END IF;
    IF NOT v ? 'predecessor' AND current_ IS NOT NULL
        AND coalesce((SELECT (doc ->> 'live')::boolean FROM execution_attempt WHERE id = current_), false) THEN
        board_answer := jsonb_build_object('outcome', 'busy', 'attempt', current_);
        EXIT admit;
    END IF;
    IF v ? 'follow' THEN
        SELECT doc INTO before_ FROM execution_attempt WHERE id = to_jsonb(v -> 'follow' ->> 'attempt');
        IF before_ IS NULL
            OR (before_ ->> 'revision')::bigint IS DISTINCT FROM (v -> 'follow' ->> 'revision')::bigint
            OR (before_ ->> 'facts')::bigint IS DISTINCT FROM (v -> 'follow' ->> 'facts_from')::bigint THEN
            RAISE EXCEPTION 'stale';
        END IF;
    END IF;
    IF v ? 'capacity' THEN
        held_ := coalesce((SELECT doc -> 'reserved' FROM execution_fence
            WHERE id = jsonb_build_array('capacity', v -> 'capacity' -> 'scope')), '[]');
        handed_ := coalesce(v -> 'follow' -> 'release' = v -> 'capacity' -> 'scope'
            AND held_ @> jsonb_build_array(v -> 'follow' -> 'attempt'), false);
        IF NOT handed_ AND jsonb_array_length(held_) >= (v -> 'capacity' ->> 'limit')::bigint THEN
            board_answer := jsonb_build_object('outcome', 'full', 'reserved', held_);
            EXIT admit;
        END IF;
        IF handed_ THEN
            held_ := coalesce((SELECT jsonb_agg(held.value) FROM jsonb_array_elements(held_) AS held
                WHERE held.value IS DISTINCT FROM v -> 'follow' -> 'attempt'), '[]');
        END IF;
        INSERT INTO execution_fence (id, doc) VALUES (jsonb_build_array('capacity', v -> 'capacity' -> 'scope'),
                jsonb_build_object('key', jsonb_build_array('capacity', v -> 'capacity' -> 'scope'),
                    'reserved', held_ || jsonb_build_array(v -> 'attempt'), 'limit', v -> 'capacity' -> 'limit'))
            ON CONFLICT (id) DO UPDATE SET doc = excluded.doc;
    END IF;
    INSERT INTO execution_attempt (id, doc) VALUES ({ATTEMPT_ID}, jsonb_build_object(
        'attempt', v -> 'attempt', 'task', v -> 'task', 'configuration', v -> 'configuration', 'created', now_,
        'identity', v -> 'row' -> 'identity', 'state', v -> 'row' -> 'state', 'workflow', v -> 'row' -> 'workflow',
        'revision', 1, 'open', true, 'live', true, 'due', now_, 'epoch', 0,
        'facts', jsonb_array_length(v -> 'facts')));
    FOR fact_ IN SELECT value FROM jsonb_array_elements(v -> 'facts') LOOP
        INSERT INTO execution_fact (id, doc) VALUES (jsonb_build_array(v -> 'attempt', fact_ -> 'seq'),
            jsonb_build_object('attempt', v -> 'attempt', 'seq', fact_ -> 'seq', 'at', now_, 'fact', fact_ -> 'fact'));
    END LOOP;
    IF v ? 'request' THEN
        INSERT INTO execution_fence (id, doc) VALUES (jsonb_build_array('request', v -> 'request'),
                jsonb_build_object('key', jsonb_build_array('request', v -> 'request'),
                    'attempt', v -> 'attempt', 'digest', v -> 'digest'))
            ON CONFLICT (id) DO UPDATE SET doc = excluded.doc;
    END IF;
    INSERT INTO execution_fence (id, doc) VALUES (jsonb_build_array('lane', v -> 'task', v -> 'lane'),
            jsonb_build_object('key', jsonb_build_array('lane', v -> 'task', v -> 'lane'), 'attempt', v -> 'attempt'))
        ON CONFLICT (id) DO UPDATE SET doc = excluded.doc;
    IF v ? 'follow' THEN
        before_ := before_ || jsonb_build_object('state', v -> 'follow' -> 'state', 'open', v -> 'follow' -> 'open',
            'live', v -> 'follow' -> 'live', 'revision', (before_ ->> 'revision')::bigint + 1,
            'facts', (before_ ->> 'facts')::bigint + jsonb_array_length(coalesce(v -> 'follow' -> 'facts', '[]')));
        before_ := CASE WHEN v -> 'follow' ? 'due_in'
            THEN before_ || jsonb_build_object('due', now_ + (v -> 'follow' ->> 'due_in')::bigint)
            ELSE before_ - 'due' END;
        UPDATE execution_attempt SET doc = before_ WHERE id = to_jsonb(v -> 'follow' ->> 'attempt');
        FOR fact_ IN SELECT value FROM jsonb_array_elements(coalesce(v -> 'follow' -> 'facts', '[]')) LOOP
            INSERT INTO execution_fact (id, doc) VALUES (jsonb_build_array(v -> 'follow' -> 'attempt', fact_ -> 'seq'),
                jsonb_build_object('attempt', v -> 'follow' -> 'attempt', 'seq', fact_ -> 'seq', 'at', now_,
                    'fact', fact_ -> 'fact'));
        END LOOP;
        IF v -> 'follow' ? 'release' AND NOT handed_ THEN
            UPDATE execution_fence SET doc = jsonb_set(doc, '{{reserved}}', coalesce((SELECT jsonb_agg(held.value)
                    FROM jsonb_array_elements(coalesce(doc -> 'reserved', '[]')) AS held
                    WHERE held.value IS DISTINCT FROM v -> 'follow' -> 'attempt'), '[]'))
                WHERE id = jsonb_build_array('capacity', v -> 'follow' -> 'release');
        END IF;
    END IF;
    board_answer := jsonb_build_object('outcome', 'admitted', 'attempt', v -> 'attempt');
END admit;"
    ))
}

pub(in crate::store) fn claim() -> Pg<'static> {
    Pg::script(format!(
        "<<claim>>
DECLARE
    now_ bigint := {NOW};
    row_ jsonb;
    live_ boolean;
    epoch_ bigint;
    lease_ jsonb;
BEGIN
    SELECT doc INTO row_ FROM execution_attempt WHERE id = {ATTEMPT_ID};
    IF row_ IS NULL THEN
        board_answer := '{{\"outcome\":\"missing\"}}';
        EXIT claim;
    END IF;
    live_ := row_ ? 'lease' AND (row_ -> 'lease' ->> 'expires')::bigint > now_;
    IF live_ AND row_ -> 'lease' -> 'owner' IS DISTINCT FROM v -> 'owner' THEN
        board_answer := jsonb_build_object('outcome', 'held', 'lease', row_ -> 'lease', 'now', now_);
        EXIT claim;
    END IF;
    epoch_ := CASE WHEN live_ THEN (row_ -> 'lease' ->> 'epoch')::bigint
        ELSE coalesce((row_ ->> 'epoch')::bigint, 0) + 1 END;
    lease_ := jsonb_build_object('owner', v -> 'owner', 'epoch', epoch_, 'expires', now_ + (v ->> 'lease_ms')::bigint);
    row_ := row_ || jsonb_build_object('lease', lease_, 'epoch', epoch_, 'due', lease_ -> 'expires',
        'revision', (row_ ->> 'revision')::bigint + 1);
    UPDATE execution_attempt SET doc = row_ WHERE id = {ATTEMPT_ID};
    board_answer := jsonb_build_object('outcome', 'claimed', 'row', row_, 'now', now_);
END claim;"
    ))
}

pub(in crate::store) fn commit() -> Pg<'static> {
    Pg::script(format!(
        "<<commit>>
DECLARE
    now_ bigint := {NOW};
    row_ jsonb;
    fact_ jsonb;
    artifact_ jsonb;
BEGIN
    SELECT doc INTO row_ FROM execution_attempt WHERE id = {ATTEMPT_ID};
    IF row_ IS NULL THEN
        board_answer := '{{\"outcome\":\"missing\"}}';
        EXIT commit;
    END IF;
    IF v ? 'lease' AND (NOT row_ ? 'lease'
        OR row_ -> 'lease' -> 'owner' IS DISTINCT FROM v -> 'lease' -> 'owner'
        OR (row_ -> 'lease' ->> 'epoch')::bigint IS DISTINCT FROM (v -> 'lease' ->> 'epoch')::bigint
        OR (row_ -> 'lease' ->> 'expires')::bigint <= now_) THEN
        board_answer := jsonb_build_object('outcome', 'fenced', 'now', now_);
        EXIT commit;
    END IF;
    IF (row_ ->> 'facts')::bigint IS DISTINCT FROM (v ->> 'facts_from')::bigint
        OR (v ? 'state' AND (row_ ->> 'revision')::bigint IS DISTINCT FROM (v ->> 'revision')::bigint) THEN
        RAISE EXCEPTION 'stale';
    END IF;
    row_ := row_ || jsonb_build_object('facts',
        (row_ ->> 'facts')::bigint + jsonb_array_length(coalesce(v -> 'facts', '[]')));
    IF v ? 'state' THEN
        row_ := row_ || jsonb_build_object('state', v -> 'state', 'open', v -> 'open', 'live', v -> 'live',
            'revision', (row_ ->> 'revision')::bigint + 1);
        row_ := CASE WHEN v ? 'due_in' THEN row_ || jsonb_build_object('due', now_ + (v ->> 'due_in')::bigint)
            ELSE row_ - 'due' END;
        IF coalesce((v ->> 'release_lease')::boolean, false) THEN
            row_ := row_ - 'lease';
        END IF;
    END IF;
    UPDATE execution_attempt SET doc = row_ WHERE id = {ATTEMPT_ID};
    FOR fact_ IN SELECT value FROM jsonb_array_elements(coalesce(v -> 'facts', '[]')) LOOP
        INSERT INTO execution_fact (id, doc) VALUES (jsonb_build_array(v -> 'attempt', fact_ -> 'seq'),
            jsonb_build_object('attempt', v -> 'attempt', 'seq', fact_ -> 'seq', 'at', now_, 'fact', fact_ -> 'fact'));
    END LOOP;
    IF v ? 'release' THEN
        UPDATE execution_fence SET doc = jsonb_set(doc, '{{reserved}}', coalesce((SELECT jsonb_agg(held.value)
                FROM jsonb_array_elements(coalesce(doc -> 'reserved', '[]')) AS held
                WHERE held.value IS DISTINCT FROM v -> 'attempt'), '[]'))
            WHERE id = jsonb_build_array('capacity', v -> 'release');
    END IF;
    FOR artifact_ IN SELECT value FROM jsonb_array_elements(coalesce(v -> 'artifacts', '[]')) LOOP
        INSERT INTO execution_artifact (id, doc) VALUES (jsonb_build_array(v -> 'attempt', artifact_ -> 'name'),
            jsonb_build_object('attempt', v -> 'attempt', 'name', artifact_ -> 'name', 'at', now_,
                'artifact', artifact_ -> 'artifact'));
    END LOOP;
    board_answer := jsonb_build_object('outcome', 'committed', 'revision', row_ -> 'revision', 'now', now_);
END commit;"
    ))
}

pub(in crate::store) fn attempt() -> Pg<'static> {
    Pg::read(format!(
        "(SELECT doc FROM execution_attempt WHERE id = {ATTEMPT_ID})"
    ))
}

pub(in crate::store) fn of_task() -> Pg<'static> {
    Pg::read(
        "coalesce((SELECT jsonb_agg(doc ORDER BY (doc ->> 'created')::bigint, doc ->> 'attempt' COLLATE \"C\")
            FROM execution_attempt WHERE doc ->> 'task' = v ->> 'task'), '[]')",
    )
}

pub(in crate::store) fn live() -> Pg<'static> {
    Pg::read(
        "coalesce((SELECT jsonb_agg(doc -> 'attempt' ORDER BY (doc ->> 'created')::bigint, doc ->> 'attempt' COLLATE \"C\")
            FROM execution_attempt WHERE doc ->> 'task' = v ->> 'task' AND (doc ->> 'live')::boolean), '[]')",
    )
}

pub(in crate::store) fn due() -> Pg<'static> {
    Pg::read(format!(
        "coalesce((SELECT jsonb_agg(due.attempt ORDER BY due.at, due.name COLLATE \"C\") FROM (
            SELECT doc -> 'attempt' AS attempt, doc ->> 'attempt' AS name, (doc ->> 'due')::bigint AS at
            FROM execution_attempt
            WHERE (doc ->> 'open')::boolean AND (doc ->> 'due')::bigint <= {NOW}
                AND doc ->> 'configuration' = v ->> 'configuration'
            ORDER BY (doc ->> 'due')::bigint, doc ->> 'attempt' COLLATE \"C\" LIMIT (v ->> 'limit')::bigint) AS due), '[]')"
    ))
}

pub(in crate::store) fn facts() -> Pg<'static> {
    Pg::read(
        "coalesce((SELECT jsonb_agg(jsonb_build_object('seq', doc -> 'seq', 'at', doc -> 'at', 'fact', doc -> 'fact')
            ORDER BY (doc ->> 'seq')::bigint) FROM execution_fact WHERE doc ->> 'attempt' = v ->> 'attempt'), '[]')",
    )
}

pub(in crate::store) fn artifacts() -> Pg<'static> {
    Pg::read(
        "coalesce((SELECT jsonb_agg(jsonb_build_object('name', doc -> 'name', 'at', doc -> 'at', 'artifact', doc -> 'artifact')
            ORDER BY doc ->> 'name' COLLATE \"C\") FROM execution_artifact WHERE doc ->> 'attempt' = v ->> 'attempt'), '[]')",
    )
}

pub(in crate::store) fn fence() -> Pg<'static> {
    Pg::read("(SELECT doc FROM execution_fence WHERE id = v -> 'key')")
}
