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

fn room(count: usize, waiting: usize, drawn: Option<(u32, u32)>) -> Room {
    Room {
        count,
        waiting,
        drawn,
    }
}

/// The section, built inside the runtime its retry handler needs, in a window `width` px wide.
#[component]
fn Shown(gates: Gates, room: Room, width: Option<u32>) -> Element {
    rsx! { GatesView { gates, now: NOW, room, width, onretry: move |_| {} } }
}

fn shown(gates: Gates) -> String {
    render(rsx! { Shown { gates, room: DEFAULT_ROOM, width: None } })
}

fn holding(room: Room, width: Option<u32>) -> String {
    render(rsx! { Shown { gates: Gates::Reading, room, width } })
}

/// Each `<li …>…</li>` of `html`, in order.
fn rows(html: &str) -> Vec<&str> {
    html.split("<li ")
        .skip(1)
        .map(|row| row.split("</li>").next().unwrap())
        .collect()
}

/// A gate's row as drawn: its name (and link), state, detail and who-and-when markup.
fn row(name: &str, state: &str, detail: &str, by: &str) -> String {
    format!(
        r#"class="gate gate-{state}" data-gate="{name}" data-state="{state}"><span class="gate-name"><span class="gate-label" dir="auto" title="{name}">{name}</span></span><span class="pill gate-state">{state}</span>{detail}{by}"#,
    )
}

#[test]
fn the_pictures_task_shows_what_it_waits_on_its_counts_and_four_gates_missing_first() {
    let html = shown(Gates::Read(fixture()));
    assert!(
        html.starts_with(r#"<section class="task-gates card" data-gates="read" aria-labelledby="task-gates-title">"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<div class="gates-head"><h2 class="eyebrow" id="task-gates-title">Gates</h2><span class="pill gate-missing gates-waits">waits on <span dir="auto">certificate</span></span><span class="gates-counts">2 met · 1 waived · 1 missing</span></div>"#),
        "{html}"
    );
    let rows = rows(&html);
    assert_eq!(rows.len(), 4, "{html}");
    assert_eq!(
        rows[0],
        row(
            "certificate",
            "missing",
            r#"<span class="gate-detail" title="Verify on the merge tree 4be91c2 running, since main moved at 14:50"><span dir="auto">Verify on the merge tree 4be91c2 running, since main moved at 14:50</span></span>"#,
            r#"<span class="gate-by" title="studio · 12:52"><span dir="auto">studio</span> · <span dir="auto">12:52</span></span>"#
        )
    );
    assert_eq!(
        rows[1],
        row(
            "dependency",
            "waived",
            r#"<span class="gate-detail" title="Waits on tools/slot-pool-by-tree, waived: tools/slot-pool-by-tree shares no code with this branch"><span dir="auto">Waits on <a href="/task/tools/slot-pool-by-tree">tools/slot-pool-by-tree</a>, waived</span><span class="gate-why">: <span dir="auto">tools/slot-pool-by-tree shares no code with this branch</span></span></span>"#,
            r#"<span class="gate-by" title="cobalt-otter · 12:12"><span dir="auto">cobalt-otter</span> · <span dir="auto">12:12</span></span>"#
        ),
        "only the slug the link names is linked"
    );
    assert!(
        rows[2].contains(r#"<span class="gate-detail" title="Night of 11-10 green on main a71f0d3"><span dir="auto">Night of 11-10 green on main a71f0d3</span></span>"#),
        "{}",
        rows[2]
    );
    assert!(
        rows[3].contains(r#"<span dir="auto">Approved in <a href="/task/tools/verify-stops-when-moved#message-124">round 2 of 2</a></span>"#),
        "only the round is linked: {}",
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
fn a_read_the_server_never_answered_says_it_took_too_long_not_to_check_before_sending() {
    let slow = Gates::of(Err(crate::task_save::NO_ANSWER.into()));
    assert_eq!(slow, Gates::Slow);
    let html = shown(slow);
    assert!(
        html.contains(r#"<p class="gates-note" role="status">The gates took too long to answer and were given up on. The rest of the page still works.</p>"#),
        "{html}"
    );
    assert!(!html.contains("sending again"), "{html}");
    assert!(html.contains(">Retry</button>"), "{html}");
}

#[test]
fn while_it_reads_the_section_is_its_own_outline_a_shadow_row_a_gate_it_expects() {
    let html = holding(room(4, 1, None), None);
    assert!(
        html.starts_with(r#"<section class="task-gates card" data-gates="reading" aria-labelledby="task-gates-title" aria-busy="true" style="">"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<span class="pill gates-waits gate-shadow"><span class="gate-bar" style="--w:9em"></span></span><span class="gates-counts"><span class="gate-bar" style="--w:11em"></span></span>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<p class="gates-said" role="status">Reading the gates…</p>"#),
        "{html}"
    );
    let rows = rows(&html);
    assert_eq!(rows.len(), 4, "{html}");
    for row in &rows {
        assert!(row.starts_with(r#"class="gate gate-shadow">"#), "{row}");
        assert!(
            row.contains("<span class=\"pill gate-state\">\u{a0}</span>"),
            "the state's own pill, empty: {row}"
        );
        for part in ["gate-label", "gate-detail", "gate-by"] {
            assert!(row.contains(&format!(r#"class="{part}""#)), "{part}: {row}");
        }
    }
    assert!(!html.contains("Retry"), "{html}");
    let calm = holding(room(2, 0, None), None);
    assert_eq!(rows_of(&calm), 2);
    assert!(!calm.contains("gates-waits"), "nothing to wait on: {calm}");
    assert_eq!(
        holding(room(0, 0, None), None),
        "",
        "none is held for a task known to have no gates"
    );
}

fn rows_of(html: &str) -> usize {
    rows(html).len()
}

#[test]
fn the_outline_holds_the_height_this_browser_drew_in_a_window_as_wide() {
    let drawn = room(4, 1, Some((390, 612)));
    assert!(
        holding(drawn, Some(390)).contains(r#"aria-busy="true" style="min-height:612px">"#),
        "{}",
        holding(drawn, Some(390))
    );
    for width in [Some(391), Some(1280), None] {
        assert!(
            holding(drawn, width).contains(r#"aria-busy="true" style="">"#),
            "{width:?}"
        );
    }
}

#[test]
fn the_panel_is_the_outline_the_server_or_this_browser_expects_until_it_reads() {
    let panel = |seen: Option<GatesSeen>| {
        render(rsx! { TaskGatesPanel { slug: "tools/x", revision: 1.0, seen } })
    };
    let html = panel(None);
    assert!(
        html.starts_with(r#"<div id="gates-tools/x" class="task-gates-slot"><section class="task-gates card" data-gates="reading""#),
        "{html}"
    );
    assert_eq!(rows_of(&html), 4, "four, nothing known: {html}");
    let two = panel(Some(GatesSeen {
        count: 2,
        waiting: 0,
    }));
    assert_eq!(rows_of(&two), 2, "{two}");
    assert!(!two.contains("gates-waits"), "{two}");
    assert_eq!(
        panel(Some(GatesSeen::default())),
        r#"<div id="gates-tools/x" class="task-gates-slot"></div>"#,
        "a task the server last answered with no gates holds no room"
    );
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
        rows[0].contains(r#"<span class="gate-detail" title="the owner holds it"><span class="gate-why"><span dir="auto">the owner holds it</span></span></span>"#),
        "a why with no detail before it takes no colon: {}",
        rows[0]
    );
    assert!(
        rows[1].contains(r#"<span class="pill gate-state">unknown</span><span class="gate-detail" title="shelf-gates did not answer in 10 s"><span dir="auto">shelf-gates did not answer in 10 s</span></span>"#),
        "{}",
        rows[1]
    );
    assert!(rows[2].contains(r#"data-state="unknown""#), "{}", rows[2]);
    assert!(
        html.contains(r#"<span class="pill gate-held gates-waits">waits on <span dir="auto">hold</span></span><span class="gates-counts">2 unknown · 1 held</span>"#),
        "{html}"
    );
}

#[test]
fn a_detail_draws_its_backticks_as_code_escapes_markup_and_links_only_its_evidence() {
    let html = shown(Gates::Read(answer(vec![
        gate(
            json!({"name":"check","state":"missing","detail":"`just verify` red at <b>9ac5149</b>: exit 1","commit":"9ac5149e0"}),
        ),
        gate(
            json!({"name":"verify","state":"met","detail":"`just verify` green at `4be91c2`","commit":"4be91c2","link":"https://ci.example/runs/77"}),
        ),
    ])));
    let rows = rows(&html);
    assert!(
        rows[0].contains(r#"<span dir="auto"><code>just verify</code> red at &#60;b&#62;9ac5149&#60;/b&#62;: exit 1</span></span><span class="gate-by" title=""></span>"#),
        "the commit the words name is not said again: {}",
        rows[0]
    );
    assert!(
        rows[1].contains(r#"<span dir="auto"><code>just verify</code> green at <a href="https://ci.example/runs/77"><code>4be91c2</code></a></span>"#),
        "the commit's words carry the link, still code: {}",
        rows[1]
    );
}

#[test]
fn a_link_with_no_words_to_stand_on_goes_on_the_gates_name() {
    let html = shown(Gates::Read(answer(vec![gate(
        json!({"name":"owner","state":"held","detail":"","link":"/docs/DECISIONS.md#card"}),
    )])));
    assert!(
        rows(&html)[0].contains(r#"<span class="gate-label" dir="auto" title="owner"><a href="/docs/DECISIONS.md#card">owner</a></span>"#),
        "{html}"
    );
}

/// The links a plugin could hand a page that would run script, read data in place or leave for
/// another host looking like a path of the app.
const UNSAFE: [&str; 9] = [
    "javascript:void(document.title='pwned')",
    "JaVaScRiPt:alert(1)",
    " javascript:alert(1)",
    "\u{1}javascript:alert(1)",
    "data:text/html,<script>alert(1)</script>",
    "vbscript:msgbox(1)",
    "//evil.example/x",
    "/\\evil.example/x",
    "\tjavascript:alert(1)",
];

#[test]
fn a_link_that_is_no_path_of_the_app_or_web_address_is_drawn_as_its_words() {
    for link in UNSAFE {
        for detail in ["Open the evidence", ""] {
            let html = shown(Gates::Read(answer(vec![gate(
                json!({"name":"plugin","state":"met","detail":detail,"link":link}),
            )])));
            assert!(!html.contains("<a "), "{link:?} {detail:?}: {html}");
            assert!(!html.contains("href"), "{link:?} {detail:?}: {html}");
            assert!(html.contains(">plugin<"), "{html}");
        }
    }
}

#[test]
fn every_part_of_a_gate_keeps_its_own_direction() {
    let html = shown(Gates::Read(answer(vec![gate(json!({
        "name":"بوابة","state":"missing","by":"لوسيد-هيرون","at":1_794_398_700.0,
        "detail":"تحقق من الشجرة","why":"السبب"
    }))])));
    for part in [
        r#"<span class="gate-label" dir="auto" title="بوابة">بوابة</span>"#,
        r#"<span dir="auto">تحقق من الشجرة</span>"#,
        r#"<span class="gate-why">: <span dir="auto">السبب</span></span>"#,
        r#"<span dir="auto">لوسيد-هيرون</span> · <span dir="auto">12:05</span>"#,
        r#"waits on <span dir="auto">بوابة</span>"#,
    ] {
        assert!(html.contains(part), "{part}: {html}");
    }
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
    let names = |state: GateState, names: &[&str]| {
        Some((state, names.iter().map(|n| n.to_string()).collect()))
    };
    assert_eq!(named(&[]), None);
    assert_eq!(named(&[("review", "met"), ("dependency", "waived")]), None);
    assert_eq!(named(&[("stamps", "unknown")]), None);
    assert_eq!(
        named(&[("certificate", "missing"), ("review", "met")]),
        names(GateState::Missing, &["certificate"])
    );
    assert_eq!(
        named(&[("owner", "held"), ("certificate", "missing")]),
        names(GateState::Held, &["owner", "certificate"])
    );
    let three = named(&[
        ("certificate", "missing"),
        ("check", "missing"),
        ("owner", "held"),
        ("nightly", "met"),
    ]);
    assert_eq!(
        three,
        names(GateState::Missing, &["certificate", "check", "owner"])
    );
    let html = shown(Gates::Read(answer(
        ["certificate", "check", "owner"]
            .map(|name| gate(json!({"name":name,"state":"missing"})))
            .to_vec(),
    )));
    assert!(
        html.contains(r#"waits on <span dir="auto">certificate</span>, <span dir="auto">check</span> and <span dir="auto">owner</span></span>"#),
        "{html}"
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
    let line = |value: serde_json::Value| byline(&gate(value), today).join(" · ");
    assert_eq!(
        byline(
            &gate(json!({"name":"a","state":"met","by":"lucid-heron","at":1_794_398_700.0})),
            today
        ),
        ["lucid-heron", "12:05"]
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
fn a_links_evidence_is_the_name_its_path_says_else_the_commit_else_the_round_else_all() {
    let linked = |detail: &str, link: &str, commit: Option<&str>| {
        let range = evidence(detail, link, commit);
        detail[range].to_string()
    };
    let dependency = "Waits on tools/slot-pool-by-tree, waived";
    assert_eq!(
        linked(dependency, "/task/tools/slot-pool-by-tree", None),
        "tools/slot-pool-by-tree"
    );
    assert_eq!(
        linked(
            "Card c in docs/analysis/x.md waits on the owner",
            "/docs/analysis/x.md#c",
            None
        ),
        "docs/analysis/x.md"
    );
    assert_eq!(
        linked(
            "Verify on 4be91c2a running",
            "https://ci.example/runs/9",
            Some("4be91c2")
        ),
        "4be91c2a",
        "the commit as the words say it, longer than the gate's"
    );
    assert_eq!(
        linked(
            "CI run 4521 passed",
            "https://ci.example/runs/4521?x=1",
            None
        ),
        "4521",
        "a web address's own path"
    );
    assert_eq!(
        linked(
            "Approved in round 2 of 2",
            "/task/tools/x#message-124",
            None
        ),
        "round 2 of 2"
    );
    assert_eq!(
        linked(
            "Changes asked in Round 1: the task moves",
            "/task/tools/x#message-9",
            None
        ),
        "Round 1",
        "never the route's own word `task`"
    );
    assert_eq!(
        linked("Played around 3 times", "/task/tools/x", None),
        "Played around 3 times",
        "`around 3` is no round"
    );
    assert_eq!(linked("all of it", "https://ci.example", None), "all of it");
    assert_eq!(
        linked(
            "tools/slot-pool-by-tree-two waits",
            "/task/tools/slot-pool-by-tree",
            None
        ),
        "tools/slot-pool-by-tree-two waits",
        "a name inside a longer one is not it"
    );
    assert_eq!(linked("", "/task/x", None), "");
    let url = "https://ci.example.com/builds/0123abcd/log?attempt=3";
    assert_eq!(
        linked(
            &format!("Verify {url} is running on a merge tree"),
            url,
            None
        ),
        url,
        "the link itself, where the words write it out"
    );
    assert_eq!(
        linked(
            "Verify https://ci.example.com/builds/0123abcd/log, attempt 3",
            url,
            None
        ),
        "https://ci.example.com/builds/0123abcd/log",
        "without its query, a sentence's comma after it"
    );
    assert_eq!(
        linked(
            "See /task/tools/x.",
            "/task/tools/x#message-9",
            Some("4be91c2")
        ),
        "/task/tools/x",
        "a path of the app written out"
    );
    assert_eq!(
        linked(
            "Verify https://ci.example.com/builds/0123abcd/log/raw is slow",
            "https://ci.example.com/builds/0123abcd/log",
            None
        ),
        "Verify https://ci.example.com/builds/0123abcd/log/raw is slow",
        "a longer address that starts like it is not it, nor are its own path's parts"
    );
}

#[test]
fn pieces_cut_code_spans_around_the_linked_words() {
    let text = "`just verify` green at `4be91c2` now";
    assert_eq!(
        pieces(text, None),
        [
            vec![
                (true, "just verify"),
                (false, " green at "),
                (true, "4be91c2"),
                (false, " now")
            ],
            vec![],
            vec![]
        ]
    );
    let at = text.find("4be91c2").unwrap();
    assert_eq!(
        pieces(text, Some(at..at + 7)),
        [
            vec![(true, "just verify"), (false, " green at ")],
            vec![(true, "4be91c2")],
            vec![(false, " now")]
        ]
    );
    let green = text.find("green").unwrap();
    assert_eq!(
        pieces(text, Some(green..green + 5)),
        [
            vec![(true, "just verify"), (false, " ")],
            vec![(false, "green")],
            vec![(false, " at "), (true, "4be91c2"), (false, " now")]
        ]
    );
    assert_eq!(
        pieces(text, Some(0..text.len())),
        [vec![], code_spans(text), vec![]],
        "all of it"
    );
    assert_eq!(pieces("", None), [vec![], vec![], vec![]]);
}

#[test]
fn the_room_is_the_servers_count_with_this_browsers_height_while_they_agree() {
    let seen = |count, waiting| Some(GatesSeen { count, waiting });
    let kept = room(4, 1, Some((390, 612)));
    assert_eq!(Room::of(None, None), DEFAULT_ROOM);
    assert_eq!(Room::of(None, Some(kept)), kept);
    assert_eq!(Room::of(seen(4, 1), Some(kept)), kept);
    assert_eq!(
        Room::of(seen(4, 2), Some(kept)),
        room(4, 2, None),
        "the server's, when it says otherwise"
    );
    assert_eq!(Room::of(seen(0, 0), None), room(0, 0, None));
    assert_eq!(kept.held(Some(390)), Some(612));
    assert_eq!(kept.held(Some(1280)), None);
    assert_eq!(kept.held(None), None);
    assert_eq!(room(4, 1, None).held(Some(390)), None);
}

#[test]
fn a_browser_keeps_the_newest_room_of_each_task_and_at_most_sixty_four() {
    let kept = keep("", "tools/x", room(4, 1, Some((390, 612))));
    assert_eq!(kept, "4 1 390 612 tools/x");
    assert_eq!(recall(&kept, "tools/x"), Some(room(4, 1, Some((390, 612)))));
    assert_eq!(recall(&kept, "tools/y"), None);
    let kept = keep(&kept, "tools/y", room(0, 0, None));
    assert_eq!(kept, "0 0 0 0 tools/y\n4 1 390 612 tools/x");
    assert_eq!(recall(&kept, "tools/y"), Some(room(0, 0, None)));
    let kept = keep(&kept, "tools/x", room(2, 0, Some((1280, 140))));
    assert_eq!(
        kept, "2 0 1280 140 tools/x\n0 0 0 0 tools/y",
        "one line a task"
    );
    let mut many = String::new();
    for n in 0..70 {
        many = keep(&many, &format!("t/{n}"), room(1, 0, None));
    }
    assert_eq!(many.lines().count(), 64);
    assert_eq!(recall(&many, "t/69"), Some(room(1, 0, None)));
    assert_eq!(recall(&many, "t/5"), None, "the oldest went");
    assert_eq!(recall("not a room\n1 x 0 0 tools/x", "tools/x"), None);
    assert_eq!(
        recall("3 1 0 0 tools/with space", "tools/with space"),
        Some(room(3, 1, None))
    );
}

#[test]
fn a_task_reads_its_gates_again_on_its_verdicts_its_branch_and_its_documents_cards() {
    let links = ["docs/analysis/x.md#Pictures".to_string()];
    let concerned = |nudge: Nudge| concerns(&nudge, "tools/x", "tools/x-branch", &links);
    assert!(concerned(Nudge::Verdict("tools/x".into())));
    assert!(!concerned(Nudge::Verdict("tools/y".into())));
    assert!(concerned(Nudge::Branch("tools/x-branch".into())));
    assert!(!concerned(Nudge::Branch("tools/x".into())));
    assert!(concerned(Nudge::Card("docs/analysis/x.md".into())));
    assert!(!concerned(Nudge::Card("docs/analysis/y.md".into())));
    assert!(!concerns(
        &Nudge::Card("docs/analysis/x.md".into()),
        "tools/x",
        "tools/x",
        &[]
    ));
}

fn operation(task: &str, nonce: &str, state: &str) -> symphony_api::WorkOperation {
    serde_json::from_value(json!({
        "machine_id":"alpha", "operation_id":nonce, "parent_id":null,
        "handle":"worker-a", "task":task, "checkout":"/c",
        "recipe":"just test-rs", "kind":"test", "state":state,
        "resource":"run_slot", "reason":"", "phase_since":1.0,
    }))
    .unwrap()
}

fn work(operations: Vec<symphony_api::WorkOperation>) -> MachineWork {
    serde_json::from_value(json!({
        "freshness":"fresh", "unassigned":[], "machines":[{
            "machine_id":"alpha", "label":"alpha", "producer_id":"p", "sequence":1,
            "age_ms":0, "freshness":"fresh", "complete":true, "reason":"",
            "counts":null, "operations":operations,
        }]
    }))
    .unwrap()
}

#[test]
fn a_tasks_work_changes_when_its_runs_start_move_on_or_end_and_not_otherwise() {
    let queued = work(vec![
        operation("tools/x", "n1", "queued"),
        operation("tools/y", "n2", "running"),
    ]);
    let running = work(vec![
        operation("tools/x", "n1", "running"),
        operation("tools/y", "n2", "running"),
    ]);
    let mut later = running.clone();
    later.machines[0].sequence = 9;
    later.machines[0].age_ms = Some(4_000);
    later.machines[0].operations[0].phase_since = Some(30.0);
    later.machines[0].operations.remove(1);
    let ended = work(vec![operation("tools/y", "n2", "running")]);
    assert_ne!(work_of(&queued, "tools/x"), work_of(&running, "tools/x"));
    assert_eq!(
        work_of(&running, "tools/x"),
        work_of(&later, "tools/x"),
        "another task's run ending, or the clock, changes nothing"
    );
    assert_ne!(work_of(&running, "tools/x"), work_of(&ended, "tools/x"));
    assert_eq!(work_of(&ended, "tools/x"), "");
    assert_eq!(work_of(&MachineWork::default(), "tools/x"), "");
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
