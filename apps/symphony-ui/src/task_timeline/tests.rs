//! The timeline's layout, pure, from the approved pictures' task (the API's fixture) and cases
//! written out by hand; and its views rendered on the host.

use super::*;
use symphony_api::timeline::{Claim, GateLane, GateSpan, Offset, Round};

const FIXTURE: &str = include_str!("../../../../crates/symphony-api/fixtures/task-timeline.json");
const CSS: &str = include_str!("../../assets/task-timeline.css");

/// The approved pictures' task: two review rounds, a build and its tests, one running now, a gate
/// met then missing again.
fn pictures() -> TaskTimeline {
    serde_json::from_str(FIXTURE).unwrap()
}

/// `minutes` after the pictures' task was made (09:02 on their clock).
fn at(minutes: f64) -> f64 {
    pictures().created + minutes * 60.0
}

fn html(timeline: TaskTimeline) -> String {
    fn page(timeline: TaskTimeline) -> Element {
        rsx! { TimelineView { timeline: Rc::new(timeline), slug: "tools/verify-stops-when-moved" } }
    }
    let mut dom = VirtualDom::new_with_props(page, timeline);
    dom.rebuild_in_place();
    dioxus_ssr::render(&dom)
}

/// The words of `fits` drawn on a track `px` wide.
fn shown(fits: &[Fit], px: u32) -> String {
    fits.iter()
        .filter(|fit| fit.from.is_none_or(|from| from <= px) && fit.to.is_none_or(|to| px < to))
        .map(|fit| fit.text.as_str())
        .collect::<Vec<_>>()
        .join("|")
}

fn run(phase: &str, label: &str, by: Option<&str>, times: (f64, f64, Option<f64>)) -> Run {
    let (queued, started, ended) = times;
    Run {
        phase: phase.into(),
        label: label.into(),
        by: by.map(str::to_string),
        commit: Some("0123456789abcdef".into()),
        host: "studio".into(),
        queued,
        started: Some(started),
        ended,
        result: if ended.is_some() {
            RunResult::Green
        } else {
            RunResult::Running
        },
    }
}

#[test]
fn a_length_reads_to_the_minute_in_hours_and_days_and_never_below_nothing() {
    let cases = [
        (-5.0, "0 m"),
        (0.0, "0 m"),
        (29.0, "0 m"),
        (31.0, "1 m"),
        (2460.0, "41 m"),
        (3580.0, "1 h"),
        (6720.0, "1 h 52 m"),
        (22_680.0, "6 h 18 m"),
        (86_400.0, "1 d"),
        (2.0 * 86_400.0 + 3.0 * 3600.0 + 20.0 * 60.0, "2 d 3 h"),
    ];
    for (seconds, said) in cases {
        assert_eq!(took(seconds), said, "{seconds} s");
    }
}

#[test]
fn a_time_reads_on_the_servers_local_clock_and_its_day_by_the_calendar() {
    let midnight = 20_768.0 * 86_400.0;
    assert_eq!(clock(midnight, 0), "00:00");
    assert_eq!(clock(midnight - 1.0, 0), "23:59");
    assert_eq!(clock(midnight, 7200), "02:00");
    assert_eq!(clock(midnight, -60), "23:59");
    assert_eq!(date(midnight - 1.0, 0), date(midnight - 86_400.0, 0));
    assert_ne!(date(midnight, -60), date(midnight, 0));
    assert_eq!(civil(0), (1970, 1, 1));
    for (year, month, day) in [(2026, 11, 11), (2024, 2, 29), (2000, 3, 1), (1969, 12, 31)] {
        let minutes = crate::words::Moment::from_parts(year, month, day, 0, 0)
            .unwrap()
            .minutes();
        assert_eq!(
            civil(minutes.div_euclid(1440)),
            (year, i64::from(month), i64::from(day))
        );
    }
    let pictures = pictures();
    assert_eq!(clock_of(&pictures, pictures.created), "09:02");
    assert_eq!(date_of(&pictures, pictures.created), "11-11");
}

#[test]
fn each_time_reads_at_the_offset_its_own_instant_had_across_a_change_of_summer_time() {
    // The clock went back an hour at 12:00 on the pictures' clock (10:00 UTC).
    let change = at(178.0);
    let timeline = TaskTimeline {
        zone: vec![
            Offset {
                from: pictures().created,
                seconds: 7200,
            },
            Offset {
                from: change,
                seconds: 3600,
            },
        ],
        ..pictures()
    };
    assert_eq!(clock_of(&timeline, timeline.created), "09:02");
    assert_eq!(clock_of(&timeline, change - 60.0), "11:59");
    assert_eq!(clock_of(&timeline, change), "11:00");
    let rows = rows(&timeline);
    let at: Vec<_> = rows.iter().map(|row| row.at.as_str()).collect();
    assert_eq!(
        at[..6],
        ["09:02", "09:43", "10:28", "10:52", "11:35", "11:27"],
        "the second round's changes came after the change"
    );
    assert_eq!(lanes(&timeline).now.unwrap().label, "now 14:20");
}

#[test]
fn the_pictures_task_heads_its_timeline_with_its_totals() {
    let (first, machine) = totals(&pictures());
    assert_eq!(first, "6 h 18 m so far · 2 review rounds · 1 bounce");
    assert_eq!(machine.as_deref(), Some("1 h 33 m, queued 26 m"));
    let closed = TaskTimeline {
        closed: Some(pictures().now),
        rounds: Vec::new(),
        bounces: 0,
        runs: Vec::new(),
        ..pictures()
    };
    assert_eq!(
        totals(&closed),
        (
            "6 h 18 m in all · 0 review rounds · 0 bounces".to_string(),
            None
        )
    );
    assert_eq!(kept_note(&pictures()), None);
    let rotated = TaskTimeline {
        runs_from: Some(at(83.0)),
        ..pictures()
    };
    assert_eq!(
        kept_note(&rotated).as_deref(),
        Some("Builds and tests before 11-11 10:25 are no longer kept.")
    );
}

/// Each row as (at, label, took, word, under its stage).
fn said(rows: &[Row]) -> Vec<(String, String, String, String, bool)> {
    rows.iter()
        .map(|row| {
            (
                row.at.clone(),
                row.label.clone(),
                row.took.clone().unwrap_or_default(),
                row.word
                    .map(|(word, _)| word)
                    .unwrap_or_default()
                    .to_string(),
                row.sub,
            )
        })
        .collect()
}

fn owned(rows: &[(&str, &str, &str, &str, bool)]) -> Vec<(String, String, String, String, bool)> {
    rows.iter()
        .map(|(at, label, took, word, sub)| {
            (
                (*at).into(),
                (*label).into(),
                (*took).into(),
                (*word).into(),
                *sub,
            )
        })
        .collect()
}

#[test]
fn the_phone_list_reads_as_the_approved_picture_each_run_and_gate_change_under_its_stage() {
    let rows = rows(&pictures());
    assert_eq!(
        said(&rows),
        owned(&[
            ("09:02", "Ready, unclaimed", "41 m", "", false),
            ("09:43", "In progress · quiet-wren", "1 h 52 m", "", false),
            // A build that went green says no more than that it was built.
            ("10:28", "Build · queued 3 m", "4 m", "", true),
            (
                "10:52",
                "Test, just verify · queued 6 m",
                "24 m",
                "green",
                true
            ),
            (
                "11:35",
                "Review, round 1 · lucid-heron",
                "52 m",
                "changes",
                false
            ),
            ("12:27", "In progress again", "1 h 10 m", "", false),
            (
                "13:00",
                "Test, just verify · queued 2 m",
                "23 m",
                "green",
                true
            ),
            (
                "13:37",
                "Review, round 2 · lucid-heron",
                "28 m",
                "approved",
                false
            ),
            (
                "14:05",
                "Approved, waiting to land",
                "1 h 15 m",
                "now",
                false
            ),
            // The machine's own run, nobody's: what it runs and the commit it ran at.
            (
                "14:05",
                "Certificate verify · e19d3a0",
                "24 m",
                "green",
                true
            ),
            ("14:12", "Dependency waived · cobalt-otter", "", "", true),
            // The certificate met at 14:34 is the green run's row: no row of its own. Missing
            // again, it says why.
            ("14:50", "Main moved: certificate again", "", "", true),
            (
                "14:52",
                "Certificate verify · 4be91c2",
                "18 m",
                "running",
                true
            ),
        ])
    );
    // One day: no day marks.
    assert!(rows.iter().all(|row| row.day.is_none()));
    // The longest step, in progress for 1 h 52 m, fills the bar's span; the rest are as long as
    // their time.
    let bar = |index: usize| rows[index].bar.clone().unwrap();
    assert_eq!(bar(1).work, BAR_SPAN);
    assert!((bar(0).work - 41.0 / 112.0 * BAR_SPAN).abs() < 1e-9);
    // A review's bar waits for its reviewer, lighter, then works.
    let round = bar(4);
    assert!(
        (round.wait - 18.0 / 112.0 * BAR_SPAN).abs() < 1e-9,
        "{round:?}"
    );
    assert!(
        (round.work - 34.0 / 112.0 * BAR_SPAN).abs() < 1e-9,
        "{round:?}"
    );
    // Waiting to land is all wait; what runs now is striped after its queue; a gate change is an
    // instant.
    assert_eq!((bar(8).work, bar(8).wait > 0.0), (0.0, true));
    assert!(bar(12).running && !bar(9).running);
    assert_eq!(bar(12).wait_tone, "primary");
    assert!(rows[10].event && rows[10].bar.is_none());
    assert_eq!(
        rows[11].title,
        "Main moved: verify on the merge tree 4be91c2 running"
    );
    assert_eq!(rows[12].tone, "primary");
    assert_eq!(rows[3].tone, "like");
    assert!(rows[9].title.contains("queued 5 m"), "{}", rows[9].title);
}

#[test]
fn a_gate_change_says_why_only_from_a_short_reason_and_shows_a_met_no_run_said() {
    assert_eq!(
        reason("Main moved: verify on the merge tree 4be91c2 running"),
        Some("Main moved")
    );
    assert_eq!(reason("Verify on the merge tree e19d3a0 running"), None);
    assert_eq!(reason(": nothing"), None);
    assert_eq!(
        reason("the branch moved on and on again: rerun"),
        None,
        "more than three words is no short reason"
    );
    // No run went green when the certificate was met: its row says so.
    let timeline = TaskTimeline {
        runs: Vec::new(),
        ..pictures()
    };
    let labels: Vec<_> = gate_changes(&timeline)
        .into_iter()
        .map(|row| row.label)
        .collect();
    assert_eq!(
        labels,
        [
            "Certificate met",
            "Main moved: certificate again",
            "Dependency waived · cobalt-otter"
        ]
    );
    // Missing again with no reason to give.
    let mut timeline = pictures();
    timeline.gates[0].states[2].detail = "Verify on the merge tree 4be91c2 running".into();
    assert_eq!(
        gate_changes(&timeline)[0].label,
        "Certificate missing again"
    );
}

#[test]
fn a_stage_left_within_its_first_half_minute_draws_no_0_m_row() {
    let mut timeline = pictures();
    // Moved to review and back within twenty seconds, before the first claim.
    timeline.stages.insert(
        1,
        Stage {
            status: "blocked".into(),
            from: at(41.0),
            to: Some(at(41.0) + 20.0),
            by: Some("cobalt-otter".into()),
            ..Stage::default()
        },
    );
    timeline.stages[2].from = at(41.0) + 20.0;
    let rows = rows(&timeline);
    assert!(
        rows.iter().all(|row| row.took.as_deref() != Some("0 m")),
        "{:?}",
        said(&rows)
    );
    assert!(rows.iter().all(|row| !row.label.starts_with("Blocked")));
    // The one stage going on now keeps its row however short.
    let now = pictures().now;
    let fresh = TaskTimeline {
        created: now - 10.0,
        stages: vec![Stage {
            status: "ready".into(),
            from: now - 10.0,
            ..Stage::default()
        }],
        rounds: Vec::new(),
        runs: Vec::new(),
        gates: Vec::new(),
        claims: Vec::new(),
        first_claimed: None,
        ..pictures()
    };
    assert_eq!(
        said(&super::rows(&fresh)),
        owned(&[("15:19", "Ready, unclaimed", "0 m", "now", false)])
    );
}

#[test]
fn with_no_run_kept_there_is_no_run_row_no_machine_time_and_no_runs_lane() {
    let timeline = TaskTimeline {
        runs: Vec::new(),
        ..pictures()
    };
    let rows = rows(&timeline);
    assert!(
        rows.iter()
            .all(|row| !row.label.starts_with("Build") && !row.label.starts_with("Test")),
        "{:?}",
        said(&rows)
    );
    assert_eq!(rows.len(), 9, "six stages and three gate changes");
    assert_eq!(totals(&timeline).1, None);
    assert!(lanes(&timeline).lanes.iter().all(|lane| lane.key != "runs"));
    let html = html(timeline);
    assert!(!html.contains("achine"), "{html}");
    assert!(!html.contains("data-lane=\"runs\""), "{html}");
    assert!(!html.contains("tl-running"), "{html}");
    assert!(!html.contains("running now"), "{html}");
}

#[test]
fn a_timeline_over_two_days_marks_each_days_first_row() {
    let pictures = pictures();
    // Made at 23:30 the day before, ready until 09:43.
    let made = pictures.created - (9.0 * 3600.0 + 32.0 * 60.0);
    let mut timeline = TaskTimeline {
        created: made,
        ..pictures
    };
    timeline.stages[0].from = made;
    let rows = rows(&timeline);
    let days: Vec<_> = rows
        .iter()
        .map(|row| (row.at.as_str(), row.day.as_deref()))
        .filter(|(_, day)| day.is_some())
        .collect();
    assert_eq!(days, [("23:30", Some("11-10")), ("09:43", Some("11-11"))]);
    assert_eq!(rows[0].label, "Ready, unclaimed");
    assert_eq!(rows[0].took.as_deref(), Some("10 h 13 m"));
}

#[test]
fn a_stage_says_who_moved_it_only_where_its_name_says_nothing_else() {
    let pictures = pictures();
    let stage = |status: &str, by: &str| Stage {
        status: status.into(),
        from: pictures.created,
        to: Some(pictures.created + 600.0),
        by: Some(by.into()),
        ..Stage::default()
    };
    let timeline = TaskTimeline {
        first_claimed: Some(pictures.created + 300.0),
        stages: vec![
            stage("ready", "glacial-merlin"),
            stage("blocked", "cobalt-otter"),
            Stage {
                to: None,
                ..stage("done", "lucid-heron")
            },
        ],
        rounds: Vec::new(),
        runs: Vec::new(),
        gates: Vec::new(),
        closed: Some(pictures.created + 600.0),
        ..pictures
    };
    let rows = rows(&timeline);
    assert_eq!(
        said(&rows),
        owned(&[
            ("09:02", "Ready", "10 m", "", false),
            ("09:02", "Blocked · cobalt-otter", "10 m", "", false),
            ("09:02", "Done · lucid-heron", "", "", false),
        ])
    );
    assert_eq!((rows[1].tone, rows[2].tone), ("dislike", "like"));
    assert!(rows[2].bar.is_none(), "a close is an instant");
    assert!(lanes(&timeline).now.is_none(), "a closed task has no now");
}

#[test]
fn a_word_is_drawn_only_at_the_track_widths_its_room_holds_it() {
    // Room enough at every width, at some, at none.
    assert_eq!(width_for("41 m", 50.0, PAD_PX), Some(None));
    assert_eq!(width_for("ready 41 m", 10.0, PAD_PX), Some(Some(750)));
    assert_eq!(width_for("ready 41 m", 0.5, PAD_PX), None);
    assert_eq!(width_for("", 50.0, PAD_PX), None);
    assert_eq!(width_for("41 m", 0.0, PAD_PX), None);
    // The long words where they fit, the short below that, nothing below the short's own.
    let fits = fitted("ready 41 m", "41 m", 10.0);
    assert_eq!(shown(&fits, 1280), "ready 41 m");
    assert_eq!(shown(&fits, 750), "ready 41 m");
    assert_eq!(shown(&fits, 700), "41 m");
    assert_eq!(shown(&fits, 400), "41 m");
    assert_eq!(shown(&fits, 350), "");
    assert_eq!(fits[1].class(), "tl-fit tl-from-400 tl-to-750");
    // A short word no narrower than the long one is never drawn beside it.
    assert_eq!(fitted("41 m", "41 m", 10.0).len(), 1);
    // The last label may run into the lanes' end margin.
    let last = Room {
        room: 1.0,
        last: true,
    };
    assert_eq!(label("running 18 m".into(), last).unwrap().from, None);
    assert_eq!(last.css(), "calc(1.00% + var(--end))");
    let between = Room {
        room: 1.0,
        last: false,
    };
    assert_eq!(label("running 18 m".into(), between), None);
    assert_eq!(between.css(), "1.00%");
}

#[test]
fn the_wide_lanes_read_as_the_approved_picture_at_its_width_and_mark_the_hours_and_now() {
    let lanes = lanes(&pictures());
    let ticks: Vec<_> = lanes.ticks.iter().map(|tick| tick.label.as_str()).collect();
    assert_eq!(
        ticks,
        [
            "09:00", "10:00", "11:00", "12:00", "13:00", "14:00", "15:00"
        ]
    );
    assert!(lanes.ticks[0].left.abs() < 1e-9);
    assert!((lanes.ticks[6].left - 360.0 / 380.0 * 100.0).abs() < 1e-9);
    let now = lanes.now.clone().unwrap();
    assert_eq!((now.left, now.label.as_str()), (100.0, "now 15:20"));
    let lane = |key: &str| lanes.lanes.iter().find(|lane| lane.key == key).unwrap();
    // The words of a lane on a track `px` wide: the picture's is 950.
    let texts = |key: &str, px: u32| -> Vec<String> {
        lane(key)
            .segs
            .iter()
            .map(|seg| shown(&seg.text, px))
            .collect()
    };
    assert_eq!(
        lanes
            .lanes
            .iter()
            .map(|lane| (lane.name, lane.who.as_str(), lane.rows))
            .collect::<Vec<_>>(),
        [
            ("Stage", "", 1),
            ("Executor", "quiet-wren", 1),
            ("Reviewer", "lucid-heron", 1),
            ("Builds & tests", "studio", 1),
        ]
    );
    assert_eq!(
        texts("stage", 950),
        [
            "ready 41 m",
            "in progress 1 h 52 m",
            "review 52 m",
            "in progress 1 h 10 m",
            "28 m",
            "waiting to land 1 h 15 m",
        ]
    );
    // At the narrowest the lanes are drawn (a 900 px page), the shorter words.
    assert_eq!(
        texts("stage", 600),
        [
            "41 m",
            "in progress 1 h 52 m",
            "review 52 m",
            "1 h 10 m",
            "28 m",
            "1 h 15 m"
        ]
    );
    assert_eq!(lane("stage").segs[5].look, Look::Waiting);
    assert_eq!(texts("executor", 950), ["claimed 1 h 52 m", "1 h 10 m"]);
    assert_eq!(texts("reviewer", 950), ["", "round 1", "", "round 2"]);
    assert_eq!(texts("reviewer", 600), ["", "", "", ""]);
    let after: Vec<_> = lane("reviewer")
        .segs
        .iter()
        .filter_map(|seg| {
            seg.after
                .as_ref()
                .map(|(word, tone, _)| (shown(std::slice::from_ref(word), 600), *tone))
        })
        .collect();
    assert_eq!(
        after,
        [
            ("changes".to_string(), "dislike"),
            ("approved".to_string(), "like")
        ]
    );
    let under: Vec<_> = lane("runs")
        .segs
        .iter()
        .filter_map(|seg| {
            seg.under
                .as_ref()
                .map(|(under, _)| shown(std::slice::from_ref(under), 950))
        })
        .collect();
    assert_eq!(
        under,
        [
            "build 4 m",
            "verify 24 m",
            "verify 23 m",
            "certificate 24 m",
            "running 18 m"
        ]
    );
    // On the narrowest track the build's label would meet the next run's: it is left out.
    assert_eq!(
        shown(&[lane("runs").segs[0].under.clone().unwrap().0], 600),
        ""
    );
    let looks: Vec<_> = lane("runs").segs.iter().map(|seg| seg.look).collect();
    assert_eq!(
        looks.iter().filter(|look| **look == Look::Running).count(),
        1
    );
    assert_eq!(looks.last(), Some(&Look::Running));
    let gates: Vec<_> = lanes
        .gates
        .iter()
        .map(|gate| {
            (
                gate.name.as_str(),
                gate.spans.iter().map(|span| span.state).collect::<Vec<_>>(),
            )
        })
        .collect();
    assert_eq!(
        gates,
        [
            (
                "certificate",
                vec![GateState::Missing, GateState::Met, GateState::Missing]
            ),
            ("dependency", vec![GateState::Missing, GateState::Waived]),
            ("nightly", vec![GateState::Met]),
            // From the move to review: not reviewed yet, changes asked, approved.
            (
                "review",
                vec![GateState::Missing, GateState::Missing, GateState::Met]
            ),
        ]
    );
    // The nightly, met before the task, holds from the task's making to now, and says so.
    let nightly = &lanes.gates[2].spans[0];
    assert!(
        (nightly.left - 2.0 / 380.0 * 100.0).abs() < 1e-9
            && (nightly.left + nightly.width - 100.0).abs() < 1e-9
    );
    assert_eq!(nightly.word.as_ref().unwrap().text, "met");
    assert!(
        nightly.title.starts_with("nightly met from 09:02: "),
        "{}",
        nightly.title
    );
    // Each step a stop for the keyboard (a round's wait and a run's queue are their step's), and
    // each gate's state.
    let parts = |key: &str| {
        let segs = &lane(key).segs;
        (segs.len(), segs.iter().filter(|seg| seg.stop).count())
    };
    assert_eq!(parts("reviewer"), (4, 2));
    assert_eq!(parts("runs"), (10, 5));
    assert_eq!(lanes.stops(), 6 + 2 + 2 + 5 + 3 + 2 + 1 + 3);
}

#[test]
fn runs_that_overlap_take_a_row_each_and_a_fourth_shares_the_row_that_frees_first() {
    let runs = [
        run("build", "just lint-rs", Some("a"), (0.0, 10.0, Some(100.0))),
        run("test", "just verify", Some("a"), (20.0, 30.0, Some(60.0))),
        run("test", "just verify", Some("b"), (40.0, 50.0, Some(90.0))),
        run("test", "just verify", Some("c"), (45.0, 50.0, Some(80.0))),
        // After the second's end: back in its row.
        run("test", "just verify", Some("d"), (70.0, 75.0, Some(95.0))),
    ];
    assert_eq!(run_rows(&runs, 200.0), (vec![0, 1, 2, 1, 1], RUN_ROWS));
    assert_eq!(run_rows(&runs[..1], 200.0), (vec![0], 1));
    assert_eq!(run_rows(&[], 200.0), (vec![], 1));
    // One after another: one row.
    let apart = [
        run("build", "just lint-rs", Some("a"), (0.0, 10.0, Some(20.0))),
        run("test", "just verify", Some("a"), (20.0, 30.0, Some(60.0))),
    ];
    assert_eq!(run_rows(&apart, 200.0).0, [0, 0]);
    // A label has room up to the next run in its own row.
    let timeline = TaskTimeline {
        runs: runs.to_vec(),
        ..pictures()
    };
    let lanes = lanes(&timeline);
    let lane = lanes.lanes.iter().find(|lane| lane.key == "runs").unwrap();
    assert_eq!(lane.rows, RUN_ROWS);
    let rows: Vec<_> = lane.segs.iter().map(|seg| seg.row).collect();
    assert_eq!(rows, [0, 0, 1, 1, 2, 2, 1, 1, 1, 1]);
}

#[test]
fn a_runs_label_says_its_result_unless_it_went_green() {
    let mut red = run("test", "just verify", Some("a"), (0.0, 1.0, Some(60.0)));
    red.result = RunResult::Red;
    assert_eq!(short(&red), "verify red");
    let mut queued = run("test", "just verify", Some("a"), (0.0, 1.0, None));
    queued.result = RunResult::Queued;
    assert_eq!(short(&queued), "verify queued");
    assert_eq!(
        short(&run("test", "just verify", None, (0.0, 1.0, None))),
        "running"
    );
    let mut build = run("build", "just lint-rs", Some("a"), (0.0, 1.0, Some(60.0)));
    assert_eq!(short(&build), "build");
    build.result = RunResult::Red;
    assert_eq!(short(&build), "build red");
    // A red build says so in the list too.
    let row = run_row(&build, &pictures(), &|_| 1.0);
    assert_eq!(row.word, Some(("red", "dislike")));
    assert_eq!(run_label(&build, 120.0), "Build · queued 2 m");
    let gate = run("test", "certificate verify", None, (0.0, 1.0, Some(60.0)));
    assert_eq!(run_label(&gate, 600.0), "Certificate verify · 0123456");
}

#[test]
fn a_long_timeline_marks_fewer_hours_and_a_midnight_by_its_date() {
    let pictures = pictures();
    let made = pictures.now - 30.0 * 3600.0;
    let mut timeline = TaskTimeline {
        created: made,
        ..pictures
    };
    timeline.stages[0].from = made;
    let ticks: Vec<_> = lanes(&timeline)
        .ticks
        .into_iter()
        .map(|tick| tick.label)
        .collect();
    assert_eq!(
        ticks,
        [
            "12:00", "16:00", "20:00", "11-11", "04:00", "08:00", "12:00"
        ]
    );
    // A step of days marks each midnight by its date.
    let made = pictures.now - 40.0 * 86_400.0;
    let mut timeline = TaskTimeline {
        created: made,
        ..timeline
    };
    timeline.stages[0].from = made;
    let ticks: Vec<_> = lanes(&timeline)
        .ticks
        .into_iter()
        .map(|tick| tick.label)
        .collect();
    assert_eq!(
        ticks,
        ["10-03", "10-10", "10-17", "10-24", "10-31", "11-07"]
    );
}

/// A week-long task of 24 review rounds, two minutes each, each sent back: the case the review
/// found the verdicts printed over each other.
fn week() -> TaskTimeline {
    let now = pictures().now;
    let made = now - 7.0 * 86_400.0;
    let mut stages = vec![Stage {
        status: "ready".into(),
        from: made,
        to: Some(made + 3600.0),
        ..Stage::default()
    }];
    let mut rounds = Vec::new();
    let mut from = made + 3600.0;
    for number in 1..=24 {
        let review = from + 6.0 * 3600.0;
        stages.push(Stage {
            status: "in_progress".into(),
            from,
            to: Some(review),
            ..Stage::default()
        });
        stages.push(Stage {
            status: "review".into(),
            from: review,
            to: Some(review + 120.0),
            round: number,
            ..Stage::default()
        });
        rounds.push(Round {
            number,
            from: review,
            taken: Some(review + 30.0),
            to: Some(review + 120.0),
            by: Some("lucid-heron".into()),
            verdict: Some(Verdict::Changes),
        });
        from = review + 120.0;
    }
    stages.push(Stage {
        status: "in_progress".into(),
        from,
        ..Stage::default()
    });
    TaskTimeline {
        created: made,
        first_claimed: Some(made + 3600.0),
        bounces: 24,
        stages,
        claims: vec![Claim {
            by: "quiet-wren".into(),
            from: made + 3600.0,
            to: None,
        }],
        rounds,
        runs: Vec::new(),
        gates: vec![GateLane {
            name: "review".into(),
            states: vec![GateSpan {
                state: GateState::Missing,
                from: made + 7.0 * 3600.0,
                by: None,
                detail: "Changes asked".into(),
            }],
        }],
        ..pictures()
    }
}

#[test]
fn a_week_of_short_rounds_prints_no_word_over_another() {
    let lanes = lanes(&week());
    let reviewer = lanes
        .lanes
        .iter()
        .find(|lane| lane.key == "reviewer")
        .unwrap();
    for px in [600, 950, 1600] {
        // No round's word drawn in its two minutes, and no verdict wider than its room.
        assert!(
            reviewer
                .segs
                .iter()
                .all(|seg| shown(&seg.text, px).is_empty())
        );
        for seg in &reviewer.segs {
            if let Some((word, _, room)) = &seg.after {
                let drawn = !shown(std::slice::from_ref(word), px).is_empty();
                let wide = word.text.chars().count() as f64 * CHAR_PX + GAP_PX;
                assert!(
                    !drawn || wide <= room.room / 100.0 * f64::from(px) + 0.5,
                    "{px}: {seg:?}"
                );
            }
        }
    }
    // Each stage's word only where it fits: two-minute reviews say nothing, six hours of work say
    // their length.
    let stage = lanes.lanes.iter().find(|lane| lane.key == "stage").unwrap();
    for seg in &stage.segs {
        let drawn = shown(&seg.text, 950);
        let wide = drawn.chars().count() as f64 * CHAR_PX + PAD_PX;
        assert!(
            drawn.is_empty() || wide <= seg.width / 100.0 * 950.0 + 0.5,
            "{seg:?}"
        );
    }
}

#[test]
fn the_host_render_draws_the_list_the_lanes_with_their_words_and_what_the_bars_mean() {
    let html = html(pictures());
    assert!(
        html.contains("6 h 18 m so far · 2 review rounds · 1 bounce"),
        "{html}"
    );
    assert!(html.contains(">Machine 1 h 33 m, queued 26 m<"), "{html}");
    assert!(
        html.contains("> · machine 1 h 33 m, queued 26 m<"),
        "{html}"
    );
    assert_eq!(html.matches("<li class=\"tl-row").count(), 13, "{html}");
    assert_eq!(
        html.matches("class=\"tl-row tl-sub tl-event\"").count(),
        2,
        "{html}"
    );
    for lane in ["stage", "executor", "reviewer", "runs"] {
        assert!(
            html.contains(&format!("data-lane=\"{lane}\"")),
            "{lane}: {html}"
        );
    }
    assert_eq!(
        html.matches("class=\"tl-lane tl-gate\"").count(),
        4,
        "{html}"
    );
    // What runs now: striped in the list and in its lane, and the legend says so.
    assert_eq!(html.matches("tl-work tl-running").count(), 1, "{html}");
    assert_eq!(html.matches("tl-seg tl-running").count(), 1, "{html}");
    assert!(html.contains("running now"), "{html}");
    assert!(html.contains(">now 15:20<"), "{html}");
    assert!(
        html.contains(">changes<") && html.contains(">approved<"),
        "{html}"
    );
    assert!(html.contains("class=\"tl-g tl-g-waived\""), "{html}");
    assert!(html.contains("--c:var(--question)"), "{html}");
    // The keyboard stops at the first part, its words read aloud; the rest by the arrow keys.
    assert_eq!(html.matches("tabindex=\"0\"").count(), 1, "{html}");
    assert_eq!(
        html.matches("tabindex=\"-1\"").count(),
        lanes(&pictures()).stops() - 1,
        "{html}"
    );
    assert!(
        html.contains("id=\"tl-stop-0\" tabindex=\"0\" role=\"img\" aria-label=\"ready 41 m, from 09:02 to 09:43\""),
        "{html}"
    );
    assert!(
        html.contains("aria-label=\"nightly met from 09:02: "),
        "{html}"
    );
    // A round's wait is drawn, not stopped at: its reviewer's time after it says all of it.
    assert!(
        html.contains("title=\"round 1: waited 18 m for a reviewer, then lucid-heron took 34 m, changes\" aria-hidden=\"true\""),
        "{html}"
    );
    // The list is a list of steps; the lanes a group; the gate states in the legend by their own
    // marks, the ones the timeline holds.
    assert!(
        html.contains("<ol class=\"tl-list\" aria-label=\"Steps\">"),
        "{html}"
    );
    assert!(
        html.contains("role=\"group\" aria-label=\"Lanes\""),
        "{html}"
    );
    // In the approved picture's order: met, missing, waived.
    let legend: Vec<_> = html
        .split(">gate ")
        .skip(1)
        .map(|after| after.split('<').next().unwrap())
        .collect();
    assert_eq!(legend, ["met", "missing", "waived"], "{html}");
    // No overflow is measured on the host: no "Show all". No part of the lanes has the focus: no
    // line for its detail.
    assert!(!html.contains("Show all"), "{html}");
    assert!(!html.contains("tl-detail"), "{html}");
    assert!(html.contains("id=\"tl-body\""), "{html}");
}

#[test]
fn the_style_sheet_shows_the_list_on_a_phone_and_the_lanes_from_900_px_in_theme_colours() {
    let (phone, wide) = CSS
        .split_once("@media (min-width: 900px)")
        .expect("one width switches the views");
    assert!(phone.contains(".tl-lanes { --name:9rem; --end:5.5rem; display:none;"));
    // Wide, the list stays for assistive technology, out of sight but not display:none.
    assert!(wide.contains(".tl-list { position:absolute; width:1px; height:1px; overflow:hidden; clip-path:inset(50%);"));
    assert!(!wide.contains(".tl-list { display:none; }"));
    assert!(wide.contains(".tl-lanes { display:block; }"));
    // A phone's text keeps clear of the + button's column.
    assert!(phone.contains("@media (max-width: 899.98px) {\n  .tl-drawn { padding-right:40px; }"));
    // A phone's clipped list is drawn from its end, the latest steps in view, and fades out at its
    // top; clipped lanes fade out at their foot.
    assert!(phone.contains(
        "  .tl-clipped .tl-body { display:flex; flex-direction:column; justify-content:flex-end; }\n  .tl-clipped .tl-list { flex:none; }\n  .tl-clipped .tl-body::after { top:0; bottom:auto; background:linear-gradient(var(--card), transparent); }\n}"
    ));
    assert!(phone.contains(".tl-clipped .tl-body::after { content:\"\"; position:absolute; left:0; right:0; bottom:0; height:28px; background:linear-gradient(transparent, var(--card)); pointer-events:none; }"));
    assert!(!wide.contains("tl-clipped"));
    // Lanes cut under a whole one hide their fade wide only: narrowed to a phone after a cut, the
    // list keeps its fade at the top.
    assert!(wide.contains(".tl-body[data-cut]::after { display:none; }"));
    assert!(!phone.contains("data-cut"));
    // "Show all" sits on the legend's line, the legend wrapping beside it, and the lanes' detail
    // reserves no line of its own.
    assert!(phone.contains(".tl-foot .tl-legend { flex:1 1 0; }"));
    assert!(!CSS.contains("min-height:1.3em"));
    // The gate states' marks are in the legend wide only, beside the lanes that draw them: a rule
    // as specific as the legend's items' own hides them on a phone.
    assert!(phone.contains(".tl-legend .tl-wide { display:none; }"));
    assert!(wide.contains(".tl-legend .tl-wide { display:inline-flex; }"));
    // A light page draws the segments' words in its own colour.
    assert!(CSS.contains(
        "@media not (prefers-color-scheme: dark) { .tl-seg { color:var(--foreground); } }"
    ));
    // Each width a word is weighed at shows it from there, and its shorter one below.
    for width in WIDTHS {
        assert!(
            CSS.contains(&format!(
                "@container (max-width: {}.5px) {{ .tl-from-{width} {{ display:none; }} }}",
                width - 1
            )),
            "{width}"
        );
        assert!(
            CSS.contains(&format!(
                "@container (min-width: {width}px) {{ .tl-to-{width} {{ display:none; }} }}"
            )),
            "{width}"
        );
    }
    // Every gate state has a pattern of its own besides its colour.
    for (state, pattern) in [
        ("missing", "dotted"),
        ("waived", "dashed"),
        ("held", "double"),
        ("unknown", "dotted"),
    ] {
        assert!(
            CSS.contains(&format!(".tl-g-{state}::before {{")) && CSS.contains(pattern),
            "{state}"
        );
    }
    // Every colour is a theme token (mixed in sRGB at most), so an imported theme colours the
    // timeline too.
    for written in ["#", "rgb(", "rgba(", "hsl("] {
        assert!(!CSS.contains(written), "a colour written out ({written})");
    }
}

#[test]
fn before_its_first_answer_the_host_draws_nothing_and_the_pages_placeholder_shows() {
    fn page() -> Element {
        rsx! { TimelineHost { slug: "tools/new", revision: 1.0 } }
    }
    let mut dom = VirtualDom::new(page);
    dom.rebuild_in_place();
    let html = dioxus_ssr::render(&dom);
    assert!(!html.contains("tl-drawn"), "{html}");
    assert!(!html.contains("Reading"), "{html}");
}

#[test]
fn a_task_with_nothing_but_its_making_draws_one_stage_and_no_other_lane() {
    let now = 1_794_403_200.0;
    let timeline = TaskTimeline {
        slug: "tools/new".into(),
        now,
        zone: vec![Offset {
            from: now - 120.0,
            seconds: 7200,
        }],
        created: now - 120.0,
        stages: vec![Stage {
            status: "ready".into(),
            from: now - 120.0,
            ..Stage::default()
        }],
        ..TaskTimeline::default()
    };
    let rows = rows(&timeline);
    assert_eq!(
        said(&rows),
        owned(&[("15:18", "Ready, unclaimed", "2 m", "now", false)])
    );
    let lanes = lanes(&timeline);
    assert_eq!(lanes.lanes.len(), 1);
    assert!(lanes.gates.is_empty());
    let html = html(timeline);
    assert!(
        html.contains("2 m so far · 0 review rounds · 0 bounces"),
        "{html}"
    );
    assert!(!html.contains("gate met"), "{html}");
}

#[test]
fn a_failed_read_says_its_reasons_first_line_as_a_sentence_never_a_stack() {
    assert_eq!(
        failed("Failed to fetch"),
        "The timeline could not be read: Failed to fetch."
    );
    assert_eq!(
        failed(" Board snapshot unavailable. "),
        "The timeline could not be read: Board snapshot unavailable."
    );
    assert_eq!(
        failed("TypeError: Failed to fetch\n    at window.fetch (shell.js:1:2)\n    at 0xe43e"),
        "The timeline could not be read: TypeError: Failed to fetch."
    );
    assert_eq!(failed(""), "The timeline could not be read.");
    assert_eq!(failed(" . "), "The timeline could not be read.");
}

#[test]
fn the_next_read_is_the_timelines_own_while_open_a_minute_after_a_failure_none_once_closed() {
    let every = symphony_api::timeline::TIMELINE_EVERY;
    let read = |timeline: TaskTimeline| next_read(Some(&Ok(Rc::new(timeline))));
    assert_eq!(
        read(TaskTimeline {
            every: 45,
            ..pictures()
        }),
        Some(45)
    );
    assert_eq!(
        read(TaskTimeline {
            every: 0,
            ..pictures()
        }),
        Some(every)
    );
    assert_eq!(
        read(TaskTimeline {
            closed: Some(pictures().now),
            ..pictures()
        }),
        None
    );
    assert_eq!(next_read(Some(&Err("Failed to fetch".into()))), Some(every));
    assert_eq!(next_read(None), None);
}

/// Wide lanes that overflow end under the last that fits whole, a pixel's rounding allowed; none
/// is cut when all fit, and none when not even the first does (the box clips as it can).
#[test]
fn lanes_that_overflow_end_under_the_last_that_fits_whole() {
    // The axis is 22 px, then Stage (to 58), Executor (to 94), Builds & tests (to 182), Gates.
    let bottoms = [58.0, 94.0, 182.0, 202.0, 222.0];
    assert_eq!(
        lane_edge(&bottoms, 90.0),
        Some(58.0),
        "Executor cut through"
    );
    assert_eq!(lane_edge(&bottoms, 94.0), Some(94.0));
    assert_eq!(lane_edge(&bottoms, 94.6), Some(94.0));
    assert_eq!(lane_edge(&bottoms, 95.0), Some(94.0), "a pixel's rounding");
    assert_eq!(
        lane_edge(&bottoms, 180.0),
        Some(94.0),
        "two pixels short of Builds & tests"
    );
    assert_eq!(lane_edge(&bottoms, 220.0), Some(202.0));
    assert_eq!(
        lane_edge(&bottoms, 221.0),
        None,
        "the last lane a pixel over fits"
    );
    assert_eq!(lane_edge(&bottoms, 222.0), None, "every lane fits");
    assert_eq!(lane_edge(&bottoms, 400.0), None);
    assert_eq!(
        lane_edge(&bottoms, 40.0),
        None,
        "not even the first lane fits"
    );
    assert_eq!(lane_edge(&[], 40.0), None, "no lanes");
}

/// A wide box keeps for the next visit what the tallest box would hold whole, so one kept short for
/// a task of few lanes grows back as the task gains lanes, and never past the tallest box's cut.
#[test]
fn a_wide_box_kept_short_grows_back_to_the_lanes_the_tallest_box_holds() {
    use crate::timeline_slot::WIDE;
    let wide = f64::from(WIDE);
    // The axis, Stage (to 58), Executor (to 94), Builds & tests (to 182), two gates.
    let bottoms = [58.0, 94.0, 182.0, 202.0, 222.0];
    let drawn = 222.0;
    // At the tallest box the foot and totals take 100 px: 140 left, Stage and Executor whole.
    assert_eq!(kept_wide(wide, 140.0, drawn, &bottoms), wide - 140.0 + 94.0);
    assert_eq!(
        kept_wide(wide, 140.0, drawn, &bottoms),
        wide - 140.0 + lane_edge(&bottoms, 140.0).unwrap(),
        "the tallest box keeps its own cut"
    );
    // A box kept at 160 px for a task of two lanes leaves 60, Stage alone; it keeps what 240 px
    // holds, Stage and Executor: it grows.
    let grown = kept_wide(160.0, 60.0, drawn, &bottoms);
    assert_eq!(grown, 160.0 - 60.0 + 94.0);
    assert!(grown > 160.0, "{grown}");
    assert_eq!(lane_edge(&bottoms, 60.0), Some(58.0), "this visit's cut");
    // Where the tallest box holds every lane, the lanes whole.
    assert_eq!(kept_wide(160.0, 60.0, 130.0, &[58.0, 94.0, 130.0]), 230.0);
    // Where not even the first lane would fit, the lanes whole: the slot keeps it within the box.
    assert_eq!(kept_wide(wide, 20.0, drawn, &bottoms), wide - 20.0 + drawn);
    // A box drawn taller than the tallest (a kept height from a taller build) keeps its own cut.
    assert_eq!(
        kept_wide(300.0, 200.0, drawn, &bottoms),
        300.0 - 200.0 + 182.0
    );
}

/// "Show all" counts what it shows: the steps of the phone's list, the lanes wide, a lane for
/// each of the timeline's own and one for its gates.
#[test]
fn show_all_counts_the_steps_on_a_phone_and_the_lanes_wide() {
    let timeline = pictures();
    let drawn = lanes(&timeline);
    assert!(!drawn.gates.is_empty());
    assert_eq!(lane_count(&drawn), drawn.lanes.len() + 1);
    let mut gateless = drawn.clone();
    gateless.gates.clear();
    assert_eq!(lane_count(&gateless), drawn.lanes.len());
    assert_eq!(
        show_all(rows(&timeline).len(), lane_count(&drawn)),
        (
            format!("Show all {} steps", rows(&timeline).len()),
            format!("Show all {} lanes", drawn.lanes.len() + 1)
        )
    );
    assert_eq!(
        show_all(1, 1),
        ("Show all 1 step".to_string(), "Show all 1 lane".to_string())
    );
}
