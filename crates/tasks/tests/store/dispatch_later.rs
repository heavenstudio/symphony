//! A command line's writes queue what each recipient hears, then hand its delivery to one detached
//! dispatcher a host at a time (`Store::dispatching_by`, `Store::dispatch_detached`): the write
//! answers without delivering, dispatchers that find another running ask it for one more pass and
//! wait for it, one waiting on a dead holder takes the host over once its lease lapses, and every
//! recipient still hears each message once.

use super::callbacks::Listener;
use super::common::{Fixture, HOST};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
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
struct Recipient {
    handle: String,
    slug: String,
    listener: Listener,
}

fn recipient(f: &Fixture, n: usize) -> Recipient {
    let handle = f.session("worker");
    let slug = format!("tools/load-{n}");
    f.task(&slug);
    f.store.claim(&slug, &handle).unwrap();
    let listener = listen(
        &f.store,
        &handle,
        &format!("{}-{n}", f.store.client().database()),
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
fn handing(f: &Fixture, asked: &Arc<AtomicUsize>) -> Store {
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
    f.store
        .client()
        .query(
            "RETURN SELECT until, asked, served FROM ONLY type::record('callback_lease', ['dispatch', $host]);",
            json!({"host": host}),
        )
        .unwrap()
        .remove(0)
}

/// This host's dispatch lease as it stands.
fn lease(f: &Fixture) -> Value {
    lease_of(f, HOST)
}

/// Whether a dispatcher holds this host's dispatch lease now.
fn held(f: &Fixture) -> bool {
    lease(f)["until"]
        .as_f64()
        .is_some_and(|until| until > f.now())
}

/// Wait until `done`, which another thread brings about within seconds.
fn until(done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "not within 30 s");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Whether `handle` acknowledged `message`, as a delivery records it.
fn delivered(f: &Fixture, message: i64, handle: &str) -> bool {
    f.store
        .client()
        .query(
            "RETURN record::exists(type::record('delivery', [$message, $handle]));",
            json!({"message": message, "handle": handle}),
        )
        .unwrap()
        .remove(0)
        .as_bool()
        .unwrap()
}

/// Whether the outbox holds `message` for `handle`, and whether it went out.
fn queued(f: &Fixture, handle: &str, message: i64) -> Option<bool> {
    let row = f
        .store
        .client()
        .query(
            "RETURN IF record::exists(type::record('callback_event', [$handle, $key])) {
                 type::record('callback_event', [$handle, $key]).delivered ?? false
             } ELSE { NONE };",
            json!({"handle": handle, "key": format!("message-{message}")}),
        )
        .unwrap()
        .remove(0);
    row.as_bool()
}

/// A post while a dispatcher holds the lease, and the dispatcher it starts.
struct WhileHeld {
    to: String,
    slug: String,
    by: String,
    /// The post's id.
    posted: Mutex<Option<i64>>,
    /// The post's own dispatcher, which waits on the holder: the passes it made.
    waiter: Mutex<Option<JoinHandle<usize>>>,
}

impl WhileHeld {
    fn new(to: &Recipient, by: &str) -> Arc<WhileHeld> {
        Arc::new(WhileHeld {
            to: to.handle.clone(),
            slug: to.slug.clone(),
            by: by.to_string(),
            posted: Mutex::new(None),
            waiter: Mutex::new(None),
        })
    }

    /// Once a dispatcher holds the lease (the first clock read after `held`): the post, handed
    /// over, and its own dispatcher, started beside the holder.
    fn meanwhile(self: &Arc<Self>, board: &Arc<Fixture>) -> impl Fn() + Send + Sync + 'static {
        let (board, this, once) = (Arc::clone(board), Arc::clone(self), AtomicBool::new(false));
        move || {
            if held(&board) && !once.swap(true, Ordering::SeqCst) {
                let asked = Arc::new(AtomicUsize::new(0));
                let message = handing(&board, &asked)
                    .say(&this.slug, &this.by, "while it ran", Some(&this.to))
                    .unwrap();
                *this.posted.lock().unwrap() = Some(message.id);
                assert_eq!(asked.load(Ordering::SeqCst), 1);
                let dispatcher = board.other();
                let asks = lease(&board)["asked"].as_u64().unwrap_or_default();
                *this.waiter.lock().unwrap() = Some(std::thread::spawn(move || {
                    dispatcher.dispatch_detached().unwrap()
                }));
                // The holder goes on only once the ask is in, as a dispatcher started by a write
                // the holder's pass did not see asks before that pass ends.
                until(|| lease(&board)["asked"].as_u64() == Some(asks + 1));
            }
        }
    }

    fn posted(&self) -> i64 {
        self.posted.lock().unwrap().expect("the write was made")
    }

    fn waited(&self) -> usize {
        let waiter = self.waiter.lock().unwrap().take();
        waiter.expect("its dispatcher started").join().unwrap()
    }
}

#[test]
fn a_write_that_hands_its_delivery_over_queues_it_and_answers_before_any_delivery() {
    let f = Fixture::empty();
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
    assert_eq!(lease(&f), json!({"until": 0, "asked": 0, "served": 0}));
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
    let f = Fixture::empty();
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
fn a_dispatcher_that_finds_another_running_waits_for_the_pass_it_asked_for() {
    let f = Arc::new(Fixture::empty());
    let to = recipient(&f, 0);
    let writer = f.session("worker");
    let before = to.listener.drain(&f, &writer, &to.handle);
    assert!(before.is_empty(), "{before:?}");
    let meanwhile = WhileHeld::new(&to, &writer);
    let running = f.busy(meanwhile.meanwhile(&f));
    assert_eq!(
        running.dispatch_detached().unwrap(),
        2,
        "the running dispatcher makes the pass it was asked for"
    );
    assert_eq!(
        meanwhile.waited(),
        0,
        "the post's dispatcher waited for that pass and made none"
    );
    assert_eq!(
        ids(&to.listener.drain(&f, &writer, &to.handle)),
        BTreeMap::from([(meanwhile.posted(), 1)])
    );
    assert_eq!(lease(&f), json!({"until": 0, "asked": 1, "served": 1}));
}

#[test]
fn a_dispatcher_whose_lease_lapsed_leaves_the_next_holder_its_lease_and_its_asks() {
    let f = Arc::new(Fixture::empty());
    let to = recipient(&f, 0);
    let writer = f.session("worker");
    let before = to.listener.drain(&f, &writer, &to.handle);
    assert!(before.is_empty(), "{before:?}");
    let meanwhile = WhileHeld::new(&to, &writer);
    let stalled = f.busy({
        let (board, meanwhile, once) = (
            Arc::clone(&f),
            Arc::clone(&meanwhile),
            AtomicBool::new(false),
        );
        // The first dispatcher stalls past its lease: the next takes the host, and while it runs a
        // write's dispatcher asks it for one more pass.
        move || {
            if held(&board) && !once.swap(true, Ordering::SeqCst) {
                board.advance(61.0);
                let next = board.busy(meanwhile.meanwhile(&board));
                assert_eq!(next.dispatch_detached().unwrap(), 2);
            }
        }
    });
    assert_eq!(
        stalled.dispatch_detached().unwrap(),
        1,
        "a holder whose lease lapsed ends after its own pass, neither answering the ask nor releasing"
    );
    assert_eq!(meanwhile.waited(), 0);
    assert_eq!(lease(&f), json!({"until": 0, "asked": 1, "served": 1}));
    assert_eq!(
        ids(&to.listener.drain(&f, &writer, &to.handle)),
        BTreeMap::from([(meanwhile.posted(), 1)])
    );
}

#[test]
fn a_dispatcher_waiting_on_a_dead_holder_takes_the_host_over_once_its_lease_lapses() {
    let f = Arc::new(Fixture::empty());
    let mut recipients: Vec<Recipient> = (0..2).map(|n| recipient(&f, n)).collect();
    recipients.sort_by(|a, b| a.handle.cmp(&b.handle));
    let (first, last) = (&recipients[0], &recipients[1]);
    let writer = f.session("worker");
    let writing = deferring(&f);
    let delivered_first = writing
        .say(
            &first.slug,
            &writer,
            "before the crash",
            Some(&first.handle),
        )
        .unwrap()
        .id;
    let waiting = writing
        .say(
            &last.slug,
            &writer,
            "before the crash, waiting",
            Some(&last.handle),
        )
        .unwrap()
        .id;
    // A dispatcher whose process is lost after its first delivery: its unwind leaves its lease as
    // a killed process would, held with nobody behind it.
    let lost = AtomicBool::new(false);
    let crashed = f.busy({
        let board = Arc::clone(&f);
        let (to_first, to_last) = (first.handle.clone(), last.handle.clone());
        move || {
            if delivered(&board, delivered_first, &to_first)
                && !delivered(&board, waiting, &to_last)
                && !lost.swap(true, Ordering::SeqCst)
            {
                panic!("the dispatcher's process is lost");
            }
        }
    });
    let ended =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crashed.dispatch_detached()));
    assert!(ended.is_err(), "the dispatcher was lost mid-pass");
    assert!(held(&f), "its lease outlives it: {}", lease(&f));
    let after = writing
        .say(&last.slug, &writer, "after the crash", Some(&last.handle))
        .unwrap()
        .id;
    let dispatcher = f.other();
    let next = std::thread::spawn(move || dispatcher.dispatch_detached().unwrap());
    // The write's dispatcher asks the dead holder, which never answers; once its lease lapses, the
    // waiting dispatcher takes the host over and delivers what the dead one left.
    until(|| lease(&f)["asked"] == json!(1));
    f.advance(61.0);
    let passes = next.join().unwrap();
    assert!(
        passes >= 1,
        "the waiting dispatcher made the passes, not {passes}"
    );
    assert_eq!(
        ids(&last.listener.drain(&f, &writer, &last.handle)),
        BTreeMap::from([(waiting, 1), (after, 1)])
    );
    assert_eq!(
        ids(&first.listener.drain(&f, &writer, &first.handle)),
        BTreeMap::from([(delivered_first, 1)])
    );
    assert_eq!(lease(&f)["until"], json!(0));
}

#[test]
fn a_dispatcher_on_another_host_delivers_there_while_this_hosts_is_busy() {
    let f = Fixture::empty();
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
        &format!("{}-remote", f.store.client().database()),
    );
    // This host's dispatcher is busy for as long as the test runs.
    f.store
        .client()
        .query(
            "UPSERT type::record('callback_lease', ['dispatch', $host]) SET until = $until, revision = 1, asked = 0, served = 0 RETURN NONE;",
            json!({"host": HOST, "until": f.now() + 3600.0}),
        )
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

#[test]
fn many_writers_at_once_each_message_reaches_its_recipient_exactly_once() {
    const RECIPIENTS: usize = 4;
    const WRITERS: usize = 16;
    let f = Fixture::empty();
    let recipients: Vec<Recipient> = (0..RECIPIENTS).map(|n| recipient(&f, n)).collect();
    let writers: Vec<String> = (0..WRITERS).map(|_| f.session("worker")).collect();
    for to in &recipients {
        let before = to.listener.drain(&f, &writers[0], &to.handle);
        assert!(before.is_empty(), "{before:?}");
    }
    let asked = Arc::new(AtomicUsize::new(0));
    let start = Arc::new(Barrier::new(WRITERS));
    let threads: Vec<_> = writers
        .iter()
        .enumerate()
        .map(|(n, writer)| {
            let to = &recipients[n % RECIPIENTS];
            let (to, slug) = (to.handle.clone(), to.slug.clone());
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
    for to in &recipients {
        let got = ids(&to.listener.drain(&f, &writers[0], &to.handle));
        let want: BTreeMap<i64, usize> = sent[&to.handle].iter().map(|id| (*id, 1)).collect();
        assert_eq!(
            got, want,
            "{}: each of its messages once, no other's",
            to.handle
        );
    }
    assert_eq!(lease(&f)["until"], json!(0));
}
