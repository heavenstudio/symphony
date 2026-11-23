//! What one pass of the server's clock sends the board does not grow with its recipients: it reads
//! every recipient's stored state, the lines due for each and the pane lines queued for each in one
//! read apiece, as a board with dozens of registered channels makes every pass.
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
