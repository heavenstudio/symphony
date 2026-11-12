//! A build slower than the delivery's deadline. A look or a write whose PULL is due builds it
//! under the recipient's lease. However long that takes by the board's clock, what it built is
//! delivered once, and the next look has nothing left to build. A build whose lease lapsed and was
//! taken over in the meantime sends nothing and writes nothing: the recipient now belongs to the
//! dispatcher that took the lease.

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
    let until = f.store.client().query(
        "RETURN type::record('callback_lease', $handle).until ?? 0;",
        json!({"handle": handle}),
    );
    until.unwrap().remove(0).as_f64().unwrap_or(0.0) > f.now()
}

/// Another store on `f`'s board whose dispatch to `handle` takes `seconds` of the board's clock to
/// build, and during which `meanwhile` acts once. The time passes once the dispatch holds the
/// recipient's lease and has read the board and the recipient's record. Also returned: whether
/// that point was reached.
fn building(
    f: &Arc<Fixture>,
    handle: &str,
    seconds: f64,
    meanwhile: impl Fn(&Fixture) + Send + Sync + 'static,
) -> (Store, Arc<AtomicBool>) {
    let (board, handle) = (Arc::clone(f), handle.to_owned());
    let built = Arc::new(AtomicBool::new(false));
    let once = Arc::clone(&built);
    let store = f.busy(move || {
        if once.load(Ordering::SeqCst) || !leased(&board, &handle) {
            return;
        }
        let trace = std::backtrace::Backtrace::force_capture().to_string();
        if trace.contains("dispatch_one")
            && !trace.contains("callback_board")
            && !once.swap(true, Ordering::SeqCst)
        {
            board.advance(seconds);
            meanwhile(&board);
        }
    });
    (store, built)
}

#[test]
fn a_look_whose_build_outlasts_the_deadline_says_it_once_and_the_next_look_builds_nothing() {
    let f = Arc::new(Fixture::empty());
    let reader = f.session("orchestrator");
    for slug in ["tools/one", "tools/two"] {
        f.task(slug);
    }
    // Its first look's whole digest takes 45 s to build, past the deadline but within the lease.
    let (slow, built) = building(&f, &reader, DEADLINE + 15.0, |_| {});
    let first = look(&slow, &reader);
    assert!(built.load(Ordering::SeqCst), "the build took its time");
    assert!(whole(&first), "what was built is said: {first:?}");
    // It was said, so nothing waits to be built again: the next look only reads. (Its clock has
    // done its one jump and reads as the fixture's from here on.)
    let (requests, writes) = sent(&f, || {
        let next = look(&slow, &reader);
        assert!(next.is_empty(), "said once: {next:?}");
    });
    assert_eq!(writes, 0, "{requests} requests");
    assert!(requests <= LOOK_REQUESTS, "{requests} requests");
}

#[test]
fn a_look_whose_lease_was_taken_while_it_built_says_nothing_and_writes_nothing() {
    let f = Arc::new(Fixture::empty());
    let reader = f.session("orchestrator");
    f.task("tools/one");
    // Its build outlasts the lease. Meanwhile another look takes the lapsed lease over, and
    // says the whole digest.
    let heard = Arc::new(Mutex::new(Vec::new()));
    let (other, waiting) = (Arc::clone(&heard), reader.clone());
    let (slow, built) = building(&f, &reader, LEASE + 1.0, move |f| {
        other.lock().unwrap().extend(look(&f.store, &waiting));
    });
    let late = look(&slow, &reader);
    assert!(built.load(Ordering::SeqCst), "the build took its time");
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

/// The keys of what waits undelivered for `handle`.
fn undelivered(f: &Fixture, handle: &str) -> Value {
    let keys = f.store.client().query(
        "RETURN SELECT VALUE key FROM callback_event WHERE handle = $handle AND delivered != true;",
        json!({"handle": handle}),
    );
    keys.unwrap().remove(0)
}

#[test]
fn a_write_whose_dispatch_outlasts_the_deadline_delivers_what_it_built() {
    let f = Arc::new(Fixture::empty());
    let sender = f.session("worker");
    let orch = f.session("orchestrator");
    let o: Listener = listen(&f, &orch);
    assert!(o.text().starts_with("PULL "));
    // A write the orchestrator hears of, whose dispatch takes 45 s to build its PULL.
    let (slow, built) = building(&f, &orch, DEADLINE + 15.0, |_| {});
    slow.create(&NewTask::new("tools/new", "Jump departure", "owner"))
        .unwrap();
    assert!(built.load(Ordering::SeqCst), "the build took its time");
    // The write's own dispatch delivered everything it queued: nothing is left for the next
    // dispatch to build again.
    assert_eq!(undelivered(&f, &orch), json!([]));
    let told = o.drain(&f, &sender, &orch);
    assert_eq!(told[0], "NEW tools/new ready — Jump departure", "{told:?}");
    assert!(told[1].starts_with("PULL "), "{told:?}");
}
