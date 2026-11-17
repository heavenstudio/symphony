//! The statements of `store/unanswered.rs`, with those of its `answers` (`unanswered_answers.rs`).

// Deliberate state changes answer work too. Automatic departure, launch and delivery records do
// not. Role acquisition also puts older role-addressed posts before the new holder's work.
const ANSWER_SQL: &str = "(kind IN ['msg', 'verdict'] OR (kind = 'system' AND
    (string::starts_with(text, sender + ' moved it ')
    OR string::starts_with(text, sender + ' claimed it')
    OR (string::starts_with(text, sender + ' released ') AND string::contains(text, ' (released)'))
    OR text = sender + ' joined as reviewer'
    OR text = sender + ' joined as executor')))";

// Each read below puts `then` after every SELECT it makes inside its closures: nothing for the
// lookup, ` EXPLAIN` for the test of the plans those closures get (the legacy planner's, which
// leaves an index or a range for a table scan where a top-level statement would not).

/// Each handle's threads that address it or hold a system record it made, by the recipient and
/// sender indexes: one equality on each index's first columns.
pub(in crate::store) fn naming(then: &str) -> String {
    format!(
        "RETURN $handles.map(|$handle| array::union(
(SELECT VALUE slug FROM message WITH INDEX message_recipient WHERE recipient = $handle{then}),
(SELECT VALUE slug FROM message WITH INDEX message_sender WHERE sender = $handle AND kind = 'system'{then})));"
    )
}

/// These handles' reviewer launches, as the board lists launches. Launches have no handle index:
/// this reads the launch table, for the rare reviewer post no launch on its task explains.
pub(in crate::store) const REVIEWER_LAUNCHES: &str = "RETURN SELECT * OMIT id FROM task_session
WHERE handle IN $handles AND role = 'reviewer' ORDER BY started, slug, handle;";

/// The asked threads and launches (`Threads`), by record range: a thread's messages are
/// `message:[slug, sid]` and its launches `task_session:[slug, handle]`.
pub(in crate::store) fn threads(then: &str) -> String {
    format!(
        "RETURN {{
threads: $slugs.map(|$slug| (SELECT sid, slug, sender, recipient, kind, ts,
IF kind = 'system' {{ text }} ELSE {{ '' }} AS text
FROM message:[$slug, NONE]..[$slug, ..] WHERE {ANSWER_SQL}
OR (kind = 'system' AND string::starts_with(text, sender + ' released ')){then})),
launches: array::flatten($launched.map(|$slug| (SELECT * OMIT id FROM task_session:[$slug, NONE]..[$slug, ..]{then})))
}};"
    )
}

/// For each ask (a recipient, the sequence it asks after and the threads below a task), the
/// sequence ids of the recipient's answers there after it, by record range from that sequence on.
pub(in crate::store) fn below(then: &str) -> String {
    format!(
        "RETURN $asks.map(|$ask| {{ LET $handle = $ask.handle; LET $after = $ask.after;
RETURN array::flatten($ask.below.map(|$slug| (SELECT VALUE sid FROM message:[$slug, $after]..[$slug, ..]
WHERE sender = $handle AND sid > $after AND {ANSWER_SQL}{then}))); }});"
    )
}

/// Equality on the first two index columns leaves only this sender/kind's sequence suffix.
/// Keep the original action predicate, including deliberate releases and role acquisitions.
pub(in crate::store) fn answers() -> String {
    format!(
        "array::sort(array::flatten(['msg', 'verdict', 'system'].map(|$kind| \
({}).filter(|$row| $row.eligible).map(|$row| [$row.sid, $row.slug]))))",
        answers_select()
    )
}

/// `$handle`'s records of `$kind` after `$post`, each with whether it answers (`eligible`).
pub(in crate::store) fn answers_select() -> String {
    // Closures run through the legacy planner. Putting ANSWER_SQL's string predicates in WHERE
    // makes it abandon the index for a full table scan, even with WITH INDEX. Compute eligibility
    // on the indexed suffix instead; read() filters it before returning only row identities.
    format!(
        "SELECT sid, slug, ({ANSWER_SQL}) AS eligible FROM message WITH INDEX message_sender \
WHERE sender = $handle AND kind = $kind AND sid > $post"
    )
}

/// The identities of `$handle`'s answers after `$post` ([`answers`]), as a read of its own.
pub(in crate::store) fn answer_rows() -> String {
    format!("RETURN {};", answers())
}

/// Each of `$slugs` with its task's parent.
pub(in crate::store) const PARENTS: &str =
    "RETURN $slugs.map(|$slug| [$slug, type::record('task', $slug).parent ?? NULL]);";

/// The task and only its executor holders' health inputs, by task/role index and record id.
pub(in crate::store) fn successor_roles() -> &'static str {
    "SELECT handle FROM role WITH INDEX role_slug WHERE slug = $slug AND role = 'executor' ORDER BY handle"
}

fn successor_inputs() -> String {
    format!(
        "{{ version: type::record('task', $slug).version,
holders: ({}).map(|$r| [$r.handle,
    (SELECT * OMIT id FROM ONLY type::record('session', $r.handle)) ?? NULL,
    type::record('callback_state', $r.handle).address ?? NULL,
    type::record('callback_state', $r.handle).confirmed_until ?? 0]) }}",
        successor_roles()
    )
}

pub(in crate::store) fn successors() -> String {
    format!("RETURN {};", successor_inputs())
}

/// The one automatic resume of `$handle` for `$post` on `$slug`, reserved, answering whether it
/// was: not when it is reserved already, the session was seen since `$seen`, its answers
/// ([`answers`]) are no longer `$answers`, a parent of `$parents` changed, or `$role` no longer
/// answers for the task in its status. Executor eligibility also fences its observed task,
/// sessions and native registrations, without consuming an attempt when they changed.
pub(in crate::store) fn reserve() -> String {
    format!("LET $key = type::record('unanswered_restore', [$slug, $handle, $post]);
IF record::exists($key) {{ RETURN false; }};
FOR $witness_handle IN array::sort(array::distinct(array::concat([$handle], ($successors.holders ?? []).map(|$h| $h[0])))) {{
    {witness_fence}
}};
IF type::record('session', $handle).last_seen != $seen {{ RETURN false; }};
IF {} != $answers {{ RETURN false; }};
IF $parents.map(|$p| [$p[0], type::record('task', $p[0]).parent ?? NULL]) != $parents {{ RETURN false; }};
LET $status = type::record('task', $slug).status;
IF ($role = 'executor' AND $status NOT IN ['in_progress', 'blocked', 'review'])
    OR ($role = 'reviewer' AND $status != 'review') {{ RETURN false; }};
IF $successors != NONE {{
    IF {successor_inputs} != $successors {{ RETURN false; }};
    FOR $successor_handle IN $successors.holders.map(|$h| $h[0]) {{
        LET $callback = type::record('callback_state', $successor_handle);
        IF record::exists($callback) {{
            UPDATE $callback SET recovery_revision = (recovery_revision ?? 0) + 1 RETURN NONE;
        }};
    }};
    UPDATE type::record('task', $slug) SET version += 1 RETURN NONE;
}};
CREATE $key CONTENT {{ slug: $slug, handle: $handle, post: $post }} RETURN NONE;
RETURN true;", answers(),
        witness_fence = super::write::FENCE_RECIPIENT.replace("$handle", "$witness_handle"),
        successor_inputs = successor_inputs(),
        )
}

/// The board's line `$text` to `@orchestrator` on `$slug`'s thread at `$now`.
pub(in crate::store) fn restore_said() -> String {
    format!(
        "{}\nRETURN NONE;",
        super::logged("$slug", "'board'", "'msg'", "$text", "'@orchestrator'")
    )
}
