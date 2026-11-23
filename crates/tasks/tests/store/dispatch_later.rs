//! A command line's writes queue what each recipient hears, then hand its delivery to one detached
//! dispatcher a host at a time (`Store::dispatching_by`, `Store::dispatch_detached`): the write
//! answers without delivering, and every recipient still hears each message once. What a
//! dispatcher that finds another running does is `dispatch_holder`'s.

use super::callbacks::Listener;
use super::common::{Fixture, HOST};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use symphony_board::Store;

/// The message ids among `lines` (posts and verdicts), each with how often it came.
pub(super) fn ids(lines: &[String]) -> BTreeMap<i64, usize> {
    let mut ids = BTreeMap::new();
    for line in lines {
        let id = (line.strip_prefix("MSG #"))
            .or_else(|| line.strip_prefix("VERDICT #"))
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|id| id.parse().ok());
        if let Some(id) = id {
            *ids.entry(id).or_default() += 1;
        }
    }
    ids
}

/// A worker executing a task of its own, its callback a listener of the test's.
pub(super) struct Recipient {
    pub(super) handle: String,
    pub(super) slug: String,
    pub(super) listener: Listener,
}

/// The `n`th recipient: a worker that claimed a task of its own and listens.
pub(super) fn recipient(f: &Fixture, n: usize) -> Recipient {
    let handle = f.session("worker");
    let slug = format!("tools/load-{n}");
    f.task(&slug);
    f.store.claim(&slug, &handle).unwrap();
    let listener = listen(
        &f.store,
        &handle,
        &format!("{}-{n}", f.store.database().database()),
    );
    Recipient {
        handle,
        slug,
        listener,
    }
}

/// `handle`'s callback, registered through `store`: a listener of the test's.
pub(super) fn listen(store: &Store, handle: &str, name: &str) -> Listener {
    let listener = Listener::new(name);
    store
        .callback(
            handle,
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    listener
}

/// Another store on the board whose writes hand their delivery over, each hand-off counted.
pub(super) fn handing(f: &Fixture, asked: &Arc<AtomicUsize>) -> Store {
    let asked = Arc::clone(asked);
    f.other().dispatching_by(Box::new(move || {
        asked.fetch_add(1, Ordering::SeqCst);
        true
    }))
}

/// Another store on the board whose writes hand their delivery to a dispatcher that never runs.
pub(super) fn deferring(f: &Fixture) -> Store {
    f.other().dispatching_by(Box::new(|| true))
}

/// `host`'s dispatch lease as it stands.
fn lease_of(f: &Fixture, host: &str) -> Value {
    (f.store
        .raw()
        .row("callback_lease", json!(["dispatch", host])))
    .fields(&["until", "asked", "served", "completed"])
    .unwrap()
}

/// This host's dispatch lease as it stands.
pub(super) fn lease(f: &Fixture) -> Value {
    lease_of(f, HOST)
}

/// Whether the outbox holds `message` for `handle`, and whether it went out.
fn queued(f: &Fixture, handle: &str, message: i64) -> Option<bool> {
    let key = format!("message-{message}");
    let row = f.store.raw().row("callback_event", json!([handle, key]));
    let row = row.fields(&["delivered"]).unwrap();
    match &row["delivered"] {
        _ if row.is_null() => None,
        Value::Null => Some(false),
        delivered => delivered.as_bool(),
    }
}

#[test]
fn a_write_that_hands_its_delivery_over_queues_it_and_answers_before_any_delivery() {
    let f = Fixture::new(crate::common::Seed::default());
    let to = recipient(&f, 0);
    let writer = f.session("worker");
    let before = to.listener.drain(&f, &writer, &to.handle);
    assert!(before.is_empty(), "{before:?}");
    let asked = Arc::new(AtomicUsize::new(0));
    let handing = handing(&f, &asked);
    let id = handing
        .say(&to.slug, &writer, "after the answer", Some(&to.handle))
        .unwrap()
        .id;
    assert_eq!(asked.load(Ordering::SeqCst), 1, "the post hands one over");
    assert_eq!(
        queued(&f, &to.handle, id),
        Some(false),
        "the write queued the post for its recipient and delivered nothing"
    );
    assert_eq!(f.other().dispatch_detached().unwrap(), 1);
    assert_eq!(
        ids(&to.listener.drain(&f, &writer, &to.handle)),
        BTreeMap::from([(id, 1)])
    );
    assert_eq!(
        lease(&f),
        json!({"until": 0, "asked": 0, "served": 0, "completed": 0})
    );
    handing.session(&writer).unwrap();
    handing.touch(&writer).unwrap();
    assert_eq!(
        asked.load(Ordering::SeqCst),
        1,
        "a read or a heartbeat hands nothing over"
    );
}

#[test]
fn a_pass_delivers_from_its_one_read_of_the_messages_what_the_inbox_lists() {
    let f = Fixture::new(crate::common::Seed::default());
    let to = recipient(&f, 0);
    let other = recipient(&f, 1);
    let (writer, reviewer) = (f.session("worker"), f.session("reviewer"));
    f.store.take_role(&to.slug, &reviewer, "reviewer").unwrap();
    // The reviewer's role on its task goes out first, a change it observes, no message.
    let first = to.listener.drain(&f, &writer, &to.handle);
    assert_eq!(ids(&first), BTreeMap::new(), "{first:?}");
    let asked = Arc::new(AtomicUsize::new(0));
    let handing = handing(&f, &asked);
    let say = |slug: &str, by: &str, text: &str, to: Option<&str>| {
        handing.say(slug, by, text, to).unwrap().id
    };
    // Every kind of post, piled up for one pass: the pass reads the messages once for every
    // recipient and filters them for each, where the inbox scans the tables for one.
    say(&to.slug, &writer, "to it by handle", Some(&to.handle));
    say(&to.slug, &writer, "to its role", Some("@executor"));
    say(&to.slug, &writer, "to nobody", None);
    say(&other.slug, &writer, "to another", Some(&other.handle));
    say(&other.slug, &writer, "to another's role", Some("@executor"));
    say(&to.slug, "owner", "from the owner", None);
    say(&to.slug, &to.handle, "its own", Some("@reviewer"));
    let read = say(&to.slug, &writer, "read already", Some(&to.handle));
    f.store.ack(&to.handle, &[read]).unwrap();
    handing.move_to(&to.slug, "review", &to.handle).unwrap();
    handing
        .verdict(&to.slug, &reviewer, "changes", "again")
        .unwrap();
    let listed: BTreeMap<i64, String> = (f.store.inbox(&to.handle).unwrap().into_iter())
        .map(|message| (message.id, message.text))
        .collect();
    assert_eq!(
        listed.values().collect::<Vec<_>>(),
        [
            "to it by handle",
            "to its role",
            "from the owner",
            "changes: again"
        ],
        "the inbox lists what reaches it, read or its own left out"
    );
    let passes = f.other().dispatch_detached().unwrap();
    assert!(passes >= 1, "the dispatcher made {passes} passes");
    assert_eq!(
        ids(&to.listener.drain(&f, &writer, &to.handle)),
        listed.keys().map(|id| (*id, 1)).collect::<BTreeMap<_, _>>()
    );
}

#[test]
fn a_hand_off_that_cannot_start_dispatches_in_the_write() {
    let f = Fixture::new(crate::common::Seed::default());
    let to = recipient(&f, 0);
    let writer = f.session("worker");
    let before = to.listener.drain(&f, &writer, &to.handle);
    assert!(before.is_empty(), "{before:?}");
    let unstarted = f.other().dispatching_by(Box::new(|| false));
    let id = unstarted
        .say(&to.slug, &writer, "here and now", Some(&to.handle))
        .unwrap()
        .id;
    assert_eq!(
        queued(&f, &to.handle, id),
        Some(true),
        "delivered in the write"
    );
    assert_eq!(
        ids(&to.listener.drain(&f, &writer, &to.handle)),
        BTreeMap::from([(id, 1)])
    );
}

#[test]
fn a_dispatcher_on_another_host_delivers_there_while_this_hosts_is_busy() {
    let f = Fixture::new(crate::common::Seed::default());
    let remote = f.elsewhere();
    let there = remote
        .register(&symphony_board::NewSession::new("worker"))
        .unwrap()
        .row
        .handle;
    let writer = f.session("worker");
    f.task("tools/remote");
    remote.claim("tools/remote", &there).unwrap();
    let listener = listen(
        &remote,
        &there,
        &format!("{}-remote", f.store.database().database()),
    );
    // This host's dispatcher is busy for as long as the test runs.
    (f.store
        .raw()
        .row("callback_lease", json!(["dispatch", HOST])))
    .put(json!({"until": f.now() + 3600.0, "revision": 1, "asked": 0, "served": 0}))
    .unwrap();
    let id = f
        .elsewhere()
        .dispatching_by(Box::new(|| true))
        .say("tools/remote", &writer, "for the other host", Some(&there))
        .unwrap()
        .id;
    assert_eq!(
        f.elsewhere().dispatch_detached().unwrap(),
        1,
        "the other host's dispatcher holds a lease of its own"
    );
    let mut lines = Vec::new();
    remote.send_agent(&writer, &there, "host-barrier").unwrap();
    let barrier = format!("AGENT from {writer} to {there} — host-barrier");
    while !lines.contains(&barrier) {
        lines.extend(listener.text().lines().map(String::from));
    }
    assert_eq!(ids(&lines), BTreeMap::from([(id, 1)]), "{lines:?}");
    assert_eq!(
        lease(&f)["asked"],
        json!(0),
        "nothing asked this host's dispatcher for another host's work"
    );
}
