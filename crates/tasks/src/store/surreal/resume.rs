//! The statements of `store/resume.rs`.

use super::{cap, logged, work};

pub(in crate::store) const CANDIDATES: &str = "SELECT VALUE slug FROM task WHERE status = 'blocked' AND string::starts_with(fields._board.blocked.on ?? '', 'task:');";

pub(in crate::store) const PREREQUISITES: &str =
    "SELECT slug, status, version FROM $required.map(|$s| type::record('task', $s));";

pub(in crate::store) fn resume() -> String {
    format!(
        "{}\nFOR $g IN $watch {{
            IF (SELECT VALUE version FROM ONLY type::record('task', $g.slug)) != $g.version {{ THROW 'stale' }};
            UPDATE type::record('task', $g.slug) SET version += 1 RETURN NONE;
        }};
        UPDATE type::record('task', $slug) SET status = $status, updated = $now, version += 1 RETURN NONE;
        {}{}{}\n{}\nRETURN NONE;",
        cap::FENCE, work::UNBLOCKED, cap::UNHOLD, cap::GUARD,
        logged("$slug", "'board'", "'system'", "$said", "NONE"),
    )
}
