//! The tick's wait for the idle sweep after a sleep, on clocks the tests set: no real wait.

use super::presence::{AT_MOST, Judging, Presence};
use std::time::{Duration, Instant};

/// The wall clock at the first round, in seconds since the epoch.
const WALL: f64 = 1_800_000_000.0;

/// Two clocks the test moves: the wall clock, and the monotonic one, which stops in a sleep.
struct Clocks {
    wall: f64,
    mono: Instant,
}

impl Clocks {
    fn new() -> Clocks {
        Clocks {
            wall: WALL,
            mono: Instant::now(),
        }
    }

    /// Awake for `seconds`: both clocks move.
    fn awake(&mut self, seconds: f64) {
        self.wall += seconds;
        self.mono += Duration::from_secs_f64(seconds);
    }

    /// Asleep for `seconds`: the wall clock alone moves.
    fn asleep(&mut self, seconds: f64) {
        self.wall += seconds;
    }

    fn round(&mut self, presence: &mut Presence) {
        presence.began(self.wall, self.mono);
        self.awake(1.0);
        presence.ended();
    }

    fn judges(&self, presence: &mut Presence) -> Judging {
        presence.judges(self.wall, self.mono)
    }
}

#[test]
fn before_any_round_and_while_awake_the_tick_judges() {
    let (mut clocks, mut presence) = (Clocks::new(), Presence::default());
    // A server that runs no sweep never waits, however long it slept.
    clocks.asleep(3600.0);
    assert_eq!(clocks.judges(&mut presence), Judging::Now);
    clocks.round(&mut presence);
    for _ in 0..12 {
        clocks.awake(299.0);
        assert_eq!(clocks.judges(&mut presence), Judging::Now);
        clocks.round(&mut presence);
    }
    // Awake past the next round's time, a late sweep is not a sleep: the tick judges.
    clocks.awake(900.0);
    assert_eq!(clocks.judges(&mut presence), Judging::Now);
}

#[test]
fn after_a_sleep_the_tick_waits_for_a_round_begun_after_the_wake() {
    let (mut clocks, mut presence) = (Clocks::new(), Presence::default());
    clocks.round(&mut presence);
    clocks.awake(60.0);
    // A sleep of the slack or less is no sleep.
    clocks.asleep(30.0);
    assert_eq!(clocks.judges(&mut presence), Judging::Now);
    clocks.asleep(1.0);
    assert_eq!(clocks.judges(&mut presence), Judging::Waits { began: true });
    clocks.awake(5.0);
    assert_eq!(
        clocks.judges(&mut presence),
        Judging::Waits { began: false }
    );
    // A round under way is not one ended: the tick still waits.
    presence.began(clocks.wall, clocks.mono);
    clocks.awake(1.0);
    assert_eq!(
        clocks.judges(&mut presence),
        Judging::Waits { began: false }
    );
    presence.ended();
    assert_eq!(clocks.judges(&mut presence), Judging::Now);
    // The next sleep waits again, from its own first tick.
    clocks.asleep(1200.0);
    assert_eq!(clocks.judges(&mut presence), Judging::Waits { began: true });
    clocks.round(&mut presence);
    assert_eq!(clocks.judges(&mut presence), Judging::Now);
}

#[test]
fn a_round_begun_before_the_sleep_and_ended_after_it_settles_nothing() {
    let (mut clocks, mut presence) = (Clocks::new(), Presence::default());
    clocks.round(&mut presence);
    clocks.awake(300.0);
    presence.began(clocks.wall, clocks.mono);
    // The machine slept under the round, which ends, failed, at the wake.
    clocks.asleep(1044.0);
    clocks.awake(17.0);
    presence.ended();
    assert_eq!(clocks.judges(&mut presence), Judging::Waits { began: true });
    clocks.round(&mut presence);
    assert_eq!(clocks.judges(&mut presence), Judging::Now);
}

#[test]
fn a_wait_ends_after_its_awake_bound_and_says_so_once() {
    let (mut clocks, mut presence) = (Clocks::new(), Presence::default());
    clocks.round(&mut presence);
    clocks.asleep(1800.0);
    assert_eq!(clocks.judges(&mut presence), Judging::Waits { began: true });
    // More sleep within the wait counts for nothing: only awake time bounds it.
    clocks.asleep(7200.0);
    clocks.awake(AT_MOST.as_secs_f64() - 1.0);
    assert_eq!(
        clocks.judges(&mut presence),
        Judging::Waits { began: false }
    );
    clocks.awake(1.0);
    assert_eq!(
        clocks.judges(&mut presence),
        Judging::Anyway { first: true }
    );
    clocks.awake(1.0);
    assert_eq!(
        clocks.judges(&mut presence),
        Judging::Anyway { first: false }
    );
    // A round at last: the tick judges, and the next sleep's wait starts over.
    clocks.round(&mut presence);
    assert_eq!(clocks.judges(&mut presence), Judging::Now);
    clocks.asleep(1800.0);
    assert_eq!(clocks.judges(&mut presence), Judging::Waits { began: true });
}

#[test]
fn a_wall_clock_set_back_never_waits_and_an_end_without_a_beginning_changes_nothing() {
    let (mut clocks, mut presence) = (Clocks::new(), Presence::default());
    presence.ended();
    clocks.asleep(3600.0);
    assert_eq!(clocks.judges(&mut presence), Judging::Now);
    clocks.round(&mut presence);
    clocks.wall -= 7200.0;
    assert_eq!(clocks.judges(&mut presence), Judging::Now);
}

/// The hub's tick across a night of sleeps, on a scratch board and the real presence sweep, its
/// rounds begun and ended as the maintenance worker tells the hub ([`crate::front`]).
mod tick {
    use super::super::Hub;
    use super::super::retry_tests::Listener;
    use crate::testing::board::{Fixture, NOW, Seed, session, sweep_presence};
    use crate::testing::{Tree, repository};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// Longer than the lost line, as a night's DarkWakes are apart.
    const SLEEP: f64 = 1800.0;

    /// A hub whose two clocks the test moves, an orchestrator live throughout, and its child
    /// executing `tools/child` with a listener of its own.
    struct Night {
        hub: Hub,
        wall: Arc<Mutex<f64>>,
        mono: Arc<Mutex<Instant>>,
        f: Fixture,
        parent: Listener,
        child: Option<Listener>,
        barriers: usize,
        _tree: Tree,
    }

    impl Night {
        fn new() -> Night {
            let tree = Tree::new();
            let root = repository(&tree);
            let mut hub = Hub::new(&root, &root.join("build/client"));
            let wall = Arc::new(Mutex::new(NOW));
            let mono = Arc::new(Mutex::new(Instant::now()));
            let (read_wall, read_mono) = (wall.clone(), mono.clone());
            hub.clock = Box::new(move || *read_wall.lock().unwrap());
            hub.retry_clock = Box::new(move || *read_mono.lock().unwrap());
            let mut parent = session("parent");
            parent.kind = "orchestrator".into();
            parent.busy_until = Some(NOW + 1_000_000.0);
            let mut child = session("child");
            child.parent = Some("parent".into());
            let f = Fixture::new(
                Seed::default()
                    .session("sender")
                    .session_row(parent)
                    .session_row(child)
                    .task("tools/child", "in_progress")
                    .role("tools/child", "child", "executor")
                    .role("tools/child", "parent", "orchestrator"),
            );
            let listener = |name: &str, handle: &str| {
                let listener = Listener::named(&f, name);
                let address = format!("uds:{}", listener.path.display());
                f.store
                    .callback(handle, Some(&address), false, false)
                    .unwrap();
                listener
            };
            let (parent, child) = (listener("", "parent"), listener("-child", "child"));
            parent.through("PULL executing=1 ");
            Night {
                hub,
                wall,
                mono,
                f,
                parent,
                child: Some(child),
                barriers: 0,
                _tree: tree,
            }
        }

        /// Asleep: the board's clock and the hub's wall clock go on, its monotonic one does not.
        fn sleep(&self) {
            self.f.advance(SLEEP);
            *self.wall.lock().unwrap() += SLEEP;
        }

        /// Awake for a second: every clock goes on.
        fn second(&self) {
            self.f.advance(1.0);
            *self.wall.lock().unwrap() += 1.0;
            *self.mono.lock().unwrap() += Duration::from_secs(1);
        }

        /// A round of the idle sweep as the maintenance worker runs one: begun, swept, ended.
        fn round(&self) {
            self.hub.sweep_began();
            let state = std::env::temp_dir().join(format!(
                "presence-night-{}-{}.json",
                std::process::id(),
                self.f.store.database().database()
            ));
            sweep_presence(&self.f.store, state);
            self.second();
            self.hub.sweep_ended();
        }

        /// One tick of the hub that judges nothing: the orchestrator's record of its child stands as
        /// the last judged tick left it. No barrier is sent here, as a write judges for itself;
        /// whatever such a tick said comes through the next [`Night::tick`]'s.
        fn held(&self) {
            self.hub.board_read(&self.f.store).unwrap();
            let state = self
                .f
                .store
                .raw()
                .row("callback_state", "parent")
                .get()
                .unwrap();
            assert_eq!(state["seen"]["children"]["child"], "Live", "{state}");
        }

        /// One tick of the hub, then every line it told the orchestrator.
        fn tick(&mut self) -> Vec<String> {
            self.hub.board_read(&self.f.store).unwrap();
            self.barriers += 1;
            let barrier = format!("barrier-{}", self.barriers);
            assert!(
                self.f
                    .store
                    .send_agent("sender", "parent", &barrier)
                    .unwrap()
            );
            (self.parent.through(&barrier).iter())
                .flat_map(|text| text.lines().map(str::to_string).collect::<Vec<_>>())
                .filter(|line| !line.contains(&barrier))
                .collect()
        }
    }

    /// What the orchestrator hears once its child is lost holding `tools/child`.
    const LOST: [&str; 3] = [
        "LOST child — its work is claimable: tools/child",
        "PULL executing=0 (was 1) in_progress_orphaned=1 (was 0)",
        "in_progress_orphaned p2 tools/child — Task tools/child",
    ];

    /// What the orchestrator hears once its child, with no terminal and no tool call since it
    /// connected, rests past the live window while its listener answers: idle, never LOST.
    const IDLE: [&str; 2] = ["PULL idle=1 (was 0)", "idle child"];

    /// [`LOST`] of a child the orchestrator was told is idle: the loss takes that row back.
    const LOST_FROM_IDLE: [&str; 4] = [
        LOST[0],
        "PULL executing=0 (was 1) in_progress_orphaned=1 (was 0) idle=0 (was 1)",
        LOST[2],
        "resolved idle child",
    ];

    #[test]
    fn after_each_sleep_the_tick_waits_for_a_round_and_a_listener_that_answers_is_never_lost() {
        let mut night = Night::new();
        night.round();
        assert_eq!(night.tick(), Vec::<String>::new());
        for wake in 0..4 {
            night.sleep();
            // The wake's first ticks judge nothing: the child's listener has not been looked at.
            night.held();
            night.second();
            night.held();
            night.round();
            // Its listener answered the round: neither STALE nor LOST is said, wake after wake.
            // At rest since it connected, it is idle, said at the first wake alone.
            let said: &[&str] = if wake == 0 { &IDLE } else { &[] };
            assert_eq!(night.tick(), said, "wake {wake}");
        }
        // Its listener gone, the round after the wake retires it: the loss is said, once, and
        // takes back the idle row the first wake said.
        night.child = None;
        night.sleep();
        night.held();
        night.round();
        assert_eq!(night.tick(), LOST_FROM_IDLE);
        night.sleep();
        night.round();
        assert_eq!(night.tick(), Vec::<String>::new());
    }

    #[test]
    fn a_tick_waits_no_longer_than_its_bound_of_awake_time_for_a_round() {
        let mut night = Night::new();
        night.round();
        assert_eq!(night.tick(), Vec::<String>::new());
        night.sleep();
        night.held();
        // No round comes: past the bound the tick judges as it did before, on the board as it is.
        let bound = super::AT_MOST.as_secs();
        night.f.advance(bound as f64);
        *night.wall.lock().unwrap() += bound as f64;
        *night.mono.lock().unwrap() += Duration::from_secs(bound);
        assert_eq!(night.tick(), LOST);
    }
}
