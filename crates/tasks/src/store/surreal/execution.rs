//! The statements of `store/execution.rs`. Every time they compare or write is the database's
//! own (`time::millis(time::now())`), never a caller's clock: two workers on skewed machines read
//! one lease alike.

/// An attempt admitted: `$row`'s identity and state written as attempt `$attempt` of `$task` under
/// `$configuration`, with its first facts `$facts` (each `{seq, fact}`), the request fence
/// `$request` (with `$digest`), the lane fence `['lane', $task, $lane]` and, under `$capacity`
/// (`{scope, limit}`), a reservation, all in one transaction. Its `outcome` is `existing` (the
/// `attempt` and `digest` a request key already admitted), `busy` (the lane's `attempt`, while it
/// is live, or while it is not `$predecessor` when one is named), `full` (the `reserved` attempts
/// of a full capacity scope) or `admitted`.
pub(in crate::store) const ADMIT: &str = "LET $now = time::millis(time::now());
                     IF $request != NONE {
                         LET $known = (SELECT attempt, digest FROM ONLY type::record('execution_fence', ['request', $request]));
                         IF $known != NONE { RETURN { outcome: 'existing', attempt: $known.attempt, digest: $known.digest }; };
                     };
                     LET $current = (SELECT VALUE attempt FROM ONLY type::record('execution_fence', ['lane', $task, $lane]));
                     IF $predecessor != NONE AND $current != $predecessor { RETURN { outcome: 'busy', attempt: $current }; };
                     IF $predecessor = NONE AND $current != NONE {
                         IF (SELECT VALUE live FROM ONLY type::record('execution_attempt', $current)) = true {
                             RETURN { outcome: 'busy', attempt: $current };
                         };
                     };
                     IF $capacity != NONE {
                         LET $held = (SELECT VALUE reserved FROM ONLY type::record('execution_fence', ['capacity', $capacity.scope])) ?? [];
                         IF array::len($held) >= $capacity.limit { RETURN { outcome: 'full', reserved: $held }; };
                         UPSERT type::record('execution_fence', ['capacity', $capacity.scope])
                             SET key = ['capacity', $capacity.scope], reserved = array::append(reserved ?? [], $attempt),
                                 limit = $capacity.limit RETURN NONE;
                     };
                     CREATE type::record('execution_attempt', $attempt) CONTENT {
                         attempt: $attempt, task: $task, configuration: $configuration, created: $now,
                         identity: $row.identity, state: $row.state,
                         revision: 1, open: true, live: true, due: $now, epoch: 0, facts: array::len($facts)
                     } RETURN NONE;
                     FOR $fact IN $facts {
                         CREATE type::record('execution_fact', [$attempt, $fact.seq])
                             CONTENT { attempt: $attempt, seq: $fact.seq, at: $now, fact: $fact.fact } RETURN NONE;
                     };
                     IF $request != NONE {
                         CREATE type::record('execution_fence', ['request', $request]) CONTENT { key: ['request', $request], attempt: $attempt, digest: $digest } RETURN NONE;
                     };
                     UPSERT type::record('execution_fence', ['lane', $task, $lane]) SET key = ['lane', $task, $lane], attempt = $attempt RETURN NONE;
                     RETURN { outcome: 'admitted', attempt: $attempt };";

/// Attempt `$attempt`'s lease taken for `$owner` for `$lease_ms`: renewed at the same epoch when
/// `$owner` holds it unexpired, else taken at the next epoch once no one holds it unexpired. The
/// attempt is due again when the lease runs out, so a worker that dies leaves it to the next.
/// Its `outcome` is `missing`, `held` (the `lease`, while another holds it) or `claimed` (the `row`),
/// each with the database's `now`.
pub(in crate::store) const CLAIM: &str = "LET $now = time::millis(time::now());
                     LET $row = (SELECT lease, epoch FROM ONLY type::record('execution_attempt', $attempt));
                     IF $row = NONE { RETURN { outcome: 'missing' }; };
                     LET $live = $row.lease != NONE AND $row.lease.expires > $now;
                     IF $live AND $row.lease.owner != $owner { RETURN { outcome: 'held', lease: $row.lease, now: $now }; };
                     LET $epoch = IF $live { $row.lease.epoch } ELSE { ($row.epoch ?? 0) + 1 };
                     LET $lease = { owner: $owner, epoch: $epoch, expires: $now + $lease_ms };
                     UPDATE type::record('execution_attempt', $attempt)
                         SET lease = $lease, epoch = $epoch, due = $lease.expires, revision += 1 RETURN NONE;
                     RETURN { outcome: 'claimed', row: (SELECT * OMIT id FROM ONLY type::record('execution_attempt', $attempt)), now: $now };";

/// Attempt `$attempt` written as its caller decided on what it read: `fenced` unless
/// `$lease` (`{owner, epoch}`), when named, still holds it unexpired by the database's clock;
/// `stale`, tried again, unless its facts are still `$facts_from` and, with a `$state`, its
/// revision `$revision`. Then the facts `$facts` appended, and with a `$state` the state, `open`,
/// `live` and `due` (`$due_in` from now, none without it) written, the lease let go when
/// `$release_lease`, the reservation under the capacity scope `$release` given back and the
/// artifacts `$artifacts` made: `committed`, with the `revision` and `now`.
pub(in crate::store) const COMMIT: &str = "LET $now = time::millis(time::now());
                     LET $row = (SELECT revision, lease, facts FROM ONLY type::record('execution_attempt', $attempt));
                     IF $row = NONE { RETURN { outcome: 'missing' }; };
                     IF $lease != NONE AND ($row.lease = NONE OR $row.lease.owner != $lease.owner
                         OR $row.lease.epoch != $lease.epoch OR $row.lease.expires <= $now) {
                         RETURN { outcome: 'fenced', now: $now };
                     };
                     IF $row.facts != $facts_from OR ($state != NONE AND $row.revision != $revision) { THROW 'stale'; };
                     LET $due = IF $due_in = NONE { NONE } ELSE { $now + $due_in };
                     IF $state != NONE {
                         IF $release_lease {
                             UPDATE type::record('execution_attempt', $attempt) SET state = $state, open = $open, live = $live,
                                 due = $due, lease = NONE, revision += 1, facts += array::len($facts) RETURN NONE;
                         } ELSE {
                             UPDATE type::record('execution_attempt', $attempt) SET state = $state, open = $open, live = $live,
                                 due = $due, revision += 1, facts += array::len($facts) RETURN NONE;
                         };
                     } ELSE {
                         UPDATE type::record('execution_attempt', $attempt) SET facts += array::len($facts) RETURN NONE;
                     };
                     FOR $fact IN $facts {
                         CREATE type::record('execution_fact', [$attempt, $fact.seq])
                             CONTENT { attempt: $attempt, seq: $fact.seq, at: $now, fact: $fact.fact } RETURN NONE;
                     };
                     IF $release != NONE {
                         UPDATE type::record('execution_fence', ['capacity', $release])
                             SET reserved = array::complement(reserved ?? [], [$attempt]) RETURN NONE;
                     };
                     FOR $artifact IN $artifacts {
                         CREATE type::record('execution_artifact', [$attempt, $artifact.name])
                             CONTENT { attempt: $attempt, name: $artifact.name, at: $now, artifact: $artifact.artifact } RETURN NONE;
                     };
                     RETURN { outcome: 'committed', revision: (SELECT VALUE revision FROM ONLY type::record('execution_attempt', $attempt)), now: $now };";

/// Attempt `$attempt`'s row, or null.
pub(in crate::store) const ATTEMPT: &str =
    "RETURN (SELECT * OMIT id FROM ONLY type::record('execution_attempt', $attempt)) ?? NULL;";

/// Every attempt of `$task`, oldest first.
pub(in crate::store) const OF_TASK: &str =
    "RETURN SELECT * OMIT id FROM execution_attempt WHERE task = $task ORDER BY created, attempt;";

/// The attempts of `$task` whose runtime may still be alive, oldest first.
pub(in crate::store) const LIVE: &str = "RETURN SELECT VALUE attempt FROM execution_attempt WHERE task = $task AND live = true ORDER BY created, attempt;";

/// Up to `$limit` open attempts under `$configuration` whose work is due by the database's
/// clock, the longest due first.
pub(in crate::store) const DUE: &str = "LET $now = time::millis(time::now());
                     RETURN SELECT VALUE attempt FROM execution_attempt
                         WHERE open = true AND due != NONE AND due <= $now AND configuration = $configuration
                         ORDER BY due, attempt LIMIT $limit;";

/// Attempt `$attempt`'s facts, in order.
pub(in crate::store) const FACTS: &str =
    "RETURN SELECT seq, at, fact FROM execution_fact WHERE attempt = $attempt ORDER BY seq;";

/// Attempt `$attempt`'s artifacts, by name.
pub(in crate::store) const ARTIFACTS: &str = "RETURN SELECT name, at, artifact FROM execution_artifact WHERE attempt = $attempt ORDER BY name;";

/// The fence `$key`, or null.
pub(in crate::store) const FENCE: &str =
    "RETURN (SELECT * OMIT id FROM ONLY type::record('execution_fence', $key)) ?? NULL;";
