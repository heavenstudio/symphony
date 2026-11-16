//! The statements of `store/dispatcher.rs`.

/// `$host`'s dispatch lease taken for `$lease_seconds` from `$now` (`taken`, its revision, and `began`,
/// the asks in then), or, while another holds it, one more pass asked of that one (`ask`, its number).
pub(in crate::store) const TAKE: &str = "LET $lease = (SELECT until, revision, asked FROM ONLY type::record('callback_lease', ['dispatch', $host]));
                     IF ($lease.until ?? 0) > $now {
                         LET $ask = ($lease.asked ?? 0) + 1;
                         UPDATE type::record('callback_lease', ['dispatch', $host]) SET asked = $ask RETURN NONE;
                         RETURN { ask: $ask };
                     };
                     LET $revision = ($lease.revision ?? 0) + 1;
                     UPSERT type::record('callback_lease', ['dispatch', $host]) SET until = $now + $lease_seconds, revision = $revision,
                         asked = asked ?? 0, served = asked ?? 0, completed = completed ?? 0 RETURN NONE;
                     RETURN { taken: $revision, began: $lease.asked ?? 0 };";

/// `$host`'s dispatch lease: the last ask a pass that ended began after, and when it lapses.
pub(in crate::store) const LEASE: &str = "RETURN (SELECT completed ?? 0 AS completed, until ?? 0 AS until
                         FROM ONLY type::record('callback_lease', ['dispatch', $host])) ?? { completed: 0, until: 0 };";

/// `$host`'s dispatch lease let go, while it is still revision `$revision`.
pub(in crate::store) const LET_GO: &str = "UPDATE type::record('callback_lease', ['dispatch', $host]) SET until = 0 WHERE revision = $revision RETURN NONE; RETURN NONE;";

/// A pass begun after ask `$began` recorded as ended on `$host`'s dispatch lease, and what its holder
/// (revision `$revision`) does next: `lapsed` once the lease is another's, `again` (the lease renewed
/// for `$lease_seconds` from `$now`, with the asks in then) while an ask stands or `$more`, else
/// `done`, the lease let go.
pub(in crate::store) const PASSED: &str = "LET $lease = (SELECT revision, asked, served FROM ONLY type::record('callback_lease', ['dispatch', $host]));
                     UPDATE type::record('callback_lease', ['dispatch', $host]) SET completed = math::max([completed ?? 0, $began]) RETURN NONE;
                     IF $lease.revision != $revision { RETURN { next: 'lapsed' }; };
                     IF ($lease.asked ?? 0) > ($lease.served ?? 0) OR $more {
                         UPDATE type::record('callback_lease', ['dispatch', $host]) SET served = asked ?? 0, until = $now + $lease_seconds RETURN NONE;
                         RETURN { next: 'again', began: $lease.asked ?? 0 };
                     };
                     UPDATE type::record('callback_lease', ['dispatch', $host]) SET until = 0 RETURN NONE;
                     RETURN { next: 'done' };";
