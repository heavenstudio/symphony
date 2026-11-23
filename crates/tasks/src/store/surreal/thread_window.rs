//! The bounded thread metadata read; its record range is the message primary key.

use crate::store::thread_window::{EVENT_TEXT, THREAD_WINDOW_LIMIT};

pub(in crate::store) fn read() -> String {
    format!("RETURN {{ known: record::exists(type::record('task', $slug)),
        parent: type::record('task', $slug).parent ?? NULL,
        status: type::record('task', $slug).status ?? NULL,
        messages: (SELECT sid, slug, sender, recipient, kind, ts, person = true AS person,
            IF kind = 'system' {{ string::slice(text, 0, {EVENT_TEXT}) }}
                ELSE {{ IF kind = 'verdict' {{ string::slice(text, 0, 16) }} ELSE {{ '' }} }} AS text,
            kind = 'system' AND string::len(text) > {EVENT_TEXT} AS cut
        FROM message:[$slug, NONE]..[$slug, $before]
        ORDER BY sid DESC LIMIT {}) }};", THREAD_WINDOW_LIMIT + 1)
}
