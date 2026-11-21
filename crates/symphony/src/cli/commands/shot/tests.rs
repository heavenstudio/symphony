//! The command line, keys, readiness and report of `symphony shot`, without a browser. The browser
//! cases (a real page, its pictures, its clean failures) are in apps/symphony-web-tests/tests/browser/shot.rs.

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
            clock: None,
            report: false,
            attach: false,
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
    // Attached, each picture goes in the order it was taken, the final one last.
    assert!(!shot.attach);
    assert_eq!(
        shot.pictures(),
        [
            PathBuf::from("open.png"),
            "again.png".into(),
            "closed.png".into()
        ]
    );
}

#[test]
fn attach_is_a_flag_and_a_shot_with_no_step_attaches_its_one_picture() {
    let shot = shot(&["http://h/x", "--attach", "--out", "evidence/a.png"]);
    assert!(shot.attach);
    assert_eq!(shot.pictures(), [PathBuf::from("evidence/a.png")]);
    assert!(
        refusal(&["http://h/x", "--out", "a.png", "--attach=yes"])
            .contains("argument --attach: ignored explicit argument \"yes\""),
        "a flag takes no value"
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
fn a_clock_is_whole_seconds_since_1970_the_last_one_given() {
    let none = shot(&["about:blank", "--out", "x.png"]);
    assert_eq!(none.clock, None);
    let held = shot(&["about:blank", "--out", "x.png", "--clock", "1772443800"]);
    assert_eq!(held.clock, Some(1_772_443_800));
    // The first second and the last a page's Date holds.
    assert_eq!(
        shot(&["about:blank", "--out", "x.png", "--clock", "0"]).clock,
        Some(0)
    );
    let last = shot(&["about:blank", "--out", "x.png", "--clock=8640000000000"]);
    assert_eq!(last.clock, Some(8_640_000_000_000));
    let twice = shot(&[
        "about:blank",
        "--out",
        "x.png",
        "--clock",
        "1",
        "--clock",
        "2",
    ]);
    assert_eq!(twice.clock, Some(2));
}

#[test]
fn the_held_clock_is_one_script_naming_its_moment_in_milliseconds() {
    let script = held_clock(1_772_443_800);
    assert!(script.contains("const at = 1772443800 * 1000;"), "{script}");
    assert!(
        script.starts_with("(() => {") && script.ends_with("})()"),
        "{script}"
    );
    assert!(!script.contains('\n'), "{script}");
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
            &["http://h/", "--out", "a", "--clock", "soon"],
            "argument --clock: invalid SECS \"soon\"",
        ),
        (
            &["http://h/", "--out", "a", "--clock", "1.5"],
            "argument --clock: invalid SECS \"1.5\"",
        ),
        (
            &["http://h/", "--out", "a", "--clock", "+5"],
            "argument --clock: invalid SECS \"+5\"",
        ),
        (
            &["http://h/", "--out", "a", "--clock", "-5"],
            "argument --clock: invalid SECS \"-5\"",
        ),
        (
            &["http://h/", "--out", "a", "--clock", "8640000000001"],
            "argument --clock: invalid SECS \"8640000000001\"",
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
fn the_help_says_what_a_click_accepts_and_what_it_refuses() {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(main(&args(&["-h"]), &mut out, &mut err), 0);
    // The help's lines are wrapped: read its words.
    let words = String::from_utf8(out)
        .unwrap()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for said in [
        // The --click line says what the aim does: the first line box in view that takes the press.
        "click the middle of its first line box in view that takes the press (most elements have \
         one box)",
        "its own label for a control, on top there",
        "the first whose middle is in view and takes the press, else the first in view that no box \
         around it clips there",
        "a link wrapped over two lines is aimed at its first line, and one taller than its \
         scrolling box at a line the box shows",
        // In view is on the screen, a phone's included.
        "In view is in what the screen shows of the page, where the pointer goes (its visual \
         viewport: on a phone, a page wider than the screen or zoomed in shows a part of its \
         layout)",
        // The point pressed is the point tested, or nothing is pressed.
        "Once aimed, the pointer moves to that middle, and the browser's own report of the move, \
         its pointermove or mousemove, says where it landed and on what",
        // A frame's document takes the move: unconfirmed there, and said so.
        "Over a frame (an iframe, frame, embed or object, in a closed shadow root too, which the \
         browser's own hit finds there) its own document takes the move and none reaches the page, \
         so the pointer's place inside the frame is not confirmed: the step goes on as aimed",
        "Where none reaches the page over anything else (the page stopping it), the element found \
         there must be hovered, or where the pointer landed is unknown",
        "Should it land anywhere but where the aim tested, or where be unknown, or, for a click, \
         anything but the element (or its label) be on top there by then (a menu its hover \
         opened; the browser's own hit confirms a label again), the step fails naming where the \
         pointer landed or what the press would reach, and nothing is pressed",
        "the label of the control it is (a checkbox hidden in its label or under the label's styled track)",
        // The point pressed is the one every hit test takes, the browser's own among them.
        "aims at the middle of one of the element's line boxes, on the whole pixel of the page \
         nearest it (where the browser's own hit test looks too)",
        // Every shadow root, open or closed, and what is slotted into one: the browser's own hit
        // confirms what the page cannot see.
        "but on a link, an object or another control inside it (in a component's shadow root \
         too, open or closed, and on what is slotted into one there), as the label's activation \
         clicks its control",
        "The browser's own hit, which sees into closed shadow roots and follows what is slotted \
         into them, confirms a press through a label, so such a control makes it fail as covered",
        "stays covered (by its own ancestor too, painted over it), fails the step (naming what \
         covers it)",
        // A clip named only where the browser or the shape backs it, and no cause made up.
        "stays clipped, the browser finding it nowhere under its middle (naming the box whose \
         overflow or clip cuts it away there, or whose clip-path, an inset, circle, ellipse or \
         polygon on its border box, in pixels, percentages or a calc() of the two, on a box \
         nothing moves but across the page, leaves its middle out), stays hidden (its \
         visibility)",
        "takes no pointer at its middle, its own ancestor on top there (pointer-events: none, its \
         own clip or clip-path leaving its middle out, or a cause it does not name, as an SVG \
         ring's unpainted middle)",
        // A target offered only where a press on it is what the element lets through.
        "so the failure names it. For pointer-events: none, which lets a press through to the \
         ancestor, it adds a selector that finds the ancestor alone to target instead (or says \
         none does), unless the ancestor is the page's body or a press there clicks another \
         control through a label",
    ] {
        assert!(words.contains(said), "{said:?} not in the help: {words}");
    }
    // Nor does it say what the aim no longer does: a custom element taken as the label's unseen,
    // a slot in a closed root unseen, a clip-path claimed only on a box nothing transforms; nor
    // what it never did, a rounded corner or another reference box read.
    for gone in [
        "a wrapped inline's first line)",
        "or on a custom element without an open shadow root",
        "goes unseen",
        "on a box nothing transforms",
        "(in a component's open shadow root too",
        "its corners rounded or not",
        "reference boxes",
    ] {
        assert!(!words.contains(gone), "{gone:?} still in the help: {words}");
    }
}

#[test]
fn the_help_says_what_the_page_waits_for_what_a_dialog_gets_and_what_the_report_names() {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(main(&args(&["--help"]), &mut out, &mut err), 0);
    let words = String::from_utf8(out)
        .unwrap()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for said in [
        // Frames and workers are waited for, but none gone or left, nor a shared worker's loads.
        "its frames' and its workers' among them, those Chrome runs in a process of their own \
         too, but none of a frame since removed, or held by a document its frame has left, nor a \
         document its frame went on from; a shared worker's own loads are not waited for",
        // A dialog is dismissed and named, never waited out.
        "A dialog the page or a frame opens is dismissed as it opens, and the page goes on: an \
         alert closed, a confirm or a prompt answered as by its Cancel, a beforeunload let leave; \
         --report names each",
        // A tab the page opens is let run behind it, and its dialogs are Chrome's to close; a
        // window of its own keeps its dialog, which only the step's --timeout ends.
        "A tab the page opens (a link, a script) runs behind it as Chrome leaves it: the page is \
         brought back in front as soon as Chrome tells of the new one, so Chrome closes that \
         one's dialogs, which --report does not name",
        "A window of its own the page opens (window.open with a size, or popup) stays in front \
         and keeps its dialog: one of the page's own site shares the page's process, which its \
         dialog holds until --timeout fails the step",
        "naming what it waited for, a frame by its address and a worker by its script's",
        // Every frame's console errors are in the report, each naming its frame.
        "the console errors and failed requests seen, a frame's (one Chrome runs in a process of \
         its own too) naming its address, the dialogs dismissed (each one's kind, text and \
         document)",
    ] {
        assert!(words.contains(said), "{said:?} not in the help: {words}");
    }
    // Nor does it still say a frame apart keeps its console errors out.
    assert!(!words.contains("not its console errors"), "{words}");
}

#[test]
fn every_line_of_the_help_fits_a_hundred_columns() {
    // The usage before it is one line, as a synopsis is; the help's own lines are wrapped.
    for line in HELP.lines() {
        assert!(
            line.chars().count() <= 100,
            "{} columns: {line:?}",
            line.chars().count()
        );
    }
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
fn a_page_is_drawn_once_loaded_navigated_with_no_region_loading_or_busy_and_only_streams_open() {
    let drawn = json!({"ready": "complete", "navigation": "ready", "loading": 0, "busy": 0});
    let stream = request("http://h/api/feed", "EventSource");
    let socket = request("ws://h/api/feed", symphony_chrome::WEB_SOCKET);
    let done = Request {
        finished: true,
        ..request("http://h/api/task-board", "Fetch")
    };
    let failed = Request {
        failed: Some("net::ERR_ABORTED".into()),
        canceled: true,
        ..request("http://h/api/old", "Fetch")
    };
    assert!(
        waiting(
            &drawn,
            &[stream.clone(), socket.clone(), done.clone(), failed.clone()]
        )
        .is_empty()
    );
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
        &json!({"ready": "interactive", "navigation": "loading", "loading": 2, "busy": 3}),
        &[vec![stream, socket, done, failed], pending].concat(),
    );
    assert_eq!(
        said,
        [
            "the document to load (it is interactive)",
            "the page's navigation (html[data-navigation=loading])",
            "2 region(s) still loading ([data-ready=loading])",
            "3 region(s) busy on the screen ([aria-busy=true])",
            "5 request(s) in flight: http://h/0, http://h/1, http://h/2 and 2 more",
        ]
    );
    // A page drawn but for a region busy on the screen (a ghost not lifted) waits for it alone.
    assert_eq!(
        waiting(
            &json!({"ready": "complete", "navigation": "ready", "loading": 0, "busy": 1}),
            &[]
        ),
        ["1 region(s) busy on the screen ([aria-busy=true])"]
    );
}

/// `request` made for the frame whose document is at `frame`.
fn framed(url: &str, kind: &str, frame: &str) -> Request {
    Request {
        frame: Some(frame.into()),
        ..request(url, kind)
    }
}

#[test]
fn a_frames_requests_in_flight_are_said_after_the_pages_own_by_the_frame_its_document_as_loading() {
    let loaded = json!({"ready": "complete", "navigation": null, "loading": 0});
    // A frame's finished and given-up requests, its streams too, hold nothing, as the page's own.
    let quiet = [
        Request {
            finished: true,
            ..framed("http://b/mock.html", "Document", "http://b/mock.html")
        },
        Request {
            failed: Some("its frame went away".into()),
            canceled: true,
            ..framed("http://b/gone.png", "Image", "http://b/mock.html")
        },
        framed("http://b/feed", "EventSource", "http://b/mock.html"),
    ];
    assert!(waiting(&loaded, &quiet).is_empty());
    let said = waiting(
        &json!({"ready": "interactive", "navigation": null, "loading": 0}),
        &[
            framed("http://c/hang.html", "Document", "http://c/hang.html"),
            request("http://a/api", "Fetch"),
            framed("http://b/1.png", "Image", "http://b/mock.html"),
            framed("http://b/2.png", "Image", "http://b/mock.html"),
            framed("http://b/3.png", "Image", "http://b/mock.html"),
            framed("http://b/4.png", "Image", "http://b/mock.html"),
            framed("about:blank#x", "Fetch", "about:srcdoc"),
        ],
    );
    assert_eq!(
        said,
        [
            "the document to load (it is interactive)",
            "1 request(s) in flight: http://a/api",
            "the frame http://c/hang.html to load (its document is in flight)",
            "4 request(s) in flight in the frame http://b/mock.html: http://b/1.png, \
             http://b/2.png, http://b/3.png and 1 more",
            "1 request(s) in flight in the frame about:srcdoc: about:blank#x",
        ]
    );
}

/// `request` a worker made, the worker's script at `script`.
fn a_workers(url: &str, kind: &str, script: &str) -> Request {
    Request {
        worker: Some(script.into()),
        ..request(url, kind)
    }
}

#[test]
fn a_workers_requests_in_flight_are_said_last_by_its_script_never_as_a_frames() {
    let loaded = json!({"ready": "complete", "navigation": null, "loading": 0});
    // A worker's finished and given-up requests, and its streams, hold nothing.
    let quiet = [
        Request {
            finished: true,
            ..a_workers("http://a/w.js", "Script", "http://a/w.js")
        },
        Request {
            failed: Some("its worker ended".into()),
            canceled: true,
            ..a_workers("http://a/data.json", "Fetch", "http://a/w.js")
        },
        a_workers("http://a/feed", "EventSource", "http://a/w.js"),
    ];
    assert!(waiting(&loaded, &quiet).is_empty());
    // Its own script still loading, and a worker's fetch, each by its worker, after a frame's and
    // the page's own, in the order each worker's first was seen.
    let said = waiting(
        &loaded,
        &[
            a_workers("http://b/v.js", "Script", "http://b/v.js"),
            a_workers("http://a/1.json", "Fetch", "http://a/w.js"),
            framed("http://b/p.png", "Image", "http://b/mock.html"),
            request("http://a/api", "Fetch"),
            a_workers("http://a/2.json", "Fetch", "http://a/w.js"),
        ],
    );
    assert_eq!(
        said,
        [
            "1 request(s) in flight: http://a/api",
            "1 request(s) in flight in the frame http://b/mock.html: http://b/p.png",
            "1 request(s) in flight in the worker http://b/v.js: http://b/v.js",
            "2 request(s) in flight in the worker http://a/w.js: http://a/1.json, http://a/2.json",
        ]
    );
}

#[test]
fn a_hidden_page_is_not_drawn_until_it_is_back_in_front() {
    let page = |visible: &str| json!({"ready": "complete", "navigation": "ready", "loading": 0, "visible": visible});
    assert_eq!(
        waiting(&page("hidden"), &[]),
        ["the page to be in front (it is hidden)"]
    );
    assert!(waiting(&page("visible"), &[]).is_empty());
}

fn wait(seconds: f64) -> Wait {
    Wait {
        until: Instant::now(),
        timeout: Duration::from_secs_f64(seconds),
    }
}

#[test]
fn a_wait_is_over_at_its_end_and_says_its_length() {
    let ended = wait(2.5);
    assert!(ended.over());
    assert_eq!(ended.said(), "2.5s");
    let running = Wait {
        until: Instant::now() + Duration::from_secs(60),
        ..wait(20.0)
    };
    assert!(!running.over());
    assert_eq!(running.said(), "20s");
}

#[test]
fn a_probe_reads_the_element_where_it_is_after_scrolling_it_or_in_the_next_frame() {
    let css = "summary[data-x=\"a\"]";
    let quoted = r#"document.querySelector("summary[data-x=\"a\"]")"#;
    let now = probe(css, Look::Now);
    assert!(now.starts_with("(() => {"), "{now}");
    assert!(
        now.contains(quoted),
        "the selector goes in as a JSON string: {now}"
    );
    assert!(!now.contains("scrollIntoView") && !now.contains("requestAnimationFrame"));
    let scrolled = probe(css, Look::Scrolled);
    let (scroll, read) = (
        scrolled.find("scrollIntoView").unwrap(),
        scrolled.find("getBoundingClientRect").unwrap(),
    );
    assert!(
        scroll < read,
        "the box is read after the scroll: {scrolled}"
    );
    assert!(!scrolled.contains("requestAnimationFrame"));
    let frame = probe(css, Look::NextFrame);
    assert_eq!(
        frame,
        format!("new Promise(resolve => requestAnimationFrame(() => resolve({now})))")
    );
}

/// Where each of `parts` is in `script`, in their order there, or a panic naming the one missing.
fn positions(script: &str, parts: &[&str]) -> Vec<usize> {
    parts
        .iter()
        .map(|part| {
            script
                .find(part)
                .unwrap_or_else(|| panic!("no {part:?} in {script}"))
        })
        .collect()
}

fn in_order(script: &str, parts: &[&str]) {
    let found = positions(script, parts);
    assert!(
        found.windows(2).all(|pair| pair[0] < pair[1]),
        "{parts:?} out of order at {found:?}: {script}"
    );
}

#[test]
fn a_probe_tests_what_is_on_top_at_the_middle_of_the_box_it_read() {
    for look in [Look::Now, Look::Scrolled, Look::NextFrame] {
        let probe = probe("#a", look);
        // The box is chosen, then its middle reached, then the answer given.
        in_order(
            &probe,
            &[
                "element.getClientRects()",
                "element.getBoundingClientRect();\nconst [x, y] = middle(box);",
                "const p = reach(x, y);",
                "return {x: box.x",
            ],
        );
        // The view is what the screen shows, the visual viewport, by its edges in the page's
        // coordinates; the scroll, the page's own.
        assert!(
            probe.contains(
                "view: [vv.offsetLeft, vv.offsetTop, vv.width, vv.height], scroll: [scrollX, scrollY], hit}"
            ),
            "{probe}"
        );
    }
}

#[test]
fn a_probe_aims_at_the_first_line_box_in_view_that_takes_the_press_then_one_nothing_clips() {
    let probe = probe("#a", Look::Now);
    // Only line boxes with an area count: a wrapped inline's empty fragment at a line's end does not.
    assert!(
        probe.contains("[...element.getClientRects()].filter(r => r.width > 0 && r.height > 0)"),
        "{probe}"
    );
    // In view as the aim reads it: the middle within what the screen shows, the visual viewport,
    // its far edges excluded; without its interface, the window.
    in_order(
        &probe,
        &[
            "const vv = window.visualViewport ?? {offsetLeft: 0, offsetTop: 0, width: innerWidth, height: innerHeight};",
            "const seen = ([x, y]) => x >= vv.offsetLeft && x < vv.offsetLeft + vv.width && y >= vv.offsetTop && y < vv.offsetTop + vv.height;",
            "const viewed = rects.filter(r => seen(middle(r)));",
        ],
    );
    assert!(!probe.contains("innerWidth && "), "{probe}");
    // The first in view that takes the press, else the first no box around it clips there, else
    // the first in view, its first, its whole box.
    assert!(
        probe.contains(
            "const box = viewed.find(r => takes(reach(...middle(r)))) ?? viewed.find(r => !clips.some(c => away(c, ...middle(r)))) ?? viewed[0] ?? rects[0] ?? element.getBoundingClientRect();"
        ),
        "{probe}"
    );
}

#[test]
fn a_probe_reads_what_clips_the_element_through_its_containing_blocks() {
    let probe = probe("#a", Look::Now);
    // Its own clip first, then each box around it in the flat tree, short of the body.
    in_order(
        &probe,
        &[
            "const self = cuts(element, style);",
            "for (let a = flat(element); a && a !== document.body && a !== document.documentElement; a = flat(a))",
        ],
    );
    // An absolute one escapes static boxes that make no containing block, a fixed one every box
    // but those a transform, a filter or containment makes one.
    assert!(
        probe.contains(
            "if ((position === 'fixed' && !confines(t)) || (position === 'absolute' && t.position === 'static' && !confines(t))) continue;"
        ),
        "{probe}"
    );
    // A box clips by its own clip, by an overflow other than visible on an axis or by paint
    // containment, to its padding edge; a display: contents one, an inline one or an SVG element
    // inside an svg makes no box to clip.
    for part in [
        "const cut = cuts(a, t);",
        "const x = paint || t.overflowX !== 'visible', y = paint || t.overflowY !== 'visible';",
        "if ((x || y) && t.display !== 'inline')",
        "[left, top, left + a.clientWidth, top + a.clientHeight]",
        "if (t.display === 'contents' || (a instanceof SVGElement && a.ownerSVGElement)) continue;",
        "const away = (c, x, y) => (c.x && (x < c.edges[0] || x >= c.edges[2])) || (c.y && (y < c.edges[1] || y >= c.edges[3]));",
    ] {
        assert!(probe.contains(part), "no {part:?} in {probe}");
    }
}

#[test]
fn a_probe_reaches_through_shadow_roots_and_takes_the_targets_own_label_as_its_activation_does() {
    let probe = probe("#a", Look::Now);
    // Through every open shadow root to the element really there, then up the flat tree, as the
    // press travels, to the first interactive element, which, when a label, clicks its control: a
    // custom element whose inside the page cannot see is passed, and marked (`blind`), the
    // browser's own hit confirming the press for it.
    in_order(
        &probe,
        &[
            "const inner = deep.shadowRoot.elementFromPoint(x, y);",
            "let first = deep, blind = false;",
            "for (; first && !first.matches(INTERACTIVE); first = flat(first)) blind ||= unseen(first);",
            "via: first && first.localName === 'label' ? first.control : null, blind};",
        ],
    );
    assert!(!probe.contains("&& !unseen(first))"), "{probe}");
    for part in [
        "const up = e => e.parentElement || (e.parentNode instanceof ShadowRoot ? e.parentNode.host : null);",
        // A slotted node's slot comes before its parent, as in the flat tree.
        "const flat = e => e.assignedSlot || up(e);",
        "const unseen = e => e instanceof HTMLElement && e.localName.includes('-') && !e.shadowRoot;",
    ] {
        assert!(probe.contains(part), "no {part:?} in {probe}");
    }
    assert!(
        probe.contains(
            "const takes = p => !!p.at && (element.contains(p.at) || p.via === element);"
        ),
        "{probe}"
    );
    // HTML's interactive content, and an object, go in as a JSON string.
    assert!(
        probe.contains(&format!("const INTERACTIVE = {};", json!(INTERACTIVE))),
        "{probe}"
    );
    for interactive in [
        "a[href]", "button", "input", "label", "select", "textarea", "iframe", "embed", "object",
    ] {
        assert!(
            INTERACTIVE.split(", ").any(|one| one == interactive),
            "{interactive}"
        );
    }
}

#[test]
fn a_probe_says_why_the_element_misses_the_press_the_first_reason_that_holds() {
    let probe = probe("#a", Look::Now);
    in_order(
        &probe,
        &[
            "const hit = !p.at ? null",
            // Taken: inside it, or through its label, for the browser's own hit to confirm.
            ": takes(p) ? element.contains(p.at) || {label: true}",
            ": style.visibility !== 'visible' ? {hidden: style.visibility}",
            ": clipper ? {clipped: named(clipper.by)}",
            // A cover in a shadow root is named in its host's; an ancestor painted over it covers it.
            ": !held || listed ? inside(p.deep)",
            ": shaped && shaped !== element ? {clipped: inside(shaped)}",
            // A label's control named only where the page saw the whole way to it.
            ": {holder: named(p.at), why: shaped || clips.some(c => !c.by && away(c, x, y)) ? 'clip' : style.pointerEvents === 'none' ? 'none' : 'unnamed', target: page ? null : alone(p.at), page, labels: p.via && !p.blind && (alone(p.via) ?? inside(p.via))};",
        ],
    );
    // What is named in a shadow root is named in its outermost host's.
    assert!(
        probe.contains(
            r#"const inside = e => { let host = e; while (host.getRootNode() instanceof ShadowRoot) host = host.getRootNode().host; return host === e ? named(e) : named(e) + ' in ' + named(host) + "'s shadow root"; };"#
        ),
        "{probe}"
    );
    // The browser lists what a press at the point could reach: the element among it, nothing clips
    // it away there.
    assert!(
        probe.contains(
            "const listed = document.elementsFromPoint(x, y).some(e => element.contains(e));"
        ),
        "{probe}"
    );
    // A clip-path is named only where its shape leaves the point out, and never for an element
    // that takes no pointer, which says why already.
    assert!(
        probe.contains(
            "for (let e = element; held && !listed && style.pointerEvents !== 'none' && e && e !== p.deep && !shaped; e = flat(e)) if (excludes(e, x, y)) shaped = e;"
        ),
        "{probe}"
    );
    // The page's body and root are no target.
    assert!(
        probe.contains("const page = p.at === document.body || p.at === document.documentElement;"),
        "{probe}"
    );
    // What is there holds the element when it is the element's ancestor in the flat tree.
    assert!(
        probe.contains("for (let e = element; e && !held; e = flat(e)) held = e === p.deep;"),
        "{probe}"
    );
    // A box around it clips it there, when the browser lists it nowhere there; its own clip makes a
    // holder's reason, not a clipping box.
    assert!(
        probe.contains("const clipper = listed ? null : clips.find(c => c.by && away(c, x, y));"),
        "{probe}"
    );
}

#[test]
fn a_probe_claims_a_clip_path_only_for_a_basic_shape_on_a_box_nothing_around_it_turns() {
    let probe = probe("#a", Look::Now);
    for part in [
        // An HTML element's box, not inline, that neither it nor any box around it up the flat tree
        // moves otherwise than across the page: a transform but a translation in the page's plane
        // (Chrome computes translateZ(0) and translate() to `matrix(1, 0, 0, 1, e, f)`), which the
        // box's place already carries, a translate with a depth, any other individual transform, a
        // motion path or a zoom, so a box mirrored or turned with its ancestor, its size kept, is
        // never tested at the mirrored point.
        "const moved = t => !/^(none|matrix\\(1, 0, 0, 1, [^,]+, [^,]+\\))$/.test(t.transform) || (t.translate || 'none').split(' ').length > 2 || ['scale', 'rotate', 'offsetPath'].some(p => (t[p] || 'none') !== 'none') || (t.zoom || '1') !== '1';",
        "const level = e => { for (let a = e; a; a = flat(a)) if (moved(getComputedStyle(a))) return false; return true; };",
        // Its layout size its drawn size too, and an inset, circle, ellipse or polygon on its
        // border box; anything else is never claimed.
        "const t = e instanceof HTMLElement ? getComputedStyle(e) : null;",
        "t.display !== 'inline' && level(e) ? /^(inset|circle|ellipse|polygon)\\((.*)\\)( border-box)?$/.exec(t.clipPath) : null;",
        "if (!m || Math.abs(b.width - e.offsetWidth) > 1 || Math.abs(b.height - e.offsetHeight) > 1) return false;",
        // Lengths in pixels, percentages, or the calc() of the two Chrome computes every calc() of
        // lengths and percentages to; a min(), a max() or a keyword claims nothing.
        "const size = (v, basis) => { const c = /^calc\\((-?\\d*\\.?\\d+)% ([+-]) (\\d*\\.?\\d+)px\\)$/.exec(v); return c ? parseFloat(c[1]) / 100 * basis + parseFloat(c[2] + c[3]) : /^-?\\d*\\.?\\d+px$/.test(v) ? parseFloat(v) : /^-?\\d*\\.?\\d+%$/.test(v) ? parseFloat(v) / 100 * basis : NaN; };",
        // Arguments split on the spaces outside parentheses, so a calc() is one.
        "const parts = v => v.match(/(?:[^\\s(]+|\\([^()]*\\))+/g) ?? [];",
        "const points = pairs.map(parts).map(",
        "const sides = parts(m[2].split(' round ')[0]);",
        "const words = form ? parts(form) : [];",
        "const [cx, cy] = at ? parts(at).map((v, i) => size(v, i ? h : w)) : [w / 2, h / 2];",
        // An inset's rectangle, its rounded corners left out (they only cut more), empty when its
        // sides cross.
        "return r <= l || b0 <= t0 || px < l || px >= r || py < t0 || py >= b0;",
        // A circle's radius as a percentage of the box's diagonal over the square root of two.
        "return Math.hypot(px - cx, py - cy) > radius(words[0], Math.hypot(w, h) / Math.SQRT2, [cx, w - cx, cy, h - cy]);",
        "((px - cx) / radius(words[0], w, [cx, w - cx])) ** 2 + ((py - cy) / radius(words[1], h, [cy, h - cy])) ** 2 > 1;",
        // A polygon by its fill rule.
        "return rule === 'evenodd' ? crossings % 2 === 0 : winding === 0;",
    ] {
        assert!(probe.contains(part), "no {part:?} in {probe}");
    }
    // Not the box's own transform alone, which a mirrored or turned ancestor slips past, nor any
    // transform but a translation, nor a split that cuts a calc() in three.
    for gone in [
        "t.transform === 'none' && (t.scale",
        "const moved = t => t.transform !== 'none'",
        ".split(' round ')[0].split(' ')",
        "pair.split(' ')",
    ] {
        assert!(!probe.contains(gone), "{gone:?} in {probe}");
    }
}

#[test]
fn a_probe_aims_at_a_whole_pixel_of_the_page_inside_the_box_nearest_its_middle() {
    let probe = probe("#a", Look::Now);
    // On each axis, the whole pixel of the document nearest the middle, while inside the box; else
    // the box's first whole pixel, while inside it; else the nearest, a box narrower than a pixel
    // between two. Back in the viewport's coordinates by the page's scroll.
    for part in [
        "const whole = (from, size, scroll) => { const start = from + scroll, end = start + size, near = Math.round(start + size / 2); return (near >= start && near < end ? near : Math.ceil(start) < end ? Math.ceil(start) : near) - scroll; };",
        "const middle = r => [whole(r.x, r.width, scrollX), whole(r.y, r.height, scrollY)];",
        "point: [x, y], view: ",
    ] {
        assert!(probe.contains(part), "no {part:?} in {probe}");
    }
    assert!(
        !probe.contains("const middle = r => [r.x + r.width / 2, r.y + r.height / 2];"),
        "{probe}"
    );
}

#[test]
fn a_press_through_a_label_is_confirmed_by_the_node_the_browser_finds_there() {
    // From the node itself, inside a closed shadow root too, up the flat tree: the element, or its
    // own label, before anything else interactive, else the node there by its name.
    in_order(
        CONFIRM,
        &[
            "const element = document.querySelector(css);",
            "for (let e = this; e; e = flat(e)) {",
            "if (e === element) return true;",
            "if (e.nodeType === 1 && e.matches(interactive)) return e.localName === 'label' && e.control === element ? true : inside(found);",
            // A node whose parent may hold a closed shadow root the page cannot see, and which no
            // slot the page sees holds: itself, for the browser to name its slot; never twice.
            "const p = e.parentElement;",
            "if (e !== known && !e.assignedSlot && p instanceof HTMLElement && !p.shadowRoot && hosts.test(p.localName)) return e;",
            "return found ? inside(found) : null;",
        ],
    );
    // A text node there, the browser's hit, is named by the element it lies in.
    assert!(
        CONFIRM.contains("const found = hit.nodeType === 1 ? hit : flat(hit);"),
        "{CONFIRM}"
    );
    // The elements a closed shadow root can be attached to: a custom element's name, or one of the
    // built-in elements the DOM lets hold one.
    assert!(
        CONFIRM.contains(
            "const hosts = /^(article|aside|blockquote|body|div|footer|h[1-6]|header|main|nav|p|section|span)$|-/;"
        ),
        "{CONFIRM}"
    );
    // The names and the walk are the probe's own.
    for part in ["const flat = ", "const inside = "] {
        assert!(NAMES.contains(part), "{part}");
        assert!(!CONFIRM.contains(part), "{part}");
        assert!(!AIM.contains(part), "{part}");
        assert!(probe("#a", Look::Now).contains(part), "{part}");
    }
    // Its answer: taken, or covered by what the browser found there.
    assert_eq!(Hit::confirmed(&json!(true)), Hit::Target);
    assert_eq!(
        Hit::confirmed(&json!("button in span#host's shadow root")),
        Hit::Missed(Miss::Covered("button in span#host's shadow root".into()))
    );
    for unsaid in [json!(null), json!(false), json!(3)] {
        assert_eq!(
            Hit::confirmed(&unsaid),
            Hit::Missed(Miss::Covered("what the browser found there".into()))
        );
    }
    // The protocol takes a point by the whole CSS pixel nearest it, in the document's coordinates:
    // the point aimed at, a whole pixel of the document already less what floating point adds,
    // moved by the page's scroll, read in the same frame.
    assert_eq!(
        [
            pixel(12.0),
            pixel(12.499),
            pixel(30.999_999_999),
            pixel(8.0),
            pixel(0.4),
            pixel(-0.4),
            pixel(-0.6)
        ],
        [12, 12, 31, 8, 0, 0, -1]
    );
    let scrolled = Place {
        scroll: (30.0, 1200.0),
        ..place(10.0, 20.0, 100.0, 40.0)
    };
    assert_eq!(scrolled.middle(), (60.0, 40.0));
    assert_eq!(scrolled.in_document(), (90.0, 1240.0));
    assert_eq!(place(10.0, 20.0, 100.0, 40.0).in_document(), (60.0, 40.0));
    // The point pressed, tested and confirmed is the one the probe aimed at, not the box's middle.
    let aimed = Place {
        point: (61.0, 40.0),
        scroll: (0.25, 0.0),
        ..place(10.25, 20.0, 100.0, 40.0)
    };
    assert_eq!(aimed.middle(), (61.0, 40.0));
    assert_eq!(aimed.in_document(), (61.25, 40.0));
    assert_eq!(aimed.on_screen(), (61.0, 40.0));
}

#[test]
fn a_walk_through_a_label_answers_or_asks_the_browser_for_a_nodes_slot() {
    // A node, by its object's id, is one to ask about; any other value is the answer, a string, a
    // boolean, null (a remote null is an object of subtype null) or something not understood.
    assert_eq!(
        Walk::read(
            &json!({"type": "object", "subtype": "node", "className": "HTMLSpanElement", "objectId": "7.1.4"})
        ),
        Walk::Ask(json!("7.1.4"))
    );
    for (result, answer) in [
        (json!({"type": "boolean", "value": true}), json!(true)),
        (
            json!({"type": "string", "value": "button in span#host's shadow root"}),
            json!("button in span#host's shadow root"),
        ),
        (
            json!({"type": "object", "subtype": "null", "value": null}),
            json!(null),
        ),
        // An object that is no node, or a node without its id, answers nothing understood.
        (
            json!({"type": "object", "className": "Object", "objectId": "7.1.5"}),
            json!(null),
        ),
        (json!({"type": "object", "subtype": "node"}), json!(null)),
        (json!({"type": "undefined"}), json!(null)),
        (json!(null), json!(null)),
    ] {
        assert_eq!(Walk::read(&result), Walk::Answer(answer), "{result}");
    }
    // The slot a node is assigned to, in a closed root too, by its backend id; none without one.
    assert_eq!(
        assigned(
            &json!({"node": {"nodeName": "SPAN", "assignedSlot": {"backendNodeId": 41, "nodeType": 1, "nodeName": "SLOT"}}})
        ),
        Some(json!(41))
    );
    for described in [
        json!({"node": {"nodeName": "SPAN"}}),
        json!({"node": {"assignedSlot": null}}),
        json!({"node": {"assignedSlot": {"backendNodeId": "41"}}}),
        json!({"node": {"assignedSlot": {"backendNodeId": -1}}}),
        json!(null),
    ] {
        assert_eq!(assigned(&described), None, "{described}");
    }
    // A remote object by its id, as `DOM.resolveNode` answers it.
    assert_eq!(
        object(&json!({"object": {"type": "object", "subtype": "node", "objectId": "9.2.1"}})),
        json!("9.2.1")
    );
    assert_eq!(object(&json!({})), Value::Null);
}

#[test]
fn what_a_press_reaches_is_read_alike_by_the_probe_and_once_the_pointer_has_moved() {
    let probe = probe("#a", Look::Now);
    let arrived = arrival("#a", (60.0, 2615.5));
    // One reach for both, its names and walks the probe's own.
    for script in [&probe, &arrived] {
        assert!(script.contains(NAMES) && script.contains(REACH), "{script}");
    }
    for part in ["const reach = ", "const takes = ", "const flat = "] {
        assert!(!AIM.contains(part) && !ARRIVED.contains(part), "{part}");
    }
    // A point's hit finds a pair, the move's report another, and either goes through one walk to
    // the control a label clicks.
    in_order(
        REACH,
        &[
            "const reach = (x, y) => {",
            "return reached(at, deep);",
            "const reached = (at, deep) => {",
            "const takes = p => ",
        ],
    );
    // The element and the point go in as JSON, the point in the page's coordinates.
    assert!(
        arrived.starts_with(
            "(async () => { const element = document.querySelector(\"#a\"); const [x, y] = [60.0, 2615.5]; "
        ),
        "{arrived}"
    );
    for (name, value) in [("INTERACTIVE", INTERACTIVE), ("FRAMES", FRAMES)] {
        assert!(
            arrived.contains(&format!("const {name} = {};", json!(value))),
            "{arrived}"
        );
    }
    assert!(arrival("a[href=\"x\"]", (1.0, 2.0)).contains(r#"querySelector("a[href=\"x\"]")"#));
}

#[test]
fn the_pointer_is_found_by_the_moves_own_report_and_set_beside_the_hit_where_the_aim_tested() {
    // Before the move: an earlier watch stopped, then one report kept, the browser's own move (no
    // page script's), its point and its target, the first heard taken and the listening ended;
    // through EventTarget's own methods, on a name the page does not list, and nothing thrown.
    in_order(
        WATCH,
        &[
            "(() => { try { const key = 'symphony-shot-pointer';",
            "try { window[key]?.stop(); } catch {}",
            "const {addEventListener: listen, removeEventListener: unlisten} = EventTarget.prototype;",
            "const kinds = ['pointermove', 'mousemove'];",
            "stop: () => { for (const kind of kinds) unlisten.call(window, kind, take, true); }",
            "const take = event => { try { if (!event.isTrusted) return;",
            "const node = event.composedPath()[0];",
            "watch.report = {x: event.clientX, y: event.clientY, at: node instanceof Element ? node : null};",
            "watch.stop(); } catch {} };",
            "Object.defineProperty(window, key, {value: watch, configurable: true, writable: true});",
            "for (const kind of kinds) listen.call(window, kind, take, true);",
            "return true; } catch { return false; } })()",
        ],
    );
    assert!(!WATCH.contains("enumerable"), "{WATCH}");
    // After it: that report, waited for over two frames at most, the watch then taken off the page
    // and stopped, then the hit at the point aimed at, now; the report's target read from the
    // document's element there, its outermost host.
    in_order(
        ARRIVED,
        &[
            "const key = 'symphony-shot-pointer';",
            "for (let frame = 0; frame < 2 && !window[key]?.report; frame++) await new \
             Promise(resolve => requestAnimationFrame(resolve));",
            "const watch = window[key];",
            "delete window[key];",
            "try { watch?.stop(); } catch {}",
            "const record = watch ? watch.report : null;",
            "while (host.getRootNode() instanceof ShadowRoot) host = host.getRootNode().host;",
            "const hit = reach(x, y);",
        ],
    );
    for part in [
        // Taken, else what is there by its name, else nothing; the element gone takes nothing.
        // Through its label, as the probe says it, for the browser's own hit to confirm.
        "const said = p => !p.at ? null : element && takes(p) ? element.contains(p.at) || {label: true} : inside(p.deep);",
        "return {gone: !element || !element.isConnected,",
        "pointer: record ? {x: record.x, y: record.y, on: record.at ? said(reached(outer(record.at), \
         record.at)) : null} : null,",
        // The hover asked of one element, the one the hit test finds, never searched for, and
        // whether that one is a frame.
        "hit: said(hit), hovered: !!hit.deep && hit.deep.matches(':hover'), framed: !!hit.deep && \
         hit.deep.matches(FRAMES),",
        // The point aimed at in the document's coordinates, for the browser's own hit.
        "document: [x + scrollX, y + scrollY]};",
    ] {
        assert!(ARRIVED.contains(part), "no {part:?} in {ARRIVED}");
    }
    assert!(!ARRIVED.contains("querySelectorAll(':hover')"), "{ARRIVED}");
    assert_eq!(FRAMES, "iframe, frame, embed, object");
}

/// Where the move's report put the pointer, and on what.
fn landed(x: f64, y: f64, on: Reached) -> Option<Pointer> {
    Some(Pointer { at: (x, y), on })
}

/// An arrival with the element still in the page, found there no frame.
fn arrived(pointer: Option<Pointer>, hit: Reached, hovered: bool) -> Arrival {
    Arrival {
        gone: false,
        pointer,
        hit,
        hovered,
        framed: false,
        document: None,
    }
}

#[test]
fn an_arrival_reads_the_moves_report_the_hit_and_its_hover() {
    use Reached::{Label, Nothing, Other, Target};
    assert_eq!(
        Arrival::read(&json!({
            "gone": false,
            "pointer": {"x": 60.25, "y": 2615, "on": true},
            "hit": "div#menu",
            "hovered": true
        })),
        arrived(
            landed(60.25, 2615.0, Target),
            Other("div#menu".into()),
            true
        )
    );
    for (on, read) in [
        (json!("html"), Other("html".into())),
        (json!(null), Nothing),
        // Through its label, for the browser's own hit to confirm; anything else under `label` is
        // not.
        (json!({"label": true}), Label),
        (json!({"label": 1}), Nothing),
    ] {
        assert_eq!(
            Arrival::read(&json!({"pointer": {"x": 1, "y": 2, "on": on}})).pointer,
            landed(1.0, 2.0, read)
        );
    }
    assert_eq!(
        Arrival::read(&json!({"hit": {"label": true}, "hovered": true})).hit,
        Label
    );
    assert_eq!(
        Arrival::read(&json!({"gone": true, "pointer": null, "hit": null, "hovered": false})),
        Arrival {
            gone: true,
            ..arrived(None, Nothing, false)
        }
    );
    assert_eq!(
        Arrival::read(&json!({
            "pointer": null,
            "hit": true,
            "hovered": false,
            "framed": true,
            "document": [60.25, 3015]
        })),
        Arrival {
            framed: true,
            document: Some((60.25, 3015.0)),
            ..arrived(None, Target, false)
        }
    );
    // A document point is two numbers, or none.
    for unsaid in [
        json!([1]),
        json!(["1", 2]),
        json!(null),
        json!({"x": 1, "y": 2}),
    ] {
        assert_eq!(
            Arrival::read(&json!({"document": unsaid})).document,
            None,
            "{unsaid}"
        );
    }
    // Anything short of an answer is nothing: no report, never gone, hovered nor a frame.
    for unsaid in [
        json!(null),
        json!({"gone": "yes", "pointer": {"x": "1", "y": 2}, "hit": 3, "hovered": 1, "framed": "yes"}),
        json!({"pointer": {"x": 1, "on": true}}),
        json!({"pointer": true}),
    ] {
        assert_eq!(
            Arrival::read(&unsaid),
            arrived(None, Nothing, false),
            "{unsaid}"
        );
    }
}

#[test]
fn a_click_or_a_hover_goes_on_only_with_the_pointer_where_the_aim_tested() {
    use Reached::{Nothing, Other, Target};
    let aimed = (1240.796875, 2616.1875);
    // The report at the point aimed at, a pointer event's exactly or a mouse event's whole pixels
    // (a pixel off on either axis at most), the element on top there: either goes on, whatever the
    // report's target (the hit, now, says what a press there reaches).
    for at in [
        aimed,
        (1240.0, 2616.0),
        (1241.796875, 2615.1875),
        (1239.796875, 2617.1875),
    ] {
        for on in [Target, Other("input#email".into())] {
            let there = arrived(landed(at.0, at.1, on), Target, false);
            for clicking in [true, false] {
                assert_eq!(there.refused(aimed, clicking), None, "{there:?}");
            }
        }
    }
    // Any farther, on either axis: the point pressed is not the point tested, whatever the hit test
    // says there, and what the report hit is named.
    for (at, on, said) in [
        (
            (450.0, 2616.0),
            Other("html".into()),
            "(450, 2616) in the page, on html",
        ),
        ((1240.0, 2617.5), Target, "(1240, 2618) in the page, on it"),
        (
            (1242.0, 2616.0),
            Nothing,
            "(1242, 2616) in the page, on no element",
        ),
    ] {
        for hit in [Target, Other("div#veil".into())] {
            let strayed = arrived(landed(at.0, at.1, on.clone()), hit, true);
            assert_eq!(
                strayed.refused(aimed, true),
                Some(format!(
                    ": the pointer landed at {said}, not at its middle (1241, 2616), where the aim \
                     tested; nothing was pressed"
                )),
                "{strayed:?}"
            );
            assert_eq!(
                strayed.refused(aimed, false),
                Some(format!(
                    ": the pointer landed at {said}, not at its middle (1241, 2616), where the aim \
                     tested"
                ))
            );
        }
    }
    // No report: a frame found there took the move in its own document, hovered (sharing the
    // page's process) or not (another site's, or sandboxed): either goes on, unconfirmed. A click
    // still needs the frame to take the press, and a frame on top of it is a cover.
    for hovered in [true, false] {
        let frame = Arrival {
            framed: true,
            ..arrived(None, Target, hovered)
        };
        for clicking in [true, false] {
            assert_eq!(frame.refused(aimed, clicking), None, "{frame:?}");
        }
        let over = Arrival {
            framed: true,
            ..arrived(None, Other("iframe#ad".into()), hovered)
        };
        assert_eq!(over.refused(aimed, false), None, "{over:?}");
        assert!(
            over.refused(aimed, true).is_some_and(|why| why.starts_with(
                ": the press would have landed on iframe#ad, on top at its middle once the pointer \
                 reached it; nothing was pressed"
            )),
            "{over:?}"
        );
    }
    // No report over anything else (a page that stops the move first): the element found there
    // hovered has the pointer over it; not hovered, where it is is unknown.
    for clicking in [true, false] {
        assert_eq!(arrived(None, Target, true).refused(aimed, clicking), None);
    }
    for (hit, found) in [
        (Target, "it, found there, is not hovered"),
        (
            Other("div#pane".into()),
            "div#pane, found there, is not hovered",
        ),
        (Nothing, "the hit test finds no element there"),
    ] {
        let unseen = arrived(None, hit, false);
        let why = format!(
            ": where the pointer landed, moved to its middle (1241, 2616), is unknown: no report \
             of the move reached the page, and {found}"
        );
        assert_eq!(unseen.refused(aimed, false), Some(why.clone()));
        assert_eq!(
            unseen.refused(aimed, true),
            Some(format!("{why}; nothing was pressed"))
        );
    }
    // The pointer where the aim tested, the page put a menu on top there as it came (its hover
    // opened it): a hover has done what it does, a click would press the menu, and the failure says
    // what to do instead.
    for menu in [
        arrived(
            landed(aimed.0, aimed.1, Target),
            Other("div#menu".into()),
            false,
        ),
        arrived(None, Other("div#menu".into()), true),
    ] {
        assert_eq!(menu.refused(aimed, false), None, "{menu:?}");
        assert_eq!(
            menu.refused(aimed, true).as_deref(),
            Some(
                ": the press would have landed on div#menu, on top at its middle once the pointer \
                 reached it; nothing was pressed (for a menu its hover opens, --hover it, then \
                 --click the item meant)"
            ),
            "{menu:?}"
        );
    }
    // A press through its label the browser's own hit has not confirmed: a hover goes on, a click
    // does not, and says so; the label is named as the element itself is, where the pointer is
    // unknown or strayed.
    let unconfirmed = arrived(
        landed(aimed.0, aimed.1, Reached::Label),
        Reached::Label,
        true,
    );
    assert_eq!(unconfirmed.refused(aimed, false), None);
    assert_eq!(
        unconfirmed.refused(aimed, true).as_deref(),
        Some(
            ": the browser's own hit did not confirm the press through its label at its middle \
             once the pointer reached it; nothing was pressed"
        )
    );
    assert!(
        arrived(None, Reached::Label, false)
            .refused(aimed, true)
            .is_some_and(|why| why.contains("and it, found there, is not hovered"))
    );
    assert!(
        arrived(landed(10.0, 10.0, Reached::Label), Target, true)
            .refused(aimed, true)
            .is_some_and(
                |why| why.starts_with(": the pointer landed at (10, 10) in the page, on it,")
            )
    );
    // No element there at all any more: said so, never where the press would land.
    let empty = arrived(landed(aimed.0, aimed.1, Nothing), Nothing, false);
    assert_eq!(empty.refused(aimed, false), None);
    assert_eq!(
        empty.refused(aimed, true).as_deref(),
        Some(
            ": the hit test finds no element at its middle once the pointer reached it; nothing \
             was pressed"
        )
    );
    // The element gone as the pointer came: a hover has done what it does, a click presses
    // nothing; a pointer elsewhere is said first.
    let gone = Arrival {
        gone: true,
        ..arrived(
            landed(aimed.0, aimed.1, Other("main".into())),
            Other("main".into()),
            true,
        )
    };
    assert_eq!(gone.refused(aimed, false), None);
    assert_eq!(
        gone.refused(aimed, true).as_deref(),
        Some(" left the page as the pointer reached it; nothing was pressed")
    );
    let strayed = Arrival {
        pointer: landed(10.0, 10.0, Other("main".into())),
        ..gone
    };
    assert!(
        strayed
            .refused(aimed, true)
            .is_some_and(|why| why.starts_with(": the pointer landed at (10, 10) in the page"))
    );
}

#[test]
fn a_press_through_a_label_on_arrival_is_what_the_browsers_own_hit_confirmed() {
    // Confirmed: the element; covered: what the browser found there; anything else stays
    // unconfirmed, and a click on it fails.
    assert_eq!(Reached::confirmed(Hit::Target), Reached::Target);
    assert_eq!(
        Reached::confirmed(Hit::Missed(Miss::Covered(
            "button#closed-button in x-closed#host's shadow root".into()
        ))),
        Reached::Other("button#closed-button in x-closed#host's shadow root".into())
    );
    for unconfirmed in [
        Hit::Label,
        Hit::Missed(Miss::Nothing),
        Hit::Missed(Miss::Hidden("hidden".into())),
    ] {
        assert_eq!(Reached::confirmed(unconfirmed), Reached::Label);
    }
    // As CONFIRM answers it, through the walk's answer.
    for (answer, reached) in [
        (json!(true), Reached::Target),
        (
            json!("button in span#host's shadow root"),
            Reached::Other("button in span#host's shadow root".into()),
        ),
        (
            json!(null),
            Reached::Other("what the browser found there".into()),
        ),
    ] {
        assert_eq!(
            Reached::confirmed(Hit::confirmed(&answer)),
            reached,
            "{answer}"
        );
    }
    assert_eq!(Reached::Label.said(), "it");
}

#[test]
fn only_an_arrival_nothing_the_page_tells_of_asks_the_browsers_own_hit_for_a_frame() {
    use Reached::{Other, Target};
    // No report, nothing hovered, no frame the page sees: unseen, and so asked.
    assert!(arrived(None, Other("x-closed#host".into()), false).unseen());
    // A report, the element found there hovered, or a frame the page sees: not asked.
    assert!(!arrived(landed(1.0, 2.0, Target), Target, false).unseen());
    assert!(!arrived(None, Target, true).unseen());
    assert!(
        !Arrival {
            framed: true,
            ..arrived(None, Target, false)
        }
        .unseen()
    );
    // The browser's hit finding a frame there, in a closed root the page cannot see into, lets the
    // step go on unconfirmed; finding none, the pointer's place stays unknown.
    let host = arrived(None, Target, false);
    let aimed = (130.0, 55.0);
    assert_eq!(
        Arrival {
            framed: true,
            ..host.clone()
        }
        .refused(aimed, true),
        None
    );
    assert!(
        host.refused(aimed, true)
            .is_some_and(|why| why.starts_with(": where the pointer landed, moved to its middle"))
    );
}

#[test]
fn a_frame_is_known_by_the_local_name_the_protocol_describes() {
    for name in ["iframe", "frame", "embed", "object"] {
        assert!(
            is_frame(&json!({"nodeName": name.to_uppercase(), "localName": name})),
            "{name}"
        );
    }
    for node in [
        json!({"nodeName": "X-CLOSED", "localName": "x-closed"}),
        json!({"nodeName": "BUTTON", "localName": "button"}),
        json!({"nodeName": "IFRAME"}),
        json!({"localName": "iframes"}),
        json!({"localName": "iframe, frame"}),
        json!(null),
    ] {
        assert!(!is_frame(&node), "{node}");
    }
}

#[test]
fn a_point_is_said_in_whole_css_pixels() {
    assert_eq!(point_said((60.25, 2615.75)), "(60, 2616)");
    assert_eq!(point_said((0.0, -3.4)), "(0, -3)");
}

#[test]
fn a_probe_names_an_element_by_a_selector_that_finds_it_alone() {
    let probe = probe("#a", Look::Now);
    // Its name when that finds it alone, else a path of names up from it, each step pinned by
    // :nth-of-type among its kin, until the path finds it alone; none in a shadow root. Its own
    // pin is never offered without its parent's name before it.
    in_order(
        &probe,
        &[
            "if (e.getRootNode() !== document) return null;",
            "if (finds(name(e), e)) return name(e);",
            "css = name(n) + (kin.length > 1 ? ':nth-of-type(' + (kin.indexOf(n) + 1) + ')' : '') + (css && ' > ' + css);",
            "if ((n !== e || kin.length === 1) && finds(css, e)) return css;",
        ],
    );
    assert!(
        probe.contains("return all.length === 1 && all[0] === e;"),
        "{probe}"
    );
}

/// A box at `x`, `y`, `width` by `height` in a 1440x900 viewport, on top at its middle, aimed at
/// there.
fn place(x: f64, y: f64, width: f64, height: f64) -> Place {
    Place {
        x,
        y,
        width,
        height,
        point: (x + width / 2.0, y + height / 2.0),
        view: view(0.0, 0.0, 1440.0, 900.0),
        scroll: (0.0, 0.0),
        hit: Hit::Target,
    }
}

/// A visual viewport at `left`, `top`, `width` by `height` in the page's coordinates.
fn view(left: f64, top: f64, width: f64, height: f64) -> Viewport {
    Viewport {
        left,
        top,
        width,
        height,
    }
}

/// The aim taken at `(x, y)`, on a desktop, where the screen's point is the page's.
fn aimed(x: f64, y: f64) -> Next {
    Next::At(Press {
        screen: (x, y),
        page: (x, y),
    })
}

/// [`place`], missing the press at its middle for `miss`.
fn missing(place: &Place, miss: Miss) -> Place {
    Place {
        hit: Hit::Missed(miss),
        ..place.clone()
    }
}

/// [`place`], with `cover` on top at its middle.
fn covered(place: &Place, cover: &str) -> Place {
    missing(place, Miss::Covered(cover.into()))
}

/// A holder by `name`, missing the press for `why`, found alone by `name` itself.
fn holder(name: &str, why: Why) -> Holder {
    Holder {
        name: name.into(),
        why,
        instead: Instead::Alone(name.into()),
        labels: None,
    }
}

#[test]
fn a_probe_answers_a_place_nothing_yet_or_no_selector() {
    let found = json!({"x": 10.5, "y": 20.0, "width": 100.0, "height": 22.0, "point": [60, 31], "view": [0, 2193.5, 390, 844], "scroll": [0, 640.5], "hit": true});
    // The point it aims at is the one the probe tested, not the box's middle worked out again.
    assert_eq!(
        Place::read("#a", &found),
        Ok(Some(Place {
            point: (60.0, 31.0),
            view: view(0.0, 2193.5, 390.0, 844.0),
            scroll: (0.0, 640.5),
            ..place(10.5, 20.0, 100.0, 22.0)
        }))
    );
    // A view short of its four edges is no place: the old two-number view among them; nor is a
    // point short of its two numbers.
    for (key, short) in [
        ("view", json!([390, 844])),
        ("view", json!([0, 0, 390])),
        ("view", json!(null)),
        ("point", json!([60])),
        ("point", json!(["60", 31])),
        ("point", json!(null)),
    ] {
        let mut read = found.clone();
        read[key] = short;
        assert_eq!(Place::read("#a", &read), Ok(None), "{read}");
    }
    let hit = |said: Value| {
        let mut read = found.clone();
        read["hit"] = said;
        Place::read("#a", &read).unwrap().unwrap().hit
    };
    // Taken through its label, for the browser's own hit to confirm; anything else under `label`
    // is not.
    assert_eq!(hit(json!({"label": true})), Hit::Label);
    assert_eq!(hit(json!({"label": "yes"})), Hit::Missed(Miss::Nothing));
    // Why it misses the press: another element by its name, its visibility, the box that clips it.
    assert_eq!(
        hit(json!("header#top.sticky")),
        Hit::Missed(Miss::Covered("header#top.sticky".into()))
    );
    assert_eq!(
        hit(json!({"hidden": "hidden"})),
        Hit::Missed(Miss::Hidden("hidden".into()))
    );
    assert_eq!(
        hit(json!({"clipped": "div#panel"})),
        Hit::Missed(Miss::Clipped("div#panel".into()))
    );
    // Its own ancestor, by its name, why, a selector that finds it alone and the control a press
    // there clicks through a label, the last two when there are such.
    for (why, read) in [
        ("none", Why::PointerEventsNone),
        ("clip", Why::OwnClip),
        ("unnamed", Why::Unnamed),
        ("anything else", Why::Unnamed),
    ] {
        assert_eq!(
            hit(
                json!({"holder": "nav.tools > button", "why": why, "target": "nav.tools > button:nth-of-type(2)", "page": false, "labels": null})
            ),
            Hit::Missed(Miss::Holder(Holder {
                name: "nav.tools > button".into(),
                why: read,
                instead: Instead::Alone("nav.tools > button:nth-of-type(2)".into()),
                labels: None,
            }))
        );
    }
    assert_eq!(
        hit(
            json!({"holder": "label#l", "why": "none", "target": null, "page": false, "labels": "input#other"})
        ),
        Hit::Missed(Miss::Holder(Holder {
            name: "label#l".into(),
            why: Why::PointerEventsNone,
            instead: Instead::Unfound,
            labels: Some("input#other".into()),
        }))
    );
    // The page's body is no target, whatever selector finds it.
    for target in [json!(null), json!("body")] {
        assert_eq!(
            hit(
                json!({"holder": "html > body", "why": "none", "target": target, "page": true, "labels": null})
            ),
            Hit::Missed(Miss::Holder(Holder {
                name: "html > body".into(),
                why: Why::PointerEventsNone,
                instead: Instead::Page,
                labels: None,
            }))
        );
    }
    // Anything else is nothing at its middle, an ancestor without its name too.
    for nothing in [json!(null), json!(false), json!({"why": "none"}), json!(3)] {
        assert_eq!(hit(nothing), Hit::Missed(Miss::Nothing));
    }
    let mut unsaid = found.clone();
    unsaid.as_object_mut().unwrap().remove("hit");
    assert_eq!(
        Place::read("#a", &unsaid).unwrap().unwrap().hit,
        Hit::Missed(Miss::Nothing)
    );
    assert_eq!(Place::read("#a", &Value::Null), Ok(None));
    // An answer short of a number is no place.
    assert_eq!(
        Place::read("#a", &json!({"x": 1, "y": 2, "width": 3, "height": 4})),
        Ok(None)
    );
    assert_eq!(
        Place::read("a[", &json!({"invalid": "'a[' is not a valid selector."})),
        Err("a[: not a selector: 'a[' is not a valid selector.".to_string())
    );
}

#[test]
fn a_place_is_drawn_with_a_box_and_in_view_by_its_middle() {
    let summary = place(100.0, 449.0, 200.0, 22.0);
    assert!(summary.drawn() && summary.in_view());
    assert_eq!(summary.middle(), (200.0, 460.0));
    assert!(!place(100.0, 449.0, 0.0, 22.0).drawn());
    assert!(!place(100.0, 449.0, 200.0, 0.0).drawn());
    // Half of it out of view is still in reach; its middle past an edge is not.
    assert!(place(-90.0, -10.0, 200.0, 22.0).in_view());
    assert!(!place(-110.0, 449.0, 200.0, 22.0).in_view());
    assert!(!place(100.0, -12.0, 200.0, 22.0).in_view());
    assert!(!place(100.0, 889.0, 200.0, 22.0).in_view(), "middle at 900");
    assert!(
        !place(1340.0, 449.0, 200.0, 22.0).in_view(),
        "middle at 1440"
    );
    assert!(place(100.0, 888.0, 200.0, 22.0).in_view(), "middle at 899");
}

/// [`place`] as a phone shows it: on a screen `view`, the page scrolled by `scroll`.
fn shown(place: Place, view: Viewport, scroll: (f64, f64)) -> Place {
    Place {
        view,
        scroll,
        ..place
    }
}

#[test]
fn a_place_on_a_phone_is_in_view_and_pressed_where_the_screen_shows_it() {
    // The page wider than the phone, at its own scale (initial-scale=1): the layout viewport grows
    // to 1560x3376, a quarter's zoom, and the screen shows 390x844 of it. A button far down,
    // scrolled to the middle of the screen, which moved within the layout viewport, not the page:
    // its middle at 2615 in the page is 422 down the screen. Main pressed it at 2615 down the screen,
    // past the page's edge, and the press landed on the page's root.
    let down = shown(
        place(20.0, 2600.0, 80.0, 30.0),
        view(0.0, 2193.0, 390.0, 844.0),
        (0.0, 0.0),
    );
    assert!(down.in_view());
    assert_eq!(down.middle(), (60.0, 2615.0));
    assert_eq!(down.on_screen(), (60.0, 422.0));
    assert_eq!(
        down.press(),
        Press {
            screen: (60.0, 422.0),
            page: (60.0, 2615.0)
        }
    );
    // The browser's own hit takes the document's point, by the page's scroll alone: the screen's
    // move within the layout viewport is no scroll of the page.
    assert_eq!(down.in_document(), (60.0, 2615.0));
    // Main read the window (the layout viewport) as the view: there the button was in view where
    // the screen does not show it.
    let main = shown(down.clone(), view(0.0, 0.0, 1560.0, 3376.0), (0.0, 0.0));
    assert!(main.in_view());
    assert!(!shown(down.clone(), view(0.0, 0.0, 390.0, 844.0), (0.0, 0.0)).in_view());
    // A button far right: the screen moved right.
    let right = shown(
        place(1200.0, 300.0, 80.0, 30.0),
        view(1045.0, 0.0, 390.0, 844.0),
        (0.0, 0.0),
    );
    assert!(right.in_view());
    assert_eq!(right.on_screen(), (195.0, 315.0));
    // Zoomed in (initial-scale=2): the screen shows 195x422 of the page, from (243, 404). The pointer
    // takes CSS pixels from the screen's corner and Chrome scales them by the zoom itself.
    let zoomed = shown(
        place(300.0, 600.0, 80.0, 30.0),
        view(243.0, 404.0, 195.0, 422.0),
        (0.0, 0.0),
    );
    assert!(zoomed.in_view());
    assert_eq!(zoomed.on_screen(), (97.0, 211.0));
    // Its far edges are out, its near ones in.
    for (x, y, seen) in [
        (243.0, 404.0, true),
        (437.5, 825.5, true),
        (438.0, 615.0, false),
        (340.0, 826.0, false),
        (242.5, 615.0, false),
        (340.0, 403.5, false),
    ] {
        let at = shown(place(x - 1.0, y - 1.0, 2.0, 2.0), zoomed.view, (0.0, 0.0));
        assert_eq!(at.in_view(), seen, "({x}, {y})");
    }
    // A scrolled page moves the document's point, never the screen's.
    let scrolled = shown(zoomed.clone(), zoomed.view, (30.0, 1200.0));
    assert_eq!(scrolled.on_screen(), (97.0, 211.0));
    assert_eq!(scrolled.in_document(), (370.0, 1815.0));
    // No meta viewport, or one without a scale, on a page that wide: zoomed out to a quarter, the
    // screen shows the whole layout viewport, so the screen's point is the page's.
    let out = shown(
        place(300.0, 600.0, 80.0, 30.0),
        view(0.0, 0.0, 1560.0, 3376.0),
        (0.0, 0.0),
    );
    assert_eq!(out.on_screen(), out.middle());
    // A desktop's: the window less its scrollbar, scrolled as a page is.
    let desktop = shown(
        place(20.0, 427.0, 80.0, 30.0),
        view(0.0, 0.0, 1425.0, 885.0),
        (0.0, 2173.0),
    );
    assert_eq!(desktop.on_screen(), (60.0, 442.0));
    assert_eq!(desktop.in_document(), (60.0, 2615.0));
    // Under the scrollbar is out of view.
    assert!(!shown(place(1420.0, 427.0, 20.0, 30.0), desktop.view, (0.0, 0.0)).in_view());
}

#[test]
fn an_aim_on_a_phone_is_taken_at_the_screens_point_and_tested_at_the_pages() {
    let down = shown(
        place(20.0, 2600.0, 80.0, 30.0),
        view(0.0, 2193.0, 390.0, 844.0),
        (0.0, 0.0),
    );
    let mut aim = Aim::default();
    assert_eq!(aim.read(Some(down.clone())), Next::Again);
    assert_eq!(
        aim.read(Some(down.clone())),
        Next::At(Press {
            screen: (60.0, 422.0),
            page: (60.0, 2615.0)
        })
    );
    // The screen moving between two reads, the box held still in the page, is a move.
    let mut aim = Aim::default();
    aim.read(Some(down.clone()));
    let moved = shown(down.clone(), view(0.0, 2000.0, 390.0, 844.0), (0.0, 0.0));
    assert_eq!(aim.read(Some(moved.clone())), Next::Again);
    assert_eq!(aim.latest, Latest::Moving);
    // Out of the screen's view, though in the layout viewport, it is scrolled to again.
    let unseen = shown(down, view(0.0, 0.0, 390.0, 844.0), (0.0, 0.0));
    assert_eq!(aim.read(Some(unseen)), Next::Scroll);
    assert_eq!(aim.latest, Latest::OutOfView);
}

#[test]
fn the_aim_is_taken_once_two_reads_in_a_row_find_the_box_in_one_place() {
    let summary = place(100.0, 449.0, 200.0, 22.0);
    let mut aim = Aim::default();
    assert_eq!(aim.read(Some(summary.clone())), Next::Again);
    assert_eq!(aim.read(Some(summary)), aimed(200.0, 460.0));
}

#[test]
fn a_box_that_moved_is_aimed_at_where_it_then_held() {
    // The summary of the brief: read at 449, then pushed down to 544 by rows mounting above it.
    let (before, after) = (
        place(100.0, 449.0, 200.0, 22.0),
        place(100.0, 544.0, 200.0, 22.0),
    );
    let mut aim = Aim::default();
    assert_eq!(aim.read(Some(before.clone())), Next::Again);
    assert_eq!(aim.read(Some(after.clone())), Next::Again);
    assert_eq!(aim.latest, Latest::Moving);
    assert_eq!(aim.read(Some(after)), aimed(200.0, 555.0));
    // A box that grew or shrank where it stands moved too.
    let mut aim = Aim::default();
    aim.read(Some(before));
    assert_eq!(
        aim.read(Some(place(100.0, 449.0, 200.0, 38.0))),
        Next::Again
    );
}

#[test]
fn a_box_held_still_under_something_else_is_scrolled_again_not_aimed_at() {
    // The two cases of the brief: rows mounting in a scrolling container push the target past the
    // container's edge, where the page below it is on top; or push it up under a sticky header.
    // Either holds still in the viewport, so only the hit test tells it is not the one there.
    let target = place(100.0, 600.0, 80.0, 20.0);
    for cover in ["div#below", "header#header"] {
        let under = covered(&target, cover);
        let mut aim = Aim::default();
        for read in 0..3 {
            assert_eq!(
                aim.read(Some(under.clone())),
                Next::Scroll,
                "{cover} {read}"
            );
            assert_eq!(aim.latest, Latest::Missed(Miss::Covered(cover.into())));
        }
        // Scrolled again, clear of the cover: two reads in a row on top in one place.
        let clear = place(100.0, 440.0, 80.0, 20.0);
        assert_eq!(aim.read(Some(clear.clone())), Next::Again, "{cover}");
        assert_eq!(aim.read(Some(clear)), aimed(140.0, 450.0), "{cover}");
    }
}

#[test]
fn a_covered_read_between_two_agreeing_ones_starts_the_reads_over() {
    let target = place(100.0, 449.0, 200.0, 22.0);
    let mut aim = Aim::default();
    assert_eq!(aim.read(Some(target.clone())), Next::Again);
    // A toast passing over it for one frame: the read before it no longer counts.
    assert_eq!(aim.read(Some(covered(&target, "div.toast"))), Next::Scroll);
    assert_eq!(aim.read(Some(target.clone())), Next::Again);
    assert_eq!(aim.read(Some(target)), aimed(200.0, 460.0));
}

/// [`place`], with its own ancestor `name` on top at its middle, missing the press for `why`.
fn held(place: &Place, name: &str, why: Why) -> Place {
    missing(place, Miss::Holder(holder(name, why)))
}

#[test]
fn a_target_whose_ancestor_takes_the_pointer_at_its_middle_is_scrolled_again_never_aimed_at() {
    // An icon with pointer-events: none in its button, an SVG ring around an empty middle, or a
    // checkbox its own clip hides in its label: held still in view, but a press at its middle would
    // reach what holds it.
    let icon = place(100.0, 449.0, 24.0, 24.0);
    for (name, why) in [
        ("button#go", Why::PointerEventsNone),
        ("svg#rings", Why::Unnamed),
        ("label#toggle", Why::OwnClip),
    ] {
        let mut aim = Aim::default();
        for read in 0..3 {
            assert_eq!(
                aim.read(Some(held(&icon, name, why))),
                Next::Scroll,
                "{name} {read}"
            );
            assert_eq!(aim.latest, Latest::Missed(Miss::Holder(holder(name, why))));
        }
        // Scrolled again, it is on top and aimed at.
        assert_eq!(aim.read(Some(icon.clone())), Next::Again, "{name}");
        assert_eq!(aim.read(Some(icon.clone())), aimed(112.0, 461.0));
    }
    // Between two agreeing reads, an ancestor's read starts them over, as a cover's does.
    let mut aim = Aim::default();
    assert_eq!(aim.read(Some(icon.clone())), Next::Again);
    assert_eq!(
        aim.read(Some(held(&icon, "button#go", Why::PointerEventsNone))),
        Next::Scroll
    );
    assert_eq!(aim.read(Some(icon.clone())), Next::Again);
    // And out of view wins over it.
    let below = held(
        &place(100.0, 1200.0, 24.0, 24.0),
        "button#go",
        Why::PointerEventsNone,
    );
    assert_eq!(aim.read(Some(below)), Next::Scroll);
    assert_eq!(aim.latest, Latest::OutOfView);
}

#[test]
fn a_target_a_box_around_it_clips_away_is_scrolled_again_and_one_its_visibility_hides_is_not() {
    let target = place(100.0, 449.0, 80.0, 20.0);
    // Clipped out of its scrolling box's view: a scroll of the box may bring it back.
    let clipped = missing(&target, Miss::Clipped("div#list".into()));
    let mut aim = Aim::default();
    for read in 0..3 {
        assert_eq!(aim.read(Some(clipped.clone())), Next::Scroll, "{read}");
        assert_eq!(aim.latest, Latest::Missed(Miss::Clipped("div#list".into())));
    }
    assert_eq!(aim.read(Some(target.clone())), Next::Again);
    assert_eq!(aim.read(Some(target.clone())), aimed(140.0, 459.0));
    // Hidden by its visibility: no scroll shows it, so it is read again until the page shows it.
    let hidden = missing(&target, Miss::Hidden("hidden".into()));
    let mut aim = Aim::default();
    assert_eq!(aim.read(Some(target.clone())), Next::Again);
    for read in 0..3 {
        assert_eq!(aim.read(Some(hidden.clone())), Next::Again, "{read}");
        assert_eq!(aim.latest, Latest::Missed(Miss::Hidden("hidden".into())));
    }
    // The read before it does not count: two shown reads in a row are.
    assert_eq!(aim.read(Some(target.clone())), Next::Again);
    assert_eq!(aim.read(Some(target)), aimed(140.0, 459.0));
}

#[test]
fn a_middle_on_nothing_in_the_page_is_scrolled_again_and_out_of_view_wins_over_the_hit() {
    let target = place(1420.0, 449.0, 30.0, 22.0);
    let mut aim = Aim::default();
    // Its middle in the viewport but under the scrollbar: no element there.
    let missed = missing(&target, Miss::Nothing);
    assert_eq!(aim.read(Some(missed)), Next::Scroll);
    assert_eq!(aim.latest, Latest::Missed(Miss::Nothing));
    // Out of view, nothing is at its middle either: it is out of view that the failure says.
    let below = missing(&place(100.0, 1200.0, 200.0, 22.0), Miss::Nothing);
    assert_eq!(aim.read(Some(below)), Next::Scroll);
    assert_eq!(aim.latest, Latest::OutOfView);
    // Nor does a box with none look for what covers it.
    let boxless = covered(&place(100.0, 449.0, 0.0, 0.0), "main");
    assert_eq!(aim.read(Some(boxless)), Next::Again);
    assert_eq!(aim.latest, Latest::Boxless);
}

#[test]
fn a_box_that_moves_on_every_read_is_never_aimed_at() {
    let mut aim = Aim::default();
    for frame in 0..100 {
        let next = aim.read(Some(place(100.0, 300.0 + f64::from(frame), 200.0, 40.0)));
        assert_eq!(next, Next::Again, "{frame}");
    }
    assert_eq!(aim.latest, Latest::Moving);
}

#[test]
fn a_box_gone_without_one_or_out_of_view_starts_the_reads_over() {
    let summary = place(100.0, 449.0, 200.0, 22.0);
    for (between, next, latest) in [
        (None, Next::Again, Latest::Gone),
        (
            Some(place(100.0, 449.0, 0.0, 0.0)),
            Next::Again,
            Latest::Boxless,
        ),
        (
            Some(place(100.0, 1200.0, 200.0, 22.0)),
            Next::Scroll,
            Latest::OutOfView,
        ),
    ] {
        let mut aim = Aim::default();
        assert_eq!(aim.read(Some(summary.clone())), Next::Again);
        assert_eq!(aim.read(between.clone()), next, "{between:?}");
        assert_eq!(aim.latest, latest);
        // The read before it is not one the box held still since.
        assert_eq!(aim.read(Some(summary.clone())), Next::Again, "{between:?}");
        assert_eq!(aim.read(Some(summary.clone())), aimed(200.0, 460.0));
    }
}

#[test]
fn an_aim_not_taken_says_what_its_latest_read_found() {
    let mut aim = Aim::default();
    assert_eq!(
        aim.late("#x", wait(5.0)),
        "#x was never read in an animation frame within 5s"
    );
    for (latest, said) in [
        (Latest::Gone, "#x left the page within 5s"),
        (
            Latest::Boxless,
            "#x is in the page but has no box within 5s",
        ),
        (
            Latest::OutOfView,
            "#x stayed out of view: its middle was outside the viewport after every scroll within 5s",
        ),
        (
            Latest::Missed(Miss::Covered("main > div".into())),
            "#x stayed covered: main > div was on top at its middle after every scroll within 5s",
        ),
        // A real cause, never "takes no pointer", and no hint to target anything else.
        (
            Latest::Missed(Miss::Hidden("hidden".into())),
            "#x stayed hidden within 5s: its visibility is hidden, so no press reaches it",
        ),
        (
            Latest::Missed(Miss::Clipped("div#panel".into())),
            "#x stayed clipped: div#panel clips it away at its middle after every scroll within 5s",
        ),
        // Never "covered" for what holds it: why it takes no pointer, and what to target instead.
        (
            Latest::Missed(Miss::Holder(holder(
                "button#go.icon",
                Why::PointerEventsNone,
            ))),
            "#x takes no pointer at its middle (pointer-events: none): button#go.icon, which holds \
             it, was on top there after every scroll within 5s; target button#go.icon instead",
        ),
        // No cause named: what holds it is named, but a press on it does nothing for the target, so
        // it is not offered, whether a selector finds it alone, none does or it is the page.
        (
            Latest::Missed(Miss::Holder(Holder {
                instead: Instead::Alone("section > svg:nth-of-type(2)".into()),
                ..holder("section > svg", Why::Unnamed)
            })),
            "#x takes no pointer at its middle: section > svg, which holds it, was on top there \
             after every scroll within 5s",
        ),
        (
            Latest::Missed(Miss::Holder(Holder {
                instead: Instead::Unfound,
                ..holder("x-card > div", Why::Unnamed)
            })),
            "#x takes no pointer at its middle: x-card > div, which holds it, was on top there \
             after every scroll within 5s",
        ),
        (
            Latest::Missed(Miss::Holder(Holder {
                instead: Instead::Page,
                ..holder("html > body", Why::Unnamed)
            })),
            "#x takes no pointer at its middle: html > body, which holds it, was on top there \
             after every scroll within 5s",
        ),
        // No selector finds it alone: said plainly.
        (
            Latest::Missed(Miss::Holder(Holder {
                instead: Instead::Unfound,
                ..holder("x-card > div", Why::PointerEventsNone)
            })),
            "#x takes no pointer at its middle (pointer-events: none): x-card > div, which holds \
             it, was on top there after every scroll within 5s; no selector finds x-card > div \
             alone, to target instead",
        ),
        // The page's body holds it: no target, said as such.
        (
            Latest::Missed(Miss::Holder(Holder {
                instead: Instead::Page,
                ..holder("html > body", Why::PointerEventsNone)
            })),
            "#x takes no pointer at its middle (pointer-events: none): html > body, which holds \
             it, was on top there after every scroll within 5s; html > body is the page itself, \
             nothing to target instead",
        ),
        // A press on a label for another control clicks that control: never offered instead, the
        // control named in its component's shadow root when it lies in one.
        (
            Latest::Missed(Miss::Holder(Holder {
                labels: Some("input#other".into()),
                ..holder("label#l", Why::PointerEventsNone)
            })),
            "#x takes no pointer at its middle (pointer-events: none): label#l, which holds it, \
             was on top there after every scroll within 5s; a press there clicks input#other (by \
             its label), not it",
        ),
        (
            Latest::Missed(Miss::Holder(Holder {
                labels: Some("input#check in x-check#host's shadow root".into()),
                instead: Instead::Unfound,
                ..holder("x-check#host", Why::PointerEventsNone)
            })),
            "#x takes no pointer at its middle (pointer-events: none): x-check#host, which holds \
             it, was on top there after every scroll within 5s; a press there clicks input#check \
             in x-check#host's shadow root (by its label), not it",
        ),
        // Its own clip hides it, the page's body holding it: nothing offered either.
        (
            Latest::Missed(Miss::Holder(Holder {
                instead: Instead::Page,
                ..holder("html > body", Why::OwnClip)
            })),
            "#x stayed clipped by its own clip: html > body, which holds it, was on top at its \
             middle after every scroll within 5s",
        ),
        // Its own clip hides it: what holds it is no substitute, whether a selector finds it or not.
        (
            Latest::Missed(Miss::Holder(holder("label#toggle", Why::OwnClip))),
            "#x stayed clipped by its own clip: label#toggle, which holds it, was on top at its \
             middle after every scroll within 5s",
        ),
        (
            Latest::Missed(Miss::Holder(Holder {
                labels: Some("input#first".into()),
                ..holder("label#toggle", Why::OwnClip)
            })),
            "#x stayed clipped by its own clip: label#toggle, which holds it, was on top at its \
             middle after every scroll within 5s; a press there clicks input#first (by its label), \
             not it",
        ),
        (
            Latest::Missed(Miss::Nothing),
            "#x stayed out of reach: nothing in the page was at its middle after every scroll \
             within 5s",
        ),
        (
            Latest::Moving,
            "#x kept moving: no two reads in a row, each in an animation frame, found its box in \
             one place within 5s",
        ),
    ] {
        aim.latest = latest;
        assert_eq!(aim.late("#x", wait(5.0)), said);
    }
}

#[test]
fn a_page_that_did_not_load_is_named_once_with_why_or_its_wait() {
    let url = "http://127.0.0.1:9/tasks";
    assert_eq!(
        not_loaded(
            url,
            wait(5.0),
            false,
            &format!("{url}: net::ERR_CONNECTION_REFUSED")
        ),
        "http://127.0.0.1:9/tasks did not load: net::ERR_CONNECTION_REFUSED"
    );
    // A reason that does not start with the URL is kept whole.
    assert_eq!(
        not_loaded(url, wait(5.0), false, "Page.navigate: the target closed"),
        "http://127.0.0.1:9/tasks did not load: Page.navigate: the target closed"
    );
    // Past the wait, the driver's own words (which call ran out) give way to the wait's length.
    assert_eq!(
        not_loaded(
            url,
            wait(5.0),
            true,
            "Page.navigate: no reply before the deadline"
        ),
        "http://127.0.0.1:9/tasks did not load within 5s"
    );
}

#[test]
fn a_page_not_drawn_says_what_it_still_waited_for_and_what_failed_meanwhile() {
    let still = [
        "the page's navigation (html[data-navigation=loading])".to_string(),
        "1 request(s) in flight: http://h/api/board".to_string(),
    ];
    assert_eq!(
        not_drawn("--key Escape", wait(10.0), &still, &[]),
        "--key Escape: the page was not drawn within 10s: still waiting for the page's navigation \
         (html[data-navigation=loading]); 1 request(s) in flight: http://h/api/board"
    );
    let failed = [
        json!({"url": "http://h/api/tasks", "status": 503}),
        json!({"url": "http://h/x.js", "error": "net::ERR_FAILED"}),
    ];
    assert_eq!(
        not_drawn(
            "http://h/tasks",
            wait(60.0),
            &["the page to paint: its fonts, then two frames".to_string()],
            &failed
        ),
        "http://h/tasks: the page was not drawn within 60s: still waiting for the page to paint: \
         its fonts, then two frames; meanwhile these failed: \
         {\"url\":\"http://h/api/tasks\",\"status\":503}, \
         {\"url\":\"http://h/x.js\",\"error\":\"net::ERR_FAILED\"}"
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
    // A WebSocket made or closed is, its frames not.
    let (made, frame, closed) = (
        event("Network.webSocketCreated"),
        event("Network.webSocketFrameReceived"),
        event("Network.webSocketClosed"),
    );
    assert_eq!(
        quiet_since([(at(50), &made), (at(80), &frame)].into_iter(), at(40)),
        at(50)
    );
    assert_eq!(
        quiet_since([(at(50), &made), (at(70), &closed)].into_iter(), at(40)),
        at(70)
    );
}

#[test]
fn the_report_keeps_errors_failures_dialogs_and_overflow_and_leaves_out_the_rest() {
    let event = |method: &str, params: Value| json!({"method": method, "params": params});
    let events = [
        event(
            "Runtime.consoleAPICalled",
            json!({"type": "error", "args": [{"type": "string", "value": "boom"}, {"type": "number", "value": 3}, {"type": "object", "description": "Error: deep"}]}),
        ),
        // A dialog, by its kind, its text and its document, whatever else Chrome says of it.
        event(
            "Page.javascriptDialogOpening",
            json!({"type": "confirm", "message": "Sure?", "url": "http://h/", "hasBrowserHandler": false, "defaultPrompt": ""}),
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
            "dialogs": [{"type": "confirm", "message": "Sure?", "url": "http://h/"}],
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
    assert_eq!(fits["dialogs"], json!([]));
}

#[test]
fn the_report_names_the_frame_each_error_came_from_but_the_pages_own() {
    let on = |session: &str, method: &str, params: Value| json!({"sessionId": session, "method": method, "params": params});
    let navigated = |session: &str, frame: &str, parent: Option<&str>, url: &str| {
        let mut told = json!({"id": frame, "url": url});
        if let Some(parent) = parent {
            told["parentId"] = json!(parent);
        }
        on(session, "Page.frameNavigated", json!({"frame": told}))
    };
    let context = |session: &str, id: i64, frame: &str| {
        on(
            session,
            "Runtime.executionContextCreated",
            json!({"context": {"id": id, "auxData": {"frameId": frame, "isDefault": true}}}),
        )
    };
    let console = |session: &str, context: i64, text: &str| {
        on(
            session,
            "Runtime.consoleAPICalled",
            json!({"type": "error", "executionContextId": context, "args": [{"type": "string", "value": text}]}),
        )
    };
    let logged = |session: &str, request: Option<&str>, text: &str| {
        let mut entry = json!({"level": "error", "source": "network", "text": text});
        if let Some(request) = request {
            entry["networkRequestId"] = json!(request);
        }
        on(session, "Log.entryAdded", json!({"entry": entry}))
    };
    let events = [
        // The page, a frame in its process (`near`), and a frame in a process of its own (`far`,
        // followed on a session of its own, its contexts numbered apart from the page's).
        navigated("page", "top", None, "http://a/"),
        context("page", 1, "top"),
        navigated("page", "near", Some("top"), "http://a/near.html"),
        context("page", 2, "near"),
        on(
            "page",
            "Target.attachedToTarget",
            json!({"sessionId": "far", "targetInfo": {"targetId": "far", "type": "iframe", "url": ""}}),
        ),
        navigated("far", "far", Some("top"), "http://b/far.html"),
        context("far", 1, "far"),
        on(
            "far",
            "Network.requestWillBeSent",
            json!({"requestId": "picture", "frameId": "far", "request": {"url": "http://b/p.png"}}),
        ),
        on(
            "page",
            "Network.requestWillBeSent",
            json!({"requestId": "icon", "frameId": "top", "request": {"url": "http://a/icon"}}),
        ),
        console("page", 1, "the page's"),
        console("page", 2, "near's"),
        console("far", 1, "far's"),
        on(
            "far",
            "Runtime.exceptionThrown",
            json!({"exceptionDetails": {"text": "Uncaught", "executionContextId": 1, "exception": {"description": "Error: far"}}}),
        ),
        // By the request it names, else by the frame's session it came on.
        logged("far", Some("picture"), "its picture"),
        logged("page", Some("icon"), "the page's icon"),
        logged("far", None, "far's violation"),
        // A context not told of: by its session, a followed frame's, else none named.
        console("far", 9, "far's unknown context"),
        console("page", 7, "the page's unknown context"),
        // A frame named by the document it was in when the error came.
        navigated("page", "near", Some("top"), "http://a/near-2.html"),
        context("page", 3, "near"),
        console("page", 3, "near's next"),
    ];
    let said = report(&events, &[], &json!({}));
    let source = |source: &str, text: &str, frame: Option<&str>| {
        let mut said = json!({"source": source, "text": text});
        if let Some(frame) = frame {
            said["frame"] = json!(frame);
        }
        said
    };
    let far = Some("http://b/far.html");
    assert_eq!(
        said["console_errors"],
        json!([
            source("console", "the page's", None),
            source("console", "near's", Some("http://a/near.html")),
            source("console", "far's", far),
            source("exception", "Error: far", far),
            source("network", "its picture", far),
            source("network", "the page's icon", None),
            source("network", "far's violation", far),
            source("console", "far's unknown context", far),
            source("console", "the page's unknown context", None),
            source("console", "near's next", Some("http://a/near-2.html")),
        ])
    );
}

#[test]
fn a_machine_without_the_named_chrome_fails_before_any_browser_starts() {
    // SYMPHONY_CHROME is read by this process alone; the variable is set only in a child.
    if std::env::var_os("SYMPHONY_SHOT_MISSING_CHROME").is_some() {
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
        .env("SYMPHONY_SHOT_MISSING_CHROME", "1")
        .env(symphony_env::CHROME.name, "/no/such/chrome")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&child.stderr);
    assert!(
        child.status.success(),
        "{}{stderr}",
        String::from_utf8_lossy(&child.stdout)
    );
    assert!(String::from_utf8_lossy(&child.stdout).contains("1 passed"));
}
