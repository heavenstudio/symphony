//! A dispatcher that finds its host's lease held asks the holder for one more pass and waits until
//! a pass begun after its ask has ended; when the holder is lost before that pass or during it, the
//! one that asked takes the host over once the lease lapses, so no ask is left with nobody behind
//! it, and every recipient still hears each message once.

use super::common::Fixture;
use super::dispatch_later::{Recipient, deferring, handing, ids, lease, recipient};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// Whether a dispatcher holds this host's dispatch lease now.
fn held(f: &Fixture) -> bool {
    lease(f)["until"]
        .as_f64()
        .is_some_and(|until| until > f.now())
}

/// Wait until `done`, which another thread brings about within seconds, up to the backstop of the
/// 30 s it once had.
#[track_caller]
fn until(done: impl Fn() -> bool) {
    let what = format!("the condition at {}", std::panic::Location::caller());
    test_time::until_true(what, test_time::backstop(Duration::from_secs(30)), done).unwrap();
}

/// Whether `handle` acknowledged `message`, as a delivery records it.
fn delivered(f: &Fixture, message: i64, handle: &str) -> bool {
    let delivery = f.store.raw().row("delivery", json!([message, handle]));
    delivery.exists().unwrap()
}

/// Whether no dispatcher holds `handle`'s own lease, the one each delivery to it takes.
fn free(f: &Fixture, handle: &str) -> bool {
    let until = f.store.raw().row("callback_lease", handle).field("until");
    (until.unwrap().as_f64()).is_none_or(|until| until <= f.now())
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
fn a_dispatcher_that_finds_another_running_waits_for_the_pass_it_asked_for() {
    let f = Arc::new(Fixture::new(crate::common::Seed::default()));
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
    assert_eq!(
        lease(&f),
        json!({"until": 0, "asked": 1, "served": 1, "completed": 1})
    );
}

#[test]
fn a_dispatcher_whose_lease_lapsed_leaves_the_next_holder_its_lease_and_its_asks() {
    let f = Arc::new(Fixture::new(crate::common::Seed::default()));
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
    assert_eq!(
        lease(&f),
        json!({"until": 0, "asked": 1, "served": 1, "completed": 1})
    );
    assert_eq!(
        ids(&to.listener.drain(&f, &writer, &to.handle)),
        BTreeMap::from([(meanwhile.posted(), 1)])
    );
}

#[test]
fn a_dispatcher_waiting_on_a_dead_holder_takes_the_host_over_once_its_lease_lapses() {
    let f = Arc::new(Fixture::new(crate::common::Seed::default()));
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
fn a_dispatcher_whose_asked_for_pass_began_but_never_ended_makes_it_itself() {
    let f = Arc::new(Fixture::new(crate::common::Seed::default()));
    let to = recipient(&f, 0);
    let writer = f.session("worker");
    let before = to.listener.drain(&f, &writer, &to.handle);
    assert!(before.is_empty(), "{before:?}");
    let first = deferring(&f)
        .say(
            &to.slug,
            &writer,
            "in the holder's first pass",
            Some(&to.handle),
        )
        .unwrap()
        .id;
    let second = Arc::new(AtomicI64::new(0));
    let waiter: Arc<Mutex<Option<JoinHandle<usize>>>> = Arc::default();
    let (lost, waiting_when_lost) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    let holder = f.busy({
        let (board, second, waiter) = (Arc::clone(&f), Arc::clone(&second), Arc::clone(&waiter));
        let (lost, waiting_when_lost) = (Arc::clone(&lost), Arc::clone(&waiting_when_lost));
        let (handle, slug, writer) = (to.handle.clone(), to.slug.clone(), writer.clone());
        move || {
            // Once the holder's first pass delivered the first post and let its recipient go: a
            // second post to that recipient, which the pass has already visited, and the post's own
            // dispatcher, which asks the holder for one more pass.
            if second.load(Ordering::SeqCst) == 0
                && delivered(&board, first, &handle)
                && free(&board, &handle)
            {
                let id = deferring(&board)
                    .say(&slug, &writer, "for the pass asked for", Some(&handle))
                    .unwrap()
                    .id;
                second.store(id, Ordering::SeqCst);
                let dispatcher = board.other();
                *waiter.lock().unwrap() = Some(std::thread::spawn(move || {
                    dispatcher.dispatch_detached().unwrap()
                }));
                until(|| lease(&board)["asked"] == json!(1));
                return;
            }
            // The holder has begun the pass it was asked for (`served` says so) and not delivered the
            // second post yet: its process is lost there, an unwind that leaves its lease as a killed
            // process would.
            let id = second.load(Ordering::SeqCst);
            if id != 0
                && lease(&board)["served"] == json!(1)
                && !delivered(&board, id, &handle)
                && !lost.swap(true, Ordering::SeqCst)
            {
                let waiting =
                    (waiter.lock().unwrap().as_ref()).is_some_and(|waiter| !waiter.is_finished());
                waiting_when_lost.store(waiting, Ordering::SeqCst);
                panic!("the holder's process is lost before the pass it was asked for delivers");
            }
        }
    });
    let ended =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| holder.dispatch_detached()));
    assert!(
        ended.is_err() && lost.load(Ordering::SeqCst),
        "the holder was lost in that pass"
    );
    assert!(
        waiting_when_lost.load(Ordering::SeqCst),
        "the dispatcher that asked still waited when the pass it asked for had only begun"
    );
    // Nothing but the dispatcher that asked is left to deliver the second post: once the lost
    // holder's lease lapses it takes the host over, with no dispatch from here.
    f.advance(61.0);
    let waiter = waiter.lock().unwrap().take();
    let passes = waiter
        .expect("the post's dispatcher started")
        .join()
        .unwrap();
    assert!(
        passes >= 1,
        "the dispatcher that asked made the pass, not {passes}"
    );
    assert_eq!(
        ids(&to.listener.drain(&f, &writer, &to.handle)),
        BTreeMap::from([(first, 1), (second.load(Ordering::SeqCst), 1)])
    );
    assert_eq!(
        lease(&f),
        json!({"until": 0, "asked": 1, "served": 1, "completed": 1})
    );
}

#[test]
fn a_failed_dispatch_pass_releases_its_host_lease_for_the_next_pass() {
    let f = Fixture::new(crate::common::Seed::default());
    let handle = f.session("worker");
    f.store
        .raw()
        .row("callback_state", handle.as_str())
        .put(json!({
            "handle":handle, "address":"codex:fixture", "host":crate::common::HOST,
            "registration_revision":"not a number",
        }))
        .unwrap();
    let error = f.other().dispatch_detached().unwrap_err();
    assert!(error.to_string().contains("invalid type"), "{error}");
    let lease_row = || {
        f.store
            .raw()
            .row("callback_lease", json!(["dispatch", crate::common::HOST]))
            .get()
            .unwrap()
    };
    let released = lease_row();
    assert_eq!(released["until"], 0);
    assert_eq!(released["revision"], 1);
    f.store
        .raw()
        .row("callback_state", handle.as_str())
        .delete()
        .unwrap();
    assert_eq!(f.other().dispatch_detached().unwrap(), 1);
    assert_eq!(lease_row()["revision"], 2);
    assert!(!held(&f));
}
