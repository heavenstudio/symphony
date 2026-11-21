//! Local session guards share a revision write with beats and departures.
//! A read alone does not fence SurrealDB's optimistic write/write transactions.

macro_rules! fence {
    ($handle:literal) => {
        concat!(
            "
LET $session_fence = type::record('session_revision', ",
            $handle,
            ");
IF record::exists($session_fence) {
    UPDATE $session_fence SET writes = (writes ?? 0) + 1 RETURN NONE;
} ELSE {
    CREATE $session_fence SET last = 0, writes = 1 RETURN NONE;
};
"
        )
    };
}

/// A checked recipient or binding target, without advancing its heartbeat.
pub(in crate::store) const FENCE_RECIPIENT: &str = fence!("$handle");

/// Authorize a machine-authored notice without fabricating a tool call.
pub(in crate::store) const AUTHORIZE: &str = concat!(
    "LET $me = (SELECT left_at FROM ONLY type::record('session', $actor));
IF $me = NONE { THROW $unknown_actor };
IF $me.left_at != NONE { THROW $left_actor };",
    fence!("$actor")
);

/// Authorize, fence departure, and record the actor's activity.
pub(in crate::store) const ACT: &str = concat!(
    "LET $me = (SELECT left_at FROM ONLY type::record('session', $actor));
IF $me = NONE { THROW $unknown_actor };
IF $me.left_at != NONE { THROW $left_actor };",
    fence!("$actor"),
    "UPDATE type::record('session', $actor) SET last_seen = math::max([last_seen, $now]) RETURN NONE;"
);

/// Fence the actor, gone witnesses and incoming role holders before changing their task.
pub(in crate::store) const WRITE: &str = concat!(
    "
LET $me = (SELECT left_at FROM ONLY type::record('session', $actor));
IF $me = NONE OR $me.left_at != NONE { THROW 'stale' };
IF (SELECT VALUE version FROM ONLY type::record('task', $slug)) != $version { THROW 'stale' };
FOR $s IN $seen {
    IF (SELECT VALUE last_seen FROM ONLY type::record('session', $s.handle)) != $s.last_seen { THROW 'stale' };
};
FOR $witness_handle IN array::sort(array::distinct(array::concat([$actor], array::concat($seen.handle, $add.handle)))) {
",
    fence!("$witness_handle"),
    "
};
UPDATE type::record('session', $actor) SET last_seen = math::max([last_seen, $now]) RETURN NONE;
FOR $r IN $drop { DELETE type::record('role', [$slug, $r.handle, $r.role]); };
FOR $r IN $add { IF type::record('session', $r.handle).left_at != NONE { THROW 'stale' }; UPSERT type::record('role', [$slug, $r.handle, $r.role]) SET slug = $r.slug, handle = $r.handle, role = $r.role, since = $r.since, expires = $r.expires RETURN NONE; };
UPDATE type::record('task', $slug) SET version += 1, status = $status ?? status, updated = $updated ?? updated RETURN NONE;
IF $say {
    LET $counted = (UPSERT ONLY counter:message SET last += 1, ts = IF ts != NONE AND ts > $now AND ts <= $now + 60 { ts } ELSE { $now });
    LET $sid = $counted.last;
    LET $ts = $counted.ts;
    CREATE type::record('message', [$slug, $sid]) SET sid = $sid, slug = $slug, sender = $actor, recipient = $say.recipient,
        kind = $say.kind, text = $say.text, ts = $ts RETURN NONE;
};"
);
