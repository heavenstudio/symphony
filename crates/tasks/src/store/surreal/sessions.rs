//! The statements of `store/sessions.rs`.

use super::reading::HELD;
use super::work::{FENCES, gives};
use super::write::{ACT, FENCE_RECIPIENT};
use super::{logged, session_read};
use crate::stages::{EVERY_ROLE, RELEASED};

/// The slugs `$actor` holds the orchestrator role on, sorted: read through the index of a role's
/// holder alone, as every read of one session's roles is, and narrowed after.
const HELD_ORCHESTRATING: &str = "array::sort((SELECT slug, role FROM role WHERE handle = $actor)
    .filter(|$r| $r.role = 'orchestrator').map(|$r| $r.slug))";

/// A session, its parent there when it names one, and a handle nobody took meanwhile.
const REGISTER: &str = "
IF $parent != NONE AND !record::exists(type::record('session', $parent)) { THROW $orphan };
IF record::exists(type::record('session', $handle)) { THROW 'stale' };
CREATE type::record('session', $handle) CONTENT $row RETURN NONE;";

/// Every bind of one agent writes its counter first, so two at once meet there rather than each
/// binding a handle of its own; then the session is still as decided on.
const BIND_GUARD: &str = "
UPSERT type::record('counter', ['agent', $agent]) SET last += 1 RETURN NONE;
LET $row = (SELECT left_at, pid, host FROM ONLY type::record('session', $handle));
IF $row.pid != $pid OR $row.host != $host { THROW 'stale' };";

// Session rows stay compatible with older readers. The hook knows whether this key is a full
// session or an in-process agent; keep that provenance in callback metadata, never in session.
const CHANNEL_BIND: &str = "
LET $held = type::record('session', $handle).agent;
LET $held_in_process = IF $held = NONE { NONE } ELSE { type::record('callback_state', ['binding', $held]).in_process };
IF !$full AND (type::record('callback_state', $handle).address != NONE
    OR type::record('callback_state', ['binding', $agent]).in_process = false
    OR $held_in_process = false) { THROW 'stale' };
UPSERT type::record('callback_state', ['binding', $agent]) SET in_process = (in_process ?? false) OR !$full RETURN NONE;
IF !$full {
    DELETE type::record('callback_state', $handle) RETURN NONE;
    UPDATE callback_event SET delivered = true, text = '' WHERE handle = $handle AND string::starts_with(key, 'chat-') RETURN NONE;
};";

/// `agent` beats for `handle` from now on, and for no other.
const BIND: &str = "
IF $row.left_at != NONE { THROW 'stale' };
IF $full AND (record::exists(type::record('barred_key', $agent))
    OR array::len(SELECT VALUE handle FROM session WHERE agent = $agent AND left_at = NONE) > 0) { THROW 'stale' };
UPDATE session SET agent = NONE, busy_until = NONE, busy_call = NONE WHERE agent = $agent AND handle != $handle RETURN NONE;
UPDATE type::record('session', $handle) SET agent = $agent, last_seen = math::max([last_seen, $now]) RETURN NONE;
RETURN NONE;";

/// `agent` is a session with a monitor of its own: it beats for nobody, ever.
const BAR: &str = "
IF !record::exists(type::record('barred_key', $agent)) {
    CREATE type::record('barred_key', $agent) SET key = $agent RETURN NONE;
};
UPDATE session SET agent = NONE, busy_until = NONE, busy_call = NONE WHERE agent = $agent RETURN NONE;
RETURN NONE;";

/// What `bind_agent` decides on.
pub(in crate::store) const BINDING: &str = "
LET $held = type::record('session', $handle).agent;
RETURN {
    row: (SELECT * OMIT id FROM ONLY type::record('session', $handle)),
    barred: record::exists(type::record('barred_key', $agent)),
    bound: (SELECT VALUE handle FROM session WHERE agent = $agent AND left_at = NONE),
    in_process: type::record('callback_state', ['binding', $agent]).in_process,
    held_in_process: IF $held = NONE { NONE } ELSE { type::record('callback_state', ['binding', $held]).in_process },
    native: type::record('callback_state', $handle).address != NONE,
};";

/// The session bound to `$agent` is seen, and its busy mark kept, moved or cleared. `boot_order`
/// keeps the known watermark even when `last_boot` is unreadable; retired boot ids prevent a
/// delayed sample from moving it back across a reboot. No arrival order proves sample order across
/// an unreadable call, so `boot_uncertain` lasts for the checkout stay. `$s` is the session, its
/// kind included, which [`super::spending::SPEND`] reads after it.
const BEAT: &str = "
LET $s = (SELECT handle, kind, busy_until, busy_call FROM session WHERE agent = $agent AND left_at = NONE LIMIT 1)[0];
LET $mark = IF $s = NONE { NONE }
    ELSE IF $busy_for != NONE AND ($s.busy_until = NONE OR $now + $busy_for >= $s.busy_until) {
        { until: $now + $busy_for, call: $call } }
    ELSE IF $ended AND $s.busy_call = $call { { until: NONE, call: NONE } }
    ELSE { { until: $s.busy_until, call: $s.busy_call } };
IF $s != NONE {
    UPDATE type::record('session', $s.handle)
        SET last_seen = math::max([last_seen, $now]), last_tool = math::max([last_tool ?? $now, $now]), busy_until = $mark.until, busy_call = $mark.call RETURN NONE;
};
LET $bound = type::record('callback_state', ['binding', $agent]);
IF $s != NONE AND $bound.in_process = true {
    IF $place != NONE AND [$bound.worktree, $bound.branch] != [$place.worktree, $place.branch] {
        UPDATE $bound SET worktree = $place.worktree, branch = $place.branch, placed_at = $now,
            stay_from = $now, placed_boot = $boot, last_boot = $boot, boot_order = $boot,
            boot_previous = [], boot_uncertain = $boot = NONE RETURN NONE;
    } ELSE {
        IF $place != NONE AND $now < ($bound.stay_from ?? $bound.placed_at) {
            UPDATE $bound SET stay_from = $now RETURN NONE;
        };
        LET $last = $bound.boot_order ?? $bound.last_boot ?? $bound.placed_boot;
        IF $boot = NONE AND ($bound.last_boot != NONE OR $bound.boot_uncertain != true) {
            UPDATE $bound SET last_boot = NONE, boot_order = $last, boot_uncertain = true RETURN NONE;
        } ELSE IF $boot != NONE AND !array::includes($bound.boot_previous ?? [], $boot.boot)
            AND ($last = NONE OR $boot.boot != $last.boot OR $boot.at > $last.at) {
            UPDATE $bound SET last_boot = $boot, boot_order = $boot,
                boot_previous = IF $last != NONE AND $boot.boot != $last.boot {
                    array::append(boot_previous ?? [], $last.boot)
                } ELSE { boot_previous ?? [] } RETURN NONE;
        };
    };
};
";

/// [`BEAT`], adding the tokens it hands after it when it `spends` ([`super::spending::SPEND`],
/// whose variables it then needs too); its answer is the handle bound to `$agent`, if any.
pub(in crate::store) fn beat(spends: bool) -> String {
    let spend = if spends { super::spending::SPEND } else { "" };
    format!("{BEAT}{spend}RETURN $s.handle;")
}

/// The handle of every session there ever was: a new one takes none of them.
pub(in crate::store) const HANDLES: &str = "RETURN SELECT VALUE handle FROM session;";

/// [`REGISTER`], then the session as a read of one gives it (`super::session_read`). Needs
/// `$handle`, `$row`, `$parent`, `$orphan` and `$executing`.
pub(in crate::store) fn register() -> String {
    format!("{REGISTER}\n{}", session_read())
}

/// `$actor` is seen, and nothing else: an act ([`ACT`]) that writes no more. Needs an act's
/// variables.
pub(in crate::store) fn touch() -> String {
    format!("{ACT}\nRETURN NONE;")
}

/// The key `$agent`, bound to `$handle` already, has its class written ([`CHANNEL_BIND`]), the
/// session still bound to it and fenced as a recipient is. Needs `$agent`, `$handle`, `$full`,
/// `$pid` and `$host`.
pub(in crate::store) fn rebind() -> String {
    format!(
        "{BIND_GUARD}{fence}
IF type::record('session', $handle).agent != $agent {{ THROW 'stale' }};
{CHANNEL_BIND} RETURN NONE;",
        fence = FENCE_RECIPIENT,
    )
}

/// [`BIND`] behind its guard, the key's class written first ([`CHANNEL_BIND`]). Needs `$agent`,
/// `$handle`, `$full`, `$now`, `$pid` and `$host`.
pub(in crate::store) fn bind() -> String {
    format!("{BIND_GUARD}{CHANNEL_BIND}{BIND}")
}

/// [`BAR`] behind the same guard as a bind. Needs `$agent`, `$handle`, `$pid` and `$host`.
pub(in crate::store) fn bar() -> String {
    format!("{BIND_GUARD}{BAR}")
}

/// `$actor` takes its handle back: an act ([`ACT`]) that writes its monitor's `$pid` and `$host`.
pub(in crate::store) fn reattach() -> String {
    format!(
        "{ACT}\nUPDATE type::record('session', $actor) SET pid = $pid, host = $host RETURN NONE;\nRETURN NONE;"
    )
}

/// The session `$handle` that left is back, seen at `$now`; `$unknown` is thrown when there is
/// none.
pub(in crate::store) const REVIVE: &str = "IF !record::exists(type::record('session', $handle)) { THROW $unknown };
            UPDATE type::record('session', $handle) SET left_at = NONE, last_seen = math::max([last_seen, $now]) RETURN NONE;
            RETURN NONE;";

/// `$actor` leaves at `$now`: every role it holds released on the record, a task left in progress
/// with no executor ready again, the pass's gives (`FENCES`, `gives`) and its callback
/// registrations gone, fenced on the orchestrator roles read (`$held`, [`held_orchestrating`]).
/// On a task a custom workflow holds only its orchestrator role goes here, and its status stays:
/// its other roles went by the position's recovery before (`Store::leave`), which kept any it
/// says a person recovers. Needs an act's and a pass's variables.
pub(in crate::store) fn leave() -> String {
    format!(
        "{ACT}{FENCES}
IF {HELD_ORCHESTRATING} != $held {{ THROW 'stale' }};
FOR $slug IN array::sort(array::distinct(SELECT VALUE slug FROM role WHERE handle = $actor
        AND (role = 'orchestrator' OR type::record('task', slug).fields._board.workflow = NONE))) {{
    LET $held = (SELECT VALUE fields._board.workflow FROM ONLY type::record('task', $slug)) != NONE;
    DELETE role WHERE slug = $slug AND handle = $actor AND (!$held OR role = 'orchestrator');
    LET $ready = !$held AND (SELECT VALUE status FROM ONLY type::record('task', $slug)) = 'in_progress'
        AND array::len(SELECT VALUE handle FROM role WHERE slug = $slug AND role = 'executor') = 0;
    UPDATE type::record('task', $slug) SET version += 1, status = IF $ready {{ 'ready' }} ELSE {{ status }},
        updated = IF $ready {{ $now }} ELSE {{ updated }} RETURN NONE;
    {}
}};
{}
UPDATE type::record('session', $actor) SET left_at = $now RETURN NONE;
DELETE type::record('callback_state', $actor) RETURN NONE;
DELETE type::record('callback_state', ['session_url', $actor]) RETURN NONE;
RETURN NONE;",
        logged(
            "$slug",
            "$actor",
            "'system'",
            &format!("$actor + '{RELEASED}{EVERY_ROLE} (left)'"),
            "NONE"
        ),
        gives()
    )
}

/// The slugs `$actor` holds the orchestrator role on, sorted ([`HELD_ORCHESTRATING`]), as a read
/// of its own.
pub(in crate::store) fn held_orchestrating() -> String {
    format!("RETURN {HELD_ORCHESTRATING};")
}

/// Every session, by when it connected, then handle, and the handles holding work another could
/// take over ([`HELD`]). Needs `$executing`.
pub(in crate::store) fn sessions() -> String {
    format!(
        "{HELD}\nRETURN {{ sessions: (SELECT * OMIT id FROM session ORDER BY connected, handle), held: $held }};"
    )
}

/// The handle of the session, not left, that `$agent` beats for, if any.
pub(in crate::store) const HANDLE_FOR_AGENT: &str =
    "RETURN (SELECT VALUE handle FROM session WHERE agent = $agent AND left_at = NONE LIMIT 1)[0];";

/// Each session that has not left and its bound key.
pub(in crate::store) const BOUND_AGENTS: &str =
    "RETURN SELECT handle, agent FROM session WHERE agent != NONE AND left_at = NONE;";

/// Each session that has not left, its bound key and what that key's binding keeps.
pub(in crate::store) const AGENT_BINDINGS: &str = "RETURN SELECT handle, agent,
                type::record('callback_state', ['binding', agent]).in_process ?? false AS in_process,
                type::record('callback_state', ['binding', agent]).worktree ?? '' AS worktree,
                type::record('callback_state', ['binding', agent]).branch ?? '' AS branch,
                type::record('callback_state', ['binding', agent]).placed_at ?? 0 AS placed_at,
                type::record('callback_state', ['binding', agent]).stay_from
                    ?? type::record('callback_state', ['binding', agent]).placed_at ?? 0 AS stay_from,
                type::record('callback_state', ['binding', agent]).placed_boot AS placed_boot,
                type::record('callback_state', ['binding', agent]).last_boot AS last_boot,
                type::record('callback_state', ['binding', agent]).boot_uncertain ?? false AS boot_uncertain
            FROM session WHERE agent != NONE AND left_at = NONE;";
