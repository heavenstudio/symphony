//! What `cleanup` never closes, its work finished or not: an orchestrator's or a chat's terminal,
//! the calling terminal (`HC_TERMINAL_ID`), and the terminal of the session it runs for.

use crate::World;
use serde_json::json;
use symphony_board::launch::cleanup_as;

#[test]
fn the_orchestrators_and_the_callers_terminal_are_never_closed() {
    let w = World::new();
    let terminal = w.add_terminal(true, Some(&w.orch.clone()), json!({}));
    assert_eq!(w.clean(false, &[]), []);
    assert!(w.has_terminal(&terminal));

    let mut w = World::new();
    let (_, terminal) = w.worker("sim/jump", Some("done"));
    w.set("HC_TERMINAL_ID", &terminal);
    assert_eq!(w.clean(false, &[]), []);
    assert!(w.has_terminal(&terminal));
}

#[test]
fn a_chats_an_orchestrators_or_the_callers_session_is_never_closed_even_with_its_work_finished() {
    for kind in ["chat", "orchestrator", "worker"] {
        let w = World::new();
        let handle = w.f.session(kind);
        w.finish_task("sim/jump", &handle, "done");
        let terminal = w.add_terminal(true, Some(&handle), json!({}));
        let m = w.machine();
        // the worker is the session cleanup runs for
        let caller = (kind == "worker").then_some(handle.as_str());
        let closures = cleanup_as(w.store(), &w.hc(), &*m.processes, false, &[], caller);
        assert_eq!(closures.unwrap(), [], "{kind}");
        assert!(w.has_terminal(&terminal), "{kind}");
    }
    // Run for no one, the same worker's terminal closes.
    let w = World::new();
    let handle = w.f.session("worker");
    w.finish_task("sim/jump", &handle, "done");
    let terminal = w.add_terminal(true, Some(&handle), json!({}));
    let [closed] = <[_; 1]>::try_from(w.clean(false, &[])).unwrap();
    assert_eq!((closed.pane, closed.closed), (terminal.clone(), true));
    assert!(!w.has_terminal(&terminal));
}
