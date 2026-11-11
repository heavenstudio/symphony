//! The command line, keys, readiness and report of `symphony shot`, without a browser. The browser
//! cases (a real page, its pictures, its clean failures) are in apps/symphony-ui/tests/browser/shot.rs.

use super::*;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn shot(values: &[&str]) -> Shot {
    read(&args(values)).unwrap().unwrap()
}

fn refusal(values: &[&str]) -> String {
    read(&args(values)).unwrap_err()
}

#[test]
fn a_bare_shot_takes_the_desktop_viewport_and_twenty_seconds() {
    assert_eq!(
        shot(&["http://127.0.0.1:1/tasks", "--out", "a.png"]),
        Shot {
            url: "http://127.0.0.1:1/tasks".into(),
            out: "a.png".into(),
            timeout: Duration::from_secs(20),
            view: View::Size(1440, 900),
            full_page: false,
            scheme: None,
            report: false,
            steps: Vec::new(),
        }
    );
}

#[test]
fn steps_keep_their_command_line_order_across_kinds_and_repeat() {
    let shot = shot(&[
        "--click",
        "button[aria-label=\"Status\"]",
        "http://h/x",
        "--shot",
        "open.png",
        "--key=Escape",
        "--out",
        "closed.png",
        "--wait-for",
        ".kanban",
        "--type",
        "input[name=q]=a=b",
        "--hover",
        "h1",
        "--eval",
        "document.title",
        "--pause",
        "250",
        "--click",
        "h1",
        "--shot",
        "again.png",
    ]);
    assert_eq!(shot.out, PathBuf::from("closed.png"));
    assert_eq!(
        shot.steps,
        [
            Step::Click("button[aria-label=\"Status\"]".into()),
            Step::Shot("open.png".into()),
            Step::Key(key("Escape").unwrap()),
            Step::WaitFor(".kanban".into()),
            Step::Type("input[name=q]".into(), "a=b".into()),
            Step::Hover("h1".into()),
            Step::Eval("document.title".into()),
            Step::Pause(Duration::from_millis(250)),
            Step::Click("h1".into()),
            Step::Shot("again.png".into()),
        ]
    );
}

#[test]
fn the_view_options_read_as_given() {
    let shot = shot(&[
        "about:blank",
        "--out=x.png",
        "--size",
        "800x600",
        "--full-page",
        "--scheme",
        "dark",
        "--report",
        "--timeout",
        "2.5",
    ]);
    assert_eq!(shot.view, View::Size(800, 600));
    assert!(shot.full_page && shot.report);
    assert_eq!(shot.scheme, Some(Scheme::Dark));
    assert_eq!(shot.timeout, Duration::from_millis(2500));
    let phone = self::shot(&[
        "data:text/html,x",
        "--out",
        "p.png",
        "--phone",
        "--scheme",
        "light",
    ]);
    assert_eq!(phone.view, View::Phone);
    assert_eq!(phone.scheme, Some(Scheme::Light));
}

#[test]
fn help_is_no_shot() {
    assert_eq!(read(&args(&["--help"])).unwrap(), None);
    assert_eq!(read(&args(&["http://h/", "-h"])).unwrap(), None);
}

#[test]
fn a_command_line_it_cannot_run_is_refused_naming_the_argument() {
    for (line, said) in [
        (
            &["--out", "a.png"][..],
            "the following arguments are required: URL",
        ),
        (
            &["http://h/"],
            "the following arguments are required: --out",
        ),
        (
            &["http://h/", "http://i/", "--out", "a"],
            "unrecognized arguments: http://i/",
        ),
        (
            &["127.0.0.1:8765/tasks", "--out", "a"],
            "is not an absolute URL",
        ),
        (
            &["http://h/", "--out", "folder/"],
            "argument --out: \"folder/\" names no file",
        ),
        (
            &["http://h/", "--out", "a", "--shot", ""],
            "argument --shot: \"\" names no file",
        ),
        (
            &["http://h/", "--out", "a", "--timeout", "0"],
            "argument --timeout: invalid SECS \"0\"",
        ),
        (
            &["http://h/", "--out", "a", "--timeout", "soon"],
            "argument --timeout: invalid SECS",
        ),
        (
            &["http://h/", "--out", "a", "--timeout", "inf"],
            "argument --timeout: invalid SECS",
        ),
        (
            &["http://h/", "--out", "a", "--size", "1440"],
            "argument --size: invalid WxH \"1440\"",
        ),
        (
            &["http://h/", "--out", "a", "--size", "0x900"],
            "argument --size: invalid WxH",
        ),
        (
            &["http://h/", "--out", "a", "--size", "1440x900x2"],
            "argument --size: invalid WxH",
        ),
        (
            &["http://h/", "--out", "a", "--size", "800x600", "--phone"],
            "argument --phone: not allowed with argument --size",
        ),
        (
            &["http://h/", "--out", "a", "--scheme", "sepia"],
            "argument --scheme: invalid choice: \"sepia\"",
        ),
        (
            &["http://h/", "--out", "a", "--pause", "1.5"],
            "argument --pause: invalid MS \"1.5\"",
        ),
        (
            &["http://h/", "--out", "a", "--key", "Hyper+a"],
            "argument --key: \"Hyper\" in \"Hyper+a\" is no modifier",
        ),
        (
            &["http://h/", "--out", "a", "--key", "F13"],
            "argument --key: \"F13\" is no key",
        ),
        (
            &["http://h/", "--out", "a", "--type", "no-equals"],
            "argument --type: \"no-equals\" is not CSS=TEXT",
        ),
        (
            &["http://h/", "--out", "a", "--type", "=text"],
            "argument --type: \"=text\" is not CSS=TEXT",
        ),
        (
            &["http://h/", "--out", "a", "--click", " "],
            "argument --click: an empty selector",
        ),
        (
            &["http://h/", "--out", "a", "--wait-for"],
            "argument --wait-for: expected one argument",
        ),
        (
            &["http://h/", "--out", "a", "--zoom", "2"],
            "unrecognized arguments: --zoom",
        ),
    ] {
        let refused = refusal(line);
        assert!(refused.contains(said), "{line:?}: {refused}");
    }
}

#[test]
fn a_refusal_prints_the_usage_and_exits_two_and_help_exits_zero() {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(main(&args(&["http://h/"]), &mut out, &mut err), 2);
    let err = String::from_utf8(err).unwrap();
    assert!(err.starts_with("usage: symphony shot"), "{err}");
    assert!(
        err.contains("symphony shot: error: the following arguments are required: --out"),
        "{err}"
    );
    assert!(out.is_empty());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(main(&args(&["--help"]), &mut out, &mut err), 0);
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("--click CSS"), "{out}");
    assert!(err.is_empty());
}

#[test]
fn typing_splits_at_the_first_equals_outside_brackets_parentheses_and_quotes() {
    for (value, split) in [
        ("#q=hello", Some(("#q", "hello"))),
        ("#q=", Some(("#q", ""))),
        ("#q=a=b", Some(("#q", "a=b"))),
        ("input[name=q]=text", Some(("input[name=q]", "text"))),
        (
            "input[name=\"a=]b\"]=x",
            Some(("input[name=\"a=]b\"]", "x")),
        ),
        (
            "li:not([data-x='1=2'])=y",
            Some(("li:not([data-x='1=2'])", "y")),
        ),
        (
            "textarea:is(.a, [b=c]) = spaced",
            Some(("textarea:is(.a, [b=c]) ", " spaced")),
        ),
        ("input[name=q]", None),
        ("plain", None),
        ("=text", None),
        (" =text", None),
    ] {
        assert_eq!(split_type(value), split, "{value}");
    }
}

#[test]
fn keys_carry_their_codes_text_and_held_modifiers() {
    let tab = key("Tab").unwrap();
    assert_eq!(
        tab,
        Key {
            key: "Tab".into(),
            code: "Tab".into(),
            key_code: 9,
            text: None,
            modifiers: 0
        }
    );
    let enter = key("Enter").unwrap();
    assert_eq!((enter.key_code, enter.text.as_deref()), (13, Some("\r")));
    let back = key("Shift+Tab").unwrap();
    assert_eq!((back.key.as_str(), back.modifiers), ("Tab", SHIFT));
    let capital = key("Shift+a").unwrap();
    assert_eq!(
        (
            capital.key.as_str(),
            capital.code.as_str(),
            capital.key_code,
            capital.text.as_deref()
        ),
        ("A", "KeyA", 65, Some("A"))
    );
    let select_all = key("Control+a").unwrap();
    assert_eq!((select_all.text, select_all.modifiers), (None, CONTROL));
    let shortcut = key("Meta+Shift+k").unwrap();
    assert_eq!((shortcut.modifiers, shortcut.text), (META | SHIFT, None));
    let digit = key("7").unwrap();
    assert_eq!((digit.code.as_str(), digit.key_code), ("Digit7", 55));
    let plus = key("+").unwrap();
    assert_eq!(
        (plus.key.as_str(), plus.code.as_str(), plus.text.as_deref()),
        ("+", "", Some("+"))
    );
    let shifted_plus = key("Shift++").unwrap();
    assert_eq!(
        (shifted_plus.key.as_str(), shifted_plus.modifiers),
        ("+", SHIFT)
    );
    let space = key("Space").unwrap();
    assert_eq!(
        (
            space.key.as_str(),
            space.code.as_str(),
            space.text.as_deref()
        ),
        (" ", "Space", Some(" "))
    );
    assert_eq!(key("Esc").unwrap().key, "Escape");
    assert_eq!(key("Alt+ArrowDown").unwrap().modifiers, ALT);
    assert!(key("").is_err());
    assert!(key("ab").is_err());
}

fn request(url: &str, kind: &str) -> Request {
    Request {
        url: url.into(),
        kind: kind.into(),
        ..Request::default()
    }
}

#[test]
fn a_page_is_drawn_once_loaded_navigated_with_no_region_loading_and_only_streams_open() {
    let drawn = json!({"ready": "complete", "navigation": "ready", "loading": 0});
    let stream = request("http://h/api/feed", "EventSource");
    let done = Request {
        finished: true,
        ..request("http://h/api/task-board", "Fetch")
    };
    let failed = Request {
        failed: Some("net::ERR_ABORTED".into()),
        canceled: true,
        ..request("http://h/api/old", "Fetch")
    };
    assert!(waiting(&drawn, &[stream.clone(), done.clone(), failed.clone()]).is_empty());
    // A page that is not the Symphony's has no navigation marker of its own.
    assert!(
        waiting(
            &json!({"ready": "complete", "navigation": null, "loading": 0}),
            &[]
        )
        .is_empty()
    );
    let pending: Vec<Request> = (0..5)
        .map(|n| request(&format!("http://h/{n}"), "Fetch"))
        .collect();
    let said = waiting(
        &json!({"ready": "interactive", "navigation": "loading", "loading": 2}),
        &[vec![stream, done, failed], pending].concat(),
    );
    assert_eq!(
        said,
        [
            "the document to load (it is interactive)",
            "the page's navigation (html[data-navigation=loading])",
            "2 region(s) still loading ([data-ready=loading])",
            "5 request(s) in flight: http://h/0, http://h/1, http://h/2 and 2 more",
        ]
    );
}

#[test]
fn traffic_is_quiet_since_its_last_request_began_finished_or_failed_but_not_before_the_wait() {
    let start = Instant::now();
    let at = |millis| start + Duration::from_millis(millis);
    let event = |method: &str| json!({"method": method, "params": {"requestId": "1"}});
    let (sent, data, finished, failed, page) = (
        event("Network.requestWillBeSent"),
        event("Network.dataReceived"),
        event("Network.loadingFinished"),
        event("Network.loadingFailed"),
        event("Page.loadEventFired"),
    );
    // Nothing yet, or only what came before the wait: the wait's own start.
    assert_eq!(quiet_since(std::iter::empty(), at(40)), at(40));
    assert_eq!(
        quiet_since([(at(10), &sent), (at(20), &finished)].into_iter(), at(40)),
        at(40)
    );
    // The last request to begin, finish or fail, however the events are ordered.
    for (events, last) in [
        (vec![(at(50), &sent)], 50),
        (vec![(at(50), &sent), (at(90), &finished)], 90),
        (vec![(at(70), &failed), (at(60), &sent)], 70),
    ] {
        assert_eq!(quiet_since(events.into_iter(), at(40)), at(last));
    }
    // A stream's data and the page's own events are no traffic.
    assert_eq!(
        quiet_since(
            [(at(50), &sent), (at(80), &data), (at(95), &page)].into_iter(),
            at(40)
        ),
        at(50)
    );
}

#[test]
fn the_report_keeps_errors_failures_and_overflow_and_leaves_out_the_rest() {
    let event = |method: &str, params: Value| json!({"method": method, "params": params});
    let events = [
        event(
            "Runtime.consoleAPICalled",
            json!({"type": "error", "args": [{"type": "string", "value": "boom"}, {"type": "number", "value": 3}, {"type": "object", "description": "Error: deep"}]}),
        ),
        event(
            "Runtime.consoleAPICalled",
            json!({"type": "log", "args": [{"type": "string", "value": "fine"}]}),
        ),
        event(
            "Runtime.exceptionThrown",
            json!({"exceptionDetails": {"text": "Uncaught", "exception": {"description": "TypeError: x is undefined"}}}),
        ),
        event(
            "Runtime.exceptionThrown",
            json!({"exceptionDetails": {"text": "Uncaught (in promise)"}}),
        ),
        event(
            "Log.entryAdded",
            json!({"entry": {"level": "error", "source": "network", "text": "Failed to load resource", "url": "http://h/missing"}}),
        ),
        event(
            "Log.entryAdded",
            json!({"entry": {"level": "warning", "source": "other", "text": "meh"}}),
        ),
        event("Network.requestWillBeSent", json!({"requestId": "1"})),
    ];
    let requests = [
        Request {
            status: 404,
            finished: true,
            ..request("http://h/missing", "Fetch")
        },
        Request {
            status: 200,
            finished: true,
            ..request("http://h/fine", "Fetch")
        },
        Request {
            failed: Some("net::ERR_CONNECTION_REFUSED".into()),
            ..request("http://h/refused", "Fetch")
        },
        Request {
            failed: Some("net::ERR_ABORTED".into()),
            canceled: true,
            ..request("http://h/given-up", "Fetch")
        },
    ];
    let said = report(
        &events,
        &requests,
        &json!({"scroll_width": 1500, "viewport_width": 1440}),
    );
    assert_eq!(
        said,
        json!({
            "console_errors": [
                {"source": "console", "text": "boom 3 Error: deep"},
                {"source": "exception", "text": "TypeError: x is undefined"},
                {"source": "exception", "text": "Uncaught (in promise)"},
                {"source": "network", "text": "Failed to load resource", "url": "http://h/missing"},
            ],
            "failed_requests": [
                {"url": "http://h/missing", "status": 404},
                {"url": "http://h/refused", "error": "net::ERR_CONNECTION_REFUSED"},
            ],
            "scroll_width": 1500,
            "viewport_width": 1440,
            "horizontal_overflow": true,
        })
    );
    let fits = report(
        &[],
        &[],
        &json!({"scroll_width": 390, "viewport_width": 390}),
    );
    assert_eq!(fits["horizontal_overflow"], json!(false));
    assert_eq!(fits["console_errors"], json!([]));
}

#[test]
fn a_machine_without_the_named_chrome_fails_before_any_browser_starts() {
    // SF_CHROME is read by this process alone; the variable is set only in a child.
    if std::env::var_os("SF_SHOT_MISSING_CHROME").is_some() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = main(
            &args(&["http://127.0.0.1:1/", "--out", "never.png"]),
            &mut out,
            &mut err,
        );
        let err = String::from_utf8(err).unwrap();
        assert_eq!(code, 1, "{err}");
        assert!(err.contains("/no/such/chrome is not a file"), "{err}");
        assert!(!Path::new("never.png").exists());
        return;
    }
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "cli::commands::shot::tests::a_machine_without_the_named_chrome_fails_before_any_browser_starts",
            "--nocapture",
        ])
        .env("SF_SHOT_MISSING_CHROME", "1")
        .env("SF_CHROME", "/no/such/chrome")
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(String::from_utf8_lossy(&child.stdout).contains("1 passed"));
}
