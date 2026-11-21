//! The agents' activity as the one server records it: each health change between two
//! snapshots, at the server's time; at a start, what it finds as seen and no change; once an
//! hour, by kind and model, the sessions started, gone stale and lost and the time they spent so,
//! the sessions running work by role and model and the load beside them; the day files those
//! lines go to, kept 90 days; and, on a scratch board, the hub's part: only the one server
//! writes, and nothing is written to the board.

use super::*;
use crate::live::Hub;
use crate::testing::{
    Tree,
    board::{Fixture, NOW, Seed, session as seeded},
    repository,
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use symphony_board::rows::SessionRow;
use symphony_board::{Role, Session, Task};

/// `NOW` is a whole hour, so the hours below start at it.
const H: f64 = NOW;

fn session(handle: &str, kind: &str, model: &str, connected: f64, health: Health) -> Session {
    Session {
        row: SessionRow {
            handle: handle.into(),
            kind: kind.into(),
            model: model.into(),
            connected,
            last_seen: connected,
            ..SessionRow::default()
        },
        health,
    }
}

/// A role on a task: holder, role, since and the holder's health.
type Holding<'a> = (&'a str, &'a str, f64, Health);

fn task(slug: &str, status: &str, roles: &[Holding<'_>]) -> Task {
    Task {
        branch: None,
        id: 1,
        slug: slug.into(),
        title: slug.into(),
        body: String::new(),
        status: status.into(),
        priority: 2,
        parent: None,
        created_by: "owner".into(),
        created: 0.0,
        updated: 0.0,
        links: Vec::new(),
        depends: Vec::new(),
        roles: roles
            .iter()
            .map(|(handle, role, since, health)| Role {
                handle: (*handle).into(),
                role: (*role).into(),
                since: *since,
                expires: None,
                health: *health,
            })
            .collect(),
        instruction: false,
        fields: BTreeMap::new(),
    }
}

fn board(tasks: Vec<Task>, sessions: Vec<Session>) -> Board {
    Board {
        tasks,
        sessions,
        latest_message: 0,
    }
}

/// A scratch folder of its own, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        static COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "symphony-agents-{name}-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn changes(lines: &[Line]) -> Vec<(f64, &str, Option<&str>, Option<&str>)> {
    lines
        .iter()
        .filter_map(|line| match line {
            Line::Change(c) => Some((c.at, c.handle.as_str(), c.from.as_deref(), c.to.as_deref())),
            _ => None,
        })
        .collect()
}

fn hours(lines: &[Line]) -> Vec<&Hour> {
    lines
        .iter()
        .filter_map(|line| match line {
            Line::Hour(hour) => Some(hour),
            _ => None,
        })
        .collect()
}

/// One executor on a task in progress, its health as given.
fn executing(health: Health) -> Board {
    board(
        vec![task(
            "tools/a",
            "in_progress",
            &[("amber-heron", "executor", H, health)],
        )],
        vec![session("amber-heron", "worker", "opus", H, health)],
    )
}

#[test]
fn a_session_that_misses_beats_goes_stale_then_lost_and_a_beat_brings_it_back_three_changes() {
    let mut recorder = Recorder::new(PathBuf::from("/nowhere"));
    let mut said = recorder.observe(&executing(Health::Live), H, None);
    for (at, health) in [
        (H + 30.0, Health::Live),
        (H + 91.0, Health::Stale),
        (H + 150.0, Health::Stale),
        (H + 601.0, Health::Lost),
        (H + 700.0, Health::Live),
        (H + 760.0, Health::Live),
    ] {
        said.extend(recorder.observe(&executing(health), at, None));
    }
    assert_eq!(
        changes(&said),
        [
            (H + 91.0, "amber-heron", Some("live"), Some("stale")),
            (H + 601.0, "amber-heron", Some("stale"), Some("lost")),
            (H + 700.0, "amber-heron", Some("lost"), Some("live")),
        ]
    );
    let Line::Change(first) = &said[2] else {
        panic!("not a change: {:?}", said[2]);
    };
    assert_eq!(
        (first.kind.as_str(), first.model.as_str()),
        ("worker", "opus")
    );
    assert_eq!(
        (first.task.as_deref(), first.role.as_deref()),
        (Some("tools/a"), Some("executor")),
        "the change names the task the session holds"
    );
}

#[test]
fn the_first_snapshot_says_a_start_and_each_session_not_left_as_seen_and_no_change() {
    let mut recorder = Recorder::new(PathBuf::from("/nowhere"));
    let found = board(
        vec![task(
            "tools/a",
            "in_progress",
            &[("amber-heron", "executor", H, Health::Lost)],
        )],
        vec![
            session("amber-heron", "worker", "opus", H, Health::Lost),
            session("birch-wren", "reviewer", "sonnet", H, Health::Stale),
            session("cedar-owl", "worker", "opus", H, Health::Left),
            session("dune-fox", "orchestrator", "opus", H, Health::Live),
        ],
    );
    let said = recorder.observe(&found, H + 5.0, None);
    assert_eq!(
        said[0],
        Line::Start(Start {
            schema: SCHEMA,
            at: H + 5.0,
            pid: std::process::id(),
            generation: H + 5.0,
            sessions: 4,
            left: 1,
        })
    );
    let seen: Vec<(&str, &str, Option<&str>)> = said[1..]
        .iter()
        .map(|line| match line {
            Line::Seen(s) => (s.handle.as_str(), s.health.as_str(), s.task.as_deref()),
            other => panic!("not seen: {other:?}"),
        })
        .collect();
    assert_eq!(
        seen,
        [
            ("amber-heron", "lost", Some("tools/a")),
            ("birch-wren", "stale", None),
            ("dune-fox", "live", None),
        ],
        "a session found left is counted on the start line alone"
    );
    // The left session is still watched: back, it is a change.
    let back = board(
        Vec::new(),
        vec![
            session("amber-heron", "worker", "opus", H, Health::Lost),
            session("birch-wren", "reviewer", "sonnet", H, Health::Stale),
            session("cedar-owl", "worker", "opus", H, Health::Live),
            session("dune-fox", "orchestrator", "opus", H, Health::Live),
        ],
    );
    assert_eq!(
        changes(&recorder.observe(&back, H + 6.0, None)),
        [(H + 6.0, "cedar-owl", Some("left"), Some("live"))]
    );
}

#[test]
fn after_a_restart_the_first_snapshot_says_what_it_finds_as_seen_and_invents_no_change() {
    let folder = Scratch::new("restart");
    let mut before = Recorder::new(folder.path().to_path_buf());
    before.record(&executing(Health::Live), H, None);
    before.record(&executing(Health::Live), H + 30.0, None);
    // The server goes; the session goes lost meanwhile; a new server starts.
    let mut after = Recorder::new(folder.path().to_path_buf());
    after.record(&executing(Health::Lost), H + 900.0, None);
    after.record(&executing(Health::Lost), H + 930.0, None);
    let lines = read_day(folder.path(), &day(H));
    let kinds: Vec<String> = lines
        .iter()
        .map(|line| match line {
            Line::Start(_) => "start".into(),
            Line::Seen(s) => format!("seen {}", s.health),
            Line::Change(c) => format!("change {:?}", c.to),
            Line::Hour(_) => "hour".into(),
        })
        .collect();
    assert_eq!(
        kinds,
        ["start", "seen live", "start", "seen lost"],
        "two starts, and no change across the restart"
    );
}

#[test]
fn a_session_that_leaves_cleanly_writes_one_change_to_left_and_no_lost() {
    let mut recorder = Recorder::new(PathBuf::from("/nowhere"));
    let mut said = recorder.observe(&executing(Health::Live), H, None);
    let left = board(
        vec![task("tools/a", "ready", &[])],
        vec![session("amber-heron", "worker", "opus", H, Health::Left)],
    );
    for at in [H + 30.0, H + 120.0, H + 700.0, H + 3000.0] {
        said.extend(recorder.observe(&left, at, None));
    }
    assert_eq!(
        changes(&said),
        [(H + 30.0, "amber-heron", Some("live"), Some("left"))]
    );
}

#[test]
fn a_session_new_on_the_board_and_one_gone_from_it_are_changes_from_and_to_nothing() {
    let mut recorder = Recorder::new(PathBuf::from("/nowhere"));
    recorder.observe(
        &board(
            Vec::new(),
            vec![session("amber-heron", "worker", "opus", H, Health::Live)],
        ),
        H,
        None,
    );
    let said = recorder.observe(
        &board(
            Vec::new(),
            vec![session(
                "birch-wren",
                "reviewer",
                "sonnet",
                H + 1.0,
                Health::Live,
            )],
        ),
        H + 2.0,
        None,
    );
    assert_eq!(
        changes(&said),
        [
            (H + 2.0, "birch-wren", None, Some("live")),
            (H + 2.0, "amber-heron", Some("live"), None),
        ]
    );
}

#[test]
fn sessions_new_after_the_others_reordered_or_gone_from_between_them_are_each_compared_by_handle() {
    let s = |handle: &str, model: &str, connected: f64, health| {
        session(handle, "worker", model, connected, health)
    };
    let (live, stale, lost) = (Health::Live, Health::Stale, Health::Lost);
    let mut recorder = Recorder::new(PathBuf::from("/nowhere"));
    let first = board(
        Vec::new(),
        vec![
            s("a", "opus", H, live),
            s("b", "opus", H + 1.0, live),
            s("c", "opus", H + 2.0, live),
        ],
    );
    recorder.observe(&first, H + 3.0, None);

    // d connected after them and b went stale.
    let grown = board(
        Vec::new(),
        vec![
            s("a", "opus", H, live),
            s("b", "opus", H + 1.0, stale),
            s("c", "opus", H + 2.0, live),
            s("d", "opus", H + 4.0, live),
        ],
    );
    let said = recorder.observe(&grown, H + 5.0, None);
    assert_eq!(
        changes(&said),
        [
            (H + 5.0, "b", Some("live"), Some("stale")),
            (H + 5.0, "d", None, Some("live")),
        ]
    );

    // a connected again, so it comes last; b is gone from between the others; c went lost.
    let moved = board(
        Vec::new(),
        vec![
            s("c", "opus", H + 2.0, lost),
            s("d", "opus", H + 4.0, live),
            s("a", "opus", H + 6.0, live),
        ],
    );
    let said = recorder.observe(&moved, H + 7.0, None);
    assert_eq!(
        changes(&said),
        [
            (H + 7.0, "c", Some("live"), Some("lost")),
            (H + 7.0, "b", Some("stale"), None),
        ]
    );
    assert!(changes(&recorder.observe(&moved, H + 8.0, None)).is_empty());

    // d's row now names another model: no change in health, but the line that says it gone names
    // the model it had last.
    let renamed = board(
        Vec::new(),
        vec![
            s("c", "opus", H + 2.0, lost),
            s("d", "sonnet", H + 4.0, live),
            s("a", "opus", H + 6.0, live),
        ],
    );
    assert!(changes(&recorder.observe(&renamed, H + 9.0, None)).is_empty());
    let shrunk = board(
        Vec::new(),
        vec![s("c", "opus", H + 2.0, lost), s("a", "opus", H + 6.0, live)],
    );
    let said = recorder.observe(&shrunk, H + 10.0, None);
    let gone: Vec<(&str, &str, Option<&str>)> = said
        .iter()
        .filter_map(|line| match line {
            Line::Change(c) => Some((c.handle.as_str(), c.model.as_str(), c.to.as_deref())),
            _ => None,
        })
        .collect();
    assert_eq!(gone, [("d", "sonnet", None)]);

    let hour = recorder.flush(H + 10.0);
    let sessions = &hours(&hour)[0].sessions;
    let opus = sessions.iter().find(|row| row.model == "opus").unwrap();
    assert_eq!((opus.went_stale, opus.went_lost), (1, 1));
    assert_eq!(opus.stale_s, 2.0, "b, stale from H + 5 until gone at H + 7");
    assert_eq!(opus.lost_s, 3.0, "c, lost from H + 7");
}

/// The hour of the sessions below, a snapshot every minute from `H + 60` to `H + 3660`: amber-heron
/// executes tools/a, live, then stale from `H + 600`, lost from `H + 1200` and live again from
/// `H + 1800`; birch-wren reviews tools/b and cedar-owl orchestrates both, live throughout;
/// dune-fox, holding nothing, starts at `H + 2000`. The load over a minute is 2 until `H + 1800`,
/// then 4; over 15 minutes, 1.
fn the_hour(at: f64) -> (Board, [f64; 2]) {
    let amber = match at - H {
        t if t < 600.0 => Health::Live,
        t if t < 1200.0 => Health::Stale,
        t if t < 1800.0 => Health::Lost,
        _ => Health::Live,
    };
    let mut sessions = vec![
        session("birch-wren", "reviewer", "sonnet", H - 100.0, Health::Live),
        session("amber-heron", "worker", "opus", H + 10.0, amber),
        session("cedar-owl", "orchestrator", "opus", H + 20.0, Health::Live),
    ];
    if at >= H + 2040.0 {
        sessions.push(session(
            "dune-fox",
            "worker",
            "sonnet",
            H + 2000.0,
            Health::Live,
        ));
    }
    let tasks = vec![
        task(
            "tools/a",
            "in_progress",
            &[
                ("amber-heron", "executor", H, amber),
                ("cedar-owl", "orchestrator", H, Health::Live),
            ],
        ),
        task(
            "tools/b",
            "review",
            &[
                ("birch-wren", "reviewer", H, Health::Live),
                ("cedar-owl", "orchestrator", H, Health::Live),
            ],
        ),
        task(
            "tools/c",
            "done",
            &[("birch-wren", "executor", H, Health::Live)],
        ),
    ];
    let load = if at < H + 1800.0 { 2.0 } else { 4.0 };
    (board(tasks, sessions), [load, 1.0])
}

fn row(role: &str, model: &str, peak: u64, mean: f64) -> RoleModel {
    RoleModel {
        role: role.into(),
        model: model.into(),
        peak,
        mean,
    }
}

fn role(role: &str, peak: u64, mean: f64) -> RoleLevel {
    RoleLevel {
        role: role.into(),
        peak,
        mean,
    }
}

fn kind(kind: &str, peak: u64, mean: f64) -> KindLevel {
    KindLevel {
        kind: kind.into(),
        peak,
        mean,
    }
}

#[test]
fn the_hourly_line_counts_by_kind_and_model_the_time_stale_and_lost_and_what_ran_beside_the_load() {
    let mut recorder = Recorder::new(PathBuf::from("/nowhere"));
    let mut said = Vec::new();
    for minute in 1..=61 {
        let at = H + 60.0 * f64::from(minute);
        let (board, load) = the_hour(at);
        said.extend(recorder.observe(&board, at, Some(load)));
    }
    said.extend(recorder.flush(H + 3690.0));
    assert_eq!(
        changes(&said),
        [
            (H + 600.0, "amber-heron", Some("live"), Some("stale")),
            (H + 1200.0, "amber-heron", Some("stale"), Some("lost")),
            (H + 1800.0, "amber-heron", Some("lost"), Some("live")),
            (H + 2040.0, "dune-fox", None, Some("live")),
        ]
    );
    let hours = hours(&said);
    // Watched from the first snapshot, `H + 60`, to the hour's end: 3540 s, in 59 snapshots.
    // amber-heron executed while live after 39 of the 59 minutes: 2340 s of 3540 s.
    assert_eq!(
        *hours[0],
        Hour {
            schema: SCHEMA,
            at: H + 3600.0,
            pid: std::process::id(),
            generation: H + 60.0,
            start: H,
            from: H + 60.0,
            to: H + 3600.0,
            observed_s: 3540.0,
            samples: 59,
            sessions: vec![
                SessionHour {
                    kind: "orchestrator".into(),
                    model: "opus".into(),
                    started: 1,
                    went_stale: 0,
                    went_lost: 0,
                    stale_s: 0.0,
                    lost_s: 0.0,
                },
                SessionHour {
                    kind: "worker".into(),
                    model: "opus".into(),
                    started: 1,
                    went_stale: 1,
                    went_lost: 1,
                    stale_s: 600.0,
                    lost_s: 600.0,
                },
                SessionHour {
                    kind: "worker".into(),
                    model: "sonnet".into(),
                    started: 1,
                    went_stale: 0,
                    went_lost: 0,
                    stale_s: 0.0,
                    lost_s: 0.0,
                },
            ],
            running: vec![
                row("executor", "opus", 1, 0.661),
                row("orchestrator", "opus", 1, 1.0),
                row("reviewer", "sonnet", 1, 1.0),
            ],
            running_roles: vec![
                role("executor", 1, 0.661),
                role("orchestrator", 1, 1.0),
                role("reviewer", 1, 1.0),
            ],
            // Three while amber-heron ran, two otherwise: (39 × 3 + 20 × 2) × 60 / 3540.
            running_agents: Level {
                peak: 3,
                mean: 2.661
            },
            // amber-heron held its work while live or stale, all but its 600 s lost: 2940 s of 3540 s.
            holding: vec![
                row("executor", "opus", 1, 0.831),
                row("orchestrator", "opus", 1, 1.0),
                row("reviewer", "sonnet", 1, 1.0),
            ],
            holding_roles: vec![
                role("executor", 1, 0.831),
                role("orchestrator", 1, 1.0),
                role("reviewer", 1, 1.0),
            ],
            holding_agents: Level {
                peak: 3,
                mean: 2.831
            },
            // Live workers: amber-heron for 2340 s, dune-fox from `H + 2040`, 1560 s: 3900 / 3540.
            live: vec![
                kind("orchestrator", 1, 1.0),
                kind("reviewer", 1, 1.0),
                kind("worker", 2, 1.102),
            ],
            // 29 minutes at 2 and 30 at 4: (29 × 2 + 30 × 4) × 60 / 3540.
            load_1m: Some(Gauge {
                peak: 4.0,
                mean: 3.017
            }),
            load_15m: Some(Gauge {
                peak: 1.0,
                mean: 1.0
            }),
        }
    );
    // The stop ends the next hour where it stopped: 90 s watched, two snapshots.
    assert_eq!(
        *hours[1],
        Hour {
            schema: SCHEMA,
            at: H + 3690.0,
            pid: std::process::id(),
            generation: H + 60.0,
            start: H + 3600.0,
            from: H + 3600.0,
            to: H + 3690.0,
            observed_s: 90.0,
            samples: 2,
            sessions: Vec::new(),
            running: vec![
                row("executor", "opus", 1, 1.0),
                row("orchestrator", "opus", 1, 1.0),
                row("reviewer", "sonnet", 1, 1.0),
            ],
            running_roles: vec![
                role("executor", 1, 1.0),
                role("orchestrator", 1, 1.0),
                role("reviewer", 1, 1.0),
            ],
            running_agents: Level { peak: 3, mean: 3.0 },
            holding: vec![
                row("executor", "opus", 1, 1.0),
                row("orchestrator", "opus", 1, 1.0),
                row("reviewer", "sonnet", 1, 1.0),
            ],
            holding_roles: vec![
                role("executor", 1, 1.0),
                role("orchestrator", 1, 1.0),
                role("reviewer", 1, 1.0),
            ],
            holding_agents: Level { peak: 3, mean: 3.0 },
            live: vec![
                kind("orchestrator", 1, 1.0),
                kind("reviewer", 1, 1.0),
                kind("worker", 2, 2.0),
            ],
            load_1m: Some(Gauge {
                peak: 4.0,
                mean: 4.0
            }),
            load_15m: Some(Gauge {
                peak: 1.0,
                mean: 1.0
            }),
        }
    );
    assert_eq!(hours.len(), 2);
    assert!(
        recorder.flush(H + 3700.0).is_empty(),
        "a stop says its hour once"
    );
}

#[test]
fn time_between_snapshots_further_apart_than_the_gap_is_not_counted() {
    let mut recorder = Recorder::new(PathBuf::from("/nowhere"));
    recorder.observe(&executing(Health::Stale), H, None);
    recorder.observe(&executing(Health::Stale), H + 100.0, None);
    // The board did not answer for ten minutes.
    recorder.observe(&executing(Health::Stale), H + 700.0, None);
    recorder.observe(&executing(Health::Stale), H + 760.0, None);
    let said = recorder.flush(H + 760.0);
    let hour = hours(&said)[0];
    assert_eq!(hour.observed_s, 160.0);
    assert_eq!(hour.samples, 4);
    assert_eq!(hour.sessions[0].stale_s, 160.0);
    assert_eq!(hour.running, Vec::new(), "a stale executor is not running");
}

#[test]
fn time_across_an_hours_end_is_split_between_the_two_hours() {
    let mut recorder = Recorder::new(PathBuf::from("/nowhere"));
    recorder.observe(&executing(Health::Lost), H + 3590.0, None);
    let said = recorder.observe(&executing(Health::Lost), H + 3630.0, None);
    let ended = hours(&said);
    assert_eq!(ended.len(), 1);
    assert_eq!(
        (ended[0].observed_s, ended[0].sessions[0].lost_s),
        (10.0, 10.0)
    );
    let said = recorder.flush(H + 3630.0);
    let next = hours(&said)[0];
    assert_eq!(
        (
            next.start,
            next.from,
            next.observed_s,
            next.sessions[0].lost_s
        ),
        (H + 3600.0, H + 3600.0, 30.0, 30.0)
    );
}

#[test]
fn a_clock_set_back_counts_no_time_and_keeps_summing_the_newest_hour() {
    let mut recorder = Recorder::new(PathBuf::from("/nowhere"));
    recorder.observe(&executing(Health::Stale), H + 3700.0, None);
    let said = recorder.observe(&executing(Health::Lost), H + 3500.0, None);
    assert!(hours(&said).is_empty(), "{said:?}");
    assert_eq!(
        changes(&said),
        [(H + 3500.0, "amber-heron", Some("stale"), Some("lost"))]
    );
    let said = recorder.flush(H + 3500.0);
    let hour = hours(&said)[0];
    assert_eq!(
        (hour.start, hour.observed_s, hour.samples),
        (H + 3600.0, 0.0, 2)
    );
    assert_eq!(hour.sessions[0].went_lost, 1);
}

#[test]
fn an_hour_with_one_snapshot_has_its_values_for_means() {
    let mut recorder = Recorder::new(PathBuf::from("/nowhere"));
    recorder.observe(&executing(Health::Live), H + 10.0, Some([3.5, 2.5]));
    let said = recorder.flush(H + 10.0);
    let hour = hours(&said)[0];
    assert_eq!(hour.running, [row("executor", "opus", 1, 1.0)]);
    assert_eq!(
        hour.load_1m,
        Some(Gauge {
            peak: 3.5,
            mean: 3.5
        })
    );
    assert_eq!(hour.load_15m.map(|g| g.mean), Some(2.5));
}

#[test]
fn a_load_that_cannot_be_read_keeps_the_last_one_and_none_ever_read_is_none() {
    let mut recorder = Recorder::new(PathBuf::from("/nowhere"));
    recorder.observe(&executing(Health::Live), H, None);
    recorder.observe(&executing(Health::Live), H + 60.0, None);
    let hour = recorder.flush(H + 60.0);
    assert_eq!(hours(&hour)[0].load_1m, None);
    assert_eq!(hours(&hour)[0].load_15m, None);

    let mut recorder = Recorder::new(PathBuf::from("/nowhere"));
    recorder.observe(&executing(Health::Live), H, Some([5.0, 1.0]));
    recorder.observe(&executing(Health::Live), H + 60.0, None);
    recorder.observe(&executing(Health::Live), H + 120.0, None);
    let hour = recorder.flush(H + 120.0);
    assert_eq!(
        hours(&hour)[0].load_1m,
        Some(Gauge {
            peak: 5.0,
            mean: 5.0
        })
    );
}

#[test]
fn running_counts_live_holders_on_work_in_progress_or_review_once_for_each_role() {
    let sessions: Vec<Session> = [("a", "opus"), ("b", "opus"), ("c", "sonnet"), ("d", "opus")]
        .into_iter()
        .map(|(handle, model)| session(handle, "worker", model, H, Health::Live))
        .collect();
    let running = counted(&board(
        vec![
            task(
                "t/1",
                "in_progress",
                &[
                    ("a", "executor", H, Health::Live),
                    ("c", "orchestrator", H, Health::Live),
                    ("d", "reserved", H, Health::Live),
                ],
            ),
            task(
                "t/2",
                "review",
                &[
                    ("a", "reviewer", H, Health::Live),
                    ("b", "reviewer", H, Health::Stale),
                    ("c", "orchestrator", H, Health::Live),
                ],
            ),
            task("t/3", "ready", &[("d", "executor", H, Health::Live)]),
            task("t/4", "blocked", &[("b", "executor", H, Health::Live)]),
        ],
        sessions,
    ));
    assert_eq!(
        running.running.by_model.into_iter().collect::<Vec<_>>(),
        [
            (("executor".to_string(), "opus".to_string()), 1),
            (("orchestrator".to_string(), "sonnet".to_string()), 1),
            (("reviewer".to_string(), "opus".to_string()), 1),
        ]
    );
    assert_eq!(running.running.agents, 2, "a and c, each once");
}

#[test]
fn a_line_names_an_unfinished_task_first_then_executing_before_reviewing_then_the_newest() {
    let at = |s: f64| H + s;
    let tasks = vec![
        task(
            "t/done",
            "done",
            &[("a", "executor", at(9.0), Health::Live)],
        ),
        task(
            "t/old",
            "in_progress",
            &[("a", "orchestrator", at(1.0), Health::Live)],
        ),
        task(
            "t/new",
            "in_progress",
            &[("a", "orchestrator", at(2.0), Health::Live)],
        ),
        task(
            "t/rev",
            "review",
            &[("b", "reviewer", at(1.0), Health::Live)],
        ),
        task(
            "t/exe",
            "in_progress",
            &[("b", "executor", at(0.0), Health::Live)],
        ),
        task(
            "t/gone",
            "cancelled",
            &[("c", "reviewer", at(0.0), Health::Live)],
        ),
        task(
            "t/res",
            "ready",
            &[("d", "reserved", at(0.0), Health::Live)],
        ),
    ];
    let board = board(tasks, Vec::new());
    let named = |handle| held(&board, handle);
    assert_eq!(named("a"), Some(("t/new".into(), "orchestrator".into())));
    assert_eq!(named("b"), Some(("t/exe".into(), "executor".into())));
    assert_eq!(named("c"), Some(("t/gone".into(), "reviewer".into())));
    assert_eq!(named("d"), None, "a reservation is no role a line names");
}

#[test]
fn lines_go_to_the_file_of_their_day_and_an_hour_to_the_day_it_began() {
    let folder = Scratch::new("days");
    let midnight = (H / DAY).ceil() * DAY;
    let mut recorder = Recorder::new(folder.path().to_path_buf());
    recorder.record(&executing(Health::Live), midnight - 30.0, None);
    recorder.record(&executing(Health::Stale), midnight + 30.0, None);
    let before = read_day(folder.path(), &day(midnight - 1.0));
    let after = read_day(folder.path(), &day(midnight));
    assert_eq!(before.len(), 3, "start, seen and the last hour: {before:?}");
    assert!(
        matches!(&before[2], Line::Hour(h) if h.start == midnight - HOUR && h.at == midnight + 30.0)
    );
    assert_eq!(
        changes(&after),
        [(midnight + 30.0, "amber-heron", Some("live"), Some("stale"))]
    );
    assert_eq!(after.len(), 1);
}

#[test]
fn every_kind_of_line_reads_back_as_written_and_what_is_none_of_them_reads_as_nothing() {
    let mut recorder = Recorder::new(PathBuf::from("/nowhere"));
    let mut lines = Vec::new();
    for minute in 1..=61 {
        let at = H + 60.0 * f64::from(minute);
        let (board, load) = the_hour(at);
        lines.extend(recorder.observe(&board, at, Some(load)));
    }
    for kind in ["start", "seen", "change", "hour"] {
        let line = lines
            .iter()
            .find(|line| serde_json::to_value(line).unwrap()["type"] == kind)
            .unwrap_or_else(|| panic!("no {kind} line"));
        let text = serde_json::to_string(line).unwrap();
        assert!(
            text.starts_with(&format!("{{\"type\":\"{kind}\",\"schema\":1,")),
            "{text}"
        );
        assert_eq!(read_line(&text).as_ref(), Some(line), "{text}");
    }
    for text in [
        "",
        "[]",
        "{\"schema\":1,\"at\":1}",
        "{\"type\":\"later\",\"schema\":1,\"at\":1}",
        "{\"type\":\"start\",\"schema\":1}",
        "{\"type\":\"change\",\"schema\":1,\"at\":\"noon\"}",
    ] {
        assert_eq!(read_line(text), None, "{text}");
    }
}

#[test]
fn a_line_after_one_cut_short_starts_on_a_line_of_its_own_and_a_reader_passes_over_the_cut() {
    let folder = Scratch::new("cut");
    let file = folder.path().join(format!("{}.jsonl", day(H)));
    fs::write(&file, b"{\"schema\":1,\"type\":\"cha").unwrap();
    let mut recorder = Recorder::new(folder.path().to_path_buf());
    recorder.record(&executing(Health::Live), H, None);
    let text = fs::read_to_string(&file).unwrap();
    assert!(
        text.starts_with("{\"schema\":1,\"type\":\"cha\n{"),
        "{text}"
    );
    let lines = read_day(folder.path(), &day(H));
    assert!(matches!(lines[0], Line::Start(_)));
    assert_eq!(lines.len(), 2);
}

#[test]
fn a_day_file_that_is_a_link_is_not_written_through_and_the_failure_is_kept() {
    let folder = Scratch::new("link");
    let elsewhere = folder.path().join("elsewhere");
    fs::write(&elsewhere, b"").unwrap();
    std::os::unix::fs::symlink(&elsewhere, folder.path().join(format!("{}.jsonl", day(H))))
        .unwrap();
    let mut recorder = Recorder::new(folder.path().to_path_buf());
    recorder.record(&executing(Health::Live), H, None);
    assert_eq!(fs::read(&elsewhere).unwrap(), b"");
    let failed = recorder.unwritten.clone();
    assert!(failed.is_some());
    assert!(read_day(folder.path(), &day(H)).is_empty());
    // A tick with nothing to write leaves the failure as it was, so the log says it once.
    recorder.record(&executing(Health::Live), H + 1.0, None);
    assert_eq!(recorder.unwritten, failed);
    // With the link gone, the next line is written and the failure is over.
    fs::remove_file(folder.path().join(format!("{}.jsonl", day(H)))).unwrap();
    recorder.record(&executing(Health::Stale), H + 91.0, None);
    assert_eq!(recorder.unwritten, None);
    assert_eq!(
        changes(&read_day(folder.path(), &day(H))),
        [(H + 91.0, "amber-heron", Some("live"), Some("stale"))]
    );
}

#[test]
fn a_day_file_that_is_a_fifo_or_a_folder_is_neither_written_nor_read_and_nothing_waits_on_it() {
    let folder = Scratch::new("fifo");
    let fifo = folder.path().join(format!("{}.jsonl", day(H)));
    let path = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: `path` is a NUL-terminated path that outlives the call.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let mut recorder = Recorder::new(folder.path().to_path_buf());
    recorder.record(&executing(Health::Live), H, None);
    assert!(
        recorder.unwritten.is_some(),
        "no reader: refused, not waited on"
    );
    assert!(read_day(folder.path(), &day(H)).is_empty());
    let refused = regular(&fifo, OpenOptions::new().read(true)).unwrap_err();
    assert!(
        refused.to_string().ends_with("not a regular file"),
        "{refused}"
    );

    let folder = Scratch::new("folder");
    let named = folder.path().join(format!("{}.jsonl", day(H)));
    fs::create_dir(&named).unwrap();
    let mut recorder = Recorder::new(folder.path().to_path_buf());
    recorder.record(&executing(Health::Live), H, None);
    assert!(recorder.unwritten.is_some());
    assert!(named.is_dir());
    assert!(read_day(folder.path(), &day(H)).is_empty());
}

#[test]
fn day_files_more_than_90_days_old_are_removed_once_a_day_and_nothing_else() {
    let folder = Scratch::new("prune");
    let dir = folder.path();
    let name = |days: f64| format!("{}.jsonl", day(H - days * DAY));
    fs::write(dir.join(name(91.0)), b"old\n").unwrap();
    fs::write(dir.join(name(90.0)), b"kept\n").unwrap();
    fs::write(dir.join(name(1.0)), b"kept\n").unwrap();
    fs::write(dir.join("notes.txt"), b"kept\n").unwrap();
    fs::create_dir(dir.join(name(100.0))).unwrap();
    std::os::unix::fs::symlink(dir.join("notes.txt"), dir.join(name(95.0))).unwrap();
    let mut recorder = Recorder::new(dir.to_path_buf());
    recorder.record(&executing(Health::Live), H, None);
    let mut left: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    left.sort();
    let mut kept = vec![
        name(100.0),
        name(95.0),
        name(90.0),
        name(1.0),
        format!("{}.jsonl", day(H)),
        "notes.txt".to_string(),
    ];
    kept.sort();
    assert_eq!(left, kept);
    // Once a day: a file grown old within the day stays until the next day's first write.
    fs::write(dir.join(name(92.0)), b"old\n").unwrap();
    recorder.record(&executing(Health::Live), H + 60.0, None);
    assert!(dir.join(name(92.0)).exists());
    recorder.record(&executing(Health::Live), H + DAY, None);
    assert!(!dir.join(name(92.0)).exists());
    // A folder that is not there has nothing to prune.
    prune(&dir.join("missing"), H);
    assert!(!dir.join("missing").exists());
}

#[test]
fn the_folder_is_the_one_named_else_none_for_a_named_board_server_else_the_common_dirs() {
    let tree = Tree::new();
    let root = repository(&tree);
    let env = |vars: &[(&str, &str)]| symphony_board::cli::Env {
        vars: vars
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect(),
        cwd: root.clone(),
    };
    let common = symphony_board::git::common_dir(&root).unwrap();
    assert_eq!(folder(&env(&[])).unwrap(), common.join("symphony/agents"));
    assert_eq!(
        folder(&env(&[("TASKS_DATABASE", "tasks")])).unwrap(),
        common.join("symphony/agents")
    );
    assert_eq!(
        folder(&env(&[("TASKS_DATABASE", "scratch")])).unwrap(),
        common.join("symphony/agents.scratch")
    );
    assert!(folder(&env(&[("TASKS_SERVER", "127.0.0.1:1")])).is_err());
    assert_eq!(
        folder(&env(&[
            ("TASKS_SERVER", "127.0.0.1:1"),
            (FOLDER_VAR, "/x/y")
        ]))
        .unwrap(),
        PathBuf::from("/x/y")
    );
    assert_eq!(
        folder(&env(&[(FOLDER_VAR, "")])).unwrap(),
        common.join("symphony/agents"),
        "an empty setting names nothing"
    );
}

#[test]
fn the_load_average_reads_on_this_machine() {
    let [one, fifteen] = load_average().expect("getloadavg answers");
    assert!(one >= 0.0 && fifteen >= 0.0, "{one} {fifteen}");
}

/// The hub's clock, as the test moves it.
#[derive(Clone)]
struct Clock(Arc<Mutex<f64>>);

impl Clock {
    fn now(&self) -> f64 {
        *self.0.lock().unwrap()
    }
}

/// A hub on a scratch repository recording to `folder` as `gate` lets it, its clock moved by the
/// test with the fixture's.
fn recording_hub(root: &Path, folder: &Path, clock: &Clock, gate: Arc<Gate>) -> Hub {
    let mut hub = Hub::new(root, &root.join("build/client"));
    *hub.agent_activity.get_mut().unwrap() = Some(Recorder::new(folder.to_path_buf()));
    hub.gate_recorders(gate);
    let clock = clock.clone();
    hub.clock = Box::new(move || clock.now());
    hub
}

/// Both clocks moved by `seconds`.
fn pass(fixture: &Fixture, clock: &Clock, seconds: f64) {
    fixture.advance(seconds);
    *clock.0.lock().unwrap() += seconds;
}

fn executor_on_a_scratch_board() -> Fixture {
    Fixture::new(
        Seed::default()
            .session_row(SessionRow {
                model: "opus".into(),
                ..seeded("amber-heron")
            })
            .task("tools/a", "in_progress")
            .role("tools/a", "amber-heron", "executor"),
    )
}

#[test]
fn on_a_scratch_board_the_hub_says_stale_lost_and_back_at_its_own_time_then_a_clean_leave() {
    let tree = Tree::new();
    let root = repository(&tree);
    let folder = tree.path("agents");
    let fixture = executor_on_a_scratch_board();
    let clock = Clock(Arc::new(Mutex::new(NOW)));
    let hub = recording_hub(&root, &folder, &clock, Arc::default());

    hub.board_read(&fixture.store).unwrap();
    pass(&fixture, &clock, 91.0);
    hub.board_read(&fixture.store).unwrap();
    pass(&fixture, &clock, 510.0);
    hub.board_read(&fixture.store).unwrap();
    pass(&fixture, &clock, 5.0);
    fixture.store.touch("amber-heron").unwrap();
    hub.board_read(&fixture.store).unwrap();
    pass(&fixture, &clock, 30.0);
    fixture.store.leave("amber-heron").unwrap();
    hub.board_read(&fixture.store).unwrap();
    pass(&fixture, &clock, 700.0);
    hub.board_read(&fixture.store).unwrap();
    hub.stop();

    let lines = read_day(&folder, &day(NOW));
    assert!(matches!(&lines[1], Line::Seen(s) if s.handle == "amber-heron" && s.health == "live"));
    assert_eq!(
        changes(&lines),
        [
            (NOW + 91.0, "amber-heron", Some("live"), Some("stale")),
            (NOW + 601.0, "amber-heron", Some("stale"), Some("lost")),
            (NOW + 606.0, "amber-heron", Some("lost"), Some("live")),
            (NOW + 636.0, "amber-heron", Some("live"), Some("left")),
        ]
    );
    let hours = hours(&lines);
    assert_eq!(hours.len(), 1, "the stop wrote the hour so far");
    let hour = hours[0];
    assert_eq!((hour.from, hour.to), (NOW, NOW + 1336.0));
    assert_eq!(hour.sessions.len(), 1);
    let worker = &hour.sessions[0];
    assert_eq!(
        (worker.kind.as_str(), worker.model.as_str()),
        ("worker", "opus")
    );
    assert_eq!((worker.went_stale, worker.went_lost), (1, 1));
    // Only the snapshots at most two minutes apart count: the 91 s it was live, the 5 s lost and
    // the 30 s live again; the 510 s stale and the 700 s left were not watched.
    assert_eq!(hour.observed_s, 126.0);
    assert_eq!((worker.stale_s, worker.lost_s), (0.0, 5.0));
}

#[test]
fn recording_makes_no_board_write_though_the_board_counts_writes() {
    let tree = Tree::new();
    let root = repository(&tree);
    let folder = tree.path("agents");
    let fixture = executor_on_a_scratch_board();
    let clock = Clock(Arc::new(Mutex::new(NOW)));
    let mut hub = Hub::new(&root, &root.join("build/client"));
    let moved = clock.clone();
    hub.clock = Box::new(move || moved.now());
    let log = fixture.store.client().queries();
    let writes = || log.report(symphony_board::sweep::trigger::now()).1.count;
    // Warm: the first reads may set the dispatch's own state up.
    hub.board_read(&fixture.store).unwrap();
    hub.board_read(&fixture.store).unwrap();
    let before = writes();
    hub.board_read(&fixture.store).unwrap();
    assert_eq!(
        writes(),
        before,
        "a read of a board that stood still writes nothing"
    );

    *hub.agent_activity.lock().unwrap() = Some(Recorder::new(folder.clone()));
    hub.gate_recorders(Arc::default());
    for _ in 0..3 {
        hub.board_read(&fixture.store).unwrap();
        pass(&fixture, &clock, 91.0);
    }
    hub.board_read(&fixture.store).unwrap();
    let lines = read_day(&folder, &day(NOW));
    assert!(
        changes(&lines).len() == 1 && lines.len() >= 3,
        "it recorded: {lines:?}"
    );
    assert_eq!(writes(), before, "recording wrote to the board");
    // The count moves for a write.
    fixture.store.touch("amber-heron").unwrap();
    assert!(writes() > before);
}

/// A hub started as the shared server or a private one, on a scratch repository and board, once
/// its first board read is through.
fn hub_started(drain_chat: bool) -> (Tree, Arc<Hub>, PathBuf) {
    let tree = Tree::new();
    let root = repository(&tree);
    let mut store = Some(executor_on_a_scratch_board().store);
    let hub = Hub::start_watching::<notify::NullWatcher>(
        &root,
        &root.join("build/client"),
        false,
        drain_chat,
        crate::backstop::PERIOD,
        move |_| store.take().ok_or_else(|| "one store in this test".into()),
    )
    .unwrap();
    hub.gate_recorders(Arc::default());
    hub.start_initial_board();
    test_time::until_true(
        "the first board read",
        test_time::backstop(Duration::from_secs(60)),
        || hub.task_snapshot().is_ok(),
    )
    .unwrap();
    let common = symphony_board::git::common_dir(&root).unwrap();
    (tree, hub, common.join(FOLDER))
}

/// Every line of every day file in `folder`, none while there is none.
fn days(folder: &Path) -> Vec<Line> {
    let mut lines = Vec::new();
    for entry in fs::read_dir(folder).into_iter().flatten() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        lines.extend(read_day(folder, name.trim_end_matches(".jsonl")));
    }
    lines
}

#[test]
fn only_the_one_server_writes_the_day_files_and_a_private_server_writes_nothing() {
    let (_private_tree, private, private_folder) = hub_started(false);
    let (_shared_tree, shared, shared_folder) = hub_started(true);
    // Until the shared server's ticks have recorded its agent; the private one, started first,
    // has ticked as long.
    test_time::until_true(
        "the shared server to record amber-heron",
        test_time::backstop(Duration::from_secs(60)),
        || {
            days(&shared_folder)
                .iter()
                .any(|l| matches!(l, Line::Seen(s) if s.handle == "amber-heron"))
        },
    )
    .unwrap();
    private.stop();
    shared.stop();
    assert!(
        !private_folder.exists(),
        "a private server wrote {}",
        private_folder.display()
    );
    let lines = days(&shared_folder);
    assert!(matches!(lines[0], Line::Start(_)), "{lines:?}");
    assert!(
        lines
            .iter()
            .any(|l| matches!(l, Line::Seen(s) if s.handle == "amber-heron")),
        "{lines:?}"
    );
    assert!(
        matches!(lines.last(), Some(Line::Hour(_))),
        "the stop wrote the hour so far: {lines:?}"
    );
}

/// What a tick's recording costs at the size of the consuming project's board (2,700 tasks, 2,600
/// sessions, about 3,000 roles) and at a tenth of it, in process CPU per snapshot, and what an
/// append costs: run by hand, `tools/cargo.sh test -p symphony-server agent_activity::tests::cost
/// -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, run by hand"]
fn cost() {
    fn sized(tasks: usize, sessions: usize) -> Board {
        let handle = |n: usize| format!("agent-{n}");
        let sessions_now: Vec<Session> = (0..sessions)
            .map(|n| {
                let health = if n % 40 == 0 {
                    Health::Live
                } else {
                    Health::Left
                };
                session(&handle(n), "worker", "opus", H + n as f64, health)
            })
            .collect();
        let tasks_now: Vec<Task> = (0..tasks)
            .map(|n| {
                let status = if n % 30 == 0 { "in_progress" } else { "done" };
                let holder = handle(n % sessions);
                let health = if n % 40 == 0 {
                    Health::Live
                } else {
                    Health::Left
                };
                task(
                    &format!("t/{n}"),
                    status,
                    &[(holder.as_str(), "executor", H, health)],
                )
            })
            .collect();
        board(tasks_now, sessions_now)
    }
    for (tasks, sessions) in [(270, 260), (2700, 2600)] {
        let mut recorder = Recorder::new(PathBuf::from("/nowhere"));
        let sized = sized(tasks, sessions);
        recorder.observe(&sized, H, Some([1.0, 1.0]));
        let rounds = 2000;
        let cpu = crate::live::cpu_millis().unwrap();
        let wall = Instant::now();
        for i in 1..=rounds {
            recorder.observe(&sized, H + f64::from(i), Some([1.0, 1.0]));
        }
        let per_wall = wall.elapsed().as_secs_f64() * 1e6 / f64::from(rounds);
        let per_cpu = (crate::live::cpu_millis().unwrap() - cpu) * 1e3 / f64::from(rounds);
        println!(
            "observe, {tasks} tasks and {sessions} sessions: {per_cpu:.1} µs CPU, {per_wall:.1} µs wall per snapshot"
        );
    }
    let rounds = 2000;
    let cpu = crate::live::cpu_millis().unwrap();
    for _ in 0..rounds {
        assert!(load_average().is_some());
    }
    println!(
        "reading the load average: {:.1} µs CPU",
        (crate::live::cpu_millis().unwrap() - cpu) * 1e3 / f64::from(rounds)
    );
    let folder = Scratch::new("cost");
    let line = [Line::Change(Change {
        schema: SCHEMA,
        at: H,
        pid: 4242,
        generation: H,
        handle: "amber-heron".into(),
        kind: "worker".into(),
        model: "claude-opus-5-5".into(),
        task: Some("tools/agent-activity-history".into()),
        role: Some("executor".into()),
        from: Some("live".into()),
        to: Some("stale".into()),
    })];
    let rounds = 2000;
    let cpu = crate::live::cpu_millis().unwrap();
    let wall = Instant::now();
    for _ in 0..rounds {
        append(folder.path(), &line).unwrap();
    }
    println!(
        "append of one change line: {:.1} µs CPU, {:.1} µs wall",
        (crate::live::cpu_millis().unwrap() - cpu) * 1e3 / f64::from(rounds),
        wall.elapsed().as_secs_f64() * 1e6 / f64::from(rounds)
    );
}

#[test]
fn holding_keeps_the_quiet_holders_running_leaves_out_and_live_counts_every_live_session_by_kind() {
    let board = board(
        vec![
            task(
                "t/build",
                "in_progress",
                &[
                    // Waiting on a long build: no tool call, so stale, but still at work.
                    ("amber-heron", "executor", H, Health::Stale),
                    ("cedar-owl", "orchestrator", H, Health::Live),
                ],
            ),
            task(
                "t/review",
                "review",
                &[
                    ("birch-wren", "reviewer", H, Health::Unknown),
                    ("dune-fox", "executor", H, Health::Lost),
                ],
            ),
            // An orchestrator whose task is only ready holds nothing running or held.
            task(
                "t/ready",
                "ready",
                &[("elm-jay", "orchestrator", H, Health::Live)],
            ),
        ],
        vec![
            session("amber-heron", "worker", "opus", H, Health::Stale),
            session("birch-wren", "reviewer", "sonnet", H, Health::Unknown),
            session("cedar-owl", "orchestrator", "opus", H, Health::Live),
            session("dune-fox", "worker", "opus", H, Health::Lost),
            session("elm-jay", "orchestrator", "opus", H, Health::Live),
            session("fir-kite", "worker", "opus", H, Health::Left),
        ],
    );
    let counted = counted(&board);
    let by_model = |holders: &Holders| holders.by_model.clone().into_iter().collect::<Vec<_>>();
    let key = |role: &str, model: &str| (role.to_string(), model.to_string());
    assert_eq!(
        by_model(&counted.running),
        [(key("orchestrator", "opus"), 1)]
    );
    assert_eq!(counted.running.agents, 1);
    assert_eq!(
        by_model(&counted.holding),
        [
            (key("executor", "opus"), 1),
            (key("orchestrator", "opus"), 1),
            (key("reviewer", "sonnet"), 1),
        ],
        "stale and unknown holders hold their work; the lost one does not"
    );
    assert_eq!(counted.holding.agents, 3);
    assert_eq!(
        counted.live.into_iter().collect::<Vec<_>>(),
        [("orchestrator".to_string(), 2)],
        "both live orchestrators, the one holding only a ready task too; no other session is live"
    );
}

/// The writer a line names: its pid and generation.
fn writer(line: &Line) -> (u32, f64) {
    match line {
        Line::Start(l) => (l.pid, l.generation),
        Line::Seen(l) => (l.pid, l.generation),
        Line::Change(l) => (l.pid, l.generation),
        Line::Hour(l) => (l.pid, l.generation),
    }
}

/// What a reader makes of `hour`'s lines, from one writer or several, as docs/workflow/AGENT_ACTIVITY.md
/// says: seconds, samples and changes added, peaks the largest, means weighted by `observed_s`,
/// `started` from the line with the latest `to`; and each line's span and writer.
#[derive(Debug, PartialEq)]
struct Summed {
    spans: Vec<(f64, f64, f64)>,
    observed_s: f64,
    samples: u64,
    went_stale: u64,
    stale_s: f64,
    started: u64,
    running_peak: u64,
    /// The running and the holding agents' means.
    means: Means,
}

/// Two means, alike to within the thousandth each line keeps of its own.
#[derive(Debug)]
struct Means(f64, f64);

impl PartialEq for Means {
    fn eq(&self, other: &Means) -> bool {
        (self.0 - other.0).abs() <= 0.001 && (self.1 - other.1).abs() <= 0.001
    }
}

fn summed(lines: &[Line], hour: f64) -> Summed {
    let lines: Vec<&Hour> = hours(lines)
        .into_iter()
        .filter(|line| line.start == hour)
        .collect();
    let observed_s: f64 = lines.iter().map(|line| line.observed_s).sum();
    let mean = |level: fn(&Hour) -> f64| {
        lines
            .iter()
            .map(|line| level(line) * line.observed_s)
            .sum::<f64>()
            / observed_s
    };
    let sessions = |count: fn(&SessionHour) -> f64| {
        lines
            .iter()
            .flat_map(|line| &line.sessions)
            .map(count)
            .sum::<f64>()
    };
    let latest = lines
        .iter()
        .max_by(|a, b| a.to.total_cmp(&b.to))
        .expect("a line of the hour");
    Summed {
        spans: lines
            .iter()
            .map(|line| (line.from, line.to, line.generation))
            .collect(),
        observed_s,
        samples: lines.iter().map(|line| line.samples).sum(),
        went_stale: sessions(|row| row.went_stale as f64) as u64,
        stale_s: sessions(|row| row.stale_s),
        started: latest.sessions.iter().map(|row| row.started).sum(),
        running_peak: lines
            .iter()
            .map(|line| line.running_agents.peak)
            .max()
            .unwrap_or(0),
        means: Means(
            mean(|line| line.running_agents.mean),
            mean(|line| line.holding_agents.mean),
        ),
    }
}

/// How a reload's hand-off at `H + 1830` ends.
#[derive(Clone, Copy)]
enum HandOff {
    Committed,
    /// The replacement never became ready: it stops, or is killed.
    FailedBeforeTheClose,
    /// Its commit failed after the old server closed its gate, which opens again at `H + 1890`.
    FailedAfterTheClose,
}

/// An hour of amber-heron executing, live and then stale from `H + 600`, every minute from `H + 60`
/// a tick of the serving server and, from `H + 1260`, of its replacement too, both recording to one
/// folder through their gates; the hand-off at `H + 1830` ends as `outcome` says, and a server
/// replaced stops at `H + 1925`. The day's lines.
fn hand_off(outcome: HandOff) -> Vec<Line> {
    let folder = Scratch::new("hand-off");
    let at = |t: f64| {
        executing(if t < H + 600.0 {
            Health::Live
        } else {
            Health::Stale
        })
    };
    let (old_gate, new_gate) = (Gate::default(), Gate::default());
    new_gate.hold();
    let mut old = Some(Recorder::new(folder.path().to_path_buf()));
    let mut new = Recorder::new(folder.path().to_path_buf());
    let mut replacement = false;
    for minute in 1..=60 {
        let t = H + 60.0 * f64::from(minute);
        match (outcome, minute) {
            (_, 21) => replacement = true,
            (HandOff::Committed, 31) => {
                old_gate.close(H + 1830.0);
                new_gate.open(H + 1830.0);
            }
            (HandOff::Committed, 33) => {
                gated_stop(&old_gate, old.as_mut().unwrap(), || H + 1925.0);
                old = None;
            }
            (HandOff::FailedBeforeTheClose, 31) => {
                gated_stop(&new_gate, &mut new, || t);
                replacement = false;
            }
            (HandOff::FailedAfterTheClose, 31) => old_gate.close(H + 1830.0),
            (HandOff::FailedAfterTheClose, 32) => {
                old_gate.reopen(t);
                replacement = false;
            }
            _ => {}
        }
        if let Some(old) = old.as_mut() {
            gated_record(&old_gate, old, &at(t), || t);
        }
        if replacement {
            gated_record(&new_gate, &mut new, &at(t), || t);
        }
    }
    read_day(folder.path(), &day(H))
}

#[test]
fn a_reloads_two_servers_never_record_the_same_moment_so_a_reader_sums_their_hour_as_any_other() {
    let starts = |lines: &[Line]| -> Vec<f64> {
        lines
            .iter()
            .filter(|line| matches!(line, Line::Start(_)))
            .map(|line| writer(line).1)
            .collect()
    };
    let stale = [(H + 600.0, "amber-heron", Some("live"), Some("stale"))];

    // Committed: the old server's hour ends at the commit, the replacement's begins at its first
    // snapshot after it; what the replacement's ticks saw before it is not recorded.
    let lines = hand_off(HandOff::Committed);
    assert_eq!(starts(&lines), [H + 60.0, H + 1860.0]);
    assert_eq!(changes(&lines), stale, "the change is said once");
    for line in &lines {
        let (pid, generation) = writer(line);
        assert_eq!(pid, std::process::id());
        let at = match line {
            Line::Hour(hour) => hour.to,
            Line::Start(l) => l.at,
            Line::Seen(l) => l.at,
            Line::Change(l) => l.at,
        };
        assert!(
            (generation == H + 60.0 && at <= H + 1830.0)
                || (generation == H + 1860.0 && at >= H + 1860.0),
            "{line:?}"
        );
    }
    assert_eq!(
        summed(&lines, H),
        Summed {
            spans: vec![
                (H + 60.0, H + 1830.0, H + 60.0),
                (H + 1860.0, H + 3600.0, H + 1860.0)
            ],
            // 1770 s watched by the old server and 1740 s by the replacement; the 30 s between
            // the commit and the replacement's first snapshot by neither.
            observed_s: 3510.0,
            samples: 59,
            went_stale: 1,
            // Stale from `H + 600` to the commit, and from the replacement's first snapshot.
            stale_s: 1230.0 + 1740.0,
            started: 1,
            running_peak: 1,
            means: Means(540.0 / 3510.0, 1.0),
        }
    );

    // The replacement never became ready: it recorded nothing, and the old server watched it all.
    let lines = hand_off(HandOff::FailedBeforeTheClose);
    assert_eq!(starts(&lines), [H + 60.0]);
    assert!(lines.iter().all(|line| writer(line).1 == H + 60.0));
    assert_eq!(changes(&lines), stale);
    assert_eq!(
        summed(&lines, H),
        Summed {
            spans: vec![(H + 60.0, H + 3600.0, H + 60.0)],
            observed_s: 3540.0,
            samples: 59,
            went_stale: 1,
            stale_s: 3000.0,
            started: 1,
            running_peak: 1,
            means: Means(540.0 / 3540.0, 1.0),
        }
    );

    // The commit failed after the close: the old server's hour ends at the close and goes on from
    // its first snapshot after the gate opened again, the time between watched by no one.
    let lines = hand_off(HandOff::FailedAfterTheClose);
    assert_eq!(starts(&lines), [H + 60.0]);
    assert_eq!(changes(&lines), stale);
    assert_eq!(
        summed(&lines, H),
        Summed {
            spans: vec![
                (H + 60.0, H + 1830.0, H + 60.0),
                (H + 1920.0, H + 3600.0, H + 60.0)
            ],
            observed_s: 1770.0 + 1680.0,
            samples: 30 + 28,
            went_stale: 1,
            stale_s: 1230.0 + 1680.0,
            started: 1,
            running_peak: 1,
            means: Means(540.0 / 3450.0, 1.0),
        }
    );
}

#[test]
fn an_hour_across_a_stop_and_a_start_has_two_lines_a_reader_adds_taking_started_from_the_later() {
    let folder = Scratch::new("restart-hour");
    // birch-wren connects at `H + 1300`, while no server runs.
    let at = |t: f64| {
        let mut board = executing(Health::Live);
        if t >= H + 1300.0 {
            board.sessions.push(session(
                "birch-wren",
                "reviewer",
                "sonnet",
                H + 1300.0,
                Health::Live,
            ));
        }
        board
    };
    let gate = Gate::default();
    let mut before = Recorder::new(folder.path().to_path_buf());
    for minute in 1..=20 {
        let t = H + 60.0 * f64::from(minute);
        gated_record(&gate, &mut before, &at(t), || t);
    }
    gated_stop(&gate, &mut before, || H + 1230.0);
    let mut after = Recorder::new(folder.path().to_path_buf());
    for minute in 25..=60 {
        let t = H + 60.0 * f64::from(minute);
        gated_record(&gate, &mut after, &at(t), || t);
    }
    let lines = read_day(folder.path(), &day(H));
    let started: Vec<u64> = hours(&lines)
        .iter()
        .map(|hour| hour.sessions.iter().map(|row| row.started).sum())
        .collect();
    assert_eq!(
        started,
        [1, 2],
        "each line counts the hour's sessions from its start"
    );
    assert_eq!(
        summed(&lines, H),
        Summed {
            spans: vec![
                (H + 60.0, H + 1230.0, H + 60.0),
                (H + 1500.0, H + 3600.0, H + 1500.0)
            ],
            observed_s: 1170.0 + 2100.0,
            samples: 20 + 35,
            went_stale: 0,
            stale_s: 0.0,
            started: 2,
            running_peak: 1,
            means: Means(1.0, 1.0),
        }
    );
}

#[test]
fn an_hour_in_which_every_board_read_failed_has_no_line() {
    let mut recorder = Recorder::new(PathBuf::from("/nowhere"));
    let mut lines = recorder.observe(&executing(Health::Live), H + 60.0, None);
    // The server runs on, but its next snapshot is two hours later.
    lines.extend(recorder.observe(&executing(Health::Live), H + 7260.0, None));
    lines.extend(recorder.flush(H + 7300.0));
    let summed: Vec<(f64, f64, f64)> = hours(&lines)
        .iter()
        .map(|hour| (hour.start, hour.from, hour.to))
        .collect();
    assert_eq!(
        summed,
        [
            (H, H + 60.0, H + 3600.0),
            (H + 7200.0, H + 7260.0, H + 7300.0)
        ],
        "no line for the hour from `H + 3600`"
    );
}

#[test]
fn a_hub_records_only_as_its_generations_gate_lets_it_and_none_without_one() {
    let tree = Tree::new();
    let root = repository(&tree);
    let folder = tree.path("agents");
    let fixture = executor_on_a_scratch_board();
    let kinds = || -> Vec<String> {
        read_day(&folder, &day(NOW))
            .iter()
            .map(|line| serde_json::to_value(line).unwrap()["type"].to_string())
            .collect()
    };

    let mut ungated = Hub::new(&root, &root.join("build/client"));
    *ungated.agent_activity.get_mut().unwrap() = Some(Recorder::new(folder.clone()));
    ungated.board_read(&fixture.store).unwrap();
    ungated.stop_agents();
    assert!(kinds().is_empty(), "a hub given no gate records nothing");

    let clock = Clock(Arc::new(Mutex::new(NOW)));
    let gate = Arc::new(Gate::default());
    gate.hold();
    let hub = recording_hub(&root, &folder, &clock, gate.clone());
    hub.board_read(&fixture.store).unwrap();
    assert!(
        kinds().is_empty(),
        "a replacement records nothing before its commit"
    );
    gate.open(NOW);
    pass(&fixture, &clock, 30.0);
    hub.board_read(&fixture.store).unwrap();
    pass(&fixture, &clock, 30.0);
    hub.board_read(&fixture.store).unwrap();
    gate.close(NOW + 75.0);
    pass(&fixture, &clock, 30.0);
    hub.board_read(&fixture.store).unwrap();
    pass(&fixture, &clock, 30.0);
    hub.board_read(&fixture.store).unwrap();
    hub.stop_agents();
    assert_eq!(kinds(), ["\"start\"", "\"seen\"", "\"hour\""]);
    let lines = read_day(&folder, &day(NOW));
    let hour = hours(&lines)[0];
    assert_eq!(
        (hour.from, hour.to, hour.observed_s, hour.samples),
        (NOW + 30.0, NOW + 75.0, 45.0, 2),
        "the hour ends at the close, written once"
    );
}

/// The line that makes the server record at all: a hub given no gate records nothing, and only
/// `api::start` gives a server's hubs theirs, its `Shared`'s. So the hub it starts for the server's
/// own project, and the one it starts for a project served beside it (`Shared::beside`), hold that
/// one gate of the server generation, the same `Arc`, for both their recorders, the agents' and
/// the board's health hours: were it not given, or not shared, or a recorder given a gate of its
/// own (a fresh one, which a reload's hold, close and reopen never reach, so each hand-off counted
/// twice), every other test here and in the board's would pass while the shared server wrote
/// nothing, or wrote the same minutes from two generations. No board is opened: the first board
/// read is never started.
#[test]
fn the_hubs_a_server_starts_hold_its_generations_one_gate_whatever_project_they_serve() {
    let tree = Tree::new();
    let (root, beside) = (
        repository(&tree),
        crate::testing::repository_at(&tree, "beside"),
    );
    let requests = crate::gate::Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now));
    let shared = crate::front::Shared::new(1, 1, &root, None, requests);
    let other = shared.beside(&beside, "/beside");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let (own, theirs) = runtime.block_on(async {
        let own = crate::api::start(&shared).await.unwrap();
        (own, crate::api::start(&other).await.unwrap())
    });
    for (hub, project) in [(&own, "its own project"), (&theirs, "a project beside it")] {
        let gate = hub
            .recording_gate
            .get()
            .expect("api::start gives a hub its gate");
        assert!(
            Arc::ptr_eq(gate, &shared.recording),
            "the agents of {project} keep to the server generation's one gate"
        );
        let health = hub
            .health
            .gate()
            .expect("api::start gives a hub's board health its gate");
        assert!(
            Arc::ptr_eq(&health, &shared.recording),
            "the board's hours of {project} keep to that same gate"
        );
    }
    shared.stop();
    other.stop();
}
