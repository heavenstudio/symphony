//! A command line's writes hand their callback dispatch to one detached dispatcher a board at a time
//! (`Store::dispatching_by`, `Store::dispatch_detached`): the write answers without dispatching,
//! dispatchers that find another running ask it for one more pass, and every recipient still hears
//! each message once.

use super::callbacks::Listener;
use super::common::Fixture;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use symphony_board::Store;

/// The message ids among `lines` (posts and verdicts), each with how often it came.
fn ids(lines: &[String]) -> BTreeMap<i64, usize> {
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
fn recipient(f: &Fixture, n: usize) -> (String, String, Listener) {
    let handle = f.session("worker");
    let slug = format!("tools/load-{n}");
    f.task(&slug);
    f.store.claim(&slug, &handle).unwrap();
    let listener = Listener::new(&format!("{}-{n}", f.store.client().database()));
    f.store
        .callback(
            &handle,
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    (handle, slug, listener)
}

/// Another store on the board whose writes hand their dispatch over, each hand-off counted.
fn handing(f: &Fixture, asked: &Arc<AtomicUsize>) -> Store {
    let asked = Arc::clone(asked);
    f.other().dispatching_by(Box::new(move || {
        asked.fetch_add(1, Ordering::SeqCst);
        true
    }))
}

/// The board's dispatch lease as it stands.
fn lease(f: &Fixture) -> Value {
    f.store
        .client()
        .query(
            "RETURN SELECT until, again FROM ONLY type::record('callback_lease', ['dispatch']);",
            json!({}),
        )
        .unwrap()
        .remove(0)
}

/// Whether a dispatcher holds the board's dispatch lease now.
fn held(f: &Fixture) -> bool {
    lease(f)["until"]
        .as_f64()
        .is_some_and(|until| until > f.now())
}

/// Once a dispatcher holds the lease (the first clock read after `held`): a post to `to` on `slug`,
/// handed over, then that post's own dispatcher, which must only ask. The post's id goes to `posted`.
fn post_while_held(
    board: &Arc<Fixture>,
    (to, slug, by): (&str, &str, &str),
    posted: &Arc<Mutex<Option<i64>>>,
) -> impl Fn() + Send + Sync + 'static {
    let (board, posted, once) = (
        Arc::clone(board),
        Arc::clone(posted),
        AtomicBool::new(false),
    );
    let (to, slug, by) = (to.to_string(), slug.to_string(), by.to_string());
    move || {
        if held(&board) && !once.swap(true, Ordering::SeqCst) {
            let asked = Arc::new(AtomicUsize::new(0));
            let message = handing(&board, &asked)
                .say(&slug, &by, "while it ran", Some(&to))
                .unwrap();
            *posted.lock().unwrap() = Some(message.id);
            assert_eq!(asked.load(Ordering::SeqCst), 1);
            assert_eq!(
                board.other().dispatch_detached().unwrap(),
                0,
                "the lease is held: this dispatcher only asks"
            );
        }
    }
}

#[test]
fn a_write_that_hands_its_dispatch_over_answers_before_any_delivery() {
    let f = Fixture::empty();
    let (handle, slug, listener) = recipient(&f, 0);
    let writer = f.session("worker");
    assert!(listener.drain(&f, &writer, &handle).is_empty());
    let asked = Arc::new(AtomicUsize::new(0));
    let handing = handing(&f, &asked);
    let id = handing
        .say(&slug, &writer, "after the answer", Some(&handle))
        .unwrap()
        .id;
    assert_eq!(asked.load(Ordering::SeqCst), 1, "the post hands one over");
    assert!(
        listener.drain(&f, &writer, &handle).is_empty(),
        "nothing was queued for the recipient before a dispatcher ran"
    );
    assert_eq!(f.other().dispatch_detached().unwrap(), 1);
    assert_eq!(
        ids(&listener.drain(&f, &writer, &handle)),
        BTreeMap::from([(id, 1)])
    );
    assert_eq!(lease(&f), json!({"until": 0, "again": false}));
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
    let f = Fixture::empty();
    let (handle, slug, listener) = recipient(&f, 0);
    let (other, other_slug, _other) = recipient(&f, 1);
    let (writer, reviewer) = (f.session("worker"), f.session("reviewer"));
    f.store.take_role(&slug, &reviewer, "reviewer").unwrap();
    // The reviewer's role on its task goes out first, a change it observes, no message.
    assert_eq!(ids(&listener.drain(&f, &writer, &handle)), BTreeMap::new());
    let asked = Arc::new(AtomicUsize::new(0));
    let handing = handing(&f, &asked);
    let say = |slug: &str, by: &str, text: &str, to: Option<&str>| {
        handing.say(slug, by, text, to).unwrap().id
    };
    // Every kind of post, piled up for one pass: the pass reads the messages once for every
    // recipient and filters them for each, where the inbox scans the tables for one.
    say(&slug, &writer, "to it by handle", Some(&handle));
    say(&slug, &writer, "to its role", Some("@executor"));
    say(&slug, &writer, "to nobody", None);
    say(&other_slug, &writer, "to another", Some(&other));
    say(&other_slug, &writer, "to another's role", Some("@executor"));
    say(&slug, "owner", "from the owner", None);
    say(&slug, &handle, "its own", Some("@reviewer"));
    let read = say(&slug, &writer, "read already", Some(&handle));
    f.store.ack(&handle, &[read]).unwrap();
    handing.move_to(&slug, "review", &handle).unwrap();
    handing
        .verdict(&slug, &reviewer, "changes", "again")
        .unwrap();
    let listed: BTreeMap<i64, String> = (f.store.inbox(&handle).unwrap().into_iter())
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
    assert!(f.other().dispatch_detached().unwrap() >= 1);
    assert_eq!(
        ids(&listener.drain(&f, &writer, &handle)),
        listed.keys().map(|id| (*id, 1)).collect::<BTreeMap<_, _>>()
    );
}

#[test]
fn a_hand_off_that_cannot_start_dispatches_in_the_write() {
    let f = Fixture::empty();
    let (handle, slug, listener) = recipient(&f, 0);
    let writer = f.session("worker");
    assert!(listener.drain(&f, &writer, &handle).is_empty());
    let unstarted = f.other().dispatching_by(Box::new(|| false));
    let id = unstarted
        .say(&slug, &writer, "here and now", Some(&handle))
        .unwrap()
        .id;
    assert_eq!(
        ids(&listener.drain(&f, &writer, &handle)),
        BTreeMap::from([(id, 1)])
    );
}

#[test]
fn a_dispatcher_that_finds_another_running_leaves_it_one_more_pass() {
    let f = Arc::new(Fixture::empty());
    let (handle, slug, listener) = recipient(&f, 0);
    let writer = f.session("worker");
    assert!(listener.drain(&f, &writer, &handle).is_empty());
    let posted = Arc::new(Mutex::new(None));
    let running = f.busy(post_while_held(&f, (&handle, &slug, &writer), &posted));
    assert_eq!(
        running.dispatch_detached().unwrap(),
        2,
        "the running dispatcher makes the pass it was asked for"
    );
    let id = posted.lock().unwrap().expect("the write was made");
    assert_eq!(
        ids(&listener.drain(&f, &writer, &handle)),
        BTreeMap::from([(id, 1)])
    );
    assert_eq!(lease(&f), json!({"until": 0, "again": false}));
}

#[test]
fn a_dispatcher_whose_lease_lapsed_leaves_the_next_holder_its_lease_and_its_asks() {
    let f = Arc::new(Fixture::empty());
    let (handle, slug, listener) = recipient(&f, 0);
    let writer = f.session("worker");
    assert!(listener.drain(&f, &writer, &handle).is_empty());
    let posted = Arc::new(Mutex::new(None));
    let stalled = f.busy({
        let (board, posted, once) = (Arc::clone(&f), Arc::clone(&posted), AtomicBool::new(false));
        let (to, slug, by) = (handle.clone(), slug.clone(), writer.clone());
        // The first dispatcher stalls past its lease: the next takes the board, and while it runs a
        // write's dispatcher asks it for one more pass.
        move || {
            if held(&board) && !once.swap(true, Ordering::SeqCst) {
                board.advance(61.0);
                let next = board.busy(post_while_held(&board, (&to, &slug, &by), &posted));
                assert_eq!(next.dispatch_detached().unwrap(), 2);
            }
        }
    });
    assert_eq!(
        stalled.dispatch_detached().unwrap(),
        1,
        "a holder whose lease lapsed ends after its own pass, neither answering the ask nor releasing"
    );
    assert_eq!(lease(&f), json!({"until": 0, "again": false}));
    let id = posted.lock().unwrap().expect("the write was made");
    assert_eq!(
        ids(&listener.drain(&f, &writer, &handle)),
        BTreeMap::from([(id, 1)])
    );
}

#[test]
fn many_writers_at_once_each_message_reaches_its_recipient_exactly_once() {
    const RECIPIENTS: usize = 4;
    const WRITERS: usize = 16;
    let f = Fixture::empty();
    let recipients: Vec<_> = (0..RECIPIENTS).map(|n| recipient(&f, n)).collect();
    let writers: Vec<String> = (0..WRITERS).map(|_| f.session("worker")).collect();
    for (handle, _, listener) in &recipients {
        assert!(listener.drain(&f, &writers[0], handle).is_empty());
    }
    let asked = Arc::new(AtomicUsize::new(0));
    let start = Arc::new(Barrier::new(WRITERS));
    let threads: Vec<_> = writers
        .iter()
        .enumerate()
        .map(|(n, writer)| {
            let (to, slug, _) = &recipients[n % RECIPIENTS];
            let (to, slug) = (to.clone(), slug.clone());
            let (writing, dispatcher) = (handing(&f, &asked), f.other());
            let (start, writer) = (Arc::clone(&start), writer.clone());
            std::thread::spawn(move || {
                start.wait();
                let id = writing
                    .say(&slug, &writer, &format!("load {n}"), Some(&to))
                    .unwrap()
                    .id;
                dispatcher.dispatch_detached().unwrap();
                (to, id)
            })
        })
        .collect();
    let mut sent: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
    for thread in threads {
        let (to, id) = thread.join().unwrap();
        sent.entry(to).or_default().insert(id);
    }
    assert_eq!(asked.load(Ordering::SeqCst), WRITERS);
    for (handle, _, listener) in &recipients {
        let got = ids(&listener.drain(&f, &writers[0], handle));
        let want: BTreeMap<i64, usize> = sent[handle].iter().map(|id| (*id, 1)).collect();
        assert_eq!(got, want, "{handle}: each of its messages once, no other's");
    }
    assert_eq!(lease(&f), json!({"until": 0, "again": false}));
}
