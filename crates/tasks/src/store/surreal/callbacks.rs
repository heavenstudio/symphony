//! The statements of `store/callbacks.rs`.

use super::launches::HOLDS;
use super::logged;
use super::write::{ACT, FENCE_RECIPIENT};
use std::sync::LazyLock;

/// `$actor`'s line `$text` queued in `$handle`'s outbox, and its key: refused for a recipient unknown
/// or left, or with no native channel (`$no_channel`), and an empty key for a line to itself. Needs
/// `ACT`'s variables.
pub(in crate::store) fn send_agent() -> String {
    format!("{ACT}
LET $recipient = (SELECT left_at FROM ONLY type::record('session', $handle));
IF $recipient = NONE {{ THROW string::concat('refused: no session ', $handle); }};
IF $recipient.left_at != NONE {{ THROW string::concat('refused: session ', $handle, ' has left'); }};
{FENCE_RECIPIENT}
IF $actor = $handle {{ RETURN ''; }};
LET $channel = (SELECT address, owner FROM ONLY type::record('callback_state', $handle));
LET $address = $channel.address ?? '';
LET $agent = type::record('session', $handle).agent;
LET $in_process = IF $agent = NONE {{ false }} ELSE {{ type::record('callback_state', ['binding', $agent]).in_process ?? true }};
IF $address = '' OR $in_process OR (string::starts_with($address, 'uds:') AND $channel.owner = NONE) {{ THROW $no_channel; }};
LET $counted = (UPSERT ONLY type::record('counter', ['callback-event', $handle]) SET last = (last ?? 0) + 1);
LET $key = string::concat('agent-', <string>$counted.last);
CREATE type::record('callback_event', [$handle, $key]) SET key = $key, handle = $handle,
    text = $text, sequence = $counted.last, attempts = 0, delivered = false RETURN NONE;
RETURN $key;")
}

/// Whether `$handle`'s outbox line `$key` was delivered, or is gone, which only a delivered one is.
pub(in crate::store) const AGENT_DELIVERED: &str = "RETURN !record::exists(type::record('callback_event', [$handle, $key])) OR type::record('callback_event', [$handle, $key]).delivered = true;";

/// `$actor`'s channel registered at `$address` on `$host` (none unregisters), by the listener `$owner`
/// from `$terminal`, taking the chat seat when `$chat` (only a free one when `$if_free`): refused for
/// an in-process binding and an address another session holds. Needs `ACT`'s variables.
pub(in crate::store) fn register() -> String {
    format!("{ACT}
UPSERT callback_state:chat SET revision = (revision ?? 0) + 1, waiting = array::complement(waiting ?? [], [$actor]) RETURN NONE;
LET $agent = type::record('session', $actor).agent;
LET $binding = IF $agent = NONE {{ NONE }} ELSE {{ type::record('callback_state', ['binding', $agent]).in_process }};
IF $address != NONE AND $agent != NONE AND $binding = NONE {{ THROW string::concat('refused: binding provenance is not recorded yet; retry just tasks callback --as ', $actor, ' so the current hook can classify the session'); }};
IF $address != NONE AND $agent != NONE AND $binding = true {{ THROW 'refused: in-process agent binding cannot register a callback; use just tasks inbox'; }};
LET $other = (SELECT VALUE handle FROM callback_state WHERE address = $address AND handle != $actor AND type::record('session', handle).left_at = NONE ORDER BY handle LIMIT 1)[0];
IF $address != NONE AND $other != NONE {{ THROW string::concat('refused: callback address already held by ', $other, '; in-process subagents use just tasks inbox'); }};
LET $old = (SELECT VALUE address FROM ONLY type::record('callback_state', $actor));
LET $old_owner = (SELECT VALUE owner FROM ONLY type::record('callback_state', $actor));
IF $old != $address AND ($old = NONE OR $address = NONE OR $old_owner = NONE OR $old_owner != $owner) {{
    UPDATE type::record('callback_state', $actor) SET pull_entries = NONE, pull_said = NONE, pull_withheld = [], pull_awaiting = NONE, pull_queued = NONE RETURN NONE;
    UPDATE callback_event SET delivered = true, text = '' WHERE handle = $actor AND delivered != true AND pull_entries != NONE RETURN NONE;
}};
IF $address != NONE AND ($old != $address OR ($chat AND !$if_free)) {{
    UPDATE callback_event SET attempts = 0, retry_at = 0, capacity_limited = false WHERE handle = $actor AND delivered != true RETURN NONE;
}};
LET $occupied = array::len(SELECT handle FROM callback_state WHERE chat = true AND address != NONE AND type::record('session', handle).left_at = NONE AND handle != $actor) > 0;
LET $take = $chat AND (!$if_free OR !$occupied);
IF $take {{
    UPDATE callback_state SET chat = false WHERE chat = true RETURN NONE;
    UPDATE callback_event SET delivered = true, text = '' WHERE handle != $actor AND string::starts_with(key, 'chat-') RETURN NONE;
}};
IF $address = NONE {{ UPDATE callback_event SET delivered = true, text = '' WHERE handle = $actor AND string::starts_with(key, 'chat-') RETURN NONE; }};
UPSERT type::record('callback_state', $actor) SET handle = $actor, address = $address, host = $host, chat_fallback = $chat AND $if_free,
    registration_revision = (registration_revision ?? 0) + 1, registration_nonce = rand::string(32),
    owner = $owner, confirmed_until = 0, chat_resume = false, terminal = $terminal,
    chat = IF $address = NONE {{ false }} ELSE IF $take {{ true }} ELSE {{ chat ?? false }} RETURN NONE;
IF $address != NONE AND string::starts_with($address, 'codex:') {{
    UPDATE task_session SET claude_session = $address WHERE handle = $actor AND closed = NONE AND claude_session = 'codex:' RETURN NONE;
}};
RETURN NONE;")
}

/// The orchestrators on the chat seat's waiting list that have not left and registered no channel.
pub(in crate::store) const WAITING_ORCHESTRATORS: &str = "LET $waiting = (SELECT VALUE waiting FROM ONLY callback_state:chat) ?? [];
             RETURN SELECT * OMIT id FROM $waiting.map(|$h| type::record('session', $h))
                WHERE kind = 'orchestrator' AND left_at = NONE AND type::record('callback_state', handle).address = NONE;";

/// The kind of `$handle`'s session, none when the board holds no such session.
pub(in crate::store) const SESSION_KIND: &str =
    "RETURN (SELECT VALUE kind FROM ONLY type::record('session', $handle));";

/// The owner's chat events no session received, in their order.
pub(in crate::store) const UNHEARD_CHAT: &str =
    "RETURN SELECT key, text FROM chat_event WHERE delivered = false ORDER BY key NUMERIC;";

/// Every registered channel of a session that has not left, with its session and whether its
/// agent's binding is in-process.
pub(in crate::store) const REGISTRATIONS: &str = "RETURN SELECT handle, address, host, terminal, registration_revision ?? 0 AS registration_revision, registration_nonce, chat ?? false AS chat, chat_fallback ?? false AS chat_fallback, owner, confirmed_until ?? 0 AS confirmed_until,
                (SELECT * OMIT id FROM ONLY type::record('session', $parent.handle)) AS session, chat_resume ?? false AS chat_resume, pane_watch ?? {} AS pane_watch,
                IF type::record('session', handle).agent = NONE { false } ELSE { type::record('callback_state', ['binding', type::record('session', handle).agent]).in_process ?? true } AS in_process
             FROM callback_state WHERE address != NONE AND type::record('session', handle).left_at = NONE;";

/// The delivered outbox lines deleted, and the bindings of agents no present session runs.
pub(in crate::store) const PRUNE: &str = "DELETE callback_event WHERE delivered = true OR (string::starts_with(key, 'chat-') AND type::record('chat_event', string::slice(key, 5)).delivered = true) RETURN NONE;
             LET $agents = array::distinct(SELECT VALUE agent FROM session WHERE agent != NONE AND left_at = NONE);
             DELETE callback_state WHERE in_process != NONE AND record::id(id)[1] NOT IN $agents RETURN NONE;
             RETURN NONE;";

/// `$handle`'s channel (`$address`, `$owner`) confirmed until `$until` and its session seen at `$now`,
/// its chat seat given back when it had yielded it: false when the registration is another's or the
/// session left.
pub(in crate::store) const SEEN: &str = "IF type::record('callback_state', $handle).address != $address OR type::record('callback_state', $handle).owner != $owner OR type::record('session', $handle).left_at != NONE { RETURN false; };
             LET $blocked = type::record('callback_state', $handle).blocked ?? false;
             LET $resume = type::record('callback_state', $handle).chat_resume ?? false;
             LET $seat = type::record('callback_state', $handle).chat_resume_seat ?? true;
             LET $occupied = array::len(SELECT handle FROM callback_state WHERE chat = true AND address != NONE AND handle != $handle AND type::record('session', handle).left_at = NONE) > 0;
             IF $resume AND (!$seat OR !$occupied) {
                UPDATE type::record('callback_state', $handle) SET chat = $seat, chat_fallback = chat_resume_fallback ?? false, chat_resume = false RETURN NONE;
             };
             UPDATE type::record('callback_state', $handle) SET confirmed_until = $until, blocked = false RETURN NONE;
             IF $blocked { UPDATE callback_event SET attempts = 0, retry_at = 0 WHERE handle = $handle AND delivered != true AND capacity_limited != true RETURN NONE; };
             UPDATE type::record('session', $handle) SET last_seen = math::max([last_seen, $now]) RETURN NONE;
             RETURN true;";

/// `$handle`'s channel (`$address`, `$owner`) marked blocked, once.
pub(in crate::store) const BLOCKED: &str = "UPDATE type::record('callback_state', $handle) SET blocked = true WHERE address = $address AND owner = $owner AND (blocked ?? false) = false RETURN NONE; RETURN NONE;";

/// `$handle`'s undelivered outbox lines that are no board message, the owner's chat events among
/// them only when `$chat`, in their order.
pub(in crate::store) const INBOX: &str = "RETURN SELECT * OMIT id FROM callback_event WHERE handle = $handle AND delivered != true AND message = NONE
                AND (!string::starts_with(key, 'chat-') OR ($chat AND type::record('chat_event', string::slice(key, 5)).delivered != true)) ORDER BY sequence, key NUMERIC;";

/// `$actor`'s outbox lines `$keys` and the owner's chat events `$chats` acknowledged, the newest PULL
/// among them recorded as the one it was told. Needs `ACT`'s variables.
pub(in crate::store) fn ack() -> String {
    format!("{ACT}\nLET $pull = (SELECT pull_entries, pull_withheld, pull_said, sequence FROM callback_event WHERE handle = $actor AND key IN $keys AND pull_entries != NONE ORDER BY sequence DESC LIMIT 1)[0];
                IF $pull != NONE {{ UPDATE type::record('callback_state', $actor) SET pull_unanswered = array::union(pull_unanswered ?? [], $pull.pull_entries.filter(|$entry| $entry[0] = 'unanswered' AND $entry NOT IN ($pull.pull_withheld ?? []))), pull_entries = $pull.pull_entries, pull_withheld = $pull.pull_withheld ?? [], pull_said = $pull.pull_said, pull_idle = $pull.pull_said.idle ?? pull_idle, pull_awaiting = NONE, pull_queued = NONE RETURN NONE; }};
                FOR $key IN $keys {{ UPDATE type::record('callback_event', [$actor, $key]) SET delivered = true RETURN NONE; }};
                FOR $key IN $chats {{ UPDATE type::record('chat_event', $key) SET delivered = true, text = '' RETURN NONE; }};
                UPDATE callback_event SET text = '' WHERE handle = $actor AND delivered = true AND string::starts_with(key, 'chat-') RETURN NONE; RETURN NONE;")
}

/// `$handle`'s session, its callback state and whether it is on the chat seat's waiting list.
pub(in crate::store) const READER: &str = "RETURN {
                    session: (SELECT * OMIT id FROM ONLY type::record('session', $handle)),
                    state: (SELECT address, pane_watch FROM ONLY type::record('callback_state', $handle)),
                    waiting: $handle IN ((SELECT VALUE waiting FROM ONLY callback_state:chat) ?? []),
                };";

/// `$handle`, here and with no channel, given its state row and put on the chat seat's waiting list.
pub(in crate::store) const WAIT: &str = "IF type::record('session', $handle).left_at = NONE AND type::record('callback_state', $handle).address = NONE {
                    IF !record::exists(type::record('callback_state', $handle)) {
                        CREATE type::record('callback_state', $handle) SET handle = $handle RETURN NONE;
                    };
                    UPSERT callback_state:chat SET waiting = array::union(waiting ?? [], [$handle]) RETURN NONE;
                 }; RETURN NONE;";

/// `$handle`'s lease taken for `$lease_seconds` from `$now`, and its new revision: none while another
/// holds it, once the session left, or with no channel unless `$collect`.
pub(in crate::store) const TAKE_LEASE: &str = "LET $s = (SELECT address, lease, revision FROM ONLY type::record('callback_state', $handle));
                 LET $lease = (SELECT * FROM ONLY type::record('callback_lease', $handle));
                 IF ($s.address = NONE AND !$collect) OR type::record('session', $handle).left_at != NONE { RETURN NONE; };
                 IF math::max([$s.lease ?? 0, $lease.until ?? 0]) > $now { RETURN NONE; };
                 LET $revision = math::max([$lease.revision ?? 0, $s.revision ?? 0]) + 1;
                 UPSERT type::record('callback_lease', $handle) SET until = $now + $lease_seconds, revision = $revision RETURN NONE;
                 RETURN $revision;";

/// `$handle`'s lease let go, while it is still revision `$revision`.
pub(in crate::store) const RELEASE_LEASE: &str = "UPDATE type::record('callback_lease', $handle) SET until = 0 WHERE revision = $revision RETURN NONE; RETURN NONE;";

/// Each of `$recipients`' callback state (each recipient `{handle, chat}`), when its PULL was last
/// checked, the lines still queued for it that a hand-over can take back, and whether a line of its
/// outbox is due ([`PENDING`]): a `Stored` each, in their order, all of them in one read.
pub(in crate::store) static STORED: LazyLock<String> = LazyLock::new(|| {
    format!("RETURN SELECT handle, (SELECT * OMIT id FROM ONLY type::record('callback_state', $parent.handle)) AS state,
                    type::record('callback_lease', handle).pull ?? 0 AS checked_pull,
                    (SELECT key, text, sequence, pull_entries, pull_withheld, pull_said FROM callback_event WHERE handle = $parent.handle AND delivered != true
                        AND (string::starts_with(key, 'pull-') OR string::starts_with(key, 'event-') OR (string::starts_with(key, 'pane-') AND string::starts_with(text, 'BLOCKED ')))) AS queued,
                    {DUE_LINE} AS pending
                FROM $recipients;")
});

/// The owner's undelivered chat events `$handle`'s outbox holds no line of yet, in their order.
pub(in crate::store) const UNQUEUED_CHAT: &str = "RETURN SELECT key, text FROM chat_event WHERE delivered = false
                        AND !record::exists(type::record('callback_event', [$handle, string::concat('chat-', key)])) ORDER BY key NUMERIC;";

/// `$handle`'s PULL recorded as checked at `$now`, on its lease while it is still revision `$revision`.
pub(in crate::store) const PULL_CHECKED: &str = "UPDATE type::record('callback_lease', $handle) SET pull = $now WHERE revision = $revision RETURN NONE; RETURN NONE;";

/// One recipient's observation written with the events it queued, while the dispatcher holds
/// `$handle`'s lease (`$revision`) and the PULLs it read as waiting (`$waiting`) still wait: whether
/// each held, and the board marker before and after (an `Accounted`). Its variables are the
/// observation's (`Store::dispatch_leased`).
pub(in crate::store) fn account() -> String {
    format!("LET $marker = {board_marker};
             LET $held = type::record('callback_lease', $handle).revision = $revision;
             LET $current = array::len($waiting.filter(|$key| record::exists(type::record('callback_event', [$handle, $key]))
                AND type::record('callback_event', [$handle, $key]).delivered != true)) = array::len($waiting);
             IF $held AND $current {{
             IF $pull_kept != NONE {{
                DELETE callback_event WHERE handle = $handle AND delivered != true AND string::starts_with(key, 'pull-') AND key != $pull_kept RETURN NONE;
                UPDATE type::record('callback_event', [$handle, $pull_kept]) SET text = $pull_text, pull_entries = $pull_awaiting.entries,
                    pull_withheld = $pull_awaiting.withheld, pull_said = $pull_said WHERE delivered != true RETURN NONE;
             }};
             IF $replace_pull {{ DELETE callback_event WHERE handle = $handle AND delivered != true AND string::starts_with(key, 'pull-') RETURN NONE; }};
             FOR $key IN $rescoped.dropped {{ DELETE type::record('callback_event', [$handle, $key]) WHERE delivered != true RETURN NONE; }};
             FOR $line IN $rescoped.rewritten {{ UPDATE type::record('callback_event', [$handle, $line[0]]) SET text = $line[1] WHERE delivered != true RETURN NONE; }};
             FOR $new IN $events.filter(|$event| string::starts_with($event.key, 'event-') AND string::starts_with($event.text, 'NEW ')) {{
                LET $prefix = string::concat('NEW ', string::split($new.text, ' ')[1], ' ready — ');
                DELETE callback_event WHERE handle = $handle AND delivered != true AND string::starts_with(key, 'event-') AND string::starts_with(text, $prefix) RETURN NONE;
             }};
             FOR $event IN $events {{
                IF !record::exists(type::record('callback_event', [$handle, $event.key])) {{
                    LET $counted = (UPSERT ONLY type::record('counter', ['callback-event', $handle]) SET last = (last ?? 0) + 1);
                    CREATE type::record('callback_event', [$handle, $event.key]) CONTENT $event RETURN NONE;
                    UPDATE type::record('callback_event', [$handle, $event.key]) SET sequence = $counted.last RETURN NONE;
                }};
             }};
             IF $computed {{ UPDATE type::record('callback_lease', $handle) SET pull = $pull WHERE revision = $revision RETURN NONE; }};
             LET $before = (SELECT seen, narrowing, unknown_children, statuses, pull_queued, inbox_cursor, message_cursor, delivery_version, registration_revision, observed_registration FROM ONLY type::record('callback_state', $handle));
             IF $before.seen != $seen OR $before.narrowing != $narrowing OR $before.unknown_children != $unknown_children OR $before.statuses != $statuses
                OR $before.pull_queued != $pull_queued OR $before.inbox_cursor != $inbox_cursor
                OR ($before.message_cursor ?? 0) < $message_cursor OR $before.delivery_version = NONE
                OR ($before.registration_revision ?? 0) != ($before.observed_registration ?? 0) OR $computed {{
             UPDATE type::record('callback_state', $handle) SET seen = $seen, narrowing = $narrowing, unknown_children = $unknown_children, snapshot_sequence = $snapshot_sequence, statuses = $statuses, revision = $revision, pull = $pull, pull_queued = $pull_queued,
                observed_registration = registration_revision ?? 0,
                pull_entries = IF $legacy_upgrade {{ $pull_account.recorded ?? pull_entries ?? [] }}
                    ELSE IF $computed AND pull_entries != NONE {{
                        array::union(pull_entries.filter(|$entry| $entry IN $pull_current AND $entry NOT IN $pull_account.untold), $pull_account.unheard)
                    }} ELSE {{ pull_entries }},
                pull_withheld = IF $computed {{ $pull_account.withheld }} ELSE {{ pull_withheld }},
                pull_awaiting = IF $computed {{ $pull_awaiting }} ELSE {{ pull_awaiting }},
                message_cursor = math::max([message_cursor ?? 0, $message_cursor]),
                inbox_cursor = $inbox_cursor,
                delivery_version = delivery_version ?? 1 RETURN NONE;
             }};
             }};
             RETURN {{ held: $held, current: $current, marker: {{ before: $marker, after: {board_marker} }} }};", board_marker = crate::schema::BOARD_MARKER)
}

/// Whether a line of the outbox of the recipient `$parent` (`{handle, chat}`) is due at `$now`, the
/// owner's chat events only when its `chat`.
const DUE_LINE: &str = "array::len(SELECT key FROM callback_event WHERE handle = $parent.handle AND delivered != true AND attempts < 3 AND (retry_at = NONE OR retry_at <= $now)
                AND (!string::starts_with(key, 'chat-') OR ($parent.chat AND type::record('chat_event', string::slice(key, 5)).delivered != true)) LIMIT 1) > 0";

/// Whether a line of each of `$recipients`' outboxes is due at `$now` (each recipient `{handle,
/// chat}`, the owner's chat events only when its `chat`): `{handle, pending}` each, in their order,
/// all of them in one read.
pub(in crate::store) static PENDING: LazyLock<String> =
    LazyLock::new(|| format!("RETURN SELECT handle, {DUE_LINE} AS pending FROM $recipients;"));

/// `$handle`'s lease renewed for `$lease_seconds` from `$now` and its outbox lines due then, in their
/// order, the owner's chat events only when `$chat`: none once the lease is no longer revision
/// `$revision`.
pub(in crate::store) const DUE: &str = "IF type::record('callback_lease', $handle).revision != $revision { RETURN NONE; };
             UPDATE type::record('callback_lease', $handle) SET until = $now + $lease_seconds RETURN NONE;
             UPDATE callback_event SET delivered = true WHERE handle = $handle AND delivered != true AND message != NONE AND record::exists(type::record('delivery', [message, $handle])) RETURN NONE;
             DELETE callback_event WHERE handle = $handle AND delivered = true RETURN NONE;
             RETURN SELECT * OMIT id FROM callback_event WHERE handle = $handle AND delivered != true AND attempts < 3 AND (retry_at = NONE OR retry_at <= $now)
                AND (!string::starts_with(key, 'chat-') OR ($chat AND type::record('chat_event', string::slice(key, 5)).delivered != true)) ORDER BY sequence, key NUMERIC;";

/// One batch's outcome recorded on `$handle`'s lines `$keys`: an attempt each, and when `$success`
/// the messages (`$messages`), the chat events (`$chat`) and the PULL (`$pull`) it carried recorded
/// as delivered at `$now`. The board marker before and after (a `Moved`).
pub(in crate::store) fn record_delivery() -> String {
    format!("LET $marker = {board_marker};
             FOR $key IN $keys {{
                UPDATE type::record('callback_event', [$handle, $key]) SET attempts += 1, delivered = $success, retry_at = $now + 5,
                    capacity_limited = $capacity OR (capacity_limited ?? false) RETURN NONE;
             }};
             IF $success {{
                IF $pull != NONE {{ UPDATE type::record('callback_state', $handle) SET pull_unanswered = array::union(pull_unanswered ?? [], $pull.pull_entries.filter(|$entry| $entry[0] = 'unanswered' AND $entry NOT IN ($pull.pull_withheld ?? []))), pull_entries = $pull.pull_entries, pull_withheld = $pull.pull_withheld ?? [], pull_said = $pull.pull_said, pull_idle = $pull.pull_said.idle ?? pull_idle, pull_awaiting = NONE, pull_queued = NONE RETURN NONE; }};
                FOR $message IN $messages {{ UPSERT type::record('delivery', [$message, $handle]) SET message = $message, handle = $handle, acked = $now RETURN NONE; }};
                FOR $key IN $chat {{ UPDATE type::record('chat_event', $key) SET delivered = true, text = '' RETURN NONE; }};
                UPDATE callback_event SET text = '' WHERE handle = $handle AND delivered = true AND string::starts_with(key, 'chat-') RETURN NONE;
             }}; RETURN {{ before: $marker, after: {board_marker} }};", board_marker = crate::schema::BOARD_MARKER)
}

/// `$handle`'s channel (`$address`, `$owner`) yielding the chat seat, remembered so a later confirmed
/// presence gives it back.
pub(in crate::store) const YIELD_CHAT_SEAT: &str = "LET $state = (SELECT address, owner, chat, chat_fallback, chat_resume FROM ONLY type::record('callback_state', $handle));
                     IF $state.address = $address AND $state.owner = $owner AND !($state.chat_resume ?? false) {
                        UPDATE type::record('callback_state', $handle) SET chat = false, chat_fallback = false, chat_resume = true,
                            chat_resume_seat = $state.chat ?? false, chat_resume_fallback = $state.chat_fallback ?? false RETURN NONE;
                     }; RETURN NONE;";

/// The registration `$handle` was read as (`$address`, `$owner`, `$terminal`, `$revision`, `$nonce`)
/// retired, and whether it was: false for one registered since. When `held`, only by the close
/// holding the session ([`HOLDS`], with its variables).
pub(in crate::store) fn retire(held: bool) -> String {
    let guard = if held { HOLDS } else { "" };
    format!("{guard}
             LET $state = (SELECT address, owner, terminal, registration_revision ?? 0 AS revision, registration_nonce AS nonce FROM ONLY type::record('callback_state', $handle));
             IF $state.address != $address OR $state.owner != $owner OR $state.terminal != $terminal OR $state.revision != $revision OR $state.nonce != $nonce {{ RETURN false }};
             DELETE type::record('callback_state', $handle) RETURN NONE;
             UPDATE callback_event SET delivered = true, text = '' WHERE handle = $handle AND string::starts_with(key, 'chat-') RETURN NONE;
             RETURN true;")
}

/// A system line `$text` put on `$slug`'s thread at `$now`: a delivery that failed for good.
pub(in crate::store) fn failure_logged() -> String {
    format!(
        "{}\nRETURN NONE;",
        logged("$slug", "'callbacks'", "'system'", "$text", "NONE")
    )
}
