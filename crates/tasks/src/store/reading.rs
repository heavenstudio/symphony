//! The health a native registration gives a session ([`native_health`]).

use crate::health::{self, Health};
use crate::rows::SessionRow;

/// A completed sweep either confirms a Codex pane or retires its callback. An expired registration
/// therefore means the sweep did not establish absence, even if ordinary heartbeats have expired.
/// Keep that uncertainty for at most another lost interval; a stalled sweep cannot hold work forever.
pub(super) fn native_health(
    row: &SessionRow,
    health: Health,
    now: f64,
    until: Option<f64>,
) -> Health {
    if row.left_at.is_none()
        && health != Health::Live
        && until.is_some_and(|until| {
            until < now && now - until <= health::LIVE_SECONDS + health::STALE_SECONDS
        })
    {
        Health::Unknown
    } else {
        health
    }
}
