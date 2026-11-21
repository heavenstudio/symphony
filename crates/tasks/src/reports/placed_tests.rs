//! Where a custom workflow's tasks spent each hour ([`placed`]), over places written out by hand,
//! as the board's recorded transitions give them: each expected figure is counted here from the
//! places, hour by hour.

use super::*;

/// A UTC midnight: the day every case reads.
const D: f64 = 1_799_971_200.0;
const H: f64 = 3_600.0;

fn at(hours: f64) -> f64 {
    D + hours * H
}

/// A place at `phase`/`state`, each `(id, label)`, projecting to `status`, from `hours` after
/// midnight.
fn place(phase: (&str, &str), state: (&str, &str), status: &str, hours: f64) -> ReportPlace {
    ReportPlace {
        phase: phase.0.into(),
        phase_label: phase.1.into(),
        state: state.0.into(),
        state_label: state.1.into(),
        status: status.into(),
        from: at(hours),
    }
}

/// A task made `created` hours after midnight, a subtask of `parent` where one is named, with no
/// thread: its places alone say where it was.
fn task(slug: &str, parent: Option<&str>, created: f64) -> ReportTask {
    ReportTask {
        slug: slug.into(),
        status: "in_progress".into(),
        created: at(created),
        updated: at(created),
        parent: parent.map(Into::into),
        instruction: false,
        marked_red: false,
        between: None,
        lines: Vec::new(),
    }
}

const BUILD: (&str, &str) = ("build", "Build");
const REVIEW: (&str, &str) = ("review", "Review");
const SECURITY: (&str, &str) = ("security", "Security review");
const WORKING: (&str, &str) = ("working", "Working");
const BLOCKED: (&str, &str) = ("blocked", "Blocked");
const AWAITING: (&str, &str) = ("awaiting", "Awaiting");
const ACCEPTED: (&str, &str) = ("accepted", "Accepted");

/// One top-level task through a rework cycle, a block in Build and two review stages to its
/// acceptance, and a subtask of it at work meanwhile.
fn cycle() -> ReportRead {
    ReportRead {
        tasks: vec![
            task("sim/flow", None, 0.5),
            task("sim/flow/notes", Some("sim/flow"), 2.0),
        ],
        workflow: true,
        places: Some(vec![
            vec![
                place(BUILD, WORKING, "in_progress", 1.0),
                place(REVIEW, AWAITING, "review", 2.0),
                // Sent back: the rework cycle.
                place(BUILD, WORKING, "in_progress", 3.0),
                place(BUILD, BLOCKED, "blocked", 3.5),
                place(BUILD, WORKING, "in_progress", 4.0),
                place(REVIEW, AWAITING, "review", 5.0),
                // The second review stage, then accepted: done, which counts no more.
                place(SECURITY, AWAITING, "review", 5.5),
                place(SECURITY, ACCEPTED, "done", 6.5),
            ],
            vec![place(BUILD, WORKING, "in_progress", 2.0)],
        ]),
        ..ReportRead::default()
    }
}

/// Each entry of `spent` as `(id, label, hours)`.
fn hours(spent: &[Spent]) -> Vec<(&str, &str, f64)> {
    spent
        .iter()
        .map(|s| (s.id.as_str(), s.label.as_str(), s.hours))
        .collect()
}

/// Each hour's phases or states of the top-level tasks (`top`) or the subtasks, as [`hours`] gives
/// them.
fn each_hour(day: &ReportDay, top: bool, phases: bool) -> Vec<Vec<(&str, &str, f64)>> {
    day.hours
        .iter()
        .map(|hour| {
            let placed = hour
                .workflow
                .as_ref()
                .expect("a board that runs a workflow");
            let part = if top { &placed.top } else { &placed.subtasks };
            hours(if phases { &part.phases } else { &part.states })
        })
        .collect()
}

#[test]
fn a_rework_cycle_a_block_and_two_review_stages_count_each_hour_by_phase_and_by_state() {
    let day = day("sim", D, at(8.0), at(8.0), &cycle());
    assert_eq!(
        day.hours.len(),
        8,
        "midnight to the hour the read falls in: 08:00 sharp is the end of 07:00"
    );
    let phases = each_hour(&day, true, true);
    assert_eq!(
        phases,
        [
            vec![],
            vec![("build", "Build", 1.0)],
            vec![("review", "Review", 1.0)],
            vec![("build", "Build", 1.0)],
            vec![("build", "Build", 1.0)],
            vec![
                ("review", "Review", 0.5),
                ("security", "Security review", 0.5)
            ],
            vec![("security", "Security review", 0.5)],
            vec![],
        ],
        "an hour before the task's first move counts nothing, and its time accepted none"
    );
    let states = each_hour(&day, true, false);
    assert_eq!(
        states,
        [
            vec![],
            vec![("working", "Working", 1.0)],
            vec![("awaiting", "Awaiting", 1.0)],
            vec![("blocked", "Blocked", 0.5), ("working", "Working", 0.5)],
            vec![("working", "Working", 1.0)],
            vec![("awaiting", "Awaiting", 1.0)],
            vec![("awaiting", "Awaiting", 0.5)],
            vec![],
        ],
        "the block is the state's, the phase stays Build"
    );
    // The two totals are of the same time: each sums to it, and neither is the other's part.
    let total = |rows: &[Vec<(&str, &str, f64)>]| -> f64 {
        rows.iter().flatten().map(|(_, _, hours)| hours).sum()
    };
    assert_eq!(total(&phases), 5.5);
    assert_eq!(total(&states), 5.5);
    // The subtask is counted apart, from its own move, and only there.
    let subtasks = each_hour(&day, false, true);
    assert_eq!(subtasks[1], vec![]);
    assert_eq!(subtasks[2], vec![("build", "Build", 1.0)]);
    assert!(
        each_hour(&day, false, false)[3..8]
            .iter()
            .all(|hour| *hour == [("working", "Working", 1.0)]),
        "still at work, hour after hour"
    );
    // The statuses' task-hours are the thread's replay, apart: this task wrote no line, so its
    // row's in_progress holds the hour the workflow splits between Working and Blocked.
    let statuses = day.hours[3].top.task_hours;
    assert_eq!((statuses.in_progress, statuses.blocked), (1.0, 0.0));
}

#[test]
fn the_last_place_counts_up_to_the_read_and_a_place_held_from_before_the_day_counts_from_midnight()
{
    let read = ReportRead {
        tasks: vec![task("sim/long", None, -30.0)],
        workflow: true,
        places: Some(vec![vec![place(BUILD, WORKING, "in_progress", -10.0)]]),
        ..ReportRead::default()
    };
    let day = day("sim", D, at(2.25), at(2.25), &read);
    assert_eq!(
        each_hour(&day, true, true),
        [
            vec![("build", "Build", 1.0)],
            vec![("build", "Build", 1.0)],
            vec![("build", "Build", 0.25)],
        ],
        "an hour with no transition in it counts the place held through it"
    );
    let before = super::day("sim", D - DAY, D, at(2.25), &read);
    let counted: Vec<usize> = (before.hours.iter().enumerate())
        .filter(|(_, hour)| !hour.workflow.as_ref().unwrap().top.phases.is_empty())
        .map(|(n, _)| n)
        .collect();
    assert_eq!(
        counted,
        (14..24).collect::<Vec<_>>(),
        "the day before counts from the move, 14:00, to its end"
    );
}

#[test]
fn a_label_renamed_by_a_later_definition_is_an_entry_of_its_own_and_never_renames_the_earlier() {
    let make = ("build", "Make");
    let read = ReportRead {
        tasks: vec![task("sim/renamed", None, 0.0), task("sim/later", None, 0.0)],
        workflow: true,
        places: Some(vec![
            vec![
                place(BUILD, WORKING, "in_progress", 0.0),
                // The same position, moved again under a new generation whose definition calls
                // the phase Make and the state Making.
                place(make, ("working", "Making"), "in_progress", 1.5),
            ],
            vec![place(make, ("working", "Making"), "in_progress", 1.0)],
        ]),
        ..ReportRead::default()
    };
    let day = day("sim", D, at(3.0), at(3.0), &read);
    assert_eq!(
        each_hour(&day, true, true),
        [
            vec![("build", "Build", 1.0)],
            vec![("build", "Build", 0.5), ("build", "Make", 1.5)],
            vec![("build", "Make", 2.0)],
        ],
        "one ID, two labels: the hours before the rename keep the name they were spent under"
    );
    assert_eq!(
        each_hour(&day, true, false)[1],
        [("working", "Making", 1.5), ("working", "Working", 0.5)],
        "ordered by ID, then label"
    );
}

#[test]
fn a_task_cancelled_counts_nothing_after_and_one_with_no_place_counts_nothing() {
    let read = ReportRead {
        tasks: vec![task("sim/gone", None, 0.0), task("sim/never", None, 0.0)],
        workflow: true,
        places: Some(vec![
            vec![
                place(BUILD, WORKING, "in_progress", 0.0),
                place(BUILD, ("cancelled", "Cancelled"), "cancelled", 0.25),
                // Reopened: counted again from its move.
                place(BUILD, WORKING, "in_progress", 1.75),
            ],
            vec![],
        ]),
        ..ReportRead::default()
    };
    let day = day("sim", D, at(2.0), at(2.0), &read);
    assert_eq!(
        each_hour(&day, true, false),
        [
            vec![("working", "Working", 0.25)],
            vec![("working", "Working", 0.25)],
        ]
    );
}

#[test]
fn a_board_that_runs_no_workflow_writes_no_field_and_the_bytes_it_wrote_before() {
    let legacy = ReportRead {
        tasks: vec![task("sim/legacy", None, 0.0)],
        workflow: false,
        places: None,
        ..ReportRead::default()
    };
    let day = day("sim", D, at(2.0), at(2.0), &legacy);
    assert!(day.hours.iter().all(|hour| hour.workflow.is_none()));
    let text = String::from_utf8(json::day(&day)).unwrap();
    assert!(!text.contains("workflow"), "{text}");
    // A board that runs one writes the field in every hour, empty where no task was placed.
    let placed = ReportRead {
        workflow: true,
        places: Some(vec![vec![]]),
        ..legacy
    };
    let day = super::day("sim", D, at(2.0), at(2.0), &placed);
    assert!(
        day.hours
            .iter()
            .all(|hour| hour.workflow == Some(Placed::default()))
    );
    let text = String::from_utf8(json::day(&day)).unwrap();
    assert!(
        text.contains(
            r#""workflow":{"top":{"phases":[],"states":[]},"subtasks":{"phases":[],"states":[]}}"#
        ),
        "{text}"
    );
}

#[test]
fn a_day_file_with_the_workflow_s_hours_is_the_text_serde_writes() {
    let mut odd = cycle();
    if let Some(places) = &mut odd.places {
        places[0][0].phase_label = "B \"q\" \\ \n\t\u{1}é☃".into();
    }
    for read in [cycle(), odd] {
        for day in [
            day("sim", D, at(8.0), at(8.0), &read),
            day("sim", D - DAY, D, D, &read),
        ] {
            let ours = String::from_utf8(json::day(&day)).unwrap();
            assert_eq!(ours, serde_json::to_string(&day).unwrap());
            let back: ReportDay = serde_json::from_str(&ours).unwrap();
            assert_eq!(back, day, "it reads back as written");
        }
    }
}
