//! The monitor's look at the board each poll: what it follows, and that it makes one request of the
//! board plus one canonical holdings read however many tasks it orchestrates. Then the watch over its
//! waits, and that a poll asks hc and the test slots once, or not at all with nobody quiet. The
//! loop itself runs through the command line in `monitor.rs`.

use crate::common::{Fixture, NOW};
use crate::{env, run};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use symphony_board::cli::Clock;
use symphony_board::cli::agents::Probes;
use symphony_board::cli::attach::{Watch, observe, watch};
use symphony_board::health::{LIVE_SECONDS, STALE_SECONDS};
use symphony_board::{NewSession, NewTask};

/// An orchestrator of `n` tasks, each with a child and a grandchild.
fn orchestrate(n: usize) -> (Fixture, String) {
    let f = Fixture::empty();
    let orch = f.session("orchestrator");
    for i in 0..n {
        let top = format!("sim/t{i}");
        f.task_with(NewTask::new(&top, "T", &orch));
        f.store.take_role(&top, &orch, "orchestrator").unwrap();
        f.task_with(NewTask {
            parent: Some(top.clone()),
            ..NewTask::new(&format!("{top}/a"), "A", &orch)
        });
        f.task_with(NewTask {
            parent: Some(format!("{top}/a")),
            ..NewTask::new(&format!("{top}/a/b"), "B", &orch)
        });
    }
    (f, orch)
}

fn requests(f: &Fixture, orch: &str) -> usize {
    let before = f.store.database().requests();
    observe(&f.store, orch, "orchestrator").unwrap();
    f.store.database().requests() - before
}

#[test]
fn it_follows_every_task_below_the_ones_it_orchestrates() {
    let (f, orch) = orchestrate(3);
    let seen = observe(&f.store, &orch, "orchestrator").unwrap();
    let mut expected: Vec<String> = (0..3)
        .flat_map(|i| ["", "/a", "/a/b"].map(|tail| format!("sim/t{i}{tail}")))
        .collect();
    expected.sort();
    assert_eq!(seen.statuses.keys().cloned().collect::<Vec<_>>(), expected);
}

#[test]
fn the_requests_it_makes_do_not_grow_with_the_tasks_it_orchestrates() {
    let (small, orch) = orchestrate(2);
    let (large, big) = orchestrate(12);
    assert_eq!(
        (requests(&small, &orch), requests(&large, &big)),
        (2, 2),
        "one Board plus one shared canonical holdings read, never one per task"
    );
}

#[test]
fn what_it_sees_of_a_worker_is_its_own_tasks_and_children() {
    let f = Fixture::empty();
    let orch = f.session("orchestrator");
    let worker = f.session_with(NewSession {
        parent: Some(orch.clone()),
        ..NewSession::new("worker")
    });
    f.task_with(NewTask::new("sim/a", "A", &orch));
    f.task_with(NewTask {
        parent: Some("sim/a".into()),
        ..NewTask::new("sim/a/b", "B", &orch)
    });
    f.store.claim("sim/a", &worker).unwrap();
    let seen = observe(&f.store, &worker, "worker").unwrap();
    assert_eq!(seen.statuses.keys().collect::<Vec<_>>(), ["sim/a"]);
    assert!(seen.ready.is_empty()); // only an orchestrator hears of new ready work
    let seen = observe(&f.store, &orch, "orchestrator").unwrap();
    assert_eq!(seen.children.keys().collect::<Vec<_>>(), [&worker]);
    assert_eq!(seen.ready.keys().collect::<Vec<_>>(), ["sim/a/b"]);
}

/// A watch on `f`'s board as `me` for `polls` waits, three seconds each and a beat every one, with
/// `meanwhile(n)` run in the n-th wait: the lines it printed.
fn watched(f: &Fixture, me: &str, polls: i64, mut meanwhile: impl FnMut(usize)) -> Vec<String> {
    let settings = Watch {
        interval: 3.0,
        beat: 3.0,
        polls: Some(polls),
        probes: None,
        landed: None,
        tips: None,
        pull_every: None,
    };
    let (mut lines, mut steps) = (Vec::new(), 0);
    watch(
        &f.store,
        me,
        &settings,
        &mut |line| lines.push(line.to_string()),
        &mut |_| {
            meanwhile(steps);
            steps += 1;
        },
    )
    .unwrap();
    lines
}

#[test]
fn a_look_that_fails_is_said_and_the_watch_goes_on() {
    let f = Fixture::empty();
    let me = f.session("worker");
    let symphony_board::Database::PostgreSql(board) = f.other_database();
    let lines = watched(&f, &me, 2, |n| {
        if n == 0 {
            // From the server's maintenance database, ending the connections the board has.
            let gone = format!("DROP DATABASE {} WITH (FORCE)", board.database());
            (board.of(symphony_board::pg::MAINTENANCE))
                .simple(&gone)
                .unwrap();
        }
    });
    let heads: Vec<&str> = lines.iter().map(|l| &l[..l.find(':').unwrap()]).collect();
    assert_eq!(
        heads,
        [
            "WATCH heartbeat failed",
            "WATCH scan failed",
            "WATCH heartbeat failed",
            "WATCH scan failed"
        ],
        "{lines:?}"
    );
    assert!(lines[1].contains("does not exist"), "{lines:?}");
}

#[test]
fn a_heartbeat_refused_ends_the_watch() {
    let f = Fixture::empty();
    let me = f.session("worker");
    let lines = watched(&f, &me, 100, |_| f.store.leave(&me).unwrap());
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("WATCH heartbeat refused: "),
        "{lines:?}"
    );
}

#[test]
fn a_pinned_clock_moves_by_its_waits_and_says_each_one() {
    let now = Arc::new(Mutex::new(NOW));
    let clock = Clock::Pinned(Arc::clone(&now));
    let (mut err, mut input) = (Vec::new(), "\nleft for the next\n".as_bytes());
    clock.wait(3.0, &mut input, &mut err);
    assert_eq!(input, b"left for the next\n"); // one line read, no more
    clock.wait(0.5, &mut "".as_bytes(), &mut err); // nothing to read is no reason to stop
    assert_eq!(*now.lock().unwrap(), NOW + 3.5);
    assert_eq!(
        String::from_utf8(err).unwrap(),
        "STEP 1800000003.0\nSTEP 1800000003.5\n"
    );
}

#[test]
fn the_machines_clock_waits_for_real_and_never_for_less_than_nothing() {
    let began = Instant::now();
    Clock::Machine.wait(0.05, &mut "".as_bytes(), &mut Vec::new());
    assert!(began.elapsed() >= Duration::from_millis(50));
    assert_eq!(Clock::span(0.05), Duration::from_millis(50));
    // What it sleeps for a span that is no wait, which a loaded machine's time would not show.
    for odd in [-1.0, f64::NAN, f64::NEG_INFINITY] {
        assert_eq!(Clock::span(odd), Duration::ZERO, "{odd}");
        Clock::Machine.wait(odd, &mut "".as_bytes(), &mut Vec::new());
    }
}

#[test]
fn a_digest_interval_that_is_no_number_is_refused_before_anything_is_written() {
    let mut e = env();
    e.vars.insert("TASKS_PULL_SECONDS".into(), "soon".into());
    let ran = run(
        &e,
        &["attach", "--kind", "orchestrator", "--polls", "0"],
        "",
    );
    assert_eq!((ran.code, ran.out.as_str()), (2, ""));
    assert_eq!(
        ran.err,
        "✖ TASKS_PULL_SECONDS=soon is not a number of seconds\n"
    );
    let who = run(&e, &["who", "--json", "--all"], "");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&who.out).unwrap(),
        serde_json::json!({"sessions": []}),
    );
}

/// Probes that answer nothing and write down each time they are asked, by name.
fn counting(asked: &Arc<Mutex<Vec<&'static str>>>) -> Probes<'static> {
    let (panes, runs, listed) = (Arc::clone(asked), Arc::clone(asked), Arc::clone(asked));
    Probes {
        panes: Box::new(move || {
            panes.lock().unwrap().push("panes");
            Ok(Vec::new())
        }),
        runs: Box::new(move || {
            runs.lock().unwrap().push("runs");
            Ok(Vec::new())
        }),
        branch_of: Box::new(|_| Ok(String::new())),
        processes: Box::new(move || {
            listed.lock().unwrap().push("processes");
            Ok(symphony_board::cli::agents::ProcessScan {
                listed: Vec::new(),
                complete: true,
            })
        }),
    }
}

/// A watch as `me` for `polls` three-second waits, beating every `beat` s and asking `probes`, the
/// board's clock moving by each wait.
fn probed(f: &Fixture, me: &str, polls: i64, beat: f64, probes: &Probes) {
    let settings = Watch {
        interval: 3.0,
        beat,
        polls: Some(polls),
        probes: Some(probes),
        landed: None,
        tips: None,
        pull_every: None,
    };
    watch(&f.store, me, &settings, &mut |_| {}, &mut |s| f.advance(s)).unwrap();
}

/// Three-second polls past the lost line.
const GONE_POLLS: i64 = ((LIVE_SECONDS + STALE_SECONDS) / 3.0) as i64 + 3;

/// With five children quiet, each poll asks hc and the test slots once (one view of the agents),
/// never once a child.
#[test]
fn a_poll_asks_hc_and_the_slots_once_however_many_children_are_quiet() {
    let f = Fixture::empty();
    let orch = f.session("orchestrator");
    for i in 0..5 {
        let child = f.session_with(NewSession {
            parent: Some(orch.clone()),
            ..NewSession::new("worker")
        });
        let slug = format!("sim/c{i}");
        f.task_with(NewTask::new(&slug, "C", &orch));
        f.store.claim(&slug, &child).unwrap();
    }
    let asked = Arc::new(Mutex::new(Vec::new()));
    probed(&f, &orch, GONE_POLLS, 1000.0, &counting(&asked));
    let asked = asked.lock().unwrap();
    let count = |name: &str| asked.iter().filter(|a| **a == name).count();
    let (panes, runs) = (count("panes"), count("runs"));
    let most = usize::try_from(GONE_POLLS + 1).unwrap();
    assert!(panes > 0 && panes <= most, "{panes} asks of hc");
    assert!(runs > 0 && runs <= most, "{runs} asks of the slots");
    assert_eq!(panes, runs, "each view asks both once");
}

/// With nobody quiet a poll asks the machine nothing, neither hc nor the slots.
#[test]
fn a_poll_with_nobody_quiet_asks_nothing() {
    let f = Fixture::empty();
    let orch = f.session("orchestrator");
    let child = f.session_with(NewSession {
        parent: Some(orch.clone()),
        ..NewSession::new("worker")
    });
    f.task_with(NewTask::new("sim/a", "A", &orch));
    f.store.claim("sim/a", &child).unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    probed(&f, &orch, 3, 20.0, &counting(&asked));
    assert!(asked.lock().unwrap().is_empty(), "{asked:?}");
}
