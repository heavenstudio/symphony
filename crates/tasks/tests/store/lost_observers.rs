//! A quiet child whose listener answers, across the machine's sleeps: what its orchestrator hears
//! as the clock's dispatch and the idle sweep take their turns at each wake.
//!
//! The sweep confirms a listener that answers and revives its session ([`sweep_presence`]), and the
//! dispatch says STALE, LOST or LIVENESS UNKNOWN once for each transition it reads. A machine
//! asleep runs neither, so at a wake the first to run decides. A dispatch run first reads the sleep's
//! gap as the child's silence, and the sweep after it revives the child, so every wake is a new
//! transition, said again, of a child quiet since the same call. So after a sleep the Symphony's
//! server dispatches only once a round of the sweep begun after the wake has ended
//! (`apps/symphony-server/src/live/presence.rs`), which these passes take in that order.

use super::callbacks::Listener;
use super::common::{Fixture, sweep_presence};
use super::quiet_callbacks::listen;
use crate::common::{Backend, NOW, Seed, session};
use symphony_board::Health;

/// A sleep between two wakes, longer than the lost line, as a night's DarkWakes are apart.
const SLEEP: f64 = 1800.0;

/// What its orchestrator hears once its child is lost holding `tools/child`.
const LOST: [&str; 3] = [
    "LOST child — its work is claimable: tools/child",
    "PULL executing=0 (was 1) in_progress_orphaned=1 (was 0)",
    "in_progress_orphaned p2 tools/child — Task tools/child",
];

/// What it hears when the child is lost again: a return says nothing, as a PULL with no new entry
/// is never sent (`pull_plan` in `crates/tasks/src/store/pull_account.rs`), so the counts it was
/// told stand; the entry left the digest when the child came back, so it is new once more.
const LOST_AGAIN: [&str; 3] = [
    "LOST child — its work is claimable: tools/child",
    "PULL counts unchanged",
    "in_progress_orphaned p2 tools/child — Task tools/child",
];

/// An orchestrator, live throughout, and the child executing its task with a listener of its own.
struct Night {
    f: Fixture,
    parent: Listener,
    child: Option<Listener>,
    listened: usize,
}

impl Night {
    /// The child busy in one long tool call until `busy`, when given.
    fn new(backend: Backend, busy: Option<f64>) -> Night {
        let mut parent = session("parent");
        parent.kind = "orchestrator".into();
        parent.busy_until = Some(NOW + 1_000_000.0);
        let mut child = session("child");
        child.parent = Some("parent".into());
        child.busy_until = busy;
        let f = Fixture::on(
            backend,
            Seed::default()
                .session("sender")
                .session_row(parent)
                .session_row(child)
                .task("tools/child", "in_progress")
                .role("tools/child", "child", "executor")
                .role("tools/child", "parent", "orchestrator"),
        );
        let parent = listen(&f, "parent");
        assert!(parent.text().starts_with("PULL executing=1 "));
        let mut night = Night {
            f,
            parent,
            child: None,
            listened: 0,
        };
        night.listen_child();
        night
    }

    /// The child's listener, new and registered.
    fn listen_child(&mut self) {
        self.listened += 1;
        let name = format!(
            "{}-child{}",
            self.f.store.database().database(),
            self.listened
        );
        let listener = Listener::new(&name);
        let address = format!("uds:{}", listener.path.display());
        self.f
            .store
            .callback("child", Some(&address), false, false)
            .unwrap();
        self.child = Some(listener);
    }

    /// The child's listener gone, its process with it.
    fn drop_child(&mut self) {
        self.child = None;
    }

    fn sleep(&self) {
        self.f.advance(SLEEP);
    }

    /// A round of the idle sweep: each listener that answers confirmed, each one gone retired.
    fn sweep(&self) {
        let state = std::env::temp_dir().join(format!(
            "lost-night-{}-{}.json",
            std::process::id(),
            self.f.store.database().database()
        ));
        sweep_presence(&self.f.store, state);
    }

    /// The clock's pass, then everything it said to the orchestrator.
    fn dispatch(&self) -> Vec<String> {
        self.f.store.dispatch_callbacks().unwrap();
        self.parent.drain(&self.f, "sender", "parent")
    }
}

on_both! {
fn a_quiet_child_whose_listener_answers_is_lost_at_each_wake_only_when_the_dispatch_runs_first(backend: Backend) {
    let night = Night::new(backend, None);
    night.sweep();
    assert_eq!(night.dispatch(), Vec::<String>::new());
    // Swept first at every wake, it is not quiet: nothing is said, night after night.
    for _ in 0..4 {
        night.sleep();
        night.sweep();
        assert_eq!(night.f.health("child"), Health::Live);
        assert_eq!(night.dispatch(), Vec::<String>::new());
    }
    // Dispatched first, every wake reads the sleep as its silence and says it lost again, and
    // the sweep after it says it is back: one quiet episode, a LOST at each wake.
    for wake in 0..3 {
        night.sleep();
        assert_eq!(night.dispatch(), if wake == 0 { LOST } else { LOST_AGAIN });
        // Its return is no new work: nothing is said of it.
        night.sweep();
        assert_eq!(night.dispatch(), Vec::<String>::new());
    }
}
}

on_both! {
fn a_child_whose_listener_is_gone_is_lost_once_and_again_once_back_and_gone_again(backend: Backend) {
    let mut night = Night::new(backend, None);
    night.sweep();
    assert_eq!(night.dispatch(), Vec::<String>::new());
    // Its listener gone, the round after the wake retires it, and its loss is said, once.
    night.drop_child();
    night.sleep();
    night.sweep();
    assert_eq!(night.dispatch(), LOST);
    for _ in 0..2 {
        night.sleep();
        night.sweep();
        assert_eq!(night.dispatch(), Vec::<String>::new());
    }
    // Back at work, then gone again: the second loss is said as the first was.
    night.f.store.touch("child").unwrap();
    night.listen_child();
    assert_eq!(night.dispatch(), Vec::<String>::new());
    night.drop_child();
    night.sleep();
    night.sweep();
    assert_eq!(night.dispatch(), LOST_AGAIN);
}
}

on_both! {
fn a_child_inside_one_long_tool_call_is_not_lost_until_the_call_could_have_ended(backend: Backend) {
    let mut night = Night::new(backend, Some(NOW + 2.5 * SLEEP));
    night.drop_child();
    for _ in 0..2 {
        night.sleep();
        night.sweep();
        assert_eq!(night.dispatch(), Vec::<String>::new());
    }
    night.sleep();
    night.sweep();
    assert_eq!(night.dispatch(), LOST);
}
}
