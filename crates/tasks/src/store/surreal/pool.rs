//! The statements of `store/pool.rs`.

use super::messages::OWNER_MOVES;

/// The newest message's number, and the delivered messages (`$delivered`) and the moves of the
/// owner's and a person's ([`OWNER_MOVES`]) after `$floor`: a pass's pool, only the fields the
/// inbox's reach test reads.
pub(in crate::store) fn pool() -> String {
    format!(
        "RETURN [(SELECT VALUE last FROM ONLY counter:message) ?? 0,
                    (SELECT sid, slug, sender, recipient, kind, ts, person FROM message WHERE sid > $floor AND (kind IN $delivered OR {}))];",
        *OWNER_MOVES
    )
}
