use super::*;
use dioxus_ssr::render_element as render;
use serde_json::json;

/// The A pictures' task: certificate missing, dependency waived, nightly met, review met.
const FIXTURE: &str = include_str!("../../../../crates/symphony-api/fixtures/task-gates.json");
/// 2026-11-11 13:10 UTC, the day of every time in the fixture.
const NOW: f64 = 1_794_402_600.0;

fn fixture() -> TaskGates {
    serde_json::from_str(FIXTURE).unwrap()
}

fn gate(value: serde_json::Value) -> Gate {
    serde_json::from_value(value).unwrap()
}

fn answer(gates: Vec<Gate>) -> TaskGates {
    TaskGates {
        slug: "tools/x".into(),
        branch: "tools/x".into(),
        gates,
        ..TaskGates::default()
    }
}

/// The section, built inside the runtime its retry handler needs.
#[component]
fn Shown(gates: Gates) -> Element {
    rsx! { GatesView { gates, now: NOW, onretry: move |_| {} } }
}

fn shown(gates: Gates) -> String {
    render(rsx! { Shown { gates } })
}

/// Each `<li …>…</li>` of `html`, in order.
fn rows(html: &str) -> Vec<&str> {
    html.split("<li ")
        .skip(1)
        .map(|row| row.split("</li>").next().unwrap())
        .collect()
}

#[test]
fn the_pictures_task_shows_what_it_waits_on_its_counts_and_four_gates_missing_first() {
    let html = shown(Gates::Read(fixture()));
    assert!(
        html.starts_with(r#"<section class="task-gates card" data-gates="read" aria-labelledby="task-gates-title">"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<div class="gates-head"><h2 class="eyebrow" id="task-gates-title">Gates</h2><span class="pill gate-missing gates-waits">waits on certificate</span><span class="gates-counts">2 met · 1 waived · 1 missing</span></div>"#),
        "{html}"
    );
    let rows = rows(&html);
    let states: Vec<(&str, &str)> = rows
        .iter()
        .map(|row| {
            let attribute = |name: &str| {
                let start = row.find(&format!("{name}=\"")).unwrap() + name.len() + 2;
                &row[start..start + row[start..].find('"').unwrap()]
            };
            (attribute("data-gate"), attribute("data-state"))
        })
        .collect();
    assert_eq!(
        states,
        [
            ("certificate", "missing"),
            ("dependency", "waived"),
            ("nightly", "met"),
            ("review", "met"),
        ]
    );
    assert_eq!(
        rows[0],
        r#"class="gate gate-missing" data-gate="certificate" data-state="missing"><span class="gate-name">certificate</span><span class="pill gate-state">missing</span><span class="gate-detail">Verify on the merge tree 4be91c2 running, since main moved at 14:50</span><span class="gate-by">studio · 12:52</span>"#
    );
    assert_eq!(
        rows[1],
        r#"class="gate gate-waived" data-gate="dependency" data-state="waived"><span class="gate-name">dependency</span><span class="pill gate-state">waived</span><span class="gate-detail"><a href="/task/tools/slot-pool-by-tree">Waits on tools/slot-pool-by-tree, waived</a><span class="gate-why">: tools/slot-pool-by-tree shares no code with this branch</span></span><span class="gate-by">cobalt-otter · 12:12</span>"#
    );
    assert!(
        rows[2].contains(r#"<span class="pill gate-state">met</span><span class="gate-detail">Night of 11-10 green on main a71f0d3</span><span class="gate-by">nightly · 02:14</span>"#),
        "{}",
        rows[2]
    );
    assert!(
        rows[3].contains(r#"<span class="gate-detail"><a href="/task/tools/verify-stops-when-moved#message-124">Approved in round 2 of 2</a></span><span class="gate-by">lucid-heron · 12:05</span>"#),
        "{}",
        rows[3]
    );
}

#[test]
fn a_task_nothing_gates_shows_no_section_and_neither_does_one_the_board_has_not() {
    assert_eq!(shown(Gates::Read(answer(Vec::new()))), "");
    assert_eq!(Gates::of(Ok(None)), Gates::Read(TaskGates::default()));
    assert_eq!(shown(Gates::of(Ok(None))), "");
    assert_eq!(Gates::of(Ok(Some(fixture()))), Gates::Read(fixture()));
}

#[test]
fn a_failed_read_says_the_gates_could_not_be_read_and_offers_a_retry() {
    let failure = Gates::of(Err("The server answered 500.".into()));
    assert_eq!(failure, Gates::Failed("The server answered 500.".into()));
    let html = shown(failure);
    assert!(
        html.starts_with(r#"<section class="task-gates card" data-gates="failed" aria-labelledby="task-gates-title">"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<p class="gates-note" role="status">The gates could not be read: The server answered 500. The rest of the page still works.</p>"#),
        "{html}"
    );
    assert!(html.contains(">Retry</button>"), "{html}");
    assert!(!html.contains("<li"), "{html}");
    assert_eq!(
        failed(" "),
        "The gates could not be read. The rest of the page still works."
    );
}

#[test]
fn while_it_reads_the_section_holds_its_height_and_says_so() {
    let html = shown(Gates::Reading);
    assert!(
        html.contains(&format!(
            r#"data-gates="reading" aria-labelledby="task-gates-title" aria-busy="true" style="min-height:{HOLD}px;box-sizing:border-box">"#
        )),
        "{html}"
    );
    assert!(
        html.contains(r#"<p class="gates-note" role="status">Reading the gates…</p>"#),
        "{html}"
    );
    assert!(!html.contains("Retry"), "{html}");
}

#[test]
fn the_panel_draws_inside_a_slot_that_holds_its_place_until_the_answer() {
    let html = render(rsx! { TaskGatesPanel { slug: "tools/x", revision: 1.0 } });
    assert!(
        html.starts_with(r#"<div id="gates-tools/x" style="contain-intrinsic-size:auto 160px;min-height:0px" class="viewport-slot ">"#),
        "{html}"
    );
    assert!(html.contains(r#"data-gates="reading""#), "{html}");
}

#[test]
fn every_state_has_its_colour_class_and_unknown_is_what_a_failing_plugin_gives() {
    let html = shown(Gates::Read(answer(vec![
        gate(
            json!({"name":"hold","state":"held","by":"owner","why":"the owner holds it","detail":""}),
        ),
        gate(
            json!({"name":"review","state":"unknown","by":"shelf-gates","detail":"shelf-gates did not answer in 10 s"}),
        ),
        gate(json!({"name":"later","state":"expired","detail":"a word this build does not know"})),
    ])));
    let rows = rows(&html);
    assert!(
        rows[0].starts_with(r#"class="gate gate-held" data-gate="hold" data-state="held">"#),
        "{}",
        rows[0]
    );
    assert!(
        rows[0].contains(r#"<span class="gate-detail"><span class="gate-why">the owner holds it</span></span><span class="gate-by">owner</span>"#),
        "a why with no detail before it takes no colon: {}",
        rows[0]
    );
    assert!(
        rows[1].contains(r#"data-state="unknown"><span class="gate-name">review</span><span class="pill gate-state">unknown</span><span class="gate-detail">shelf-gates did not answer in 10 s</span><span class="gate-by">shelf-gates</span>"#),
        "{}",
        rows[1]
    );
    assert!(rows[2].contains(r#"data-state="unknown""#), "{}", rows[2]);
    assert!(
        html.contains(r#"<span class="pill gate-held gates-waits">waits on hold</span><span class="gates-counts">2 unknown · 1 held</span>"#),
        "{html}"
    );
}

#[test]
fn a_detail_draws_its_backticks_as_code_escapes_markup_and_links_only_words() {
    let html = shown(Gates::Read(answer(vec![
        gate(
            json!({"name":"check","state":"missing","detail":"`just verify` red at <b>9ac5149</b>: exit 1","commit":"9ac5149e0"}),
        ),
        gate(json!({"name":"owner","state":"held","detail":"","link":"/docs/DECISIONS.md#card"})),
    ])));
    let rows = rows(&html);
    assert!(
        rows[0].contains(r#"<span class="gate-detail"><code>just verify</code> red at &#60;b&#62;9ac5149&#60;/b&#62;: exit 1</span><span class="gate-by"></span>"#),
        "the commit the words name is not said again: {}",
        rows[0]
    );
    assert!(
        rows[1].contains(r#"<span class="gate-detail"></span>"#),
        "a link with no words to carry it draws nothing: {}",
        rows[1]
    );
}

#[test]
fn what_the_task_waits_on_names_every_missing_or_held_gate_in_their_order() {
    let named = |states: &[(&str, &str)]| {
        waits_on(&answer(
            states
                .iter()
                .map(|(name, state)| gate(json!({"name":name,"state":state})))
                .collect(),
        ))
    };
    assert_eq!(named(&[]), None);
    assert_eq!(named(&[("review", "met"), ("dependency", "waived")]), None);
    assert_eq!(named(&[("stamps", "unknown")]), None);
    assert_eq!(
        named(&[("certificate", "missing"), ("review", "met")]),
        Some((GateState::Missing, "waits on certificate".into()))
    );
    assert_eq!(
        named(&[("owner", "held"), ("certificate", "missing")]),
        Some((GateState::Held, "waits on owner and certificate".into()))
    );
    assert_eq!(
        named(&[
            ("certificate", "missing"),
            ("check", "missing"),
            ("owner", "held"),
            ("nightly", "met"),
        ]),
        Some((
            GateState::Missing,
            "waits on certificate, check and owner".into()
        ))
    );
}

#[test]
fn the_counts_go_from_what_no_longer_stands_in_the_way_to_what_the_task_waits_on() {
    let states = [
        "missing", "met", "held", "unknown", "waived", "met", "missing", "met",
    ];
    let every = answer(
        states
            .iter()
            .enumerate()
            .map(|(n, state)| gate(json!({"name":format!("g{n}"),"state":state})))
            .collect(),
    );
    assert_eq!(
        counts(&every),
        "3 met · 1 waived · 1 unknown · 1 held · 2 missing"
    );
    assert_eq!(counts(&fixture()), "2 met · 1 waived · 1 missing");
    assert_eq!(counts(&answer(Vec::new())), "");
}

#[test]
fn a_byline_says_who_when_and_the_commit_its_words_do_not_name() {
    let today = local_minutes(NOW);
    let line = |value: serde_json::Value| byline(&gate(value), today);
    assert_eq!(
        line(json!({"name":"a","state":"met","by":"lucid-heron","at":1_794_398_700.0})),
        "lucid-heron · 12:05"
    );
    assert_eq!(
        line(json!({"name":"a","state":"met","at":1_794_398_700.0})),
        "12:05"
    );
    assert_eq!(
        line(json!({"name":"a","state":"met","by":"nightly"})),
        "nightly"
    );
    assert_eq!(
        line(json!({"name":"a","state":"met","by":""})),
        "",
        "an empty handle says nothing"
    );
    assert_eq!(line(json!({"name":"a","state":"met"})), "");
    assert_eq!(
        line(
            json!({"name":"a","state":"met","by":"studio","at":1_794_363_240.0 - 86_400.0,"commit":"e19d3a0c4b7f2a9e"})
        ),
        "studio · 11-10 02:14 · e19d3a0",
        "a full commit id is said short, on another day with its date"
    );
    assert_eq!(
        line(json!({"name":"a","state":"met","commit":"e19d3a0","detail":"green at e19d3a0"})),
        ""
    );
    assert_eq!(
        line(
            json!({"name":"a","state":"waived","commit":"e19d3a0","why":"e19d3a0 is the same tree"})
        ),
        "",
        "nor one its why names"
    );
    assert_eq!(
        line(json!({"name":"a","state":"met","commit":""})),
        "",
        "an empty commit says nothing"
    );
}

#[test]
fn a_time_is_said_as_short_as_today_lets_it_be() {
    let at = |text: &str| crate::words::Moment::parse(text).unwrap().minutes();
    let now = at("2026-11-11 13:10");
    assert_eq!(when(at("2026-11-11 00:00"), now), "00:00");
    assert_eq!(when(at("2026-11-11 23:59"), now), "23:59");
    assert_eq!(when(at("2026-11-10 23:59"), now), "11-10 23:59");
    assert_eq!(when(at("2026-01-01 00:00"), now), "01-01 00:00");
    assert_eq!(
        when(at("2026-11-12 08:00"), now),
        "11-12 08:00",
        "a clock ahead of the reader's still says its day"
    );
    assert_eq!(when(at("2025-12-31 23:59"), now), "2025-12-31");
    assert_eq!(when(at("2024-02-29 12:00"), now), "2024-02-29");
    assert_eq!(when(at("1969-12-31 23:00"), now), "1969-12-31");
    assert_eq!(
        when(at("1969-12-31 23:00"), at("1969-12-31 23:30")),
        "23:00",
        "minutes before the epoch fall in the day before it"
    );
}

#[test]
fn local_minutes_on_the_host_are_utc_minutes_rounded_down() {
    assert_eq!(local_minutes(0.0), 0);
    assert_eq!(local_minutes(59.9), 0);
    assert_eq!(local_minutes(60.0), 1);
    assert_eq!(local_minutes(-1.0), -1);
    assert_eq!(local_minutes(NOW), 1_794_402_600 / 60);
}

#[test]
fn every_day_from_1900_to_2100_is_the_date_it_was_counted_from() {
    let from = crate::words::Moment::from_parts(1900, 1, 1, 0, 0)
        .unwrap()
        .minutes()
        / 1440;
    let to = crate::words::Moment::from_parts(2100, 12, 31, 0, 0)
        .unwrap()
        .minutes()
        / 1440;
    for day in from..=to {
        let (year, month, date) = civil(day);
        let counted = crate::words::Moment::from_parts(
            year,
            u32::try_from(month).unwrap(),
            u32::try_from(date).unwrap(),
            0,
            0,
        )
        .unwrap_or_else(|| panic!("day {day} is {year}-{month}-{date}"))
        .minutes()
            / 1440;
        assert_eq!(counted, day, "{year}-{month:02}-{date:02}");
    }
    assert_eq!(civil(0), (1970, 1, 1));
    assert_eq!(civil(-1), (1969, 12, 31));
}

#[test]
fn code_spans_pair_their_backticks_and_keep_a_lone_one() {
    assert_eq!(code_spans(""), Vec::<(bool, &str)>::new());
    assert_eq!(code_spans("plain words"), [(false, "plain words")]);
    assert_eq!(
        code_spans("`just verify` green at 4be91c2"),
        [(true, "just verify"), (false, " green at 4be91c2")]
    );
    assert_eq!(
        code_spans("ran `a` then `b`"),
        [(false, "ran "), (true, "a"), (false, " then "), (true, "b")]
    );
    assert_eq!(
        code_spans("an empty `` pair"),
        [(false, "an empty "), (true, ""), (false, " pair")]
    );
    assert_eq!(code_spans("one `left open"), [(false, "one `left open")]);
    assert_eq!(
        code_spans("`a` and one ` more"),
        [(true, "a"), (false, " and one ` more")]
    );
}
