//! What one pass of the server's clock sends the board does not grow with its recipients: it reads
//! every recipient's stored state, the lines due for each and the pane lines queued for each in one
//! read apiece, as a board with dozens of registered channels makes every pass. A pass after a post
//! visits each recipient under its lease, and reads no inbox the post did not reach.
use super::common::{Backend, Fixture, Seed};
use super::quiet_callbacks::listen;

on_both! {
fn a_pass_sends_as_many_requests_for_six_recipients_as_for_two(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let beating = f.session("worker");
    let mut listeners = Vec::new();
    let mut register = |count: usize| {
        for _ in 0..count {
            let handle = f.session("worker");
            listeners.push(listen(&f, &handle));
        }
    };
    let pass = || {
        let before = f.store.database().requests();
        f.store.dispatch_callbacks_snapshot().unwrap();
        f.store.database().requests() - before
    };
    // A quiet pass, and one after a heartbeat elsewhere, which moves the board's marker.
    let measure = |pass: &dyn Fn() -> usize| {
        pass();
        pass();
        let quiet = pass();
        f.advance(1.0);
        f.elsewhere().touch(&beating).unwrap();
        (quiet, pass())
    };
    register(2);
    let two = measure(&pass);
    register(4);
    let six = measure(&pass);
    assert_eq!(six, two, "requests of a quiet pass and of a pass after a heartbeat");
    assert!(two.0 < two.1, "the heartbeat's pass reads the board and the stored states: {two:?}");
    drop(listeners);
}
}

/// What a pass after a post sends the board for each recipient the post does not reach: its visit
/// under its lease takes the lease, reads the marker, its stored state and its outbox's due line,
/// writes its account and releases the lease. It reads no inbox: the pass's message pool says none
/// since the recipient's cursor could reach it ([`Store::could_hear`]), and the read would make it
/// seven.
const LEASED_VISIT: usize = 6;

on_both! {
fn a_pass_after_a_post_to_someone_else_reads_no_recipients_inbox(backend: Backend) {
    let f = Fixture::on(backend, Seed::default());
    let writer = f.session("worker");
    let addressee = f.session("worker");
    f.task("sim/a");
    let mut listeners = Vec::new();
    let mut register = |count: usize| {
        for _ in 0..count {
            let handle = f.session("worker");
            listeners.push(listen(&f, &handle));
        }
    };
    // The requests of a pass after a post to a session with no channel, once the passes before it
    // left every recipient settled. The post moves every recipient's look, so each one's visit
    // goes under its lease.
    let after_a_post = |text: &str| {
        f.store.dispatch_callbacks_snapshot().unwrap();
        f.store.dispatch_callbacks_snapshot().unwrap();
        f.advance(1.0);
        (f.elsewhere().say("sim/a", &writer, text, Some(&addressee))).unwrap();
        let before = f.store.database().requests();
        f.store.dispatch_callbacks_snapshot().unwrap();
        f.store.database().requests() - before
    };
    register(2);
    let two = after_a_post("for two");
    register(4);
    let six = after_a_post("for six");
    assert_eq!(
        six - two,
        4 * LEASED_VISIT,
        "requests of a pass after a post, for two recipients ({two}) and for six ({six})"
    );
    drop(listeners);
}
}
