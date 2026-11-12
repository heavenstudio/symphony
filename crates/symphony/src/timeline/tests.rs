//! The timeline built from a thread, run records and gates, each case's times written out by hand.
//! The pictures' task is the fixture the API's wire test and the client's views read.

use super::*;
use symphony_api::GatePast;
use symphony_api::timeline::GateSpan;

/// The approved pictures' task: made 09:02 (the pictures' clock, two hours ahead of UTC), now 15:20.
const FIXTURE: &str = include_str!("../../../symphony-api/fixtures/task-timeline.json");
const SLUG: &str = "tools/verify-stops-when-moved";
const MADE: f64 = 1_794_380_520.0;
const NOW: f64 = 1_794_403_200.0;
const M: f64 = 60.0;

/// `minutes` after 09:02, the task's creation.
fn at(minutes: f64) -> f64 {
    MADE + minutes * M
}

fn message(id: i64, kind: &str, sender: &str, ts: f64, text: &str) -> symphony_board::Message {
    symphony_board::Message {
        id,
        slug: SLUG.into(),
        sender: sender.into(),
        recipient: None,
        kind: kind.into(),
        text: text.into(),
        ts,
    }
}

fn system(id: i64, sender: &str, ts: f64, text: &str) -> symphony_board::Message {
    message(id, "system", sender, ts, text)
}

/// The pictures' thread: claimed after 41 m, two review rounds, the first sent back, the second
/// approved by the project's review gate, which leaves the task in review waiting to land.
fn thread() -> Vec<symphony_board::Message> {
    vec![
        system(1, "glacial-merlin", MADE, "created by glacial-merlin"),
        system(2, "quiet-wren", 1_794_382_980.0, "quiet-wren claimed it"),
        system(
            3,
            "quiet-wren",
            1_794_389_700.0,
            "quiet-wren moved it in_progress → review",
        ),
        system(
            4,
            "lucid-heron",
            1_794_390_780.0,
            "lucid-heron joined as reviewer",
        ),
        message(
            5,
            "verdict",
            "lucid-heron",
            1_794_392_820.0,
            "changes: the verify keeps running after the branch moves",
        ),
        system(
            6,
            "lucid-heron",
            1_794_392_880.0,
            "lucid-heron released reviewer (released)",
        ),
        message(7, "msg", "quiet-wren", 1_794_393_000.0, "on it"),
        system(
            8,
            "quiet-wren",
            1_794_397_020.0,
            "quiet-wren moved it in_progress → review",
        ),
        system(
            9,
            "lucid-heron",
            1_794_397_260.0,
            "lucid-heron joined as reviewer",
        ),
    ]
}

fn facts(status: &str) -> Facts<'_> {
    Facts {
        status,
        created: MADE,
        updated: 1_794_397_260.0,
    }
}

/// A run `n` of the task: queued, started and ended when given, its exit code if it has one.
fn record(n: u32, phase: Phase, label: &str, times: (f64, Option<f64>, Option<f64>)) -> Record {
    let (queued, started, ended) = times;
    Record {
        version: 1,
        operation_id: format!("{n:032x}"),
        parent_id: None,
        phase,
        label: label.into(),
        state: match (started, ended) {
            (_, Some(_)) => Step::Finished,
            (Some(_), None) => Step::Running,
            (None, None) => Step::Queued,
        },
        handle: Some("quiet-wren".into()),
        task: Some(SLUG.into()),
        checkout: "/work/tools-verify-stops-when-moved".into(),
        branch: Some(SLUG.into()),
        commit: None,
        machine_id: None,
        host: "studio".into(),
        pid: 10,
        child_pid: started.map(|_| 11),
        queued_at: queued,
        started_at: started,
        ended_at: ended,
        beat_at: ended.or(started).unwrap_or(queued),
        exit_code: ended.map(|_| 0),
        signal: None,
        reason: String::new(),
    }
}

fn runs() -> Vec<Record> {
    let with = |mut record: Record, commit: &str, by: Option<&str>| {
        record.commit = Some(commit.into());
        record.handle = by.map(str::to_string);
        record
    };
    let wren = Some("quiet-wren");
    vec![
        with(
            record(
                1,
                Phase::Build,
                "just lint-rs",
                (
                    1_794_385_680.0,
                    Some(1_794_385_860.0),
                    Some(1_794_386_100.0),
                ),
            ),
            "9ac5149",
            wren,
        ),
        // Kept out of order: the timeline orders runs by when they were queued.
        with(
            record(
                3,
                Phase::Test,
                "just verify",
                (
                    1_794_394_800.0,
                    Some(1_794_394_920.0),
                    Some(1_794_396_300.0),
                ),
            ),
            "c3d2e1f",
            wren,
        ),
        with(
            record(
                2,
                Phase::Test,
                "just verify",
                (
                    1_794_387_120.0,
                    Some(1_794_387_480.0),
                    Some(1_794_388_920.0),
                ),
            ),
            "9ac5149",
            wren,
        ),
        with(
            record(
                4,
                Phase::Test,
                "certificate verify",
                (
                    1_794_398_700.0,
                    Some(1_794_399_000.0),
                    Some(1_794_400_440.0),
                ),
            ),
            "e19d3a0",
            None,
        ),
        with(
            record(
                5,
                Phase::Test,
                "certificate verify",
                (1_794_401_520.0, Some(1_794_402_120.0), None),
            ),
            "4be91c2",
            None,
        ),
    ]
}

fn gate(name: &str, state: GateState, by: Option<&str>, at: Option<f64>, detail: &str) -> Gate {
    Gate {
        name: name.into(),
        state,
        by: by.map(str::to_string),
        at,
        detail: detail.into(),
        ..Gate::default()
    }
}

fn past(state: GateState, at: f64, by: Option<&str>, detail: &str) -> GatePast {
    GatePast {
        state,
        at,
        by: by.map(str::to_string),
        commit: None,
        detail: detail.into(),
    }
}

/// The pictures' gates, as the project's plugin answers them: the certificate met, then missing
/// again once main moved; the dependency waived; the nightly met before the task; review met.
fn gates() -> Vec<Gate> {
    vec![
        Gate {
            past: vec![
                past(
                    GateState::Missing,
                    1_794_398_700.0,
                    Some("studio"),
                    "Verify on the merge tree e19d3a0 running",
                ),
                past(
                    GateState::Met,
                    1_794_400_440.0,
                    Some("studio"),
                    "Verify on the merge tree e19d3a0 green",
                ),
            ],
            ..gate(
                "certificate",
                GateState::Missing,
                Some("studio"),
                Some(1_794_401_400.0),
                "Main moved: verify on the merge tree 4be91c2 running",
            )
        },
        Gate {
            past: vec![past(
                GateState::Missing,
                1_794_370_000.0,
                None,
                "Waits on tools/slot-pool-by-tree",
            )],
            ..gate(
                "dependency",
                GateState::Waived,
                Some("cobalt-otter"),
                Some(1_794_399_120.0),
                "Waits on tools/slot-pool-by-tree, waived",
            )
        },
        gate(
            "nightly",
            GateState::Met,
            Some("nightly"),
            Some(1_794_363_240.0),
            "Night of 11-10 green on main a71f0d3",
        ),
        Gate {
            past: vec![past(
                GateState::Missing,
                1_794_392_820.0,
                Some("lucid-heron"),
                "Changes asked in round 1: the verify keeps running after the branch moves",
            )],
            ..gate(
                "review",
                GateState::Met,
                Some("lucid-heron"),
                Some(1_794_398_700.0),
                "Approved in round 2 of 2",
            )
        },
    ]
}

#[test]
fn the_pictures_task_gives_the_timeline_the_pictures_draw() {
    let built = build(
        SLUG,
        facts("review"),
        &thread(),
        &runs(),
        // In the answer's order, what the task waits on first: lanes by name all the same.
        &gates().into_iter().rev().collect::<Vec<_>>(),
        NOW,
        7200,
    );
    let fixture: TaskTimeline = serde_json::from_str(FIXTURE).unwrap();
    assert_eq!(built.stages, fixture.stages);
    assert_eq!(built.claims, fixture.claims);
    assert_eq!(built.rounds, fixture.rounds);
    assert_eq!(built.runs, fixture.runs);
    assert_eq!(built.gates, fixture.gates);
    assert_eq!(built, fixture);
}

#[test]
fn with_no_run_kept_for_the_task_it_has_no_runs_and_with_no_gates_no_lanes() {
    let built = build(SLUG, facts("review"), &thread(), &[], &[], NOW, 0);
    assert_eq!(built.runs, Vec::new());
    assert_eq!(built.gates, Vec::new());
    assert_eq!(
        (built.machine_seconds(), built.queued_seconds()),
        (0.0, 0.0)
    );
    // With no review gate to approve it, round 2 goes on: no verdict, no time waiting to land.
    let last = built.rounds.last().unwrap();
    assert_eq!((last.number, last.to, last.verdict), (2, None, None));
    assert_eq!(last.taken, Some(1_794_397_260.0));
    assert!(built.stages.iter().all(|stage| !stage.approved));
    assert_eq!(built.stages.len(), 5);
}

#[test]
fn a_step_inside_a_recorded_run_is_its_runs_and_each_end_reads_as_its_result() {
    let mut verify = record(
        1,
        Phase::Test,
        "just verify",
        (at(0.0), Some(at(1.0)), Some(at(30.0))),
    );
    verify.exit_code = Some(101);
    let mut lint = record(
        2,
        Phase::Build,
        "just lint-rs",
        (at(2.0), Some(at(2.0)), Some(at(5.0))),
    );
    lint.parent_id = Some(verify.operation_id.clone());
    // A parent the task's runs do not hold: the step stands on its own.
    let mut orphan = record(
        3,
        Phase::Build,
        "cargo build",
        (at(40.0), Some(at(41.0)), Some(at(42.0))),
    );
    orphan.parent_id = Some(format!("{:032x}", 99));
    orphan.exit_code = None;
    orphan.signal = Some(9);
    let mut lost = record(
        4,
        Phase::Test,
        "just test-rs",
        (at(50.0), None, Some(at(51.0))),
    );
    lost.exit_code = None;
    let queued = record(5, Phase::Test, "just test-rs", (at(60.0), None, None));
    let built = build(
        SLUG,
        facts("review"),
        &thread(),
        &[lint, verify, orphan, lost, queued],
        &[],
        NOW,
        0,
    );
    let read: Vec<(&str, RunResult)> = built
        .runs
        .iter()
        .map(|run| (run.label.as_str(), run.result))
        .collect();
    assert_eq!(
        read,
        [
            ("just verify", RunResult::Red),
            ("cargo build", RunResult::Red),
            ("just test-rs", RunResult::Unknown),
            ("just test-rs", RunResult::Queued),
        ]
    );
    assert_eq!(built.runs[0].phase, "test");
    assert_eq!(built.runs[1].phase, "build");
}

/// A round as (number, taken, to, by, verdict).
type Said<'a> = (
    u32,
    Option<f64>,
    Option<f64>,
    Option<&'a str>,
    Option<Verdict>,
);

#[test]
fn a_board_approval_closes_its_round_and_a_review_left_without_a_verdict_has_none() {
    let mut lines = thread();
    lines.truncate(6);
    // The changes verdict came 205 minutes in, the reviewer let go a minute later.
    lines.extend([
        // Sent back by its executor, with no verdict: the round ends there.
        system(
            7,
            "quiet-wren",
            at(220.0),
            "quiet-wren moved it in_progress → review",
        ),
        system(
            8,
            "quiet-wren",
            at(230.0),
            "quiet-wren moved it review → in_progress",
        ),
        system(
            9,
            "quiet-wren",
            at(240.0),
            "quiet-wren moved it in_progress → review",
        ),
        system(10, "moss", at(245.0), "moss joined as reviewer"),
        message(11, "verdict", "moss", at(260.0), "approve: it holds"),
    ]);
    let built = build(SLUG, facts("done"), &lines, &[], &gates(), NOW, 0);
    let rounds: Vec<Said<'_>> = built
        .rounds
        .iter()
        .map(|r| (r.number, r.taken, r.to, r.by.as_deref(), r.verdict))
        .collect();
    assert_eq!(
        rounds,
        [
            (
                1,
                Some(1_794_390_780.0),
                Some(1_794_392_820.0),
                Some("lucid-heron"),
                Some(Verdict::Changes)
            ),
            (2, None, Some(at(230.0)), None, None),
            (
                3,
                Some(at(245.0)),
                Some(at(260.0)),
                Some("moss"),
                Some(Verdict::Approved)
            ),
        ]
    );
    // The verdict closed it: no stretch waits to land, and the task is done at the verdict.
    assert!(built.stages.iter().all(|stage| !stage.approved));
    let last = built.stages.last().unwrap();
    assert_eq!(
        (last.status.as_str(), last.from, last.by.as_deref()),
        ("done", at(260.0), Some("moss"))
    );
    assert_eq!(built.closed, Some(at(260.0)));
    assert_eq!(built.bounces, 2);
}

#[test]
fn a_reviewer_holding_the_role_takes_the_round_up_at_once_and_claims_follow_hand_overs() {
    let lines = vec![
        system(1, "amber", MADE, "created by amber"),
        system(2, "teal", at(10.0), "teal claimed it"),
        system(3, "plum", at(15.0), "plum joined as reviewer"),
        system(
            4,
            "jade",
            at(20.0),
            "jade claimed it, handed over from teal (lost)",
        ),
        system(5, "jade", at(30.0), "jade moved it in_progress → review"),
        system(6, "jade", at(40.0), "jade released every role (left)"),
    ];
    let facts = Facts {
        status: "review",
        created: MADE,
        updated: at(40.0),
    };
    let built = build(SLUG, facts, &lines, &[], &[], at(60.0), 0);
    assert_eq!(
        built.claims,
        vec![
            Claim {
                by: "teal".into(),
                from: at(10.0),
                to: Some(at(20.0)),
            },
            Claim {
                by: "jade".into(),
                from: at(20.0),
                to: Some(at(30.0)),
            },
        ]
    );
    let round = &built.rounds[0];
    assert_eq!(
        (round.taken, round.to, round.by.as_deref()),
        (Some(at(30.0)), None, Some("plum"))
    );
    assert_eq!(built.first_claimed, Some(at(10.0)));
}

#[test]
fn a_gates_states_start_no_earlier_than_the_task_and_one_with_no_time_follows_the_one_before() {
    let lane_of = |gate: Gate| lane(&gate, at(0.0));
    let spans = |lane: GateLane| -> Vec<(GateState, f64)> {
        lane.states.into_iter().map(|s| (s.state, s.from)).collect()
    };
    // Two states before the task: only the later one shows, from the task's creation.
    let early = Gate {
        past: vec![
            past(GateState::Missing, at(-90.0), None, ""),
            past(GateState::Met, at(-30.0), None, ""),
        ],
        ..gate("nightly", GateState::Missing, None, Some(at(20.0)), "")
    };
    assert_eq!(
        spans(lane_of(early)),
        [(GateState::Met, at(0.0)), (GateState::Missing, at(20.0))]
    );
    // No time at all: from the task's creation; after a past, from where the past began.
    assert_eq!(
        spans(lane_of(gate("check", GateState::Unknown, None, None, ""))),
        [(GateState::Unknown, at(0.0))]
    );
    let untimed = Gate {
        past: vec![past(GateState::Missing, at(10.0), None, "")],
        ..gate("check", GateState::Met, None, None, "")
    };
    assert_eq!(spans(lane_of(untimed)), [(GateState::Met, at(10.0))]);
    // A past written out of order is read no earlier than the state before it.
    let crossed = Gate {
        past: vec![
            past(GateState::Missing, at(30.0), None, ""),
            past(GateState::Met, at(20.0), None, ""),
        ],
        ..gate("check", GateState::Missing, None, Some(at(50.0)), "")
    };
    assert_eq!(
        spans(lane_of(crossed)),
        [(GateState::Met, at(30.0)), (GateState::Missing, at(50.0))]
    );
    let kept = lane_of(gates().remove(1));
    assert_eq!(
        kept.states[1],
        GateSpan {
            state: GateState::Waived,
            from: 1_794_399_120.0,
            by: Some("cobalt-otter".into()),
            detail: "Waits on tools/slot-pool-by-tree, waived".into(),
        }
    );
}

#[test]
#[expect(
    clippy::cast_precision_loss,
    reason = "a time in whole seconds since the epoch is exact as an f64"
)]
fn the_offset_turns_a_time_into_this_machines_local_minute() {
    // A mid-January noon, far from any change of clocks, and a mid-July one.
    let noon: i64 = 1_800_000_000 - 1_800_000_000 % 86_400 + 12 * 3600;
    for (minute, seconds) in [(noon, 0), (noon, 59), (noon + 86_400 * 180, 30)] {
        let now = (minute + seconds) as f64;
        let local = crate::clock::stamp_at(minute + seconds);
        let minutes = crate::clock::stamp_minutes(&local).unwrap();
        assert_eq!(minutes * 60, minute + offset(now), "{local}");
        assert_eq!(offset(now) % 60, 0);
    }
}
