use super::*;

fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_string()).collect()
}

fn visit(run: usize, probe: Value) -> Visit {
    Visit {
        run,
        probe,
        ..Visit::default()
    }
}

#[test]
fn a_base_alone_measures_one_page_of_each_kind_three_times_each() {
    let asked = read(&args(&["http://127.0.0.1:4100/symphony/"]))
        .unwrap()
        .unwrap();
    assert_eq!(
        asked,
        Asked {
            base: "http://127.0.0.1:4100/symphony".into(),
            pages: PAGES.iter().map(|page| (*page).to_string()).collect(),
            runs: 3,
            tabs: 0,
            timeout: Duration::from_secs(120),
            note: None,
            against: None,
        }
    );
}

#[test]
fn the_pages_runs_tabs_timeout_note_and_earlier_run_are_read_as_given() {
    let asked = read(&args(&[
        "http://h:1/p",
        "/tasks",
        "/queue",
        "--runs",
        "5",
        "--timeout",
        "30",
        "--tabs",
        "6",
        "--note",
        "before the outline fix",
        "--against",
        "2026-11-15-095412",
    ]))
    .unwrap()
    .unwrap();
    assert_eq!(asked.pages, ["/tasks", "/queue"]);
    assert_eq!(asked.runs, 5);
    assert_eq!(asked.tabs, 6);
    assert_eq!(asked.timeout, Duration::from_secs(30));
    assert_eq!(asked.note.as_deref(), Some("before the outline fix"));
    assert_eq!(asked.against.as_deref(), Some("2026-11-15-095412"));
}

#[test]
fn a_kept_run_is_named_by_its_local_second() {
    let said = stamp(1_763_196_852);
    assert_eq!(said.len(), "2026-11-15 09:54:12".len(), "{said}");
    assert!(said.ends_with(":12"), "{said}");
    assert!(said.starts_with("2025-11-1"), "{said}");
}

#[test]
fn the_checkout_is_named_by_its_commit_and_none_outside_one() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let commit = checkout(here).expect("this crate sits in a checkout");
    assert_eq!(commit.len(), 40, "{commit}");
    assert!(commit.chars().all(|c| c.is_ascii_hexdigit()), "{commit}");
    assert_eq!(checkout(Path::new("/")), None);
}

#[test]
fn a_command_line_it_cannot_run_is_refused_saying_why() {
    for (words, why) in [
        (&[][..], "the following arguments are required: BASE"),
        (&["127.0.0.1:8765"][..], "is not an http address"),
        (&["http://h:1", "tasks"][..], "does not start with /"),
        (
            &["http://h:1", "--runs", "0"][..],
            "is not a whole number of 1 or more",
        ),
        (
            &["http://h:1", "--tabs", "x"][..],
            "is not a whole number of 0 or more",
        ),
        (
            &["http://h:1", "--timeout", "soon"][..],
            "is not a whole number of 1 or more",
        ),
    ] {
        let refused = read(&args(words)).unwrap_err();
        assert!(refused.contains(why), "{words:?}: {refused}");
    }
    assert_eq!(read(&args(&["--help"])).unwrap(), None);
}

#[test]
fn the_median_return_is_by_synced_with_an_unsynced_one_the_slowest() {
    let visits = [
        visit(0, json!({"synced": 10.0})),
        visit(1, json!({"synced": 900.0})),
        visit(2, json!({"synced": null})),
        visit(3, json!({"synced": 300.0})),
    ];
    assert_eq!(median(&visits).map(|visit| visit.run), Some(1));
    assert_eq!(median(&visits[..2]).map(|visit| visit.run), Some(1));
    assert!(
        median(&visits[..1]).is_none(),
        "the first visit is no return"
    );
    let unsynced = [visit(1, json!({})), visit(2, json!({"synced": 5.0}))];
    assert_eq!(median(&unsynced).map(|visit| visit.run), Some(2));
}

#[test]
fn an_address_is_named_by_its_path_and_a_long_one_cut_short() {
    assert_eq!(
        short("http://127.0.0.1:1/symphony/api/board?x=1"),
        "/symphony/api/board?x=1"
    );
    assert_eq!(short("/tasks"), "/tasks");
    let long = format!("http://h/{}", "a".repeat(100));
    assert_eq!(short(&long).chars().count(), 72);
    assert!(short(&long).ends_with('…'));
}

#[test]
fn the_waterfall_lists_what_began_before_synced_in_order_and_leaves_cached_files_out() {
    let probe = json!({
        "drawn": 120.0,
        "checked": 890.0,
        "synced": 900.0,
        "document": {"sent": 2.0, "first": 30.0, "end": 31.0, "loaded": 60.0},
        "requests": [
            {"url": "http://h/symphony/api/board", "start": 130.0, "sent": 131.0, "first": 400.0,
             "end": 410.0, "bytes": 1500.0, "status": 200},
            {"url": "http://h/assets/kind-0123456789abcdef.js", "start": 40.0, "sent": 40.0,
             "first": 41.0, "end": 41.0, "bytes": 0.0, "status": 200},
            {"url": "http://h/symphony/api/late", "start": 950.0, "sent": 950.0, "first": 960.0,
             "end": 961.0, "bytes": 10.0, "status": 200}
        ],
        "reads": [{"part": "board: board", "start": 70.0, "end": 110.0, "hit": true}],
        "feeds": [{"kind": "socket", "url": "ws://h/symphony/api/feed?since=3", "made": 420.0,
                   "open": 880.0, "first": null}],
        "longtasks": [[100.0, 75.0], [1000.0, 60.0]],
        "paint": {"first-contentful-paint": 90.0},
    });
    let said: Vec<String> = rows(&visit(1, probe))
        .into_iter()
        .map(|row| format!("{:.0} {}", row.at, row.said))
        .collect();
    assert_eq!(
        said,
        [
            "0 the page          sent 2  first byte 30  end 31  parsed 60",
            "70 device read board: board  end 110  kept",
            "90 first contentful paint",
            "100 long task 75 ms",
            "120 DRAWN from the device",
            "130 /symphony/api/board  queued 1  server 269  body 10  end 410  200  1.5 KB",
            "420 feed socket /symphony/api/feed?since=3  open 880  first message -",
            "890 CHECKED: nothing from the device unanswered, the feed open",
            "900 SYNCED: every read answered too",
        ]
    );
}

#[test]
fn a_visit_never_synced_lists_everything_it_began() {
    let probe = json!({
        "requests": [{"url": "http://h/symphony/api/board", "start": 5000.0, "sent": 5000.0,
                      "first": 9000.0, "end": 9001.0, "bytes": 10.0, "status": 504}]
    });
    let rows = rows(&visit(1, probe));
    assert_eq!(rows.len(), 1);
    assert!(rows[0].said.contains("server 4000"), "{}", rows[0].said);
    assert!(rows[0].said.contains("504"), "{}", rows[0].said);
}

#[test]
fn the_table_has_a_row_a_page_with_each_returns_drawn_and_synced() {
    let measured = vec![
        (
            "/tasks".to_string(),
            vec![
                visit(0, json!({"synced": 2000.4})),
                visit(
                    1,
                    json!({"drawn": 100.0, "checked": 1400.0, "synced": 1500.0}),
                ),
                visit(2, json!({"drawn": 90.0})),
            ],
        ),
        ("/reports".to_string(), vec![visit(0, json!({}))]),
    ];
    let said = table("http://h/p", &measured, Some([80.4, 100.0, 170.6]));
    let lines: Vec<&str> = said.lines().collect();
    assert!(
        lines[1].contains("http://h/p") && lines[1].contains("load 80 100 171"),
        "{said}"
    );
    assert!(lines[3].starts_with("/tasks"), "{said}");
    assert!(lines[3].ends_with("2000  100/1400/1500  90/-/-"), "{said}");
    assert!(
        lines[4].starts_with("/reports") && lines[4].ends_with("-  "),
        "{said:?}"
    );
}

/// A visit synced at 40 ms whose board read answered 503 and whose task read had no answer.
fn failing() -> Visit {
    visit(
        1,
        json!({"drawn": 10.0, "checked": 30.0, "synced": 40.0, "fetches": [
            {"url": "http://h/p/api/board", "status": 503, "own": true},
            {"url": "http://h/p/api/task/a", "status": "failed", "own": true},
        ]}),
    )
}

#[test]
fn a_read_of_the_pages_own_fails_on_any_status_but_2xx_or_304_or_on_no_answer() {
    let said = visit(
        1,
        json!({"fetches": [
            {"url": "http://h/p/api/board", "status": 200, "own": true},
            {"url": "http://h/p/api/runs", "status": 204, "own": true},
            {"url": "http://h/p/api/docs", "status": 304, "own": true},
            {"url": "http://h/p/api/out", "status": null, "own": true},
            {"url": "http://h/p/api/ahead", "status": 404, "own": false},
            {"url": "http://h/p/api/task/a", "status": 404, "own": true},
            {"url": "http://h/p/api/gone", "status": 302, "own": true},
            {"url": "http://h/p/api/down", "status": "failed", "own": true},
        ]}),
    )
    .failed();
    assert_eq!(
        said,
        [
            "/p/api/task/a 404",
            "/p/api/gone 302",
            "/p/api/down no answer"
        ]
    );
    assert!(visit(1, json!({})).failed().is_empty());
}

#[test]
fn a_visit_whose_own_read_failed_names_it_on_its_line() {
    let said = waterfall("/tasks", &[failing()]);
    let line = said
        .lines()
        .find(|line| line.contains("return 1 "))
        .unwrap();
    assert!(
        line.ends_with("  READS FAILED: /p/api/board 503, /p/api/task/a no answer"),
        "{said}"
    );
    assert!(!said.contains("LEFT SYNCED"), "{said}");
}

#[test]
fn a_visit_that_left_synced_says_when() {
    let said = waterfall(
        "/queue",
        &[visit(
            1,
            json!({"synced": 32.0, "unsynced": 851.6, "fetches": []}),
        )],
    );
    assert!(said.contains("synced     32"), "{said}");
    assert!(said.contains("  LEFT SYNCED at 852"), "{said}");
    assert!(!said.contains("READS FAILED"), "{said}");
}

#[test]
fn the_table_marks_a_visit_whose_own_read_failed_and_says_what_the_mark_is() {
    let mut first = failing();
    first.run = 0;
    let measured = vec![
        (
            "/tasks".to_string(),
            vec![
                first,
                failing(),
                visit(2, json!({"drawn": 9.0, "synced": 50.0})),
            ],
        ),
        (
            "/queue".to_string(),
            vec![visit(0, json!({"synced": 30.0}))],
        ),
    ];
    let said = table("http://h/p", &measured, None);
    let lines: Vec<&str> = said.lines().collect();
    assert!(lines[3].starts_with("/tasks"), "{said}");
    assert!(lines[3].ends_with("40! 10/30/40!  9/-/50"), "{said}");
    assert!(lines[4].ends_with("30  "), "{said:?}");
    assert_eq!(
        lines[5],
        "! a read of the page's own failed: its visit's line above names it"
    );
    let clean = table("http://h/p", &measured[1..], None);
    assert!(!clean.contains('!'), "{clean}");
}

#[test]
fn each_visit_says_its_drawn_synced_spending_and_why_it_ended_unsynced() {
    let mut late = visit(
        1,
        json!({"drawn": 50.0, "last": "task device stale", "commitsSynced": null}),
    );
    late.error = Some("not synced within 120 s".into());
    late.spent = vec![("TaskDuration".into(), 812.4), ("ThreadTime".into(), 640.0)];
    let said = waterfall(
        "/task/a",
        &[visit(0, json!({"synced": 700.0, "commitsSynced": 4})), late],
    );
    assert!(
        said.contains("first     drawn      -  checked      -  synced    700"),
        "{said}"
    );
    assert!(said.contains("commits 4"), "{said}");
    assert!(said.contains("tasks 812 ms  cpu 640 ms"), "{said}");
    assert!(
        said.contains("(not synced within 120 s; last state \"task device stale\")"),
        "{said}"
    );
    assert!(said.contains("return 1, the median:"), "{said}");
}
