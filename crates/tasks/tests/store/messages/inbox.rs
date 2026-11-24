//! Inbox acknowledgements, and the inbox of a recipient further behind than a dispatch pass reads.

use super::texts;
use crate::common::{Fixture, NOW, Seed};

#[test]
fn ack_empties_the_inbox_and_new_messages_arrive_again() {
    let f = Fixture::new(Seed::default());
    let a = f.session("worker");
    f.task("sim/jump");
    f.store.claim("sim/jump", &a).unwrap();
    f.store.say("sim/jump", "owner", "one", None).unwrap();
    let ids: Vec<i64> = f.store.inbox(&a).unwrap().iter().map(|m| m.id).collect();
    f.store.ack(&a, &ids).unwrap();
    f.store.ack(&a, &ids).unwrap(); // again: nothing new
    assert!(f.store.inbox(&a).unwrap().is_empty());
    f.store.say("sim/jump", "owner", "two", None).unwrap();
    assert_eq!(texts(f.store.inbox(&a).unwrap()), ["two"]);
    assert_eq!(f.store.raw().table("delivery").count().unwrap(), 1);
}

#[test]
fn acknowledgements_keep_adjacent_large_integer_ids_and_recipients_distinct() {
    use symphony_board::rows::{DeliveryRow, MessageRow};

    let ids = [
        1,
        9_007_199_254_740_992,
        9_007_199_254_740_993,
        i64::MAX - 2,
    ];
    let mut seed = Seed::default()
        .session("amber-heron")
        .session("brisk-finch")
        .task("sim/jump", "ready");
    seed.dump.messages = ids
        .iter()
        .map(|&id| MessageRow {
            id,
            slug: "sim/jump".into(),
            sender: "owner".into(),
            recipient: Some("amber-heron".into()),
            kind: "msg".into(),
            text: format!("message {id}"),
            ts: NOW,
            person: false,
        })
        .collect();
    seed.dump.deliveries = [
        (ids[0], "amber-heron"),
        (ids[1], "amber-heron"),
        (ids[2], "brisk-finch"),
    ]
    .into_iter()
    .map(|(message, handle)| DeliveryRow {
        message,
        handle: handle.into(),
        acked: NOW,
    })
    .collect();
    let f = Fixture::new(seed);
    let unread = || {
        f.store
            .inbox("amber-heron")
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(unread(), ids[2..]);
    f.store.ack("amber-heron", &[ids[2]]).unwrap();
    assert_eq!(unread(), [ids[3]]);
    f.store.ack("amber-heron", &[ids[3]]).unwrap();
    assert!(unread().is_empty());
}

/// A recipient further behind than one dispatch pass's pool reaches (a session long unobserved,
/// its cursor 2100 lines back) reads its inbox from the message table, from its cursor on, and
/// still hears what was said to it, and nothing of the lines it could never hear.
#[test]
fn a_recipient_far_behind_the_board_reads_its_inbox_from_its_cursor() {
    use crate::callbacks::Listener;
    use symphony_board::rows::MessageRow;
    let mut seed = Seed::default()
        .session("worker")
        .session("writer")
        .task("tools/far", "in_progress");
    seed.dump.messages = (1..=2100)
        .map(|id| MessageRow {
            id,
            slug: "tools/far".into(),
            sender: "writer".into(),
            recipient: None,
            kind: "msg".into(),
            text: format!("line {id}"),
            ts: NOW,
            person: false,
        })
        .collect();
    let f = Fixture::new(seed);
    let listener = Listener::new(f.store.database().database());
    f.store
        .callback(
            "worker",
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    (f.store.raw().row("callback_state", "worker"))
        .set(serde_json::json!({"message_cursor": 1, "inbox_cursor": 1}))
        .unwrap();
    f.store
        .say("tools/far", "writer", "far news", Some("worker"))
        .unwrap();
    let heard = listener.drain(&f, "writer", "worker");
    assert!(
        heard.iter().any(|line| line.contains("far news")),
        "{heard:?}"
    );
    assert!(
        !heard.iter().any(|line| line.contains("line ")),
        "{heard:?}"
    );
}

/// The reader advances only after acknowledging every line it saw. Writers share the message
/// counter, so no committed smaller id can appear behind that cursor afterwards.
#[test]
fn concurrent_writers_leave_no_gap_or_duplicate_in_a_readers_inbox() {
    use std::collections::BTreeSet;
    use std::sync::{Arc, Barrier};
    let f = Fixture::new(Seed::default());
    let reader = f.session("worker");
    let writers: Vec<_> = (0..3).map(|_| f.session("worker")).collect();
    for slug in ["one", "two"] {
        f.task(slug);
    }
    let stores: Vec<_> = writers.iter().map(|_| f.other()).collect();
    let start = Arc::new(Barrier::new(4));
    let mut seen = Vec::new();
    let mut posted = Vec::new();
    std::thread::scope(|scope| {
        let joins: Vec<_> = stores
            .into_iter()
            .zip(&writers)
            .map(|(store, writer)| {
                let start = Arc::clone(&start);
                let reader = &reader;
                scope.spawn(move || {
                    start.wait();
                    (0..12)
                        .map(|n| {
                            store
                                .say(
                                    if n % 2 == 0 { "one" } else { "two" },
                                    writer,
                                    &format!("{writer}:{n}"),
                                    Some(reader),
                                )
                                .unwrap()
                                .id
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        start.wait();
        loop {
            let messages = f.store.inbox(&reader).unwrap();
            let ids: Vec<_> = messages.iter().map(|m| m.id).collect();
            for id in &ids {
                assert!(
                    seen.last().is_none_or(|last| last < id),
                    "{seen:?} then {ids:?}"
                );
                seen.push(*id);
            }
            f.store.ack(&reader, &ids).unwrap();
            if joins.iter().all(|join| join.is_finished()) {
                break;
            }
            std::thread::yield_now();
        }
        for join in joins {
            posted.extend(join.join().unwrap());
        }
    });
    let remaining = f.store.inbox(&reader).unwrap();
    let ids: Vec<_> = remaining.iter().map(|m| m.id).collect();
    for id in &ids {
        assert!(seen.last().is_none_or(|last| last < id));
        seen.push(*id);
    }
    f.store.ack(&reader, &ids).unwrap();
    posted.sort_unstable();
    assert_eq!(seen, posted);
    assert_eq!(seen.len(), 36);
    assert_eq!(seen.iter().collect::<BTreeSet<_>>().len(), 36);
    assert!(f.store.inbox(&reader).unwrap().is_empty());
}
