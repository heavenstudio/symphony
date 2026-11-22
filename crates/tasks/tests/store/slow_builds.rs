//! A build slower than the delivery's deadline. A look or a write whose PULL is due builds it
//! under the recipient's lease. However long that takes by the board's clock, what it built is
//! delivered once, by a dispatcher still holding the lease when it sends, and the next look has
//! nothing left to build. Suppose the lease lapses and another dispatcher takes it. If that
//! happens during the build, the build writes nothing and sends nothing. If it happens after the
//! build recorded what it built but before delivery began, nothing is sent. Either way the
//! recipient now belongs to the dispatcher that took the lease. And a delivery stops at its
//! deadline: a batch whose turn comes 30 s or more after the delivery renewed the lease is not
//! begun, but waits for the next look, untried.
use crate::common::Backend;

use super::callbacks::Listener;
use super::common::Fixture;
use super::pushes::{LOOK_REQUESTS, sent, whole};
use super::quiet_callbacks::listen;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use symphony_board::cli::Env;
use symphony_board::{NewTask, Store};

/// How long a delivery may go on once it renewed the recipient's lease, by the board's clock
/// (`DELIVERY_SECONDS` in `store/callbacks.rs`).
const DEADLINE: f64 = 30.0;

/// How long a recipient's lease lasts (`CALLBACK_LEASE_SECONDS`).
const LEASE: f64 = 60.0;

/// The builds a slow dispatch with nobody taking its recipient over is held to: past the deadline
/// but within the lease, and past the whole lease.
const SLOW: [f64; 2] = [DEADLINE + 15.0, LEASE + 1.0];

/// What one look of `tasks wait` by `handle` on `store` printed, line by line.
fn look(store: &Store, handle: &str) -> Vec<String> {
    let mut said = Vec::new();
    let mut print = |text: &str| {
        said.extend(text.lines().map(str::to_owned));
        Ok(())
    };
    (store.collect_callbacks(handle, &Env::of_process(), &mut print)).unwrap();
    said
}

/// Whether a dispatcher holds `handle`'s lease now, by the board's clock.
fn leased(f: &Fixture, handle: &str) -> bool {
    let until = f.store.raw().row("callback_lease", handle).field("until");
    until.unwrap().as_f64().unwrap_or(0.0) > f.now()
}

/// Where in a leased dispatch the time passes:
/// - once it has read the board and the recipient's record (its first read of the clock in
///   `dispatch_leased`, which `dispatch_one` calls under the lease, past the board's read);
/// - once it has recorded what it built and is about to deliver it (`deliver_pending`'s first);
/// - once it has renewed the lease and its first batch's turn comes (the deadline's check in
///   `deliver_batches`).
const BUILDING: &str = "dispatch_leased";
const DELIVERING: &str = "deliver_pending";
const SENDING: &str = "deliver_batches";

/// A store on the board whose dispatch to one recipient takes its time, and what it was seen to do.
struct Slow {
    store: Store,
    /// Whether the build's time passed.
    built: Arc<AtomicBool>,
    /// Whether, once the time passed, it still held the recipient's lease by the board's clock when
    /// it recorded its first send; none while it recorded none.
    held_when_sent: Arc<Mutex<Option<bool>>>,
}

impl Slow {
    fn built(&self) -> bool {
        self.built.load(Ordering::SeqCst)
    }

    fn held_when_sent(&self) -> Option<bool> {
        *self.held_when_sent.lock().unwrap()
    }
}

/// Another store on `f`'s board whose dispatch to `handle` takes `seconds` of the board's clock,
/// during which `meanwhile` acts once. The time passes at the first read of the clock from the
/// frame `at` while the dispatch holds the recipient's lease.
fn building(
    f: &Arc<Fixture>,
    handle: &str,
    at: &'static str,
    seconds: f64,
    meanwhile: impl Fn(&Fixture) + Send + Sync + 'static,
) -> Slow {
    let (board, handle) = (Arc::clone(f), handle.to_owned());
    let (built, held_when_sent) = (Arc::new(AtomicBool::new(false)), Arc::new(Mutex::new(None)));
    let (once, sending) = (Arc::clone(&built), Arc::clone(&held_when_sent));
    let store = f.busy(move || {
        if !once.load(Ordering::SeqCst) {
            if !leased(&board, &handle) {
                return;
            }
            let trace = std::backtrace::Backtrace::force_capture().to_string();
            if trace.contains(at)
                && !trace.contains("callback_board")
                && !once.swap(true, Ordering::SeqCst)
            {
                board.advance(seconds);
                meanwhile(&board);
            }
            return;
        }
        let mut sending = sending.lock().unwrap();
        if sending.is_none()
            && (std::backtrace::Backtrace::force_capture().to_string())
                .contains("record_callback_delivery")
        {
            *sending = Some(leased(&board, &handle));
        }
    });
    Slow {
        store,
        built,
        held_when_sent,
    }
}

on_both! {
fn a_look_whose_build_outlasts_the_deadline_says_it_once_under_its_lease_and_the_next_builds_nothing(backend: Backend)
 {
    for seconds in SLOW {
        let f = Arc::new(Fixture::on(backend, crate::common::Seed::default()));
        let reader = f.session("orchestrator");
        for slug in ["tools/one", "tools/two"] {
            f.task(slug);
        }
        // Its first look's whole digest takes this long to build.
        let slow = building(&f, &reader, BUILDING, seconds, |_| {});
        let first = look(&slow.store, &reader);
        assert!(slow.built(), "{seconds} s: the build took its time");
        assert!(
            whole(&first),
            "{seconds} s: what was built is said: {first:?}"
        );
        assert_eq!(
            slow.held_when_sent(),
            Some(true),
            "{seconds} s: nobody else could hold the reader while it was said"
        );
        // It was said, so nothing waits to be built again: the next look only reads. (Its clock
        // has made its one jump and reads as the fixture's from here on.)
        let (requests, writes) = sent(&f, || {
            let next = look(&slow.store, &reader);
            assert!(next.is_empty(), "{seconds} s: said once: {next:?}");
        });
        assert_eq!(writes, 0, "{seconds} s: {requests} requests");
        assert!(
            requests <= LOOK_REQUESTS,
            "{seconds} s: {requests} requests"
        );
    }
}
}

on_both! {
fn a_look_whose_lease_was_taken_while_it_built_says_nothing_and_writes_nothing(backend: Backend) {
    let f = Arc::new(Fixture::on(backend, crate::common::Seed::default()));
    let reader = f.session("orchestrator");
    f.task("tools/one");
    // Its build outlasts the lease. Meanwhile another look takes the lapsed lease over, and
    // says the whole digest.
    let heard = Arc::new(Mutex::new(Vec::new()));
    let (other, waiting) = (Arc::clone(&heard), reader.clone());
    let slow = building(&f, &reader, BUILDING, LEASE + 1.0, move |f| {
        other.lock().unwrap().extend(look(&f.store, &waiting));
    });
    let late = look(&slow.store, &reader);
    assert!(slow.built(), "the build took its time");
    let heard = heard.lock().unwrap().clone();
    assert!(
        whole(&heard),
        "the look that took the lease says it: {heard:?}"
    );
    assert!(late.is_empty(), "the reader hears it once: {late:?}");
    // The late look did not write its record over the record of the look that told it, and queued
    // nothing for the next look to build.
    let (requests, writes) = sent(&f, || {
        let next = look(&f.store, &reader);
        assert!(next.is_empty(), "{next:?}");
    });
    assert_eq!(writes, 0, "{requests} requests");
}
}

/// The keys of what waits undelivered for `handle`.
fn undelivered(f: &Fixture, handle: &str) -> Value {
    let waiting = (f.store.raw().table("callback_event")).matching(json!({"handle": handle}));
    let rows = waiting.fields(&["key", "delivered"]).unwrap();
    let rows = rows.as_array().unwrap().iter();
    (rows.filter(|row| row["delivered"] != json!(true)))
        .map(|row| row["key"].clone())
        .collect()
}

/// Another dispatcher takes `handle`'s lapsed lease, as a dispatcher's take does (its revision
/// moves on), and holds it until `let_go`.
fn take_lease(f: &Fixture, handle: &str) {
    let lease = f.store.raw().row("callback_lease", handle);
    let revision = lease.field("revision").unwrap().as_u64().unwrap_or(0) + 1;
    lease
        .set(json!({"until": f.now() + LEASE, "revision": revision}))
        .unwrap();
}

fn let_go(f: &Fixture, handle: &str) {
    let released = (f.store.raw().row("callback_lease", handle)).set(json!({"until": 0}));
    released.unwrap();
}

on_both! {
fn a_delivery_whose_lease_was_taken_before_it_began_sends_nothing(backend: Backend) {
    let f = Arc::new(Fixture::on(backend, crate::common::Seed::default()));
    let reader = f.session("orchestrator");
    f.task("tools/one");
    // Its build outlasts the lease. Once it has recorded what it built, and before it delivers it,
    // another dispatcher takes the lapsed lease.
    let waiting = reader.clone();
    let slow = building(&f, &reader, DELIVERING, LEASE + 1.0, move |f| {
        take_lease(f, &waiting);
    });
    let late = look(&slow.store, &reader);
    assert!(slow.built(), "the build took its time");
    assert!(late.is_empty(), "the lease is another's: {late:?}");
    assert_eq!(slow.held_when_sent(), None, "it sent nothing");
    // What it built waits for the holder, untried, and a look once the holder lets go says it.
    assert_ne!(undelivered(&f, &reader), json!([]));
    let_go(&f, &reader);
    let next = look(&f.store, &reader);
    assert!(whole(&next), "{next:?}");
    assert!(look(&f.store, &reader).is_empty(), "said once");
}
}

/// How many times each PULL waiting for `handle` was tried.
fn pull_attempts(f: &Fixture, handle: &str) -> Value {
    let waiting = (f.store.raw().table("callback_event")).matching(json!({"handle": handle}));
    let rows = waiting.fields(&["attempts", "key", "delivered"]).unwrap();
    let rows = rows.as_array().unwrap().iter();
    let pulls = rows.filter(|row| {
        let pull = (row["key"].as_str()).is_some_and(|key| key.starts_with("pull-"));
        row["delivered"] != json!(true) && pull
    });
    pulls.map(|row| row["attempts"].clone()).collect()
}

on_both! {
fn a_batch_whose_turn_comes_at_or_past_the_deadline_is_not_begun_and_waits_for_the_next_look(backend: Backend) {
    for seconds in [DEADLINE - 1.0, DEADLINE, DEADLINE + 1.0] {
        let f = Arc::new(Fixture::on(backend, crate::common::Seed::default()));
        let reader = f.session("orchestrator");
        f.task("tools/one");
        // The first batch's turn comes this long after the delivery renewed the reader's lease.
        let slow = building(&f, &reader, SENDING, seconds, |_| {});
        let first = look(&slow.store, &reader);
        assert!(slow.built(), "{seconds} s: the delivery took its time");
        if seconds < DEADLINE {
            // Before the deadline it is said, by the holder of the lease, and once.
            assert!(whole(&first), "{seconds} s: {first:?}");
            assert_eq!(slow.held_when_sent(), Some(true), "{seconds} s");
            let next = look(&f.store, &reader);
            assert!(next.is_empty(), "{seconds} s: said once: {next:?}");
            continue;
        }
        // At or past it, nothing is sent and no attempt is spent: the PULL waits whole.
        assert!(first.is_empty(), "{seconds} s: {first:?}");
        assert_eq!(slow.held_when_sent(), None, "{seconds} s: it sent nothing");
        assert_eq!(pull_attempts(&f, &reader), json!([0]), "{seconds} s");
        // The next look says it, once.
        let next = look(&f.store, &reader);
        assert!(whole(&next), "{seconds} s: {next:?}");
        let after = look(&f.store, &reader);
        assert!(after.is_empty(), "{seconds} s: said once: {after:?}");
    }
}
}

on_both! {
fn a_write_whose_dispatch_outlasts_the_deadline_delivers_what_it_built_under_its_lease(backend: Backend) {
    for seconds in SLOW {
        let f = Arc::new(Fixture::on(backend, crate::common::Seed::default()));
        let sender = f.session("worker");
        let orch = f.session("orchestrator");
        let o: Listener = listen(&f, &orch);
        assert!(o.text().starts_with("PULL "));
        // A write the orchestrator hears of, whose dispatch takes this long to build its PULL.
        let slow = building(&f, &orch, BUILDING, seconds, |_| {});
        (slow.store)
            .create(&NewTask::new("tools/new", "Jump departure", "owner"))
            .unwrap();
        assert!(slow.built(), "{seconds} s: the build took its time");
        assert_eq!(
            slow.held_when_sent(),
            Some(true),
            "{seconds} s: nobody else could hold the orchestrator while it was sent"
        );
        // The write's own dispatch delivered everything it queued: nothing is left for the next
        // dispatch to build again.
        assert_eq!(undelivered(&f, &orch), json!([]), "{seconds} s");
        let told = o.drain(&f, &sender, &orch);
        let new = "NEW tools/new ready — Jump departure";
        assert_eq!(told[0], new, "{seconds} s: {told:?}");
        assert!(told[1].starts_with("PULL "), "{seconds} s: {told:?}");
    }
}
}
