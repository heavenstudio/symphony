//! The statements of `store/session_urls.rs`.

use super::write::ACT;

/// Every address of a session that has not left, by handle: one read of the `session_url` rows,
/// by their ids' range.
pub(in crate::store) const READ: &str = "RETURN SELECT VALUE [record::id(id)[1], url]
    FROM callback_state:['session_url', NONE]..['session_url', ..]
    WHERE url != NONE AND type::record('session', record::id(id)[1]).left_at = NONE;";

/// `$actor`'s web address set to `$url`, or cleared when it is none. Needs `ACT`'s variables.
pub(in crate::store) fn set() -> String {
    format!(
        "{ACT}
LET $key = type::record('callback_state', ['session_url', $actor]);
IF $url = NONE {{ DELETE $key RETURN NONE; }}
ELSE {{ UPSERT $key SET url = $url RETURN NONE; }};
RETURN NONE;"
    )
}
