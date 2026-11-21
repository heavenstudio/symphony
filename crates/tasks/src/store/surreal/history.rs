//! The statements of `store/history.rs`.

use crate::store::history::PAGE;

/// The lines since `$since` that may say a status changed: every verdict, and the system lines of a
/// move, a claim, a block by a cancelled dependency and the end of a block, of a change to what a
/// task's landing waits on, and of a move in the tree.
pub(in crate::store) const STATUS_LINES: &str = "SELECT slug, sid, ts, kind, text FROM message
    WHERE ts >= $since AND kind IN ['system', 'verdict']
    AND (kind = 'verdict' OR string::contains(text, $moved) OR string::contains(text, $claimed)
        OR string::starts_with(text, $block) OR string::starts_with(text, $unblock)
        OR string::contains(text, $waits) OR string::contains(text, $under));";

/// A line before `$since` that may say a status changed, as [`STATUS_LINES`] reads one.
const STEP_BEFORE: &str = "ts < $since AND kind IN ['system', 'verdict']
    AND (kind = 'verdict' OR string::contains(text, $moved) OR string::contains(text, $claimed)
        OR string::starts_with(text, $block) OR string::starts_with(text, $unblock))";

/// A line before `$since` that may record what a task's landing waits on, as [`STATUS_LINES`] reads
/// one.
const WAITS_BEFORE: &str = "ts < $since AND kind = 'system' AND string::contains(text, $waits)";

/// A line that may have taken a task into review: a move, or the end of a block that returns it
/// there.
const INTO_REVIEW: &str = "kind = 'system'
    AND (string::ends_with(text, $moved_in)
        OR (string::starts_with(text, $unblock) AND string::contains(text, $back_in)))";

/// The read, of each thread `$from` names, of its newest [`PAGE`] lines `filter` matches before the
/// line of its `sid`: a range of the thread's keys.
fn page_query(filter: &str) -> String {
    format!(
        "RETURN $from.map(|$line| (SELECT slug, sid, ts, kind, text
            FROM message:[$line.slug, NONE]..[$line.slug, $line.sid]
            WHERE {filter} ORDER BY sid DESC LIMIT {PAGE}));"
    )
}

/// A page ([`page_query`]) of each thread's lines before `$since` that may say a status changed
/// ([`STEP_BEFORE`]); its other variables are those of [`STATUS_LINES`].
pub(in crate::store) fn steps_before() -> String {
    page_query(STEP_BEFORE)
}

/// A page ([`page_query`]) of each thread's lines before `$since` that may record what its landing
/// waits on ([`WAITS_BEFORE`], `$waits`).
pub(in crate::store) fn waits_before() -> String {
    page_query(WAITS_BEFORE)
}

/// A page ([`page_query`]) of each thread's lines that may have taken it into review
/// ([`INTO_REVIEW`], `$moved_in`, `$unblock` and `$back_in`).
pub(in crate::store) fn into_review() -> String {
    page_query(INTO_REVIEW)
}

/// The lines of `$slug`'s thread written at `$at` that may record what its landing waits on
/// (`$waits`).
pub(in crate::store) const WAITS_AT: &str =
    "SELECT slug, sid, ts, kind, text FROM message WHERE slug = $slug AND ts = $at
            AND kind = 'system' AND string::contains(text, $waits);";
