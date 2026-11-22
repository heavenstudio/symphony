//! The wire format, pinned by the fixtures the client's tests and the server's tests share:
//! each fixture reads into the types and writes back as the same JSON.

use serde_json::Value;
use symphony_api::{Board, Change, Delta, Front};
#[path = "wire/board_fields.rs"]
mod board_fields;
#[path = "wire/cache_format.rs"]
mod cache_format;
#[path = "wire/first_part.rs"]
mod first_part;
#[path = "wire/fixture.rs"]
mod fixture;
#[path = "wire/listing.rs"]
mod listing;
#[path = "wire/machine_work.rs"]
mod machine_work;
#[path = "wire/reader_tests.rs"]
mod reader_tests;
#[path = "wire/section_ends.rs"]
mod section_ends;
#[path = "wire/task_diff.rs"]
mod task_diff;
#[path = "wire/waits_on.rs"]
mod waits_on;

fn fixture(name: &str) -> String {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let reader =
        repository_read::Reader::process(root.clone()).expect("a valid repository audit context");
    fixture::read(&reader, &root.join("fixtures"), name).expect(name)
}

/// The fixture read into `T` and written back, as JSON values, beside the fixture itself.
fn round_trip<T: serde::de::DeserializeOwned + serde::Serialize>(name: &str) -> (Value, Value) {
    let text = fixture(name);
    let typed: T = serde_json::from_str(&text).expect(name);
    let written = serde_json::to_value(&typed).expect("a wire type serializes");
    (written, serde_json::from_str(&text).expect(name))
}

/// This package's tests link `serde_json` with the features the server's build unifies on it (the
/// dev-dependency in its Cargo.toml), so a test here reads JSON as the server, the `tasks` binary and
/// the workspace's run do: keys kept in the order written (`preserve_order`), a number kept as its text
/// (`arbitrary_precision`), a float read to its nearest (`float_roundtrip`), and raw values
/// (`raw_value`, which this target would not build without).
#[test]
fn these_tests_read_json_as_the_server_links_serde_json() {
    let value: Value = serde_json::from_str(r#"{"b":1.50,"a":2}"#).unwrap();
    assert_eq!(value.to_string(), r#"{"b":1.50,"a":2}"#);
    // One ulp off without float_roundtrip: 51.24817837550541.
    let float: f64 = serde_json::from_str("51.248178375505404").unwrap();
    assert_eq!(float.to_bits(), 51.248_178_375_505_404_f64.to_bits());
    let raw = serde_json::value::RawValue::from_string("[1.50]".into()).unwrap();
    assert_eq!(raw.get(), "[1.50]");
}

#[test]
fn the_board_reads_and_writes_back_as_the_same_json() {
    let (written, fixture) = round_trip::<Board>("board.json");
    assert_eq!(written, fixture);
}

/// The gates of the task the approved pictures draw (docs/product/analysis/reports-gates-a-*.svg): what the
/// gates panel and the timeline read.
#[test]
fn a_tasks_gates_read_and_write_back_as_the_same_json_in_the_order_the_page_draws() {
    use symphony_api::{GateState, TaskGates};
    let (written, fixture) = round_trip::<TaskGates>("task-gates.json");
    assert_eq!(written, fixture);
    let gates: TaskGates = serde_json::from_str(&self::fixture("task-gates.json")).unwrap();
    assert_eq!(
        gates
            .gates
            .iter()
            .map(|gate| (gate.name.as_str(), gate.state))
            .collect::<Vec<_>>(),
        [
            ("certificate", GateState::Missing),
            ("dependency", GateState::Waived),
            ("nightly", GateState::Met),
            ("review", GateState::Met),
        ]
    );
    assert_eq!(
        gates.gates,
        symphony_api::gates::ranked(gates.gates.clone()),
        "the fixture is in the order the server sends"
    );
    let waived = &gates.gates[1];
    assert_eq!(
        waived.why.as_deref(),
        Some("tools/slot-pool-by-tree shares no code with this branch")
    );
    let review = &gates.gates[3];
    assert_eq!(review.past.len(), 1);
    assert_eq!(review.past[0].state, GateState::Missing);
    assert!(review.past[0].at < review.at.unwrap());
}

/// The timeline of the task the approved pictures draw (docs/product/analysis/reports-timeline-*.svg): what
/// the server answers for it and the task page draws.
#[test]
fn a_tasks_timeline_reads_and_writes_back_as_the_same_json_with_the_pictures_totals() {
    use symphony_api::timeline::{RunResult, Verdict};
    use symphony_api::{GateState, TaskTimeline};
    let (written, fixture) = round_trip::<TaskTimeline>("task-timeline.json");
    assert_eq!(written, fixture);
    let timeline: TaskTimeline =
        serde_json::from_str(&self::fixture("task-timeline.json")).unwrap();
    // 6 h 18 m so far, the machine's 1 h 33 m and 26 m queued, as the pictures' heading says.
    assert_eq!(
        timeline.end() - timeline.created,
        6.0 * 3600.0 + 18.0 * 60.0
    );
    assert_eq!(timeline.machine_seconds(), 93.0 * 60.0);
    assert_eq!(timeline.queued_seconds(), 26.0 * 60.0);
    assert_eq!(
        timeline
            .rounds
            .iter()
            .map(|round| round.verdict)
            .collect::<Vec<_>>(),
        [Some(Verdict::Changes), Some(Verdict::Approved)]
    );
    assert_eq!(timeline.bounces, 1);
    let last = timeline.stages.last().unwrap();
    assert!(last.approved && last.to.is_none(), "it waits to land now");
    assert_eq!(timeline.runs.last().unwrap().result, RunResult::Running);
    // The stages follow one another from the task's creation.
    assert_eq!(timeline.stages[0].from, timeline.created);
    for pair in timeline.stages.windows(2) {
        assert_eq!(pair[0].to, Some(pair[1].from));
    }
    let certificate: Vec<GateState> = timeline.gates[0]
        .states
        .iter()
        .map(|span| span.state)
        .collect();
    assert_eq!(
        certificate,
        [GateState::Missing, GateState::Met, GateState::Missing],
        "met, then missing again once main moved"
    );
}

/// A task page carries the room its gates took when the server last answered them, and none
/// before; a page from a server that never sends it reads as none.
#[test]
fn a_task_page_says_the_room_its_gates_took_once_the_server_has_answered_them() {
    use symphony_api::{GatesSeen, TaskPage};
    let page = |gates: Option<Value>| {
        let mut page = serde_json::json!({
            "task": {"slug": "tools/x", "title": "X", "body": "", "status": "review",
                "priority": 1, "parent": null, "created_by": "owner", "created": 0.0,
                "updated": 0.0, "links": [], "depends": [], "roles": [], "instruction": false},
            "blocks": [], "children": [], "children_count": 0
        });
        if let Some(gates) = gates {
            page["gates"] = gates;
        }
        serde_json::from_value::<TaskPage>(page).unwrap()
    };
    let none = page(None);
    assert_eq!(none.gates, None);
    assert!(serde_json::to_value(&none).unwrap().get("gates").is_none());
    let seen = page(Some(
        serde_json::json!({"count": 4, "waiting": 1, "unsigned": 1}),
    ));
    assert_eq!(
        seen.gates,
        Some(GatesSeen {
            count: 4,
            waiting: 1,
            unsigned: 1
        })
    );
    assert_eq!(
        serde_json::to_value(&seen).unwrap()["gates"],
        serde_json::json!({"count": 4, "waiting": 1, "unsigned": 1})
    );
    assert_eq!(
        page(Some(serde_json::json!({"count": 0}))).gates,
        Some(GatesSeen::default())
    );
}

#[test]
fn the_board_names_the_client_build_served_and_how_often_the_feed_pings() {
    let board: Board = serde_json::from_str(&fixture("board.json")).unwrap();
    assert_eq!(board.build, "/assets/symphony-shell-0123456789abcdef.js");
    assert_eq!(board.ping_ms, 5_000);
    assert!(board.ping_ms <= symphony_api::PING_MAX_MS);
}

/// The owner reads the board from a phone over the tailnet, where a stream goes silent but open on
/// every network change: a client gives such a stream up after two and a half pings, which no
/// server may stretch past 12.5 s (ruled on tools/python-zero/ui-board).
#[test]
fn no_server_lets_a_silent_stream_hold_past_twelve_and_a_half_seconds() {
    let given_up_ms = u64::from(symphony_api::PING_MAX_MS) * 5 / 2;
    assert!(
        given_up_ms <= 12_500,
        "a silent stream is given up only after {given_up_ms} ms"
    );
}

#[test]
fn an_empty_board_without_a_task_board_reads_and_writes_back_as_the_same_json() {
    let (written, fixture) = round_trip::<Board>("board-empty.json");
    assert_eq!(written, fixture);
    let board: Board = serde_json::from_str(&self::fixture("board-empty.json")).unwrap();
    assert_eq!(board.next, Vec::<symphony_api::Card>::new());
    assert_eq!(
        board.counts.tasks, None,
        "a server with no task board sends null"
    );
}

#[test]
fn every_region_of_a_delta_reads_and_writes_back_as_the_same_json() {
    for name in ["delta-card.json", "delta-regions.json"] {
        let (written, fixture) = round_trip::<Delta>(name);
        assert_eq!(written, fixture, "{name}");
    }
    let delta: Delta = serde_json::from_str(&fixture("delta-regions.json")).unwrap();
    let regions: Vec<&str> = delta
        .changes
        .iter()
        .map(|change| match change {
            Change::Card(_) => "card",
            Change::Queue(_) => "queue",
            Change::Counts(_) => "counts",
            Change::Progress(_) => "progress",
            Change::Client { .. } => "client",
            Change::TaskAsks(_) => "task-asks",
            Change::Other => "other",
        })
        .collect();
    assert_eq!(regions, ["queue", "counts", "progress", "client"]);
}

#[test]
fn a_card_links_to_its_anchor_in_its_document() {
    let board: Board = serde_json::from_str(&fixture("board.json")).unwrap();
    assert_eq!(
        board.next[3].href(),
        "/docs/game/STATIONS.md#dock-menu-order"
    );
}

#[test]
fn a_card_without_pictures_summary_or_date_leaves_them_out() {
    let board: Board = serde_json::from_str(&fixture("board.json")).unwrap();
    let card = serde_json::to_value(&board.next[2]).unwrap();
    assert!(
        card.get("summary").is_none(),
        "an empty summary is not sent: {card}"
    );
    let bare = serde_json::to_value(&board.next[1]).unwrap();
    assert!(
        bare.get("pictures").is_none(),
        "no pictures, no field: {bare}"
    );
    let lantern = &board.next[4].pictures[2];
    assert_eq!(
        (lantern.src.as_deref(), lantern.waiting),
        (None, false),
        "a lamp not drawn yet"
    );
}

#[test]
fn a_region_this_client_does_not_know_yet_reads_as_other() {
    let text = r#"{"version": 9, "changes": [
        {"region": "activity", "data": [{"kind": "comment"}]},
        {"region": "counts", "data": {"waiting": 1, "docs_waiting": 1, "spot_check": 0, "answered_today": 0}}
    ]}"#;
    let delta: Delta =
        serde_json::from_str(text).expect("an unknown region must not refuse the event");
    assert_eq!(delta.changes[0], Change::Other);
    assert!(
        matches!(&delta.changes[1], Change::Counts(counts) if counts.waiting == 1 && counts.tasks.is_none())
    );
}

#[test]
fn a_change_names_its_region_before_its_data_and_a_known_region_carries_data() {
    let data_first = r#"{"version": 9, "changes": [{"data": {"build": "x"}, "region": "client"}]}"#;
    let err = serde_json::from_str::<Delta>(data_first)
        .unwrap_err()
        .to_string();
    assert!(err.contains("names its region before its data"), "{err}");
    let no_data = r#"{"version": 9, "changes": [{"region": "counts"}]}"#;
    let err = serde_json::from_str::<Delta>(no_data)
        .unwrap_err()
        .to_string();
    assert!(err.contains("the counts region has no data"), "{err}");
    let unknown_bare = r#"{"version": 9, "changes": [{"region": "activity"}]}"#;
    assert_eq!(
        serde_json::from_str::<Delta>(unknown_bare).unwrap().changes,
        [Change::Other]
    );
}

#[test]
fn a_front_reads_with_or_without_the_old_census_fields_and_always_sends_its_orphans() {
    let bare = r#"{"pid": 7, "port": 8765, "root": "/r", "ready": true, "app": null,
        "tailnet": null}"#;
    let front: Front = serde_json::from_str(bare).expect("a server that counts nothing answers");
    assert_eq!(front.orphans, 0);
    let counted = bare.replace(
        '}',
        r#", "orphans": 2, "uncounted": true, "census_pending": true}"#,
    );
    let older: Front = serde_json::from_str(&counted).expect("an older server still answers");
    assert_eq!(older.orphans, 2);
    // A starter built when the server counted leftovers requires the field.
    let sent = serde_json::to_value(&front).unwrap();
    assert_eq!(sent["orphans"], 0, "{sent}");
}

#[test]
fn the_project_the_page_inlines_reads_and_writes_back_as_the_same_json() {
    let (written, fixture) = round_trip::<symphony_api::ProjectView>("project.json");
    assert_eq!(written, fixture);
    let plain: symphony_api::ProjectView =
        serde_json::from_str(r#"{"name":"Orbit","register":"notes/register.md"}"#).unwrap();
    assert!(
        plain.nav.is_empty(),
        "a project without plugins has no pages of theirs"
    );
    assert_eq!(plain.entrypoints(), ["docs"], "none sent is the convention");
}

/// An hour of the board's health as the shared server appends it, a line of a day file in
/// `symphony/health/`: what the Reports page reads.
#[test]
fn an_hour_of_the_boards_health_reads_and_writes_back_as_the_same_json() {
    use symphony_api::{HEALTH_SCHEMA, HealthHour, HourPasses};
    let (written, fixture) = round_trip::<HealthHour>("health-hour.json");
    assert_eq!(written, fixture);
    let hour: HealthHour = serde_json::from_str(&self::fixture("health-hour.json")).unwrap();
    assert_eq!(
        (hour.schema, hour.day().as_str()),
        (HEALTH_SCHEMA, "2027-01-15")
    );
    let older: HealthHour = serde_json::from_str(
        r#"{"schema":1,"at":1.0,"pid":1,"generation":1.0,"start":0.0,"from":0.0,"to":3600.0,"partial":false}"#,
    )
    .unwrap();
    assert_eq!(
        (older.dispatch, older.requests.slowest, older.writes.p50_ms),
        (HourPasses::default(), None, None),
        "a line written before a field was added reads with it empty"
    );
}

/// An agents' hour line as the shared server appends it to `symphony/agents/`, read by its type:
/// what the Reports page sums. The file's line carries its `type` first; the line's own fields
/// read and write back as they are, and one written before a field was added reads with it empty.
#[test]
fn an_agents_hour_line_reads_and_writes_back_as_the_same_json_after_its_type() {
    use symphony_api::agent_activity::Level;
    use symphony_api::{AGENT_ACTIVITY_SCHEMA, AgentHour, AgentLine};
    let (written, fixture) = round_trip::<AgentHour>("agent-hour.json");
    assert_eq!(written, fixture);
    let hour: AgentHour = serde_json::from_str(&self::fixture("agent-hour.json")).unwrap();
    assert_eq!(
        (hour.schema, run_day_of(&AgentLine::Hour(hour.clone()))),
        (AGENT_ACTIVITY_SCHEMA, "2027-01-15".to_string())
    );
    let line = serde_json::to_string(&AgentLine::Hour(hour.clone())).unwrap();
    assert!(line.starts_with(r#"{"type":"hour","schema":1,"#), "{line}");
    let read: AgentHour = serde_json::from_str(&line).unwrap();
    assert_eq!(read, hour, "the type is passed over when the line is read");
    let older: AgentHour = serde_json::from_str(
        r#"{"schema":1,"at":1.0,"pid":1,"generation":1.0,"start":0.0,"from":0.0,"to":3600.0,"observed_s":0.0,"samples":0}"#,
    )
    .unwrap();
    assert_eq!(
        (older.running_agents, older.live.len(), older.load_1m),
        (Level::default(), 0, None)
    );
}

fn run_day_of(line: &symphony_api::AgentLine) -> String {
    line.day()
}

/// The Reports page's one read: periods oldest first, each source's part none where no file held
/// an hour of it (`null`, never a zero), a figure per status by the statuses that have one.
#[test]
fn the_reports_page_reads_and_writes_back_as_the_same_json_its_gaps_none() {
    use symphony_api::report_page::{REPORTS_HOUR_MAX_DAYS, REPORTS_MAX_DAYS};
    use symphony_api::{Grain, ReportsPage};
    let (written, fixture) = round_trip::<ReportsPage>("reports-page.json");
    assert_eq!(written, fixture);
    let page: ReportsPage = serde_json::from_str(&self::fixture("reports-page.json")).unwrap();
    assert_eq!((page.grain, page.days), (Grain::Day, 2));
    assert_eq!(page.buckets[0].end, page.buckets[1].start);
    assert_eq!(page.buckets.last().map(|b| b.end), Some(page.to));
    let gap = &page.buckets[1];
    assert!(gap.flow.is_none() && gap.git.is_none() && gap.agents.is_some());
    assert_eq!(page.buckets[0].flow.unwrap().held.review, 1);
    // A period's tasks done, each once, beside its entries into done, which count a second approve.
    let flow = page.buckets[0].flow.unwrap();
    assert_eq!((flow.entered.done, flow.done), (6, Some(5)));
    // Each mark names what its commit changed; one from a server that named none reads with none.
    assert_eq!(page.marks[0].changed, ["settings", "skill work"]);
    // A period's hours by phase and by state; one with no hour that records them has none.
    let placed = page.buckets[0].workflow.as_ref().unwrap();
    assert_eq!((placed.phases.len(), placed.states.len()), (3, 3));
    assert_eq!(page.buckets[1].workflow, None);
    // A period's tokens by role, an unpriced model's in no dollar; a period no roll-up told has
    // none (unknown, never none spent), and the window's landed tasks are priced at one date.
    let spend = page.buckets[0].spend.as_ref().unwrap();
    assert_eq!(
        spend.iter().map(|r| r.role.as_str()).collect::<Vec<_>>(),
        ["executor", "reviewer"]
    );
    assert_eq!(spend[1].cost.unpriced[0].model, "gpt-x");
    assert_eq!(page.buckets[1].spend, None);
    let landed = page.landed.as_ref().unwrap();
    assert_eq!((landed.tasks, landed.without, landed.again), (1, 1, 1));
    assert_eq!(landed.median, Some(1.46));
    assert_eq!(landed.dearest[0].slug, "sim/jump");
    assert_eq!(page.prices, "2026-11-16");
    let old: symphony_api::report_page::ReportMark =
        serde_json::from_str(r#"{"at":1.0,"commit":"c","subject":"s"}"#).unwrap();
    assert_eq!(old.changed, Vec::<String>::new());
    // An empty page, as a server with no file yet answers, reads with every list empty.
    let empty: ReportsPage =
        serde_json::from_str(r#"{"grain":"hour","days":1,"from":0.0,"to":3600.0}"#).unwrap();
    assert_eq!(
        (empty.buckets.len(), empty.first, empty.marks.len()),
        (0, None, 0)
    );
    assert_eq!((empty.landed, empty.prices.as_str()), (None, ""));
    for (word, grain) in [
        ("hour", Grain::Hour),
        ("day", Grain::Day),
        ("week", Grain::Week),
    ] {
        assert_eq!(Grain::parse(word), Some(grain));
        assert_eq!(grain.word(), word);
        assert_eq!(
            serde_json::to_string(&grain).unwrap(),
            format!("\"{word}\"")
        );
    }
    assert_eq!(Grain::parse("month"), None);
    assert_eq!(Grain::Hour.days(Some(30)), REPORTS_HOUR_MAX_DAYS);
    assert_eq!(Grain::Week.days(Some(365)), REPORTS_MAX_DAYS);
    assert_eq!(Grain::Day.days(Some(0)), 1);
    assert_eq!(Grain::Day.days(None), 14);
}

#[test]
fn a_route_is_a_document_page_under_an_entrypoint_unless_the_symphony_answers_it() {
    let project: symphony_api::ProjectView =
        serde_json::from_str(&fixture("project.json")).unwrap();
    for (path, page) in [
        ("docs", true),
        ("docs/", true),
        ("docs/game", true),
        ("docs/game/SPEC.md", true),
        ("docs/pictures/a.png", false),
        ("docs/v1.0", false),
        ("docs/v1.0/", true),
        ("docs/pictures/a.png/", true),
        ("docsx/A.md", false),
        ("", false),
        ("tasks", false),
        ("tasks/", false),
        ("lamps", false),
        ("README.md", false),
    ] {
        assert_eq!(project.is_document_route(path), page, "{path}");
    }
    let dotted = symphony_api::ProjectView {
        entrypoints: vec!["v1.0".into()],
        ..project.clone()
    };
    for (path, page) in [
        ("v1.0", true),
        ("v1.0/", true),
        ("v1.0/A.md", true),
        ("v1.0/a.png", false),
        ("v1.0x", false),
    ] {
        assert_eq!(dotted.is_document_route(path), page, "{path}");
    }
    // a page keeps its kind: a folder whatever its name, a document only as Markdown with no slash
    let markdown_named = symphony_api::ProjectView {
        entrypoints: vec!["guide.md".into(), "docs".into()],
        ..project.clone()
    };
    for (path, folder) in [
        ("guide.md", true),
        ("guide.md/", true),
        ("guide.md/A.md", false),
        ("guide.md/old.md/", true),
        ("docs", true),
        ("docs/game", true),
        ("docs/game/SPEC.md", false),
        ("docs/a.png", false),
        ("tasks/", false),
    ] {
        assert_eq!(markdown_named.is_folder_route(path), folder, "{path}");
    }
    assert_eq!(project.documents_href(), "/docs/");
    assert_eq!(project.readme_route(), "docs/README.md");
    let spec = symphony_api::ProjectView {
        entrypoints: vec![String::new(), "notes".into()],
        ..project
    };
    for (path, page) in [
        ("", true),
        ("index.md", true),
        ("guide", true),
        ("guide/START.md", true),
        ("notes/A.md", true),
        ("artifacts/A.md", true),
        ("pictures/a.png", false),
        ("tasks", false),
        ("task/a/b", false),
        ("agents", false),
        ("queue", false),
        ("api/docs", false),
        ("artifacts/uploads/a.md", false),
        ("favicon.ico", false),
        ("lamps", false),
        ("lampsx", true),
    ] {
        assert_eq!(spec.is_document_route(path), page, "{path}");
    }
    assert_eq!(spec.documents_href(), "/");
    assert_eq!(spec.readme_route(), "README.md");
}

/// An HTML page under an entrypoint is a document route, drawn in its frame, whatever the case of
/// its extension; with a slash it names a folder, as any name does, and outside every entrypoint, or
/// on a route the Symphony answers, it is a file served as itself.
#[test]
fn an_html_page_under_an_entrypoint_is_a_document_drawn_in_its_frame() {
    let project: symphony_api::ProjectView =
        serde_json::from_str(&fixture("project.json")).unwrap();
    for (path, document, folder, html) in [
        ("docs/analysis/readout.html", true, false, true),
        ("docs/A.HTM", true, false, true),
        ("docs/a.htm", true, false, true),
        ("docs/game/SPEC.md", true, false, false),
        ("docs/analysis/readout.html/", true, true, false),
        ("docs/a.html.md", true, false, false),
        ("docs/a.xhtml", false, false, false),
        ("docs/a.html5", false, false, false),
        ("docs/html", true, true, false),
        ("readout.html", false, false, false),
        ("artifacts/readout.html", false, false, false),
        ("tasks/a.html", false, false, false),
    ] {
        assert_eq!(
            (
                project.is_document_route(path),
                project.is_folder_route(path),
                project.is_html_route(path)
            ),
            (document, folder, html),
            "{path}"
        );
    }
    for (name, html) in [
        ("a.html", true),
        ("A.HTML", true),
        ("a.Htm", true),
        ("docs/a.htm", true),
        ("a.md", false),
        ("html", false),
        (".html", false),
    ] {
        assert_eq!(symphony_api::is_html(name), html, "{name}");
    }
}

/// The frame of an HTML document asks for its file by a query on its route, the route's own folder
/// kept for the files beside it: added before a fragment, after a query of the author's, once.
#[test]
fn an_html_documents_frame_asks_for_its_file_by_a_query_on_its_route() {
    for (src, file) in [
        ("/docs/a/readout.html", "/docs/a/readout.html?raw"),
        ("/alpha/@wt/docs/a.htm", "/alpha/@wt/docs/a.htm?raw"),
        ("/docs/a.html#part", "/docs/a.html?raw#part"),
        ("/docs/a.html?state=b", "/docs/a.html?state=b&raw"),
        ("/docs/a.html?state=b#x", "/docs/a.html?state=b&raw#x"),
        ("/docs/a.html?raw", "/docs/a.html?raw"),
        ("/docs/a.html?x=1&raw=1#y", "/docs/a.html?x=1&raw=1#y"),
    ] {
        assert_eq!(symphony_api::file_address(src), file, "{src}");
        let query = file.split(['?', '#']).nth(1).unwrap();
        assert!(symphony_api::asks_for_file(query), "{file}");
    }
    for query in ["", "rawness", "x=raw", "raws=1", "state=b&r=aw"] {
        assert!(!symphony_api::asks_for_file(query), "{query}");
    }
    assert!(symphony_api::asks_for_file("raw="));
    assert_eq!(symphony_api::FILE_QUERY, "raw");
}

/// A project's day file (`symphony/reports/YYYY-MM-DD.json` in its git common dir), as the hourly
/// roll-up writes it and the Reports page reads it: every figure of the hours and the day reads and
/// writes back, each hour is named by its first second, and the hours sum to a span's figure.
#[test]
fn a_day_file_reads_and_writes_back_as_the_same_json_and_its_hours_sum_to_the_span() {
    use symphony_api::{REPORT_SCHEMA, ReportDay, ReportHour, report_file};
    let (written, fixture) = round_trip::<ReportDay>("report-day.json");
    assert_eq!(written, fixture);
    let day: ReportDay = serde_json::from_str(&self::fixture("report-day.json")).unwrap();
    assert_eq!(
        (day.schema, report_file(&day.day)),
        (REPORT_SCHEMA, "2027-01-15.json".into())
    );
    assert_eq!(symphony_api::run_day(day.start), day.day);
    assert_eq!(
        day.hours.iter().map(|h| h.start).collect::<Vec<_>>(),
        [day.start, day.start + 3_600.0],
        "each hour by its first second, the key the other day files join on"
    );
    let sum = |pick: fn(&ReportHour) -> u64| -> u64 { day.hours.iter().map(pick).sum() };
    assert_eq!(sum(|h| h.git.unwrap().added), 212);
    assert_eq!(sum(|h| u64::from(h.decisions.unwrap().auto_approved)), 1);
    assert_eq!(sum(|h| u64::from(h.decisions.unwrap().withdrawn)), 1);
    assert_eq!(sum(|h| u64::from(h.landed)), 1);
    assert_eq!(
        sum(|h| u64::from(h.top.done.unwrap() + h.subtasks.done.unwrap())),
        1
    );
    // A day file written before its hours counted the tasks done reads with none in each, never a
    // zero, and writes that back as `null`.
    let mut older: Value = serde_json::from_str(&self::fixture("report-day.json")).unwrap();
    for hour in older["hours"].as_array_mut().unwrap() {
        for part in ["top", "subtasks"] {
            hour[part].as_object_mut().unwrap().remove("done");
        }
    }
    let older: ReportDay = serde_json::from_value(older).unwrap();
    assert!(
        (older.hours.iter()).all(|h| h.top.done.is_none() && h.subtasks.done.is_none()),
        "no hour of an older file counted them"
    );
    let written = serde_json::to_value(&older).unwrap();
    assert_eq!(written["hours"][0]["top"]["done"], Value::Null);
    // A red names the commits it came between and the tasks that landed in them.
    let release = &day.reds[1];
    assert_eq!(
        (release.between.as_deref(), release.landed.as_deref()),
        (
            Some(
                "1a5037fc9e2b4d6a8c0e1f3a5b7c9d0e2f4a6b8c..fd70821942b4c97d1e17b420217ada28988e27da"
            ),
            Some(
                &[
                    "tests/release-red-verify-lint".to_string(),
                    "app/board-filter".into()
                ][..]
            )
        )
    );
    let decisions = day.decisions.unwrap();
    assert_eq!(
        decisions.open[0].since, None,
        "a card with no dated line has no wait to tell"
    );
    assert_eq!(
        (decisions.auto_approved.len(), decisions.withdrawn.len()),
        (1, 1)
    );
    assert_eq!(day.subtasks.lead, None);
    // A custom workflow's hours are an hour's own, by phase and by state, a label renamed since an
    // entry of its own; an hour of a board that runs none has none, and writes no field for it.
    let placed = day.hours[1].workflow.as_ref().unwrap();
    let total =
        |spent: &[symphony_api::reports::Spent]| -> f64 { spent.iter().map(|s| s.hours).sum() };
    assert_eq!(
        (total(&placed.top.phases), total(&placed.top.states)),
        (2.0, 2.0)
    );
    assert_eq!(
        (placed.top.phases.iter())
            .map(|s| (s.id.as_str(), s.label.as_str()))
            .collect::<Vec<_>>(),
        [("build", "Build"), ("build", "Make"), ("verify", "Verify")]
    );
    assert_eq!(day.hours[0].workflow, None);
    let first = serde_json::to_value(&day.hours[0]).unwrap();
    assert!(first.get("workflow").is_none(), "{first}");
}

/// A figure per status is written as a map of the statuses whose figure is not zero, by name, as a
/// map of names would be: none written for an hour where nothing moved, a name that is no status
/// left out when read, and no figure for it.
#[test]
fn a_figure_per_status_writes_the_statuses_that_have_one_by_name() {
    use symphony_api::reports::ByStatus;
    let mut held = ByStatus::<u32>::default();
    assert_eq!(serde_json::to_string(&held).unwrap(), "{}");
    for status in ["review", "in_progress", "backlog", "review"] {
        *held.get_mut(status).unwrap() += 1;
    }
    assert_eq!(held.get_mut("open"), None, "no status of the board's");
    assert_eq!(
        serde_json::to_string(&held).unwrap(),
        r#"{"backlog":1,"in_progress":1,"review":2}"#
    );
    let read: ByStatus<u32> =
        serde_json::from_str(r#"{"review":2,"open":5,"backlog":1,"in_progress":1}"#).unwrap();
    assert_eq!(read, held);
    let hours = ByStatus {
        ready: 0.5,
        cancelled: 0.0,
        ..ByStatus::default()
    };
    assert_eq!(serde_json::to_string(&hours).unwrap(), r#"{"ready":0.5}"#);
    let names: Vec<&str> = hours.each().iter().map(|(name, _)| *name).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted, "written in the order of their names");
    let mut each = hours;
    for name in names {
        assert!(each.get_mut(name).is_some(), "{name}");
    }
}

/// A day file written before a figure was added reads with that figure empty: none for the
/// decisions and git, no hours, no auto-approval or withdrawal in an hour's decisions.
#[test]
fn a_day_file_without_its_later_figures_reads_with_them_empty() {
    use symphony_api::reports::ByStatus;
    use symphony_api::{ReportDay, ReportHour};
    let day: ReportDay = serde_json::from_str(
        r#"{"schema":1,"project":"p","day":"2027-01-15","start":1799971200.0,
            "read":1800057600.0,"until":1800057600.0,"closed":true}"#,
    )
    .unwrap();
    assert!(day.closed && day.hours.is_empty() && day.rounds.is_empty());
    assert_eq!((day.decisions, day.git), (None, None));
    let hour: ReportHour = serde_json::from_str(r#"{"start":1799982000.0}"#).unwrap();
    assert_eq!(
        (hour.landed, hour.decisions, hour.git, hour.workflow),
        (0, None, None, None),
        "a file written before the workflow's hours were counted has none"
    );
    assert!(hour.top.held == ByStatus::default() && hour.waits.approved_to_landed.is_empty());
    let hour: ReportHour =
        serde_json::from_str(r#"{"start":1799982000.0,"decisions":{"open":2,"answered":1}}"#)
            .unwrap();
    let counts = hour.decisions.unwrap();
    assert_eq!(
        (
            counts.open,
            counts.answered,
            counts.auto_approved,
            counts.withdrawn
        ),
        (2, 1, 0, 0)
    );
    let red: symphony_api::reports::Red = serde_json::from_str(
        r#"{"slug":"tests/main-red-1","series":"main","filed":1799982000.0,"status":"ready"}"#,
    )
    .unwrap();
    assert_eq!(
        (red.closed, red.between, red.landed),
        (None, None, None),
        "a red of a file written before reds named their commits names none"
    );
}
