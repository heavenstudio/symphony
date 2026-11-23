//! The statements of `store/pane_watch.rs`.

use std::sync::LazyLock;

/// The pane lines still queued for the recipient `handle` names.
fn queued(handle: &str) -> String {
    format!(
        "(SELECT key, text FROM callback_event WHERE handle = {handle} AND delivered != true AND message = NONE AND string::starts_with(key, 'pane-'))"
    )
}

/// The pane lines still queued for each of `$recipients` (each `{handle}`): `{handle, queued}` each,
/// in their order, all of them in one read.
pub(in crate::store) static QUEUED: LazyLock<String> = LazyLock::new(|| {
    format!(
        "RETURN SELECT handle, {} AS queued FROM $recipients;",
        queued("$parent.handle")
    )
});

/// `$handle`'s saved pane watch and the pane lines still queued for it (a `Saved`).
pub(in crate::store) static SAVED: LazyLock<String> = LazyLock::new(|| {
    format!(
        "RETURN {{ watch: (SELECT VALUE pane_watch FROM ONLY type::record('callback_state', $handle)) ?? {{}}, queued: {} }};",
        queued("$handle")
    )
});

/// A look's outcome for `$handle`: the queued pane lines `$retired` deleted, `$events` queued, each
/// once, and the watch saved as `$after`.
pub(in crate::store) const RECORD: &str = "DELETE callback_event WHERE handle = $handle AND delivered != true AND message = NONE AND key IN $retired RETURN NONE;
                 FOR $event IN $events {
                    IF !record::exists(type::record('callback_event', [$handle, $event.key])) {
                        LET $counted = (UPSERT ONLY type::record('counter', ['callback-event', $handle]) SET last = (last ?? 0) + 1);
                        CREATE type::record('callback_event', [$handle, $event.key]) CONTENT $event RETURN NONE;
                        UPDATE type::record('callback_event', [$handle, $event.key]) SET sequence = $counted.last RETURN NONE;
                    };
                 };
                 UPDATE type::record('callback_state', $handle) SET pane_watch = $after RETURN NONE;
                 RETURN NONE;";
