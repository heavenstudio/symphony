//! What `cleanup` never closes, its work finished or not: an orchestrator's or a chat's terminal,
//! the calling terminal (`HC_TERMINAL_ID`), and the terminal of the session it runs for.

use crate::World;
use serde_json::json;
use symphony_board::launch::cleanup_as;

impl World {
    /// Finish without claiming: the session keeps its reservation as its only role.
    pub fn finish_reserved_task(&self, handle: &str, status: &str) {
        if status == "cancelled" {
            self.store().move_to("sim/jump", status, handle).unwrap();
        } else {
            self.store()
                .move_to("sim/jump", "in_progress", handle)
                .unwrap();
            self.store().move_to("sim/jump", "review", handle).unwrap();
            let reviewer = self.f.session("reviewer");
            self.store()
                .take_role("sim/jump", &reviewer, "reviewer")
                .unwrap();
            self.store()
                .verdict("sim/jump", &reviewer, "approve", "ok")
                .unwrap();
        }
    }
}

#[test]
fn cleanup_keeps_a_live_reservation_and_closes_after_it_expires_or_its_task_finishes() {
    for end in ["expired", "done", "cancelled"] {
        let w = World::new();
        let (handle, terminal) = w.worker("sim/jump", None);
        let task = w.store().get("sim/jump").unwrap();
        assert_eq!(task.status, "ready");
        assert_eq!(task.roles.len(), 1, "the reservation is its only hold");
        assert_eq!(task.holders("reserved"), [handle.as_str()]);
        let before = w.changes();
        let [kept] = <[_; 1]>::try_from(w.clean(true, &[])).unwrap();
        assert_eq!(kept.pane, terminal, "{end}");
        assert_eq!(kept.handle.as_deref(), Some(handle.as_str()));
        assert!(kept.kept);
        assert!(!kept.closed);
        assert_eq!(kept.why, "it holds a live reservation on sim/jump");
        assert_eq!(w.changes(), before, "the dry run changes no terminal");
        assert_eq!(w.clean(false, &[]), [], "{end}: live reservation");
        assert!(w.has_terminal(&terminal));
        assert!(w.store().sessions_of(&handle).unwrap()[0].closed.is_none());
        if end == "expired" {
            w.f.advance(3601.0);
        } else {
            w.finish_reserved_task(&handle, end);
            assert_eq!(
                w.store().get("sim/jump").unwrap().holders("reserved"),
                [handle.as_str()]
            );
        }
        let [dry] = <[_; 1]>::try_from(w.clean(true, &[])).unwrap();
        assert_eq!(dry.pane, terminal, "{end}");
        assert!(!dry.closed);
        assert!(w.has_terminal(&terminal));
        let [closed] = <[_; 1]>::try_from(w.clean(false, &[])).unwrap();
        assert_eq!(closed.pane, terminal, "{end}");
        assert!(closed.closed);
        assert!(!w.has_terminal(&terminal));
        assert!(w.store().sessions_of(&handle).unwrap()[0].closed.is_some());
    }
}

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
