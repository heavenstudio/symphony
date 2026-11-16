//! The statements of `store/pool.rs`.

use super::messages::OWNER_MOVES;

/// The newest message's number, and the delivered messages (`$delivered`) and the owner's moves
/// ([`OWNER_MOVES`]) after `$floor`: a pass's pool, only the fields the inbox's reach test reads.
pub(in crate::store) fn pool() -> String {
    format!(
        "RETURN [(SELECT VALUE last FROM ONLY counter:message) ?? 0,
                    (SELECT sid, slug, sender, recipient, kind, ts FROM message WHERE sid > $floor AND (kind IN $delivered OR {}))];",
        *OWNER_MOVES
    )
}
