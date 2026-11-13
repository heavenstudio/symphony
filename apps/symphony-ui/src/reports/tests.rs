//! The Reports page drawn on the host: its scale, bars and lines, its words for periods and
//! figures, a gap hatched and said, the readout of a tapped period, the table, the four figures,
//! the empty state that says when the first roll-up comes, and the ghost before the first answer.

use super::chart::table;
use super::*;
use symphony_api::agent_activity::{Gauge, Level, RoleLevel};
use symphony_api::health::HourPasses;
use symphony_api::report_page::{
    BucketAgents, BucketFlow, BucketHealth, BucketVerdicts, BucketWaits, ReportBucket, ReportsPage,
};
use symphony_api::reports::{ByStatus, DecisionCounts, GitCounts, Spread};

/// Monday 2026-11-09, 00:00 UTC.
const MONDAY: f64 = 1_794_182_400.0;

fn series(name: &'static str, mark: Mark, values: &[Option<f64>]) -> SeriesData {
    SeriesData {
        name,
        mark,
        values: values.to_vec(),
    }
}

fn chart(unit: Unit, series: Vec<SeriesData>) -> ChartData {
    let periods = series.first().map_or(0, |s| s.values.len());
    ChartData {
        id: "test",
        title: "Test",
        note: "A test chart.".into(),
        unit,
        series,
        cover: vec![1.0; periods],
        instead: None,
    }
}

/// `wire` as the page reads it: sent as the server sends it and parsed, as the browser does.
fn read(wire: &ReportsPage) -> Page {
    serde_json::from_value(serde_json::to_value(wire).unwrap()).unwrap()
}

/// [`wire`] as the page reads it.
fn page() -> Page {
    read(&wire())
}

/// Three days: Monday rolled up whole with its agents watched, Tuesday recorded by no source,
/// Wednesday under way, half rolled up; a settings change on Monday.
fn wire() -> ReportsPage {
    let mut monday_flow = BucketFlow::default();
    // One task approved again after its landing sent it back: four entries into done, three tasks.
    monday_flow.entered.done = 4;
    monday_flow.done = Some(3);
    monday_flow.held.ready = 2;
    monday_flow.held.review = 1;
    monday_flow.bounces = 1;
    let mut wednesday_flow = BucketFlow::default();
    wednesday_flow.entered.done = 4;
    wednesday_flow.done = Some(4);
    wednesday_flow.held.in_progress = 5;
    ReportsPage {
        grain: Grain::Day,
        days: 3,
        from: MONDAY,
        to: MONDAY + 2.0 * DAY + 11.0 * HOUR,
        buckets: vec![
            ReportBucket {
                start: MONDAY,
                end: MONDAY + DAY,
                rolled: 24,
                flow: Some(monday_flow),
                decisions: Some(DecisionCounts {
                    open: 4,
                    answered: 1,
                    auto_approved: 2,
                    withdrawn: 0,
                }),
                git: Some(GitCounts {
                    commits: 12,
                    merges: 3,
                    added: 1200,
                    removed: 300,
                    reverts: 1,
                }),
                lead: Some(Spread {
                    n: 2,
                    median: 3600.0,
                    p90: 7200.0,
                }),
                agents: Some(BucketAgents {
                    observed_s: 3600.0,
                    samples: 60,
                    running: vec![
                        RoleLevel {
                            role: "executor".into(),
                            peak: 3,
                            mean: 2.0,
                        },
                        RoleLevel {
                            role: "orchestrator".into(),
                            peak: 1,
                            mean: 1.0,
                        },
                        RoleLevel {
                            role: "designer".into(),
                            peak: 1,
                            mean: 0.5,
                        },
                    ],
                    running_agents: Level { peak: 3, mean: 2.5 },
                    ..BucketAgents::default()
                }),
                ..ReportBucket::default()
            },
            ReportBucket {
                start: MONDAY + DAY,
                end: MONDAY + 2.0 * DAY,
                ..ReportBucket::default()
            },
            ReportBucket {
                start: MONDAY + 2.0 * DAY,
                end: MONDAY + 2.0 * DAY + 11.0 * HOUR,
                rolled: 5,
                flow: Some(wednesday_flow),
                decisions: Some(DecisionCounts {
                    open: 2,
                    answered: 1,
                    ..DecisionCounts::default()
                }),
                lead: Some(Spread {
                    n: 1,
                    median: 1800.0,
                    p90: 1800.0,
                }),
                ..ReportBucket::default()
            },
        ],
        first: Some(MONDAY - 30.0 * DAY),
        marks: vec![ReportMark {
            at: MONDAY + 6.0 * HOUR,
            commit: "4f1c0a9b2d3e".into(),
            subject: "settings: land without the check".into(),
        }],
        unread: Vec::new(),
    }
}

fn find<'a>(charts: &'a [(&'static str, Vec<ChartData>)], id: &str) -> &'a ChartData {
    charts
        .iter()
        .flat_map(|(_, charts)| charts)
        .find(|chart| chart.id == id)
        .unwrap()
}

#[test]
fn a_scale_steps_round_and_holds_every_stack_and_line() {
    let counts = chart(
        Unit::Count,
        vec![
            series("a", Mark::Bar, &[Some(3.0), Some(4.0), None]),
            series("b", Mark::Bar, &[Some(1.0), Some(3.0), None]),
            series("c", Mark::Line, &[Some(6.0), None, None]),
        ],
    );
    assert_eq!(
        Scale::of(&counts).ticks,
        [0.0, 2.0, 4.0, 6.0, 8.0],
        "the stack of 7 and the line's 6 under the top"
    );
    assert!(
        Scale::of(&counts).low.is_sign_positive(),
        "nothing below the axis starts it at 0, never -0"
    );
    let lines = chart(
        Unit::Count,
        vec![
            series("added", Mark::Bar, &[Some(120.0)]),
            series("removed", Mark::Below, &[Some(45.0)]),
        ],
    );
    let scale = Scale::of(&lines);
    assert_eq!(
        (scale.low, scale.high, scale.ticks.len()),
        (-50.0, 150.0, 5)
    );
    assert!((scale.at(0.0) - 25.0).abs() < 1e-9, "zero a quarter up");
    let waits = chart(
        Unit::Seconds,
        vec![series("w", Mark::Line, &[Some(5400.0)])],
    );
    assert_eq!(
        Scale::of(&waits).ticks,
        [0.0, 1800.0, 3600.0, 5400.0],
        "half hours"
    );
    let nothing = chart(Unit::Count, vec![series("n", Mark::Bar, &[None, None])]);
    assert_eq!(
        Scale::of(&nothing).ticks,
        [0.0, 1.0],
        "a whole one at least"
    );
    let agents = chart(Unit::Mean, vec![series("m", Mark::Bar, &[Some(2.5)])]);
    assert_eq!(Scale::of(&agents).ticks, [0.0, 1.0, 2.0, 3.0]);
    let small = chart(Unit::Count, vec![series("s", Mark::Line, &[Some(1.0)])]);
    assert_eq!(Scale::of(&small).ticks, [0.0, 1.0], "no quarter of a count");
    let ten = chart(Unit::Count, vec![series("t", Mark::Line, &[Some(10.0)])]);
    assert_eq!(
        Scale::of(&ten).ticks,
        [0.0, 5.0, 10.0],
        "no half of a count"
    );
    let hundred = chart(Unit::Count, vec![series("h", Mark::Bar, &[Some(100.0)])]);
    assert_eq!(Scale::of(&hundred).ticks, [0.0, 25.0, 50.0, 75.0, 100.0]);
    let hours = chart(Unit::Hours, vec![series("h", Mark::Bar, &[Some(10.0)])]);
    assert_eq!(
        Scale::of(&hours).ticks,
        [0.0, 2.5, 5.0, 7.5, 10.0],
        "hours take halves"
    );
}

#[test]
fn a_bar_stacks_up_and_down_its_far_ends_rounded_and_a_gap_over_each_part_below() {
    let stacked = chart(
        Unit::Count,
        vec![
            series("a", Mark::Bar, &[Some(2.0), Some(0.0)]),
            series("b", Mark::Bar, &[Some(2.0), Some(4.0)]),
            series("c", Mark::Below, &[Some(2.0), None]),
        ],
    );
    let scale = Scale::of(&stacked);
    assert_eq!((scale.low, scale.high), (-2.0, 4.0));
    let parts: Vec<_> = segments(&stacked, 0, &scale)
        .into_iter()
        .map(|s| {
            (
                s.series,
                s.bottom.round(),
                s.height.round(),
                s.up,
                s.end,
                s.after,
            )
        })
        .collect();
    assert_eq!(
        parts,
        [
            (0, 33.0, 33.0, true, false, false),
            (1, 67.0, 33.0, true, true, true),
            (2, 0.0, 33.0, false, true, false)
        ]
    );
    // A zero draws nothing: the next part is the first.
    let parts = segments(&stacked, 1, &scale);
    assert_eq!(parts.len(), 1);
    assert!(parts[0].end && !parts[0].after && parts[0].series == 1);
}

#[test]
fn a_line_breaks_across_a_gap_and_a_period_alone_between_gaps_is_a_dot() {
    let line = chart(
        Unit::Count,
        vec![series(
            "l",
            Mark::Line,
            &[
                Some(1.0),
                Some(2.0),
                None,
                Some(3.0),
                None,
                Some(4.0),
                Some(4.0),
            ],
        )],
    );
    let scale = Scale::of(&line);
    let (lines, dots) = runs(&line.series[0].values, &scale);
    assert_eq!(lines, ["0.5,75 1.5,50", "5.5,0 6.5,0"]);
    assert_eq!(dots, [(3.5, 25.0)]);
    assert_eq!(runs(&[None, None], &scale), (Vec::new(), Vec::new()));
}

#[test]
fn a_unit_writes_its_figures_short() {
    for (unit, value, text) in [
        (Unit::Count, 12.0, "12"),
        (Unit::Count, 0.5, "0.5"),
        (Unit::Count, 1234.0, "1.2k"),
        (Unit::Count, -300.0, "-300"),
        (Unit::Mean, 2.0, "2"),
        (Unit::Mean, 2.75, "2.75"),
        (Unit::Seconds, 45.0, "45s"),
        (Unit::Seconds, 90.0, "1.5m"),
        (Unit::Seconds, 720.0, "12m"),
        (Unit::Seconds, 5400.0, "1.5h"),
        (Unit::Seconds, 200_000.0, "2.3d"),
        (Unit::Hours, 12.3, "12.3h"),
        (Unit::Hours, 3.0, "3h"),
        (Unit::Load, 6.5, "6.5"),
        // An empty sum of floats is negative zero, and a tiny negative rounds to it.
        (Unit::Mean, -0.0, "0"),
        (Unit::Count, -0.001, "0"),
        (Unit::Seconds, -0.0, "0s"),
        (Unit::Hours, -0.01, "0h"),
    ] {
        assert_eq!(unit.format(value), text, "{unit:?} {value}");
    }
    assert_eq!(coverage(0.75, 24.0).as_deref(), Some("18 of 24 h recorded"));
    assert_eq!(coverage(0.5, 1.0).as_deref(), Some("0.5 of 1 h recorded"));
    assert_eq!(coverage(1.0, 24.0), None, "a whole period says nothing");
    assert_eq!(coverage(0.0, 24.0), None, "a gap says it is one instead");
}

#[test]
fn a_period_is_named_by_its_utc_day_week_or_hour_and_the_axis_labels_at_most_six() {
    assert_eq!(period(Grain::Day, MONDAY), "Mon 9 Nov");
    assert_eq!(period(Grain::Week, MONDAY), "Week of 9 Nov");
    assert_eq!(
        period(Grain::Hour, MONDAY + 14.0 * HOUR),
        "Mon 9 Nov, 14:00 UTC"
    );
    assert_eq!(period(Grain::Day, MONDAY - DAY), "Sun 8 Nov");
    assert_eq!(period(Grain::Day, MONDAY + 23.0 * DAY), "Wed 2 Dec");
    let page = page();
    let axis = axis(&page);
    assert_eq!(axis.periods, ["Mon 9 Nov", "Tue 10 Nov", "Wed 11 Nov"]);
    assert_eq!(axis.hours, [24.0, 24.0, 11.0]);
    assert_eq!(
        axis.ticks,
        [
            (0, "9 Nov".to_string()),
            (1, "10 Nov".to_string()),
            (2, "11 Nov".to_string())
        ]
    );
    let [(left, at, text)] = axis.marks.as_slice() else {
        panic!("{:?}", axis.marks);
    };
    assert_eq!(
        (*at, text.as_str()),
        (0, "settings: land without the check (4f1c0a9b2d3e)")
    );
    assert!(
        (left - 25.0 / 3.0).abs() < 1e-9,
        "a quarter into the first of three: {left}"
    );
    // By the hour, six labels at most on the clock's round hours: a week's every other midnight,
    // two days' midnights and noons, a day's every fourth hour.
    let hours = |count: u32, first: f64| {
        let mut hours = wire();
        hours.grain = Grain::Hour;
        hours.buckets = (0..count)
            .map(|n| ReportBucket {
                start: first + f64::from(n) * HOUR,
                end: first + f64::from(n + 1) * HOUR,
                ..ReportBucket::default()
            })
            .collect();
        super::axis(&read(&hours)).ticks
    };
    let labels = |ticks: Vec<(usize, String)>| -> Vec<String> {
        ticks
            .into_iter()
            .map(|(at, label)| format!("{at} {label}"))
            .collect()
    };
    assert_eq!(
        labels(hours(168, MONDAY)),
        ["0 9 Nov", "48 11 Nov", "96 13 Nov", "144 15 Nov"]
    );
    assert_eq!(
        labels(hours(48, MONDAY + 5.0 * HOUR)),
        ["7 12:00", "19 10 Nov", "31 12:00", "43 11 Nov"]
    );
    assert_eq!(
        labels(hours(24, MONDAY + 11.0 * HOUR)),
        [
            "1 12:00",
            "5 16:00",
            "9 20:00",
            "13 10 Nov",
            "17 04:00",
            "21 08:00"
        ]
    );
}

#[test]
fn the_figures_sum_the_window_and_say_what_is_missing() {
    let [done, lead, agents, decisions] = figures(&page());
    assert_eq!(
        (done.value.as_str(), done.detail.as_str()),
        ("7", "3.5 a day on average")
    );
    assert_eq!(
        (lead.value.as_str(), lead.detail.as_str()),
        ("50m", "median of 3 closed · p90 up to 2h")
    );
    assert_eq!(
        (agents.value.as_str(), agents.detail.as_str()),
        ("2.5", "running on average · 3 at most")
    );
    assert_eq!(
        (decisions.value.as_str(), decisions.detail.as_str()),
        ("2", "2 auto-approved · 2 answered in the window")
    );
    let empty = read(&ReportsPage {
        buckets: vec![ReportBucket::default()],
        ..ReportsPage::default()
    });
    for figure in figures(&empty) {
        assert_eq!(figure.value, "–", "{figure:?}");
    }
    let mut hours = page();
    hours.grain = Grain::Hour;
    assert_eq!(figures(&hours)[1].detail, "kept by the day");
}

#[test]
fn the_charts_take_each_series_from_its_source_and_none_where_it_has_no_record() {
    let charts = charts(&page());
    assert_eq!(
        charts
            .iter()
            .map(|(heading, _)| *heading)
            .collect::<Vec<_>>(),
        [
            "Flow",
            "Agents",
            "Decisions and code",
            "Quality",
            "Board health"
        ]
    );
    let open = find(&charts, "open");
    assert_eq!(
        open.note,
        "Top-level tasks in each open status at the end of each day."
    );
    assert_eq!(open.series[0].values, [Some(2.0), None, Some(0.0)], "Ready");
    assert!(
        open.gap(1) && !open.gap(2),
        "Tuesday a gap, Wednesday's zero a zero"
    );
    assert_eq!(open.cover, [1.0, 0.0, 5.0 / 11.0]);
    let agents = find(&charts, "agents");
    let values: Vec<_> = agents.series.iter().map(|s| s.values[0]).collect();
    assert_eq!(
        values,
        [Some(1.0), Some(2.0), Some(0.0), Some(0.5), Some(2.5)],
        "orchestrators, executors, reviewers, the other roles together, each session once"
    );
    assert_eq!(
        agents.series[0].values[2], None,
        "no agents' hour on Wednesday"
    );
    let lines = find(&charts, "lines");
    assert_eq!(lines.series[1].mark, Mark::Below);
    assert_eq!(lines.series[1].values[0], Some(300.0));
    assert!(
        charts
            .iter()
            .flat_map(|(_, c)| c)
            .all(|c| c.instead.is_none())
    );
    let mut hours = page();
    hours.grain = Grain::Hour;
    assert!(
        find(&charts_of(&hours), "lead").instead.is_some(),
        "lead time is kept by the day"
    );
}

fn charts_of(page: &Page) -> Vec<(&'static str, Vec<ChartData>)> {
    charts(page)
}

/// Tasks done reads the tasks done, each once ([`BucketFlow::done`]), never the entries into done;
/// a period whose roll-up did not count them is a gap there, and the figure is over the periods
/// that did, missing when none did.
#[test]
fn tasks_done_are_the_tasks_counted_once_and_a_period_that_did_not_count_them_is_a_gap() {
    let charts = charts(&page());
    let done = find(&charts, "done");
    assert_eq!(
        done.series[0].values,
        [Some(3.0), None, Some(4.0)],
        "Monday's three tasks, not its four entries into done"
    );
    assert!(
        done.note.contains("each once at its last close"),
        "{}",
        done.note
    );
    assert!(done.note.contains("UTC"), "{}", done.note);
    let mut wire = wire();
    for bucket in &mut wire.buckets {
        if let Some(flow) = &mut bucket.flow {
            flow.done = None;
        }
    }
    let uncounted = read(&wire);
    let charts = charts_of(&uncounted);
    let done = find(&charts, "done");
    assert_eq!(done.series[0].values, [None, None, None]);
    assert!(
        done.gap(0) && done.gap(2),
        "no zero drawn where nothing was counted"
    );
    let [figure, ..] = figures(&uncounted);
    assert_eq!(
        (figure.value.as_str(), figure.detail.as_str()),
        ("–", "no roll-up in the window counted them")
    );
    wire.buckets[0].flow.as_mut().unwrap().done = Some(3);
    let [figure, ..] = figures(&read(&wire));
    assert_eq!(
        (figure.value.as_str(), figure.detail.as_str()),
        ("3", "3 a day on average"),
        "over Monday alone, the one period that counted them"
    );
}

#[test]
fn a_tapped_period_shows_every_value_and_a_gap_says_it_has_none() {
    let page = page();
    let charts = charts(&page);
    let open = find(&charts, "open").clone();
    let axis = axis(&page);
    let draw = |selected: Option<usize>| card(open.clone(), axis.clone(), selected);
    // None tapped: the last period with a value, Wednesday, under way and half rolled up.
    let html = draw(None);
    assert!(
        html.contains("<strong>Wed 11 Nov</strong> · 5 of 11 h recorded"),
        "{html}"
    );
    assert!(html.contains("In progress <strong>5</strong>"), "{html}");
    assert!(html.contains("class=\"chart-col on\""), "{html}");
    assert_eq!(
        html.matches("class=\"chart-col").count() - html.matches("class=\"chart-cols\"").count(),
        3,
        "a column a period"
    );
    // Tuesday tapped: hatched, and said to have no data.
    let html = draw(Some(1));
    assert!(html.contains("class=\"chart-col gap on\""), "{html}");
    assert!(
        html.contains("<strong>Tue 10 Nov</strong> · no data recorded"),
        "{html}"
    );
    assert!(!html.contains("Ready <strong>"), "{html}");
    // Monday tapped: each series' value, the settings change in it, the stack's parts.
    let html = draw(Some(0));
    assert!(html.contains("Ready <strong>2</strong>"), "{html}");
    assert!(html.contains("Review <strong>1</strong>"), "{html}");
    assert!(html.contains("Blocked <strong>0</strong>"), "{html}");
    assert!(
        html.contains("Settings changed: settings: land without the check (4f1c0a9b2d3e)"),
        "{html}"
    );
    assert_eq!(
        html.matches("class=\"seg up").count(),
        3,
        "Monday's two parts and Wednesday's one"
    );
    assert!(html.contains("class=\"chart-mark\""));
    assert!(
        html.contains("Show the table") && html.contains("aria-expanded=false"),
        "{html}"
    );
    // A chart of lines: one polyline per run, a dot for a period alone.
    let lead = find(&charts, "lead").clone();
    let html = card(lead, axis.clone(), None);
    assert_eq!(
        html.matches("<polyline").count(),
        0,
        "Monday and Wednesday, a gap between"
    );
    assert!(html.matches("class=\"dot\"").count() >= 4, "{html}");
}

#[test]
fn a_chart_s_table_says_every_period_and_a_gap_as_a_dash() {
    let page = page();
    let charts = charts(&page);
    let chart = find(&charts, "done").clone();
    let axis = axis(&page);
    let html = dioxus_ssr::render_element(table(&chart, &axis));
    assert!(
        html.contains("<caption>Tasks done, by period</caption>"),
        "{html}"
    );
    assert!(
        html.contains("<th scope=\"row\">Mon 9 Nov</th><td>3</td>"),
        "{html}"
    );
    assert!(
        html.contains("<th scope=\"row\">Tue 10 Nov</th><td>–</td>"),
        "{html}"
    );
}

#[test]
fn a_chart_the_window_cannot_show_says_so_in_its_place() {
    let mut hours = page();
    hours.grain = Grain::Hour;
    let chart = find(&charts_of(&hours), "lead").clone();
    let html = card(chart, Axis::default(), None);
    assert!(html.contains("switch to Days or Weeks"), "{html}");
    assert!(!html.contains("chart-plot"), "{html}");
}

#[derive(Clone, PartialEq, Props)]
struct CardProps {
    chart: ChartData,
    axis: Axis,
    selected: Option<usize>,
}

/// A chart drawn alone, `selected` its period tapped.
fn card(chart: ChartData, axis: Axis, selected: Option<usize>) -> String {
    let mut dom = VirtualDom::new_with_props(
        |props: CardProps| rsx! { ChartCard { chart: props.chart, axis: props.axis, selected: props.selected, onselect: |_| {} } },
        CardProps {
            chart,
            axis,
            selected,
        },
    );
    dom.rebuild_in_place();
    dioxus_ssr::render(&dom)
}

fn view(page: Option<Page>) -> String {
    failed(page, None)
}

fn failed(page: Option<Page>, error: Option<String>) -> String {
    let mut dom = VirtualDom::new_with_props(
        |props: ViewProps| rsx! { ReportsView { grain: props.grain, days: props.days, page: props.page, error: props.error, onask: |_| {} } },
        ViewProps {
            grain: Grain::Day,
            days: 3,
            page,
            error,
        },
    );
    dom.rebuild_in_place();
    dioxus_ssr::render(&dom)
}

#[derive(Clone, PartialEq, Props)]
struct ViewProps {
    grain: Grain,
    days: u32,
    page: Option<Page>,
    error: Option<String>,
}

#[test]
fn before_the_first_answer_the_page_is_its_ghost_and_the_window_chosen_is_marked() {
    let html = view(None);
    assert!(html.contains("aria-busy=true"), "{html}");
    assert!(html.contains("data-ready=\"loading\""), "{html}");
    assert!(html.contains("Loading the reports…"), "{html}");
    assert!(
        html.contains("Days · the last 3 days · times in UTC"),
        "{html}"
    );
    assert!(
        html.contains("aria-pressed=true data-grain=\"day\"")
            || html.contains("data-grain=\"day\""),
        "{html}"
    );
    assert_eq!(html.matches("class=\"ghost-chart\"").count(), 2);
    assert!(!html.contains("chart-plot"));
}

#[test]
fn a_read_that_fails_says_why_and_keeps_what_it_showed() {
    let why = Some("The server could not be reached".to_string());
    let html = failed(None, why.clone());
    assert!(html.contains("data-ready=\"failed\""), "{html}");
    assert!(
        html.contains(
            "role=\"alert\">The reports could not be read: The server could not be reached</p>"
        ),
        "{html}"
    );
    let html = failed(Some(page()), why);
    assert!(html.contains("data-ready=\"reports\""), "{html}");
    assert!(html.contains("The reports could not be read"), "{html}");
    assert!(
        html.contains("chart-plot"),
        "the last answer still drawn: {html}"
    );
}

#[test]
fn the_report_draws_its_figures_marks_and_every_chart() {
    let html = view(Some(page()));
    assert!(html.contains("aria-busy=false"), "{html}");
    assert!(html.contains("data-ready=\"reports\""), "{html}");
    assert!(
        html.contains("<dt>Done</dt><dd class=\"value\">7</dd>"),
        "{html}"
    );
    assert!(
        html.contains("Mon 9 Nov: settings changed, settings: land without the check"),
        "{html}"
    );
    assert_eq!(
        html.matches("class=\"chart\"").count(),
        14,
        "every chart near the screen on the host"
    );
    assert!(!html.contains("No roll-up yet"));
    assert!(!html.contains("not read"));
}

#[test]
fn with_no_roll_up_yet_the_page_says_when_the_first_comes() {
    let mut empty = wire();
    empty.first = None;
    for bucket in &mut empty.buckets {
        *bucket = ReportBucket {
            start: bucket.start,
            end: bucket.end,
            ..ReportBucket::default()
        };
    }
    let html = view(Some(read(&empty)));
    assert!(html.contains("No roll-up yet."), "{html}");
    assert!(html.contains("the next is due by 11:00 UTC"), "{html}");
    assert!(!html.contains("chart-plot"), "no chart of nothing: {html}");
    // The agents recorded before the first roll-up still draw.
    empty.buckets[0].agents = wire().buckets[0].agents.clone();
    empty.unread = vec!["/repo/.git/symphony/reports/2026-11-10.json: not a regular file".into()];
    let html = view(Some(read(&empty)));
    assert!(html.contains("No roll-up yet."));
    assert!(html.contains("chart-plot"));
    assert!(
        html.contains("1 file not read: their hours show as gaps"),
        "{html}"
    );
    assert!(html.contains("2026-11-10.json: not a regular file"));
}

#[test]
fn the_window_chosen_is_this_browser_s_when_it_is_one_the_page_offers() {
    let saved = |grain: &'static str, days: &'static str| {
        move |key: &str| match key {
            "reports-grain" => Some(grain.to_string()),
            "reports-days" => Some(days.to_string()),
            _ => None,
        }
    };
    assert_eq!(chosen(|_| None), (Grain::Day, 14));
    assert_eq!(chosen(saved("hour", "7")), (Grain::Hour, 7));
    assert_eq!(chosen(saved("week", "84")), (Grain::Week, 84));
    assert_eq!(
        chosen(saved("hour", "90")),
        (Grain::Hour, 2),
        "no 90 days by the hour"
    );
    assert_eq!(chosen(saved("month", "x")), (Grain::Day, 14));
    for grain in [Grain::Hour, Grain::Day, Grain::Week] {
        assert!(ranges(grain).contains(&grain.default_days()), "{grain:?}");
        assert!(ranges(grain).iter().all(|days| *days <= grain.max_days()));
    }
    assert_eq!(range_name(Grain::Hour, 1), "24 hours");
    assert_eq!(range_name(Grain::Week, 84), "12 weeks");
    assert_eq!(range_name(Grain::Day, 30), "30 days");
}

#[test]
fn a_period_s_number_is_none_where_its_source_has_no_record_and_zero_where_a_count_left_it_out() {
    let bucket = Bucket(serde_json::json!({
        "start": 10.0,
        "end": 70.0,
        "flow": { "held": { "ready": 2 }, "task_hours": {} },
        "waits": { "changes_to_review": null, "review_to_reviewer": { "median": 30.5 } },
        "agents": null,
    }));
    assert_eq!(bucket.number("flow.held.ready"), Some(2.0));
    assert_eq!(
        bucket.number("flow.held.blocked"),
        Some(0.0),
        "a status at zero is left out"
    );
    assert_eq!(bucket.number("flow.task_hours.review"), Some(0.0));
    assert_eq!(bucket.number("waits.review_to_reviewer.median"), Some(30.5));
    assert_eq!(
        bucket.number("waits.changes_to_review.median"),
        None,
        "no wait of the kind ended"
    );
    assert_eq!(
        bucket.number("agents.started"),
        None,
        "no agents' line in the period"
    );
    assert_eq!(bucket.number("git.commits"), None, "no source at all");
    assert_eq!(
        bucket.number("flow.held.ready.more"),
        None,
        "a number has no members"
    );
    assert!(bucket.has("flow") && !bucket.has("agents") && !bucket.has("git"));
    assert_eq!(bucket.span(), (10.0, 70.0));
    assert!(bucket.list("agents", "running").is_empty());
}

/// A period every source recorded whole, with every status, wait and gauge it can have.
fn full() -> ReportBucket {
    let all = |n: u32| ByStatus {
        backlog: n,
        blocked: n,
        cancelled: n,
        concept: n,
        design: n,
        done: n,
        in_progress: n,
        ready: n,
        review: n,
    };
    let spread = Some(Spread {
        n: 1,
        median: 1.0,
        p90: 1.0,
    });
    ReportBucket {
        start: MONDAY,
        end: MONDAY + DAY,
        rolled: 24,
        flow: Some(BucketFlow {
            held: all(1),
            entered: all(1),
            task_hours: ByStatus {
                blocked: 1.0,
                in_progress: 1.0,
                ready: 1.0,
                review: 1.0,
                ..ByStatus::default()
            },
            done: Some(1),
            bounces: 1,
            reds_filed: 1,
            ..BucketFlow::default()
        }),
        decisions: Some(DecisionCounts::default()),
        git: Some(GitCounts::default()),
        waits: Some(BucketWaits {
            review_to_reviewer: spread,
            changes_to_resumed: spread,
            changes_to_review: spread,
            approved_to_landed: spread,
        }),
        lead: spread,
        verdicts: Some(BucketVerdicts::default()),
        red_fix: spread,
        agents: Some(BucketAgents {
            observed_s: 3600.0,
            load_1m: Some(Gauge {
                peak: 1.0,
                mean: 1.0,
            }),
            ..BucketAgents::default()
        }),
        health: Some(BucketHealth {
            covered_s: 3600.0,
            dispatch: HourPasses { ok: 1, failed: 1 },
            sweep: HourPasses { ok: 1, failed: 1 },
            ..BucketHealth::default()
        }),
    }
}

/// Each chart reads its series where the wire writes them: a path the wire has not, as a member
/// misspelt, would read a source's every period as nothing or a status's as zero.
#[test]
fn every_series_reads_a_member_the_wire_writes() {
    let wire = serde_json::to_value(full()).unwrap();
    let written = |path: &str| {
        wire.pointer(&format!("/{}", path.replace('.', "/")))
            .is_some_and(serde_json::Value::is_number)
    };
    let mut read = 0;
    for (_, specs) in SECTIONS {
        for spec in specs {
            for series in spec.series {
                let paths = match series.pick {
                    Pick::At(path) => vec![path],
                    Pick::Sum(one, other) => vec![one, other],
                    Pick::Role(_) => vec![],
                };
                for path in paths {
                    assert!(written(path), "{}: {} reads {path}", spec.id, series.name);
                    read += 1;
                }
            }
        }
    }
    assert_eq!(
        read, 38,
        "every series' path but the four by role's, and the failed passes' two"
    );
}

/// The wire's own fixture, as the server writes it, read by the page.
#[test]
fn the_page_reads_the_wire_s_fixture() {
    let page: Page = serde_json::from_str(include_str!(
        "../../../../crates/symphony-api/fixtures/reports-page.json"
    ))
    .unwrap();
    let charts = charts(&page);
    assert_eq!(find(&charts, "done").series[0].values[0], Some(5.0));
    assert_eq!(
        find(&charts, "task-hours").series[3].values[0],
        Some(0.0),
        "no blocked hours"
    );
    let agents = find(&charts, "agents");
    assert_eq!(
        agents
            .series
            .iter()
            .map(|s| s.values[0])
            .collect::<Vec<_>>(),
        [Some(0.0), Some(2.125), Some(0.75), Some(0.0), Some(2.875)]
    );
    assert_eq!(
        find(&charts, "health").series[2].values[0],
        Some(12.0),
        "dispatch's and sweep's failed passes"
    );
    assert_eq!(
        find(&charts, "waits").series[1].values[0],
        None,
        "no such wait ended"
    );
    assert_eq!(figures(&page)[0].value, "5");
}
